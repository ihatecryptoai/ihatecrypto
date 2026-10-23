import EVMChain
import Foundation
import IhatecryptoCore
import LLMCore
import RelayBridge
import WalletCore

/// What the network's explorer says about a contract the chat wants to call: a contract at all, its source (and a
/// proxy's implementation) published and verified, its name and its age. Blockscout's; none for a network without one.
public struct ContractTrust: Sendable, Equatable {
    public var isContract = false
    public var verified = false
    public var name: String?
    /// The code behind a proxy, by name (Blockscout names only a verified one).
    public var implementation: String?
    public var days: Int?
    /// No explorer for this network, or it didn't answer: nothing is known.
    public var unknown = false

    static let browser = Holdings.browser

    public static func check(_ address: String, on network: String) async -> ContractTrust {
        guard let host = Holdings.blockscout[network],
              let data = try? await HTTP.get("https://\(host)/api/v2/addresses/\(address)", headers: browser, timeout: 15),
              let a = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return ContractTrust(unknown: true) }
        var t = ContractTrust()
        t.isContract = a["is_contract"] as? Bool ?? false
        t.verified = a["is_verified"] as? Bool ?? false
        t.name = a["name"] as? String
        if let impl = (a["implementations"] as? [[String: Any]])?.first {
            t.implementation = impl["name"] as? String
            if t.implementation == nil { t.verified = false }
        }
        if let tx = a["creation_transaction_hash"] as? String,
           let d = try? await HTTP.get("https://\(host)/api/v2/transactions/\(tx)", headers: browser, timeout: 15),
           let stamp = ((try? JSONSerialization.jsonObject(with: d)) as? [String: Any])?["timestamp"] as? String {
            let f = ISO8601DateFormatter()
            f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
            if let date = f.date(from: stamp) ?? ISO8601DateFormatter().date(from: stamp) {
                t.days = Int(Date().timeIntervalSince(date) / 86_400)
            }
        }
        return t
    }

    /// «verified · FluidVault (proxy → FluidVaultT1) · 412 days»
    public var line: String {
        if unknown { return "not checked: no explorer answered for this network" }
        if !isContract { return "not a contract" }
        return (verified ? "verified source" : "unverified source") + (name.map { " · \($0)" } ?? "")
            + (implementation.map { " (proxy → \($0))" } ?? "") + (days.map { " · \($0) days old" } ?? "")
    }
}

extension Executor {
    /// A contract call the chat wrote and the user confirmed: its exact approve first when it has one (with the call as
    /// one transaction where batches are on), then the call. The contract is passed as a recipient when the approve lets
    /// it spend or coins go with the call: the signer asks for it in person, address whole, the first time.
    public func contractCall(_ net: EVMNetwork, to: String, data: Data, value: UInt64, approve: (token: String, units: Decimal)?,
                             approveNote: String, note: String) async throws -> String {
        let hash: String
        if let a = approve {
            hash = try await approveAndSend(net.rpc, token: a.token, spender: to, units: a.units, approveNote: approveNote,
                                            to: to, data: data, note: note, recipients: [to])
        } else {
            hash = try await sendEVM(net.rpc, to: to, data: data, note: note, recipients: value > 0 ? [to] : [], value: value)
        }
        return "\(note): done. \(net.explorer)/tx/\(hash)"
    }
}

/// What else is known of a contract outside ihatecrypto's own list: its TVL priced by DefiLlama (its supply × the price
/// DefiLlama gives its token — a vault's shares, stETH), whether a scam list names it, and whether this wallet ever sent
/// it a transaction.
public struct ContractReputation: Sendable, Equatable {
    public var tvlUSD: Decimal?
    /// Why it's a scam, when a list says so.
    public var scam: String?
    public var firstTime = true
    /// The scam list couldn't be read: said on the card, the other checks still hold.
    public var unchecked = false

    /// Below this a protocol isn't one DefiLlama counts as having money in it.
    public init() {}

    public static let minTVL: Decimal = 1_000_000
    public static let minDays = 30

    public static func check(_ contract: String, on net: EVMNetwork, me: String) async -> ContractReputation {
        var r = ContractReputation()
        async let listed = ScamList.shared.names(contract)
        async let price = prices([contract], on: net)[contract.lowercased()]
        async let supply = try? await net.rpc.read(contract, Data([0x18, 0x16, 0x0d, 0xdd]))   // totalSupply()
        async let decimals = try? await net.rpc.read(contract, Data([0x31, 0x3c, 0xe5, 0x67]))   // decimals()
        async let token = blockscout(net, "tokens/\(contract)")
        async let sent = blockscout(net, "addresses/\(me)/transactions?filter=from")
        if let p = await price, let s = (await supply).flatMap({ ABI.decimal(in: $0, word: 0) }),
           let d = (await decimals).flatMap({ ABI.uint(in: $0, word: 0) }), d <= 36 {
            r.tvlUSD = s / pow(10, Int(d)) * p
        }
        switch await listed {
        case .some(let why): r.scam = why
        case .none: r.unchecked = await ScamList.shared.unavailable
        }
        if r.scam == nil, ((await token)?["reputation"] as? String)?.lowercased() == "scam" { r.scam = "Blockscout marks it as a scam" }
        let to = ((await sent)?["items"] as? [[String: Any]] ?? []).compactMap { ($0["to"] as? [String: Any])?["hash"] as? String }
        r.firstTime = !to.contains { $0.lowercased() == contract.lowercased() }
        return r
    }

    /// Dollar prices DefiLlama gives these contracts on the network (lowercased keys); missing when it has none.
    public static func prices(_ tokens: [String], on net: EVMNetwork) async -> [String: Decimal] {
        await Holdings.llamaPrices(net.key, tokens).mapValues(\.price)
    }

    /// The network's own coin in dollars (Relay's price); 0 when it doesn't answer.
    public static func coinUSD(_ net: EVMNetwork) async -> Decimal {
        ((try? await Relay.nativePrice(chainId: net.rpc.chainId)) ?? nil) ?? 0
    }

    static func blockscout(_ net: EVMNetwork, _ path: String) async -> [String: Any]? {
        guard let host = Holdings.blockscout[net.key], let d = try? await HTTP.get("https://\(host)/api/v2/\(path)", headers: ContractTrust.browser, timeout: 15) else { return nil }
        return (try? JSONSerialization.jsonObject(with: d)) as? [String: Any]
    }
}

/// ScamSniffer's open list of drainer and scam addresses (github.com/scamsniffer/scam-database), read once every six hours.
public actor ScamList {
    public static let shared = ScamList()
    static let url = "https://raw.githubusercontent.com/scamsniffer/scam-database/main/blacklist/address.json"
    private var addresses: Set<String> = []
    private var readAt: Date?
    public private(set) var unavailable = false

    /// Why the list names it, or nil.
    public func names(_ address: String) async -> String? {
        if readAt.map({ Date().timeIntervalSince($0) > 6 * 3600 }) ?? true {
            if let d = try? await HTTP.get(Self.url, timeout: 15), let list = (try? JSONSerialization.jsonObject(with: d)) as? [String] {
                addresses = Set(list.map { $0.lowercased() })
                readAt = Date()
                unavailable = false
            } else {
                unavailable = addresses.isEmpty
            }
        }
        return addresses.contains(address.lowercased()) ? "ScamSniffer's scam list names it" : nil
    }
}

/// The last checks of a contract call: the rights it hands out, the money it puts at risk, and whether what's about to be
/// signed still does what the card showed.
public enum ContractCallGuard {
    /// Settings › Wallet: the most a call to a protocol outside ihatecrypto's own list may put at risk, in dollars.
    public static let capKey = "contractCallCapUSD"
    public static var capUSD: Decimal { Decimal(UserDefaults.standard.object(forKey: capKey) as? Double ?? 50) }

    /// Why a right this call hands out is refused: to anyone but the called contract, unlimited, over all NFTs, an
    /// operator's or an authorization, credit, ownership. Only a limited approve to the contract itself passes.
    public static func grantProblem(_ grants: [EVMSimulation.Grant], contract: String) -> String? {
        for g in grants {
            if g.to.lowercased() != contract.lowercased() {
                return "it would give \(g.to) a right over your \(g.kind == "approve" ? "tokens" : "assets") (\(g.kind) at \(g.asset))"
            }
            if g.kind != "approve" || g.unlimited {
                return "it would give the contract \(g.unlimited && g.kind == "approve" ? "unlimited spending" : "\(g.kind) rights") over your assets"
            }
        }
        return nil
    }

    /// What the re-simulation changed against the card: nil when every balance moves within 1% of what it showed and
    /// the rights are the same.
    public static func drift(card: EVMSimulation, now: EVMSimulation) -> String? {
        if let revert = now.revert { return "it would now fail — \(revert)" }
        for asset in Set(card.changes.keys).union(now.changes.keys) {
            let a = card.changes[asset] ?? 0, b = now.changes[asset] ?? 0
            if a == 0 || b == 0 ? a != b : abs(b - a) > abs(a) / 100 { return "a balance would change differently than the card showed (\(asset))" }
        }
        return now.grants == card.grants ? nil : "it would now hand out other rights than the card showed"
    }
}

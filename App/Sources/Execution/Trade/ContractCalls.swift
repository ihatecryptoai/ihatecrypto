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
    /// one transaction where batches are on and no coins go with it), then the call with its coins. The contract is passed
    /// as a recipient when the approve lets it spend or coins go with the call: the signer asks for it in person, address
    /// whole, the first time.
    public func contractCall(_ net: EVMNetwork, to: String, data: Data, value: UInt64, approve: (token: String, units: Decimal)?,
                             approveNote: String, note: String) async throws -> String {
        let hash: String
        if let a = approve {
            hash = try await approveAndSend(net.rpc, token: a.token, spender: to, units: a.units, approveNote: approveNote,
                                            to: to, data: data, note: note, recipients: [to], value: value)
        } else {
            hash = try await sendEVM(net.rpc, to: to, data: data, note: note, recipients: value > 0 ? [to] : [], value: value)
        }
        return "\(note): done. \(net.explorer)/tx/\(hash)"
    }
}

/// What else is known of a contract outside ihatecrypto's own list: its TVL priced by DefiLlama (its supply × the price
/// DefiLlama gives its token — a vault's shares, stETH), whether GoPlus Security reports it, and whether this wallet ever sent
/// it a transaction.
public struct ContractReputation: Sendable, Equatable {
    public var tvlUSD: Decimal?
    /// Who reports it as a scam and for what: GoPlus Security, or Blockscout's token reputation.
    public var scam: ScamList.Report?
    public var firstTime = true
    /// GoPlus didn't answer: said on the card, the other checks still hold.
    public var unchecked = false

    /// Below this a protocol isn't one DefiLlama counts as having money in it.
    public init() {}

    public static let minTVL: Decimal = 1_000_000
    public static let minDays = 30

    public static func check(_ contract: String, on net: EVMNetwork, me: String) async -> ContractReputation {
        var r = ContractReputation()
        async let listed = ScamList.shared.check(contract, chainId: net.rpc.chainId)
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
        case .reported(let why): r.scam = why
        case .unchecked: r.unchecked = true
        case .clean: break
        }
        if r.scam == nil, ((await token)?["reputation"] as? String)?.lowercased() == "scam" {
            r.scam = .init(en: "Blockscout marks this address as a scam", ru: "Blockscout помечает этот адрес как скам")
        }
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

/// GoPlus Security's reports on an address (api.gopluslabs.io, SlowMist's and ScamSniffer's data among others), asked
/// without a key. ScamSniffer's own address list stopped in February 2024. A report is never «clean» by default: GoPlus
/// answers its rate limit with HTTP 200 and code 4029, and that, like any failure, is «unchecked». It doesn't know every
/// drainer, so nothing says an address is clean — only that GoPlus reports it or didn't answer.
public actor ScamList {
    public static let shared = ScamList()
    public struct Report: Sendable, Equatable { public let en: String; public let ru: String }
    public enum Verdict: Sendable, Equatable { case clean, unchecked, reported(Report) }
    /// GoPlus's networks among ours. Another (HyperEVM, Tempo) answers zeros even for a drainer: its labels are per
    /// address, so Ethereum's are asked.
    static let chains: Set<UInt64> = [1, 10, 56, 137, 4663, 8453, 42161]
    static func chain(_ id: UInt64) -> UInt64 { chains.contains(id) ? id : 1 }
    /// GoPlus's fields in the order they're named; any other one set is named by its key.
    static let labels: [(key: String, en: String, ru: String)] = [
        ("stealing_attack", "theft", "кража"), ("phishing_activities", "phishing", "фишинг"),
        ("blacklist_doubt", "suspected malice", "подозрительный адрес"), ("honeypot_related_address", "ties to a honeypot", "связан с ханипотом"),
        ("number_of_malicious_contracts_created", "malicious contracts it created", "создавал вредоносные контракты"),
        ("fake_token", "a fake token", "поддельный токен"), ("fake_standard_interface", "a fake standard interface", "поддельный стандартный интерфейс"),
        ("cybercrime", "cybercrime", "киберпреступление"), ("financial_crime", "financial crime", "финансовое преступление"),
        ("money_laundering", "money laundering", "отмывание денег"), ("mixer", "mixer use", "миксер"), ("sanctioned", "sanctions", "санкции"),
        ("darkweb_transactions", "darkweb deals", "сделки в даркнете"), ("blackmail_activities", "blackmail", "шантаж"),
        ("malicious_mining_activities", "malicious mining", "вредоносный майнинг"), ("gas_abuse", "gas abuse", "злоупотребление газом"),
        ("fake_kyc", "fake KYC", "поддельный KYC"), ("reinit", "a contract that can be redeployed", "контракт можно развернуть заново"),
    ]
    private var seen: [String: (verdict: Verdict, at: Date)] = [:]

    /// An hour's answer is kept; «unchecked» never is. Five seconds at most: it runs again after Sign, and a GoPlus that
    /// doesn't answer only says so. The wallet's own addresses aren't to be passed: GoPlus sees each one.
    public func check(_ address: String, chainId: UInt64) async -> Verdict {
        let address = address.lowercased()
        guard address.wholeMatch(of: /0x[0-9a-f]{40}/) != nil else { return .unchecked }
        let key = "\(Self.chain(chainId)):\(address)"
        if let s = seen[key], Date().timeIntervalSince(s.at) < 3600 { return s.verdict }
        guard let d = try? await HTTP.get("https://api.gopluslabs.io/api/v1/address_security/\(address)?chain_id=\(Self.chain(chainId))", timeout: 5)
        else { return .unchecked }
        let v = Self.verdict(d)
        if v != .unchecked { seen[key] = (v, Date()) }
        return v
    }

    /// Each address asked at once, keyed lowercased.
    public func verdicts(_ addresses: some Sequence<String>, chainId: UInt64) async -> [String: Verdict] {
        await withTaskGroup(of: (String, Verdict).self) { g in
            for a in Set(addresses.map { $0.lowercased() }) { g.addTask { (a, await self.check(a, chainId: chainId)) } }
            var out: [String: Verdict] = [:]
            for await (a, v) in g { out[a] = v }
            return out
        }
    }

    /// Code 1 with a result, else unchecked. Any field set but contract_address (a contract or not) and data_source
    /// (whose data it is) is a report.
    static func verdict(_ data: Data) -> Verdict {
        guard let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any], j["code"] as? Int == 1,
              let result = j["result"] as? [String: Any], !result.isEmpty else { return .unchecked }
        let set = Set(result.compactMap { k, v -> String? in
            let s = (v as? String) ?? (v as? NSNumber)?.stringValue ?? "0"
            return ["contract_address", "data_source"].contains(k) || ["", "0"].contains(s) ? nil : k
        })
        guard !set.isEmpty else { return .clean }
        let named = labels.filter { set.contains($0.key) }
        let other = set.subtracting(named.map(\.key)).sorted().map { $0.replacingOccurrences(of: "_", with: " ") }
        let en = named.map(\.en) + other, ru = named.map(\.ru) + other
        let sources = ((result["data_source"] as? String) ?? "").split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        let from = sources.joined(separator: ", ")
        return .reported(Report(
            en: "GoPlus Security reports this address for " + list(en, "and") + (from.isEmpty ? "" : " (\(from) data)"),
            ru: "у GoPlus Security на этом адресе " + (ru.count == 1 ? "метка " : "метки ") + ru.map { "«\($0)»" }.joined(separator: ", ")
                + (from.isEmpty ? "" : " (данные \(from))")))
    }

    private static func list(_ items: [String], _ and: String) -> String {
        items.count < 2 ? items.joined() : items.dropLast().joined(separator: ", ") + " \(and) " + items.last!
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

    public enum Risk: Equatable { case usd(Decimal), unpriced(String), uncounted(String) }

    /// What a call to a protocol outside the list puts at risk, in dollars: what leaves the wallet, or its approve when
    /// that's more (the contract may take the rest of an approve later). A token neither DefiLlama nor the dollar list
    /// prices fails closed, an approve too — it's a spend that hasn't happened yet, never $0. An outflow without decimals
    /// (no amount) can't be counted; its price is asked first, so an NFT leaving says it can't be priced.
    public static func atRisk(leaving: [(token: String, symbol: String, amount: Decimal?)],
                              approve: (token: String, symbol: String, amount: Decimal)?,
                              priced: [String: Decimal]) -> Risk {
        func price(_ token: String, _ symbol: String) -> Decimal? {
            priced[token.lowercased()] ?? (Stablecoins.dollars.contains(symbol.uppercased()) ? 1 : nil)
        }
        var sum: Decimal = 0
        for t in leaving {
            guard let p = price(t.token, t.symbol) else { return .unpriced(t.symbol) }
            guard let amount = t.amount else { return .uncounted(t.symbol) }
            sum += amount * p
        }
        // An approve of 0 is a revoke: it lets the contract take nothing.
        guard let a = approve, a.amount > 0 else { return .usd(sum) }
        guard let p = price(a.token, a.symbol) else { return .unpriced(a.symbol) }
        return .usd(max(sum, a.amount * p))
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

import EVMChain
import Foundation
import SignerAPI
import SolanaChain
import Synchronization
import TradeExecution
import TxGuard
import WalletCore

// Only in the copies build-app.sh makes (DEBUG_API): a release has no way to arm it.
#if DEBUG_API
/// The debug API's attack drill (POST /tamper): the next signature request on its way to the signer is changed the way
/// a program that took over the app would change it, after every check the app makes, so the run shows what the
/// signer itself catches. `payee`: the money goes to a fresh address; `amount`: twice as much to the same payee;
/// `relay`: the Relay order the signer is asked to hold pays a fresh address. A changed request the signer signs is
/// never sent: `judged` throws instead, and the chat says so.
nonisolated enum Tamper {
    enum Kind: String, CaseIterable, Sendable { case payee, amount, relay }

    struct Status: Sendable {
        /// Cleared the moment a request takes it: one drill changes one request.
        var armed: Kind?
        /// What was changed, in words, once a request was.
        var did: String?
        /// The signer's answer that shows the change: «refused: …» or «SIGNED (not sent)».
        var signer: String?
        /// A changed Relay order is only held: the next Relay deposit is the signature that shows whether it got through.
        var watchingRelay = false
    }

    private static let state = Mutex(Status())

    static func arm(_ kind: Kind?) { state.withLock { $0 = Status(armed: kind) } }
    static var status: Status { state.withLock { $0 } }

    /// The request changed as armed, or nil (nothing armed, or nothing of the kind in this request). Taken in one lock, so
    /// two signatures at once can't both be changed.
    static func apply(_ request: SignerRequest) -> SignerRequest? {
        state.withLock { s in
            guard let kind = s.armed, let (changed, did) = change(request, kind) else { return nil }
            s = Status(did: did, watchingRelay: kind == .relay)
            return changed
        }
    }

    /// The signer's reply to `request` (as the app built it; `changed`: it went to the signer changed). A changed request
    /// it signs is dropped by this request's own flag, whatever happened to the drill's state meanwhile, so nothing
    /// changed here ever reaches a network. A refusal goes on as any refusal does.
    static func judged(_ reply: SignerReply, request: SignerRequest, changed: Bool) throws -> SignerReply {
        let order: Bool = if case .relayOrder = request { true } else { false }
        let judge: Bool
        if changed {
            // A changed order the signer accepts is only held: the deposit after it tells.
            judge = !(order && reply.ok)
            if order, !reply.ok { state.withLock { $0.watchingRelay = false } }
        } else {
            judge = !order && relayDeposit(request) && state.withLock { s in
                defer { s.watchingRelay = false }
                return s.watchingRelay
            }
        }
        guard judge else { return reply }
        if reply.ok {
            let did = state.withLock { s in
                s.signer = "SIGNED (not sent)"
                return s.did ?? ""
            }
            throw GuardStop("Tamper drill: the signer signed \(changed ? "the changed request" : "a Relay deposit after the changed order") (\(did)). It was not sent.")
        }
        state.withLock { $0.signer = "refused: " + XPCTradeSigner.reason(reply) }
        return reply
    }

    /// A signature that pays into Relay for an order: the Solana or EVM deposit, the gasless permit, Hyperliquid's mapping.
    static func relayDeposit(_ request: SignerRequest) -> Bool {
        switch request {
        case .signRelayMulticallPermit, .signRelayNonceMapping: true
        case .signSolana(let wire, _, _, _, _, _): (try? SolanaMessage.parse(transaction: wire)).map { !SolanaGuard.relayDepositIDs($0).isEmpty } ?? false
        case .signEVMTransaction(_, _, _, _, _, let to, _, let data, _): EVMCall.relayDepositID(to: to, data: data) != nil
        case .signEVMBatch(_, _, _, _, _, let calls, _): calls.contains { EVMCall.relayDepositID(to: $0.to, data: $0.data) != nil }
        default: false
        }
    }

    static func change(_ request: SignerRequest, _ kind: Kind) -> (SignerRequest, String)? {
        switch (request, kind) {
        case (.signSolana(let wire, let accounts, let recipients, let owners, let extra, let sponsor), .payee),
             (.signSolana(let wire, let accounts, let recipients, let owners, let extra, let sponsor), .amount):
            guard let (w, a, did) = solana(wire, accounts, kind) else { return nil }
            return (.signSolana(wire: w, accounts: a, recipients: recipients, tokenOwners: owners, extraPrograms: extra, sponsor: sponsor), did)
        case (.signEVMTransaction(let chainId, let nonce, let tip, let fee, let gas, let to, let value, let data, let recipients), .payee),
             (.signEVMTransaction(let chainId, let nonce, let tip, let fee, let gas, let to, let value, let data, let recipients), .amount):
            guard let (t, v, d, did) = evm(to: to, value: value, data: data, kind) else { return nil }
            return (.signEVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFee: tip, maxFee: fee, gasLimit: gas, to: t, value: v,
                                        data: d, recipients: recipients), did)
        case (.relayOrder(let json), .relay):
            guard var o = try? JSONSerialization.jsonObject(with: json) as? [String: Any], var output = o["output"] as? [String: Any],
                  var payments = output["payments"] as? [[String: Any]], !payments.isEmpty, let chain = output["chainId"] as? String,
                  let was = payments[0]["recipient"] as? String else { return nil }
            let fresh = chain == "solana" ? solanaAddress() : evmAddress()
            payments[0]["recipient"] = fresh
            output["payments"] = payments
            o["output"] = output
            guard let changed = try? JSONSerialization.data(withJSONObject: o) else { return nil }
            return (.relayOrder(order: changed), "Relay order's payee \(was) → \(fresh)")
        default:
            return nil
        }
    }

    /// The first transfer of SOL or a token, or Jupiter v2 swap, whose payee is a static account: its payee replaced in
    /// the message and in the account list (the app's own list would name it too), or its amount doubled.
    static func solana(_ wire: Data, _ accounts: [String], _ kind: Kind) -> (Data, [String], String)? {
        guard let m = try? SolanaMessage.parse(transaction: wire) else { return nil }
        for ix in m.instructions where ix.programIndex < m.staticKeys.count {
            let program = m.staticKeys[ix.programIndex], data = Data(ix.data)
            var to: Int?, amount: Range<Int>?
            if program == "11111111111111111111111111111111", data.count >= 12, data.prefix(4) == Data([2, 0, 0, 0]) { to = 1; amount = 4..<12 }
            else if [SPL.tokenProgram, SPL.token2022Program].contains(program), data.count >= 9, data[0] == 3 || data[0] == 12 {
                to = data[0] == 3 ? 1 : 2; amount = 1..<9
            }
            // SolanaGuard.jupiterRoutes' output accounts: route_v2, shared_accounts_route_v2.
            else if program == SolanaGuard.jupiterSwap { to = ["bb64facc31c4af14": 2, "d19853937cfed8e9": 5][data.prefix(8).hex] }
            guard let to, to < ix.accounts.count else { continue }
            var w = wire, a = accounts
            switch kind {
            case .payee:
                let i = ix.accounts[to]
                guard i < m.staticKeys.count, i < a.count, let old = try? Base58.decode(m.staticKeys[i]) else { continue }
                let fresh = Data((0..<32).map { _ in UInt8.random(in: 0...255) })
                guard replaceOnce(&w, old, fresh) else { continue }
                a[i] = Base58.encode(fresh)
                return (w, a, "payee \(m.staticKeys[i]) → \(a[i])")
            case .amount:
                guard let amount else { continue }
                let units = data[amount].enumerated().reduce(UInt64(0)) { $0 | UInt64($1.element) << (8 * $1.offset) }
                guard units > 0, units < UInt64.max / 2 else { continue }
                var bigger = data
                bigger.replaceSubrange(amount, with: withUnsafeBytes(of: (units * 2).littleEndian) { Data($0) })
                guard replaceOnce(&w, data, bigger) else { continue }
                return (w, a, "amount \(units) → \(units * 2) units")
            case .relay:
                return nil
            }
        }
        return nil
    }

    /// A coin or an ERC-20 `transfer`: its payee replaced, or its amount doubled.
    static func evm(to: String, value: UInt64, data: Data, _ kind: Kind) -> (String, UInt64, Data, String)? {
        if data.count >= 68, data.prefix(4).hex == "a9059cbb" {
            var d = data
            switch kind {
            case .payee:
                let fresh = evmAddress()
                d.replaceSubrange(16..<36, with: Data(evmAddress: fresh)!)
                return (to, value, d, "token payee 0x\(data[16..<36].hex) → \(fresh)")
            case .amount:
                let word = Array(data[36..<68])
                guard word[0] & 0x80 == 0, word.contains(where: { $0 != 0 }) else { return nil }
                var doubled = word, carry: UInt8 = 0
                for i in (0..<32).reversed() { let b = word[i]; doubled[i] = b << 1 | carry; carry = b >> 7 }
                d.replaceSubrange(36..<68, with: doubled)
                return (to, value, d, "token amount doubled (0x\(Data(word).hex.drop { $0 == "0" }) → 0x\(Data(doubled).hex.drop { $0 == "0" }))")
            case .relay: return nil
            }
        }
        guard data.isEmpty, value > 0 else { return nil }
        switch kind {
        case .payee: let fresh = evmAddress(); return (fresh, value, data, "payee \(to) → \(fresh)")
        case .amount: guard value < UInt64.max / 2 else { return nil }; return (to, value * 2, data, "amount \(value) → \(value * 2) wei")
        case .relay: return nil
        }
    }

    /// Replaces `old` where it occurs exactly once; false when it doesn't, so a change never lands on the wrong bytes.
    static func replaceOnce(_ data: inout Data, _ old: Data, _ new: Data) -> Bool {
        guard let r = data.range(of: old), data.range(of: old, in: r.upperBound..<data.endIndex) == nil else { return false }
        data.replaceSubrange(r, with: new)
        return true
    }

    static func evmAddress() -> String { "0x" + Data((0..<20).map { _ in UInt8.random(in: 0...255) }).hex }
    static func solanaAddress() -> String { Base58.encode(Data((0..<32).map { _ in UInt8.random(in: 0...255) })) }
}
#endif

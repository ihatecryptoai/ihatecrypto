import Foundation
import WalletCore

/// A Relay bridge order as its quote gives it (protocol.v2.orderData, version v1). Its id is the order's EIP-712 hashStruct
/// with no domain — the 32 bytes a deposit into Relay's depository names (Solana deposit_token/deposit_native, EVM
/// depositErc20/depositNative, Hyperliquid's NonceMapping) — and Relay's oracle pays out only what the order says. Who
/// gets the money is in the order, so the signer, which has no network, recomputes the id from the order and checks the
/// order before any deposit naming it signs (relay-settlement packages/sdk/src/order getOrderId and normalizeOrder;
/// docs.relay.link references/api/api_core_concepts/input-validation; checked against 11 live quotes 2026-10-24).
public struct RelayOrder: Sendable, Equatable {
    public let id: Data
    public let inputChain: String
    public let inputAmount: String
    public let outputChain: String
    /// The output's one payee, as its chain writes it (an EVM address lowercased, a Solana address in Base58).
    public let recipient: String
    public let minimumAmount: String
    public let deadline: UInt64

    /// For the prompt that allows its payee: what the order does.
    public var summary: String {
        "bridge \(inputAmount) units from \(inputChain) through Relay: at least \(minimumAmount) units to \(recipient) on \(outputChain)"
    }

    enum VM { case evm, svm, hypevm }
    /// The Relay networks ihatecrypto bridges between, by Relay's own name for them (/chains protocol.v2.chainId).
    static let vms: [String: VM] = [
        "solana": .svm, "hyperliquid": .hypevm, "ethereum": .evm, "base": .evm, "arbitrum": .evm, "optimism": .evm, "polygon": .evm,
        "bnb": .evm, "hyperevm": .evm, "robinhood": .evm, "tempo": .evm,
    ]

    public struct Refusal: LocalizedError, Sendable {
        public let reason: String
        public var errorDescription: String? { reason }
    }

    /// The order read and checked: one input, one payee on the output network, every refund to `evm`/`solana` (or the
    /// zero placeholder, which means the depositor), no calls run for it, no fees taken out of it, not yet expired.
    /// The payee may be someone else's address: the signer asks about it in person like any other.
    public static func read(_ json: Data, evm: String?, solana: String?, now: UInt64) throws -> RelayOrder {
        func refuse(_ why: String) -> Refusal { Refusal(reason: "The Relay order " + why + ". Nothing was signed.") }
        guard let o = try? JSONSerialization.jsonObject(with: json) as? [String: Any] else { throw refuse("can't be read") }
        guard o["version"] as? String == "v1" else { throw refuse("is of a version ihatecrypto doesn't read") }
        guard let inputs = o["inputs"] as? [[String: Any]], inputs.count == 1, let payment = inputs[0]["payment"] as? [String: Any],
              let inChain = payment["chainId"] as? String, vms[inChain] != nil else { throw refuse("isn't one input from a network ihatecrypto knows") }
        guard let output = o["output"] as? [String: Any], let outChain = output["chainId"] as? String, vms[outChain] != nil,
              let payments = output["payments"] as? [[String: Any]], payments.count == 1, let pay = payments[0]["recipient"] as? String
        else { throw refuse("doesn't pay one address on a network ihatecrypto knows") }
        guard (output["calls"] as? [Any])?.isEmpty == true else { throw refuse("runs calls of its own on the other network") }
        guard (o["fees"] as? [Any])?.isEmpty == true else { throw refuse("takes fees out for someone") }
        let deadline = (output["deadline"] as? NSNumber)?.uint64Value ?? 0
        guard deadline > now else { throw refuse("has expired") }
        func ours(_ address: String, on chain: String) -> Bool {
            guard let raw = try? bytes(address, chain: chain) else { return false }
            if raw.allSatisfy({ $0 == 0 }) { return true }   // the native-currency placeholder: «the depositor»
            let own = vms[chain] == .svm ? solana.flatMap { try? Base58.decode($0) } : evm.flatMap { Data(hex: $0) }
            return own == raw
        }
        for refund in inputs[0]["refunds"] as? [[String: Any]] ?? [] {
            guard let chain = refund["chainId"] as? String, let to = refund["recipient"] as? String, ours(to, on: chain) else {
                throw refuse("refunds someone else")
            }
        }
        let recipient = vms[outChain] == .svm ? pay : pay.lowercased()
        return RelayOrder(id: try hashOrder(o), inputChain: inChain, inputAmount: payment["amount"] as? String ?? "", outputChain: outChain,
                          recipient: recipient, minimumAmount: payments[0]["minimumAmount"] as? String ?? "", deadline: deadline)
    }

    /// An address or a currency as its network's raw bytes: Base58 on Solana, 20 bytes on an EVM network, 1–20 on Hyperliquid.
    static func bytes(_ s: String, chain: String) throws -> Data {
        switch vms[chain] {
        case .svm?:
            let b = try Base58.decode(s)
            guard b.count == 32 else { throw Refusal(reason: "Not a Solana address: \(s).") }
            return b
        case .evm?:
            guard let b = Data(hex: s), b.count == 20 else { throw Refusal(reason: "Not an EVM address: \(s).") }
            return b
        case .hypevm?:
            guard let b = Data(hex: s), (1...20).contains(b.count) else { throw Refusal(reason: "Not a Hyperliquid address: \(s).") }
            return b
        case nil: throw Refusal(reason: "A network ihatecrypto doesn't bridge to: \(chain).")
        }
    }

    // MARK: EIP-712 hashStruct of the Order

    static let types: [String: [(name: String, type: String)]] = [
        "Order": [("version", "string"), ("solverChainId", "string"), ("solver", "address"), ("salt", "uint256"),
                  ("inputs", "Input[]"), ("output", "Output"), ("fees", "Fee[]")],
        "Input": [("payment", "InputPayment"), ("refunds", "InputRefund[]")],
        "InputPayment": [("chainId", "string"), ("currency", "bytes"), ("amount", "uint256"), ("weight", "uint256")],
        "InputRefund": [("chainId", "string"), ("recipient", "bytes"), ("currency", "bytes"), ("minimumAmount", "uint256"),
                        ("deadline", "uint32"), ("extraData", "bytes")],
        "Output": [("chainId", "string"), ("payments", "OutputPayment[]"), ("deadline", "uint32"), ("calls", "bytes[]"), ("extraData", "bytes")],
        "OutputPayment": [("recipient", "bytes"), ("currency", "bytes"), ("minimumAmount", "uint256"), ("expectedAmount", "uint256")],
        "Fee": [("recipientChainId", "string"), ("recipient", "bytes"), ("currencyChainId", "string"), ("currency", "bytes"), ("amount", "uint256")],
    ]

    static func typeString(_ name: String) -> String {
        func one(_ n: String) -> String { n + "(" + types[n]!.map { "\($0.type) \($0.name)" }.joined(separator: ",") + ")" }
        var deps = Set<String>()
        func walk(_ n: String) {
            for f in types[n]! {
                let base = f.type.hasSuffix("[]") ? String(f.type.dropLast(2)) : f.type
                if types[base] != nil, !deps.contains(base) { deps.insert(base); walk(base) }
            }
        }
        walk(name)
        return one(name) + deps.subtracting([name]).sorted().map(one).joined()
    }

    /// The id: the order's fields hashed the way normalizeOrder hands them over — addresses and currencies as their
    /// network's bytes (which network: the payment's, the refund's, the output's, the fee's own), hex as bytes.
    static func hashOrder(_ o: [String: Any]) throws -> Data {
        func k(_ d: Data) -> Data { Keccak.hash256(d) }
        func hex(_ v: Any?) throws -> Data {
            guard let s = v as? String, let d = Data(hex: s) ?? (s == "0x" ? Data() : nil) else { throw Refusal(reason: "Not hex in the Relay order.") }
            return d
        }
        func string(_ v: Any?) throws -> Data {
            guard let s = v as? String else { throw Refusal(reason: "Not text in the Relay order.") }
            return k(Data(s.utf8))
        }
        func uint(_ v: Any?) throws -> Data { try TypedData.word(v) }
        func addr(_ v: Any?, _ chain: Any?) throws -> Data {
            guard let s = v as? String, let c = chain as? String else { throw Refusal(reason: "An address without its network in the Relay order.") }
            return k(try bytes(s, chain: c))
        }
        func typeHash(_ n: String) -> Data { k(Data(typeString(n).utf8)) }
        func list(_ v: Any?, _ each: ([String: Any]) throws -> Data) throws -> Data {
            guard let items = v as? [[String: Any]] else { throw Refusal(reason: "Not a list in the Relay order.") }
            return k(try items.reduce(Data()) { $0 + (try each($1)) })
        }
        func payment(_ p: [String: Any]) throws -> Data {
            k(typeHash("InputPayment") + (try string(p["chainId"])) + (try addr(p["currency"], p["chainId"])) + (try uint(p["amount"])) + (try uint(p["weight"])))
        }
        func refund(_ r: [String: Any]) throws -> Data {
            k(typeHash("InputRefund") + (try string(r["chainId"])) + (try addr(r["recipient"], r["chainId"])) + (try addr(r["currency"], r["chainId"]))
              + (try uint(r["minimumAmount"])) + (try uint(r["deadline"])) + k(try hex(r["extraData"])))
        }
        func input(_ i: [String: Any]) throws -> Data {
            guard let p = i["payment"] as? [String: Any] else { throw Refusal(reason: "An input without its payment in the Relay order.") }
            return k(typeHash("Input") + (try payment(p)) + (try list(i["refunds"], refund)))
        }
        guard let out = o["output"] as? [String: Any], let outChain = out["chainId"] else { throw Refusal(reason: "The Relay order has no output.") }
        func outputPayment(_ p: [String: Any]) throws -> Data {
            k(typeHash("OutputPayment") + (try addr(p["recipient"], outChain)) + (try addr(p["currency"], outChain))
              + (try uint(p["minimumAmount"])) + (try uint(p["expectedAmount"])))
        }
        guard let calls = out["calls"] as? [String] else { throw Refusal(reason: "The Relay order's calls can't be read.") }
        let output = k(typeHash("Output") + (try string(outChain)) + (try list(out["payments"], outputPayment)) + (try uint(out["deadline"]))
                       + k(try calls.reduce(Data()) { $0 + k(try hex($1)) }) + k(try hex(out["extraData"])))
        func fee(_ f: [String: Any]) throws -> Data {
            k(typeHash("Fee") + (try string(f["recipientChainId"])) + (try addr(f["recipient"], f["recipientChainId"]))
              + (try string(f["currencyChainId"])) + (try addr(f["currency"], f["currencyChainId"])) + (try uint(f["amount"])))
        }
        guard let solver = o["solver"] as? String, let solverBytes = Data(hex: solver), solverBytes.count == 20 else {
            throw Refusal(reason: "The Relay order's solver isn't an address.")
        }
        return k(typeHash("Order") + (try string(o["version"])) + (try string(o["solverChainId"])) + Data(count: 12) + solverBytes
                 + (try uint(o["salt"])) + (try list(o["inputs"], input)) + output + (try list(o["fees"], fee)))
    }
}

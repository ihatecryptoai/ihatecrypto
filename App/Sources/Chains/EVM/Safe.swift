import Foundation
import WalletCore

/// A multisig on Safe (safe.global, v1.4.1): a contract that moves money only when `threshold` of its owners have signed
/// the move. Its contracts sit at the same addresses with the same code on all nine networks ihatecrypto signs for
/// (compared 2026-10-09), so the same owners, threshold and salt make the same multisig address on each of them.
public enum Safe {
    /// SafeProxyFactory 1.4.1.
    public static let factory = "0x4e1dcf7ad4e460cfd30791ccc4f9c8a4f820ec67"
    /// SafeL2 1.4.1, the code every multisig runs: it logs each move, so Safe's own apps and explorers see them.
    public static let singleton = "0x29fcb43b46531bca003ddc8fcb67ffe91900c762"
    /// CompatibilityFallbackHandler 1.4.1, what Safe's own app sets up.
    public static let fallbackHandler = "0xfd0732dc9e303f09fcef3a7388ad10a83459ec99"
    static let zero = "0x0000000000000000000000000000000000000000"

    /// The factory's proxyCreationCode(), read from it on Base (the same on every network): with the singleton it makes the
    /// init code whose hash, with the salt, fixes the address (CREATE2).
    static let proxyCreationCode = Data(hex: "608060405234801561001057600080fd5b506040516101e63803806101e68339818101604052602081101561003357600080fd5b8101908080519060200190929190505050600073ffffffffffffffffffffffffffffffffffffffff168173ffffffffffffffffffffffffffffffffffffffff1614156100ca576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004018080602001828103825260228152602001806101c46022913960400191505060405180910390fd5b806000806101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff1602179055505060ab806101196000396000f3fe608060405273ffffffffffffffffffffffffffffffffffffffff600054167fa619486e0000000000000000000000000000000000000000000000000000000060003514156050578060005260206000f35b3660008037600080366000845af43d6000803e60008114156070573d6000fd5b3d6000f3fea264697066735822122003d1488ee65e08fa41e58e888a9865554c535f2c77126a82cb4c0f917f31441364736f6c63430007060033496e76616c69642073696e676c65746f6e20616464726573732070726f7669646564")!

    static func selector(_ signature: String) -> Data { Data(Keccak.hash256(Data(signature.utf8)).prefix(4)) }
    static let execSelector = selector("execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)")
    /// nonce(), getOwners(), getThreshold(): what a multisig says of itself.
    public static let nonceCall = selector("nonce()")
    public static let ownersCall = selector("getOwners()")
    public static let thresholdCall = selector("getThreshold()")

    /// Why these can't own a multisig, or nil: at least one owner, each a distinct address, and `threshold` of them, 1 or more.
    public static func ownersProblem(_ owners: [String], threshold: Int) -> String? {
        guard !owners.isEmpty else { return "A multisig needs at least one key." }
        guard owners.allSatisfy({ $0.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil && $0.lowercased() != zero && $0.lowercased() != "0x0000000000000000000000000000000000000001" }) else {
            return "Every key of a multisig is an EVM address."
        }
        guard Set(owners.map { $0.lowercased() }).count == owners.count else { return "The same key is in the list twice." }
        guard threshold >= 1, threshold <= owners.count else { return "A multisig needs 1 to \(owners.count) signatures, not \(threshold)." }
        return nil
    }

    /// setup(owners, threshold, to, data, fallbackHandler, paymentToken, payment, paymentReceiver): the owners, how many must
    /// sign and Safe's fallback handler; no module call and nobody paid for creating it.
    public static func setup(owners: [String], threshold: Int) throws -> Data {
        if let problem = ownersProblem(owners, threshold: threshold) { throw WalletError(problem) }
        let n = UInt64(owners.count)
        var out = selector("setup(address[],uint256,address,bytes,address,address,uint256,address)")
        out += ABI.uint(8 * 32) + ABI.uint(UInt64(threshold)) + (try ABI.address(zero)) + ABI.uint(8 * 32 + 32 * (1 + n))
        out += (try ABI.address(fallbackHandler)) + (try ABI.address(zero)) + ABI.uint(0) + (try ABI.address(zero))
        out += ABI.uint(n)
        for owner in owners { out += try ABI.address(owner) }
        return out + ABI.uint(0)   // the empty `data`
    }

    /// createProxyWithNonce(singleton, initializer, saltNonce): the factory's call that makes the multisig.
    public static func create(owners: [String], threshold: Int, salt: UInt64) throws -> Data {
        let initializer = try setup(owners: owners, threshold: threshold)
        return selector("createProxyWithNonce(address,bytes,uint256)") + (try ABI.address(singleton)) + ABI.uint(0x60) + ABI.uint(salt)
            + ABI.uint(UInt64(initializer.count)) + padded(initializer)
    }

    /// Where `create` puts the multisig, before it exists: keccak(0xff ‖ factory ‖ salt ‖ keccak(init code))[12…], the salt
    /// being keccak(keccak(initializer) ‖ saltNonce). Lowercase.
    public static func address(owners: [String], threshold: Int, salt: UInt64) throws -> String {
        let initializer = try setup(owners: owners, threshold: threshold)
        let s = Keccak.hash256(Keccak.hash256(initializer) + ABI.uint(salt))
        let code = Keccak.hash256(proxyCreationCode + (try ABI.address(singleton)))
        return "0x" + Keccak.hash256(Data([0xff]) + Data(hex: factory)! + s + code).suffix(20).hex
    }

    static func padded(_ d: Data) -> Data { d + Data(count: (32 - d.count % 32) % 32) }

    /// One move out of a multisig, as its owners sign it: a plain CALL of `to` with `value` wei and `data`, at the
    /// multisig's `nonce`. Operation, gas refund and refund receiver are always zero here — no DELEGATECALL that would run
    /// another contract's code as the multisig, nobody paid for the gas.
    public struct Move: Sendable, Equatable, Codable {
        public let chainId: UInt64
        public let safe: String
        public let to: String
        /// ponytail: a coin amount in wei as UInt64 caps one payment at ~18 ETH, as EVMTransaction does.
        public let value: UInt64
        public let data: Data
        public let nonce: UInt64

        public init(chainId: UInt64, safe: String, to: String, value: UInt64, data: Data, nonce: UInt64) {
            (self.chainId, self.safe, self.to, self.value, self.data, self.nonce) = (chainId, safe, to, value, data, nonce)
        }

        /// The SafeTx typed data (EIP-712): a Ledger shows it field by field.
        public var typed: EIP712.Typed {
            EIP712.Typed(domain: [EIP712.Field("chainId", "uint256", .uint(chainId)), EIP712.Field("verifyingContract", "address", .address(safe))],
                         type: "SafeTx",
                         message: [EIP712.Field("to", "address", .address(to)), EIP712.Field("value", "uint256", .uint(value)),
                                   EIP712.Field("data", "bytes", .bytes(data)), EIP712.Field("operation", "uint8", .uint(0)),
                                   EIP712.Field("safeTxGas", "uint256", .uint(0)), EIP712.Field("baseGas", "uint256", .uint(0)),
                                   EIP712.Field("gasPrice", "uint256", .uint(0)), EIP712.Field("gasToken", "address", .address(zero)),
                                   EIP712.Field("refundReceiver", "address", .address(zero)), EIP712.Field("nonce", "uint256", .uint(nonce))])
        }

        /// What each owner signs: the multisig's getTransactionHash for this move.
        public func hash() throws -> Data { try typed.parts().digest }

        /// «send 200 USDC to 0x3b9C…e21A on Base from the multisig 0x7a3F…91c2»: read from the calldata, for a prompt.
        public var summary: String {
            EVMCall.decode(data).summary(chainId: chainId, to: to, value: value) + " from the multisig \(safe.prefix(6))…\(safe.suffix(4))"
        }

        /// execTransaction(…, signatures): the move with its owners' signatures, sorted by owner as the contract wants them.
        /// Anyone may send it; the multisig checks the signatures itself.
        public func exec(_ signatures: [(owner: String, signature: EVMSignature)]) throws -> Data {
            let sorted = signatures.sorted { $0.owner.lowercased() < $1.owner.lowercased() }
            var packed = Data()
            for s in sorted {
                guard s.signature.r.count <= 32, s.signature.s.count <= 32, s.signature.v == 27 || s.signature.v == 28 else {
                    throw WalletError("A malformed signature for the multisig.")
                }
                packed += Data(count: 32 - s.signature.r.count) + s.signature.r + Data(count: 32 - s.signature.s.count) + s.signature.s + Data([UInt8(s.signature.v)])
            }
            let dataPart = ABI.uint(UInt64(data.count)) + padded(data)
            var out = execSelector + (try ABI.address(to)) + ABI.uint(value) + ABI.uint(10 * 32) + ABI.uint(0) + ABI.uint(0) + ABI.uint(0) + ABI.uint(0)
            out += (try ABI.address(zero)) + (try ABI.address(zero)) + ABI.uint(UInt64(10 * 32 + dataPart.count))
            return out + dataPart + ABI.uint(UInt64(packed.count)) + padded(packed)
        }
    }

    /// An execTransaction's call read back: what the multisig would do. nil when the calldata isn't one.
    public static func readExec(_ calldata: Data) -> (to: String, value: Data, data: Data, operation: UInt8)? {
        let c = Data(calldata)   // a slice keeps its parent's indices
        guard c.count >= 4 + 10 * 32, c.prefix(4) == execSelector else { return nil }
        let args = Data(c.dropFirst(4))
        guard let to = ABI.address(in: args, word: 0), let offset = ABI.uint(in: args, word: 2), let op = ABI.uint(in: args, word: 3), op <= 255,
              offset % 32 == 0, offset + 32 <= UInt64(args.count), let length = ABI.uint(in: args, word: Int(offset / 32)),
              offset + 32 + length <= UInt64(args.count) else { return nil }
        let start = Int(offset) + 32
        return (to, Data(args.subdata(in: 32..<64)), Data(args.subdata(in: start..<(start + Int(length)))), UInt8(op))
    }

    /// An execTransaction this wallet sends itself (signEVMTransaction, not signSafeMove), held to the same rules: no call
    /// into the multisig (its keys, modules, guard), no contract call the signer can't read, and no gas refund paid out of
    /// it (gasPrice 0) — a refund goes to a receiver the move doesn't name. nil: not an execTransaction, or a plain payout.
    public static func execRefusal(_ calldata: Data, safe: String) -> String? {
        guard let e = readExec(calldata) else { return nil }
        let args = Data(Data(calldata).dropFirst(4))
        if args.count >= 7 * 32, args.subdata(in: 192..<224).contains(where: { $0 != 0 }) {
            return "The multisig move pays a gas refund to an address it doesn't show. ihatecrypto doesn't sign that."
        }
        if e.to.lowercased() == safe.lowercased() { return "The move changes the multisig itself (its keys, modules or guard). ihatecrypto doesn't sign that." }
        if case .contract = EVMCall.decode(e.data) { return "The multisig move calls a contract ihatecrypto can't read. It signs only the multisig's payouts." }
        return nil
    }

    /// getOwners()'s answer: a dynamic address array.
    public static func owners(in data: Data) -> [String]? {
        let d = Data(data)
        guard let offset = ABI.uint(in: d, word: 0), offset % 32 == 0, let n = ABI.uint(in: d, word: Int(offset / 32)), n <= 1_000 else { return nil }
        return (0..<Int(n)).map { ABI.address(in: d, word: Int(offset / 32) + 1 + $0) }.reduce(into: [String]?([])) { list, a in
            if let a { list?.append(a) } else { list = nil }
        }
    }

    /// Why an owner mustn't sign this move, or nil. ihatecrypto signs only the multisig paying out — its own coin to an
    /// address, or a token's plain transfer — never a call into the multisig itself (its keys, modules, guard) or into any
    /// other contract; and the money goes only to `approved` (lowercase: the signer's own address, the multisig itself, and
    /// addresses the user approved in person).
    public static func refusal(_ m: Move, approved: Set<String>) -> String? {
        guard EVMRPC.signable.contains(m.chainId) else { return "ihatecrypto doesn't sign for network \(m.chainId)." }
        guard m.safe.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil, m.to.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil else { return "Not an EVM address in the multisig's move." }
        guard m.to.lowercased() != m.safe.lowercased() else {
            return "The move changes the multisig itself (its keys, modules or guard). ihatecrypto doesn't sign that."
        }
        let call = EVMCall.decode(m.data)
        switch call {
        case .plain where m.data.isEmpty: break
        case .transfer where m.value == 0: break
        default: return "A multisig move ihatecrypto signs is a plain payment of a coin or a token; this one calls the contract \(m.to)."
        }
        for payee in call.payees(to: m.to, value: m.value, chainId: m.chainId) where !approved.contains(payee.lowercased()) {
            return "Money from the multisig goes to an unapproved address \(payee)."
        }
        return nil
    }
}

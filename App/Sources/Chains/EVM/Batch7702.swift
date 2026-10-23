import Foundation
import WalletCore

/// EIP-7702: for one transaction the wallet's own address runs a contract's code, so an approve and the call that spends
/// it go out as one transaction under one Touch ID. The only code ihatecrypto lets the wallet run is eth-infinitism's
/// Simple7702Account v0.8 (verified source on Blockscout; the same bytecode, keccak 0x82c1e6c0f83d…, on Base, Arbitrum,
/// Ethereum, Polygon and Robinhood Chain, checked 2026-10-05): `executeBatch` runs the calls in order and undoes them all
/// when one fails, and only the wallet itself or ERC-4337's EntryPoint v0.8 may call it — so the signer never signs a
/// user operation (TypedData.refusal) nor a call into the wallet's own address (`selfCall`).
public enum Batch7702 {
    public static let delegate = "0x4cd241e8d1510e30b2076397afc7508ae59c66c9"
    /// Networks past Prague (type-4 transactions taken, checked by a signed authorization's gas estimate) with that
    /// bytecode at `delegate`: Ethereum, Polygon, Arbitrum, Base, Robinhood Chain, and Optimism (the same code there,
    /// checked 2026-10-07). Not HyperEVM: no code at `delegate`, and its node refuses type-4 transactions.
    public static let chains: Set<UInt64> = [1, 137, 42161, 8453, 4663, 10]
    /// ERC-4337's EntryPoints v0.6, v0.7 and v0.8: a delegated account obeys a user operation its key signed for one.
    public static let entryPoints: Set<String> = ["0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789", "0x0000000071727de22e5e9d8baf0edac6f37da032",
                                                  "0x4337084d9e255ff0702461cf8895ce9e3b5ff108"]
    /// Gas a transaction's authorization adds on top of what its calls use (EIP-7702's PER_EMPTY_ACCOUNT_COST): the
    /// dry run, made with the code already in place, doesn't count it.
    public static let authorizationGas: UInt64 = 25_000
    /// More calls than any of ihatecrypto's moves makes (Robinhood's v4 swap is three).
    public static let maxCalls = 4

    /// One call of the batch; no coins go with it.
    public struct Call: Codable, Sendable, Equatable {
        public let to: String
        public let data: Data
        public init(to: String, data: Data) { (self.to, self.data) = (to, data) }
    }

    /// executeBatch((address target, uint256 value, bytes data)[]), every value zero.
    public static func executeBatch(_ calls: [Call]) throws -> Data {
        func padded(_ d: Data) -> Data { d + Data(count: (32 - d.count % 32) % 32) }
        let tuples = try calls.map { c in try ABI.address(c.to) + ABI.uint(0) + ABI.uint(96) + ABI.uint(UInt64(c.data.count)) + padded(c.data) }
        var offsets = Data(), at = UInt64(32 * calls.count)
        for t in tuples {
            offsets += ABI.uint(at)
            at += UInt64(t.count)
        }
        return Data([0x34, 0xfc, 0xd5, 0xbe]) + ABI.uint(32) + ABI.uint(UInt64(calls.count)) + offsets + tuples.reduce(Data(), +)
    }

    /// The calls of an executeBatch input, read back; nil when it isn't one (or isn't well formed).
    public static func calls(in data: Data) -> [Call]? {
        guard data.prefix(4) == Data([0x34, 0xfc, 0xd5, 0xbe]) else { return nil }
        let body = Data(data.dropFirst(4))
        func word(_ at: Int) -> Int? {
            guard at >= 0, at + 32 <= body.count, body[at..<at + 24].allSatisfy({ $0 == 0 }) else { return nil }
            return Int(body[at + 24..<at + 32].reduce(UInt64(0)) { $0 << 8 | UInt64($1) })
        }
        guard let array = word(0), let count = word(array), count <= 64 else { return nil }
        var out: [Call] = []
        for i in 0..<count {
            guard let offset = word(array + 32 + 32 * i) else { return nil }
            let tuple = array + 32 + offset
            guard tuple + 96 <= body.count, let at = word(tuple + 64), let length = word(tuple + at), tuple + at + 32 + length <= body.count else { return nil }
            out.append(Call(to: "0x" + body[tuple + 12..<tuple + 32].hex, data: Data(body[tuple + at + 32..<tuple + at + 32 + length])))
        }
        return out
    }

    /// keccak256(0x05 ‖ rlp([chainId, delegate, nonce])): what the key signs to let its address run `delegate`'s code.
    public static func authorizationDigest(chainId: UInt64, nonce: UInt64) -> Data {
        Keccak.hash256(Data([0x05]) + RLP.list([.uint(chainId), .bytes(Data(hex: delegate)!), .uint(nonce)]).encoded)
    }

    /// Why a batch isn't signed, or the empty list. Each call is held to the one-call rules (EVMCall.refusal), none may
    /// call the wallet's own address, and only networks in `chains` take one. Payees are checked by the caller, which
    /// knows the approved recipients.
    public static func problems(_ calls: [Call], owner: String, chainId: UInt64) -> [String] {
        var out: [String] = []
        if !chains.contains(chainId) { out.append("ihatecrypto doesn't batch calls on network \(chainId).") }
        if calls.isEmpty || calls.count > maxCalls { out.append("A batch holds 1 to \(maxCalls) calls, not \(calls.count).") }
        for c in calls {
            if Data(hex: c.to)?.count != 20 { out.append("Not an address: \(c.to).") }
            if let why = selfCall(to: c.to, data: c.data, owner: owner) { out.append(why) }
            if let why = EVMCall.decode(c.data).refusal(sender: owner, chainId: chainId) { out.append(why) }
            if let why = EVMCall.targetRefusal(to: c.to, data: c.data, sender: owner) { out.append(why) }
        }
        return out
    }

    /// A call into the wallet's own address runs whatever code it is delegated to — the batch account's executeBatch
    /// would make it any calls at all, past the one-call rules. Only the signer's own batch transaction does that.
    public static func selfCall(to: String, data: Data, owner: String) -> String? {
        to.lowercased() == owner.lowercased() && !data.isEmpty
            ? "A call into this wallet's own address: it would run calls nobody checked. Refused." : nil
    }

    /// The signed type-4 transaction: from and to the wallet, executeBatch(calls), with the authorization to run
    /// `delegate` at nonce + 1 — the sender's nonce goes up before authorizations are read. Given every time, so the code
    /// that runs is always `delegate`'s, whatever the address was delegated to before.
    public static func signed(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                              owner: String, calls: [Call], privateKey: Data) throws -> (raw: Data, hash: String) {
        let auth = try EVMSigner.sign(digest: authorizationDigest(chainId: chainId, nonce: nonce + 1), privateKey: privateKey)
        var tx = EVMTransaction(chainId: chainId, nonce: nonce, maxPriorityFeePerGas: maxPriorityFee, maxFeePerGas: maxFee,
                                gasLimit: gasLimit, to: owner, value: 0, data: try executeBatch(calls))
        tx.authorizations = [.init(chainId: chainId, address: delegate, nonce: nonce + 1, signature: auth)]
        return try tx.signed(privateKey: privateKey)
    }
}

import CryptoKit
import Foundation

/// A NEAR transaction in borsh (nearcore's Transaction V0), built from plain fields so that what is signed is what
/// was checked: signer, ed25519 key, nonce, receiver, block hash, actions. The hash that is signed is sha256 of it.
public enum NearTransaction {
    public enum Action: Equatable, Sendable {
        case functionCall(method: String, args: Data, gas: UInt64, deposit: UInt128)
        case transfer(deposit: UInt128)
        /// Deletes the signer's own account; NEAR sends all that is left on it to `beneficiary` (which must exist,
        /// or the balance is burned).
        case deleteAccount(beneficiary: String)
    }

    public static let wrapContract = "wrap.near"
    /// wrap.near's storage_balance_bounds (min = max = 0.00125 NEAR), read 2026-09-28.
    public static let wrapStorage: UInt128 = 1_250_000_000_000_000_000_000
    static let tgas: UInt64 = 1_000_000_000_000

    /// Native NEAR into a 1Click deposit: ONE wrap.near transaction — storage_deposit for us and for the deposit
    /// address (wrap.near refunds it when already registered; an unregistered receiver makes ft_transfer revert),
    /// near_deposit of exactly `amount`, ft_transfer of that wNEAR to the deposit address. Actions in one receipt
    /// are atomic: if any fails, nothing moves.
    public static func intentDeposit(account: String, depositAddress: String, amount: UInt128) -> [Action] {
        func json(_ object: [String: Any]) -> Data { (try? JSONSerialization.data(withJSONObject: object, options: .sortedKeys)) ?? Data() }
        return [
            .functionCall(method: "storage_deposit", args: json(["account_id": account, "registration_only": true]), gas: 15 * tgas, deposit: wrapStorage),
            .functionCall(method: "storage_deposit", args: json(["account_id": depositAddress, "registration_only": true]), gas: 15 * tgas, deposit: wrapStorage),
            .functionCall(method: "near_deposit", args: Data("{}".utf8), gas: 15 * tgas, deposit: amount),
            .functionCall(method: "ft_transfer", args: json(["receiver_id": depositAddress, "amount": "\(amount)"]), gas: 30 * tgas, deposit: 1),
        ]
    }

    /// Besides the amount, what `intentDeposit` needs on the account when it is sent: the two storage deposits and
    /// the 75 Tgas of prepaid gas at NEAR's pessimistic price — measured 10.44× the block's gas price for this
    /// transaction (2026-09-28: 0.0783 NEAR at 10⁸ yocto a gas), so 11× here. Unused gas and a registered account's
    /// storage deposit come back afterwards.
    // ponytail: a measured multiplier; if NEAR changes its inflation rule, read the cost from a failed send instead.
    public static func intentDepositOverhead(gasPrice: UInt128) -> UInt128 {
        2 * wrapStorage + 1 + UInt128(75 * tgas) * gasPrice * 11
    }

    public static func serialize(signer: String, publicKey: Data, nonce: UInt64, receiver: String, blockHash: Data, actions: [Action]) -> Data {
        var out = string(signer)
        out.append(0) // ed25519
        out.append(publicKey)
        out.append(le(nonce))
        out.append(string(receiver))
        out.append(blockHash)
        out.append(le(UInt32(actions.count)))
        for action in actions {
            switch action {
            case .functionCall(let method, let args, let gas, let deposit):
                out.append(2)
                out.append(string(method))
                out.append(le(UInt32(args.count)))
                out.append(args)
                out.append(le(gas))
                out.append(le(deposit))
            case .transfer(let deposit):
                out.append(3)
                out.append(le(deposit))
            case .deleteAccount(let beneficiary):
                out.append(7)
                out.append(string(beneficiary))
            }
        }
        return out
    }

    /// The borsh SignedTransaction ready for `send_tx`, and the transaction hash (base58 of it is the explorer id).
    public static func sign(receiver: String, nonce: UInt64, blockHash: Data, actions: [Action], privateKey: Data) throws -> (signed: Data, hash: Data) {
        let key = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let publicKey = key.publicKey.rawRepresentation
        let tx = serialize(signer: publicKey.hex, publicKey: publicKey, nonce: nonce, receiver: receiver, blockHash: blockHash, actions: actions)
        let hash = Data(SHA256.hash(data: tx))
        return (tx + Data([0]) + (try key.signature(for: hash)), hash)
    }

    /// yoctoNEAR as NEAR with up to 6 decimals: «0.1», «0.758442».
    public static func near(_ yocto: UInt128) -> String {
        let micro = yocto / 1_000_000_000_000_000_000, rest = String(micro % 1_000_000)
        let fraction = (String(repeating: "0", count: 6 - rest.count) + rest).replacing(/0+$/, with: "")
        return "\(micro / 1_000_000)" + (fraction.isEmpty ? "" : "." + fraction)
    }

    static func string(_ s: String) -> Data { le(UInt32(s.utf8.count)) + Data(s.utf8) }
    static func le<T: FixedWidthInteger>(_ v: T) -> Data { withUnsafeBytes(of: v.littleEndian) { Data($0) } }
}

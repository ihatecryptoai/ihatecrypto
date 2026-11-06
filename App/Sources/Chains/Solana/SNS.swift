import CryptoKit
import Foundation
import WalletCore

/// Solana Name Service names (toly.sol) to their owner, read on Solana itself: the name's account is a program address
/// of the Name Service from sha256("SPL Name Service" + name) under the .sol root, and its data holds the parent (which
/// must be that root) and the owner. An owner off the curve is a program (a tokenized domain's escrow), never a wallet to
/// pay: refused. Only plain ASCII names, as for ENS.
public enum SNS {
    public static let program = "namesLPneVptA9Z5rqUDD9tMTWEJwofgaYwp8cawRkX"
    public static let root = "58PwtjSDuFHuUkYjH9BYnnQKHfwo9reZhC2zMJv9JPkx"

    public static func isName(_ name: String) -> Bool {
        name.wholeMatch(of: /[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.sol/) != nil
    }

    /// The name's account: find_program_address([sha256(prefix + name), 32 zero bytes (no class), .sol root], program).
    public static func account(_ name: String) throws -> String {
        let label = name.hasSuffix(".sol") ? String(name.dropLast(4)) : name
        let hashed = Data(SHA256.hash(data: Data(("SPL Name Service" + label).utf8)))
        return Base58.encode(try SPL.programAddress(seeds: [hashed, Data(count: 32), try Base58.decode(root)], program: try Base58.decode(program)))
    }

    public static func resolve(_ name: String, rpc: SolanaRPC = SolanaRPC()) async throws -> String {
        guard isName(name) else { throw WalletError("\(name) isn't a .sol name ihatecrypto reads: plain letters, digits and hyphens.") }
        let r = try await rpc.call("getAccountInfo", [try account(name), ["encoding": "base64", "commitment": "confirmed"]]) as? [String: Any]
        guard let value = r?["value"] as? [String: Any], (value["owner"] as? String) == program,
              let data = (value["data"] as? [Any])?.first as? String, let raw = Data(base64Encoded: data), raw.count >= 96 else {
            throw WalletError("\(name) isn't registered.")
        }
        guard Base58.encode(raw.prefix(32)) == root else { throw WalletError("\(name)'s account isn't under .sol.") }
        let owner = raw.subdata(in: 32..<64)
        guard Ed25519.isOnCurve(owner) else { throw WalletError("\(name) is held by a program (a tokenized domain), not a wallet. Paste the address itself.") }
        return Base58.encode(owner)
    }
}

import CryptoKit
import Foundation
import P256K

/// Paths common wallets use, so imported addresses match what the user already sees.
public enum WalletPath {
    /// MetaMask, Rabby, Hardhat: m/44'/60'/0'/0/i
    public static func evm(_ account: Int = 0) -> String { "m/44'/60'/0'/0/\(account)" }
    /// Phantom, Solflare: m/44'/501'/i'/0'
    public static func solana(_ account: Int = 0) -> String { "m/44'/501'/\(account)'/0'" }
    /// MyNearWallet, Ledger, near-seed-phrase: m/44'/397'/i'
    public static func near(_ account: Int = 0) -> String { "m/44'/397'/\(account)'" }
}

public enum EVMAccount {
    /// EIP-55 checksummed address of a secp256k1 private key.
    public static func address(privateKey: Data) throws -> String {
        let uncompressed = try P256K.Signing.PrivateKey(dataRepresentation: privateKey).publicKey.uncompressedRepresentation
        return checksum(Keccak.hash256(uncompressed.dropFirst()).suffix(20).hex)
    }

    /// EIP-55: a hex letter is uppercase when the matching nibble of keccak(lowercase address) is ≥ 8.
    public static func checksum(_ hex: String) -> String {
        let lower = (hex.hasPrefix("0x") ? String(hex.dropFirst(2)) : hex).lowercased()
        let hash = Keccak.hash256(Data(lower.utf8)).hex
        return "0x" + String(zip(lower, hash).map { c, h in c.isLetter && Int(String(h), radix: 16)! >= 8 ? Character(c.uppercased()) : c })
    }
}

public enum SolanaAccount {
    /// Base58 of the ed25519 public key.
    public static func address(privateKey: Data) throws -> String {
        Base58.encode(try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey).publicKey.rawRepresentation)
    }
}

public enum NearAccount {
    /// The implicit account: lowercase hex of the ed25519 public key. It exists on chain once NEAR arrives.
    public static func implicit(privateKey: Data) throws -> String {
        try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey).publicKey.rawRepresentation.hex
    }
}

public enum Base58 {
    static let alphabet = Array("123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz")

    public static func encode(_ data: Data) -> String {
        var digits: [UInt8] = []
        for byte in data {
            var carry = Int(byte)
            for i in digits.indices {
                carry += Int(digits[i]) << 8
                digits[i] = UInt8(carry % 58)
                carry /= 58
            }
            while carry > 0 {
                digits.append(UInt8(carry % 58))
                carry /= 58
            }
        }
        let zeros = data.prefix { $0 == 0 }.count
        return String(repeating: "1", count: zeros) + String(digits.reversed().map { alphabet[Int($0)] })
    }

    public static func decode(_ string: String) throws -> Data {
        var bytes: [UInt8] = []
        for character in string {
            guard let value = alphabet.firstIndex(of: character) else { throw WalletError("Not base58: \(string)") }
            var carry = value
            for i in bytes.indices {
                carry += Int(bytes[i]) * 58
                bytes[i] = UInt8(carry & 0xff)
                carry >>= 8
            }
            while carry > 0 {
                bytes.append(UInt8(carry & 0xff))
                carry >>= 8
            }
        }
        let zeros = string.prefix { $0 == "1" }.count
        return Data(repeating: 0, count: zeros) + Data(bytes.reversed())
    }
}

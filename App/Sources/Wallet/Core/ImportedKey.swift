import CryptoKit
import Foundation

/// A private key pasted from another wallet: which chain it belongs to and its 32 secret bytes. Phantom exports a
/// Solana key as base58 of the 64-byte keypair (secret ‖ public), the Solana CLI as a JSON array of those 64 numbers;
/// MetaMask exports an EVM key as 64 hex digits, with or without 0x. A key covers only its own chain.
public enum ImportedKey: Equatable, Sendable {
    /// The 32-byte ed25519 secret.
    case solana(Data)
    /// The 32-byte secp256k1 secret.
    case evm(Data)

    public static func parse(_ text: String) throws -> ImportedKey {
        let s = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.hasPrefix("[") {
            guard let numbers = try? JSONDecoder().decode([Int].self, from: Data(s.utf8)), numbers.count == 64,
                  numbers.allSatisfy({ (0...255).contains($0) }) else {
                throw WalletError("A Solana key file is a list of 64 numbers from 0 to 255.")
            }
            return try solanaKeypair(Data(numbers.map { UInt8($0) }))
        }
        let hex = s.lowercased().hasPrefix("0x") ? String(s.dropFirst(2)) : s
        if hex.count == 64, let bytes = Data(hex: hex) {
            guard (try? EVMAccount.address(privateKey: bytes)) != nil else { throw WalletError("Those 64 hex digits aren't a valid EVM private key.") }
            return .evm(bytes)
        }
        guard let bytes = try? Base58.decode(s) else {
            throw WalletError("Not a key that can be imported: a Solana key (base58 from Phantom, or the 64 numbers of a key file) or an EVM key (64 hex digits from MetaMask).")
        }
        switch bytes.count {
        case 64: return try solanaKeypair(bytes)
        case 32: return .solana(bytes)
        default: throw WalletError("A Solana private key is 64 bytes (32 for the secret alone); this one decodes to \(bytes.count).")
        }
    }

    /// secret ‖ public: the public half must be the one the secret makes, else the key was copied incompletely.
    static func solanaKeypair(_ bytes: Data) throws -> ImportedKey {
        let secret = Data(bytes.prefix(32))
        guard try Curve25519.Signing.PrivateKey(rawRepresentation: secret).publicKey.rawRepresentation == bytes.suffix(32) else {
            throw WalletError("The key's second half isn't its own public key: it was copied incompletely, or it isn't a Solana key.")
        }
        return .solana(secret)
    }

    /// The address this key signs for, to compare with Phantom or MetaMask before saving.
    public func address() throws -> String {
        switch self {
        case .solana(let secret): try SolanaAccount.address(privateKey: secret)
        case .evm(let secret): try EVMAccount.address(privateKey: secret)
        }
    }

    public var chain: String {
        switch self {
        case .solana: "Solana"
        case .evm: "EVM · Hyperliquid"
        }
    }
}

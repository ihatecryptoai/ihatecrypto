import CryptoKit
import Foundation
import P256K

/// Keys from a seed along a path like "m/44'/60'/0'/0/0".
public enum Derivation {
    static let hardened: UInt32 = 0x8000_0000

    public static func parse(_ path: String) throws -> [UInt32] {
        let parts = path.split(separator: "/")
        guard parts.first == "m" else { throw WalletError("The path must start with m/: \(path)") }
        return try parts.dropFirst().map { part in
            let isHardened = part.hasSuffix("'") || part.hasSuffix("h")
            guard let index = UInt32(isHardened ? part.dropLast() : part), index < hardened else {
                throw WalletError("Invalid path step: \(part)")
            }
            return isHardened ? index | hardened : index
        }
    }

    /// BIP-32 on secp256k1: EVM and Hyperliquid keys. Returns the 32-byte private key.
    public static func secp256k1(seed: Data, path: String) throws -> Data {
        var (key, chain) = split(hmac(key: Data("Bitcoin seed".utf8), seed))
        _ = try P256K.Signing.PrivateKey(dataRepresentation: key) // rejects an invalid master key
        for index in try parse(path) {
            var data = index >= hardened
                ? Data([0]) + key
                : try P256K.Signing.PrivateKey(dataRepresentation: key).publicKey.dataRepresentation // compressed, 33 bytes
            data.append(contentsOf: withUnsafeBytes(of: index.bigEndian, Array.init))
            let (tweak, nextChain) = split(hmac(key: chain, data))
            // child = (parent + tweak) mod n; libsecp256k1 rejects tweak ≥ n or a zero result, as BIP-32 requires.
            key = try P256K.Signing.PrivateKey(dataRepresentation: key).add(Array(tweak)).dataRepresentation
            chain = nextChain
        }
        return key
    }

    /// SLIP-0010 on ed25519: Solana keys. Every step must be hardened.
    public static func ed25519(seed: Data, path: String) throws -> Data {
        var (key, chain) = split(hmac(key: Data("ed25519 seed".utf8), seed))
        for index in try parse(path) {
            guard index >= hardened else { throw WalletError("For ed25519 every path step must be hardened (with ').") }
            var data = Data([0]) + key
            data.append(contentsOf: withUnsafeBytes(of: index.bigEndian, Array.init))
            (key, chain) = split(hmac(key: chain, data))
        }
        return key
    }

    private static func hmac(key: Data, _ data: Data) -> Data {
        Data(HMAC<SHA512>.authenticationCode(for: data, using: SymmetricKey(data: key)))
    }

    private static func split(_ i: Data) -> (Data, Data) { (Data(i.prefix(32)), Data(i.suffix(32))) }
}

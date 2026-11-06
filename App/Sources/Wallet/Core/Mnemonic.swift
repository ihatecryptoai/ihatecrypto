import CommonCrypto
import CryptoKit
import Foundation

public struct WalletError: LocalizedError, Equatable {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

/// BIP-39 recovery phrase: validation and seed.
/// ponytail: the phrase and seed live in ordinary Swift memory and are not zeroed; the XPC signer
/// planned before real funds is where that gets tightened.
public enum Mnemonic {
    /// Checks word count, words and checksum. Returns the normalized phrase (lowercase, single spaces).
    public static func validate(_ phrase: String) throws -> String {
        let words = phrase.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard [12, 15, 18, 21, 24].contains(words.count) else {
            throw WalletError("The phrase must have 12, 15, 18, 21 or 24 words, not \(words.count).")
        }
        var bits = [Bool]()
        for (position, word) in words.enumerated() {
            guard let index = BIP39.index[word] else { throw WalletError("Word #\(position + 1) isn't in the BIP-39 word list.") }
            for shift in (0..<11).reversed() { bits.append(index >> shift & 1 == 1) }
        }
        let checksumBits = bits.count / 33
        let entropyBits = bits.count - checksumBits
        var entropy = [UInt8](repeating: 0, count: entropyBits / 8)
        for i in 0..<entropyBits where bits[i] { entropy[i / 8] |= 0x80 >> UInt8(i % 8) }
        let hash = Array(SHA256.hash(data: entropy))
        for i in 0..<checksumBits where bits[entropyBits + i] != (hash[i / 8] & (0x80 >> UInt8(i % 8)) != 0) {
            throw WalletError("The phrase checksum doesn't match: check the word order and spelling.")
        }
        return words.joined(separator: " ")
    }

    /// A new phrase from 128 bits of system randomness (12 words), or from given entropy for test vectors.
    public static func generate(entropy: Data? = nil) -> String {
        let bytes: [UInt8] = entropy.map(Array.init) ?? {
            var g = SystemRandomNumberGenerator()
            return (0..<16).map { _ in UInt8.random(in: .min ... .max, using: &g) }
        }()
        let checksum: UInt8 = Array(SHA256.hash(data: bytes))[0]
        var bits: [Bool] = []
        for byte in bytes {
            for i in 0..<8 { bits.append((byte >> (7 - i)) & 1 == 1) }
        }
        for i in 0..<(bytes.count / 4) { bits.append((checksum >> (7 - i)) & 1 == 1) }
        var words: [String] = []
        for start in stride(from: 0, to: bits.count, by: 11) {
            var index = 0
            for bit in bits[start..<(start + 11)] { index = index << 1 | (bit ? 1 : 0) }
            words.append(BIP39.words[index])
        }
        return words.joined(separator: " ")
    }

    /// 64-byte seed: PBKDF2-HMAC-SHA512, 2048 rounds, salt "mnemonic" + passphrase, both NFKD.
    public static func seed(_ phrase: String, passphrase: String = "") throws -> Data {
        let password = Array(try validate(phrase).decomposedStringWithCompatibilityMapping.utf8)
        let salt = Array(("mnemonic" + passphrase).decomposedStringWithCompatibilityMapping.utf8)
        var seed = [UInt8](repeating: 0, count: 64)
        let status = password.withUnsafeBufferPointer { p in
            CCKeyDerivationPBKDF(
                CCPBKDFAlgorithm(kCCPBKDF2), UnsafeRawPointer(p.baseAddress!).assumingMemoryBound(to: CChar.self), p.count,
                salt, salt.count, CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA512), 2048, &seed, seed.count
            )
        }
        guard status == kCCSuccess else { throw WalletError("PBKDF2 failed with error \(status).") }
        return Data(seed)
    }
}

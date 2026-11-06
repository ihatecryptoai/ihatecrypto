import CryptoKit
import Foundation

/// Checks an address the user saves: the right shape for its network, the checksum when there is one, and known traps.
/// An address is never guessed or corrected — a wrong one is refused with the reason.
public enum AddressCheck {
    public enum Network: String, Codable, CaseIterable, Sendable {
        case solana, hyperliquid, ethereum, arbitrum, base, robinhood, near, tempo, polygon, hyperevm, optimism, bnb
        /// Networks ihatecrypto has no wallet on: NEAR Intents' 1Click delivers there to a saved address (an exchange's).
        case bitcoin, zcash, tron, xrp, dogecoin, sui

        public var title: String {
            switch self {
            case .solana: "Solana"
            case .hyperliquid: "Hyperliquid"
            case .ethereum: "Ethereum"
            case .arbitrum: "Arbitrum"
            case .base: "Base"
            case .robinhood: "Robinhood Chain"
            case .near: "NEAR"
            case .tempo: "Tempo"
            case .polygon: "Polygon"
            case .hyperevm: "HyperEVM"
            case .optimism: "Optimism"
            case .bnb: "BNB Chain"
            case .bitcoin: "Bitcoin"
            case .zcash: "Zcash"
            case .tron: "TRON"
            case .xrp: "XRP Ledger"
            case .dogecoin: "Dogecoin"
            case .sui: "Sui"
            }
        }
    }

    /// Addresses that swallow money sent the usual way.
    static let traps: [String: String] = [
        "0x2df1c51e09aecf9cacb7bc98cb1742757f163df7": "This is Hyperliquid’s old bridge (Bridge2): USDC sent here the wrong way is lost. Deposit through the Hyperliquid app instead.",
    ]

    /// nil when the address can be saved for this network; otherwise why not.
    public static func problem(_ address: String, network: Network) -> String? {
        let a = address.trimmingCharacters(in: .whitespacesAndNewlines)
        if a.isEmpty { return "Paste the address." }
        switch network {
        case .solana:
            guard (32...44).contains(a.count), let bytes = try? Base58.decode(a), bytes.count == 32 else {
                return "Not a Solana address: 32–44 letters and digits in base58."
            }
            return nil
        case .hyperliquid, .ethereum, .arbitrum, .base, .robinhood, .tempo, .polygon, .hyperevm, .optimism, .bnb:
            guard a.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil else { return "Not an \(network.title) address: 0x and 40 hex characters." }
            let hex = a.dropFirst(2)
            // Mixed case carries an EIP-55 checksum: one wrong character breaks it.
            if hex != hex.lowercased(), hex != hex.uppercased(), EVMAccount.checksum(a) != a {
                return "The checksum doesn’t match, so a character is wrong. Copy the address again from the source."
            }
            return traps[a.lowercased()]
        case .near:
            // An implicit account (64 hex, lower case) or a named one: lower-case parts joined by dots, ending in .near.
            guard a.wholeMatch(of: /[0-9a-f]{64}/) != nil || (a.count <= 64 && a.wholeMatch(of: /[a-z0-9_-]+(?:\.[a-z0-9_-]+)*\.near/) != nil) else {
                return "Not a NEAR account: 64 hex characters, or a name ending in .near (lower case)."
            }
            return nil
        case .bitcoin:
            if a.lowercased().hasPrefix("bc1") { return segwit(a, hrp: "bc") == nil ? "Not a Bitcoin address: its bc1… checksum doesn't match. Copy it again." : nil }
            guard let p = base58Check(a), p.count == 21, [0x00, 0x05].contains(p[0]) else {
                return "Not a Bitcoin address: bc1…, 1… or 3…, with a checksum that matches."
            }
            return nil
        case .dogecoin:
            guard let p = base58Check(a), p.count == 21, [0x1e, 0x16].contains(p[0]) else { return "Not a Dogecoin address: D… (or 9…/A…), with a checksum that matches." }
            return nil
        case .tron:
            guard let p = base58Check(a), p.count == 21, p[0] == 0x41 else { return "Not a TRON address: T… and 34 characters, with a checksum that matches." }
            return nil
        case .zcash:
            guard let p = base58Check(a), p.count == 22, p[0] == 0x1c, [0xb8, 0xbd].contains(p[1]) else {
                return "Not a transparent Zcash address (t1… or t3…). NEAR Intents pays out to those; a shielded or unified one isn't taken here."
            }
            return nil
        case .xrp:
            // A classic r… account, or an X-address, which carries the destination tag an exchange needs.
            guard let p = base58Check(a, alphabet: xrpAlphabet), (p.count == 21 && p[0] == 0x00) || (p.count == 31 && p.prefix(2) == Data([0x05, 0x44])) else {
                return "Not an XRP address: r… or X…, with a checksum that matches."
            }
            return nil
        case .sui:
            guard a.wholeMatch(of: /0x[0-9a-fA-F]{64}/) != nil else { return "Not a Sui address: 0x and 64 hex characters." }
            return nil
        }
    }

    static let xrpAlphabet = Array("rpshnaf39wBUDNEGHJKLM4PQRST7VWXYZ2bcdeCg65jkm8oFqi1tuvAxyz")

    /// Base58 with a 4-byte double SHA-256 checksum (Bitcoin's alphabet, or XRP's own): the payload, or nil.
    static func base58Check(_ s: String, alphabet: [Character] = Base58.alphabet) -> Data? {
        let mapped = String(s.compactMap { c in alphabet.firstIndex(of: c).map { Base58.alphabet[$0] } })
        guard mapped.count == s.count, let raw = try? Base58.decode(mapped), raw.count > 4 else { return nil }
        let payload = raw.dropLast(4)
        return Data(SHA256.hash(data: Data(SHA256.hash(data: payload)))).prefix(4) == raw.suffix(4) ? Data(payload) : nil
    }

    /// A segwit address's witness version and program, its checksum checked (bech32 for version 0, bech32m after,
    /// BIP-173 and BIP-350); nil when anything is off.
    static func segwit(_ s: String, hrp: String) -> (version: Int, program: Data)? {
        let a = s.lowercased()
        guard s == a || s == s.uppercased(), a.count <= 90, let sep = a.lastIndex(of: "1"), String(a[..<sep]) == hrp else { return nil }
        let charset = Array("qpzry9x8gf2tvdw0s3jn54khce6mua7l")
        var values: [Int] = []
        for c in a[a.index(after: sep)...] {
            guard let v = charset.firstIndex(of: c) else { return nil }
            values.append(v)
        }
        guard values.count >= 7, let version = values.first, version <= 16 else { return nil }
        let generator = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
        var check = 1
        for v in hrp.unicodeScalars.map({ Int($0.value) >> 5 }) + [0] + hrp.unicodeScalars.map({ Int($0.value) & 31 }) + values {
            let top = check >> 25
            check = (check & 0x1ffffff) << 5 ^ v
            for i in 0..<5 where (top >> i) & 1 == 1 { check ^= generator[i] }
        }
        guard check == (version == 0 ? 1 : 0x2bc830a3) else { return nil }
        var acc = 0, bits = 0, program = Data()
        for v in values.dropFirst().dropLast(6) {
            acc = (acc << 5 | v) & 0xfff
            bits += 5
            if bits >= 8 { bits -= 8; program.append(UInt8((acc >> bits) & 0xff)) }
        }
        guard bits < 5, acc & ((1 << bits) - 1) == 0, (2...40).contains(program.count), version != 0 || [20, 32].contains(program.count) else { return nil }
        return (version, program)
    }

    /// An XRP account with a destination tag as one X-address (XLS-5d): how NEAR Intents takes a tag an exchange needs.
    /// nil when `classic` isn't an r… account.
    public static func xAddress(_ classic: String, tag: UInt32) -> String? {
        guard let p = base58Check(classic, alphabet: xrpAlphabet), p.count == 21, p[0] == 0 else { return nil }
        var payload = Data([0x05, 0x44]) + p.dropFirst() + Data([0x01])
        payload += withUnsafeBytes(of: tag.littleEndian) { Data($0) } + Data(count: 4)
        let raw = payload + Data(SHA256.hash(data: Data(SHA256.hash(data: payload)))).prefix(4)
        return String(Base58.encode(raw).map { Base58.alphabet.firstIndex(of: $0).map { xrpAlphabet[$0] } ?? $0 })
    }

    /// Why a name or memo can't go on the Touch ID prompt beside its address, or nil: at most 64 characters, none that
    /// break the line, reorder the text (bidi marks) or don't show — else a name could draw a fake address line.
    public static func labelProblem(_ text: String) -> String? {
        if text.count > 64 { return "A name or memo is at most 64 characters." }
        if text.unicodeScalars.contains(where: hidden) { return "A name or memo can't hold line breaks or invisible characters." }
        return nil
    }

    /// Line breaks, control characters and invisible formatting (bidi overrides, zero-width marks).
    public static func hidden(_ c: Unicode.Scalar) -> Bool {
        [.control, .format, .lineSeparator, .paragraphSeparator].contains(c.properties.generalCategory)
    }

    /// «7xKX 9fQm … 3kPq 2Wd1»: the start and the end in groups of four, the way to compare an address by eye.
    public static func ends(_ address: String) -> (start: String, end: String) {
        func groups(_ s: Substring) -> String { stride(from: 0, to: s.count, by: 4).map { i in String(s.dropFirst(i).prefix(4)) }.joined(separator: " ") }
        return (groups(address.prefix(8)), groups(address.suffix(8)))
    }
}

import CryptoKit
import Foundation

/// What a NEAR account owner signs off-chain, for NEAR Intents' Agent API: a NEP-413 message and a NEP-366 delegate
/// action (a gasless transaction a relayer sends). Plain functions over plain fields, so the signer checks exactly
/// what it signs; the borsh layouts follow the NEPs and nearcore.
public enum NearOwnerSigning {
    /// NEP-413: sha256 of the tag 2³¹+413 and borsh {message, nonce[32], recipient, callbackUrl: None}, signed ed25519.
    public static func nep413Hash(message: String, nonce: Data, recipient: String) -> Data {
        var out = NearTransaction.le(UInt32(2_147_484_061))   // 2^31 + 413
        out.append(NearTransaction.string(message))
        out.append(nonce)
        out.append(NearTransaction.string(recipient))
        out.append(0)   // callbackUrl: None
        return Data(SHA256.hash(data: out))
    }

    public static func nep413(message: String, nonce: Data, recipient: String, privateKey: Data) throws -> Data {
        guard nonce.count == 32 else { throw Problem("A NEP-413 nonce is 32 bytes, not \(nonce.count).") }
        return try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey).signature(for: nep413Hash(message: message, nonce: nonce, recipient: recipient))
    }

    /// borsh DelegateAction: sender, receiver, actions (no nested delegate), nonce, max block height, ed25519 key.
    public static func delegateAction(sender: String, receiver: String, actions: [NearTransaction.Action], nonce: UInt64,
                                      maxBlockHeight: UInt64, publicKey: Data) -> Data {
        // A transaction's body from the receiver on is the same borsh: reuse it, then put the fields in NEP-366's order.
        let body = NearTransaction.serialize(signer: sender, publicKey: publicKey, nonce: nonce, receiver: receiver,
                                             blockHash: Data(count: 32), actions: actions)
        var out = NearTransaction.string(sender) + NearTransaction.string(receiver)
        let actionsAt = NearTransaction.string(sender).count + 1 + 32 + 8 + NearTransaction.string(receiver).count + 32
        out.append(body[actionsAt...])
        out.append(NearTransaction.le(nonce))
        out.append(NearTransaction.le(maxBlockHeight))
        out.append(0)
        out.append(publicKey)
        return out
    }

    /// The borsh SignedDelegateAction: the action and its ed25519 signature over sha256(2³⁰+366 tag ‖ action).
    public static func signDelegate(receiver: String, actions: [NearTransaction.Action], nonce: UInt64, maxBlockHeight: UInt64,
                                    privateKey: Data) throws -> Data {
        let key = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let publicKey = key.publicKey.rawRepresentation
        let action = delegateAction(sender: publicKey.hex, receiver: receiver, actions: actions, nonce: nonce,
                                    maxBlockHeight: maxBlockHeight, publicKey: publicKey)
        let hash = Data(SHA256.hash(data: NearTransaction.le(UInt32(1_073_742_190)) + action))   // 2^30 + 366
        return action + Data([0]) + (try key.signature(for: hash))
    }

    /// A JSON value written as JavaScript's JSON.stringify writes it: compact, keys in the order they came, «/» and
    /// non-ASCII as they are. The Agent API signs `args` that way, and Foundation's JSON writers reorder keys.
    public static func stringify(_ json: Data) throws -> Data {
        var p = Parser(bytes: [UInt8](json))
        var out = [UInt8]()
        try p.value(into: &out)
        p.space()
        guard p.i == p.bytes.count else { throw Problem("Trailing bytes after the JSON value.") }
        return Data(out)
    }

    /// The raw text of `key`'s value in a JSON object, as it came: for fields that must be signed byte for byte.
    public static func rawValue(_ key: String, in object: Data) throws -> Data {
        var p = Parser(bytes: [UInt8](object))
        p.space()
        guard p.peek == UInt8(ascii: "{") else { throw Problem("Not a JSON object.") }
        p.i += 1
        while true {
            p.space()
            if p.peek == UInt8(ascii: "}") { break }
            var name = [UInt8]()
            try p.string(into: &name)
            p.space()
            guard p.peek == UInt8(ascii: ":") else { throw Problem("Bad JSON.") }
            p.i += 1
            p.space()
            let start = p.i
            var skip = [UInt8]()
            try p.value(into: &skip)
            if (try? JSONSerialization.jsonObject(with: Data(name), options: .fragmentsAllowed)) as? String == key {
                return Data(p.bytes[start..<p.i])
            }
            p.space()
            if p.peek == UInt8(ascii: ",") { p.i += 1 }
        }
        throw Problem("No «\(key)» in the JSON object.")
    }

    /// The raw text of each element of a JSON array, as it came.
    public static func rawElements(_ array: Data) throws -> [Data] {
        var p = Parser(bytes: [UInt8](array))
        p.space()
        guard p.peek == UInt8(ascii: "[") else { throw Problem("Not a JSON array.") }
        p.i += 1
        p.space()
        if p.peek == UInt8(ascii: "]") { return [] }
        var out = [Data](), skip = [UInt8]()
        while true {
            p.space()
            let start = p.i
            try p.value(into: &skip)
            out.append(Data(p.bytes[start..<p.i]))
            p.space()
            if p.peek == UInt8(ascii: ",") { p.i += 1; continue }
            guard p.peek == UInt8(ascii: "]") else { throw Problem("Bad JSON array.") }
            return out
        }
    }

    public struct Problem: LocalizedError {
        public let message: String
        public init(_ message: String) { self.message = message }
        public var errorDescription: String? { message }
    }

    private struct Parser {
        let bytes: [UInt8]
        var i = 0
        var peek: UInt8? { i < bytes.count ? bytes[i] : nil }

        mutating func space() { while let c = peek, c == 0x20 || c == 0x0A || c == 0x0D || c == 0x09 { i += 1 } }

        mutating func value(into out: inout [UInt8]) throws {
            space()
            guard let c = peek else { throw Problem("JSON ends early.") }
            switch c {
            case UInt8(ascii: "{"), UInt8(ascii: "["):
                let close: UInt8 = c == UInt8(ascii: "{") ? UInt8(ascii: "}") : UInt8(ascii: "]")
                out.append(c); i += 1; space()
                if peek == close { out.append(close); i += 1; return }
                // A key twice reads differently here (the first wins) and in JavaScript or Rust (the last wins): refused.
                var keys = Set<String>()
                while true {
                    if c == UInt8(ascii: "{") {
                        space()
                        let keyStart = i
                        try string(into: &out)
                        let name = (try? JSONSerialization.jsonObject(with: Data(bytes[keyStart..<i]), options: .fragmentsAllowed)) as? String ?? ""
                        guard keys.insert(name).inserted else { throw Problem("A JSON key appears twice («\(name)»): not signed.") }
                        space()
                        guard peek == UInt8(ascii: ":") else { throw Problem("Bad JSON object.") }
                        out.append(UInt8(ascii: ":")); i += 1
                    }
                    try value(into: &out); space()
                    if peek == UInt8(ascii: ",") { out.append(UInt8(ascii: ",")); i += 1; continue }
                    guard peek == close else { throw Problem("Bad JSON.") }
                    out.append(close); i += 1; return
                }
            case UInt8(ascii: "\""): try string(into: &out)
            default:
                // A number, true, false or null: its own characters, unchanged.
                let start = i
                while let d = peek, !(d == UInt8(ascii: ",") || d == UInt8(ascii: "}") || d == UInt8(ascii: "]") || d == 0x20 || d == 0x0A || d == 0x0D || d == 0x09) { i += 1 }
                guard i > start else { throw Problem("Bad JSON value.") }
                out.append(contentsOf: bytes[start..<i])
            }
        }

        /// A string re-escaped as JSON.stringify does: \" \\ \b \f \n \r \t, other controls as \u00XX, the rest as is.
        mutating func string(into out: inout [UInt8]) throws {
            guard peek == UInt8(ascii: "\"") else { throw Problem("Expected a JSON string.") }
            let start = i
            i += 1
            while let c = peek, c != UInt8(ascii: "\"") { i += c == UInt8(ascii: "\\") ? 2 : 1 }
            guard peek == UInt8(ascii: "\"") else { throw Problem("Unterminated JSON string.") }
            i += 1
            guard let text = try JSONSerialization.jsonObject(with: Data(bytes[start..<i]), options: .fragmentsAllowed) as? String else {
                throw Problem("Bad JSON string.")
            }
            out.append(UInt8(ascii: "\""))
            for scalar in text.unicodeScalars {
                switch scalar {
                case "\"": out += Array("\\\"".utf8)
                case "\\": out += Array("\\\\".utf8)
                case "\u{08}": out += Array("\\b".utf8)
                case "\u{0C}": out += Array("\\f".utf8)
                case "\n": out += Array("\\n".utf8)
                case "\r": out += Array("\\r".utf8)
                case "\t": out += Array("\\t".utf8)
                default:
                    if scalar.value < 0x20 { out += Array(String(format: "\\u%04x", scalar.value).utf8) }
                    else { out += Array(String(scalar).utf8) }
                }
            }
            out.append(UInt8(ascii: "\""))
        }
    }
}

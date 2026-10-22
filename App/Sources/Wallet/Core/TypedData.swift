import Foundation

/// eth_signTypedData_v4 as sites send it, parsed and hashed here rather than trusted from the page.
/// Only flat structs (what logins use); anything nested is refused, and token spending permissions (Permit, Permit2
/// and their kin), Hyperliquid's own actions and payments by signature are recognised so they can be refused outright.
public struct TypedData: Sendable {
    public let primaryType: String
    public let domainName: String
    public let chainId: String
    /// The domain's verifyingContract, lowercased, or "".
    public var verifyingContract = ""
    /// name, type and the value as shown, in the order the site declared them.
    public let fields: [(name: String, type: String, value: String)]
    public let digest: Data

    static let permitTypes: Set<String> = [
        "Permit", "PermitSingle", "PermitBatch", "PermitTransferFrom", "PermitBatchTransferFrom",
        "PermitWitnessTransferFrom", "PermitBatchWitnessTransferFrom", "PermitForAll",
    ]

    /// A signature that lets someone else spend the user's tokens: the drainers' favourite.
    public var isTokenPermission: Bool {
        if Self.permitTypes.contains(primaryType) || primaryType.lowercased().contains("permit") { return true }
        let names = Set(fields.map { $0.name.lowercased() })
        return names.contains("spender") || (names.contains("operator") && names.contains("approved"))
    }

    /// EIP-3009: USDC and its kin move by a signed message that whoever holds it submits.
    static let authorizations: Set<String> = ["TransferWithAuthorization", "ReceiveWithAuthorization"]
    static let payeeNames: Set<String> = ["to", "recipient", "destination", "receiver", "beneficiary", "dst"]

    /// Why a site's typed data isn't signed, or nil. Besides spending permissions: Hyperliquid's own actions — its
    /// user-signed ones (Withdraw, UsdSend, SpotSend, ApproveAgent…, domain «HyperliquidSignTransaction») and the L1
    /// «Agent» that signs an action's hash blind — which the main key, the Hyperliquid account's own, signs only in
    /// ihatecrypto's guarded steps; EIP-3009 transfers by signature; any struct that names a payee and an amount.
    public var refusal: String? {
        // ERC-4337's EntryPoints (v0.6, v0.7, v0.8; Batch7702.entryPoints, which this module can't import): a wallet
        // delegated to an account contract (EIP-7702) does whatever a user operation signed for one says.
        let entryPoints: Set<String> = ["0x5ff137d4b0fdcd49dca30c7cf57e578a026d2789", "0x0000000071727de22e5e9d8baf0edac6f37da032",
                                        "0x4337084d9e255ff0702461cf8895ce9e3b5ff108"]
        if entryPoints.contains(verifyingContract) || domainName == "ERC4337" || primaryType.contains("UserOperation") {
            return "\(primaryType): an ERC-4337 user operation — it would run any calls from this wallet. ihatecrypto doesn't sign these."
        }
        // A Safe multisig's own types: a move, or a message the multisig then vouches for (EIP-1271) — a permit's hash
        // wrapped in one lets a site spend the multisig's tokens. Moves out of a multisig are signed only in ihatecrypto's own
        // steps (signSafeMove), never for a site.
        if primaryType == "SafeTx" || primaryType == "SafeMessage" {
            return "\(primaryType): a signature for a Safe multisig. ihatecrypto signs a multisig's moves only in its own steps, never for a site."
        }
        if isTokenPermission {
            return "\(primaryType): this is permission to spend your tokens (Permit) — the drainers' main trick. ihatecrypto doesn't sign these."
        }
        if domainName == "HyperliquidSignTransaction" || (domainName == "Exchange" && primaryType == "Agent") {
            return "\(primaryType): a Hyperliquid action signed with your main key (a withdrawal, a transfer, a key approval). ihatecrypto signs those only in its own steps, never for a site."
        }
        if Self.authorizations.contains(primaryType) {
            return "\(primaryType): moves your tokens by signature (EIP-3009). ihatecrypto doesn't sign that for a site."
        }
        let names = fields.map { $0.name.lowercased() }
        if let payee = names.first(where: Self.payeeNames.contains), names.contains(where: { $0 == "value" || $0 == "wad" || $0.contains("amount") }) {
            return "\(primaryType) sends an amount to its «\(payee)»: ihatecrypto doesn't sign payments as typed data."
        }
        return nil
    }

    /// The Touch ID prompt's text: the type, the site's name for itself and the fields with their values, as text that
    /// can't draw a line of its own (breaks, controls and bidi marks dropped) and doesn't run on.
    public var prompt: String {
        func plain(_ s: String, _ n: Int) -> String { String(String(s.unicodeScalars.filter { !AddressCheck.hidden($0) }).prefix(n)) }
        // What passed `refusal` is still a kind ihatecrypto can't read: the site's own word for it is all there is.
        return (["sign \(plain(primaryType, 60)) for «\(plain(domainName, 60))»",
                 "ihatecrypto can't tell what this signature allows. Sign only if you trust this site."]
                + fields.prefix(8).map { "\(plain($0.name, 30)): \(plain($0.value, 70))" })
            .joined(separator: "\n")
    }

    public static func parse(_ json: Data) throws -> TypedData {
        guard let j = try JSONSerialization.jsonObject(with: json) as? [String: Any],
              let types = j["types"] as? [String: [[String: String]]], let primary = j["primaryType"] as? String,
              let domain = j["domain"] as? [String: Any], let message = j["message"] as? [String: Any],
              let declared = types[primary] else { throw WalletError("Unreadable typed data.") }
        let domainType = types["EIP712Domain"] ?? ["name", "version", "chainId", "verifyingContract", "salt"].compactMap { key in
            domain[key].map { _ in ["name": key, "type": ["chainId": "uint256", "verifyingContract": "address", "salt": "bytes32"][key] ?? "string"] }
        }
        func fields(_ declared: [[String: String]], _ values: [String: Any]) throws -> [EIP712.Field] {
            try declared.map { d in
                guard let name = d["name"], let type = d["type"] else { throw WalletError("A field without a name or type.") }
                guard types[type] == nil, !type.hasSuffix("]") else { throw WalletError("ihatecrypto doesn't sign nested typed data (\(type)) yet.") }
                return EIP712.Field(name, type, try value(values[name], type: type))
            }
        }
        let domainFields = try fields(domainType, domain)
        let messageFields = try fields(declared, message)
        let shown = declared.map { d in (d["name"] ?? "", d["type"] ?? "", describe(message[d["name"] ?? ""])) }
        var typed = TypedData(primaryType: primary, domainName: domain["name"] as? String ?? "", chainId: describe(domain["chainId"]),
                              fields: shown, digest: try EIP712.digest(domain: domainFields, type: primary, message: messageFields))
        typed.verifyingContract = (domain["verifyingContract"] as? String ?? "").lowercased()
        return typed
    }

    static func describe(_ v: Any?) -> String {
        switch v {
        case let s as String: s
        case let n as NSNumber: n.stringValue
        case nil: "—"
        default: "\(v!)"
        }
    }

    static func value(_ v: Any?, type: String) throws -> EIP712.Value {
        switch type {
        case "string": return .string(v as? String ?? "")
        case "address":
            guard let a = v as? String else { throw WalletError("No address.") }
            return .address(a)
        case "bool": return .bool(v as? Bool ?? ((v as? String) == "true"))
        case "bytes32":
            guard let s = v as? String, let d = Data(hex: s), d.count == 32 else { throw WalletError("Invalid bytes32.") }
            return .bytes32(d)
        case "bytes":
            guard let s = v as? String, let d = Data(hex: s) else { throw WalletError("Invalid bytes.") }
            return .bytes32(Keccak.hash256(d))
        case let t where t.hasPrefix("uint"):
            return .bytes32(try word(v))
        default:
            throw WalletError("ihatecrypto doesn't sign type \(type) yet.")
        }
    }

    /// An unsigned integer as a 32-byte big-endian word, from a JSON number, decimal or 0x-hex string.
    static func word(_ v: Any?) throws -> Data {
        var text = (v as? String) ?? (v as? NSNumber)?.stringValue ?? ""
        if text.hasPrefix("0x") {
            text.removeFirst(2)
            guard let d = Data(hex: String(repeating: "0", count: text.count % 2) + text), d.count <= 32 else { throw WalletError("Number out of uint256 range.") }
            return Data(count: 32 - d.count) + d
        }
        guard !text.isEmpty, text.allSatisfy(\.isNumber) else { throw WalletError("Not a number: \(text).") }
        var bytes = [UInt8](repeating: 0, count: 32)
        for digit in text.compactMap(\.wholeNumberValue) {
            // bytes = bytes × 10 + digit, big-endian
            var carry = digit
            for i in (0..<32).reversed() {
                let x = Int(bytes[i]) * 10 + carry
                bytes[i] = UInt8(x & 0xff)
                carry = x >> 8
            }
            guard carry == 0 else { throw WalletError("Number out of uint256 range.") }
        }
        return Data(bytes)
    }
}

/// EIP-191 personal_sign: what «Sign-In with Ethereum» and most site logins ask for.
public enum PersonalMessage {
    public static func digest(_ message: Data) -> Data {
        Keccak.hash256(Data("\u{19}Ethereum Signed Message:\n\(message.count)".utf8) + message)
    }

    /// 32 raw bytes are a hash, not text: signing one blind can authorise anything.
    public static func isBlindHash(_ message: Data) -> Bool {
        message.count == 32 && String(data: message, encoding: .utf8).map { $0.unicodeScalars.contains { $0.value < 0x20 } } != false
    }
}

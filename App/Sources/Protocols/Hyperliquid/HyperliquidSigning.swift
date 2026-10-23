import Foundation
import WalletCore

/// MessagePack exactly as Python's msgpack.packb writes it: smallest int form, str8 for 32–255 byte strings,
/// maps in insertion order. The action hash depends on every byte, so key order is part of the format.
public indirect enum MsgPack: Sendable, Equatable {
    case null
    case bool(Bool)
    case uint(UInt64)
    case string(String)
    case array([MsgPack])
    case map([(String, MsgPack)])

    public static func == (a: MsgPack, b: MsgPack) -> Bool { a.packed == b.packed }

    public var packed: Data {
        var out = Data()
        write(to: &out)
        return out
    }

    private func write(to out: inout Data) {
        func be<T: FixedWidthInteger>(_ v: T) { withUnsafeBytes(of: v.bigEndian) { out.append(contentsOf: $0) } }
        switch self {
        case .null: out.append(0xc0)
        case .bool(let b): out.append(b ? 0xc3 : 0xc2)
        case .uint(let n):
            switch n {
            case 0..<0x80: out.append(UInt8(n))
            case 0x80...0xff: out.append(0xcc); out.append(UInt8(n))
            case 0x100...0xffff: out.append(0xcd); be(UInt16(n))
            case 0x1_0000...0xffff_ffff: out.append(0xce); be(UInt32(n))
            default: out.append(0xcf); be(n)
            }
        case .string(let s):
            let bytes = Data(s.utf8)
            switch bytes.count {
            case 0..<32: out.append(0xa0 | UInt8(bytes.count))
            case 32...0xff: out.append(0xd9); out.append(UInt8(bytes.count))
            case 0x100...0xffff: out.append(0xda); be(UInt16(bytes.count))
            default: out.append(0xdb); be(UInt32(bytes.count))
            }
            out.append(bytes)
        case .array(let items):
            if items.count < 16 { out.append(0x90 | UInt8(items.count)) } else { out.append(0xdc); be(UInt16(items.count)) }
            for item in items { item.write(to: &out) }
        case .map(let pairs):
            if pairs.count < 16 { out.append(0x80 | UInt8(pairs.count)) } else { out.append(0xde); be(UInt16(pairs.count)) }
            for (key, value) in pairs {
                MsgPack.string(key).write(to: &out)
                value.write(to: &out)
            }
        }
    }
}

public struct HLOrder: Sendable {
    public enum Kind: Sendable {
        /// tif: "Gtc", "Ioc" or "Alo"
        case limit(tif: String)
        /// tpsl: "tp" or "sl"
        case trigger(price: Decimal, isMarket: Bool, tpsl: String)
    }

    public var asset: Int
    public var isBuy: Bool
    public var price: Decimal
    public var size: Decimal
    public var reduceOnly: Bool
    public var kind: Kind
    /// 16 bytes as "0x" + 32 hex chars. Derived from the plan hash so a resend is recognisable.
    public var cloid: String?

    public init(asset: Int, isBuy: Bool, price: Decimal, size: Decimal, reduceOnly: Bool, kind: Kind, cloid: String? = nil) {
        self.asset = asset
        self.isBuy = isBuy
        self.price = price
        self.size = size
        self.reduceOnly = reduceOnly
        self.kind = kind
        self.cloid = cloid
    }
}

/// ihatecrypto's fee on Hyperliquid trades (a builder code) and its referral code. Both are public by design: every order
/// that pays the fee names this address on the exchange, and the code is a public link.
public enum HLBuilder {
    /// Where the fee goes, lowercase as an order's builder field must be.
    public static let address = "0x3828b920d705587b16be05d2d3764333a58c87ec"
    /// Tenths of a basis point: 50 = 0.05% of the trade's value.
    public static let fee: UInt64 = 50
    /// The approval's maxFeeRate: exactly the fee, so the fee can't grow without a new approval.
    public static let rate = "0.05%"
    /// The approval taken back.
    public static let revoke = "0%"
    public static let referralCode = "IHATECRYPTO"
    /// The fee as a share of the trade: 0.0005.
    public static var share: Decimal { Decimal(fee) / 100_000 }

    /// approveBuilderFee, signed by the main key: `maxFeeRate` is `rate` to approve, `revoke` to take it back.
    public static func approveAction(maxFeeRate: String = rate, nonce: UInt64, mainnet: Bool) -> MsgPack {
        .map([
            ("type", .string("approveBuilderFee")), ("maxFeeRate", .string(maxFeeRate)), ("builder", .string(address)),
            ("nonce", .uint(nonce)), ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string(mainnet ? "Mainnet" : "Testnet")),
        ])
    }

    /// setReferrer, an L1 action: the account gets 4% off Hyperliquid's fees, ihatecrypto a share of them.
    public static var referrerAction: MsgPack { .map([("type", .string("setReferrer")), ("code", .string(referralCode))]) }
}

public enum HLAction {
    /// {"type": "order", "orders": […], "grouping": …}. grouping: "na", "normalTpsl" or "positionTpsl".
    public static func order(_ orders: [HLOrder], grouping: String = "na") throws -> MsgPack {
        .map([
            ("type", .string("order")),
            ("orders", .array(try orders.map(wire))),
            ("grouping", .string(grouping)),
        ])
    }

    /// The action's "type" field.
    public static func type(_ action: MsgPack) -> String? { text(action, "type") }

    /// A cancel, or orders that are all reduce-only (a stop, a take, a close): nothing that opens or adds to a position.
    public static func onlyReduces(_ action: MsgPack) -> Bool {
        guard case .map(let pairs) = action else { return false }
        switch type(action) {
        case "cancel", "cancelByCloid": return true
        case "order":
            guard case .array(let orders)? = pairs.first(where: { $0.0 == "orders" })?.1, !orders.isEmpty else { return false }
            return orders.allSatisfy { o in
                guard case .map(let f) = o, case .bool(true)? = f.first(where: { $0.0 == "r" })?.1 else { return false }
                return true
            }
        default: return false
        }
    }

    /// A string field of the action.
    public static func text(_ action: MsgPack, _ key: String) -> String? {
        guard case .map(let pairs) = action, case .string(let s)? = pairs.first(where: { $0.0 == key })?.1 else { return nil }
        return s
    }

    /// The same order action paying ihatecrypto's fee: {"b": address, "f": fee} as its last key, after grouping, where the
    /// SDK puts it (the hash covers the order of keys). Anything else comes back as it was.
    public static func withBuilder(_ action: MsgPack) -> MsgPack {
        guard type(action) == "order", case .map(let pairs) = action, !pairs.contains(where: { $0.0 == "builder" }) else { return action }
        return .map(pairs + [("builder", .map([("b", .string(HLBuilder.address)), ("f", .uint(HLBuilder.fee))]))])
    }

    /// {"type": "cancel", "cancels": [{"a": asset, "o": oid}, …]}
    public static func cancel(_ orders: [(asset: Int, oid: UInt64)]) -> MsgPack {
        .map([("type", .string("cancel")), ("cancels", .array(orders.map { .map([("a", .uint(UInt64($0.asset))), ("o", .uint($0.oid))]) }))])
    }

    static func wire(_ o: HLOrder) throws -> MsgPack {
        let type: MsgPack = switch o.kind {
        case .limit(let tif): .map([("limit", .map([("tif", .string(tif))]))])
        case .trigger(let price, let isMarket, let tpsl):
            .map([("trigger", .map([("isMarket", .bool(isMarket)), ("triggerPx", .string(try number(price))), ("tpsl", .string(tpsl))]))])
        }
        var fields: [(String, MsgPack)] = [
            ("a", .uint(UInt64(o.asset))), ("b", .bool(o.isBuy)),
            ("p", .string(try number(o.price))), ("s", .string(try number(o.size))),
            ("r", .bool(o.reduceOnly)), ("t", type),
        ]
        if let cloid = o.cloid {
            guard cloid.hasPrefix("0x"), cloid.count == 34, Data(hex: cloid) != nil else { throw WalletError("cloid must be 0x plus 32 hex characters.") }
            fields.append(("c", .string(cloid)))
        }
        return .map(fields)
    }

    /// Like the SDK's _slippage_price: 5 significant figures, then at most 6 − szDecimals decimals (8 for spot).
    public static func price(_ px: Decimal, szDecimals: Int, spot: Bool = false) -> Decimal {
        let magnitude = Int(floor(log10(NSDecimalNumber(decimal: px).doubleValue)))
        var value = px, significant = Decimal(), result = Decimal()
        NSDecimalRound(&significant, &value, max(0, 4 - magnitude), .plain)
        NSDecimalRound(&result, &significant, (spot ? 8 : 6) - szDecimals, .plain)
        return result
    }

    /// Size in whole steps of the market's szDecimals, rounded down so it never exceeds what was asked.
    public static func size(_ sz: Decimal, szDecimals: Int) -> Decimal {
        var value = sz, result = Decimal()
        NSDecimalRound(&result, &value, szDecimals, .down)
        return result
    }

    /// Like the SDK's float_to_wire: at most 8 decimals, no trailing zeros, no exponent ("100", "0.0147").
    public static func number(_ d: Decimal) throws -> String {
        var value = d, rounded = Decimal()
        NSDecimalRound(&rounded, &value, 8, .plain)
        guard rounded == d else { throw WalletError("\(d): more than 8 decimal places.") }
        return rounded.isZero ? "0" : NSDecimalNumber(decimal: rounded).stringValue
    }
}

public enum HLSigner {
    /// Orders, cancels, leverage and the referral code: what a trading (agent) key may sign. Nothing here moves money out
    /// of the account (the guard holds an order's fee and the code to ihatecrypto's own).
    public static func isTrading(_ action: MsgPack) -> Bool {
        HLAction.type(action).map(["order", "cancel", "updateLeverage", "batchModify", "modify", "setReferrer"].contains) ?? false
    }

    /// keccak256(msgpack(action) ‖ nonce ‖ vault flag [‖ vault] [‖ 0x00 ‖ expiresAfter]).
    public static func actionHash(_ action: MsgPack, vault: String?, nonce: UInt64, expiresAfter: UInt64? = nil) throws -> Data {
        var data = action.packed
        data.append(contentsOf: withUnsafeBytes(of: nonce.bigEndian, Array.init))
        if let vault {
            guard let bytes = Data(hex: vault), bytes.count == 20 else { throw WalletError("Invalid vault address.") }
            data.append(0x01)
            data.append(bytes)
        } else {
            data.append(0x00)
        }
        if let expiresAfter {
            data.append(0x00)
            data.append(contentsOf: withUnsafeBytes(of: expiresAfter.bigEndian, Array.init))
        }
        return Keccak.hash256(data)
    }

    /// Orders, cancels, leverage: EIP-712 over a "phantom agent" that carries the action hash.
    public static func signL1(
        _ action: MsgPack, privateKey: Data, vault: String? = nil, nonce: UInt64, expiresAfter: UInt64? = nil, mainnet: Bool
    ) throws -> EVMSignature {
        try EVMSigner.sign(digest: try l1Parts(action, vault: vault, nonce: nonce, expiresAfter: expiresAfter, mainnet: mainnet).digest, privateKey: privateKey)
    }

    /// The typed data an L1 action is signed as, in the two hashes a Ledger signs.
    public static func l1Parts(_ action: MsgPack, vault: String? = nil, nonce: UInt64, expiresAfter: UInt64? = nil, mainnet: Bool) throws -> EIP712.Parts {
        try l1Typed(action, vault: vault, nonce: nonce, expiresAfter: expiresAfter, mainnet: mainnet).parts()
    }

    public static func l1Typed(_ action: MsgPack, vault: String? = nil, nonce: UInt64, expiresAfter: UInt64? = nil, mainnet: Bool) throws -> EIP712.Typed {
        let hash = try actionHash(action, vault: vault, nonce: nonce, expiresAfter: expiresAfter)
        return EIP712.Typed(
            domain: [
                .init("name", "string", .string("Exchange")),
                .init("version", "string", .string("1")),
                .init("chainId", "uint256", .uint(1337)),
                .init("verifyingContract", "address", .address("0x0000000000000000000000000000000000000000")),
            ],
            type: "Agent",
            message: [
                .init("source", "string", .string(mainnet ? "a" : "b")),
                .init("connectionId", "bytes32", .bytes32(hash)),
            ]
        )
    }

    /// Transfers, withdrawals, approving an agent: signed by the main wallet with the action fields themselves.
    /// `fields` start after hyperliquidChain, which is added here.
    public static func signUserAction(
        _ type: String, fields: [EIP712.Field], privateKey: Data, mainnet: Bool
    ) throws -> EVMSignature {
        try EVMSigner.sign(digest: try userParts(type, fields: fields, mainnet: mainnet).digest, privateKey: privateKey)
    }

    public static func userParts(_ type: String, fields: [EIP712.Field], mainnet: Bool) throws -> EIP712.Parts {
        try userTyped(type, fields: fields, mainnet: mainnet).parts()
    }

    public static func userTyped(_ type: String, fields: [EIP712.Field], mainnet: Bool) -> EIP712.Typed {
        EIP712.Typed(
            domain: [
                .init("name", "string", .string("HyperliquidSignTransaction")),
                .init("version", "string", .string("1")),
                .init("chainId", "uint256", .uint(0x66eee)),
                .init("verifyingContract", "address", .address("0x0000000000000000000000000000000000000000")),
            ],
            type: "HyperliquidTransaction:\(type)",
            message: [.init("hyperliquidChain", "string", .string(mainnet ? "Mainnet" : "Testnet"))] + fields
        )
    }

    /// Hyperliquid user-signed actions a signer knows, with their EIP-712 fields after hyperliquidChain.
    static let userSchemas: [String: (type: String, fields: [(String, String)])] = [
        "usdClassTransfer": ("UsdClassTransfer", [("amount", "string"), ("toPerp", "bool"), ("nonce", "uint64")]),
        "spotSend": ("SpotSend", [("destination", "string"), ("token", "string"), ("amount", "string"), ("time", "uint64")]),
        "sendAsset": ("SendAsset", [("destination", "string"), ("sourceDex", "string"), ("destinationDex", "string"),
                                    ("token", "string"), ("amount", "string"), ("fromSubAccount", "string"), ("nonce", "uint64")]),
        "withdraw3": ("Withdraw", [("destination", "string"), ("amount", "string"), ("time", "uint64")]),
        "usdSend": ("UsdSend", [("destination", "string"), ("amount", "string"), ("time", "uint64")]),
        "approveBuilderFee": ("ApproveBuilderFee", [("maxFeeRate", "string"), ("builder", "address"), ("nonce", "uint64")]),
    ]

    /// The EIP-712 type and fields of a user-signed action, read from the action itself: what is signed is what the
    /// guard checked.
    public static func userAction(_ action: MsgPack) throws -> (String, [EIP712.Field]) {
        guard case .map(let pairs) = action else { throw WalletError("Unrecognized action.") }
        let values = Dictionary(pairs, uniquingKeysWith: { a, _ in a })
        guard case .string(let kind)? = values["type"], let schema = userSchemas[kind] else {
            throw WalletError("Signing this Hyperliquid action isn't supported.")
        }
        let fields: [EIP712.Field] = try schema.fields.map { name, type in
            switch (type, values[name]) {
            case ("string", .string(let s)?): return EIP712.Field(name, type, .string(s))
            case ("address", .string(let s)?): return EIP712.Field(name, type, .address(s))
            case ("bool", .bool(let b)?): return EIP712.Field(name, type, .bool(b))
            case ("uint64", .uint(let n)?): return EIP712.Field(name, type, .uint(n))
            default: throw WalletError("The action has no \(name) field of type \(type).")
            }
        }
        return (schema.type, fields)
    }

    /// Moves USDC between the HyperCore spot and perp balances. Returns the action to send with the signature.
    public static func usdClassTransfer(amount: Decimal, toPerp: Bool, nonce: UInt64, privateKey: Data, mainnet: Bool) throws -> (MsgPack, EVMSignature) {
        let amountString = try HLAction.number(amount)
        let signature = try signUserAction("UsdClassTransfer", fields: [
            .init("amount", "string", .string(amountString)),
            .init("toPerp", "bool", .bool(toPerp)),
            .init("nonce", "uint64", .uint(nonce)),
        ], privateKey: privateKey, mainnet: mainnet)
        let action = MsgPack.map([
            ("type", .string("usdClassTransfer")), ("amount", .string(amountString)), ("toPerp", .bool(toPerp)),
            ("nonce", .uint(nonce)), ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string(mainnet ? "Mainnet" : "Testnet")),
        ])
        return (action, signature)
    }

    /// Sends a spot token on HyperCore to another address (a 1Click deposit address for the way back).
    /// `token` is "NAME:tokenId", e.g. HyperCore USDC "USDC:0x6d1e7cde53ba9467b783cb7c530ce054".
    public static func spotSend(destination: String, token: String, amount: Decimal, time: UInt64, privateKey: Data, mainnet: Bool) throws -> (MsgPack, EVMSignature) {
        let amountString = try HLAction.number(amount)
        let signature = try signUserAction("SpotSend", fields: [
            .init("destination", "string", .string(destination)),
            .init("token", "string", .string(token)),
            .init("amount", "string", .string(amountString)),
            .init("time", "uint64", .uint(time)),
        ], privateKey: privateKey, mainnet: mainnet)
        let action = MsgPack.map([
            ("type", .string("spotSend")), ("destination", .string(destination)), ("token", .string(token)),
            ("amount", .string(amountString)), ("time", .uint(time)),
            ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string(mainnet ? "Mainnet" : "Testnet")),
        ])
        return (action, signature)
    }

    /// Relay's binding of a Hyperliquid deposit to an order: EIP-712 «RelayNonceMapping», signed by the depositor.
    public static func relayNonceMappingDigest(wallet: String, depositor: String, id: Data, nonce: UInt64) throws -> Data {
        try relayNonceMappingParts(wallet: wallet, depositor: depositor, id: id, nonce: nonce).digest
    }

    public static func relayNonceMappingParts(wallet: String, depositor: String, id: Data, nonce: UInt64) throws -> EIP712.Parts {
        try relayNonceMappingTyped(wallet: wallet, depositor: depositor, id: id, nonce: nonce).parts()
    }

    public static func relayNonceMappingTyped(wallet: String, depositor: String, id: Data, nonce: UInt64) throws -> EIP712.Typed {
        guard id.count == 32 else { throw WalletError("Invalid Relay mapping id.") }
        return EIP712.Typed(
            domain: [.init("name", "string", .string("RelayNonceMapping")), .init("version", "string", .string("2")),
                     .init("chainId", "uint256", .uint(1)), .init("verifyingContract", "address", .address("0x0000000000000000000000000000000000000000"))],
            type: "NonceMapping",
            message: [.init("chainId", "string", .string("hyperliquid")), .init("wallet", "address", .address(wallet)),
                      .init("depositor", "address", .address(depositor)), .init("id", "bytes32", .bytes32(id)),
                      .init("nonce", "uint256", .uint(nonce))])
    }

    /// usdSend: USDC from the perp balance to another Hyperliquid address; `time` (ms) is the nonce. For a signer in
    /// another process, which builds the EIP-712 «UsdSend» from these very fields.
    public static func usdSendAction(destination: String, amount: String, time: UInt64, mainnet: Bool) -> MsgPack {
        MsgPack.map([
            ("type", .string("usdSend")), ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string(mainnet ? "Mainnet" : "Testnet")),
            ("destination", .string(destination)), ("amount", .string(amount)), ("time", .uint(time)),
        ])
    }

    /// The sendAsset action alone, for a signer in another process.
    public static func sendAssetAction(destination: String, sourceDex: String, destinationDex: String, token: String,
                                       amount: String, nonce: UInt64, mainnet: Bool) -> MsgPack {
        MsgPack.map([
            ("type", .string("sendAsset")), ("destination", .string(destination)), ("sourceDex", .string(sourceDex)),
            ("destinationDex", .string(destinationDex)), ("token", .string(token)), ("amount", .string(amount)),
            ("fromSubAccount", .string("")), ("nonce", .uint(nonce)),
            ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string(mainnet ? "Mainnet" : "Testnet")),
        ])
    }

    /// Moves a token between balances or to another address (Relay's depository for the way out).
    /// sourceDex "" = the perp balance, "spot" = spot.
    public static func sendAsset(destination: String, sourceDex: String, destinationDex: String, token: String,
                                 amount: String, nonce: UInt64, privateKey: Data, mainnet: Bool) throws -> (MsgPack, EVMSignature) {
        let signature = try signUserAction("SendAsset", fields: [
            .init("destination", "string", .string(destination)),
            .init("sourceDex", "string", .string(sourceDex)),
            .init("destinationDex", "string", .string(destinationDex)),
            .init("token", "string", .string(token)),
            .init("amount", "string", .string(amount)),
            .init("fromSubAccount", "string", .string("")),
            .init("nonce", "uint64", .uint(nonce)),
        ], privateKey: privateKey, mainnet: mainnet)
        return (sendAssetAction(destination: destination, sourceDex: sourceDex, destinationDex: destinationDex, token: token,
                                amount: amount, nonce: nonce, mainnet: mainnet), signature)
    }

    /// The one approval that lets a separate agent key trade (but not withdraw) for this account.
    /// Signed by the main key; afterwards orders are signed by the agent key.
    public static func approveAgent(agent: String, name: String, nonce: UInt64, privateKey: Data, mainnet: Bool) throws -> (MsgPack, EVMSignature) {
        let (action, typed) = approveAgentTyped(agent: agent, name: name, nonce: nonce, mainnet: mainnet)
        return (action, try EVMSigner.sign(digest: try typed.parts().digest, privateKey: privateKey))
    }

    /// The approval as typed data, for a key held elsewhere (a Ledger signs it and shows the agent's address).
    public static func approveAgentTyped(agent: String, name: String, nonce: UInt64, mainnet: Bool) -> (MsgPack, EIP712.Typed) {
        let typed = userTyped("ApproveAgent", fields: [
            .init("agentAddress", "address", .address(agent)),
            .init("agentName", "string", .string(name)),
            .init("nonce", "uint64", .uint(nonce)),
        ], mainnet: mainnet)
        let action = MsgPack.map([
            ("type", .string("approveAgent")), ("agentAddress", .string(agent)), ("agentName", .string(name)),
            ("nonce", .uint(nonce)), ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string(mainnet ? "Mainnet" : "Testnet")),
        ])
        return (action, typed)
    }
}

/// Sends a signed action to /exchange. Returns the raw JSON reply; the caller decides what counts as success.
public enum HLExchange {
    public static func url(mainnet: Bool) -> URL {
        URL(string: mainnet ? "https://api.hyperliquid.xyz/exchange" : "https://api.hyperliquid-testnet.xyz/exchange")!
    }

    public static func body(action: MsgPack, nonce: UInt64, signature: EVMSignature, vault: String? = nil) -> [String: Any] {
        [
            "action": action.json,
            "nonce": nonce,
            "signature": ["r": "0x" + signature.r.hex, "s": "0x" + signature.s.hex, "v": signature.v],
            "vaultAddress": vault ?? NSNull(),
        ]
    }

    public static func send(action: MsgPack, nonce: UInt64, signature: EVMSignature, mainnet: Bool) async throws -> Data {
        var request = URLRequest(url: url(mainnet: mainnet), timeoutInterval: 20)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body(action: action, nonce: nonce, signature: signature))
        return try await URLSession.shared.data(for: request).0
    }
}

/// Codable with maps as ordered [key, value] pairs, so an action crosses the XPC boundary with its key order intact.
extension MsgPack: Codable {
    enum Kind: String, Codable { case null, bool, uint, string, array, map }
    enum CodingKeys: String, CodingKey { case kind, bool, uint, string, array, keys, values }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(Kind.self, forKey: .kind) {
        case .null: self = .null
        case .bool: self = .bool(try c.decode(Bool.self, forKey: .bool))
        case .uint: self = .uint(try c.decode(UInt64.self, forKey: .uint))
        case .string: self = .string(try c.decode(String.self, forKey: .string))
        case .array: self = .array(try c.decode([MsgPack].self, forKey: .array))
        case .map: self = .map(Array(zip(try c.decode([String].self, forKey: .keys), try c.decode([MsgPack].self, forKey: .values))))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .null: try c.encode(Kind.null, forKey: .kind)
        case .bool(let b): try c.encode(Kind.bool, forKey: .kind); try c.encode(b, forKey: .bool)
        case .uint(let n): try c.encode(Kind.uint, forKey: .kind); try c.encode(n, forKey: .uint)
        case .string(let s): try c.encode(Kind.string, forKey: .kind); try c.encode(s, forKey: .string)
        case .array(let a): try c.encode(Kind.array, forKey: .kind); try c.encode(a, forKey: .array)
        case .map(let pairs):
            try c.encode(Kind.map, forKey: .kind)
            try c.encode(pairs.map(\.0), forKey: .keys)
            try c.encode(pairs.map(\.1), forKey: .values)
        }
    }
}

extension MsgPack {
    /// The same action as JSON for the HTTP body; the server re-encodes it to msgpack to check the hash.
    public var json: Any {
        switch self {
        case .null: NSNull()
        case .bool(let b): b
        case .uint(let n): n
        case .string(let s): s
        case .array(let items): items.map(\.json)
        case .map(let pairs): Dictionary(pairs.map { ($0.0, $0.1.json) }, uniquingKeysWith: { a, _ in a })
        }
    }
}

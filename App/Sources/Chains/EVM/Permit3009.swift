import Foundation
import WalletCore

/// A dollar that Relay takes by the owner's signature (EIP-3009 ReceiveWithAuthorization) instead of a transaction: Relay's
/// approval proxy pulls it and its solver pays the network fee, so a wallet with USDC and none of the network's coin can still
/// move it to another network. The signer builds the typed data itself from these fields: a token it knows by its own domain,
/// from this wallet, only to Relay's approval proxy, valid an hour at most. The nonce is Relay's hash of the calls its proxy
/// runs with the money, so the signature can't be spent on other calls.
public struct Permit3009: Codable, Sendable, Equatable {
    public let chainId: UInt64
    public let token: String
    public let from: String
    public let to: String
    /// In the token's units (6 decimals for every token below).
    public let value: UInt64
    /// Unix seconds; validAfter is always 0.
    public let validBefore: UInt64
    public let nonce: Data

    public init(chainId: UInt64, token: String, from: String, to: String, value: UInt64, validBefore: UInt64, nonce: Data) {
        (self.chainId, self.token, self.from, self.to, self.value, self.validBefore, self.nonce) = (chainId, token, from, to, value, validBefore, nonce)
    }

    /// The dollars it's signed for, by "chain:token": their EIP-712 domain name and version, each checked against the
    /// contract's DOMAIN_SEPARATOR (2026-10-23). Not on BNB Chain (Binance-Peg USDC has no EIP-3009) or HyperEVM: Relay asks
    /// for an approval transaction there.
    public static let tokens: [String: (name: String, version: String, symbol: String)] = [
        "8453:0x833589fcd6edb6e08f4c7c32d4f71b54bda02913": ("USD Coin", "2", "USDC"),
        "42161:0xaf88d065e77c8cc2239327c5edb3a432268e5831": ("USD Coin", "2", "USDC"),
        "10:0x0b2c639c533813f4aa9d7837caf62653d097ff85": ("USD Coin", "2", "USDC"),
        "137:0x3c499c542cef5e3811e1192ce70d8cc03d5c3359": ("USD Coin", "2", "USDC"),
        "1:0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48": ("USD Coin", "2", "USDC"),
        "4663:0x5fc5360d0400a0fd4f2af552add042d716f1d168": ("Global Dollar", "1", "USDG"),
    ]
    /// Relay's approval proxies, at one address on every network: v3, and v3.2 from 2026-10-26 (both stay live a while).
    public static let relayProxies: Set<String> = ["0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", "0x5567954cd026cebfe486062cb75848e0f1c12e58"]

    public static func supports(chainId: UInt64, token: String) -> Bool { tokens["\(chainId):\(token.lowercased())"] != nil }

    var known: (name: String, version: String, symbol: String)? { Self.tokens["\(chainId):\(token.lowercased())"] }

    /// Why the signer won't sign it, or nil. `now`: Unix seconds.
    public func refusal(owner: String, now: UInt64) -> String? {
        guard known != nil else { return "\(token) on network \(chainId) isn't a dollar ihatecrypto pays by signature." }
        guard from.lowercased() == owner.lowercased() else { return "The payment isn't from this wallet." }
        guard Self.relayProxies.contains(to.lowercased()) else { return "Only Relay's approval proxy may take dollars by signature, not \(to)." }
        guard value > 0, nonce.count == 32 else { return "The payment by signature is incomplete." }
        guard validBefore > now, validBefore <= now + 3600 else { return "The signature would stay valid for more than an hour, or has run out." }
        return nil
    }

    public func typed() throws -> EIP712.Typed {
        guard let known else { throw WalletError("Unknown dollar token \(token) on network \(chainId).") }
        return EIP712.Typed(domain: [.init("name", "string", .string(known.name)), .init("version", "string", .string(known.version)),
                                     .init("chainId", "uint256", .uint(chainId)), .init("verifyingContract", "address", .address(token))],
                            type: "ReceiveWithAuthorization",
                            message: [.init("from", "address", .address(from)), .init("to", "address", .address(to)),
                                      .init("value", "uint256", .uint(value)), .init("validAfter", "uint256", .uint(0)),
                                      .init("validBefore", "uint256", .uint(validBefore)), .init("nonce", "bytes32", .bytes32(nonce))])
    }

    /// What the prompt says.
    public var summary: String {
        let amount = Decimal(value) / 1_000_000
        return "pay \(amount) \(known?.symbol ?? "tokens") on \(EVMRPC.names[chainId] ?? "network \(chainId)") to Relay by signature (Relay pays the network fee)"
    }
}

/// Relay's ERC-3009 payment on its v3.2 approval proxy (from 2026-10-26): a second signature, MulticallAuthorization,
/// ties the dollar to the exact calls the proxy then runs with it, and its EIP-712 digest is the ERC-3009 nonce. The
/// signer checks those calls itself — one call into Relay's router that deposits this dollar into Relay's depository with
/// this wallet as the depositor (`cleanupErc20sViaCall`, as Relay's quotes build a deposit to another network) — computes
/// the digest, and only then signs both: the app never hands it the nonce. Hashing follows RelayApprovalProxy.sol
/// (relay-periphery): domain «RelayApprovalProxy» «3.1», read from the proxy's eip712Domain() on 2026-10-23.
public struct RelayMulticallPermit: Codable, Sendable, Equatable {
    public struct Call: Codable, Sendable, Equatable {
        public let target: String
        public let allowFailure: Bool
        public let value: UInt64
        public let callData: Data
        public init(target: String, allowFailure: Bool, value: UInt64, callData: Data) {
            (self.target, self.allowFailure, self.value, self.callData) = (target, allowFailure, value, callData)
        }
    }

    public let chainId: UInt64
    public let token: String
    public let from: String
    public let value: UInt64
    public let validBefore: UInt64
    /// Who sends the transaction (msg.sender, Relay's relayer) and gets what's left over.
    public let relayer: String
    public let refundTo: String
    public let nftRecipient: String
    public let metadata: Data
    public let calls: [Call]

    public init(chainId: UInt64, token: String, from: String, value: UInt64, validBefore: UInt64, relayer: String, refundTo: String,
                nftRecipient: String, metadata: Data, calls: [Call]) {
        (self.chainId, self.token, self.from, self.value, self.validBefore) = (chainId, token, from, value, validBefore)
        (self.relayer, self.refundTo, self.nftRecipient, self.metadata, self.calls) = (relayer, refundTo, nftRecipient, metadata, calls)
    }

    public static let proxy = "0x5567954cd026cebfe486062cb75848e0f1c12e58"
    /// Relay's ERC-20 routers (v3, v3.2) and its depository, at one address on every network.
    public static let routers: Set<String> = ["0xb92fe925dc43a0ecde6c8b1a2709c170ec4fff4f", "0x0729e2a4716d9f7dd17858d5d504426d228697f7"]
    public static let depository = "0x4cd00e387622c35bddb9b4c962c136462338bc31"
    /// cleanupErc20sViaCall(address[],address[],bytes[],uint256[]) on the router; depositErc20(address,address,bytes32) on
    /// the depository (the router's whole balance of the token, for `depositor`).
    static let cleanupViaCall = "73b7bb2f", depositAll = "5a1ee3ac"

    /// The ERC-3009 half, whose nonce is this authorization's digest.
    public func permit() throws -> Permit3009 {
        Permit3009(chainId: chainId, token: token, from: from, to: Self.proxy, value: value, validBefore: validBefore, nonce: try digest())
    }

    /// Why the signer won't sign it, or nil.
    /// The Relay order id its deposit names: depositAll's last word, inside the router's one call (refusal reads the shape).
    public var depositID: Data? {
        guard let call = calls.first, let datas = ABI.bytesArray(Data(call.callData.dropFirst(4)), at: 64), let deposit = datas.first,
              deposit.count == 4 + 96 else { return nil }
        return Data(deposit.suffix(32))
    }

    public func refusal(owner: String, now: UInt64) -> String? {
        if let refusal = (try? permit())?.refusal(owner: owner, now: now) { return refusal }
        // What the proxy has left after the calls goes to refundTo: this wallet, or the relayer that sends it (bound by the
        // digest, as msg.sender) — Relay's quotes name the relayer. The whole dollar is deposited, so none of it is left.
        guard [owner.lowercased(), relayer.lowercased()].contains(refundTo.lowercased()) else {
            return "Relay's leftovers would go to \(refundTo), neither this wallet nor its relayer."
        }
        guard calls.count == 1, let call = calls.first, Self.routers.contains(call.target.lowercased()), !call.allowFailure, call.value == 0,
              call.callData.prefix(4).hex == Self.cleanupViaCall else {
            return "Relay's calls with this dollar aren't one deposit through its router."
        }
        let args = Data(call.callData.dropFirst(4))
        guard let tokens = Self.addresses(args, at: 0), let tos = Self.addresses(args, at: 32), let datas = ABI.bytesArray(args, at: 64),
              let amounts = Self.numbers(args, at: 96), tokens.count == 1, tos.count == 1, datas.count == 1, amounts.count == 1,
              tokens[0] == token.lowercased(), tos[0] == Self.depository, amounts[0] == 0 || amounts[0] == value else {
            return "Relay's router call doesn't deposit this dollar into Relay's depository."
        }
        let deposit = Data(datas[0].dropFirst(4))
        guard datas[0].prefix(4).hex == Self.depositAll, deposit.count == 96, ABI.address(in: deposit, word: 0)?.lowercased() == owner.lowercased(),
              ABI.address(in: deposit, word: 1)?.lowercased() == token.lowercased() else {
            return "Relay's deposit isn't for this wallet."
        }
        return nil
    }

    /// EIP-712 digest of MulticallAuthorization(from, relayer, msgValue 0, refundTo, nftRecipient, metadata, fundingIndex 0,
    /// calls, funding [this dollar]) under the v3.2 proxy's domain.
    public func digest() throws -> Data {
        let k = { (s: String) in Keccak.hash256(Data(s.utf8)) }
        let call3 = "Call3Value(address target,bool allowFailure,uint256 value,bytes callData)"
        let funding = "FundingAuthorization(address token,address from,uint256 value,uint256 validAfter,uint256 validBefore)"
        let callsHash = Keccak.hash256(try calls.reduce(Data()) {
            $0 + Keccak.hash256(k(call3) + (try ABI.address($1.target)) + ABI.uint($1.allowFailure ? 1 : 0) + ABI.uint($1.value) + Keccak.hash256($1.callData))
        })
        let fundingHash = Keccak.hash256(Keccak.hash256(k(funding) + (try ABI.address(token)) + (try ABI.address(from)) + ABI.uint(value) + ABI.uint(0) + ABI.uint(validBefore)))
        let type = "MulticallAuthorization(address from,address relayer,uint256 msgValue,address refundTo,address nftRecipient,bytes metadata,uint256 fundingIndex,Call3Value[] calls,FundingAuthorization[] funding)" + call3 + funding
        let message = Keccak.hash256(k(type) + (try ABI.address(from)) + (try ABI.address(relayer)) + ABI.uint(0) + (try ABI.address(refundTo))
                                     + (try ABI.address(nftRecipient)) + Keccak.hash256(metadata) + ABI.uint(0) + callsHash + fundingHash)
        let domain = try EIP712.hashStruct("EIP712Domain", [.init("name", "string", .string("RelayApprovalProxy")), .init("version", "string", .string("3.1")),
                                                            .init("chainId", "uint256", .uint(chainId)), .init("verifyingContract", "address", .address(Self.proxy))])
        return Keccak.hash256(Data([0x19, 0x01]) + domain + message)
    }

    /// An `address[]` / `uint256[]` argument whose offset sits at byte `at`; nil when it runs out of the data or a word
    /// doesn't fit.
    static func addresses(_ args: Data, at: Int) -> [String]? {
        words(args, at: at)?.map { "0x" + $0.suffix(20).hex }
    }
    static func numbers(_ args: Data, at: Int) -> [UInt64]? {
        guard let w = words(args, at: at) else { return nil }
        let n = w.compactMap { $0.prefix(24).allSatisfy { $0 == 0 } ? $0.suffix(8).reduce(UInt64(0)) { $0 << 8 | UInt64($1) } : nil }
        return n.count == w.count ? n : nil
    }
    static func words(_ args: Data, at: Int) -> [Data]? {
        let data = Data(args)
        guard let offset = ABI.uint(in: data, word: at / 32), offset % 32 == 0, offset < UInt64(data.count),
              let count = ABI.uint(in: data, word: Int(offset) / 32), count <= 64, Int(offset) + 32 + Int(count) * 32 <= data.count else { return nil }
        return (0..<Int(count)).map { data.subdata(in: (Int(offset) + 32 + $0 * 32)..<(Int(offset) + 64 + $0 * 32)) }
    }
}

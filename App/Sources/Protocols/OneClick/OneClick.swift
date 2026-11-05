import Foundation
import WalletCore

/// NEAR Intents 1Click: cross-chain swaps and bridges as quote → deposit → status.
/// Works without a partner key (checked 2026-09-25), so there is no server of our own.
public enum OneClick {
    static let base = "https://1click.chaindefuser.com/v0"

    /// The user's partner key: a JWT from partners.near-intents.org, sent as X-API-Key on every call. Without one 1Click adds
    /// 0.25% to every quote; with one it charges its partner fees, 0.20% or 0.01% on stablecoin and same-asset routes
    /// (docs.near-intents.org/resources/fees, 2026-10-05). Set at launch from the Keychain, and from Settings.
    public static var key: String? {
        get { lock.withLock { own } }
        set { lock.withLock { own = newValue.flatMap { $0.isEmpty ? nil : $0 } } }
    }
    private static let lock = NSLock()
    nonisolated(unsafe) private static var own: String?

    /// A token 1Click swaps: its asset id, network (its `blockchain`: «sol», «base», «near»…), symbol, decimals, contract.
    public struct Token: Sendable, Equatable {
        public let assetId: String
        public let blockchain: String
        public let symbol: String
        public let decimals: Int
        public let contract: String?
        public let price: Decimal?
    }

    /// Every token 1Click swaps (/v0/tokens: ≈ 200 on 37 networks, 2026-10-08), read again after ten minutes.
    public static func tokens() async throws -> [Token] {
        if let (at, list) = lock.withLock({ cached }), Date().timeIntervalSince(at) < 600 { return list }
        let (data, _) = try await URLSession.shared.data(for: request(URL(string: base + "/tokens")!))
        let list = parseTokens(data)
        guard !list.isEmpty else { throw OneClickError(data) }
        lock.withLock { cached = (Date(), list) }
        return list
    }
    nonisolated(unsafe) private static var cached: (Date, [Token])?

    public static func parseTokens(_ data: Data) -> [Token] {
        ((try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]] ?? []).compactMap { t in
            guard let id = t["assetId"] as? String, let chain = t["blockchain"] as? String, let symbol = t["symbol"] as? String,
                  let decimals = (t["decimals"] as? NSNumber)?.intValue else { return nil }
            return Token(assetId: id, blockchain: chain, symbol: symbol, decimals: decimals, contract: (t["contractAddress"] as? String)?.lowercased(),
                         price: (t["price"] as? NSNumber).map { Decimal($0.doubleValue) })
        }
    }

    /// A request to 1Click, with the partner key when there is one.
    static func request(_ url: URL) -> URLRequest {
        var r = URLRequest(url: url, timeoutInterval: 30)
        if let key { r.setValue(key, forHTTPHeaderField: "X-API-Key") }
        return r
    }

    /// Why 1Click won't take `key`, or nil: a dry quote (1 USDC, Solana → Base) asked with it. A key it doesn't know
    /// answers 401 «Invalid token».
    public static func problem(withKey key: String) async -> String? {
        var r = URLRequest(url: URL(string: base + "/quote")!, timeoutInterval: 30)
        r.httpMethod = "POST"
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.setValue(key, forHTTPHeaderField: "X-API-Key")
        r.httpBody = try? JSONSerialization.data(withJSONObject: [
            "dry": true, "swapType": "EXACT_INPUT", "slippageTolerance": 100, "originAsset": Asset.solanaUSDC, "depositType": "ORIGIN_CHAIN",
            "destinationAsset": Asset.usdc(onEVM: "base")!, "amount": "1000000", "refundTo": "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm",
            "refundType": "ORIGIN_CHAIN", "recipient": "0x00000000000000000000000000000000000f0a11", "recipientType": "DESTINATION_CHAIN",
            "deadline": ISO8601DateFormatter().string(from: Date().addingTimeInterval(1800)),
        ] as [String: Any])
        guard let (data, response) = try? await URLSession.shared.data(for: r) else { return "1Click didn't answer. Try again in a minute." }
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        if code == 401 || code == 403 { return "1Click turned the key away (\(code)): copy the whole token from partners.near-intents.org again." }
        return (200..<300).contains(code) ? nil : OneClickError(data).errorDescription
    }

    public enum Asset {
        public static let solanaSOL = "nep141:sol.omft.near"
        public static let solanaUSDC = "nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near"
        /// Lands on the HyperCore spot balance (hip1 token), not directly in perp margin.
        public static let hyperCoreUSDC = "1cs_v1:hypercore:hip1:0x6d1e7cde53ba9467b783cb7c530ce054"
        /// NEAR on the NEAR chain (listed as wNEAR, 24 decimals).
        public static let near = "nep141:wrap.near"
        /// Circle's USDC on the EVM networks 1Click quotes it from (/v0/tokens; a dry quote into NEAR answered for Base,
        /// Arbitrum and Ethereum on 2026-10-05; Polygon's «Quoting for this pair is not available»), by ihatecrypto's key.
        public static func usdc(onEVM key: String) -> String? {
            switch key {
            case "base": "nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near"
            case "arbitrum": "nep141:arb-0xaf88d065e77c8cc2239327c5edb3a432268e5831.omft.near"
            case "ethereum": "nep141:eth-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.omft.near"
            default: nil
            }
        }

        /// What a private send can pay on `network`: the coins of a network ihatecrypto has no wallet on (`across`), or
        /// USDC on Solana and on the EVM networks 1Click takes it on.
        public static func payout(_ symbol: String, on network: String) -> (symbol: String, id: String, decimals: Int)? {
            if let a = across[network]?.first(where: { $0.symbol == symbol.uppercased() }) { return a }
            guard symbol.uppercased() == "USDC", let id = network == "solana" ? solanaUSDC : usdc(onEVM: network) else { return nil }
            return ("USDC", id, 6)
        }

        /// What 1Click pays out on networks ihatecrypto has no wallet on, by the address book's network (/v0/tokens,
        /// 2026-10-06): the asset, its ticker and decimals.
        public static let across: [String: [(symbol: String, id: String, decimals: Int)]] = [
            "bitcoin": [("BTC", "nep141:btc.omft.near", 8)],
            "zcash": [("ZEC", "nep141:zec.omft.near", 8)],
            "tron": [("USDT", "nep141:tron-d28a265909efecdcee7c5028585214ea0b96f015.omft.near", 6), ("TRX", "nep141:tron.omft.near", 6)],
            "xrp": [("XRP", "nep141:xrp.omft.near", 6)],
            "dogecoin": [("DOGE", "nep141:doge.omft.near", 8)],
            "sui": [("SUI", "nep141:sui.omft.near", 9), ("USDC", "nep141:sui-c1b81ecaf27933252d31a963bc5e9458f13c18ce.omft.near", 6)],
        ]
    }

    public struct Quote: Sendable {
        public let depositAddress: String?
        public let amountIn: String
        public let amountInFormatted: String
        /// Present for EXACT_OUTPUT: the input without the slippage reserve.
        public let minAmountIn: String?
        public let amountOutFormatted: String
        public let minAmountOut: String?
        public let deadline: String?
        public let timeEstimate: Int?
        /// What goes in and what comes out, in dollars as 1Click prices them: their gap is its fee and the network's.
        public var amountInUSD: Decimal?
        public var amountOutUSD: Decimal?
    }

    /// `dry: false` issues a deposit address. `refundTo` and `recipient` must be the user's own addresses then.
    public static func quote(
        dry: Bool, exactOutput: Bool = false, origin: String, destination: String, amount: String,
        refundTo: String, recipient: String, slippageBps: Int = 100,
        // A deposit someone else sends (`receiveAcross`): any amount from the quote's minimum, a longer deadline, and the
        // refund to a NEAR Intents account when there's no address of ours on the origin network.
        flexible: Bool = false, refundToIntents: Bool = false, minutes: Double = 30,
        // Confidential Intents: the deposit and the payout can't be matched to each other on chain. Needs the partner key.
        confidential: Bool = false
    ) async throws -> Quote {
        let deadline = ISO8601DateFormatter().string(from: Date().addingTimeInterval(minutes * 60))
        var body: [String: Any] = [
            "dry": dry, "swapType": flexible ? "FLEX_INPUT" : exactOutput ? "EXACT_OUTPUT" : "EXACT_INPUT", "slippageTolerance": slippageBps,
            "originAsset": origin, "depositType": "ORIGIN_CHAIN", "destinationAsset": destination, "amount": amount,
            "refundTo": refundTo, "refundType": refundToIntents ? "INTENTS" : "ORIGIN_CHAIN", "recipient": recipient, "recipientType": "DESTINATION_CHAIN",
            "deadline": deadline, "quoteWaitingTimeMs": 3000,
        ]
        if confidential {
            guard key != nil else { throw OneClickError(message: "A private send needs a NEAR Intents partner key: add it in Settings › General › NEAR Intents key.") }
            body["confidentiality"] = "basic"
        }
        let data = try await post("/quote", body)
        guard let q = (try JSONSerialization.jsonObject(with: data) as? [String: Any])?["quote"] as? [String: Any],
              let amountIn = q["amountIn"] as? String else { throw OneClickError(data) }
        return Quote(
            depositAddress: q["depositAddress"] as? String, amountIn: amountIn,
            amountInFormatted: q["amountInFormatted"] as? String ?? amountIn, minAmountIn: q["minAmountIn"] as? String,
            amountOutFormatted: q["amountOutFormatted"] as? String ?? "?", minAmountOut: q["minAmountOut"] as? String,
            deadline: q["deadline"] as? String, timeEstimate: (q["timeEstimate"] as? NSNumber)?.intValue,
            amountInUSD: (q["amountInUsd"] as? String).flatMap { Decimal(string: $0) }, amountOutUSD: (q["amountOutUsd"] as? String).flatMap { Decimal(string: $0) }
        )
    }

    /// What 1Click has bound to a deposit address: recipient, refundTo, originAsset, destinationAsset, amount.
    /// Read back from 1Click itself, so an address slipped in from elsewhere does not match.
    public static func boundRequest(depositAddress: String) async throws -> [String: String] {
        let url = URL(string: "\(base)/status?depositAddress=\(depositAddress)")!
        let (data, _) = try await URLSession.shared.data(for: request(url))
        guard let quote = (try JSONSerialization.jsonObject(with: data) as? [String: Any])?["quoteResponse"] as? [String: Any],
              let request = quote["quoteRequest"] as? [String: Any] else { throw OneClickError(data) }
        var out: [String: String] = [:]
        for key in ["recipient", "refundTo", "originAsset", "destinationAsset", "amount", "confidentiality"] { out[key] = request[key] as? String }
        out["depositAddress"] = (quote["quote"] as? [String: Any])?["depositAddress"] as? String
        return out
    }

    /// Optional: tells 1Click about the deposit so it does not wait to notice it.
    public static func submitDeposit(txHash: String, depositAddress: String) async throws {
        _ = try await post("/deposit/submit", ["txHash": txHash, "depositAddress": depositAddress])
    }

    public struct Status: Sendable {
        public let status: String
        public let amountOutFormatted: String?
        public let refundedFormatted: String?
        public let destinationTxs: [String]
        public var isFinal: Bool { ["SUCCESS", "REFUNDED", "FAILED"].contains(status) }
    }

    public static func status(depositAddress: String) async throws -> Status {
        let url = URL(string: "\(base)/status?depositAddress=\(depositAddress)")!
        let (data, _) = try await URLSession.shared.data(for: request(url))
        guard let j = try JSONSerialization.jsonObject(with: data) as? [String: Any], let status = j["status"] as? String else {
            throw OneClickError(data)
        }
        let details = j["swapDetails"] as? [String: Any]
        let txs = (details?["destinationChainTxHashes"] as? [[String: Any]] ?? []).compactMap { $0["hash"] as? String }
        return Status(status: status, amountOutFormatted: details?["amountOutFormatted"] as? String,
                      refundedFormatted: details?["refundedAmountFormatted"] as? String, destinationTxs: txs)
    }

    /// A finished swap as 1Click keeps it: what went in and came out, and for whom. For the cost of what the wallet got.
    public struct Settled: Sendable, Equatable {
        public let origin: String, destination: String, recipient: String, refundTo: String
        public let amountIn: Decimal, amountOut: Decimal, inUSD: Decimal

        /// From /status; nil unless it succeeded.
        public init?(_ j: [String: Any]) {
            let d = j["swapDetails"] as? [String: Any] ?? [:]
            let q = (j["quoteResponse"] as? [String: Any])?["quoteRequest"] as? [String: Any] ?? [:]
            func dec(_ x: Any?) -> Decimal? { (x as? String).flatMap { Decimal(string: $0) } }
            guard j["status"] as? String == "SUCCESS", let i = dec(d["amountInFormatted"]), let o = dec(d["amountOutFormatted"]),
                  let origin = q["originAsset"] as? String, let destination = q["destinationAsset"] as? String else { return nil }
            (self.origin, self.destination, amountIn, amountOut) = (origin, destination, i, o)
            (recipient, refundTo, inUSD) = (q["recipient"] as? String ?? "", q["refundTo"] as? String ?? "", dec(d["amountInUsd"]) ?? 0)
        }

        /// The NEAR it moved for `account`: + bought, with the dollars it cost (USDC at its amount, another coin at its
        /// dollar value then); − sold. nil: no NEAR of this account's in it.
        public func near(for account: String) -> (amount: Decimal, dollars: Decimal)? {
            let usdc = origin == Asset.solanaUSDC || ["base", "arbitrum", "ethereum"].contains { Asset.usdc(onEVM: $0) == origin }
            if destination == Asset.near, recipient == account { return (amountOut, usdc ? amountIn : inUSD) }
            if origin == Asset.near, refundTo == account { return (-amountIn, 0) }
            return nil
        }
    }

    public static func settled(depositAddress: String) async throws -> Settled? {
        let (data, _) = try await URLSession.shared.data(for: request(URL(string: "\(base)/status?depositAddress=\(depositAddress)")!))
        return (try JSONSerialization.jsonObject(with: data) as? [String: Any]).flatMap(Settled.init)
    }

    static func post(_ path: String, _ body: [String: Any]) async throws -> Data {
        var r = request(URL(string: base + path)!)
        r.httpMethod = "POST"
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, response) = try await URLSession.shared.data(for: r)
        guard (200..<300).contains((response as? HTTPURLResponse)?.statusCode ?? 0) else { throw OneClickError(data) }
        return data
    }
}

public struct OneClickError: LocalizedError {
    public let message: String
    init(_ data: Data) { message = RPCRetry.said(data) }
    init(message: String) { self.message = message }
    public var errorDescription: String? { "1Click: \(message)" }
}

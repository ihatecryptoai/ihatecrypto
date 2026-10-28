import Foundation
import WalletCore

/// Swaps inside Solana through Jupiter's free API. Jupiter builds the transaction; the wallet only checks
/// that it is the fee payer and signs (`SolanaTransaction.sign`).
public enum Jupiter {
    static let base = "https://lite-api.jup.ag/swap/v1"
    public static let solMint = "So11111111111111111111111111111111111111112"
    public static let usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

    public struct Quote: Sendable {
        /// The quote exactly as Jupiter returned it; /swap needs it back unchanged.
        public let raw: Data
        public let inAmount: UInt64
        public let outAmount: UInt64
        /// Least output after slippage: the floor the user agrees to.
        public let minOut: UInt64
        public let route: String
        /// The route's pools by Jupiter's label, e.g. ["HumidiFi", "Hadron"]: what `excludeDexes` takes.
        public let dexes: [String]
        /// How far the swap moves the price, 0…1 (Jupiter's priceImpactPct): 0.12 for 2M WIF sold at once (2026-10-28).
        public var impact: Decimal = 0
        /// What comes out, in USD by Jupiter's prices (swapUsdValue); 0 when it doesn't say.
        public var usd: Decimal = 0
    }

    /// Jupiter's price impact (its sign dropped: v2 says it negative) and the swap's USD worth, from a quote or an order.
    static func impactAndWorth(_ j: [String: Any]) -> (impact: Decimal, usd: Decimal) {
        func dec(_ v: Any?) -> Decimal { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber)?.decimalValue ?? 0 }
        return (abs(dec(j["priceImpactPct"])), dec(j["swapUsdValue"]))
    }

    /// `excludeDexes`: labels Jupiter must route around (a route that failed before signing).
    public static func quote(input: String, output: String, amount: UInt64, slippageBps: Int = 50, excludeDexes: [String] = []) async throws -> Quote {
        var url = URL(string: "\(base)/quote?inputMint=\(input)&outputMint=\(output)&amount=\(amount)&slippageBps=\(slippageBps)")!
        if !excludeDexes.isEmpty { url.append(queryItems: [URLQueryItem(name: "excludeDexes", value: excludeDexes.joined(separator: ","))]) }
        let ask = url
        // A throttle or a gateway's 5xx is asked again (RPCRetry): one 503 here stopped a chain of moves at its swap.
        return try await RPCRetry.run({ try await URLSession.shared.data(from: ask) }) { data, status in
            guard status == 200,
                  let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let inAmount = (j["inAmount"] as? String).flatMap(UInt64.init),
                  let outAmount = (j["outAmount"] as? String).flatMap(UInt64.init),
                  let minOut = (j["otherAmountThreshold"] as? String).flatMap(UInt64.init) else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            let dexes = (j["routePlan"] as? [[String: Any]] ?? []).compactMap { ($0["swapInfo"] as? [String: Any])?["label"] as? String }
            let (impact, usd) = impactAndWorth(j)
            return Quote(raw: data, inAmount: inAmount, outAmount: outAmount, minOut: minOut, route: dexes.joined(separator: " → "), dexes: dexes,
                         impact: impact, usd: usd)
        }
    }

    /// Unsigned versioned transaction for `quote`, paid by `user`.
    public static func swapTransaction(_ quote: Quote, user: String) async throws -> (wire: Data, lastValidBlockHeight: UInt64) {
        let quoteObject = try JSONSerialization.jsonObject(with: quote.raw)
        var request = URLRequest(url: URL(string: "\(base)/swap")!, timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: [
            "quoteResponse": quoteObject, "userPublicKey": user,
            "wrapAndUnwrapSol": true, "dynamicComputeUnitLimit": true,
            "prioritizationFeeLamports": ["priorityLevelWithMaxLamports": ["maxLamports": 200_000, "priorityLevel": "medium"]],
        ])
        let ask = request
        // Nothing is signed yet, so this is asked again like the quote.
        return try await RPCRetry.run({ try await URLSession.shared.data(for: ask) }) { data, status in
            guard let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let b64 = j["swapTransaction"] as? String, let wire = Data(base64Encoded: b64),
                  let height = (j["lastValidBlockHeight"] as? NSNumber)?.uint64Value else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            return (wire, height)
        }
    }
}

/// Jupiter's gasless swap (Swap API v2, keyless on lite-api): for a wallet with under 0.01 SOL, Jupiter's own wallet pays
/// the fee and the rent and takes them out of the output (`feeBps` grows). Its transaction comes with Jupiter's wallet as
/// the fee payer and ours as the second signer; it goes back through Jupiter's /execute, which adds its signature and sends.
extension Jupiter {
    static let v2 = "https://lite-api.jup.ag/swap/v2"

    public struct Order: Sendable {
        public let requestId: String
        public let wire: Data
        public let inAmount: UInt64
        public let outAmount: UInt64
        /// The least the swap gives after slippage.
        public let minOut: UInt64
        /// Whoever pays the fee: Jupiter's wallet when `gasless`, else the taker.
        public let gasless: Bool
        public let feeBps: Int
        public let route: String
        /// The block its blockhash is good until (0 when Jupiter doesn't say).
        public let lastValidBlockHeight: UInt64
        /// As on a Quote: how far it moves the price, and what comes out in USD.
        public var impact: Decimal = 0
        public var usd: Decimal = 0
    }

    /// The swap for `taker` as Jupiter builds it; nil transaction when it can't (too little, no route): its words.
    /// `excludeRouters`: routers Jupiter must leave out («jupiterz»: its market makers, through a program of their own).
    public static func order(input: String, output: String, amount: UInt64, taker: String, excludeRouters: [String] = []) async throws -> Order {
        let skip = excludeRouters.isEmpty ? "" : "&excludeRouters=" + excludeRouters.joined(separator: ",")
        let url = URL(string: "\(v2)/order?inputMint=\(input)&outputMint=\(output)&amount=\(amount)&taker=\(taker)\(skip)")!
        return try await RPCRetry.run({ try await URLSession.shared.data(from: url) }) { data, status in
            guard status == 200, let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
                throw RPCRetry.Again(status == 429 || status >= 500, JupiterError(data, status))
            }
            guard let requestId = j["requestId"] as? String, let b64 = j["transaction"] as? String, let wire = Data(base64Encoded: b64), !wire.isEmpty,
                  let inAmount = (j["inAmount"] as? String).flatMap(UInt64.init), let outAmount = (j["outAmount"] as? String).flatMap(UInt64.init),
                  let minOut = (j["otherAmountThreshold"] as? String).flatMap(UInt64.init) else {
                throw JupiterError(message: j["errorMessage"] as? String ?? j["error"] as? String ?? "no transaction for this swap")
            }
            let dexes = (j["routePlan"] as? [[String: Any]] ?? []).compactMap { ($0["swapInfo"] as? [String: Any])?["label"] as? String }
            let (impact, usd) = impactAndWorth(j)
            return Order(requestId: requestId, wire: wire, inAmount: inAmount, outAmount: outAmount, minOut: minOut,
                         gasless: j["gasless"] as? Bool ?? false, feeBps: (j["feeBps"] as? NSNumber)?.intValue ?? 0,
                         route: dexes.joined(separator: " → "),
                         lastValidBlockHeight: (j["lastValidBlockHeight"] as? NSNumber)?.uint64Value ?? UInt64(j["lastValidBlockHeight"] as? String ?? "") ?? 0,
                         impact: impact, usd: usd)
        }
    }

    /// Hands our signed transaction to Jupiter, which signs as the fee payer and sends it. The same transaction and id may
    /// be sent again (Jupiter answers for the first one), so a dropped answer is asked again. Returns the signature.
    public static func execute(_ signed: Data, requestId: String) async throws -> String {
        var request = URLRequest(url: URL(string: "\(v2)/execute")!, timeoutInterval: 60)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["signedTransaction": signed.base64EncodedString(), "requestId": requestId])
        let ask = request
        return try await RPCRetry.run({ try await URLSession.shared.data(for: ask) }) { data, status in
            let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            guard status == 200, let j, j["status"] as? String == "Success", let signature = j["signature"] as? String else {
                throw RPCRetry.Again(status == 429 || status >= 500,
                                     JupiterError(message: "\(j?["error"] as? String ?? RPCRetry.said(data, status: status)) (code \(j?["code"] ?? status))"))
            }
            return signature
        }
    }
}

public struct JupiterError: LocalizedError {
    public let message: String
    /// Jupiter's own words when it answers JSON («Each order valued at 30.00 USDC, minimum is 50.00 USDC»), else the
    /// body as text; a gateway's page is only its status (RPCRetry.said).
    init(message: String) { self.message = message }
    init(_ body: Data, _ status: Int = 0) {
        message = ((try? JSONSerialization.jsonObject(with: body)) as? [String: Any])?["error"] as? String ?? RPCRetry.said(body, status: status)
    }
    public var errorDescription: String? { "Jupiter: \(message)" }
}

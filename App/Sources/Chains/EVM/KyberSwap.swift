import Foundation
import WalletCore

/// KyberSwap's aggregator: a route over every exchange on the network, built by its API and paid through its
/// MetaAggregationRouterV2 — one address on every network it serves (its API's routerAddress on Ethereum, Base, Arbitrum,
/// Polygon, Optimism, BNB Chain and HyperEVM, 2026-10-28). On 1 BTC it gave 0.06–0.19% more than Relay's swap on each.
public enum KyberSwap {
    public static let router = "0x6131b5fae19ea4f9d964eac0408e4408b66337b5"
    /// Its API's name for each network ihatecrypto swaps on there.
    public static let chains: [UInt64: String] = [1: "ethereum", 10: "optimism", 56: "bsc", 137: "polygon", 999: "hyperevm",
                                                  8453: "base", 42161: "arbitrum"]
    static let api = "https://aggregator-api.kyberswap.com"

    /// What a swap call does, as the router reads its descriptor (SwapDescriptionV2): the tokens in and out, how much goes
    /// in, the least that must come out, who gets it, who else takes a fee, and whether a permit rides along.
    public struct Swap: Equatable, Sendable {
        public let srcToken: String
        public let dstToken: String
        public let dstReceiver: String
        public let amount: Decimal
        public let minReturn: Decimal
        public let feeReceivers: [String]
        public let permit: Bool
    }

    /// swap(SwapExecutionParams{callTarget, approveTarget, targetData, desc, clientData}) — e21fd0e9, the call its API
    /// builds. nil for anything else, or offsets that aren't 32-byte aligned or run past the data (someone's crafted call).
    public static func decode(_ data: Data) -> Swap? {
        guard data.count >= 4, data.prefix(4).hex == "e21fd0e9" else { return nil }
        let args = Data(data.dropFirst(4))
        func word(_ at: Int) -> Int? {
            guard at >= 0, at % 32 == 0, let n = ABI.uint(in: args, word: at / 32), n < UInt64(args.count), n % 32 == 0 else { return nil }
            return Int(n)
        }
        func address(_ at: Int) -> String? { at % 32 == 0 ? ABI.address(in: args, word: at / 32) : nil }
        guard let params = word(0), let descAt = word(params + 3 * 32) else { return nil }
        let desc = params + descAt
        guard let src = address(desc), let dst = address(desc + 32), let receiver = address(desc + 6 * 32),
              let amount = ABI.decimal(in: args, word: (desc + 7 * 32) / 32), let minReturn = ABI.decimal(in: args, word: (desc + 8 * 32) / 32),
              let fees = RelayMulticallPermit.addresses(Data(args.dropFirst(desc)), at: 4 * 32), let permitAt = word(desc + 10 * 32), let permitLength = ABI.uint(in: args, word: (desc + permitAt) / 32) else { return nil }
        return Swap(srcToken: src, dstToken: dst, dstReceiver: receiver, amount: amount, minReturn: minReturn, feeReceivers: fees, permit: permitLength > 0)
    }

    /// Why the signer won't sign this swap, or nil: it pays someone else, takes a fee for someone, or carries a permit
    /// (ihatecrypto approves the exact amount itself).
    public static func refusal(_ s: Swap, sender: String) -> String? {
        if s.dstReceiver.lowercased() != sender.lowercased() { return "The KyberSwap route would pay \(s.dstReceiver), not this wallet." }
        if let fee = s.feeReceivers.first { return "The KyberSwap route pays a fee to \(fee). ihatecrypto's routes pay no one else." }
        if s.permit { return "The KyberSwap route carries a permit to spend your tokens. ihatecrypto approves the exact amount itself." }
        if s.minReturn <= 0 { return "The KyberSwap route sets no least amount to come out: any price would do. ihatecrypto always sets one." }
        return nil
    }

    /// A route its API found: what comes out, both sides and the gas in its USD, and its summary as given (the build takes
    /// it back unchanged).
    public struct Route: Sendable {
        public let out: Decimal
        public let inUSD: Decimal
        public let outUSD: Decimal
        public let gasUSD: Decimal
        public let gas: UInt64
        let summary: Data
        /// Its price impact by its own valuation, 0…1: what the trade loses against what goes in; nil when it prices either
        /// side at nothing (a token it doesn't know), rather than a made-up 0% or 100%.
        public var impact: Decimal? { inUSD > 0 && outUSD > 0 ? max(0, 1 - outUSD / inUSD) : nil }
    }

    static func request(_ path: String, body: [String: Any]? = nil) -> URLRequest {
        var r = URLRequest(url: URL(string: api + path)!, timeoutInterval: 20)
        r.setValue("ihatecrypto", forHTTPHeaderField: "x-client-id")
        if let body {
            r.httpMethod = "POST"
            r.setValue("application/json", forHTTPHeaderField: "Content-Type")
            r.httpBody = try? JSONSerialization.data(withJSONObject: body)
        }
        return r
    }

    static func number(_ v: Any?) -> Decimal? { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber)?.decimalValue }

    /// The route for `amount` (smallest units) of `from` into `to` on the network.
    public static func route(chainId: UInt64, from: String, to: String, amount: Decimal) async throws -> Route {
        guard let chain = chains[chainId] else { throw WalletError("KyberSwap doesn't serve network \(chainId).") }
        let ask = request("/\(chain)/api/v1/routes?tokenIn=\(from)&tokenOut=\(to)&amountIn=\(NSDecimalNumber(decimal: amount).stringValue)")
        return try await RPCRetry.run({ try await URLSession.shared.data(for: ask) }) { data, status in
            guard status == 200, let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let d = j["data"] as? [String: Any], let s = d["routeSummary"] as? [String: Any],
                  (d["routerAddress"] as? String)?.lowercased() == router,
                  let out = number(s["amountOut"]), out > 0, let summary = try? JSONSerialization.data(withJSONObject: s) else {
                throw RPCRetry.Again(status == 429 || status >= 500, WalletError("KyberSwap: \(RPCRetry.said(data, status: status))"))
            }
            return Route(out: out, inUSD: number(s["amountInUsd"]) ?? 0, outUSD: number(s["amountOutUsd"]) ?? 0,
                         gasUSD: number(s["gasUsd"]) ?? 0, gas: (number(s["gas"]).map { NSDecimalNumber(decimal: $0).uint64Value }) ?? 0, summary: summary)
        }
    }

    /// The router call for `route`, paying `sender`, at most `slippageBps` under its quote. Read back by the caller
    /// (`decode`) before anything is signed.
    public static func build(_ route: Route, chainId: UInt64, sender: String, slippageBps: Int) async throws -> Data {
        guard let chain = chains[chainId], let summary = try? JSONSerialization.jsonObject(with: route.summary) else {
            throw WalletError("KyberSwap doesn't serve network \(chainId).")
        }
        let ask = request("/\(chain)/api/v1/route/build", body: ["routeSummary": summary, "sender": sender, "recipient": sender,
                                                                 "slippageTolerance": slippageBps, "source": "ihatecrypto"])
        // Nothing is signed yet, so this is asked again like the route.
        return try await RPCRetry.run({ try await URLSession.shared.data(for: ask) }) { data, status in
            guard status == 200, let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
                  let d = j["data"] as? [String: Any], (d["routerAddress"] as? String)?.lowercased() == router,
                  let hex = d["data"] as? String, let call = Data(hex: hex) else {
                throw RPCRetry.Again(status == 429 || status >= 500, WalletError("KyberSwap: \(RPCRetry.said(data, status: status))"))
            }
            return call
        }
    }
}

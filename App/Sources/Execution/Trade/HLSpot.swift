import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore

/// Hyperliquid's spot book: HYPE and the other spot tokens bought with, and sold for, USDC on HyperCore. A spot order is
/// an order like a perp's, on asset 10000 + the pair's index (spotMeta), paid from the spot USDC balance — which the perp
/// balance tops up first when short (usdClassTransfer, the user's own signature). IOC within 1% of the mid; the exchange
/// takes no order under 10 USDC.
extension Executor {
    struct SpotPair: Equatable {
        /// 10000 + the pair's index in spotMeta's universe.
        let asset: Int
        /// The pair's key in allMids: «@107», or «PURR/USDC» for the first pairs.
        let key: String
        let symbol: String
        let szDecimals: Int
    }

    static let hlInfo = URL(string: "https://api.hyperliquid.xyz/info")!
    static let spotMinimum: Decimal = 10

    static func hlInfo(_ body: [String: Any]) async throws -> Any {
        var r = URLRequest(url: hlInfo, timeoutInterval: 20)
        r.httpMethod = "POST"
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try JSONSerialization.jsonObject(with: try await URLSession.shared.data(for: r).0, options: .fragmentsAllowed)
    }

    /// The token's pair against USDC (token 0) on HyperCore, from spotMeta: its tokens and its pairs.
    static func spotPair(_ symbol: String) async throws -> SpotPair {
        guard let meta = try await hlInfo(["type": "spotMeta"]) as? [String: Any],
              let tokens = meta["tokens"] as? [[String: Any]], let universe = meta["universe"] as? [[String: Any]] else {
            throw ExecutionError("Hyperliquid's spot list didn't come. Nothing was signed.")
        }
        return try spotPair(symbol, tokens: tokens, universe: universe)
    }

    static func spotPair(_ symbol: String, tokens: [[String: Any]], universe: [[String: Any]]) throws -> SpotPair {
        // «USDT» is USDT0 there, «USDe» USDE.
        let want = (Stablecoins.find(symbol, on: "hyperliquid")?.symbol ?? symbol).uppercased()
        guard let token = tokens.first(where: { ($0["name"] as? String)?.uppercased() == want }),
              let index = (token["index"] as? NSNumber)?.intValue, let szDecimals = (token["szDecimals"] as? NSNumber)?.intValue,
              let pair = universe.first(where: { ($0["tokens"] as? [NSNumber])?.map(\.intValue) == [index, 0] }),
              let pairIndex = (pair["index"] as? NSNumber)?.intValue, let key = pair["name"] as? String else {
            throw ExecutionError("\(want) has no USDC pair on Hyperliquid's spot book.")
        }
        return SpotPair(asset: 10_000 + pairIndex, key: key, symbol: token["name"] as? String ?? want, szDecimals: szDecimals)
    }

    static func spotMid(_ pair: SpotPair) async throws -> Decimal {
        guard let mids = try await hlInfo(["type": "allMids"]) as? [String: String], let mid = mids[pair.key].flatMap({ Decimal(string: $0) }), mid > 0 else {
            throw ExecutionError("Hyperliquid has no price for \(pair.symbol) now. Nothing was signed.")
        }
        return mid
    }

    /// What a spot buy (`usdc`) or sale (`sell`) gets at the book's mid now, fees taken off, for the route's card: «0.243 HYPE».
    // ponytail: the mid, not a walk of the book; the order itself takes at most 1% off it (IOC), a big one may get less.
    public static func spotQuote(_ symbol: String, usdc: Decimal?, sell: Decimal?) async throws -> String? {
        let pair = try await spotPair(symbol), mid = try await spotMid(pair)
        if let usdc { return "\(Fmt.num(usdc / mid * (1 - RiskCalc.spotTakerFee), maxFraction: 6)) \(pair.symbol)" }
        return sell.map { "\(Fmt.num($0 * mid * (1 - spotSellFee), maxFraction: 2)) USDC" }
    }

    /// A spot sell pays the exchange's taker fee and ihatecrypto's; a buy only the exchange's.
    static let spotSellFee = RiskCalc.spotTakerFee + RiskCalc.builderFee

    /// One spot token for another at the books' mids less both orders' fees, for the card: «≈ 12.4 PURR». nil amount = what's free of `from`.
    public static func spotSwapQuote(from: String, to: String, amount: Decimal) async throws -> String {
        let a = try await spotPair(from), b = try await spotPair(to)
        let out = amount * (try await spotMid(a)) * (1 - spotSellFee) / (try await spotMid(b)) * (1 - RiskCalc.spotTakerFee)
        return "\(Fmt.num(out, maxFraction: 6)) \(b.symbol)"
    }

    /// One spot token for another on Hyperliquid: its book trades each against USDC, so `from` is sold and the USDC it
    /// fetched buys `to`, each order IOC within 1% of the mid. nil amount = all that's free of `from`.
    public func hlSpotSwap(from: String, to: String, amount: Decimal?) async throws -> String {
        let before = try await spotFree("USDC")
        let sold = try await hlSpot(buy: false, symbol: from, usdc: nil, amount: amount)
        var got: Decimal = 0
        for _ in 0..<20 {
            got = (try await spotFree("USDC") - before).rounded(2, .down)
            if got > 0 { break }
            try await Task.sleep(for: .milliseconds(500))
        }
        guard got >= Self.spotMinimum else {
            return sold + " The \(Fmt.num(got)) USDC it fetched stays on spot: the book takes orders from 10 USDC, so no \(to.uppercased()) was bought."
        }
        return sold + " " + (try await hlSpot(buy: true, symbol: to, usdc: got, amount: nil))
    }

    /// What's free on the spot balance of `coin` (total less what open orders hold).
    func spotFree(_ coin: String) async throws -> Decimal {
        guard let state = try await Self.hlInfo(["type": "spotClearinghouseState", "user": wallet.evm]) as? [String: Any],
              let balances = state["balances"] as? [[String: Any]] else { throw ExecutionError("Hyperliquid's spot balance didn't come.") }
        guard let row = balances.first(where: { ($0["coin"] as? String)?.uppercased() == coin.uppercased() }) else { return 0 }
        let total = Decimal(string: row["total"] as? String ?? "0") ?? 0, hold = Decimal(string: row["hold"] as? String ?? "0") ?? 0
        return max(0, total - hold)
    }

    /// USDC from the perp balance to the spot one, the user's own signature, then the wait for it to show.
    func usdcToSpot(_ usdc: Decimal) async throws {
        let before = try await spotFree("USDC"), n = nonce()
        let amount = try HLAction.number(usdc)
        let action = MsgPack.map([
            ("type", .string("usdClassTransfer")), ("amount", .string(amount)), ("toPerp", .bool(false)),
            ("nonce", .uint(n)), ("signatureChainId", .string("0x66eee")), ("hyperliquidChain", .string("Mainnet")),
        ])
        let signature = try await signer.signHyperliquidUser(action, recipients: [])
        try journal.append(.init(chain: "hyperliquid", id: "\(n)", kind: .signed, note: "usdClassTransfer perp → spot \(amount)"))
        let reply = HLReply.parse(try await HLExchange.send(action: action, nonce: n, signature: signature, mainnet: true))
        try journal.append(.init(chain: "hyperliquid", id: "\(n)", kind: reply.isError ? .failed : .confirmed, note: "\(reply)"))
        guard !reply.isError else { throw ExecutionError("Hyperliquid turned the move to spot away: \(reply). Nothing was bought.") }
        for _ in 0..<20 {
            if try await spotFree("USDC") >= before + usdc * Decimal(string: "0.999")! { report("Spot balance: +\(Fmt.num(usdc)) USDC from perp."); return }
            try await Task.sleep(for: .milliseconds(500))
        }
        throw ExecutionError("The USDC didn't show on the spot balance within 10 s. Nothing was bought.")
    }

    /// Buys `symbol` on Hyperliquid's spot book for `usdc`, or sells `amount` of it (nil = all that's free) for USDC.
    public func hlSpot(buy: Bool, symbol: String, usdc: Decimal?, amount: Decimal?) async throws -> String {
        let pair = try await Self.spotPair(symbol), mid = try await Self.spotMid(pair)
        try await prepareFee()
        if buy {
            guard let usdc, usdc >= Self.spotMinimum else { throw ExecutionError("Hyperliquid's spot book takes orders from 10 USDC.") }
            let free = try await spotFree("USDC")
            if free < usdc {
                let need = (usdc - free).rounded(2, .up)
                let perp = try await Hyperliquid.balance(user: wallet.evm).withdrawable
                guard perp >= need else {
                    throw ExecutionError("\(Fmt.num(free)) USDC on spot and \(Fmt.num(perp)) free on perp: \(Fmt.num(usdc)) is more. Nothing was signed.")
                }
                try await usdcToSpot(need)
            }
            let px = HLAction.price(mid * Decimal(string: "1.01")!, szDecimals: pair.szDecimals, spot: true)
            let size = HLAction.size(usdc / px, szDecimals: pair.szDecimals)
            guard size > 0 else { throw ExecutionError("\(Fmt.num(usdc)) USDC buys less than one step of \(pair.symbol).") }
            report("Hyperliquid spot: \(Fmt.num(size, maxFraction: pair.szDecimals)) \(pair.symbol) at most \(Fmt.price(px)) (mid \(Fmt.price(mid))), IOC.")
            let reply = try await post(try HLAction.order([HLOrder(asset: pair.asset, isBuy: true, price: px, size: size, reduceOnly: false, kind: .limit(tif: "Ioc"))]),
                                       label: "spot buy \(pair.symbol)")
            guard case .filled(let got, let avg) = reply else { throw ExecutionError("The spot order didn't fill (\(reply)). The USDC stays on your spot balance.") }
            return "Bought \(Fmt.num(got, maxFraction: pair.szDecimals)) \(pair.symbol) on Hyperliquid spot at \(Fmt.price(avg)) — ≈ \(Fmt.usd(got * avg)). It's on your spot balance."
        }
        let free = try await spotFree(pair.symbol)
        let size = HLAction.size(min(amount ?? free, free), szDecimals: pair.szDecimals)
        guard size > 0 else { throw ExecutionError("No \(pair.symbol) on your Hyperliquid spot balance.") }
        guard size * mid >= Self.spotMinimum else { throw ExecutionError("\(Fmt.num(size)) \(pair.symbol) is ≈ \(Fmt.usd(size * mid)): Hyperliquid's spot book takes orders from 10 USDC.") }
        let px = HLAction.price(mid * Decimal(string: "0.99")!, szDecimals: pair.szDecimals, spot: true)
        report("Hyperliquid spot: sell \(Fmt.num(size, maxFraction: pair.szDecimals)) \(pair.symbol) at least \(Fmt.price(px)) (mid \(Fmt.price(mid))), IOC.")
        let reply = try await post(try HLAction.order([HLOrder(asset: pair.asset, isBuy: false, price: px, size: size, reduceOnly: false, kind: .limit(tif: "Ioc"))]),
                                   label: "spot sell \(pair.symbol)")
        guard case .filled(let sold, let avg) = reply else { throw ExecutionError("The spot order didn't fill (\(reply)). The \(pair.symbol) stays on your spot balance.") }
        return "Sold \(Fmt.num(sold, maxFraction: pair.szDecimals)) \(pair.symbol) on Hyperliquid spot at \(Fmt.price(avg)) for ≈ \(Fmt.num(sold * avg)) USDC, on your spot balance."
    }
}

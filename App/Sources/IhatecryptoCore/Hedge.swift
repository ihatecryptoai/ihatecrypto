import Foundation

/// «hedge SOL»: a short on Hyperliquid the size of the coins the wallet holds, so a fall in them is covered. It has no
/// take-profit, and its stop sits far out only to keep the margin from liquidation; the risk rules count it net of the
/// coins it covers.
public enum Hedge {
    /// Leverage a hedge takes when none is named: liquidation ≈ 46% above the entry, half the notional as margin.
    public static let leverage: Decimal = 2
    /// How far a token's own price may be from its market's for it to count as that coin.
    static let priceTolerance = Decimal(string: "0.1")!

    /// Wrapped coins and the market they track one to one.
    static let wrapped = ["WETH": "ETH", "UETH": "ETH", "WBTC": "BTC", "CBBTC": "BTC", "UBTC": "BTC", "BTCB": "BTC", "USOL": "SOL",
                          "WSOL": "SOL", "WHYPE": "HYPE"]

    /// What the wallet holds of each market's coin, from what it holds on every network. `price` nil is a network's own
    /// balance (SOL, ETH), taken as it is. A token counts only when its own price is within a tenth of its market's
    /// `marks` price: an airdropped look-alike, a namesake (MetaDAO's META is not Meta's stock, a GOLD memecoin not gold)
    /// or a token in other units (kPEPE, a share multiplier) would otherwise size a hedge the rules count as riskless.
    /// The market is the coin itself, its wrapped form, or a stock token's HIP-3 market (Robinhood's «TSLA», Ondo's
    /// «NVDAon» → «xyz:TSLA», «xyz:NVDA»).
    public static func spot(_ held: [(symbol: String, amount: Decimal, price: Decimal?)], marks: [String: Decimal]) -> [String: Decimal] {
        // A ticker's market: the main exchange's, else the first HIP-3 one by name.
        var byTicker: [String: String] = [:]
        for name in marks.keys.sorted() {
            let ticker = name.split(separator: ":").last.map(String.init) ?? name
            if byTicker[ticker] == nil || !name.contains(":") { byTicker[ticker] = name }
        }
        return held.reduce(into: [:]) { out, h in
            var ticker = h.symbol.uppercased()
            if let m = h.symbol.wholeMatch(of: /([A-Z]{1,6})on/) { ticker = String(m.1) }
            ticker = wrapped[ticker] ?? ticker
            guard h.amount > 0, let market = byTicker[ticker], let mark = marks[market], mark > 0 else { return }
            if let price = h.price, abs(price - mark) > mark * priceTolerance { return }
            out[market, default: 0] += h.amount
        }
    }

    /// The stop a hedge gets when none is named, in %: four fifths of the way to liquidation, so the exchange closes it
    /// before liquidation takes the margin and its fee.
    public static func guardStop(leverage: Decimal, market: Market) -> Decimal {
        let l = 1 / (2 * Decimal(market.maxLeverage))
        return ((1 / leverage - l) / (1 + l) * 80).rounded(1, .down)
    }
}

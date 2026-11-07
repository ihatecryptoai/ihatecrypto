import Foundation
@testable import IhatecryptoCore
import Testing

@Test(arguments: [
    ("chart ETH", ChartCommand.chart(coin: "ETH", range: .day)),
    ("график нефти за неделю", .chart(coin: "xyz:CL", range: .week)),
    ("покажи эфир", .chart(coin: "ETH", range: .day)),
    ("show me BTC for a month", .chart(coin: "BTC", range: .month)),
    ("how's SOL doing?", .chart(coin: "SOL", range: .day)),
    ("свечи золота", .chart(coin: "xyz:GOLD", range: .day)),
    ("what's moving?", .movers),
    ("что растёт сегодня", .movers),
    ("top gainers", .movers),
] as [(String, ChartCommand)])
func chartPhrases(_ phrase: String, _ expected: ChartCommand) {
    #expect(ChartCommand.parse(phrase, markets: ["ETH", "BTC", "SOL", "xyz:CL", "xyz:GOLD"]) == expected, "«\(phrase)»")
}

@Test(arguments: ["long ETH risk $5", "сколько стоит нефть", "show my addresses", "покажи адреса", "задепозить"])
func notCharts(_ phrase: String) {
    #expect(ChartCommand.parse(phrase, markets: ["ETH", "xyz:CL"]) == nil, "«\(phrase)»")
}

@Test func candlesFromHyperliquidJSON() throws {
    let json = #"[{"t":1790000000000,"T":1790000899999,"s":"ETH","i":"15m","o":"2690.1","c":"2688.0","h":"2695.5","l":"2680.2","v":"12.3","n":40},{"t":1789999100000,"T":1789999999999,"s":"ETH","i":"15m","o":"2685.0","c":"2690.1","h":"2692.0","l":"2684.0","v":"8.1","n":22}]"#
    let candles = try Hyperliquid.parseCandles(Data(json.utf8))
    #expect(candles.count == 2)
    #expect(candles[0].close == Decimal(string: "2690.1")!)   // sorted oldest first
    #expect(candles[1].high == Decimal(string: "2695.5")!)
    #expect(ChartCommand.Range.week.interval == "1h" && ChartCommand.Range.week.span / 3_600_000 == 168)
}

@Test func allocationShares() {
    let s = PortfolioSplit.slices([("Solana", 2), ("Hyperliquid", 10), ("Jupiter", 3), ("Aave", 0), ("Base", -1)])
    #expect(s.map(\.name) == ["Hyperliquid", "Jupiter", "Solana"])
    #expect(s.reduce(0) { $0 + $1.share } == 100)
    #expect(s[0].share == Decimal(string: "66.7")!)
    #expect(PortfolioSplit.slices([]).isEmpty && PortfolioSplit.slices([("x", 0)]).isEmpty)
}

@Test func portfolioNetworkOrder() {
    // The wallet of 2026-10-01: Solana, Hyperliquid at $0 and Robinhood under $100 still lead; Base and Arbitrum follow
    // by money; NEAR and Tempo, under $100, fold; Ethereum with nothing on it is left out.
    let held: [ModuleNetwork: Decimal] = [.base: Decimal(string: "2685.70")!, .solana: Decimal(string: "190.77")!, .arbitrum: Decimal(string: "102.94")!,
                                          .robinhood: Decimal(string: "43.83")!, .near: Decimal(string: "0.38")!, .tempo: Decimal(string: "0.43")!,
                                          .hyperliquid: 0, .ethereum: 0]
    let order = PortfolioSplit.networkCards(held, present: [.solana, .hyperliquid])
    #expect(order.cards == [.solana, .hyperliquid, .robinhood, .base, .arbitrum])
    #expect(order.folded == [.tempo, .near])
    // A pinned network without an account and without money gets no card; exactly $100 folds.
    let empty = PortfolioSplit.networkCards([.robinhood: 0, .polygon: 100], present: [])
    #expect(empty.cards.isEmpty && empty.folded == [.polygon])
}

@Test func marketNames() {
    #expect(IntentParser.pretty("xyz:TSLA") == "Tesla (xyz:TSLA)")
    #expect(IntentParser.pretty("xyz:CL") == "WTI oil (xyz:CL)")
    #expect(IntentParser.pretty("ETH") == "ETH")
}

@Test func favoriteTokenPhrases() {
    let ai = "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18"
    #expect(TokenCommand.parse("и добавь в favorite вот этот токен на робингуде \(ai)") == .favorite(address: ai, chain: "robinhood"))
    #expect(TokenCommand.parse("watch token \(ai) on Base") == .favorite(address: ai, chain: "base"))
    #expect(TokenCommand.parse("add \(ai) to favorites") == .favorite(address: ai, chain: nil))
    #expect(TokenCommand.parse("remove token AI from favorites") == .unfavorite(symbol: "AI"))
    // An address for the book, or a transfer, is not a token.
    #expect(TokenCommand.parse("save \(ai) as Bybit on Base") == nil)
    // «купи 0x… токен на $2» is a trade, not a favorite.
    #expect(TokenCommand.parse("купи \(ai) токен на $2") == nil)
    #expect(WalletCommand.parse("купи \(ai) токен на $2") == .buyStock(symbol: ai, usdc: 2))
    #expect(WalletCommand.parse("sell 3 tokens \(ai)") == .sellStock(symbol: ai, amount: 3))
    #expect(WalletCommand.parse("send 2 USDC to \(ai)") != .buyStock(symbol: ai, usdc: 2))
    #expect(TokenCommand.parse("send 1 USDC to \(ai)") == nil)
}

@Test func dexscreenerPairsToTokenInfo() {
    let json = #"{"pairs":[{"chainId":"robinhood","url":"https://dexscreener.com/robinhood/0xpool1","baseToken":{"address":"0x2E8C31162B855A2FFA90F6F8634643AD6F111E18","name":"Artificial Inu","symbol":"AI"},"priceUsd":"0.2366","priceChange":{"h24":-3.5},"liquidity":{"usd":1369513.5},"info":{"imageUrl":"https://cdn.dexscreener.com/cms/images/x"}},{"chainId":"robinhood","baseToken":{"address":"0x2e8c31162b855a2ffa90f6f8634643ad6f111e18","name":"Artificial Inu","symbol":"AI"},"priceUsd":"0.2352","liquidity":{"usd":3301648.7}},{"chainId":"robinhood","baseToken":{"address":"0xother","name":"X","symbol":"X"},"priceUsd":"9","liquidity":{"usd":9999999}}]}"#
    let infos = TokenLookup.parse(Data(json.utf8), address: "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18")
    #expect(infos.count == 1)
    #expect(infos[0].symbol == "AI" && infos[0].liquidityUSD == Decimal(string: "3301648.7")!)   // the deepest pair wins
    #expect(infos[0].priceUSD == Decimal(string: "0.2352")!)
}

@Test func lookalikeTickersAreSuspicious() {
    let usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", genuine: Set = [usdc.lowercased()]
    #expect(!TokenLookup.suspicious("USDC", contract: usdc, genuine: genuine))
    #expect(TokenLookup.suspicious("USDC", contract: "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", genuine: genuine))
    #expect(TokenLookup.suspicious("USD\u{0421}", contract: usdc, genuine: genuine))   // Cyrillic «С»
    #expect(!TokenLookup.suspicious("AI", contract: "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", genuine: genuine))
}

@Test func pinchKeepsThePointUnderTheFingers() {
    let pan = (x: 40.0, y: -30.0), scale = 0.8, fingers = (x: 300.0, y: 220.0)
    // The content point under the fingers before the pinch…
    let c = (x: (fingers.x - pan.x) / scale, y: (fingers.y - pan.y) / scale)
    for k in [0.5, 1.7, 2.5] {
        let next = CanvasZoom.pan(keeping: fingers, from: pan, by: k)
        // …is still under them after it.
        #expect(abs(next.x + c.x * scale * k - fingers.x) < 0.0001)
        #expect(abs(next.y + c.y * scale * k - fingers.y) < 0.0001)
    }
}

@Test func aTrackpadScrollMovesEvenlyFrameToFrame() {
    // Momentum as recorded on a trackpad: an event every 8.3 ms give or take a millisecond, around the 120 Hz frames'
    // edges. Moved on each event, frames went 0, 100, 0, 50, 100 pt.
    let jitter = [-0.3, 0.8, -0.9, 0.2, 0.9, -0.6, 0.4, -1.0, 0.7, -0.2, 1.0, -0.8], frame = 1.0 / 120
    let events = (1...60).map { (t: 1 + Double($0) * frame + jitter[$0 % jitter.count] / 1000, d: 50.0) }
    var trail = ScrollTrail(), fed = 0, shown = 0.0, steps: [Double] = []
    for f in 1...80 {
        let v = 1 + Double(f) * frame
        // A frame's tick runs a frame before it's shown, knowing the events that came by then.
        while fed < events.count, events[fed].t < v - frame {
            trail.add((events[fed].d, 0), at: events[fed].t)
            fed += 1
        }
        let now = trail.frame(at: v).x
        steps.append(now - shown)
        shown = now
    }
    #expect(steps[12..<56].allSatisfy { (40...60).contains($0) })
    #expect(shown == 3000)
    // After a pause a step plays over a few frames, not at once.
    var one = ScrollTrail()
    one.add((10, 0), at: 5)
    #expect(one.frame(at: 5).x < 1)
    #expect(one.frame(at: 5.1).x == 10)
}

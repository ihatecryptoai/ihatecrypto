import Foundation
import Testing
@testable import IhatecryptoCore

private let markets: Set<String> = ["SOL", "ETH", "BTC", "HYPE", "xyz:TSLA", "xyz:NVDA", "xyz:CL"]
private let sol = Market(name: "SOL", markPx: 100, szDecimals: 2, maxLeverage: 20, prevDayPx: 110, funding: Decimal(string: "0.00001"), atrPct: 2)

private func parse(_ text: String, to intent: TradeIntent = TradeIntent()) -> TradeIntent? {
    IntentParser.apply(text, to: intent, markets: markets)
}

@Test func hedgeFormsParseInEnglishAndRussian() {
    let all = parse("hedge SOL")
    #expect(all?.asset == "SOL" && all?.side == .short && all?.hedgeShare == 1 && all?.hedge == nil)
    #expect(parse("hedge SOL half")?.hedgeShare == Decimal(string: "0.5"))
    #expect(parse("hedge 0.1 SOL")?.hedge == Decimal(string: "0.1") && parse("hedge 0.1 SOL")?.hedgeShare == nil)
    #expect(parse("hedge ETH leverage 3x")?.leverage == 3)
    #expect(parse("захеджируй SOL")?.hedgeShare == 1)
    #expect(parse("захеджируй половину эфира").map { $0.asset == "ETH" && $0.hedgeShare == Decimal(string: "0.5") } == true)
    #expect(parse("хедж 0,1 SOL")?.hedge == Decimal(string: "0.1"))
    #expect(parse("hedge TSLA")?.asset == "xyz:TSLA")
    // A share in percent, never as coins; a third said as a percent by the model.
    #expect(parse("hedge SOL 30%").map { $0.hedgeShare == Decimal(string: "0.3") && $0.hedge == nil } == true)
    #expect(parse("захеджируй 50% эфира").map { $0.asset == "ETH" && $0.hedgeShare == Decimal(string: "0.5") } == true)
    #expect(parse("hedge SOL stop 30%").map { $0.hedgeShare == 1 && $0.stopPct == 30 } == true)
    // No coin named: the model picks one from what the wallet holds. Dollars aren't a hedge's size; «don't hedge» isn't one.
    #expect(parse("хедж мне что-нибудь") == nil && parse("hedge something") == nil)
    #expect(parse("hedge $500 ETH") == nil && parse("hedge ETH 500 USDC") == nil && parse("захеджируй эфир на 500 баксов") == nil)
    #expect(parse("не хеджируй SOL") == nil && parse("сними хедж с SOL") == nil)
    #expect(parse("long SOL, no hedge").map { !$0.isHedge && $0.side == .long && $0.asset == "SOL" } == true)
    #expect(ChatRouter.parses("hedge SOL", markets: markets) && ChatRouter.parses("hedge 0.1 SOL", markets: markets))
}

@Test func aHedgeIsAPlanOfItsOwn() throws {
    // A hedge never keeps the canvas plan's risk, take or side; a long after it is a trade again.
    var long = TradeIntent()
    long.asset = "ETH"; long.riskUSD = 1; long.stopPct = 3; long.takePct = 6
    let hedge = try #require(parse("hedge SOL", to: long))
    #expect(hedge.riskUSD == nil && hedge.takePct == nil && hedge.stopPct == nil && hedge.side == .short)
    // A change alone keeps it a hedge; a take, a risk or money said for one are left out, not listed as changes.
    #expect(parse("leverage 3x", to: hedge).map { $0.isHedge && $0.leverage == 3 } == true)
    #expect(parse("take 5% risk $1 10 USDC", to: hedge) == hedge)
    let trade = try #require(parse("long SOL risk $1", to: hedge))
    #expect(!trade.isHedge && trade.side == .long)
    // Nothing for the rules to fill, and the command reads back to the same plan.
    #expect(hedge.started.autoRules == nil)
    for text in ["hedge SOL", "hedge SOL half", "hedge SOL 30%", "hedge 0.1 SOL leverage 3x"] {
        let i = try #require(parse(text))
        #expect(parse(i.command) == i, "\(text) → \(i.command)")
    }
}

@Test func aHedgeIsSizedByTheCoinsHeld() throws {
    let policy = RiskPolicy()
    let (r, notes) = Plan.resolve(try #require(parse("hedge SOL")), market: sol, equity: 58, policy: policy, spot: Decimal(string: "0.2032"))
    #expect(r.hedge == Decimal(string: "0.2032") && r.leverage == 2 && r.takePct == nil && r.partialTake == nil)
    // Four fifths of the way to liquidation at 2× on a 20× market: (0.5 − 0.025) / 1.025 × 80 = 37.07.
    #expect(r.stopPct == 37 && notes.count == 2)
    let e = Plan.evaluate(r, market: sol)
    let s = try #require(e.sizing)
    #expect(e.problem == nil && s.size == Decimal(string: "0.2") && s.notional == 20 && s.margin == 10 && s.takePrice == nil)
    // The stop is above the entry and below liquidation; what it loses the SOL gains, so only the fees are at risk.
    #expect(s.stopPrice == 137 && s.liquidationPrice > 137 && s.lossAtStopUSD == s.feesUSD)
    let advice = RiskRules.check(r, s, market: sol, equity: 58, policy: policy)
    #expect(!advice.contains { $0.level == .block })
    #expect(!advice.contains { $0.text.contains("take-profit is close") || $0.text.contains("Liquidation") || $0.text.contains("24h") })

    // Half; more than is held is cut to what is held; nothing held, or not read yet.
    #expect(Plan.resolve(try #require(parse("hedge SOL half")), market: sol, equity: 58, policy: policy, spot: Decimal(string: "0.3")).intent.hedge
            == Decimal(string: "0.15"))
    let cut = Plan.resolve(try #require(parse("hedge 5 SOL")), market: sol, equity: 58, policy: policy, spot: Decimal(string: "0.2"))
    #expect(cut.intent.hedge == Decimal(string: "0.2") && cut.notes.first?.contains("covers that") == true)
    let none = Plan.resolve(try #require(parse("hedge SOL")), market: sol, equity: 58, policy: policy, spot: 0).intent
    #expect(Plan.evaluate(none, market: sol).problem?.contains("holds no") == true)
    let unread = Plan.resolve(try #require(parse("hedge SOL")), market: sol, equity: 58, policy: policy).intent
    #expect(Plan.evaluate(unread, market: sol).missing == ["what the wallet holds of SOL"])
    // Under Hyperliquid's $10 minimum.
    let small = Plan.resolve(try #require(parse("hedge SOL")), market: sol, equity: 58, policy: policy, spot: Decimal(string: "0.05")).intent
    #expect(Plan.evaluate(small, market: sol).problem?.contains("$10 minimum") == true)
}

@Test func spotHeldMapsToItsMarket() {
    let marks: [String: Decimal] = ["SOL": 100, "ETH": 2500, "BTC": 80000, "xyz:TSLA": 400, "xyz:NVDA": 180, "xyz:META": 700, "xyz:GOLD": 4000]
    let held: [(symbol: String, amount: Decimal, price: Decimal?)] = [
        ("SOL", 1, nil), ("ETH", 2, nil), ("WETH", 1, 2490), ("cbBTC", Decimal(string: "0.1")!, 79500), ("TSLA", 3, 401), ("NVDAon", 2, 181),
        ("BONK", 1000, Decimal(string: "0.00002")!), ("USDC", 50, 1), ("SOL", 0, nil),
        // An airdropped «WETH» with no real price, MetaDAO's META, a GOLD memecoin: none of them is that market's coin.
        ("WETH", 50, 0), ("META", 10, Decimal(string: "0.3")!), ("GOLD", 5000, Decimal(string: "0.01")!),
    ]
    let spot = Hedge.spot(held, marks: marks)
    #expect(spot == ["SOL": 1, "ETH": 3, "BTC": Decimal(string: "0.1")!, "xyz:TSLA": 3, "xyz:NVDA": 2])
    #expect(Hedge.guardStop(leverage: 2, market: sol) == 37)
    #expect(Hedge.guardStop(leverage: 1, market: sol) == 76)   // 0.975 / 1.025 × 80
}

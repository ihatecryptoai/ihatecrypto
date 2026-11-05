import Foundation
import Testing
@testable import IhatecryptoCore

private let markets: Set<String> = ["ETH", "BTC", "SOL"]
private let eth = Market(name: "ETH", markPx: 3000, szDecimals: 4, maxLeverage: 25, index: 1)

private func parse(_ text: String, to intent: TradeIntent = TradeIntent()) -> TradeIntent? {
    IntentParser.apply(text, to: intent, markets: markets)
}

/// «Купи ETH на $2000 за 2 часа» is rewritten by the model as the TWAP form; the parser reads it, and the Russian words, the
/// same way. «in 2 hours» / «через 2 часа» say when, not how long: never a TWAP.
@Test func twapFormsParseInEnglishAndRussian() throws {
    let en = try #require(parse("long ETH 2000 USDC over 2 h leverage 1x"))
    #expect(en.side == .long && en.asset == "ETH" && en.amountUSDC == 2000 && en.leverage == 1 && en.twapMinutes == 120 && en.twapRandomize == nil)
    let ru = try #require(parse("лонг ETH 2000 USDC за 2 часа плечо 1"))
    #expect(ru.twapMinutes == 120 && ru.amountUSDC == 2000 && ru.leverage == 1)
    #expect(parse("long BTC 500 USDC over 30 min randomize leverage 1x").map { $0.twapMinutes == 30 && $0.twapRandomize == true } == true)
    #expect(parse("лонг BTC 500 USDC за полчаса рандомно").map { $0.twapMinutes == 30 && $0.twapRandomize == true } == true)
    #expect(parse("шорт SOL 300 USDC в течение 90 минут")?.twapMinutes == 90)
    #expect(parse("long ETH 200 USDC twap 1 d")?.twapMinutes == 1440)
    #expect(parse("лонг ETH 200 USDC за час")?.twapMinutes == 60)
    #expect(parse("long ETH 200 USDC over 1,5 h")?.twapMinutes == 90)
    for when in ["long ETH 200 USDC in 2 hours", "лонг ETH 200 USDC через 2 часа"] { #expect(parse(when)?.twapMinutes == nil, "\(when)") }
    // «random» is no change without a TWAP; on a TWAP plan «randomize» and «one order» change it.
    #expect(parse("long a random coin")?.twapRandomize == nil)
    #expect(parse("randomize", to: en)?.twapRandomize == true)
    #expect(parse("одним ордером", to: en).map { $0.twapMinutes == nil && $0.amountUSDC == 2000 } == true)
    #expect(parse("over 3 h", to: en)?.twapMinutes == 180)
    // The command reads back to the same plan, and the router takes it.
    for text in ["long ETH 2000 USDC over 2 h leverage 1x", "long BTC 500 USDC over 30 min randomize leverage 1x"] {
        let i = try #require(parse(text))
        #expect(parse(i.command) == i, "\(text) → \(i.command)")
        #expect(ChatRouter.parses(text, markets: markets), "\(text)")
    }
    #expect(ChatRouter.forms.contains("long|short <ASSET> N USDC over N min|h [randomize]"))
    #expect(ChatRouter.forms.contains("= long ETH 2000 USDC over 2 h leverage 1x"))
    // Stopping it: by its id, found from the words.
    for text in ["cancel the TWAP", "stop the twap", "отмени TWAP", "останови твап"] { #expect(PositionCommand.parse(text) == .cancelOrders, "\(text)") }
}

/// The same rules as one order, plus Hyperliquid's own: $100 at least, 5 minutes to 7 days, nothing but the entry.
@Test func aTwapKeepsTheRulesOfOneOrder() throws {
    var i = try #require(parse("long ETH 300 USDC over 2 h stop 2% risk $6 leverage 1x"))
    var e = Plan.evaluate(i, market: eth)
    #expect(e.problem == nil && e.sizing.map { $0.notional >= 100 } == true)
    var live = Live()
    live.market = eth
    let lines = Plan.position(i, e, live).lines
    #expect(lines.first.map { $0.hasPrefix("TWAP: 0.") && $0.contains(" ETH over 2 h, parts at the market every 30 s or more") && $0.hasSuffix("equal sizes.") } == true, "\(lines)")
    #expect(lines.contains { $0.contains("ihatecrypto's 0.05% isn't on a TWAP") })

    i.riskUSD = 1   // $50 of ETH: under the TWAP's $100
    e = Plan.evaluate(i, market: eth)
    #expect(e.problem?.hasPrefix("A TWAP on Hyperliquid is from $100") == true)

    i.riskUSD = 6; i.twapMinutes = 3
    #expect(Plan.evaluate(i, market: eth).problem == "A TWAP on Hyperliquid runs from 5 minutes to 7 days.")
    i.twapMinutes = 120; i.trailingPct = 1
    #expect(Plan.evaluate(i, market: eth).problem?.hasPrefix("A TWAP is the whole entry") == true)
}

@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing
import TradeExecution

/// The guardian's decisions and the position's line, as the Mac's (ExitRules.duties, positionSummary), and Hyperliquid's
/// card note with HLP (PortfolioNetworks.swift).
private func en(_ english: String, _: String) -> String { english }

private func orders(_ json: String) throws -> [HyperliquidAccount.Order] {
    try JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(json.utf8))
}

@Test func aPlansFillLeavesTheGuardianItsRules() {
    var intent = TradeIntent()
    intent.side = .long
    // The exchange holds the stop and the take: nothing for the guardian.
    #expect(Engine.exitRules(size: 1, entry: 100, pending: false, partialTake: nil, intent: intent) == nil)
    // A partial take: the stop goes to breakeven after it.
    let partial = Engine.exitRules(size: 1, entry: 100, pending: false, partialTake: 110, intent: intent)
    #expect(partial?.breakevenAfterPartial == true && partial?.partialOrderPending == false && partial?.trailingPct == nil)
    // A limit entry with a take in parts: the half at 1R waits for the fill.
    intent.partialTake = true
    #expect(Engine.exitRules(size: 1, entry: 100, pending: true, partialTake: nil, intent: intent)?.partialOrderPending == true)
    // A trailing stop alone.
    intent.partialTake = nil
    intent.trailingPct = 2
    let trailing = Engine.exitRules(size: 1, entry: 100, pending: false, partialTake: nil, intent: intent)
    #expect(trailing?.trailingPct == 2 && trailing?.breakevenAfterPartial == false && trailing?.long == true)
}

@Test func thePositionSaysItsStopTakesAndGuardian() throws {
    let book = try orders("""
    [{"coin":"ETH","oid":1,"isTrigger":true,"triggerPx":"2300","orderType":"Stop Market","reduceOnly":true},
     {"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"2600","orderType":"Take Profit Market","reduceOnly":true},
     {"coin":"ETH","oid":3,"isTrigger":true,"triggerPx":"2500","orderType":"Take Profit Market","reduceOnly":true},
     {"coin":"BTC","oid":4,"isTrigger":true,"triggerPx":"90000","orderType":"Stop Market","reduceOnly":true}]
    """)
    let line = Engine.summary(coin: "ETH", size: Decimal(string: "0.05")!, entry: 2412.5, orders: book, say: en)
    #expect(line.hasPrefix("0.05 at ") && line.contains("· stop ") && line.contains("2,300"))
    // The nearest take first for a long.
    #expect(line.range(of: "2,500")!.lowerBound < line.range(of: "2,600")!.lowerBound)
    #expect(Engine.summary(coin: "SOL", size: -3, entry: 150, orders: book, say: en).hasSuffix("stop none"))

    var rules = ExitRules(openedSize: 1, entry: 100, long: true, breakevenAfterPartial: true, trailingPct: 2)
    let said = Engine.duties(rules, say: en)
    #expect(said.count == 2 && said[0].hasPrefix("After the first half, moves the stop to breakeven") && said[1].hasPrefix("Trailing 2%"))
    rules.movedToBreakeven = true
    #expect(Engine.duties(rules, say: en)[0].hasPrefix("Stop is already at breakeven"))
    #expect(Engine.duties(ExitRules(openedSize: 1, entry: 1, long: true, breakevenAfterPartial: false, trailingPct: nil), say: en)
        == ["Stop and take-profit sit on the exchange; ihatecrypto doesn't move anything."])
}

@Test func hyperliquidsNoteCountsHLP() {
    var perps = PortfolioView.Row(symbol: "Perps", amount: "10", value: 10, usd: 10)
    perps.section = "tokens"
    var hlp = PortfolioView.Row(symbol: "HLP", amount: "5", value: 5, usd: 5)
    hlp.section = "defi"
    let n = PortfolioView.Network(key: "hyperliquid", name: "Hyperliquid", usd: 15, rows: [perps, hlp])
    #expect(Engine.note(n) == "$10.00 · margin · HLP $5.00")
    #expect(Engine.note(PortfolioView.Network(key: "hyperliquid", name: "Hyperliquid", usd: 10, rows: [perps])) == "$10.00 · margin")
}

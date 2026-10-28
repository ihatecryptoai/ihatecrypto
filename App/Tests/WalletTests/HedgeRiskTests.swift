import Foundation
import Testing
@testable import TradeExecution

@Test func aShortOfCoinsHeldAddsNoRiskAtItsStop() throws {
    let orders = try JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(#"""
    [{"coin":"SOL","oid":1,"isTrigger":true,"triggerPx":"137","orderType":"Stop Market"}]
    """#.utf8))
    let short = HyperliquidAccount.Position(coin: "SOL", size: Decimal(string: "-0.2")!, entry: 100, unrealizedPnl: 0, liquidation: nil)
    // Alone it loses (137 − 100) × 0.2 at the stop; with the SOL it covers held, nothing; half held, half.
    #expect(HyperliquidAccount.riskAtStops(positions: [short], orders: orders) == Decimal(string: "7.4"))
    #expect(HyperliquidAccount.riskAtStops(positions: [short], orders: orders, spot: ["SOL": Decimal(string: "0.25")!]) == 0)
    #expect(HyperliquidAccount.riskAtStops(positions: [short], orders: orders, spot: ["SOL": Decimal(string: "0.1")!]) == Decimal(string: "3.7"))
    // A long is never covered by coins held: they fall with it.
    let long = HyperliquidAccount.Position(coin: "SOL", size: Decimal(string: "0.2")!, entry: 100, unrealizedPnl: 0, liquidation: nil)
    let below = try JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(#"""
    [{"coin":"SOL","oid":1,"isTrigger":true,"triggerPx":"95","orderType":"Stop Market"}]
    """#.utf8))
    #expect(HyperliquidAccount.riskAtStops(positions: [long], orders: below, spot: ["SOL": 1]) == 1)
}

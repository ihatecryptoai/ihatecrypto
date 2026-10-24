import CWinSec
@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// The «plan» part of wave 4: a price alert tied to the plan, Jupiter's other platforms in the portfolio's total, the
/// token card's held P/L said as the Mac's TokenBoard says it.
@Suite(.serialized) struct Plan5Tests {
    private static func planBoard(_ state: [String: Any]?) -> [String: Any]? {
        (state?["boards"] as? [[String: Any]] ?? []).first { $0["kind"] as? String == "move" && $0["state"] as? String == "plan" }
    }
    private static func firstStep(_ state: [String: Any]?) -> [String: Any]? { (planBoard(state)?["steps"] as? [[String: Any]])?.first }
    private static func alerts(_ state: [String: Any]?) -> [[String: Any]] { state?["alerts"] as? [[String: Any]] ?? [] }

    /// «When ETH is below 100: long ETH …» while that plan is on the canvas: the alert is the plan's first step, not a row
    /// of the alerts block; an edit redraws the plan with it still first; a new level replaces the old; «clear the plan»
    /// takes it away with the plan.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aPlanAlertIsThePlansFirstStepAndGoesWithThePlan() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        let planned = try engine.call("send", ["text": "long ETH risk $5 stop 3%"])
        // Hyperliquid unreachable even after its retries: no plan, and no price to watch.
        guard Self.planBoard(planned.state) != nil else { return }

        let tied = try engine.call("send", ["text": "when ETH is below 100: long ETH risk $5 stop 3%"])
        let said = EngineTests.chat(tied.state).last ?? ""
        guard !said.contains("no market") else { return }
        #expect(said.hasPrefix("🔔 Added to the plan as its first step: when ETH is below 100 (now "), "\(said)")
        #expect(said.hasSuffix("this plan comes back to you to confirm, with any changes you make to it until then."), "\(said)")
        let first = Self.firstStep(tied.state)
        #expect(first?["label"] as? String == "WHEN · PRICE FALLS TO" && first?["title"] as? String == "ETH 100"
                && first?["status"] as? String == "watching", "\(first ?? [:])")
        #expect((first?["lines"] as? [String])?.last == "then this plan comes to you to confirm")
        #expect(Self.alerts(tied.state).isEmpty, "the plan's alert is drawn on the plan, not in the alerts block")

        // An edit redraws the plan, the alert still its first step.
        let edited = try engine.call("send", ["text": "stop 4%"])
        #expect(Self.firstStep(edited.state)?["label"] as? String == "WHEN · PRICE FALLS TO")

        // A new level for the same plan replaces the old one: one alert per plan.
        let moved = try engine.call("send", ["text": "when ETH is below 90: long ETH risk $5 stop 4%"])
        #expect(Self.firstStep(moved.state)?["title"] as? String == "ETH 90")
        let steps = Self.planBoard(moved.state)?["steps"] as? [[String: Any]] ?? []
        #expect(steps.filter { $0["status"] as? String == "watching" }.count == 1)

        // «clear the plan» takes the alert with it: none is left to show in the alerts block once the plan is gone.
        let cleared = try engine.call("send", ["text": "clear the plan"])
        #expect(EngineTests.chat(cleared.state).last == "The plan is cleared, and its price alert with it. Tell me what to do next.")
        #expect(Self.alerts(cleared.state).isEmpty && Self.planBoard(cleared.state) == nil)
    }

    /// Jupiter's portfolio: Earn is left to Jupiter Lend's own row; staked JUP, perps and predictions are DeFi rows, and
    /// the Solana card's total counts them.
    @Test func jupiterPlatformsCountInTheTotal() {
        let rows = Engine.jupiterRows([
            ("jupiter-exchange-earn", "jupiter-exchange", "Earn", nil, 50, 0.05),
            ("jupiter-governance", "jupiter-governance", "Staked JUP", nil, 12.346, 0.0731),
            ("jupiter-perps", "jupiter-perps", "Perps", "SOL long", 7.5, nil),
            ("jupiter-prediction", "jupiter-prediction", nil, nil, 3, 0),
        ])
        #expect(rows.map(\.symbol) == ["Jupiter", "Jupiter", "Jupiter"])
        #expect(rows.map(\.note) == ["Staked JUP · 7.31%", "Perps · SOL long", "jupiter-prediction"])
        #expect(rows.allSatisfy { $0.section == "defi" && $0.icon == "logos/jup.png" })
        #expect(rows.map(\.usd) == [Decimal(string: "12.35"), Decimal(string: "7.5"), 3])

        let usdc = PortfolioView.Row(symbol: "USDC", amount: "10", value: 10, usd: 10)
        let card = Engine.tucked(PortfolioView.Network(key: "solana", name: "Solana", usd: 0, rows: [usdc] + rows), choices: [:])
        #expect(card.usd == Decimal(string: "32.85"))
    }

    /// The token card's line under its P/L: what was bought, what came in unpaid at the price then, what it's worth now,
    /// and since when.
    @Test func theHeldPnLIsSaidAsTheMacSaysIt() {
        var cost = CostBasis()
        cost.bought(10, for: 20, at: Date(timeIntervalSince1970: 1_791_633_600))   // 2026-10-10, noon UTC
        cost.came(2, worth: 5)
        let told = Engine.heldPaid(symbol: "NEAR", amount: 12, gain: 3, cost: cost, ru: false)
        #expect(told.paid == "Bought 10 NEAR for $20.00 at $2.00 · 2 more came in from elsewhere, counted at the price then: $2.50 · worth $28.00 now")
        #expect(told.since == "since Oct 10")
        // More held than the cost knows: the rest is said to be uncounted.
        var bought = CostBasis()
        bought.bought(4, for: 8, at: Date(timeIntervalSince1970: 1_791_633_600))
        let more = Engine.heldPaid(symbol: "SOL", amount: 5, gain: 1, cost: bought, ru: false)
        #expect(more.paid == "Bought 4 SOL for $8.00 at $2.00 · worth $9.00 now · 1 more not counted: no price for it")
        #expect(more.since == "since you bought on Oct 10")
        #expect(Engine.heldPaid(symbol: "SOL", amount: 5, gain: nil, cost: nil, ru: false).paid == "No price yet to count its P/L from")
        #expect(Engine.tokenPrice(Decimal(string: "0.000123")!) == "$0.000123")
    }
}

import CWinSec
@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// A trade plan as the Mac draws it (TradePlan.swift): its whole route whether the rules stop it or not, what stops it,
/// the checklist in the chat, the take-profit ways and the market picker; and «stop 2%» after a trade moves only that
/// position's orders, never past the per-trade limit.
@Suite(.serialized) struct TradePlanTests {
    // MARK: After a trade

    private func plan(_ change: (inout TradeIntent) -> Void = { _ in }) -> TradeIntent {
        var i = TradeIntent()
        i.riskUSD = 5; i.stopPct = 3; i.takePct = 6
        change(&i)
        return i
    }

    /// «stop 2%» on a long from 100: the stop goes to 98 and nothing else moves; «take 10%» moves only the take.
    @Test func onlyWhatTheWordsChangedMoves() {
        let before = plan()
        let stop = plan { $0.stopPct = 2 }
        let moved = Engine.orderEdit(before: before, asked: stop, resolved: stop, size: 1, entry: 100, equity: 1000, maxRiskPct: 2)
        #expect(moved.moves == [.stopTo(98)] && moved.tooWide == nil)
        let take = plan { $0.takePct = 10 }
        #expect(Engine.orderEdit(before: before, asked: take, resolved: take, size: 1, entry: 100, equity: 1000, maxRiskPct: 2).moves == [.takeTo(110)])
        // A short's stop is above its entry, its take below.
        let both = plan { $0.stopPct = 2; $0.takePct = 4 }
        #expect(Engine.orderEdit(before: before, asked: both, resolved: both, size: -1, entry: 100, equity: 1000, maxRiskPct: 2).moves == [.stopTo(102), .takeTo(96)])
        #expect(Engine.orderEdit(before: before, asked: before, resolved: before, size: 1, entry: 100, equity: 1000, maxRiskPct: 2).moves.isEmpty)
    }

    /// A wider stop that would lose more than the per-trade limit moves nothing and says what it would lose.
    @Test func aStopPastTheLimitIsRefused() {
        let before = plan(), wide = plan { $0.stopPct = 10 }
        // 1 coin from 100 to 90: $10 plus fees, over 2% of $200.
        let refused = Engine.orderEdit(before: before, asked: wide, resolved: wide, size: 1, entry: 100, equity: 200, maxRiskPct: 2)
        #expect(refused.moves.isEmpty)
        #expect((refused.tooWide ?? 0) > 10 && (refused.tooWide ?? 0) < 10.2)
        // With capital unknown there's nothing to measure against: it moves.
        #expect(Engine.orderEdit(before: before, asked: wide, resolved: wide, size: 1, entry: 100, equity: nil, maxRiskPct: 2).moves == [.stopTo(90)])
    }

    /// «Plan updated: …» in Russian: the core's English fragments said from meaning, a number kept as it is.
    @Test func thePlansChangesInRussian() {
        var next = plan()
        next.side = .short; next.pullbackPct = 2; next.autoRules = true
        let ru = next.changes(from: TradeIntent()).map(Engine.changeRu)
        #expect(ru == ["шорт", "риск $5.00", "стоп 3%", "тейк 6%", "остальное по правилам риска", "вход на откате 2%"], "\(ru)")
    }

    // MARK: The plan on the canvas

    private func walletEngine() throws -> EngineTests.Engine {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        return engine
    }

    private static func planBoard(_ state: [String: Any]?) -> [String: Any]? {
        (state?["boards"] as? [[String: Any]] ?? []).first { $0["kind"] as? String == "move" && $0["state"] as? String == "plan" }
    }

    /// An empty wallet asked for a trade: the whole route is on the canvas with the money step stopped, no card waits,
    /// and the chat checks the plan item by item.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aPlanTheRulesStopIsDrawnWithItsRoute() throws {
        let engine = try walletEngine()
        try engine.call("refresh")
        let r = try engine.call("send", ["text": "long ETH risk $5 stop 3%"])
        let said = EngineTests.chat(r.state).last ?? ""
        // Hyperliquid unreachable even after its retries (many readers at once): the plan says the price didn't load.
        guard !said.contains("has no price yet") else { return }
        let board = try #require(Self.planBoard(r.state), "\(said)")
        let steps = board["steps"] as? [[String: Any]] ?? []
        let labels = steps.compactMap { $0["label"] as? String }
        #expect(labels.first == "FROM · SOLANA" && labels.contains("CHECK RULES") && labels.contains("OPEN LONG · ETH")
                && labels.contains("PROTECT") && labels.last == "RESULT · ESTIMATE", "\(labels)")
        let plan = try #require(board["plan"] as? [String: Any])
        #expect(plan["blocked"] as? Bool == true && plan["market"] as? String == "ETH")
        #expect(r.state?["card"] == nil || r.state?["card"] is NSNull)
        #expect(said.contains("✓ Market: ETH") && said.contains("✓ Stop: 3%"), "\(said)")
        // Solana's balance read: the money step stops it. Not read (its node refusing many readers at once): the
        // preparation does, and says so in the plan's place.
        if plan["short"] as? String == "not enough USDC" {
            #expect(steps.first?["status"] as? String == "blocked")
            #expect((plan["stopped"] as? String)?.hasPrefix("Short by") == true && said.contains("✗ Money: Short by"), "\(said)")
        } else {
            #expect(plan["short"] as? String == "not prepared" && said.contains("Can't open it: "), "\(said)")
        }
    }

    /// «Take profit like the pros» puts the three ways on the plan; one picked goes into it. The picker's markets are
    /// read without a new state; one picked re-plans there; Cancel takes the plan off the canvas.
    @Test(.enabled(if: winsec_is_stub() == 1)) func takeWaysAndTheMarketPicker() throws {
        let engine = try walletEngine()
        try engine.call("send", ["text": "long ETH risk $5 stop 3%"])
        let asked = try engine.call("send", ["text": "take profit like the pros"])
        let takes = (Self.planBoard(asked.state)?["plan"] as? [String: Any])?["takes"] as? [[String: Any]] ?? []
        #expect(takes.compactMap { $0["id"] as? String } == ["classic", "partial", "trend"])
        #expect(takes.first { $0["recommended"] as? Bool == true }?["id"] as? String == "partial")

        let picked = try engine.call("planTake", ["name": "partial"])
        #expect(EngineTests.chat(picked.state).last?.hasPrefix("Take: in parts.") == true, "\(EngineTests.chat(picked.state).last ?? "")")
        #expect((Self.planBoard(picked.state)?["plan"] as? [String: Any])?["takes"] == nil)

        let list = try engine.call("planMarkets")
        let json = try #require((list.answer["result"] as? [String: Any])?["json"] as? String)
        let groups = try #require(try JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: [String]])
        // It only reads, so it sends no state (Engine.answersOnly); the engine's own minute refresh may, so the set is checked.
        #expect(DesktopEngine.Engine.answersOnly.contains("planMarkets"))
        // Hyperliquid unreachable even after its retries: no markets to pick from.
        guard groups["Crypto"]?.isEmpty == false else { return }
        #expect(groups["Crypto"]?.contains("BTC") == true && groups["Crypto"]?.contains("ETH") == true)

        let btc = try engine.call("planMarket", ["name": "BTC"])
        #expect(EngineTests.chat(btc.state).last?.hasPrefix("Market: BTC · Hyperliquid — crypto.") == true)
        #expect((Self.planBoard(btc.state)?["plan"] as? [String: Any])?["market"] as? String == "BTC")

        let cancelled = try engine.call("planCancel")
        #expect(Self.planBoard(cancelled.state) == nil)
        #expect(EngineTests.chat(cancelled.state).last?.contains("The plan is cleared") == true)
    }
}

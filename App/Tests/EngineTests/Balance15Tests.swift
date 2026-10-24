@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Wave 14's balance part: the dimmed balance is the last whole one, a first read waits for Solana and Hyperliquid, and
/// a canvas keeps its board's name in the Mac's order. In-process, state set by hand; no addresses.
@MainActor @Suite(.serialized) struct Balance15Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("balance15-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    @Test func theLastWholeBalanceIsKeptPerWallet() {
        Engine.rememberBalance(120, wallet: "balance15-a")
        Engine.rememberBalance(7, wallet: "balance15-b")
        let kept = Store.load([String: Decimal].self, "balances.json")
        #expect(kept?["balance15-a"] == 120)
        #expect(kept?["balance15-b"] == 7)
    }

    @Test func aCanvasIsNamedInTheMacsOrder() {
        let chart = Board(kind: "chart", title: "BTC · 1 day")
        var rates = Board(kind: "result", title: "Rates · USDC")
        rates.data = BlockData(rates: [.init(place: "Kamino", asset: "USDC", apy: 5, how: nil, note: "")])
        var plan = Board(kind: "move", title: "Plan · long ETH")
        plan.plan = PlanView(market: "ETH", group: "", footer: "")
        #expect(Engine.boardTitle([chart]) == "Chart · BTC · 1 day")
        #expect(Engine.boardTitle([chart, rates]) == "Rates · USDC")
        #expect(Engine.boardTitle([chart, plan]) == "Plan · long ETH")
        plan.plan?.takes = [TakeChoice(id: "pros", title: "Like the pros", detail: "", note: "", payoff: "+$2.00", recommended: true)]
        #expect(Engine.boardTitle([plan]) == "Take-profit choices")
        #expect(Engine.boardTitle([plan, Board(kind: "move", title: "Send 5 USDC to Mom")]) == "Send 5 USDC to Mom")
    }

    @Test func closingTheChartKeepsTheCanvasName() throws {
        try engine.newCanvas()
        let id = engine.canvasID
        engine.boards = [Board(kind: "chart", title: "BTC · 1 day")]
        engine.boards = []
        #expect(engine.canvasRows.first { $0.id == id }?.title == "Chart · BTC · 1 day")
    }
}

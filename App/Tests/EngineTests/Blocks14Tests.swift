@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing
import WalletCore

/// Wave 13's blocks part: a canvas is named after its chart or its rates when no move names it; a position closed outside
/// ihatecrypto and a limit that filled are runs of the position's item, without joining its story; a recurring buy's run
/// opens to what it said; a switch to a wallet that's gone says so in the chat. In-process, state set by hand; public
/// test data only.
@MainActor @Suite(.serialized) struct Blocks14Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("blocks14-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    private func fresh() throws {
        engine.settings.language = "en"
        engine.tasks = []
        engine.scheduleRuns = []
        try engine.newCanvas()
    }

    @Test func aCanvasIsNamedAfterItsChartOrRatesWhenNoMoveNamesIt() {
        let chart = Board(kind: "chart", title: "BTC · 1D", chart: ChartLine(title: "BTC · 1D", closes: [1, 2], low: "1", high: "2", change: "+1%", coin: "BTC"))
        var rates = Board(kind: "result", title: "Rates · USDC", lines: ["Kamino (USDC) · 5%"])
        rates.data = BlockData(rates: [.init(place: "Kamino", asset: "USDC", apy: 5, how: nil, note: "")])
        let pnl = Board(kind: "result", title: "P/L · 7 days", lines: [])
        let move = Board(kind: "move", title: "Send 1 USDC to Mom")
        let doc = CanvasDoc(title: "what's bitcoin doing")
        #expect(Engine.shownTitle(doc, boards: [chart]) == "Chart · BTC")
        #expect(Engine.shownTitle(doc, boards: [rates]) == "Rates · USDC")
        // A result that isn't the rates (P/L, approvals) leaves the first words, as the Mac's canvasBubbles.
        #expect(Engine.shownTitle(doc, boards: [pnl]) == "what's bitcoin doing")
        // A move names it over a chart drawn after it, and the person's own name over everything.
        #expect(Engine.shownTitle(doc, boards: [chart, move]) == "Send 1 USDC to Mom")
        var named = doc
        named.name = "My BTC"
        #expect(Engine.shownTitle(named, boards: [chart, move]) == "My BTC")
    }

    @Test func closedOutsideAndLimitFilledAreRunsOfThePositionNotItsStory() throws {
        try fresh()
        engine.notify(.positions, "Blocks14", "Blocks14 position closed outside ihatecrypto at 100: profit $1.00.", schedule: "perp:BLOCKS14")
        let kept = try #require(HistoryStore.saved.automatic.last)
        #expect(kept.schedule == "perp:BLOCKS14")
        #expect(kept.coin == nil, "a coin would put it in the position's story as the guardian's step")
        let run = try #require(engine.recentRuns.first { $0.task == "perp:BLOCKS14" })
        #expect(run.line?.contains("closed outside") == true)
        // The guardian's own records still come by their coin.
        engine.recordAutomatic("Partial profit taken on BLOCKS14B", "Stop moved to breakeven.", coin: "BLOCKS14B")
        #expect(engine.recentRuns.contains { $0.task == "perp:BLOCKS14B" })
    }

    @Test func aRecurringBuysRunOpensToWhatItSaid() throws {
        try fresh()
        let id = UUID().uuidString
        engine.scheduleRuns = [ScheduleRun(at: Date(), title: "buy SOL for 5 USDC", result: "done", line: "Bought 0.03 SOL for 5 USDC.", canvas: nil, task: id)]
        // Filled only while an item's own block is on the canvas, as the LAST RUNS read them.
        #expect(engine.recentRuns.first { $0.task == id }?.activity == nil)
        engine.pin(Board(kind: "scheduleItem", title: "task:\(id)"))
        let activity = try #require(engine.recentRuns.first { $0.task == id }?.activity)
        #expect(activity.kind == "automatic" && activity.title == "buy SOL for 5 USDC" && activity.lines == ["Bought 0.03 SOL for 5 USDC."])
        #expect(!activity.failed)
    }

    @Test func anOldHistoryRecordReadsWithoutItsItem() throws {
        let old = #"{"kind":"automatic","at":0,"title":"BTC","lines":["closed"],"bubbles":[],"failed":false}"#
        let a = try JSONDecoder().decode(HistoryTabs.Activity.self, from: Data(old.utf8))
        #expect(a.schedule == nil && a.title == "BTC")
    }

    @Test func aSwitchToAWalletThatsGoneSaysSoInTheChat() async throws {
        try fresh()
        // Said once in the chat; the throw carries the same reason for Settings' toast (the chat is behind it there).
        let error = await #expect(throws: WalletError.self) { try await engine.useWallet("blocks14-gone") }
        #expect(engine.messages.last?.text == "That wallet isn't here any more.", "\(engine.messages.last?.text ?? "")")
        #expect(error?.localizedDescription == "That wallet isn't here any more.")
    }
}

@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// Wave 12's live part: a star leaves the Favorites block where it is; «approve the trading key on my Ledger» never asks
/// for the main wallet's key; a task's canvas keeps the Schedule; closing the Ledger block stops its search; the History
/// block opens a move to its whole request, a run to its canvas's, and the guardian's moves are a position's runs; a
/// recurring buy gets its board. In-process, state set by hand; public test data only (Hardhat's accounts).
@MainActor @Suite(.serialized) struct Live13Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("live13-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    static let mom = "0x70997970C51812dc3A65C64090c6Cd9F57a2c4Fc"

    private func fresh() throws {
        engine.settings.language = "en"
        engine.settings.schedulePaused = nil
        engine.tasks = []
        try engine.newCanvas()
    }

    @Test func aStarLeavesTheFavoritesBlockWhereItIs() throws {
        try fresh()
        engine.pin(Board(kind: "favorites", title: "favorites"))
        engine.pin(Board(kind: "result", title: "Live13 above"))
        let e = SignerAddress(name: "Live13 Star", network: "base", address: Self.mom, memo: "")
        engine.star(e, true, bringBlock: false)
        #expect(engine.boards.map(\.kind) == ["result", "favorites"])
        #expect(engine.starred.contains(Engine.starKey(e)))
        // From the chat a star still brings the block up.
        engine.star(e, false)
        #expect(engine.boards.first?.kind == "favorites")
        #expect(!engine.starred.contains(Engine.starKey(e)))
    }

    @Test func theLedgersTradingKeyIsNeverTheMainWalletsWhileTheLedgerIsOff() async throws {
        try fresh()
        #expect(!engine.ledgerActive)
        let handled = await engine.tradingKeyCommand("approve the trading key on my Ledger")
        #expect(handled)
        #expect(engine.messages.last?.text.contains("Switch to your Ledger first") == true, "\(engine.messages.last?.text ?? "")")
        #expect(engine.pending == nil && engine.card == nil, "a card for the main wallet's key")
    }

    @Test func aTasksCanvasKeepsTheSchedule() throws {
        try fresh()
        let task = ScheduledTask(steps: ["remember: live13"], next: Date().addingTimeInterval(3600), every: .days(1))
        engine.tasks = [task]
        defer { engine.tasks = []; try? engine.closeSchedule() }
        engine.pin(Board(kind: "schedule", title: "schedule"))
        try engine.openSchedule("task:\(task.id)")
        #expect(engine.scheduleKey == "task:\(task.id)")
        #expect(engine.boards.map(\.kind) == ["scheduleItem", "schedule"], "\(engine.boards.map(\.kind))")
        // Opened from somewhere the Schedule wasn't, it doesn't come along.
        try engine.closeSchedule()
        engine.boards = []
        try engine.openSchedule("task:\(task.id)")
        #expect(engine.boards.map(\.kind) == ["scheduleItem"])
    }

    @Test func closingTheLedgerBlockStopsItsSearch() async throws {
        try fresh()
        engine.ledgerConnect = LedgerConnect()
        let id = engine.pin(Board(kind: "ledger", title: "ledger"))
        let r = try JSONDecoder().decode(Engine.Request.self, from: Data(#"{"op":"closeBoard","board":"\#(id)"}"#.utf8))
        await engine.handle(r)
        #expect(engine.ledgerConnect == nil)
        #expect(!engine.boards.contains { $0.id == id })
    }

    @Test func theHistoryBlockOpensAMoveToItsWholeRequest() throws {
        try fresh()
        let asked = "send 5 USDC to Live13 Mom"
        engine.messages.append(Message(fromUser: true, text: asked))
        engine.messages.append(Message(fromUser: false, text: "Sent 5 USDC to Live13 Mom on Base. https://basescan.org/tx/0xabc"))
        _ = HistoryStore.record(engine.canvases)
        // Without the block nothing reads every chat for it.
        #expect(engine.historyMoves.first { $0.request == asked }?.activity == nil)
        engine.pin(Board(kind: "history", title: "history"))
        let row = try #require(engine.historyMoves.first { $0.request == asked })
        #expect(row.activity?.request == asked)
        #expect(row.activity?.lines.contains { $0.hasPrefix("Sent 5 USDC") } == true, "\(row.activity?.lines ?? [])")
    }

    /// D7, D8: a run opens to what its canvas's request did; the guardian's moves on a position are that item's runs.
    @Test func aRunOpensToItsRequestAndTheGuardiansMovesAreRuns() throws {
        try fresh()
        let asked = "Scheduled: remember: live13 run"
        engine.messages.append(Message(fromUser: true, text: asked, note: "schedule"))
        engine.messages.append(Message(fromUser: false, text: "Noted."))
        _ = HistoryStore.record(engine.canvases)
        let kept = engine.scheduleRuns
        defer { engine.scheduleRuns = kept }
        engine.scheduleRuns.append(ScheduleRun(at: Date(), title: "remember: live13 run", result: "done", line: "Noted.", canvas: engine.canvasID, task: "live13"))
        #expect(engine.recentRuns.first { $0.task == "live13" }?.activity == nil, "read with no item's block on the canvas")
        engine.pin(Board(kind: "scheduleItem", title: "task:live13"))
        #expect(engine.recentRuns.first { $0.task == "live13" }?.activity?.request == asked)

        engine.recordAutomatic("Partial profit taken on LIVE13", "Stop moved to breakeven.", coin: "LIVE13")
        let run = try #require(engine.recentRuns.first { $0.task == "perp:LIVE13" })
        #expect(run.title == "Partial profit taken on LIVE13" && run.line == "Stop moved to breakeven." && run.result == "done")
        #expect(!engine.scheduleRuns.contains { $0.task == "perp:LIVE13" }, "a view only: the saved runs stay the runs")
    }

    @Test func aRecurringBuyGetsItsBoard() async throws {
        try fresh()
        defer { engine.tasks = [] }
        let buy = try #require(WalletCommand.parse("buy SOL for 5 USDC"))
        // No wallet here: the first buy fails at «Add a wallet first», and the schedule stays, as on the Mac.
        try? await engine.startRecurring(buy, days: 7)
        #expect(engine.boards.contains { $0.kind == "recurring" && $0.title == "SOL" }, "\(engine.boards.map { "\($0.kind) \($0.title)" })")
        let view = try #require(engine.recurringViews.first)
        #expect(view.symbol == "SOL" && view.usdc == 5 && view.days == 7 && !view.paused)
        #expect(engine.recurringFacts("sol").first == "5 USDC → SOL every 7 day(s)")
        engine.tasks = []
        #expect(engine.recurringViews.isEmpty)
        #expect(engine.recurringFacts("SOL").first?.hasPrefix("stopped") == true)
    }
}

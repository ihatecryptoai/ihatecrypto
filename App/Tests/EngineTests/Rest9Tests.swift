@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Wave 8's «rest» (audit7): a recurring buy in the History block, the History block before anything moved, a found
/// update waiting in About, and the guardian's reason said once whatever the clock says.
@MainActor @Suite(.serialized) struct Rest9Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("rest9-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    /// A recurring buy runs with no canvas and writes no chat line: its run's swap is still a move in «History · latest»
    /// and in the day's line (the Mac records it as an activity), with no canvas to open.
    @Test func aRecurringBuysSwapIsAMoveInTheHistoryBlock() {
        let before = engine.scheduleRuns
        defer { engine.scheduleRuns = before }
        let at = Date().addingTimeInterval(86_400 * 400)    // newer than anything another test made
        engine.scheduleRuns.append(ScheduleRun(at: at, title: "buy SOL for 5 USDC", result: "done", line: "Swapped 5 USDC → SOL +0.0312.", canvas: nil, task: nil))
        engine.scheduleRuns.append(ScheduleRun(at: at, title: "buy SOL for 5 USDC", result: "failed", line: "Swapped nothing: refused.", canvas: nil, task: nil))
        let row = engine.historyMoves.first
        #expect(row?.kind == "swapped" && row?.canvas == nil && row?.request == "buy SOL for 5 USDC", "\(String(describing: row))")
        #expect(engine.historyMoves.filter { $0.at == at }.count == 1, "a failed run isn't a move")
        #expect(engine.movesDay?.day == Calendar.current.startOfDay(for: at))
    }

    /// 11:58 AM and 12:08 PM are one reason: the time it asks again goes with its AM/PM (R6's leftover).
    @Test func aReasonIsTheSameAcrossNoon() {
        let morning = "The trading key wasn't opened, so this order wasn't signed. ihatecrypto asks again from 11:58 AM."
        let noon = "The trading key wasn't opened, so this order wasn't signed. ihatecrypto asks again from 12:08 PM."
        #expect(Engine.reasonOnly(morning) == Engine.reasonOnly(noon))
        #expect(Engine.reasonOnly(morning) != Engine.reasonOnly("Hyperliquid refused the order: reduce only."))
    }

    /// The updater's find is kept for About, which offers Install without a check.
    @Test func aFoundUpdateWaitsInTheState() {
        engine.updateReady("9.9.9")
        defer { Engine.availableUpdate = nil }
        #expect(Engine.availableUpdate == "9.9.9")
    }
}

/// Before anything moved, the History block shows the latest three requests, as the Mac's HistoryBoard does. In a
/// process of its own, so no other test's moves are there.
@Suite struct RecentRequestsTests {
    @Test func nothingMovedShowsTheLatestRequests() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let r = try engine.call("send", ["text": "clear my alerts"])
        #expect((r.state?["moves"] as? [Any])?.isEmpty == true)
        let recent = r.state?["recentRequests"] as? [[String: Any]] ?? []
        #expect(recent.first?["request"] as? String == "clear my alerts", "\(recent)")
        #expect((recent.first?["lines"] as? [String])?.last?.contains("no price alerts") == true, "\(recent)")
    }
}

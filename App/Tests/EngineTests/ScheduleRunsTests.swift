@testable import DesktopEngine
import CWinSec
import Foundation
import Testing

/// The Schedule as the Mac runs it (Tasks.swift): a run on a canvas of its own and back, the same task asked twice found,
/// «Pays from» on a task, the automation wallet in Addresses with its top-up card, the notification sounds.
@Suite(.serialized) struct ScheduleRunsTests {
    static func tasks(_ s: [String: Any]?) -> [[String: Any]] { (s?["tasks"] as? [[String: Any]]) ?? [] }

    @Test func aRunGoesOnItsOwnCanvasAndComesBack() throws {
        // Due 40 minutes ago: past the half hour a task waits for the person to step away.
        let task: [String: Any] = ["id": UUID().uuidString, "steps": ["remember: its own canvas"], "next": Date().timeIntervalSinceReferenceDate - 2400]
        let engine = try EngineTests.Engine(seed: ["tasks.json": try JSONSerialization.data(withJSONObject: [task])])
        let first = try #require(try engine.call("hello").state)
        let home = try #require(first["canvas"] as? String)
        let ran = try #require(try LiveTests.wait(engine) { !(($0["scheduleRuns"] as? [Any]) ?? []).isEmpty })
        let run = try #require((ran["scheduleRuns"] as? [[String: Any]])?.first)
        // It ran on a canvas of its own, named by its request, and the person is back where they were.
        #expect(run["canvas"] as? String != home, "\(run)")
        #expect(ran["canvas"] as? String == home)
        let rows = (ran["canvases"] as? [[String: Any]]) ?? []
        #expect(rows.count == 2 && rows.contains { $0["id"] as? String == run["canvas"] as? String && ($0["title"] as? String)?.hasPrefix("Scheduled:") == true }, "\(rows)")
    }

    @Test func theSameTaskAskedTwiceIsFoundAndResumed() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("send", ["text": "every day at 09:00: buy SOL for 5 USDC"])
        let twice = try engine.call("send", ["text": "every day at 09:00: buy SOL for 5 USDC"])
        #expect(Self.tasks(twice.state).count == 1)
        #expect(EngineTests.chat(twice.state).last?.contains("already scheduled") == true, "\(EngineTests.chat(twice.state).last ?? "")")
        // Paused, then asked for again: switched back on, not made twice.
        let id = try #require(Self.tasks(twice.state).first?["id"] as? String)
        try engine.call("taskOn", ["wallet": id, "test": false])
        let again = try engine.call("send", ["text": "every day at 09:00: buy SOL for 5 USDC"])
        #expect(Self.tasks(again.state).count == 1 && Self.tasks(again.state).first?["paused"] as? Bool == false)
        #expect(EngineTests.chat(again.state).last?.contains("It’s on again") == true, "\(EngineTests.chat(again.state).last ?? "")")
        // Without an automation wallet there's nothing to pick where it pays from.
        #expect(Self.tasks(again.state).first?["pick"] as? Bool == false)
        try engine.call("taskPays", ["wallet": id, "test": true])
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func theAutomationWalletIsInAddressesAndATaskPicksWhereItPays() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        let made = try #require(try engine.call("createAutomationWallet").state)
        let automation = try #require(made["automation"] as? [String: Any])
        let label = try #require(automation["label"] as? String)
        // In Addresses on Solana and Base, so «send 20 USDC to Automation» tops it up; and that's offered.
        let book = (made["book"] as? [[String: Any]]) ?? []
        #expect(Set(book.filter { $0["name"] as? String == label }.compactMap { $0["network"] as? String }) == ["solana", "base"], "\(book)")
        let offered = ((made["options"] as? [[String: Any]]) ?? []).compactMap { $0["title"] as? String }
        #expect(offered.contains("Top up 20 USDC"), "\(offered)")

        // A buy runs from it by itself, so it starts paused; the block can switch it to pay from the wallet that's on.
        let s = try #require(try engine.call("send", ["text": "every day at 09:00: buy SOL for 5 USDC"]).state)
        let t = try #require(Self.tasks(s).first)
        #expect(t["pick"] as? Bool == true && t["automatic"] as? Bool == true && t["paused"] as? Bool == true, "\(t)")
        let id = try #require(t["id"] as? String)
        let moved = try #require(try engine.call("taskPays", ["wallet": id, "test": true]).state)
        #expect(Self.tasks(moved).first?["automatic"] as? Bool == false)
        let back = try #require(try engine.call("taskPays", ["wallet": id, "test": false]).state)
        #expect(Self.tasks(back).first?["automatic"] as? Bool == true)
    }

    /// The Mac's three sounds by what a notice is about: a coin for money, a chime for a look, a knock for a failure.
    @Test func eachNoticeHasTheMacsSound() {
        #expect([Notice.incoming, .orders, .moves, .hlp].allSatisfy { $0.sound() == "coin" })
        #expect([Notice.alerts, .confirm, .signature, .positions, .update].allSatisfy { $0.sound() == "chime" })
        #expect(Notice.moves.sound(failed: true) == "knock" && Notice.confirm.sound(failed: true) == "knock")
    }
}

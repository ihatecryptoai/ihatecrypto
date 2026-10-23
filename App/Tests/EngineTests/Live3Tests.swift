import Foundation
import Testing

/// The live blocks, wave 2: the History block lists each move with the request it came from and sums up the day; a
/// scheduled task opens on a canvas of its own, «Run now» keeps a repeating one's next run, Close leaves nothing behind;
/// an alert's steps are drawn one block each.
@Suite(.serialized) struct Live3Tests {
    @Test func theHistoryBlockListsMovesWithTheirRequest() throws {
        let now = Date().timeIntervalSinceReferenceDate
        let canvases: [[String: Any]] = [["id": "c1", "title": "Moves", "boards": [], "at": now - 100, "messages": [
            ["id": "m1", "fromUser": true, "text": "send 5 USDC to Mom", "at": now - 90],
            ["id": "m2", "fromUser": false, "text": "Sent 5 USDC to Mom on Base. https://basescan.org/tx/0xabc", "at": now - 80],
            ["id": "m3", "fromUser": false, "text": "💸 Received 2 USDC on Solana.", "at": now - 10],
        ]]]
        let engine = try EngineTests.Engine(seed: ["canvases.json": try JSONSerialization.data(withJSONObject: canvases)])
        try engine.call("hello")
        let s = try #require(try engine.call("refresh").state)
        let moves = try #require(s["moves"] as? [[String: Any]])
        #expect(moves.map { $0["kind"] as? String } == ["received", "sent"], "\(moves)")
        #expect(moves[1]["request"] as? String == "send 5 USDC to Mom")
        #expect(moves[1]["to"] as? String == "Mom" && moves[1]["link"] as? String == "https://basescan.org/tx/0xabc")
        #expect((moves[1]["amount"] as? NSNumber)?.doubleValue == 5 && moves[1]["canvas"] as? String == "c1")
        let day = try #require(s["movesDay"] as? [String: Any])
        #expect(!((day["lines"] as? [String]) ?? []).isEmpty, "\(day)")
    }

    @Test func aTaskOpensOnItsOwnCanvasAndRunNowKeepsItsNextRun() throws {
        let id = UUID().uuidString, next = Date().addingTimeInterval(3600).timeIntervalSinceReferenceDate
        let task: [String: Any] = ["id": id, "steps": ["remember: run now"], "next": next, "every": ["days": ["_0": 1]]]
        let engine = try EngineTests.Engine(seed: ["tasks.json": try JSONSerialization.data(withJSONObject: [task])])
        try engine.call("hello")
        let key = "task:" + id
        let opened = try #require(try engine.call("openSchedule", ["name": key]).state)
        #expect(opened["scheduleKey"] as? String == key)
        #expect((opened["boards"] as? [[String: Any]])?.first?["kind"] as? String == "scheduleItem")
        #expect((opened["canvases"] as? [Any])?.count == 2)

        let ran = try #require(try engine.call("runNow", ["name": key]).state)
        #expect((ran["scheduleRuns"] as? [[String: Any]])?.first?["task"] as? String == id, "\(ran["scheduleRuns"] ?? "")")
        // A repeating one keeps its next run as it was, not a day from now.
        let t = try #require((ran["tasks"] as? [[String: Any]])?.first { $0["id"] as? String == id })
        let kept = ISO8601DateFormatter().date(from: t["next"] as? String ?? "")
        #expect(kept.map { abs($0.timeIntervalSinceReferenceDate - next) < 2 } == true, "\(t)")
        #expect(t["every"] is String, "\(t)")

        // Opened again: the same canvas, not a second one.
        try engine.call("closeSchedule")
        let again = try #require(try engine.call("openSchedule", ["name": key]).state)
        #expect((again["canvases"] as? [Any])?.count == 2)
        let closed = try #require(try engine.call("closeSchedule").state)
        #expect(closed["scheduleKey"] == nil || closed["scheduleKey"] is NSNull)
        #expect((closed["canvases"] as? [Any])?.count == 1, "an item's canvas nothing was written on leaves the list")
        #expect(try engine.call("openSchedule", ["name": "nonsense"]).answer["ok"] as? Bool == false)
    }

    @Test func anAlertsStepsAreItsBlocks() throws {
        let alert: [String: Any] = ["id": UUID().uuidString, "coin": "ETH", "above": true, "price": 999_999,
                                    "created": Date().timeIntervalSinceReferenceDate, "steps": ["send 1 USDC to Mom"]]
        let engine = try EngineTests.Engine(seed: ["alerts.json": try JSONSerialization.data(withJSONObject: [alert])])
        try engine.call("hello")
        let s = try #require(try engine.call("refresh").state)
        let a = try #require((s["alerts"] as? [[String: Any]])?.first)
        let blocks = try #require(a["blocks"] as? [[String: Any]])
        #expect(blocks.count == 1 && blocks[0]["plan"] as? Bool == false && blocks[0]["title"] as? String == "send 1 USDC to Mom", "\(blocks)")
    }
}

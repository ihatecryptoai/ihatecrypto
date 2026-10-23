import Foundation
import Testing

/// The live blocks' engine side: the Schedule's pause holds every task back and its History logs each run; the
/// Favorites «+» tells a token from a wallet before anything is looked up.
@Suite(.serialized) struct LiveTests {
    /// A task due an hour ago that moves no money.
    static func dueTask() throws -> Data {
        try JSONSerialization.data(withJSONObject: [["id": UUID().uuidString, "steps": ["remember: the schedule ran"],
                                                     "next": Date().timeIntervalSinceReferenceDate - 3600]])
    }

    /// States until one passes `done`, for what the engine does on its own minute loop.
    static func wait(_ engine: EngineTests.Engine, tries: Int = 10, _ done: ([String: Any]) -> Bool) throws -> [String: Any]? {
        var last: [String: Any]?
        for _ in 0..<tries {
            last = try engine.call("refresh").state ?? last
            if let s = last, done(s) { return s }
        }
        return last
    }

    @Test func aPausedScheduleRunsNothingAndARunGoesInTheHistory() throws {
        let paused = try EngineTests.Engine(seed: ["tasks.json": try Self.dueTask(), "settings.json": Data(#"{"schedulePaused":true}"#.utf8)])
        try paused.call("hello")
        let held = try #require(try Self.wait(paused, tries: 3) { _ in false })
        #expect(held["schedulePaused"] as? Bool == true)
        #expect((held["scheduleRuns"] as? [Any])?.isEmpty == true)
        #expect((held["tasks"] as? [Any])?.count == 1, "a paused schedule ran its task")

        let engine = try EngineTests.Engine(seed: ["tasks.json": try Self.dueTask()])
        try engine.call("hello")
        let ran = try #require(try Self.wait(engine) { !(($0["scheduleRuns"] as? [Any]) ?? []).isEmpty })
        let run = try #require((ran["scheduleRuns"] as? [[String: Any]])?.first)
        #expect(run["title"] as? String == "remember: the schedule ran" && run["result"] as? String == "done", "\(run)")
        // What it said goes with it; a scheduled «remember» isn't the user's own words, so nothing is kept.
        #expect((run["line"] as? String)?.contains("your own words") == true, "\(run)")
        #expect((ran["memory"] as? [String])?.isEmpty == true)
        #expect((ran["tasks"] as? [Any])?.isEmpty == true)

        // The pause is the window's switch, kept in the settings.
        #expect(try engine.call("schedulePause", ["test": true]).state?["schedulePaused"] as? Bool == true)
        #expect(try engine.call("schedulePause", ["test": false]).state?["schedulePaused"] as? Bool == false)
    }

    @Test func thePlusBoxTellsWhatWasPasted() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        #expect(try engine.call("lookToken", ["address": "hello there"]).answer["result"] as? [String: String] == ["kind": "invalid"])
        // A Bitcoin address is never a DexScreener token: no lookup, a wallet to save on Bitcoin.
        let btc = try #require(try engine.call("lookToken", ["address": " bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq "]).answer["result"] as? [String: String])
        #expect(btc == ["kind": "address", "address": "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", "network": "bitcoin"])
    }
}

import Foundation
import Testing

/// Notifications by kind, as the Mac's Notice: each kind has its switch in Settings, the engine sends only those on,
/// Settings' test always goes out, and a crash the window kept is one line in the chat.
@Suite(.serialized) struct NoticesTests {
    /// One request, and the `notify` events that came with it.
    static func notices(_ engine: EngineTests.Engine, _ op: String, _ args: [String: Any] = [:]) throws -> [[String: String]] {
        var request = args
        request["id"] = 900_000 + Int.random(in: 0..<99_999)
        request["op"] = op
        engine.input.fileHandleForWriting.write(try JSONSerialization.data(withJSONObject: request) + Data("\n".utf8))
        var found: [[String: String]] = [], answered = false
        while let line = try? engine.readLine(timeout: answered ? 1.5 : 30) {
            guard let o = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { continue }
            if o["event"] as? String == "notify", let data = o["data"] as? [String: String] { found.append(data) }
            if o["id"] as? Int == request["id"] as? Int { answered = true }
        }
        return found
    }

    @Test func eachKindHasItsSwitchAndOnlyThoseOnGoOut() throws {
        let engine = try EngineTests.Engine()
        let hello = try engine.call("hello")
        let kinds = try #require(hello.state?["notices"] as? [[String: Any]])
        #expect(kinds.map { $0["id"] as? String } == ["incoming", "positions", "orders", "alerts", "moves", "hlp", "signature", "confirm", "update"])
        #expect(kinds.allSatisfy { $0["on"] as? Bool == true })

        // A new version, said with its kind.
        let ready = try Self.notices(engine, "updateReady", ["text": "0.2.0"])
        #expect(ready.count == 1 && ready.first?["kind"] == "update" && ready.first?["title"]?.contains("0.2.0") == true, "\(ready)")

        // Its switch off: nothing goes out, and the state says so.
        let off = try engine.call("notice", ["name": "update", "test": false])
        #expect((off.state?["notices"] as? [[String: Any]])?.first { $0["id"] as? String == "update" }?["on"] as? Bool == false)
        #expect(try Self.notices(engine, "updateReady", ["text": "0.2.1"]).isEmpty)

        // Settings' test goes out whatever is off.
        let test = try Self.notices(engine, "noticeTest")
        #expect(test.first?["kind"] == "test", "\(test)")

        // On again.
        try engine.call("notice", ["name": "update", "test": true])
        #expect(try Self.notices(engine, "updateReady", ["text": "0.2.2"]).count == 1)
        // A kind the engine doesn't know changes nothing.
        let unknown = try engine.call("notice", ["name": "everything", "test": false])
        #expect((unknown.state?["notices"] as? [[String: Any]])?.allSatisfy { $0["on"] as? Bool == true } == true)
    }

    @Test func openAtLoginIsKeptAndToldToTheWindow() throws {
        let engine = try EngineTests.Engine()
        #expect(try engine.call("hello").state?["openAtLogin"] as? Bool == false)
        // The switch: kept, and main.rs told to set Windows' entry.
        engine.input.fileHandleForWriting.write(Data(#"{"id":7001,"op":"openAtLogin","test":true}"#.utf8) + Data("\n".utf8))
        var told = false, state: [String: Any]?
        while let line = try? engine.readLine(timeout: 3), let o = try? JSONSerialization.jsonObject(with: line) as? [String: Any] {
            if o["event"] as? String == "openAtLogin" { told = (o["data"] as? [String: String])?["on"] == "1" }
            if o["event"] as? String == "state" { state = o["data"] as? [String: Any] }
        }
        #expect(told && state?["openAtLogin"] as? Bool == true)
        // Windows says it's off (removed in its Startup apps): the switch follows.
        #expect(try engine.call("loginState", ["test": false]).state?["openAtLogin"] as? Bool == false)
    }

    @Test func aKeptCrashIsOneLineInTheChat() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let said = try engine.call("crashed", ["name": "1792800000", "text": "the engine stopped (exit code 0x80000003)", "address": #"C:\Users\dev\AppData\Local\app\Crashes"#])
        let line = try #require(EngineTests.chat(said.state).last)
        #expect(line.contains("closed unexpectedly") && line.contains("exit code 0x80000003") && line.contains("Crashes"), "\(line)")
    }
}

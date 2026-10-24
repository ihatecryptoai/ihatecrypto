import CWinSec
import Foundation
import Testing

/// The app's lock (Lock.swift): with a code set the window gets the lock alone — no balances, chat or history — and
/// nothing but the lock's own requests is answered until the code (checked by the signer) or Windows Hello opens it.
@Suite(.serialized) struct LockTests {
    static func lock(_ state: [String: Any]?) -> [String: Any] { state?["lock"] as? [String: Any] ?? [:] }

    // Windows Hello answers only the stand-in: a real one would wait for a person.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aCodeLocksTheWindowAndOnlyTheCodeOrWindowsHelloOpensIt() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let made = try engine.call("setCode", ["text": "481902"])
        #expect(made.answer["ok"] as? Bool == true, "\(made.answer)")
        #expect(Self.lock(made.state)["codeSet"] as? Bool == true)

        let locked = try #require(try engine.call("lockApp").state)
        #expect(Self.lock(locked)["locked"] as? Bool == true)
        #expect(Set(locked.keys) == ["lock", "russian", "language"], "a locked window got \(locked.keys.sorted())")
        #expect(try engine.call("send", ["text": "what's my balance"]).answer["ok"] as? Bool == false)

        let wrong = try engine.call("unlock", ["text": "481903"])
        #expect((Self.lock(wrong.state)["problem"] as? String)?.contains("4 tries left") == true, "\(Self.lock(wrong.state))")
        let open = try engine.call("unlock", ["text": "481902"])
        #expect(Self.lock(open.state)["locked"] as? Bool == false)
        #expect(open.state?["portfolio"] != nil)

        // Windows Hello opens it too.
        try engine.call("lockApp")
        #expect(Self.lock(try engine.call("unlockHello").state)["locked"] as? Bool == false)

        // Five wrong codes in a row turn the code off: only Windows Hello opens it, and then a new code is offered.
        try engine.call("lockApp")
        var last: [String: Any] = [:]
        for _ in 0..<5 { last = Self.lock(try engine.call("unlock", ["text": "000001"]).state) }
        #expect(last["codeOff"] as? Bool == true && last["locked"] as? Bool == true, "\(last)")
        #expect(Self.lock(try engine.call("unlock", ["text": "481902"]).state)["locked"] as? Bool == true)
        let hello = Self.lock(try engine.call("unlockHello").state)
        #expect(hello["locked"] as? Bool == false && hello["offerNewCode"] as? Bool == true && hello["codeSet"] as? Bool == false)
    }

    // Windows Hello answers only the stand-in: a real one would wait for a person.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aLaunchAfterACodeStartsLocked() throws {
        // The settings say a code was set; this signer has none (its file gone): locked, and only Windows Hello opens it.
        let engine = try EngineTests.Engine(seed: ["settings.json": Data(#"{"codeSet":true}"#.utf8)])
        let first = Self.lock(try engine.call("hello").state)
        #expect(first["locked"] as? Bool == true && first["codeOff"] as? Bool == true, "\(first)")
        #expect(Self.lock(try engine.call("unlockHello").state)["locked"] as? Bool == false)
    }

    @Test func minutesAwayAreOnlyTheMacsChoices() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        #expect(Self.lock(try engine.call("lockAfter", ["seconds": 900]).state)["lockAfter"] as? Int == 15)
        #expect(Self.lock(try engine.call("lockAfter", ["seconds": 120]).state)["lockAfter"] as? Int == 15)
    }
}

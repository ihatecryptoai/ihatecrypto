import CWinSec
import Foundation
import Testing

/// Wave 12's settings part: the first run holds the chat's own offers, «Forgot the code?» opens with Windows Hello and
/// offers a new code, and an account that isn't the chat's keeps its own model.
extension EngineTests {
    static func says(_ state: [String: Any]?, _ text: String) -> Bool { chat(state).contains { $0.contains(text) } }

    /// Every module a later version brings is asked about once, but not while the first run is on screen (the Mac's
    /// ModulesUI.swift:334): the question waits for the next launch.
    @Test func theFirstRunHoldsTheNewModulesQuestion() throws {
        let known = ["settings.json": Data(#"{"modulesKnown":[]}"#.utf8)]
        let held = try Engine(seed: known)
        try held.call("onboarding", ["test": true])
        #expect(!Self.says(try held.call("hello").state, "New in ihatecrypto"))

        let later = try Engine(seed: known)
        try later.call("onboarding", ["test": false])
        #expect(Self.says(try later.call("hello").state, "New in ihatecrypto"))
    }

    /// «Keep» on an account that isn't the chat's saves its model; the chat's own stays as it was.
    @Test func anAccountKeepsItsOwnModel() throws {
        let engine = try Engine()
        let before = try #require(try engine.call("hello").state)
        let chatModel = (before["model"] as? [String: Any])?["model"] as? String
        try #require((before["model"] as? [String: Any])?["provider"] as? String != "openrouter")
        let state = try #require(try engine.call("modelFor", ["provider": "openrouter", "model": "z-ai/glm-test"]).state)
        #expect((state["modelChoices"] as? [String: String])?["openrouter"] == "z-ai/glm-test")
        #expect((state["model"] as? [String: Any])?["model"] as? String == chatModel)
        // Empty goes back to the account's default.
        let back = try #require(try engine.call("modelFor", ["provider": "openrouter", "model": ""]).state)
        #expect((back["modelChoices"] as? [String: String])?["openrouter"] != "z-ai/glm-test")
    }

    // Windows Hello answers only the stand-in: a real one would wait for a person.
    /// «Forgot the code?»: Windows Hello opens the lock and a new code is offered, as the Mac's Passcode forgot path.
    @Test(.enabled(if: winsec_is_stub() == 1)) func forgotTheCodeOpensWithWindowsHelloAndOffersANewOne() throws {
        let engine = try Engine()
        try engine.call("hello")
        #expect(try engine.call("setCode", ["text": "481902"]).answer["ok"] as? Bool == true)
        try engine.call("lockApp")
        let plain = LockTests.lock(try engine.call("unlockHello").state)
        #expect(plain["locked"] as? Bool == false && plain["offerNewCode"] as? Bool != true, "\(plain)")

        try engine.call("lockApp")
        let forgot = LockTests.lock(try engine.call("unlockHello", ["test": true]).state)
        #expect(forgot["locked"] as? Bool == false && forgot["offerNewCode"] as? Bool == true, "\(forgot)")
    }
}

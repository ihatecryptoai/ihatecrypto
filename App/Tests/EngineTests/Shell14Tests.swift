import CWinSec
import Foundation
import Testing

/// The shell's wave 13 (audit12 N1): the browser's phishing check still answers while the app is locked, as the Mac's
/// browser checks whatever the lock, so the page kept beside the chat under the lock screen never loads a listed host;
/// nothing else is answered. A subprocess engine with the list seeded (no network), public test data only.
@Suite(.serialized) struct Shell14Tests {
    // Windows Hello answers only the stand-in: a real one would wait for a person.
    @Test(.enabled(if: winsec_is_stub() == 1)) func thePhishingCheckAnswersWhileLocked() throws {
        let list = Data(#"{"blacklist":["phish-test.example"],"whitelist":[]}"#.utf8)
        let engine = try EngineTests.Engine(seed: ["phishing.json": list])
        try engine.call("hello")
        #expect(try engine.call("setCode", ["text": "481902"]).answer["ok"] as? Bool == true)
        #expect(LockTests.lock(try engine.call("lockApp").state)["locked"] as? Bool == true)

        let listed = try engine.call("siteState", ["address": "https://phish-test.example"]).answer
        #expect(listed["ok"] as? Bool == true, "\(listed)")
        #expect((listed["result"] as? [String: Any])?["blocked"] as? String == "true", "\(listed)")
        let plain = try engine.call("siteState", ["address": "https://jup.ag"]).answer
        #expect((plain["result"] as? [String: Any])?["blocked"] as? String == "false", "\(plain)")

        // Everything else still waits for the code: the chat, and a site's request for the wallet.
        #expect(try engine.call("send", ["text": "what's my balance"]).answer["ok"] as? Bool == false)
        #expect(try engine.call("dapp", ["name": "connect", "text": "{}", "address": "https://jup.ag"]).answer["ok"] as? Bool == false)
    }
}

import CWinSec
import Foundation
import Testing

/// Wave 13's settings part: each «Forgot the code?» offers a new code again, and What's New tells a PC only what's true
/// of the PC.
extension EngineTests {
    // Windows Hello answers only the stand-in: a real one would wait for a person.
    /// A second «Forgot the code?» with no code made in between is a new offer (the Mac opens Settings › Wallet on every
    /// password unlock); making a code ends it.
    @Test(.enabled(if: winsec_is_stub() == 1)) func eachForgotTheCodeIsANewOffer() throws {
        let engine = try Engine()
        try engine.call("hello")
        #expect(try engine.call("setCode", ["text": "481902"]).answer["ok"] as? Bool == true)
        try engine.call("lockApp")
        let first = LockTests.lock(try engine.call("unlockHello", ["test": true]).state)
        try engine.call("lockApp")
        let second = LockTests.lock(try engine.call("unlockHello", ["test": true]).state)
        #expect(first["offerNewCode"] as? Bool == true && second["offerNewCode"] as? Bool == true, "\(second)")
        let a = try #require(first["newCodeOffers"] as? Int), b = try #require(second["newCodeOffers"] as? Int)
        #expect(b == a + 1, "\(a) → \(b)")

        let made = LockTests.lock(try engine.call("setCode", ["text": "530617"]).state)
        #expect(made["offerNewCode"] as? Bool == false, "\(made)")
    }

    /// whatsnew.py says the Mac's stories for a PC: none of them claims the Mac's rebuild check, Sparkle's window, a
    /// daily check or the mascot film for this app.
    @Test func whatsNewOnAPCMakesNoMacOnlyClaim() throws {
        let data = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Windows").appendingPathComponent("app").appendingPathComponent("ui").appendingPathComponent("whatsnew-data.js")
        let text = try String(contentsOf: data, encoding: .utf8)
        for mac in ["every release rebuilds from its tag", "Every release also rebuilds", "Skip This Version", "once a day",
                    "played as a film", "Ctrl+-scroll", "Keychain", "your PC password"] {
            #expect(!text.contains(mac), "What's New still says «\(mac)»")
        }
        #expect(text.contains("The Windows installer isn't rebuilt and compared that way."))
    }
}

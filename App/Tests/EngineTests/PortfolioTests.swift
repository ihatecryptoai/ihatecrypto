import CWinSec
import Foundation
import Testing

/// The portfolio frame's own state: tokens the person hides stay hidden after a restart, and a bridge order signed and
/// not landed yet is «In transit» on the next read.
@Suite(.serialized) struct PortfolioTests {
    typealias Engine = EngineTests.Engine

    static func settings(_ engine: Engine) throws -> [String: Any] {
        let data = try Data(contentsOf: engine.dir.appendingPathComponent("engine/settings.json"))
        return try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    @Test func aTokenTheUserHidesStaysHiddenAndCanBeShownAgain() throws {
        let engine = try Engine()
        try engine.call("hello")
        let hidden = try engine.call("showToken", ["address": "base:0x940181a94a35a4569e4529a3cdfb74e38fd98631", "test": false])
        #expect(hidden.answer["ok"] as? Bool == true, "\(hidden.answer)")
        let shown = try engine.call("showToken", ["address": "DoVAVzViX8Bjy3r15nwikSaSbzE6dV4ovd28aWpJpump", "test": true])
        #expect(shown.answer["ok"] as? Bool == true)
        let choices = try #require(try Self.settings(engine)["tokenChoices"] as? [String: Bool])
        #expect(choices == ["base:0x940181a94a35a4569e4529a3cdfb74e38fd98631": false, "DoVAVzViX8Bjy3r15nwikSaSbzE6dV4ovd28aWpJpump": true])

        // A restart reads them back: the same choices, and a change of mind replaces one.
        let again = try Engine(seed: ["settings.json": try JSONSerialization.data(withJSONObject: ["tokenChoices": choices])])
        try again.call("hello")
        try again.call("showToken", ["address": "base:0x940181a94a35a4569e4529a3cdfb74e38fd98631", "test": true])
        let after = try #require(try Self.settings(again)["tokenChoices"] as? [String: Bool])
        #expect(after["base:0x940181a94a35a4569e4529a3cdfb74e38fd98631"] == true)
        #expect(after["DoVAVzViX8Bjy3r15nwikSaSbzE6dV4ovd28aWpJpump"] == true)
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func aBridgeOrderOnItsWayIsInTransitUntilItLands() throws {
        // Signed a minute ago, so the read doesn't ask Relay yet; an older one that settled is not in transit.
        let iso = ISO8601DateFormatter()
        let lines = [
            ["at": iso.string(from: Date().addingTimeInterval(-60)), "chain": "relay", "id": "0xdemo-open", "kind": "signed",
             "note": "Solana USDC→Base 5, tx 5xDemoSignature"],
            ["at": iso.string(from: Date().addingTimeInterval(-3600)), "chain": "relay", "id": "0xdemo-done", "kind": "signed", "note": "Base USDC→Solana 3"],
            ["at": iso.string(from: Date().addingTimeInterval(-3000)), "chain": "relay", "id": "0xdemo-done", "kind": "settled", "note": "success"],
        ]
        let journal = try lines.map { try JSONSerialization.data(withJSONObject: $0) + Data("\n".utf8) }.reduce(Data(), +)
        let engine = try Engine(seed: ["journal.jsonl": journal])
        try engine.call("hello")
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        #expect(try engine.call("importPhrase", ["phrase": phrase]).answer["ok"] as? Bool == true)
        // The order is read at the end of a refresh, after every network: under load that takes a while.
        var transit: [[String: Any]] = []
        let deadline = Date().addingTimeInterval(90)
        while transit.isEmpty, Date() < deadline {
            transit = try engine.call("refresh").state?["inTransit"] as? [[String: Any]] ?? []
        }
        #expect(transit.count == 1, "\(transit)")
        let order = try #require(transit.first)
        #expect(order["id"] as? String == "0xdemo-open")
        #expect(order["what"] as? String == "Solana USDC→Base 5")
        #expect(order["bridge"] as? String == "Relay")
        #expect(order["tx"] as? String == "5xDemoSignature")
        let links = (order["links"] as? [[String: String]] ?? []).compactMap { $0["title"] }
        #expect(links == ["Relay", "Solscan"])
    }
}

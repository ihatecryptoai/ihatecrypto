import CWinSec
import Foundation
import Testing

/// A route step's details (the Mac's StepDetails): the waiting move's amount changes into a new card through the same
/// path as a typed command, an amount that isn't one number is refused with the Mac's words, and «Why this way?»
/// answers from the step itself while the card keeps waiting.
@Suite(.serialized) struct RouteEditTests {
    static func card(_ state: [String: Any]?) -> String? { (state?["card"] as? [String: Any])?["id"] as? String }
    static func edit(_ state: [String: Any]?) -> [String: Any]? { state?["stepEdit"] as? [String: Any] }

    @Test(.enabled(if: winsec_is_stub() == 1)) func aWaitingMovesAmountChangesAndWhyAnswersFromTheStep() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")

        let waiting = try engine.call("send", ["text": "buy native NEAR for 1 USDC"])
        let first = try #require(Self.card(waiting.state), "\(EngineTests.chat(waiting.state).last ?? "")")
        let edit = try #require(Self.edit(waiting.state))
        #expect(edit["board"] as? String == first && edit["amount"] as? String == "1" && edit["unit"] as? String == "USDC" && edit["ways"] as? Bool == false)

        // Not one number: refused, and the card is the same one.
        for bad in ["5abc", "1,000", "", "0", "1.2.3"] {
            let refused = try engine.call("editStep", ["board": first, "text": bad])
            #expect(refused.answer["ok"] as? Bool == false, "«\(bad)» was taken")
            #expect((refused.answer["error"] as? String)?.contains("one number") == true)
            #expect(Self.card(refused.state) == first)
        }

        // «Why this way?»: the step's facts in the chat, from the route; the card keeps waiting.
        let why = try engine.call("whyStep", ["board": first, "index": 0])
        #expect(why.answer["ok"] as? Bool == true)
        let said = try #require((why.state?["chat"] as? [[String: Any]])?.last)
        #expect((said["text"] as? String)?.hasPrefix("Why this way — ") == true && said["note"] as? String == "from the route", "\(said)")
        #expect(Self.card(why.state) == first)

        // A new amount is a new plan: a new card, the old one marked as not signed.
        let changed = try engine.call("editStep", ["board": first, "text": "2,5"])
        let second = try #require(Self.card(changed.state))
        #expect(second != first && Self.edit(changed.state)?["amount"] as? String == "2.5")
        let old = (changed.state?["boards"] as? [[String: Any]])?.first { $0["id"] as? String == first }
        #expect(old?["state"] as? String == "cancelled")

        // The same amount again changes nothing; thousands and decimal marks read as the Mac reads them.
        let same = try engine.call("editStep", ["board": second, "text": "2.5"])
        #expect(Self.card(same.state) == second)
        for (typed, read) in [("1 000", "1000"), ("1.000,50", "1000.5"), ("1,000.50", "1000.5"), ("0,5", "0.5")] {
            let next = try engine.call("editStep", ["text": typed])
            #expect(Self.edit(next.state)?["amount"] as? String == read, "«\(typed)»")
        }

        // A move that no longer waits can't be edited.
        try engine.call("cancel")
        let gone = try engine.call("editStep", ["board": second, "text": "3"])
        #expect(gone.answer["ok"] as? Bool == false)
    }
}

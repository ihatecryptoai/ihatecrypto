import Foundation
import Testing

/// What the Mac's app reads in the chat itself (ChatCommands.swift): exact commands before any model, folders and their
/// notes, a question answered without a model. No model is connected here, so every message is read as typed.
@Suite(.serialized) struct ChatCommandsTests {
    static func canvases(_ state: [String: Any]?) -> [[String: Any]] { state?["canvases"] as? [[String: Any]] ?? [] }
    static func notes(_ state: [String: Any]?) -> [[String: Any]] {
        (state?["boards"] as? [[String: Any]] ?? []).filter { $0["kind"] as? String == "note" }
    }

    @Test func notesFoldersAndTheAppsOwnCommands() throws {
        let engine = try EngineTests.Engine()
        let hello = try engine.call("hello")
        let first = try #require(hello.state?["canvas"] as? String)
        try engine.call("createWallet", ["test": true])
        try engine.call("phraseSaved")

        // On a fresh canvas the note makes it the note's own, in its folder.
        let noted = try engine.call("send", ["text": "write to folder Ideas: buy the dip on SOL"])
        #expect(EngineTests.chat(noted.state).last?.contains("Saved the note to «Ideas»") == true, "\(EngineTests.chat(noted.state))")
        let own = try #require(Self.notes(noted.state).first)
        #expect(own["title"] as? String == "Ideas" && own["lines"] as? [String] == ["buy the dip on SOL"] && own["opens"] == nil)
        #expect(Self.canvases(noted.state).first { $0["id"] as? String == first }?["folder"] as? String == "Ideas")

        // «new canvas» switches, and the command itself stays off the canvas it left.
        let fresh = try engine.call("send", ["text": "new canvas"])
        let second = try #require(fresh.state?["canvas"] as? String)
        #expect(second != first && EngineTests.chat(fresh.state).isEmpty)
        try engine.call("addBoard", ["name": "receive"])

        // From a canvas with something on it: a canvas of its own, a card with «Open» here; a typo is the folder there.
        let elsewhere = try engine.call("send", ["text": "write to folder Idaes: second idea"])
        let card = try #require(Self.notes(elsewhere.state).first)
        let target = try #require(card["opens"] as? String)
        #expect(target != second && target != first)
        #expect(Self.canvases(elsewhere.state).first { $0["id"] as? String == target }?["folder"] as? String == "Ideas")
        // The next note to that folder joins the note the card opens.
        let more = try engine.call("send", ["text": "write to folder Ideas: third"])
        #expect(Self.notes(more.state).first?["lines"] as? [String] == ["second idea", "third"])

        let saved = try engine.call("send", ["text": "save this canvas to folder Trading"])
        #expect(Self.canvases(saved.state).first { $0["id"] as? String == second }?["folder"] as? String == "Trading")

        let cleared = try engine.call("send", ["text": "clear the plan"])
        #expect(EngineTests.chat(cleared.state).last == "The plan is cleared. Tell me what to do next.")
        let news = try engine.call("send", ["text": "what's new"])
        #expect(EngineTests.chat(news.state).last?.hasPrefix("What's new is on screen") == true)
        let nobody = try engine.call("send", ["text": "switch to wallet Nope"])
        #expect(EngineTests.chat(nobody.state).last?.hasPrefix("No wallet has that name. Yours: «") == true, "\(EngineTests.chat(nobody.state))")

        // A question without a model is answered from the positions and the closed trades, never as a plan.
        let asked = try engine.call("send", ["text": "what happened to my ETH position?"])
        let answer = EngineTests.chat(asked.state).last ?? ""
        #expect(answer.contains("position") && !answer.contains("I didn’t get that"), "\(answer)")
    }
}

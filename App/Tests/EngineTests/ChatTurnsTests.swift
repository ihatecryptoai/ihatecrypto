import CWinSec
@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// The chat's turn as the Mac has it (ChatTurns.swift): a message about several blocks carries one chip each, a block
/// whose numbers didn't go says so, the «/» list is the block catalog, a swap's price line reads as the Mac's, and the
/// position chips' phrases are commands the engine parses.
@Suite(.serialized) struct ChatTurnsTests {
    @Test(.enabled(if: winsec_is_stub() == 1)) func aMessageAboutSeveralBlocksCarriesAChipForEach() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        try engine.call("addBoard", ["name": "receive"])
        let added = try engine.call("addBoard", ["name": "alerts"])
        let ids = ((added.state?["boards"] as? [[String: Any]]) ?? []).filter { ["receive", "alerts"].contains($0["kind"] as? String) }.compactMap { $0["id"] as? String }
        #expect(ids.count == 2)

        let sent = try engine.call("send", ["text": "what are these", "board": ids[0], "boards": ids])
        let mine = try #require((sent.state?["chat"] as? [[String: Any]])?.last { $0["fromUser"] as? Bool == true })
        let chips = (mine["contexts"] as? [[String: Any]]) ?? []
        // One chip each, in the order they were attached.
        let kinds = ids.compactMap { id in ((added.state?["boards"] as? [[String: Any]]) ?? []).first { $0["id"] as? String == id }?["kind"] as? String }
        #expect(chips.compactMap { $0["title"] as? String } == kinds, "\(chips)")
        // No alerts yet: only that block's name went, and the chip says so.
        #expect(chips.first { $0["title"] as? String == "alerts" }?["bare"] as? Bool == true)
        // The thinking step and Stop are gone once the turn is over; nothing waits to be tried again.
        #expect(sent.state?["agentStep"] == nil || sent.state?["agentStep"] is NSNull)
        #expect(sent.state?["modelRunning"] as? Bool == false)
        #expect(sent.state?["retryBoard"] == nil || sent.state?["retryBoard"] is NSNull)
        // Stop with nothing running, Refresh with nothing priced: no-ops.
        #expect(try engine.call("stopModel").answer["ok"] as? Bool == true)
        #expect(try engine.call("requote").answer["ok"] as? Bool == true)
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func theSlashListIsTheBlockCatalog() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let answer = try engine.call("blockCatalog").answer
        let list = try #require(((answer["result"] as? [String: Any])?["list"] as? String)?.data(using: .utf8))
        let rows = try #require(JSONSerialization.jsonObject(with: list) as? [[String: String]])
        #expect(rows.count == BlockCatalog.all.filter { !$0.automatic && !$0.example.isEmpty }.count && rows.allSatisfy { !($0["example"] ?? "").isEmpty })
        // Not an address: nothing goes out.
        let none = try engine.call("lookUpContract", ["address": "hello"]).answer
        #expect((none["result"] as? [String: Any])?["list"] as? String == "[]")
    }

    @Test func aSwapsPriceLineReadsAsTheMacs() {
        let sol = SpotAsset.all[0]
        #expect(Engine.swapLine(.buy(sol, usdc: 10), gets: "0.05 SOL") == "You get ≈ 0.05 SOL · 1 SOL ≈ $200")
        #expect(Engine.swapLine(.sell(sol, amount: 2), gets: "300 USDC") == "You get ≈ 300 USDC · 1 SOL ≈ $150")
        #expect(Engine.swapLine(.buyOn("base", token: "ETH", usdc: 25), gets: "a price only") == "You get ≈ a price only")
    }

    @Test func thePositionChipsAreCommands() {
        for phrase in ["move stop to breakeven", "trail 2%", "close half", "close"] {
            #expect(PositionCommand.parse(phrase) != nil, "«\(phrase)»")
            #expect(PositionCommand.parse(phrase + " ETH") != nil, "«\(phrase) ETH»")
        }
    }
}

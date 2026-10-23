import CWinSec
@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// The route's own note (the Mac's MoveRoute footer) on the board and in «Why this way?», which moves get their ways
/// quoted, and boards saved before these fields still read.
@Suite(.serialized) struct Route3Tests {
    @Test func notesSayWhatIsSignedInBothLanguages() {
        let en: (String, String) -> String = { a, _ in a }, ru: (String, String) -> String = { _, b in b }
        #expect(RouteEdit.note(.withdrawToSolana(5), say: en)?.contains("the trading key can’t withdraw") == true)
        #expect(RouteEdit.note(.buyStock(symbol: "TSLA", usdc: 2), say: en)?.contains("Uniswap v4, v3 or Relay’s swap") == true)
        #expect(RouteEdit.note(.sellOn("base", token: "AERO", amount: 1), say: en)?.contains("Uniswap’s token list") == true)
        #expect(RouteEdit.note(.bridge(from: "base", to: "arbitrum", 2), say: ru)?.contains("Платит только на твой же адрес в Arbitrum") == true)
        #expect(RouteEdit.note(.stake(1), say: ru)?.hasPrefix("Одна подпись в Solana") == true)
        // A payment's and a call's cards say it themselves.
        #expect(RouteEdit.note(.pay(method: "GET", url: "https://example.com", body: nil), say: en) == nil)
    }

    @Test func tradesOnEVMNetworksAndRobinhoodAreQuoted() {
        #expect(RouteEdit.quoted(.buyStock(symbol: "TSLA", usdc: 2)) && RouteEdit.quoted(.sellStock(symbol: "TSLA", amount: nil)))
        #expect(RouteEdit.quoted(.buyOn("base", token: "AERO", usdc: 1)))
        #expect(!RouteEdit.quoted(.stake(1)) && !RouteEdit.quoted(.sendTo(name: "mom", amount: 1)))
        #expect(RouteEdit.stockWayName("v4") == "Uniswap v4" && RouteEdit.stockWayName("relay") == "Relay swap")
    }

    @Test func aBoardSavedBeforeTheNoteAndTheTradeStillReads() throws {
        let old = #"{"id":"b","kind":"move","title":"Long ETH","lines":[],"state":"done","at":"2026-10-22T10:00:00Z"}"#
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let board = try decoder.decode(Board.self, from: Data(old.utf8))
        #expect(board.note == nil && board.trade == nil && board.title == "Long ETH")
        let chart = #"{"title":"ETH · 7 days","closes":[1,2],"low":"$1","high":"$2","change":"+1%"}"#
        #expect(try JSONDecoder().decode(ChartLine.self, from: Data(chart.utf8)).coin == nil)
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func theRoutesNoteIsOnTheBoardAndInWhyThisWay() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")

        let waiting = try engine.call("send", ["text": "stake 1 SOL"])
        let id = try #require((waiting.state?["card"] as? [String: Any])?["id"] as? String, "\(EngineTests.chat(waiting.state).last ?? "")")
        let board = try #require((waiting.state?["boards"] as? [[String: Any]])?.first { $0["id"] as? String == id })
        let note = try #require(board["note"] as? String)
        #expect(note.hasPrefix("One Solana signature, checked and simulated"))

        let why = try engine.call("whyStep", ["board": id, "index": 0])
        let said = try #require((why.state?["chat"] as? [[String: Any]])?.last?["text"] as? String)
        #expect(said.hasPrefix("Why this way — ") && said.hasSuffix(note), "\(said)")
    }
}

@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import LLMCatalog
import Testing
@testable import TradeExecution

/// The chat as the Mac's (audit11 area A): the position chips only on the plan's open position, an offer's card that
/// knows the command it fills, ↑↓ history without confirmations and repeats, each account's chosen model named as the
/// Mac names it. In-process, with no network and no model.
@MainActor @Suite(.serialized) struct Chat13Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("chat13-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    private var engine: Engine { Self.engine }

    private func position(_ coin: String) -> HyperliquidAccount.Position {
        HyperliquidAccount.Position(coin: coin, size: 1, entry: 100, unrealizedPnl: 0, liquidation: nil)
    }

    @Test func positionChipsOnlyOnThePlansOpenPosition() {
        var plan = TradeIntent()
        plan.asset = "SOL"
        engine.intent = plan
        engine.positions = [position("ETH")]
        #expect(engine.chipsPosition == nil, "another coin's position isn't the plan's")
        engine.positions = [position("ETH"), position("SOL")]
        #expect(engine.chipsPosition == "SOL")
        #expect(engine.snapshot().chipsPosition == "SOL")
        // Once traded the plan is cleared; the traded one stands in while its position is open.
        engine.intent = TradeIntent()
        engine.tradedPlan = plan
        #expect(engine.chipsPosition == "SOL")
        engine.positions = []
        #expect(engine.chipsPosition == nil)
        engine.tradedPlan = nil
    }

    @Test func anOffersCardCarriesTheCommandItFills() {
        engine.messages.append(Message(fromUser: false, text: "Which one?"))
        engine.options = [ChatRouter.Option(title: "Long", comment: "with a stop", command: "long ETH by the book"),
                          ChatRouter.Option(title: "Answer", comment: "", command: "")]
        #expect(engine.messages.last?.options?.map(\.command) == ["long ETH by the book", ""])
        #expect(engine.snapshot().options.map(\.command) == ["long ETH by the book", ""])
        engine.options = []
    }

    @Test func historyLeavesOutConfirmationsAndRepeats() {
        engine.messages = ["long ETH", "do it", "long ETH", "да", "short SOL", "short SOL"].map { Message(fromUser: true, text: $0) }
            + [Message(fromUser: false, text: "a reply")]
        #expect(engine.snapshot().sent == ["long ETH", "short SOL"])
        engine.messages = []
    }

    @Test func aDownloadRefusedForAnOldOllamaOffersTheUpdate() {
        // State only: nothing here asks Ollama anything.
        engine.localServers["ollama"] = true
        engine.liveModels["ollama"] = ["qwen3:8b"]
        engine.localSetup.ollamaUpdated = true
        engine.localSetup.ollamaOutdated = true
        #expect(engine.ollamaState == "outdated")
        #expect(engine.needsNewOllama)
        engine.localSetup.ollamaOutdated = false
        #expect(engine.ollamaState != "outdated")
        engine.localServers["ollama"] = nil
        engine.liveModels["ollama"] = nil
        engine.localSetup = LocalSetup()
    }

    @Test func askInChatOnACoinGoesAsItsFactsWithAChip() throws {
        // The window's shape (chat.js attachAbout → send's `abouts`); not sent here, so no model is asked.
        let r = try JSONDecoder().decode(Engine.Request.self, from: Data(#"{"op":"send","text":"sell half?","abouts":[{"title":"SOL · Solana","about":"About the coin «SOL: 0.5 held»"}]}"#.utf8))
        let about = try #require(r.abouts?.first)
        #expect(about.title == "SOL · Solana" && about.about == "About the coin «SOL: 0.5 held»")
        engine.alsoAttached = [Board(kind: "result", title: about.title, lines: [about.about])]
        #expect(engine.routerContext().canvas.contains { $0.contains("About the coin «SOL: 0.5 held»") })
        #expect(engine.sentContexts()?.map(\.title) == ["SOL · Solana"])
        engine.alsoAttached = []
    }

    @Test func eachAccountsChosenModelIsNamedAsTheMacNamesIt() throws {
        engine.setModel("gpt-5.4", for: "openrouter")
        let view = try #require(engine.providerViews.first { $0.id == "openrouter" })
        #expect(view.chosen == "gpt-5.4")
        #expect(view.names["gpt-5.4"] == LLMCatalog.displayName(model: "gpt-5.4"))
        for m in view.models { #expect(view.names[m] == LLMCatalog.displayName(model: m)) }
        engine.setModel(nil, for: "openrouter")
    }
}

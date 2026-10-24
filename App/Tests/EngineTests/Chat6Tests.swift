@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import LLMCatalog
import Testing

/// The chat and the canvases as the Mac keeps them (audit4 C, D): a model per account and per canvas, no canvas switch
/// while the chat answers, a canvas's plan, skills and cards back when it opens again, the History kept apart from the
/// canvases, what the app did by itself in it, the cards seen read with the answer to them, the forms the Mac offers.
/// In-process, with no network and no model.
@MainActor @Suite(.serialized) struct Chat6Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("chat6-\(UUID().uuidString)").path)
        return Engine(out: Outbox())
    }()

    private var engine: Engine { Self.engine }

    @Test func eachAccountKeepsItsModelAndASubscriptionStandsInForADroppedOne() throws {
        engine.setModel("some-model", for: "openrouter")
        #expect(engine.model(of: "openrouter") == "some-model")
        engine.setModel(nil, for: "openrouter")
        #expect(engine.model(of: "openrouter") == engine.provider("openrouter")?.info.defaultModel)
        // A subscription that no longer lists the choice: its first listed model stands in, and the choice comes back.
        let sub = try #require(LLMCatalog.all.first { $0.info.kind == .subscription }).info.id
        engine.setModel("gone-model", for: sub)
        engine.liveModels[sub] = ["live-a", "live-b"]
        #expect(engine.model(of: sub) == "live-a")
        engine.liveModels[sub] = ["live-a", "gone-model"]
        #expect(engine.model(of: sub) == "gone-model")
        engine.liveModels[sub] = nil
        engine.setModel(nil, for: sub)
    }

    @Test func onlyAnAccountWithALoginOrOnThisPCIsSwitchedTo() {
        let before = engine.providerID
        engine.useAccount("no-such-provider")
        #expect(engine.providerID == before)
        if let p = LLMCatalog.all.first(where: { $0.info.kind != .local && $0.info.unavailable == nil && Store.credentials()[$0.info.id] == nil }) {
            engine.useAccount(p.info.id)
            #expect(engine.providerID == before, "\(p.info.id) has no login")
        }
        engine.liveModels["ollama"] = nil
        engine.useAccount("ollama", model: "local-model")
        #expect(engine.providerID == "ollama" && engine.model == "local-model")
        // A downloaded chat model stands in for one this PC doesn't have.
        engine.liveModels["ollama"] = ["qwen3:32b"]
        engine.useAccount("ollama")
        #expect(engine.model == "qwen3:32b")
        engine.liveModels["ollama"] = nil
        engine.settings.provider = before
    }

    @Test func noCanvasOpensWhileTheChatAnswers() throws {
        engine.messages.append(Message(fromUser: true, text: "first"))
        let a = engine.canvasID
        try engine.newCanvas()
        engine.messages.append(Message(fromUser: true, text: "second"))
        let b = engine.canvasID
        engine.thinking = true
        defer { engine.thinking = false }
        #expect(!engine.canSwitchCanvas)
        #expect(throws: (any Error).self) { try engine.openCanvas(a) }
        #expect(throws: (any Error).self) { try engine.newCanvas() }
        #expect(engine.canvasID == b)
        // The chat's own «new canvas» is the answer itself, and goes.
        try engine.newCanvas(fromChat: true)
        #expect(engine.canvasID != b)
    }

    @Test func aCanvasKeepsItsPlanSkillsAndCards() throws {
        engine.messages.append(Message(fromUser: true, text: "long SOL risk $5"))
        let a = engine.canvasID
        var plan = TradeIntent()
        plan.asset = "SOL"
        plan.riskUSD = 5
        engine.intent = plan
        engine.loadedSkills = ["trading-study"]
        engine.messages.append(Message(fromUser: false, text: "Two ways:"))
        engine.options = [ChatRouter.Option(title: "Kamino", comment: "", command: "deposit 1 USDC into Kamino")]
        try engine.newCanvas()
        #expect(engine.intent == TradeIntent() && engine.loadedSkills.isEmpty && engine.options.isEmpty)
        // What's kept with the canvas is what canvases.json carries across a restart.
        let kept = try #require(engine.canvases.first { $0.id == a })
        #expect(kept.intent == plan && kept.skills == ["trading-study"] && kept.offers?.map(\.title) == ["Kamino"])
        try engine.openCanvas(a)
        #expect(engine.intent == plan)
        #expect(engine.loadedSkills == ["trading-study"])
        #expect(engine.options.map(\.title) == ["Kamino"])
        // A card picked, or something said after, and the cards don't come back.
        engine.markPicked(0)
        try engine.newCanvas()
        try engine.openCanvas(a)
        #expect(engine.options.isEmpty)
        engine.intent = TradeIntent()
        engine.loadedSkills = []
    }

    @Test func theHistoryKeepsWhatACanvasHadAfterItGoes() throws {
        let words = "kept in the history \(UUID().uuidString.prefix(8))"
        engine.messages.append(Message(fromUser: true, text: words))
        engine.saveChat()
        let id = engine.canvasID
        try engine.newCanvas()
        try engine.removeCanvas(id)
        #expect(!engine.canvases.contains { $0.id == id })
        #expect(engine.historyActivities.contains { $0.request == words })
        // A scheduled run is a request of its own, «Scheduled: …», with no coin (the Mac's Tasks.swift:130): never a
        // «Guardian» step of a trade in History › Positions.
        let run = "Scheduled: buy ETH \(UUID().uuidString.prefix(8))"
        engine.messages.append(Message(fromUser: true, text: run, note: "schedule"))
        engine.saveChat()
        let record = try #require(engine.historyActivities.first { $0.request == run })
        #expect(record.kind == "request" && record.coin == nil)
    }

    @Test func whatTheAppDidByItselfIsARecord() {
        let alert = "Alert \(UUID().uuidString.prefix(8))"
        engine.notify(.alerts, alert, "ETH went above $3,000")
        #expect(engine.historyActivities.contains { $0.kind == "automatic" && $0.title == alert && $0.lines == ["ETH went above $3,000"] })
        // A confirmed move has its own record already, a card waiting isn't one.
        let moved = "Moved \(UUID().uuidString.prefix(8))", waiting = "Waiting \(UUID().uuidString.prefix(8))"
        engine.notify(.moves, moved, "Done")
        engine.notify(.confirm, waiting, "A card")
        #expect(!engine.historyActivities.contains { $0.title == moved || $0.title == waiting })
        // A trailing stop moving again is one more line on its record.
        let trail = "Trailing stop \(UUID().uuidString.prefix(8))"
        engine.recordAutomatic(trail, "stop moved to $100", merging: true)
        engine.recordAutomatic(trail, "stop moved to $101", merging: true)
        #expect(HistoryStore.saved.automatic.filter { $0.title == trail }.map(\.lines) == [["stop moved to $100", "stop moved to $101"]])
    }

    @Test func theCardsSeenGoWithTheAnswerToThem() {
        engine.options = []
        ChatOffers.seen = [ChatRouter.Option(title: "Kamino", comment: "", command: "deposit 1 USDC into Kamino")]
        defer { ChatOffers.seen = [] }
        #expect(engine.routerContext().offered == ["Kamino → deposit 1 USDC into Kamino"])
    }

    @Test func theFormsTheMacOffersAreOffered() {
        engine.offer([ChatRouter.Option(title: "Automation", comment: "", command: "create an automation wallet"),
                      ChatRouter.Option(title: "Cancel", comment: "", command: "cancel all scheduled tasks"),
                      ChatRouter.Option(title: "New", comment: "", command: "new canvas"),
                      ChatRouter.Option(title: "Nonsense", comment: "", command: "fly to the moon")], userText: "what now", found: [])
        #expect(engine.options.map(\.title) == ["Automation", "Cancel", "New"])
        engine.options = []
    }

    @Test func eachWalletHasItsOwnFiles() {
        #expect(Engine.scoped("costs.json", nil) == "costs.json")
        #expect(Engine.scoped("costs.json", "1") == "costs.json")
        #expect(Engine.scoped("costs.json", "2") == "costs-2.json")
    }
}

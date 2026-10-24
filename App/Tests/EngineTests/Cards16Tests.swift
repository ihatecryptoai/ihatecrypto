@testable import DesktopEngine
import Foundation
import WalletCore
import IhatecryptoCore
import Testing
@testable import TradeExecution

/// Wave 15's cards: a chain led by a priced swap is quoted, counted down and held to its price as a single move's card
/// is, and its first move can be edited with the rest kept behind it. In-process: this test's own store folder,
/// balances set by hand, quotes set by hand, nothing sent.
@MainActor @Suite(.serialized) struct Cards16Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards16-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    private var engine: Engine { Self.engine }
    private var sol: SpotAsset { SpotAsset.all.first { $0.symbol == "SOL" }! }

    private func wallet(_ networks: [String: [PortfolioView.Row]]) {
        engine.settings.language = "en"
        engine.portfolio = PortfolioView(networks: networks.map { PortfolioView.Network(key: $0.key, name: WalletCommand.placeName($0.key), usd: 0, rows: $0.value) },
                                         updatedAt: Date())
        engine.wayQuotes = nil
        engine.jupiterShown = nil
        engine.quotedAt = nil
        engine.status = nil
    }

    /// A chain's card pinned by hand, as show() draws it (show() would ask for its quotes): its id.
    private func chainCard(_ pending: Pending, title: String) -> String {
        let id = engine.pin(Board(id: UUID().uuidString, kind: "move", title: title, lines: ["1. first", "2. then"], steps: engine.steps(for: pending, title: "", lines: []), state: "waiting"))
        engine.pending = pending
        engine.card = Card(id: id, title: title, lines: ["1. first", "2. then"], confirm: "Confirm")
        return id
    }

    private func done() {
        if let id = engine.card?.id { engine.boards.removeAll { $0.id == id } }
        engine.pending = nil
        engine.card = nil
    }

    @Test func aChainLedByASolanaSwapIsQuotedCountedDownAndHeldToItsPrice() {
        wallet(["solana": [Engine.row("USDC", 20, usd: 20)]])
        let first = WalletCommand.buy(sol, usdc: 5), then = WalletCommand.solanaTo("arbitrum", 2)
        let id = chainCard(.chain([.wallet(first), .wallet(then)]), title: "2 steps, one Confirm")
        defer { done() }
        #expect(engine.quotedMove == first)
        // Before Jupiter answers: «Quoting…», as a single swap's card.
        #expect(engine.quoteView?.state == "quoting")
        // What quoteCard does once Jupiter answers for the chain's first move.
        engine.jupiterShown = (first, 51_200_000, "≈ 0.0512 SOL", "Jupiter: USDC → SOL")
        engine.redrawRoute(id, first)
        let steps = engine.boards.first { $0.id == id }?.steps ?? []
        let buy = steps.firstIndex { $0.label == "BUY · JUPITER" }
        #expect(buy.map { steps[$0].title } == "≈ 0.0512 SOL", "\(steps.map { ($0.label, $0.title) })")
        #expect(buy.map { steps[$0].lines.first } == "Jupiter: USDC → SOL")
        // The later move is still on the card, pale, after the quoted step.
        #expect(steps.last?.status == "then" && buy.map { $0 < steps.count - 1 } == true, "\(steps.map { ($0.label, $0.status ?? "") })")
        // Confirm counts down from the quote, with the price in the bar.
        let q = engine.quoteView
        #expect(q?.state == "fresh" && q?.line?.contains("0.0512 SOL") == true, "\(String(describing: q))")
        // The chain's swap is held to what the card showed (the Executor's floor), and only that command.
        #expect(engine.shownOut(first) == 51_200_000)
        #expect(engine.shownOut(.buy(sol, usdc: 6)) == nil)
        // Its first move can be edited: its amount, the later steps kept.
        #expect(engine.stepEdit?.amount == "5" && engine.stepEdit?.board == id)
    }

    @Test func aChainLedByAnEVMBuyGetsItsWaysAndRefresh() {
        wallet(["base": [Engine.row("USDC", 20, usd: 20)]])
        let first = WalletCommand.buyOn("base", token: "AERO", usdc: 5), then = WalletCommand.bridge(from: "base", to: "arbitrum", 1)
        let id = chainCard(.chain([.wallet(first), .wallet(then)]), title: "2 steps, one Confirm")
        defer { done() }
        #expect(engine.quoteView?.state == "quoting")
        engine.wayQuotes = (first, [RouteEdit.Way(way: "relay", gets: "2.41 AERO", worth: "$4.90"), RouteEdit.Way(way: "uniswap", gets: "2.38 AERO", worth: "$4.84")])
        engine.redrawRoute(id, first)
        let q = engine.quoteView
        #expect(q?.state == "fresh" && q?.line?.contains("2.41 AERO") == true, "\(String(describing: q))")
        // The step's way picker has both ways, as a single trade's.
        #expect(engine.stepEdit?.ways == true && engine.stepEdit?.quotes?.count == 2)
        #expect((engine.boards.first { $0.id == id }?.steps ?? []).contains { $0.status == "then" })
        // Refresh drops the old price: «Quoting…» again until the new one lands.
        engine.requote()
        #expect(engine.quoteView?.state == "quoting")
    }

    @Test func aChainLedByAnUnpricedMoveHasNoQuoteBar() {
        wallet(["solana": [Engine.row("USDC", 20, usd: 20)]])
        _ = chainCard(.chain([.wallet(.solanaTo("base", 5)), .wallet(.buyOn("base", token: "AERO", usdc: 2))]), title: "2 steps, one Confirm")
        defer { done() }
        #expect(engine.quoteView == nil)
    }

    @Test func editingAChainsFirstMoveKeepsTheRestBehindIt() async throws {
        wallet(["solana": [Engine.row("USDC", 20, usd: 20)]])
        let first = WalletCommand.buy(sol, usdc: 5), then = WalletCommand.solanaTo("arbitrum", 2)
        let id = chainCard(.chain([.wallet(first), .wallet(then)]), title: "2 steps, one Confirm")
        defer { done() }
        try await engine.editStep(board: id, amount: "3", way: nil)
        guard case .chain(let list)? = engine.pending else { Issue.record("not a chain: \(String(describing: engine.pending))"); return }
        #expect(list.count == 2)
        if case .wallet(let c)? = list.first { #expect(c == .buy(sol, usdc: 3)) } else { Issue.record("first isn't a move") }
        if case .wallet(let c)? = list.last { #expect(c == then) } else { Issue.record("the rest is gone") }
        // A new card with the chain's title, the first line saying the new amount.
        #expect(engine.card?.id != id && engine.card?.title == "2 steps, one Confirm")
        #expect(engine.card?.lines.first?.hasPrefix("1. ") == true && engine.card?.lines.first?.contains("3") == true, "\(engine.card?.lines ?? [])")
    }

    @Test func aRequestThatDrewNothingKeepsTheCanvasItWasAskedOn() {
        let chart = Board(kind: "chart", title: "BTC")
        #expect(Engine.standing([chart])?.id == chart.id)
        let rates = Board(kind: "result", title: "Rates", data: BlockData(rates: [BlockData.RateRow(place: "Kamino", asset: "USDC", apy: 5, how: nil, note: "")]))
        // Rates over a chart, as the Mac's canvasBubbles; nothing standing, nothing kept.
        #expect(Engine.standing([chart, rates])?.id == rates.id)
        #expect(Engine.standing([]) == nil)
        // Titled as the Mac's activity: the chart's coin, the rates' title.
        #expect(Engine.boardTitle([rates]) == "Rates")
    }
}

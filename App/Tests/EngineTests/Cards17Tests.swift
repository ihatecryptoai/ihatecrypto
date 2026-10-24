@testable import DesktopEngine
import Foundation
import WalletCore
import IhatecryptoCore
import Testing
@testable import TradeExecution

/// Wave 16's cards: a way picked in a step's details redraws the route at once; the chat's quote line says a NEAR
/// Intents send's quote in its own words and warns when fees take more than a tenth; Confirm is held only where the Mac
/// holds it; a request's History card is the canvas frozen as it was handled. In-process: its own store folder,
/// balances and quotes set by hand, nothing sent and no network.
@MainActor @Suite(.serialized) struct Cards17Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards17-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    private var engine: Engine { Self.engine }

    private func wallet(_ networks: [String: [PortfolioView.Row]]) {
        engine.settings.language = "en"
        engine.portfolio = PortfolioView(networks: networks.map { PortfolioView.Network(key: $0.key, name: WalletCommand.placeName($0.key), usd: 0, rows: $0.value) },
                                         updatedAt: Date())
        engine.wayQuotes = nil
        engine.jupiterShown = nil
        engine.quotedAt = nil
        engine.unpriced = nil
        engine.routeWay = nil
        engine.status = nil
    }

    /// A single move's card pinned by hand, as show() draws it (show() would ask for its quotes): its id.
    private func card(_ c: WalletCommand) -> String {
        let id = engine.pin(Board(id: UUID().uuidString, kind: "move", title: c.summary, steps: engine.steps(for: .wallet(c), title: "", lines: []), state: "waiting"))
        engine.pending = .wallet(c)
        engine.card = Card(id: id, title: c.summary, lines: [], confirm: "Confirm")
        return id
    }

    private func done() {
        if let id = engine.card?.id { engine.boards.removeAll { $0.id == id } }
        engine.pending = nil
        engine.card = nil
        engine.routeWay = nil
    }

    @Test func aWayPickedInTheStepsDetailsRedrawsTheRoute() async throws {
        wallet(["solana": [Engine.row("USDC", 20, usd: 20)]])
        let buy = WalletCommand.buyOn("base", token: "AERO", usdc: 5)
        let id = card(buy)
        defer { done() }
        let labels = { (engine.boards.first { $0.id == id }?.steps ?? []).map(\.label) }
        #expect(!labels().contains("RELAY + UNISWAP") && !labels().contains("APPROVE · EXACT"), "\(labels())")
        // Uniswap picked alone, no amount changed: the card says the way Confirm will take, with Uniswap's exact approve.
        try await engine.editStep(board: id, amount: nil, way: "uniswap")
        #expect(labels().contains("RELAY + UNISWAP") && labels().contains("APPROVE · EXACT"), "\(labels())")
        try await engine.editStep(board: id, amount: nil, way: "relay")
        #expect(labels().contains("RELAY SWAP") && !labels().contains("APPROVE · EXACT"), "\(labels())")
    }

    @Test func theQuoteLineSaysANearSendInItsOwnWordsAndWarnsOfFees() {
        let near = "≈ 25.31 USDC for $24.80 of it · fees ≈ $0.51 (2%) · ≈ 2 min"
        #expect(Engine.swapLine(.sendToken(name: "Alice", token: "BTC", amount: Decimal(string: "0.0003")!, network: nil), gets: near) == near)
        // $1 in, $0.82 worth arriving: fees take more than a tenth.
        let small = Engine.swapLine(.buyOn("base", token: "AERO", usdc: 1), gets: "0.5 AERO", worth: Decimal(string: "0.82")!)
        #expect(small.contains("⚠️ fees take ≈ $0.18 (18%)"), "\(small)")
        // A tenth or less: no warning; nor without a worth.
        #expect(!Engine.swapLine(.buyOn("base", token: "AERO", usdc: 10), gets: "5 AERO", worth: Decimal(string: "9.5")!).contains("fees take"))
        #expect(!Engine.swapLine(.buy(SpotAsset.all.first { $0.symbol == "SOL" }!, usdc: 1), gets: "0.005 SOL").contains("fees take"))
        // A cross-network quote's own «≈ $X» is its worth.
        #expect(Engine.swapLine(.crossSwap(from: "USDC", on: "solana", to: "ETH", at: "base", amount: 2), gets: "0.0004 ETH ≈ $1.50").contains("fees take ≈ $0.50"))
        // A sale by contract names its ticker once known.
        let contract = "0x2e8c0000000000000000000000000000000e1e18"
        #expect(Engine.swapLine(.sellOn("base", token: contract, amount: 100, keep: false), gets: "1.23 USDC", sold: "AI").contains("1 AI ≈ $0.0123"))
    }

    @Test func confirmIsHeldOnlyWhereTheMacHoldsIt() {
        wallet(["solana": [Engine.row("USDC", 20, usd: 20)]])
        // Hyperliquid's spot book: its quote shows on the card, Confirm isn't held for it.
        #expect(!engine.priced(.hlSpotBuy(symbol: "HYPE", usdc: 5)) && !engine.priced(.hlSpotSell(symbol: "HYPE", amount: 1)))
        #expect(!engine.priced(.swapOn("hyperliquid", from: "HYPE", to: "USDC", amount: 1)))
        // A quote asked and not given: Refresh and «over 15 s old», not «Quoting…» for as long as it fails.
        let buy = WalletCommand.buyOn("base", token: "AERO", usdc: 5)
        _ = card(buy)
        defer { done() }
        #expect(engine.quoteView?.state == "quoting")
        engine.quoteFailed(buy)
        #expect(engine.quoteView?.state == "stale")
        // The step's details still say it's being asked, not an empty list.
        #expect(engine.stepEdit?.quotes == nil)
        // A swap Jupiter can't price: Confirm as always.
        engine.unpriced = buy
        #expect(engine.quoteView == nil)
    }

    @Test func aRequestsHistoryCardIsTheCanvasFrozenAsItWasHandled() {
        wallet(["solana": [Engine.row("USDC", 20, usd: 20)]])
        engine.intent = TradeIntent()
        // A rates board shown by an earlier message, then a follow-up: the Mac clears the rates at the next message.
        let earlier = Date().addingTimeInterval(-60)
        engine.pin(Board(id: UUID().uuidString, kind: "result", title: "Rates · USDC", at: earlier,
                         data: BlockData(rates: [.init(place: "Kamino", asset: "USDC", apy: 5, note: "")])))
        let follow = Message(fromUser: true, text: "which is safest?")
        engine.snapshotCanvas(for: follow)
        #expect(HistoryStore.saved.shown?[follow.id] == HistoryTabs.Snapshot())
        // A chart standing on the canvas with no plan under way: the follow-up is that chart's.
        engine.pin(Board(id: UUID().uuidString, kind: "chart", title: "SOL", at: earlier))
        let next = Message(fromUser: true, text: "and now?")
        engine.snapshotCanvas(for: next)
        #expect(HistoryStore.saved.shown?[next.id]?.title == "Chart · SOL", "\(String(describing: HistoryStore.saved.shown?[next.id]))")
        // Rates this very message showed win over the chart.
        let ask = Message(fromUser: true, text: "best rates for USDC", at: Date().addingTimeInterval(-1))
        engine.pin(Board(id: UUID().uuidString, kind: "result", title: "Rates · USDC", data: BlockData(rates: [.init(place: "Jupiter Lend", asset: "USDC", apy: 6, note: "")])))
        engine.snapshotCanvas(for: ask)
        #expect(HistoryStore.saved.shown?[ask.id]?.title == "Rates" && HistoryStore.saved.shown?[ask.id]?.bubbles.first?.kind == "yield")
        // A card waiting for Confirm: its title and route, frozen.
        let buy = WalletCommand.buyOn("base", token: "AERO", usdc: 5)
        let id = card(buy)
        defer { done() }
        let waiting = Message(fromUser: true, text: "is that safe?")
        engine.snapshotCanvas(for: waiting)
        let snap = HistoryStore.saved.shown?[waiting.id]
        #expect(snap?.title == engine.card?.title && snap?.bubbles.count == engine.boards.first { $0.id == id }?.steps?.count)
    }
}

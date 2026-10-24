@testable import DesktopEngine
import Foundation
import WalletCore
import IhatecryptoCore
import EVMChain
import Testing
@testable import TradeExecution

/// Wave 14's cards: a chain's card keeps every step, its title and a footer true to its route when its first move's
/// quote lands — a Merkl claim's as a swap's. In-process: this test's own store folder, balances set by hand, nothing sent.
@MainActor @Suite(.serialized) struct Cards15Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards15-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    // Public test data only: a Hardhat address as the reward token.
    static let aero = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"

    private var engine: Engine { Self.engine }

    private func wallet(_ networks: [String: [PortfolioView.Row]]) {
        engine.settings.language = "en"
        engine.portfolio = PortfolioView(networks: networks.map { PortfolioView.Network(key: $0.key, name: WalletCommand.placeName($0.key), usd: 0, rows: $0.value) },
                                         updatedAt: Date())
        engine.wayQuotes = nil
        engine.claimOwed = nil
        engine.status = nil
    }

    /// A chain's card pinned by hand, as show() draws it (show() would ask for its quotes): its id.
    private func chainCard(_ pending: Pending, title: String) -> String {
        let id = engine.pin(Board(id: UUID().uuidString, kind: "move", title: title, lines: [], steps: engine.steps(for: pending, title: "", lines: []), state: "waiting"))
        engine.pending = pending
        engine.card = Card(id: id, title: title, confirm: "Confirm")
        engine.noteRoute(id, pending)
        return id
    }

    private func done(_ id: String) { engine.pending = nil; engine.card = nil; engine.boards.removeAll { $0.id == id } }

    @Test func aChainLedByAClaimKeepsItsLaterStepsWhenMerklAnswers() {
        wallet(["base": [Engine.row("USDC", 20, usd: 20)]])
        let claim = WalletCommand.claimRewards, then = WalletCommand.sellOn("base", token: "AERO", amount: nil)
        let id = chainCard(.chain([.wallet(claim), .wallet(then)]), title: "2 steps, one Confirm")
        defer { done(id) }
        // What quoteRoute does once Merkl answers for a chain: the claim becomes its first move's route.
        let reward = Merkl.Reward(chainId: 8453, token: Self.aero, symbol: "AERO", decimals: 18, amount: 2_000_000_000_000_000_000, claimed: 0,
                                  proofs: [], root: "", price: Decimal(string: "1.5")!)
        engine.claimOwed = (id, claim, [reward])
        #expect(engine.redrawChain(id))
        let b = engine.boards.first { $0.id == id }
        let steps = b?.steps ?? []
        #expect(steps.first?.label == "CLAIM · MERKL ON BASE", "\(steps.map(\.label))")
        // The sell after it is still on the card, pale, and the title is still the chain's.
        #expect(steps.filter { $0.status == "then" }.count >= 2 && steps.last?.label == "TO · SOLANA", "\(steps.map { ($0.label, $0.status ?? "") })")
        #expect(b?.title == "2 steps, one Confirm" && engine.card?.title == "2 steps, one Confirm")
        #expect(b?.note?.contains("Merkl") == true, "\(b?.note ?? "")")
        // Another card's claim isn't this one's route.
        engine.claimOwed = ("another card", claim, [reward])
        #expect(engine.routeSteps(claim) == nil)
    }

    @Test func aChainsFooterFollowsTheWayItsFirstMoveWasQuoted() {
        wallet(["base": [Engine.row("USDC", 20, usd: 20)]])
        let first = WalletCommand.crossSwap(from: "USDC", on: "base", to: "SOL", at: "solana", amount: 5), then = WalletCommand.buyOn("base", token: "AERO", usdc: 2)
        let id = chainCard(.chain([.wallet(first), .wallet(then)]), title: "2 steps, one Confirm")
        defer { done(id) }
        let note = { engine.boards.first { $0.id == id }?.note ?? "" }
        #expect(note().contains("Relay's solver"), "\(note())")
        // NEAR Intents takes it: the footer says the 1Click deposit, as the steps now do.
        engine.wayQuotes = (first, [RouteEdit.Way(way: "NEAR Intents", gets: "≈ 0.031 SOL", worth: "")])
        engine.redrawRoute(id, first)
        #expect(note().contains("1Click") && !note().contains("Relay's solver"), "\(note())")
        let b = engine.boards.first { $0.id == id }
        #expect(b?.title == "2 steps, one Confirm" && (b?.steps ?? []).contains { $0.status == "then" })
    }

    @Test func historyKeepsARatesBoardAsSmallRateCards() {
        let rates = (1...10).map { BlockData.RateRow(place: "Place \($0)", asset: "USDC", apy: Decimal($0), how: nil, note: "") }
        let b = Board(kind: "result", title: "Rates", data: BlockData(rates: rates))
        let bubbles = Engine.bubbles(b)
        #expect(bubbles.count == 8 && bubbles.allSatisfy { $0.kind == "yield" }, "\(bubbles.map(\.kind))")
        #expect(bubbles.first.map { [$0.label, $0.title] + $0.lines } == ["USDC", "Place 1", "1%"])
        // A route's steps are still each a card, anything else one card.
        #expect(Engine.bubbles(Board(kind: "chart", title: "BTC")).map(\.label) == ["Chart"])
    }
}

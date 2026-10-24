@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Wave 5's cards: the chart board's live parts, a held token valued as the wallet values it, stablecoin and NEAR rows,
/// a step attached on its own, Add block's price alert for the plan, a key's address before it's saved, a task's facts.
/// In-process, with balances set by hand; public test data only (Hardhat's account #0 and its published key).
@MainActor @Suite(.serialized) struct Cards6Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards6-\(UUID().uuidString)").path)
        return Engine(out: Outbox())
    }()
    private var engine: Engine { Self.engine }

    @Test func aChartBoardReadsItsMarketLevelsAndWhatIsHeld() {
        engine.settings.language = "en"
        engine.markets = ["SOL": Market(name: "SOL", markPx: 200, szDecimals: 2, maxLeverage: 20)]
        engine.alerts = [PriceAlert(coin: "SOL", above: false, price: 180)]
        engine.portfolio = PortfolioView(networks: [PortfolioView.Network(key: "solana", name: "Solana", usd: 0,
                                                                          rows: [Engine.row("SOL", 2, chain: "solana", contract: CostBasis.wsol)])],
                                         updatedAt: Date())
        let x = engine.chartExtras("SOL", network: nil)
        #expect(x.tiles.map(\.title) == ["Volume 24h", "Funding 1h", "Max leverage"] && x.tiles.last?.value == "20×", "\(x.tiles)")
        #expect(x.levels.map(\.label) == ["Alert"] && x.levels.first?.dashed == true)
        // SOL's home is Solana: held there at Hyperliquid's price even from a chart the chat opened.
        #expect(x.held?.usd == 400 && x.held?.how == "valued at Hyperliquid's price", "\(String(describing: x.held))")
        // Only chart boards get them, read for the window, never kept.
        let chart = Board(kind: "chart", title: "SOL · 24 h", chart: ChartLine(title: "SOL · 24 h", closes: [1, 2], low: "$1", high: "$2", change: "+1%", coin: "SOL"))
        #expect(engine.withChartExtras([chart, Board(kind: "result", title: "x")]).first?.chart?.extras?.held != nil)
        engine.alerts = []
    }

    @Test func aStablecoinKeepsItsContractAndNearHasACostKey() {
        let usdt = Engine.row("USDT", 5, usd: 5, chain: "ethereum", contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7")
        // No P/L for a dollar, but its row still opens and trades by its contract.
        #expect(usdt.key == nil && usdt.contract == "0xdAC17F958D2ee523a2206206994597C13D831ec7")
        #expect(Engine.row("NEAR", 3, chain: "near", contract: "native").key == "near:native")
    }

    @Test func aStepGoesToTheChatOnItsOwn() throws {
        var b = Board(kind: "move", title: "Move 5 USDC to Base")
        b.steps = [Board.Step(label: "FROM · SOLANA", title: "5 USDC", lines: ["free\t20 USDC"]), Board.Step(label: "TO · BASE", title: "≈ 4.99 USDC")]
        let step = try #require(engine.stepBoard(b, 1))
        #expect(step.title == "Step 2 · TO · BASE · ≈ 4.99 USDC")
        #expect(step.lines.first?.contains("of the route «Move 5 USDC to Base»") == true, "\(step.lines)")
        #expect(engine.stepBoard(b, 5) == nil)
    }

    @Test func addBlocksPriceAlertIsForThePlanOnTheCanvas() {
        engine.markets = ["ETH": Market(name: "ETH", markPx: 2000, szDecimals: 4, maxLeverage: 25)]
        engine.positions = []
        engine.intent = TradeIntent()
        #expect(engine.planAlertExample == nil)
        engine.intent = IntentParser.apply("long ETH risk $5", to: TradeIntent(), markets: ["ETH"]) ?? TradeIntent()
        #expect(engine.planAlertExample?.hasPrefix("when ETH is below 1,960: long ETH") == true, "\(engine.planAlertExample ?? "nil")")
        engine.intent = TradeIntent()
    }

    @Test func aPastedKeyShowsItsAddressAndNothingIsKept() throws {
        // Hardhat's account #0: its key is published with Hardhat for tests.
        let r = try engine.keyAddress("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        #expect(r["chain"] == "EVM · Hyperliquid" && r["address"] == "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "\(r)")
        #expect(throws: (any Error).self) { try engine.keyAddress("not a key") }
        #expect(Engine.answersOnly.isSuperset(of: ["keyAddress", "phraseAccounts", "multisigFees"]))
    }

    @Test func aTaskSaysWhatItBuysAndWhatItSpreads() {
        #expect(ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date(), every: nil).buysCoins)
        #expect(ScheduledTask(steps: ["spread USDC across the best rates"], next: Date(), every: nil).spreadsUSDC)
        #expect(!ScheduledTask(steps: ["check: is SOL above 300?"], next: Date(), every: nil).movesMoney)
    }
}

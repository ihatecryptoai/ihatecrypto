@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// What the model reads and may use, as the Mac's (audit5 G2, G8, R4, E4): a contract a lookup found only in a contract
/// call (checked again on its card) or a block that shows something; the waiting card's route and how the moves ended,
/// never a saved address; one chart a canvas; the held amount for «all X». In-process, balances set by hand.
@MainActor @Suite(.serialized) struct Model7Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("model7-\(UUID().uuidString)").path)
        return Engine(out: Outbox())
    }()
    private var engine: Engine { Self.engine }
    // Public test data only: Hardhat's deployer contract and account #3, Solana's test phrase address.
    static let vault = "0x5FbDB2315678afecb367f032d93F642f64180aa3"
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"

    @Test func aFoundContractOnlyInACallOrADisplay() {
        let found = [Self.vault]
        let call = "call arbitrum \(Self.vault) deposit(uint256,address) 10e6 me approving 10 USDC"
        #expect(ChatRouter.addressesAllowed(call, userText: "put 10 USDC into Fluid on Arbitrum", known: [], found: found, displays: Engine.foundAllowed, markets: []))
        // Before this, only displays could: the form's own path («find the contract first») ended refused.
        #expect(!ChatRouter.addressesAllowed(call, userText: "put 10 USDC into Fluid on Arbitrum", known: [], found: found, displays: Engine.displays, markets: []))
        // A buy at a contract the user never wrote stays refused, alone or as a chain's step after a call.
        let buy = "buy token \(Self.vault) on base for 5 USDC"
        #expect(!ChatRouter.addressesAllowed(buy, userText: "buy something good", known: [], found: found, displays: Engine.foundAllowed, markets: []))
        #expect(!ChatRouter.addressesAllowed("\(call) then \(buy)", userText: "x", known: [], found: found, displays: Engine.foundAllowed, markets: []))
    }

    @Test func heldTokensAreKnownByTheirContract() {
        var share = Engine.row("fUSDC", 12, usd: 12, chain: "arbitrum", contract: Self.vault)
        share.section = "tokens"
        engine.portfolio = PortfolioView(networks: [.init(key: "arbitrum", name: "Arbitrum", usd: 12, rows: [share, Engine.row("ETH", 1, usd: 3000, chain: "arbitrum", contract: "native")])],
                                         updatedAt: Date())
        #expect(engine.knownContracts.contains(Self.vault) && !engine.knownContracts.contains("native"))
        #expect(engine.portfolioFacts().contains { $0.contains("fUSDC (\(Self.vault))") }, "\(engine.portfolioFacts())")
        #expect(!engine.portfolioFacts().contains { $0.contains("ETH (native)") })
    }

    @Test func theModelReadsTheCardsRouteAndHowMovesEnded() {
        engine.book = [SignerAddress(name: "Mom", network: "base", address: Self.mom, memo: "")]
        let done = engine.pin(Board(kind: "move", title: "Send 1 USDC to Mom", state: "done"))
        _ = engine.pin(Board(kind: "move", title: "Send 2 USDC to Mom", state: "unknown"))
        let id = engine.pin(Board(kind: "move", title: "Send 5 USDC to Mom", lines: ["To \(Self.mom)"],
                                  steps: [Board.Step(label: "FROM · BASE", title: "5 USDC", lines: ["fee ≈ $0.01"],
                                                     money: Board.Money(place: "Base", symbol: "USDC", was: "20", after: "15")),
                                          Board.Step(label: "TO · MOM", title: "≈ 5 USDC")], state: "waiting"))
        engine.card = Card(id: id, title: "send 5 USDC to Mom", confirm: "Confirm")
        defer { engine.card = nil }
        let facts = engine.moveFacts()
        let waiting = facts.first { $0.hasPrefix("waiting for Confirm") } ?? ""
        #expect(waiting.contains("FROM · BASE 5 USDC (Base 20 → 15 USDC) (fee ≈ $0.01) → TO · MOM ≈ 5 USDC"), "\(waiting)")
        // A saved address is read as its name, never as the address.
        #expect(!facts.joined().contains(Self.mom) && waiting.contains("To Mom"), "\(facts)")
        #expect(facts.contains("just finished: Send 2 USDC to Mom — sent, no outcome back"), "\(facts)")
        #expect(facts.contains("done earlier on this canvas: Send 1 USDC to Mom — done"), "\(facts)")
        _ = done
    }

    @Test func aCanvasHasOneChart() {
        func line(_ coin: String) -> ChartLine { ChartLine(title: "\(coin) · Day", closes: [1, 2], low: "1", high: "2", change: "+100%", coin: coin) }
        let first = engine.placeChart(line("BTC"))
        let second = engine.placeChart(line("ETH"))
        #expect(first == second)
        #expect(engine.boards.filter { $0.kind == "chart" }.count == 1 && engine.boards.first?.chart?.coin == "ETH")
    }

    @Test func allIsTheHeldAmountWhereItsHeldInOnePlace() {
        var ray = Engine.row("RAY", Decimal(string: "1.0412")!, usd: 2, chain: "solana", contract: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R")
        ray.section = "tokens"
        engine.portfolio = PortfolioView(networks: [.init(key: "solana", name: "Solana", usd: 2, rows: [ray, Engine.row("USDC", 5, usd: 5)])], updatedAt: Date())
        #expect(engine.withAmounts("sell all RAY") == "sell 1.0412 RAY")
        // USDC is on every network: «all» stays words, as on the Mac.
        #expect(engine.withAmounts("send all USDC to Mom") == "send all USDC to Mom")
        #expect(engine.withAmounts("sell all BONK") == "sell all BONK")
    }

    @Test func aCanvasRowShowsItsPlansCoinOrItsNote() {
        let row = engine.canvasRows.first { $0.id == engine.canvasID }
        #expect(row?.coin == nil && row?.note == false)
        engine.intent.asset = "SOL"
        #expect(engine.canvasRows.first { $0.id == engine.canvasID }?.coin == "SOL")
        engine.intent = TradeIntent()
    }

    @Test func useLoginWithNoLoginOnThisPCSaysSo() {
        #expect(throws: (any Error).self) { try engine.useLogin(provider: "openrouter") }
    }
}

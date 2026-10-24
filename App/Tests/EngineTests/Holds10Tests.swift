@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing
@testable import TradeExecution

/// What one Confirm carries out (audit8 R8-1: never a card nobody saw), the capital the money rules measure (M2), and the
/// smaller items behind them. In-process, everything set by hand; nothing goes to the network or the signer.
@MainActor @Suite(.serialized) struct Holds10Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("holds10-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    // Public test data only: Hardhat's accounts, the abandon-about phrase's Solana address.
    static let safe = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let newer = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
    static let sol = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    static let evm = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

    private func fresh() {
        engine.settings.language = "en"
        engine.pending = nil
        engine.card = nil
        engine.chainState = ChainState()
        engine.book = []
        engine.multisigs = []
    }

    /// R8-1: a confirmed run (the steps held behind a card, run on its Confirm) carries out only what the Mac's runSteps
    /// would — a move, a transfer, a position, an address — and leaves a contract call, a trade plan and a recurring buy's
    /// first buy waiting on their own cards.
    @Test func aConfirmCarriesOutOnlyWhatTheMacsRunStepsWould() throws {
        let call = try #require(WalletCommand.parse("call base \(Self.safe) claim()"))
        #expect(!Engine.carriedOnChain(.wallet(call)))
        #expect(!Engine.carriedOnChain(.recurring(.buy(.usdc, usdc: 5), days: 7)))
        #expect(!Engine.carriedOnChain(nil))
        #expect(Engine.carriedOnChain(.wallet(.sendSol(name: "Mom", amount: 1))))
        #expect(Engine.carriedOnChain(.transfer(SignerAddress(name: "Mom", network: "base", address: Self.safe), 5)))
        #expect(Engine.carriedOnChain(.position(.close(1), coin: "ETH")))
        #expect(Engine.carriedOnChain(.address(SignerAddress(name: "Mom", network: "base", address: Self.safe))))
    }

    /// R8-2: a card a chain stops at holds the steps after it (its Confirm names them, and they run once it's through) —
    /// any card, not only a contract call's; with no card nothing is held.
    @Test func anyCardAChainStopsAtHoldsTheStepsAfterIt() {
        fresh()
        engine.holdAfterCall(["chart SOL"])
        #expect(engine.chainState.afterCall == nil)
        engine.show(.address(SignerAddress(name: "Mom", network: "base", address: Self.safe)), title: "Save Mom")
        engine.holdAfterCall(["chart SOL", "send 1 USDC to Mom"])
        let held = engine.chainState.afterCall
        #expect(held?.board == engine.card?.id && held?.rest == ["chart SOL", "send 1 USDC to Mom"])
        #expect(engine.card?.lines.last?.hasPrefix("Then, on this Confirm, once it’s through: chart SOL → send 1 USDC to Mom") == true, "\(engine.card?.lines ?? [])")
        engine.cancelPending()
    }

    /// M2: the money rules measure against the Mac's `equity` — everything on show but the networks' own coins (SOL, ETH,
    /// the other gas coins); NEAR counts, as on the Mac.
    @Test func theRulesCapitalLeavesOutTheGasCoins() {
        engine.portfolio = PortfolioView(networks: [
            .init(key: "solana", name: "Solana", usd: 1800, rows: [Engine.row("USDC", 800, usd: 800), Engine.row("SOL", 5, usd: 1000, chain: "solana", contract: CostBasis.wsol)]),
            .init(key: "base", name: "Base", usd: 230, rows: [Engine.row("USDC", 200, usd: 200), Engine.row("ETH", Decimal(string: "0.01")!, usd: 30, chain: "base", contract: "native")]),
            .init(key: "near", name: "NEAR", usd: 50, rows: [Engine.row("NEAR", 20, usd: 50, chain: "near", contract: "native")]),
        ], total: 2080, updatedAt: Date())
        #expect(engine.ruleEquity == 1050)
        engine.portfolio = PortfolioView(networks: [], updatedAt: Date())
        #expect(engine.ruleEquity == nil)
    }

    /// G22: a task that only changes a position runs on the automation wallet by itself, as the Mac's confirmed runSteps.
    @Test func aPositionOnlyTaskIsTheAutomationWallets() {
        let task = ScheduledTask(steps: ["move stop to breakeven"], next: Date(), every: .days(1))
        #expect(task.managesPosition && !task.movesMoney)
        #expect(!ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date(), every: .days(1)).managesPosition)
    }

    /// T5: with two positions open and no coin named, «stop 2%» is about the coin just traded, as on the Mac.
    @Test func theCoinJustTradedIsTheOneAPositionCommandMeans() {
        fresh()
        engine.intent = TradeIntent()
        engine.positions = [HyperliquidAccount.Position(coin: "ETH", size: 1, entry: 2000, unrealizedPnl: 0, liquidation: nil),
                            HyperliquidAccount.Position(coin: "BTC", size: Decimal(string: "0.01")!, entry: 60000, unrealizedPnl: 0, liquidation: nil)]
        engine.tradedPlan = nil
        #expect(engine.targetCoin(for: "move stop to breakeven") == nil)
        var traded = TradeIntent()
        traded.asset = "ETH"
        engine.tradedPlan = traded
        #expect(engine.targetCoin(for: "move stop to breakeven") == "ETH")
        engine.positions = []
        engine.tradedPlan = nil
    }

    /// T4: another wallet here is found by its Solana address, as the Mac's ownWallet: on Base it's said, never sent.
    @Test func anotherWalletHereIsItsSolanaAddress() {
        fresh()
        var s = SignerReply(ok: true)
        s.wallets = [SignerWallet(id: "1", label: "Main", kind: "phrase", solana: Self.sol, evm: Self.evm, near: nil),
                     SignerWallet(id: "2", label: "Test", kind: "phrase", solana: Self.sol, evm: Self.safe, near: nil)]
        s.activeWallet = "1"
        engine.status = s
        let found = engine.entry("Test", network: "base")
        #expect(found?.network == "solana" && found?.address == Self.sol, "\(String(describing: found))")
        engine.prepareTransfer(10, to: "Test", network: "base")
        #expect(engine.card == nil)
        #expect(engine.messages.last?.text.contains("is a solana address: it can't receive on base") == true, "\(engine.messages.last?.text ?? "")")
        engine.status = nil
    }

    /// T9: an older build's own «Multisig» book entry leaves the name to the newest Safe, as the Mac's multisigEntry.
    @Test func anOlderMultisigEntryGivesWayToTheNewestSafe() {
        fresh()
        engine.book = [SignerAddress(name: "Multisig", network: "base", address: Self.safe)]
        engine.multisigs = [Multisig(address: Self.safe.lowercased(), keys: [], threshold: 1, salt: 0, networks: ["base"]),
                            Multisig(address: Self.newer.lowercased(), keys: [], threshold: 1, salt: 1, networks: ["base"])]
        #expect(engine.entry("Multisig", network: "base")?.address == Self.newer.lowercased())
        // A Safe the person saved under a name of their own stays theirs, on its networks.
        engine.book = [SignerAddress(name: "Treasury", network: "base", address: Self.safe)]
        #expect(engine.entry("Treasury", network: "base")?.address == Self.safe)
        #expect(engine.entry("Treasury", network: "arbitrum") == nil)
        fresh()
    }
}

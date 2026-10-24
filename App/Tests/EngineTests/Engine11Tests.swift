@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// What a held run may confirm (audit9 R9-3: only the card it drew, with nothing scheduled in its middle), Cancel taking
/// the held steps with it (R9-2), the capital the model is told (M2′), multisig steps in a chain (G27) and the smaller
/// items. In-process, everything set by hand; nothing goes to the network or the signer.
@MainActor @Suite(.serialized) struct Engine11Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("engine11-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    // Public test data only: Hardhat's account #3.
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"

    private func fresh() {
        engine.settings.language = "en"
        engine.settings.schedulePaused = nil
        engine.pending = nil
        engine.card = nil
        engine.awaitingAmount = nil
        engine.chainState = ChainState()
        engine.tasks = []
    }

    private func dueTask() -> ScheduledTask {
        // An alert's steps go at once (no waiting for the person to step away): the case R9-3 is about.
        var t = ScheduledTask(steps: ["send 50 USDC to Mom"], next: Date().addingTimeInterval(-60), every: nil)
        t.because = "SOL went above $1"
        return t
    }

    /// R9-3 + T12: while held steps run (or are about to, right after their card's Confirm) and while a «How much?» is
    /// open, no scheduled run starts — so no scheduled card can land where a held step's Confirm would take it.
    @Test func nothingScheduledStartsWhileHeldStepsRunOrAHowMuchWaits() async {
        fresh()
        let task = dueTask()
        engine.tasks = [task]
        engine.chainState.runningHeld = true
        await engine.runDueTasks()
        #expect(engine.card == nil && engine.tasks.first?.next == task.next, "a scheduled run started in a held run's middle")
        engine.chainState.runningHeld = false
        engine.awaitingAmount = .buy(.usdc, usdc: nil)
        await engine.runDueTasks()
        #expect(engine.card == nil && engine.tasks.first?.next == task.next, "a scheduled run started over a «How much?»")
        engine.awaitingAmount = nil
        engine.tasks = []
    }

    /// R9-3: each card drawn counts, so a held step takes only the one card it drew itself.
    @Test func everyCardDrawnIsCounted() {
        fresh()
        let before = engine.chainState.cardsDrawn
        engine.show(.address(SignerAddress(name: "Mom", network: "base", address: Self.mom)), title: "Save Mom")
        engine.show(.address(SignerAddress(name: "Dad", network: "base", address: Self.mom)), title: "Save Dad")
        #expect(engine.chainState.cardsDrawn == before + 2)
        engine.cancelPending()
    }

    /// R9-2: Cancel on the card the steps were held behind takes them with it; another card's Cancel leaves them.
    @Test func cancelTakesTheStepsHeldBehindItsCard() throws {
        fresh()
        engine.show(.address(SignerAddress(name: "Mom", network: "base", address: Self.mom)), title: "Save Mom")
        let id = try #require(engine.card?.id)
        engine.chainState.afterCall = (id, engine.canvasID, ["chart SOL"], Date())
        engine.cancelPending(card: "another")
        #expect(engine.chainState.afterCall != nil)
        engine.cancelPending(card: id)
        #expect(engine.chainState.afterCall == nil && engine.card == nil)
    }

    /// T11 and G27: a recurring buy's first buy goes on the earlier Confirm, as the Mac's carryOut; a multisig, made or
    /// paid from, waits on its own card.
    @Test func whatAHeldRunCarriesOut() throws {
        #expect(Engine.carriedOnChain(.recurring(.buy(.usdc, usdc: 5), days: 7)))
        let make = try #require(WalletCommand.parse("make a multisig 2 of 3"))
        guard case .multisigCreate = make else { Issue.record("not a multisig: \(make)"); return }
        #expect(!Engine.carriedOnChain(.wallet(make)))
        #expect(!Engine.carriedOnChain(.wallet(.multisigSend(token: "USDC", amount: 10, to: Self.mom, network: "base"))))
    }

    /// M2′: the model is told the capital the risk rules measure — the gas coins left out, as the Mac's equity.
    @Test func theModelIsToldTheRulesCapital() {
        fresh()
        engine.portfolio = PortfolioView(networks: [
            PortfolioView.Network(key: "solana", name: "Solana", usd: 1800, rows: [Engine.row("USDC", 800, usd: 800), Engine.row("SOL", 5, usd: 1000, chain: "solana", contract: CostBasis.wsol)]),
        ], total: 1800, updatedAt: Date())
        #expect(engine.ruleEquity == 800)
        #expect(engine.routerContext().capital == Fmt.usd(800), "\(engine.routerContext().capital ?? "nil")")
        engine.portfolio = PortfolioView(networks: [], updatedAt: Date())
    }
}

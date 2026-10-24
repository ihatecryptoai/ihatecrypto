@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// The tenth audit's last items: a card whose own move fails drops the steps held behind it (R10-1), an address save that
/// comes first in a chain holds the rest (G28), a first recurring buy a chain's card named starts on that Confirm (T16),
/// and no scheduled run starts over half-typed text (T18). In-process, no wallet: every move fails at «Add a wallet first».
@MainActor @Suite(.serialized) struct Final12Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("final12-\(UUID().uuidString)").path)
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
        Engine.composing = false
    }

    /// R10-1: the card's own move fails, so what was held behind it goes (the Mac's continueTask drops taskQueue whatever
    /// the outcome) and the model may carry on.
    @Test func aCardThatFailsDropsTheStepsHeldBehindIt() async throws {
        fresh()
        engine.show(.transfer(SignerAddress(name: "Mom", network: "base", address: Self.mom), 1), title: "Send 1 USDC to Mom")
        let id = try #require(engine.card?.id)
        engine.chainState.afterCall = (id, engine.canvasID, ["clear the plan"], Date())
        await engine.confirm(card: id)
        #expect(engine.chainState.afterCall == nil, "the held steps outlived a card that didn't go through")
        #expect(!engine.chainState.runningHeld)
        #expect(engine.boards.first { $0.id == id }?.state == "failed")
    }

    /// G28: «save … as Phone on base, then …»: the save's card waits, the steps after it held behind its Confirm.
    @Test func anAddressSaveFirstInAChainHoldsTheRest() async throws {
        fresh()
        let handled = await engine.chain(["save \(Self.mom) as Phone on base", "clear the plan"], note: nil)
        #expect(handled)
        guard case .address(let entry)? = engine.pending else {
            Issue.record("no save card: \(engine.messages.last?.text ?? "")")
            return
        }
        #expect(entry.name == "Phone")
        #expect(engine.chainState.afterCall?.board == engine.card?.id && engine.chainState.afterCall?.rest == ["clear the plan"])
        engine.cancelPending()
        #expect(engine.chainState.afterCall == nil)
    }

    /// T16: a chain's card named «buy SOL for 5 USDC every week»: its Confirm starts the schedule and the first buy, with
    /// no second card waiting; the schedule stays when the first buy fails (here: no wallet), as the Mac's firstBuyOf.
    @Test func aRecurringBuyAChainsCardNamedStartsOnItsConfirm() async throws {
        fresh()
        // The app's own way picked already (as after the two ways' cards): its card, not the offer, comes up.
        engine.dcaChoice = WalletCommand.parse("buy SOL for 5 USDC every week")
        engine.show(.chain([.said("buy SOL for 5 USDC every week")]), title: "1 steps, one Confirm")
        let id = try #require(engine.card?.id)
        await engine.confirm(card: id)
        #expect(engine.pending == nil && engine.card == nil, "the recurring buy waited on a card of its own")
        // SOL's token is read from Jupiter live: under a loaded run it may not answer, and then nothing is set and that's said.
        if engine.messages.suffix(3).contains(where: { $0.text.contains("Jupiter doesn’t know the token") }) { engine.tasks = []; return }
        let kept = engine.tasks.filter { engine.recurringBuys.contains($0.id) }
        #expect(kept.count == 1 && kept.first?.every == .days(7), "\(engine.tasks.map(\.steps)) · \(engine.messages.suffix(3).map(\.text))")
        engine.tasks = []
    }

    /// T18: with half-typed text in the field a scheduled run waits (the Mac's draft.isEmpty).
    @Test func nothingScheduledStartsOverHalfTypedText() async {
        fresh()
        var task = ScheduledTask(steps: ["send 50 USDC to Mom"], next: Date().addingTimeInterval(-60), every: nil)
        task.because = "SOL went above $1"
        engine.tasks = [task]
        Engine.composing = true
        await engine.runDueTasks()
        #expect(engine.card == nil && engine.tasks.first?.next == task.next, "a scheduled run started over the person's typing")
        Engine.composing = false
        engine.tasks = []
    }
}

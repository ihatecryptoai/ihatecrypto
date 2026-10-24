@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Wave 7's runs (audit6 M1, R6, R7 and the trivials): the scheduled run beside the 30 s loop, one at a time; the
/// guardian's reason said once whatever time it names; a recurring buy kept by its mint shown by its symbol and its first
/// buy's schedule kept even when that buy fails; the Mac's Spanish lines. In-process, no network, no signer.
@MainActor @Suite(.serialized) struct Runs8Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("runs8-\(UUID().uuidString)").path)
        return Engine(out: Outbox())
    }()
    private var engine: Engine { Self.engine }

    @Test func aSpanishChatGetsTheMacsSpanishLines() {
        let was = engine.settings.language
        defer { engine.settings.language = was }
        engine.settings.language = "es"
        #expect(engine.say("Cancelled — nothing moved.", "Отменено — ничего не ушло.", "Cancelado: no se movió nada.") == "Cancelado: no se movió nada.")
        // A line given in two languages stays English for a Spanish chat, as on the Mac.
        #expect(engine.say("Done", "Готово") == "Done")
        engine.settings.language = "ru"
        #expect(engine.say("Cancelled — nothing moved.", "Отменено — ничего не ушло.", "Cancelado: no se movió nada.") == "Отменено — ничего не ушло.")
        engine.settings.language = "en"
        #expect(engine.say("Cancelled — nothing moved.", "Отменено — ничего не ушло.", "Cancelado: no se movió nada.") == "Cancelled — nothing moved.")
    }

    @Test func theGuardiansReasonIsOneWhateverTimeItNames() {
        let first = "The Hyperliquid trading key … wasn't opened. Orders through it ask again from 10:42 AM, and any move opens it."
        let later = "The Hyperliquid trading key … wasn't opened. Orders through it ask again from 10:52 AM, and any move opens it."
        #expect(Engine.reasonOnly(first) == Engine.reasonOnly(later))
        #expect(Engine.reasonOnly(first) != Engine.reasonOnly("Hyperliquid refused the order: reduce only."))
    }

    @Test func theScheduledRunIsOneAtATime() async {
        engine.startScheduledRun()
        let first = engine.scheduledRun
        #expect(first != nil)
        // A second start while it runs leaves it be: no second run beside it.
        engine.startScheduledRun()
        #expect(engine.scheduledRun == first)
        await first?.value
        #expect(engine.scheduledRun == nil)
    }

    @Test func aRecurringBuyByItsMintShowsItsSymbol() {
        let mint = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU"    // SAMO's public mint
        let step = "buy token \(mint) for 5 USDC"
        // The names are one store for the test process: another test may have bought SAMO first.
        engine.tokenNames[mint] = nil
        #expect(engine.shownStep(step) == step)
        // A mint's short form isn't a name.
        engine.rememberName(.buy(.byMint(mint), usdc: 5))
        #expect(engine.shownStep(step) == step)
        engine.rememberName(.buy(SpotAsset(symbol: "SAMO", mint: mint, decimals: 9), usdc: 5))
        #expect(engine.shownStep(step) == "buy SAMO for 5 USDC")
        // A coin of the fixed list keeps its own words.
        #expect(engine.shownStep("buy SOL for 5 USDC") == "buy SOL for 5 USDC")
    }

    @Test func aRecurringBuysFirstConfirmKeepsTheScheduleWhenTheBuyFails() async throws {
        let sol = try #require(SpotAsset.all.first { $0.symbol == "SOL" })
        let buy = WalletCommand.buy(sol, usdc: 5)
        engine.tasks.removeAll { $0.steps == [Engine.recurringStep(buy)] }
        engine.show(.recurring(buy, days: 7), title: "Buy SOL for 5 USDC every week")
        // No wallet here, so the buy itself fails («Add a wallet first»): the schedule its Confirm made stays, as on the Mac.
        await engine.confirm(card: engine.card?.id)
        let kept = try #require(engine.tasks.first { $0.steps == [Engine.recurringStep(buy)] })
        #expect(engine.recurringBuys.contains(kept.id))
        #expect(kept.next > Date().addingTimeInterval(6 * 86_400))
        engine.tasks.removeAll { $0.id == kept.id }
    }
}

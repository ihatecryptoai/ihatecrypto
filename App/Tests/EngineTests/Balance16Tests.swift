@testable import DesktopEngine
import Foundation
import Testing

/// Wave 15's balance part (audit14 A14-C2-1, -2, -4): the balance is whole once Solana (priced — an unpriced read takes
/// the last prices, UnpricedBalanceTests) and Hyperliquid have each answered since the wallet came on screen; a refresh asked for during a read runs after it; a wallet switch
/// forgets whose positions were read, so the guardian waits for the new wallet's own. In-process, no network.
@MainActor @Suite(.serialized) struct Balance16Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("balance16-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    @Test func theBalanceIsWholeOnceEachNetworkAnswered() {
        // Solana answered in one read, Hyperliquid in the next: whole (A14-C2-4).
        #expect(Engine.balanceReady(solana: true, evm: true, solanaPriced: true, hl: true, automation: true))
        // Not yet heard from Hyperliquid, or Solana came unpriced (a coin Jupiter didn't price, new since the last read).
        #expect(!Engine.balanceReady(solana: true, evm: true, solanaPriced: true, hl: false, automation: true))
        #expect(!Engine.balanceReady(solana: true, evm: true, solanaPriced: false, hl: true, automation: true))
        #expect(!Engine.balanceReady(solana: true, evm: true, solanaPriced: nil, hl: true, automation: true))
        // A wallet of one chain waits only for its own; the Automation wallet unread keeps it loading.
        #expect(Engine.balanceReady(solana: false, evm: true, solanaPriced: nil, hl: true, automation: true))
        #expect(!Engine.balanceReady(solana: false, evm: true, solanaPriced: nil, hl: true, automation: false))
    }

    @Test func aRefreshAskedDuringOneRunsAfterIt() async {
        engine.refreshing = true
        engine.refreshAgain = false
        let asked = Task { await engine.refresh() }
        while engine.readWaiters.isEmpty { await Task.yield() }
        #expect(engine.refreshAgain, "a refresh asked for during a read must run after it, not be dropped")
        // The read it queued lands (Balance17Tests has the rest).
        engine.readsStarted += 1
        engine.landReads()
        await asked.value
        engine.refreshing = false
        engine.refreshAgain = false
    }

    @Test func aSwitchForgetsWhosePositionsWereRead() {
        engine.positionsFor = "0x90f79bf6eb2c4f870365e785982e1f101e93b906"
        engine.walletChanged()
        #expect(engine.positionsFor == nil && engine.hlOrders.isEmpty)
    }
}

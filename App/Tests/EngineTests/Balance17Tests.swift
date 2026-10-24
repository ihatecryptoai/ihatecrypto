@testable import DesktopEngine
import IhatecryptoCore
import Foundation
import Testing

/// Wave 16's balance part (audit15 A15-C1-3, A15-C2-1, A15-C2-3, A15-C1-2's reset): a refresh asked during a read
/// returns once the read it queued has landed — never on the read already under way — and a wallet switch forgets
/// everything the last wallet's reads left. In-process, no network; public test addresses only.
@MainActor @Suite(.serialized) struct Balance17Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("balance17-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    /// Actor-held flag, set once the waiting refresh returns.
    actor Flag { var on = false; func set() { on = true } }

    @Test func aRefreshAskedDuringAReadWaitsForTheReadItQueued() async {
        engine.refreshing = true
        engine.refreshAgain = false
        engine.readsStarted = 5   // read 5 is under way when the person asks
        let back = Flag()
        let asked = Task { await engine.refresh(); await back.set() }
        while engine.readWaiters.isEmpty { await Task.yield() }
        #expect(engine.readWaiters.first?.pass == 6)
        // Read 5 lands: the ask came during it, so it isn't the read that was asked for.
        engine.landReads()
        for _ in 0..<20 { await Task.yield() }
        #expect(await !back.on, "a refresh returned on the read that was already running")
        #expect(engine.readWaiters.count == 1)
        // Read 6, begun after the ask, lands: now it returns.
        engine.readsStarted = 6
        engine.landReads()
        await asked.value
        #expect(await back.on && engine.readWaiters.isEmpty)
        engine.refreshing = false
        engine.refreshAgain = false
    }

    @Test func aSwitchForgetsWhatTheLastWalletsReadsLeft() {
        engine.seenSolanaPriced = true
        engine.seenHL = true
        engine.solanaHoldings = [Holding(chain: "solana", contract: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", amount: 5, symbol: "USDC")]
        engine.placeFacts.hyperliquid = .init(accountValue: 12, withdrawable: 10, spotUSDC: 2, dexes: [], hlpEquity: 0, hlpLockedUntil: nil)
        engine.recentlyClosed = [ClosedTrade(coin: "ETH", name: "ETH", long: true, closed: Date(), realized: 10)]
        NoticeMemory.positions = ["ETH"]
        NoticeMemory.entries = ["BTC"]
        engine.walletChanged()
        #expect(engine.seenSolanaPriced == nil && !engine.seenHL && engine.solanaHoldings.isEmpty)
        #expect(engine.placeFacts.hyperliquid == nil && engine.recentlyClosed.isEmpty)
        // The new wallet's first read finds no «new» position, so nothing «filled» on the old wallet's entry is said.
        #expect(NoticeMemory.positions == nil && NoticeMemory.entries.isEmpty)
    }
}

import Foundation
import HyperliquidSigning
@testable import SignerCore
import SignerAPI
import Testing
import WalletCore

/// After a restart the trading key waits for its first Windows Hello (AgentStore). An order no one is there for — the
/// guardian's stop — is refused with why, never handed to the main key (whose prompt wouldn't close and would say «the
/// 7 days are over»), and the key doesn't ask again for ten minutes; an order that opens asks at once.
extension VaultTests {
    func signed(_ action: MsgPack, _ nonce: UInt64) throws -> SignerReply {
        try send(.signHyperliquidL1(action: action, nonce: nonce, mainnet: true))
    }

    @Test(.enabled(if: fakeHello)) func anUnopenedTradingKeyRefusesAStopAndWaitsBeforeAskingAgain() throws {
        // The opened key and the quiet minutes live in memory, past the suite's own clean-up: gone with this test.
        defer { restart(); setEnv("WINSEC_FAKE_CANCEL", "0") }
        _ = try importWallet()
        _ = try send(.enableAgent)
        #expect(try send(.setAgentActive(true)).agentActive == true)
        restart()
        let stop = HLAction.cancel([(asset: 0, oid: 1)])

        // No one at Windows Hello: refused, and the reason says what protects the position meanwhile.
        setEnv("WINSEC_FAKE_CANCEL", "1")
        do { _ = try signed(stop, 1); Issue.record("an unopened key signed") } catch {
            let said = (error as? WalletError)?.message ?? "\(error)"
            #expect(said.contains("wasn't opened") && said.contains("stop and take on the exchange"), "\(said)")
        }
        // Ten quiet minutes: someone there now still isn't asked by a stop (it would have signed if it asked).
        setEnv("WINSEC_FAKE_CANCEL", "0")
        #expect(throws: (any Error).self) { try signed(stop, 2) }

        // An order that opens comes from a card just confirmed: it asks, opens the key and signs.
        let open = try HLAction.order([HLOrder(asset: 1, isBuy: true, price: 100, size: 1, reduceOnly: false, kind: .limit(tif: "Gtc"))])
        #expect(try signed(open, 3).signature != nil)
        // Open now: the next stop goes with no one there.
        setEnv("WINSEC_FAKE_CANCEL", "1")
        #expect(try signed(stop, 4).signature != nil)
    }
}

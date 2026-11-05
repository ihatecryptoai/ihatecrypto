import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
@testable import TradeExecution
import Testing
import TxGuard
import WalletCore

/// The address a signature of r = s = 1 recovers to over an L1 action at nonce 1793880000000: Hyperliquid answered
/// «User or API Wallet 0x… does not exist» with exactly these for the same JSON on 2026-11-05, so these are the bytes it
/// hashes (key order, activationPx as nil). An API wallet may sign them, like orders.
func recovered(_ action: MsgPack) throws -> String {
    let one = Data(count: 31) + Data([1])
    let digest = try HLSigner.l1Parts(action, nonce: 1_793_880_000_000, mainnet: true).digest
    return try EVMSigner.recover(digest: digest, signature: EVMSignature(r: one, s: one, v: 27)).lowercased()
}

@Test func trailingStopIsWhatHyperliquidHashes() throws {
    let a = try HLAction.trailingStop(asset: 1, isBuy: false, size: Decimal(string: "0.01")!, pct: 1)
    #expect(try recovered(a) == "0x086f4dd8ce587ccb2c04d006f04ebe1364b55fb4")
    #expect(try recovered(try HLAction.trailingStop(asset: 1, isBuy: false, size: Decimal(string: "0.01")!, pct: Decimal(string: "0.5")!))
            == "0x0d48a25e9ecd35e4b55b4847515b47473a214c02")

    // The trading key signs it, also past its week: it only reduces. The guard passes it reduce-only and nothing else.
    #expect(HLSigner.isTrading(a) && HLAction.onlyReduces(a))
    let v = HLGuard.check(a, owner: "0x0000000000000000000000000000000000000001", recipients: [])
    #expect(v.ok && v.checks == ["Trailing stop: sell 0.01 on market #1 once the price falls back 1% from its best, only reducing a position → your account"])
    guard case .map(let pairs) = a else { return }
    let opens = MsgPack.map(pairs.map { $0.0 == "reduceOnly" ? ($0.0, .bool(false)) : $0 })
    #expect(!HLGuard.check(opens, owner: "0x0000000000000000000000000000000000000001", recipients: []).ok)
    #expect(!HLAction.onlyReduces(opens))

    // One status, not a list: an error in it is an error.
    #expect(HLReply.parse(Data(#"{"status":"ok","response":{"type":"trailingStop","data":{"oid":77738308}}}"#.utf8)) == .ok)
    #expect(HLReply.parse(Data(#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"error":"Invalid TWAP duration: 1 min(s)"}}}}"#.utf8))
            == .error("Invalid TWAP duration: 1 min(s)"))
}

extension FakeHyperliquid {
    /// «trail 2%» on an open long puts a reduce-only trailing stop for the whole position on the exchange, then cancels the
    /// trailing stop it replaces; the fixed stop and take stay. A stop moved afterwards leaves the trailing stop alone.
    @Test func trailPlacesATrailingStopOnTheExchange() async throws {
        URLProtocol.registerClass(FakeHL.self)
        defer { URLProtocol.unregisterClass(FakeHL.self) }
        let trailing: [String: Any] = ["coin": "ETH", "oid": 8, "isTrigger": true, "triggerPx": "2950", "orderType": "Trailing Stop Market",
                                       "reduceOnly": true, "side": "A", "sz": "0.01"]
        FakeHL.lock.withLock { FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [stop(2, "2940"), take(3, "3120"), trailing]) }
        let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("trail-\(UUID().uuidString).jsonl"))
        let e = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(), journal: journal) { _ in }
        let line = try await e.manage(.trail(2), asset: "ETH")
        #expect(line.hasPrefix("On the exchange: a trailing stop 2% behind the best price"))
        #expect(FakeHL.lock.withLock { FakeHL.state.sent } == ["trailing sell 0.01 2%", "cancel 8"])

        // A stop moved by hand replaces the fixed stop only.
        FakeHL.lock.withLock { FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [stop(2, "2940"), take(3, "3120"), trailing]) }
        _ = try await e.manage(.stopTo(2970), asset: "ETH")
        #expect(FakeHL.lock.withLock { FakeHL.state.sent } == ["sl", "cancel 2"])
    }
}

import ExecutionCore
import Foundation
import IhatecryptoCore
@testable import TradeExecution
import Testing

private func d(_ s: String) -> Decimal { Decimal(string: s)! }

/// `addPartialTake` against FakeHL (an ETH position entered at 3000) as `setup` leaves it: what went to the exchange,
/// the outcome or the error, and the executor's report lines.
private func addPartialTake(refuses: Bool = false, _ setup: (inout FakeHL.State) -> Void) async -> (sent: [String], outcome: Executor.PartialTake?, error: Error?, reported: [String]) {
    URLProtocol.registerClass(FakeHL.self)
    defer { URLProtocol.unregisterClass(FakeHL.self) }
    FakeHL.lock.withLock {
        FakeHL.state = .init()
        setup(&FakeHL.state)
    }
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("partial-\(UUID().uuidString).jsonl"))
    let executor = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(refuses: refuses), journal: journal) { line in
        FakeHL.lock.withLock { FakeHL.state.reported.append(line) }
    }
    var outcome: Executor.PartialTake?, error: Error?
    do { outcome = try await executor.addPartialTake(asset: "ETH") } catch let e { error = e }
    let state = FakeHL.lock.withLock { FakeHL.state }
    return (state.sent, outcome, error, state.reported)
}

/// A limit still waiting on the book with its stop and take as children (normalTpsl).
private var resting: [String: Any] {
    ["coin": "ETH", "oid": 9, "isTrigger": false, "limitPx": "2990", "orderType": "Limit", "reduceOnly": false, "side": "B", "sz": "0.017",
     "children": [stop(10, "2940"), take(11, "3120")]]
}

extension FakeHyperliquid {
    /// Partly filled: the stop is still the limit's child, armed only on the full fill. Nothing is signed; it waits.
    @Test func aPartlyFilledLimitWaits() async {
        let r = await addPartialTake { s in s.position = d("0.003"); s.orders = [resting] }
        #expect(r.outcome == .waiting && r.error == nil && r.sent == [])
    }

    /// Filled: one reduce-only take at 1R (3000 + (3000 − 2940)) for half the position.
    @Test func aFilledLimitGetsItsHalfAt1R() async {
        let r = await addPartialTake { s in s.position = d("0.02"); s.orders = [stop(2, "2940"), take(3, "3120")] }
        #expect(r.outcome == .placed(3060) && r.error == nil)
        #expect(r.sent == ["tp"])
        #expect(r.reported.contains { $0.hasPrefix("Partial take-profit: 0.01 ETH at 1R → 3,060") })
    }

    /// The half went on but its reply was lost: the next try finds it on the book and sends nothing.
    @Test func aHalfAlreadyOnTheBookIsNotSentAgain() async {
        var r = await addPartialTake { s in s.position = d("0.02"); s.orders = [stop(2, "2940")]; s.lost = 1 }
        #expect(r.error != nil && r.sent == ["tp"])
        let book = FakeHL.lock.withLock { FakeHL.state.orders }
        r = await addPartialTake { s in s.position = d("0.02"); s.orders = book }
        #expect(r.outcome == .placed(3060) && r.error == nil && r.sent == [])
        // The chat was last told it failed: it hears that the half is on.
        #expect(r.reported.contains { $0.hasPrefix("Partial take-profit: 0.01 ETH at 1R → 3,060") })
    }

    /// Hyperliquid turns the half down: an error to try again on, not a quiet nil.
    @Test func anExchangeRefusalThrows() async {
        let r = await addPartialTake { s in s.position = d("0.02"); s.orders = [stop(2, "2940")]; s.refusals = 1 }
        #expect(r.outcome == nil && r.sent == ["tp"])
        #expect((r.error as? ExecutionError)?.message.contains("Trigger order has invalid price") == true)
    }

    /// Touch ID declined or the signer refused: a GuardStop, which the guardian doesn't ask again.
    @Test func aDeclinedSignatureStaysAGuardStop() async {
        let r = await addPartialTake(refuses: true) { s in s.position = d("0.02"); s.orders = [stop(2, "2940")] }
        #expect(r.error is GuardStop && r.sent == [])
    }

    /// Half of $15 is under the $10 minimum order: skipped, nothing sent.
    @Test func aHalfUnderTenDollarsIsSkipped() async {
        let r = await addPartialTake { s in s.position = d("0.005"); s.orders = [stop(2, "2940")] }
        #expect(r.outcome == .skipped && r.error == nil && r.sent == [])
    }

    /// A position with no stop and no limit waiting (the rest of a partly filled limit was cancelled): it says so.
    @Test func noStopAndNoRestingLimitThrows() async {
        let r = await addPartialTake { s in s.position = d("0.02") }
        #expect(r.outcome == nil && r.sent == [])
        #expect((r.error as? ExecutionError)?.message.contains("no stop on the exchange") == true)
    }
}

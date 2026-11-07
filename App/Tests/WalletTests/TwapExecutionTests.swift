import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
@testable import TradeExecution
import Testing
import TxGuard
import WalletCore

/// twapOrder and twapCancel are the bytes Hyperliquid hashes (the addresses it named on 2026-11-05, see `recovered`), the
/// trading key signs them, and the guard lets through only a TWAP of 5 minutes to 4 hours.
@Test func twapActionsAreWhatHyperliquidHashes() throws {
    let twap = try HLAction.twapOrder(asset: 1, isBuy: true, size: Decimal(string: "0.01")!, minutes: 5, randomize: false)
    #expect(try recovered(twap) == "0xdc93f6450dcc88ebb33d7e4a8877978c7d781ac6")
    let cancel = HLAction.twapCancel(asset: 1, id: 12345)
    #expect(try recovered(cancel) == "0x8fe1b20b56935c715c38aa52a184fc1d601b981f")

    #expect(HLSigner.isTrading(twap) && HLSigner.isTrading(cancel))
    // Past the trading key's week a cancel still goes; a TWAP that opens a position takes the main key, like an order.
    #expect(HLAction.onlyReduces(cancel) && !HLAction.onlyReduces(twap))
    #expect(HLAction.onlyReduces(try HLAction.twapOrder(asset: 1, isBuy: false, size: 1, reduceOnly: true, minutes: 30, randomize: true)))

    let owner = "0x0000000000000000000000000000000000000001"
    let v = HLGuard.check(twap, owner: owner, recipients: [])
    #expect(v.ok && v.checks == ["TWAP: buy 0.01 on market #1 over 5 min at the market → your account"])
    #expect(HLGuard.check(cancel, owner: owner, recipients: []).checks == ["Stops a TWAP inside your account."])
    #expect(HLGuard.check(try HLAction.twapOrder(asset: 1, isBuy: true, size: 1, minutes: 240, randomize: false), owner: owner, recipients: []).ok)
    for minutes in [1, 4, 241, 10_080] {
        #expect(!HLGuard.check(try HLAction.twapOrder(asset: 1, isBuy: true, size: 1, minutes: minutes, randomize: false), owner: owner, recipients: []).ok)
    }

    #expect(HLReply.parse(Data(#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"running":{"twapId":77738308}}}}}"#.utf8))
            == .running(twapId: 77738308))
    #expect(HLReply.parse(Data(#"{"status":"ok","response":{"type":"twapCancel","data":{"status":"success"}}}"#.utf8)) == .ok)
}

/// A TWAP plan from the chat's words, as `prepare` hands it to `execute` (ETH at 3000 in FakeHL).
private func twapPlan() throws -> Executor.Prepared {
    let market = Market(name: "ETH", markPx: 3000, szDecimals: 4, maxLeverage: 25, index: 0)
    let intent = try #require(IntentParser.apply("long ETH 300 USDC over 2 h stop 2% take 4% risk $6 leverage 1x", to: TradeIntent(), markets: ["ETH"]))
    let evaluation = Plan.evaluate(intent, market: market)
    let sizing = try #require(evaluation.sizing)
    #expect(evaluation.problem == nil)
    return Executor.Prepared(intent: intent, market: market, sizing: sizing, leverage: 1, bridge: nil, changes: [], steps: [], preparedAt: Date())
}

extension FakeHyperliquid {
    /// The entry is one TWAP; once its first part shows as a position, the stop and take go on for the whole position
    /// (size 0, they grow with it). Refused both ways, the TWAP stops and what it bought is closed.
    @Test func aTwapEntryGetsItsStopForTheWholePosition() async throws {
        URLProtocol.registerClass(FakeHL.self)
        defer { URLProtocol.unregisterClass(FakeHL.self) }
        let p = try twapPlan()
        let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("twap-\(UUID().uuidString).jsonl"))
        let e = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(), journal: journal) { _ in }
        let running = #"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"running":{"twapId":77}}}}}"#
        func run(_ setup: (inout FakeHL.State) -> Void) async -> (sent: [String], opened: Executor.Opened?, error: String, state: FakeHL.State) {
            FakeHL.lock.withLock {
                FakeHL.state = .init()
                setup(&FakeHL.state)
            }
            var opened: Executor.Opened?, text = ""
            do { opened = try await e.execute(p) } catch { text = error.localizedDescription }
            let state = FakeHL.lock.withLock { FakeHL.state }
            return (state.sent, opened, text, state)
        }

        // Started, the first part filled: the stop and take for the whole position, nothing else.
        var r = await run { s in s.entry = { s in s.position = Decimal(string: "0.0008")!; return running } }
        #expect(r.sent == ["leverage", "twap 120 min", "sl+tp"] && r.error.isEmpty, "\(r.sent) \(r.error)")
        #expect(r.state.protections.map { $0["s"] as? String } == ["0", "0"])
        #expect(r.opened?.twapId == 77 && r.opened?.size == p.sizing.size && r.opened?.stop == 2940 && r.opened?.take == 3120)

        // Hyperliquid won't start it: nothing bought, nothing else sent.
        r = await run { s in s.entry = { _ in #"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"error":"Invalid TWAP duration: 1 min(s)"}}}}"# } }
        #expect(r.sent == ["leverage", "twap 120 min"] && r.error.contains("didn't start the TWAP") && r.opened == nil)

        // The stop is refused both ways: the TWAP stops first, then what it bought is closed.
        r = await run { s in
            s.refusals = 2
            s.entry = { s in s.position = Decimal(string: "0.0008")!; return running }
            s.close = { s in s.position = 0; return filled("0.0008") }
        }
        #expect(r.sent == ["leverage", "twap 120 min", "sl+tp", "sl+tp", "twap cancel 77", "close sell"], "\(r.sent)")
        #expect(r.error.contains("The TWAP is stopped") && r.opened == nil)
    }

    /// «cancel the TWAP» stops a running one by its id and keeps the position's stop; «close» stops it before closing.
    @Test func aRunningTwapStopsByItsId() async throws {
        URLProtocol.registerClass(FakeHL.self)
        defer { URLProtocol.unregisterClass(FakeHL.self) }
        let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("twapcancel-\(UUID().uuidString).jsonl"))
        let e = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(), journal: journal) { _ in }
        let row: [String: Any] = ["state": ["coin": "ETH", "side": "B", "sz": "0.1", "minutes": 120], "status": ["status": "activated"], "twapId": 77]
        // twapHistory is a log: a TWAP that finished has its «activated» row too, and isn't running.
        let done: [[String: Any]] = [["state": ["coin": "ETH"], "status": ["status": "finished"], "twapId": 5],
                                     ["state": ["coin": "ETH"], "status": ["status": "activated"], "twapId": 5]]
        FakeHL.lock.withLock { FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [stop(2, "2940"), take(3, "3120")]); FakeHL.state.twaps = [row] + done }
        let line = try await e.manage(.cancelOrders, asset: "ETH")
        #expect(line == "ETH TWAP stopped (1). What it bought stays open with its stop and take-profit.")
        #expect(FakeHL.lock.withLock { FakeHL.state.sent } == ["twap cancel 77"])

        FakeHL.lock.withLock {
            FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [stop(2, "2940")], close: { s in s.position = 0; return filled("0.01") })
            FakeHL.state.twaps = [row]
        }
        _ = try await e.close(asset: "ETH")
        #expect(FakeHL.lock.withLock { FakeHL.state.sent }.prefix(2) == ["twap cancel 77", "close sell"])

        // The guardian, each pass: with the stop on the exchange and the loss at it within 1.5 × the plan's, nothing is done.
        let wholeStop: [String: Any] = ["coin": "ETH", "oid": 2, "isTrigger": true, "triggerPx": "2940", "orderType": "Stop Market", "reduceOnly": true,
                                        "side": "A", "sz": "0.0", "isPositionTpsl": true]
        func watch(_ risk: Decimal? = nil, _ setup: (inout FakeHL.State) -> Void) async throws -> (Executor.TwapWatch, [String]) {
            FakeHL.lock.withLock { FakeHL.state = .init(); FakeHL.state.twaps = [row]; setup(&FakeHL.state) }
            let w = try await e.guardTwap(asset: "ETH", risk: risk)
            return (w, FakeHL.lock.withLock { FakeHL.state.sent })
        }
        var w = try await watch { s in s.position = Decimal(string: "0.01")!; s.orders = [wholeStop] }
        #expect(w.0 == .running && w.1.isEmpty)
        // Its stop fired and the TWAP bought again: stopped first, then what it bought with no stop is closed.
        w = try await watch { s in s.position = Decimal(string: "0.004")!; s.close = { s in s.position = 0; return filled("0.004") } }
        #expect(w.1 == ["twap cancel 77", "close sell"])
        if case .stopped(let line) = w.0 { #expect(line.hasPrefix("The ETH TWAP kept buying after its stop: it's stopped. ETH position closed")) } else { Issue.record("\(w.0)") }
        // Flat (its stop or take closed it): only the TWAP stops.
        w = try await watch { s in s.position = 0 }
        #expect(w.0 == .stopped("The ETH TWAP is stopped: its position's stop or take closed it.") && w.1 == ["twap cancel 77"])
        // Hyperliquid doesn't run it any more: nothing left to watch.
        w = try await watch { s in s.twaps = []; s.position = Decimal(string: "0.1")!; s.orders = [wholeStop] }
        #expect(w.0 == .ended && w.1.isEmpty)
        // A TWAP that won't stop throws: nothing is closed, and the guardian keeps watching it (its twapId stays).
        for position in [Decimal(0), Decimal(string: "0.004")!] {
            await #expect(throws: ExecutionError.self) { _ = try await watch { s in s.position = position; s.twapCancelRefused = true } }
            #expect(FakeHL.lock.withLock { FakeHL.state.sent } == ["twap cancel 77"])
        }
        // The loss at the stop past 1.5 × the plan's: 0.05 ETH held from 3000 and 0.05 still to buy at 3000, both down to
        // 2940, is $6: over 1.5 × $3, within 1.5 × $5.
        w = try await watch(3) { s in s.position = Decimal(string: "0.05")!; s.orders = [wholeStop] }
        #expect(w.1 == ["twap cancel 77"])
        if case .stopped(let line) = w.0 { #expect(line.contains("would lose $6.00, more than 1.5 × the plan's $3.00"), "\(line)") } else { Issue.record("\(w.0)") }
        w = try await watch(5) { s in s.position = Decimal(string: "0.05")!; s.orders = [wholeStop] }
        #expect(w.0 == .running && w.1.isEmpty)

        // A stop moved by hand during a TWAP stays one for the whole position (size 0).
        FakeHL.lock.withLock { FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [wholeStop]) }
        _ = try await e.manage(.stopTo(2970), asset: "ETH")
        #expect(FakeHL.lock.withLock { (FakeHL.state.sent, FakeHL.state.protections.map { $0["s"] as? String }) } == (["sl", "cancel 2"], ["0"]))

        // A wider stop is held to the rule on everything the TWAP builds: 0.01 held and 0.09 still to buy, $100 under the
        // price, is −$10.56 of $100 capital. Without the TWAP the same stop is −$1.06 and goes.
        FakeHL.lock.withLock { FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [stop(2, "2940")]); FakeHL.state.twaps = [row] }
        let wide = await #expect(throws: ExecutionError.self) { _ = try await e.manage(.stopTo(2900), asset: "ETH", equity: 100) }
        #expect(wide?.message.contains("−$10.56") == true && FakeHL.lock.withLock { FakeHL.state.sent }.isEmpty, "\(wide?.message ?? "")")
        FakeHL.lock.withLock { FakeHL.state = .init(position: Decimal(string: "0.01")!, orders: [stop(2, "2940")]) }
        _ = try await e.manage(.stopTo(2900), asset: "ETH", equity: 100)
        #expect(FakeHL.lock.withLock { FakeHL.state.sent } == ["sl", "cancel 2"])
    }
}

/// A running TWAP's parts not yet bought count in the open risk at its coin's stop; with no stop the sum is unknown.
@Test func aRunningTwapCountsInTheOpenRisk() {
    let twap = HyperliquidAccount.Twap(id: 1, coin: "ETH", size: Decimal(string: "0.1")!, isBuy: true)
    let marks: [String: Decimal] = ["ETH": 3000]
    let position = HyperliquidAccount.Position(coin: "ETH", size: Decimal(string: "0.04")!, entry: 3000, unrealizedPnl: 0, liquidation: nil)
    let stopOrder = try! JSONDecoder().decode(HyperliquidAccount.Order.self, from: Data(
        #"{"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"2940","orderType":"Stop Market","reduceOnly":true,"side":"A","sz":"0.0"}"#.utf8))
    // 0.06 ETH still to buy, $60 above the stop.
    #expect(HyperliquidAccount.twapRisk([twap], positions: [position], orders: [stopOrder], marks: marks) == Decimal(string: "3.6"))
    #expect(HyperliquidAccount.twapRisk([twap], positions: [position], orders: [], marks: marks) == nil)
    #expect(HyperliquidAccount.twapRisk([], positions: [], orders: [], marks: marks) == 0)
}

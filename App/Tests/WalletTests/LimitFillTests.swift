import ExecutionCore
import Foundation
import IhatecryptoCore
@testable import TradeExecution
import Testing

/// A plan from the chat's words, as `prepare` hands it to `execute` (ETH at 3000 in FakeHL, so a limit at 2990 waits).
private func plan(_ phrase: String) throws -> Executor.Prepared {
    let market = Market(name: "ETH", markPx: 3000, szDecimals: 4, maxLeverage: 25, index: 0)
    let intent = try #require(IntentParser.apply(phrase, to: TradeIntent(), markets: ["ETH"]))
    let evaluation = Plan.evaluate(intent, market: market)
    #expect(evaluation.problem == nil && evaluation.missing.isEmpty)
    let sizing = try #require(evaluation.sizing)
    return Executor.Prepared(intent: intent, market: market, sizing: sizing, leverage: 2, bridge: nil, changes: [], steps: [], preparedAt: Date())
}

private struct Outcome {
    var sent: [String]
    var opened: Executor.Opened?
    var text: String
    var state: FakeHL.State
}

/// `execute` against FakeHL as `setup` leaves it: what went to the exchange, and the Opened or the error's words.
private func execute(_ p: Executor.Prepared, _ setup: (inout FakeHL.State) -> Void) async -> Outcome {
    URLProtocol.registerClass(FakeHL.self)
    defer { URLProtocol.unregisterClass(FakeHL.self) }
    FakeHL.lock.withLock {
        FakeHL.state = .init()
        setup(&FakeHL.state)
    }
    let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("limit-\(UUID().uuidString).jsonl"))
    let executor = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(), journal: journal) { line in
        FakeHL.lock.withLock { FakeHL.state.reported.append(line) }
    }
    var opened: Executor.Opened?, text = ""
    do { opened = try await executor.execute(p) } catch { text = error.localizedDescription }
    let state = FakeHL.lock.withLock { FakeHL.state }
    return Outcome(sent: state.sent, opened: opened, text: text, state: state)
}

/// The fill of the plan's whole size: the position opens, and with `armed` its group's stop and take are listed.
private func fillsAtOnce(_ p: Executor.Prepared, armed: Bool, reply: String? = nil) -> (inout FakeHL.State) -> Void {
    { s in
        s.entry = { s in
            s.position = p.sizing.size
            if armed { s.orders = [stop(7, "2900.3"), take(8, "3169.4")] }
            return reply ?? filled("\(p.sizing.size)", at: "2988")
        }
    }
}

/// The plan's limit waiting on the book.
private func book(_ p: Executor.Prepared) -> [String: Any] {
    ["coin": "ETH", "oid": 9, "isTrigger": false, "limitPx": "2990", "orderType": "Limit", "reduceOnly": false, "side": "B", "sz": "\(p.sizing.size)"]
}

private let phrases = ["long ETH limit 2990 risk $0.5 stop 3% take 6% leverage 2x", "лонг ETH лимит 2990 риск $0.5 стоп 3% тейк 6% плечо 2"]

extension FakeHyperliquid {
    /// The price was already past the limit at Confirm: it fills at once, and the group's stop and take are listed —
    /// nothing more is sent.
    @Test(arguments: phrases) func limitFilledAtOnceKeepsItsArmedStop(_ phrase: String) async throws {
        let p = try plan(phrase)
        #expect(p.intent.entryPx == 2990 && p.intent.leverage == 2)
        let r = await execute(p, fillsAtOnce(p, armed: true))
        let opened = try #require(r.opened, "\(r.text)")
        #expect(r.sent == ["leverage", "limit sl+tp"])
        #expect(!opened.pending && opened.size == p.sizing.size && opened.entry == 2988 && opened.stop == d("2900.3") && opened.take == d("3169.4"))
    }

    /// «take in parts» on a limit that fills at once: the half at 1R goes on as on a market entry, so the app's exit
    /// rules have it (it was waiting for a fill that had already happened).
    @Test func limitFilledAtOnceGetsItsPartialTake() async throws {
        let p = try plan("long ETH limit 2990 risk $2 stop 3% take 6% in parts leverage 2x")
        let r = await execute(p, fillsAtOnce(p, armed: true))
        let opened = try #require(r.opened, "\(r.text)")
        #expect(r.sent == ["leverage", "limit sl+tp", "tp"])
        #expect(opened.partialTake == d("3075.7"))   // 2988 + (2988 − 2900.3)
    }

    /// Filled at once, its stop not listed: the stop and take go on the position again, from the limit's prices, for the
    /// position's size. The trade stays.
    @Test(arguments: phrases) func limitFilledAtOnceWithoutItsStopGetsItAgain(_ phrase: String) async throws {
        let p = try plan(phrase)
        let r = await execute(p, fillsAtOnce(p, armed: false))
        let opened = try #require(r.opened, "\(r.text)")
        #expect(r.sent == ["leverage", "limit sl+tp", "sl+tp"])
        let sl = try #require(r.state.protections.first)
        let trigger = (sl["t"] as? [String: Any])?["trigger"] as? [String: Any]
        #expect(sl["r"] as? Bool == true && trigger?["tpsl"] as? String == "sl" && trigger?["triggerPx"] as? String == "2900.3")
        #expect(sl["s"] as? String == "\(p.sizing.size)")
        #expect(!opened.pending && opened.size == p.sizing.size && opened.stop == d("2900.3"))
        #expect(r.state.reported.contains { $0.contains("didn't confirm its stop") && $0.contains("placing it again") })
        #expect(r.state.reported.contains { $0.hasPrefix("On the exchange: stop 2,900.3") })
    }

    /// The stop is turned down again: the position is closed, as a market entry's is.
    @Test func limitFilledAtOnceStopRefusedAgainCloses() async throws {
        let p = try plan(phrases[0])
        let r = await execute(p) { s in
            fillsAtOnce(p, armed: false)(&s)
            s.refusals = 1
            s.close = { s in s.position = 0; return filled("\(p.sizing.size)") }
        }
        #expect(r.opened == nil && r.sent == ["leverage", "limit sl+tp", "sl+tp", "close sell"])
        #expect(r.text.hasPrefix("Position closed: the exchange didn't confirm the stop."))
    }

    /// Turned down, and the close doesn't fill either: the stop goes back on what is open.
    @Test func limitFilledAtOnceCloseFailsPutsTheStopBack() async throws {
        let p = try plan(phrases[0])
        let r = await execute(p) { s in
            fillsAtOnce(p, armed: false)(&s)
            s.refusals = 1
            s.close = { _ in noMatch }
        }
        #expect(r.opened == nil && r.sent == ["leverage", "limit sl+tp", "sl+tp", "close sell", "sl"])
        #expect(r.text.contains("emergency close failed too") && r.text.contains("Its stop at 2,900.3 is back on the exchange now."))
    }

    /// Filled at once and already gone (the price ran through the stop): it says so, and sends nothing more.
    @Test func limitFilledAtOnceAlreadyStoppedOut() async throws {
        let p = try plan(phrases[0])
        let r = await execute(p) { s in s.entry = { _ in filled("\(p.sizing.size)", at: "2988") } }
        #expect(r.opened == nil && r.sent == ["leverage", "limit sl+tp"])
        #expect(r.text.contains("no ETH position is open now"))
    }

    /// The entry filled and its stop was turned down in the same reply: that's a position without a stop, never «No
    /// position» — also when /info shows the fill a moment late. It gets the stop for the position's own size, at the
    /// limit's prices. Without one open it's a plain refusal; with the positions unread it doesn't claim either.
    @Test func filledEntryWithARefusedStopIsNotNoPosition() async throws {
        let p = try plan(phrases[0])
        let mixed = #"{"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"\#(p.sizing.size)","avgPx":"2988","oid":9}},{"error":"Trigger order has invalid price."}]}}}"#
        let held = d("0.0051")
        #expect(held != p.sizing.size)
        var r = await execute(p) { s in s.entry = { s in s.position = held; s.down = 2; return mixed } }
        let opened = try #require(r.opened, "\(r.text)")
        #expect(r.sent == ["leverage", "limit sl+tp", "sl+tp"])
        #expect(r.state.protections.first?["s"] as? String == "0.0051" && opened.size == held && opened.entry == 3000 && opened.stop == d("2900.3"))
        #expect(r.state.reported.contains { $0.contains("Trigger order has invalid price") && $0.contains("placing it again") })

        r = await execute(p) { s in s.entry = { _ in mixed } }
        #expect(r.opened == nil && r.sent == ["leverage", "limit sl+tp"])
        #expect(r.text.hasPrefix("Limit rejected: Trigger order has invalid price") && r.text.hasSuffix("No position."))

        r = await execute(p) { s in s.entry = { s in s.positionsDown = true; return mixed } }
        #expect(r.opened == nil && !r.text.contains("No position") && r.text.contains("can't be read"))
    }

    /// The entry rests and its stop was turned down: it would fill with no stop, so it is cancelled, and the chat says so.
    @Test func restingEntryWithARefusedStopIsCancelled() async throws {
        let p = try plan(phrases[0])
        let mixed = #"{"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":9}},{"error":"Trigger order has invalid price."}]}}}"#
        let r = await execute(p) { s in s.entry = { s in s.orders = [book(p)]; return mixed } }
        #expect(r.opened == nil && r.sent == ["leverage", "limit sl+tp", "cancel 9"] && r.state.orders.isEmpty)
        #expect(r.text.contains("Trigger order has invalid price") && r.text.contains("was cancelled; no position"))
    }

    /// Rested, then gone from the book a moment later because it filled: a position like one that filled at once. One
    /// that is on the book waits, as before — also when the first reads of the book fail or come back late.
    @Test func limitThatFilledAfterRestingIsAPosition() async throws {
        let p = try plan(phrases[0])
        let resting = #"{"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":9}},"waitingForFill","waitingForFill"]}}}"#
        var r = await execute(p, fillsAtOnce(p, armed: true, reply: resting))
        var opened = try #require(r.opened, "\(r.text)")
        #expect(r.sent == ["leverage", "limit sl+tp"] && !opened.pending && opened.size == p.sizing.size)

        for down in [0, 3] {
            r = await execute(p) { s in s.entry = { s in s.orders = [book(p)]; s.down = down; return resting } }
            opened = try #require(r.opened, "\(r.text)")
            #expect(r.sent == ["leverage", "limit sl+tp"] && opened.pending)
        }
    }

    /// The protection chain the limit now shares, on a market entry: protected, or closed when the stop is refused.
    @Test func marketEntryStopRefusedStillCloses() async throws {
        let p = try plan("long ETH risk $0.5 stop 3% take 6% leverage 2x")
        let opens: (inout FakeHL.State) -> Void = { s in s.entry = { s in s.position = p.sizing.size; return filled("\(p.sizing.size)") } }
        var r = await execute(p, opens)
        let opened = try #require(r.opened, "\(r.text)")
        #expect(r.sent == ["leverage", "entry", "sl+tp"] && opened.stop == d("2909"))   // 3% under the fill at 2999

        r = await execute(p) { s in
            opens(&s)
            s.refusals = 1
            s.close = { s in s.position = 0; return filled("\(p.sizing.size)") }
        }
        #expect(r.opened == nil && r.sent == ["leverage", "entry", "sl+tp", "close sell"])
        #expect(r.text.hasPrefix("Position closed: the exchange didn't confirm the stop."))
    }
}

private func d(_ s: String) -> Decimal { Decimal(string: s)! }

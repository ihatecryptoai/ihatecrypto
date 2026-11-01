import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
@testable import TradeExecution
import Testing
import WalletCore

/// A Hyperliquid with one market (ETH, mark 3000): /info answers from `state`, /exchange writes down what was sent
/// and answers the close as `state.close` says (nil: its reply is lost).
private final class FakeHL: URLProtocol {
    struct State {
        var position: Decimal = 0
        var orders: [[String: Any]] = []
        var sent: [String] = []
        var close: (inout State) -> String? = { _ in nil }
        /// clearinghouseState times out.
        var positionsDown = false
    }
    nonisolated(unsafe) static var state = State()
    static let lock = NSLock()

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "api.hyperliquid.xyz" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var data = Data()
        if let stream = request.httpBodyStream {   // URLProtocol gets the body as a stream, not httpBody
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 { data.append(buffer, count: n) }
            stream.close()
        }
        let body = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
        let reply: String? = Self.lock.withLock {
            request.url?.path == "/exchange" ? Self.exchange(body["action"] as? [String: Any] ?? [:]) : Self.info(body)
        }
        guard let reply else { return client!.urlProtocol(self, didFailWithError: URLError(.timedOut)) }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(reply.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }

    static func info(_ body: [String: Any]) -> String? {
        let main = body["dex"] == nil
        switch body["type"] as? String {
        case "metaAndAssetCtxs": return #"[{"universe":[{"name":"ETH","szDecimals":4,"maxLeverage":25}]},[{"markPx":"3000"}]]"#
        case "perpDexs": return "[]"
        case "maxBuilderFee": return "0"
        case "frontendOpenOrders": return main ? json(state.orders) : "[]"
        case "clearinghouseState":
            if state.positionsDown { return nil }
            guard main, state.position != 0 else { return #"{"assetPositions":[]}"# }
            return #"{"assetPositions":[{"position":{"coin":"ETH","szi":"\#(state.position)","entryPx":"3000","unrealizedPnl":"0.1"}}]}"#
        default: return "null"
        }
    }

    static func exchange(_ action: [String: Any]) -> String? {
        if action["type"] as? String == "cancel" {
            let oids = (action["cancels"] as? [[String: Any]] ?? []).compactMap { ($0["o"] as? NSNumber)?.uint64Value }
            state.sent.append("cancel " + oids.map(String.init).joined(separator: ","))
            state.orders.removeAll { oids.contains(($0["oid"] as? NSNumber)?.uint64Value ?? 0) }
            return #"{"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}}"#
        }
        let orders = action["orders"] as? [[String: Any]] ?? []
        let triggers = orders.compactMap { ($0["t"] as? [String: Any])?["trigger"] as? [String: Any] }
        guard !triggers.isEmpty else {
            let o = orders.first ?? [:]
            state.sent.append(o["r"] as? Bool != true ? "not reduce-only" : o["b"] as? Bool == true ? "close buy" : "close sell")
            return state.close(&state)
        }
        // A stop put back: listed among the open orders at once, so verifyProtection finds it.
        state.sent.append(triggers.compactMap { $0["tpsl"] as? String }.joined(separator: "+"))
        for t in triggers { state.orders.append(stop(100, t["triggerPx"] as? String ?? "")) }
        return #"{"status":"ok","response":{"type":"order","data":{"statuses":["waitingForTrigger"]}}}"#
    }

    static func json(_ v: Any) -> String { String(decoding: try! JSONSerialization.data(withJSONObject: v), as: UTF8.self) }
}

private func stop(_ oid: UInt64, _ px: String, side: String = "A") -> [String: Any] {
    ["coin": "ETH", "oid": oid, "isTrigger": true, "triggerPx": px, "orderType": "Stop Market", "reduceOnly": true, "side": side, "sz": "0.01"]
}
private func take(_ oid: UInt64, _ px: String) -> [String: Any] {
    ["coin": "ETH", "oid": oid, "isTrigger": true, "triggerPx": px, "orderType": "Take Profit Market", "reduceOnly": true, "side": "A", "sz": "0.01"]
}
private func filled(_ size: String) -> String {
    #"{"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"\#(size)","avgPx":"2999","oid":9}}]}}}"#
}
private let noMatch = #"{"status":"ok","response":{"type":"order","data":{"statuses":[{"error":"Order could not immediately match against any resting orders. asset=0"}]}}}"#

/// Signs with zeros; `refuses`: the signer turns the order down (Touch ID declined, the guard), as the app's signer does.
private struct ZeroSigner: TradeSigner {
    var refuses = false
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature {
        if refuses { throw GuardStop("declined") }
        return EVMSignature(r: Data(count: 32), s: Data(count: 32), v: 27)
    }
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError("no") }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw ExecutionError("no") }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data { throw ExecutionError("no") }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError("no") }
}

/// «close ETH»: the stop and take stay on the exchange until the position is flat. A close that doesn't fill, fills in
/// part or loses its reply leaves what is open under its stop; after the emergency close, restoreStop puts one back once.
@Test func closeKeepsTheStopUntilTheCloseFills() async throws {
    URLProtocol.registerClass(FakeHL.self)
    defer { URLProtocol.unregisterClass(FakeHL.self) }
    let guarded = [stop(2, "2940"), take(3, "3120")]
    func run(position: Decimal = Decimal(string: "0.01")!, orders: [[String: Any]] = guarded, refuses: Bool = false,
             close: @escaping (inout FakeHL.State) -> String? = { _ in nil },
             _ act: ((Executor) async throws -> String)? = nil) async -> (sent: [String], text: String, failed: Bool) {
        FakeHL.lock.withLock { FakeHL.state = .init(position: position, orders: orders, close: close) }
        let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("close-\(UUID().uuidString).jsonl"))
        let executor = Executor(wallet: TradeWallet(solana: "", evm: "0x0"), signer: ZeroSigner(refuses: refuses), journal: journal) { _ in }
        let outcome: (String, Bool)
        do {
            let text = if let act { try await act(executor) } else { try await executor.close(asset: "ETH") }
            outcome = (text, false)
        } catch { outcome = (error.localizedDescription, true) }
        return (FakeHL.lock.withLock { FakeHL.state.sent }, outcome.0, outcome.1)
    }

    // Filled: the close goes first, its stop and take are cancelled after it.
    var r = await run { s in s.position = 0; return filled("0.01") }
    #expect(r.sent == ["close sell", "cancel 2,3"] && !r.failed)
    #expect(r.text.hasPrefix("ETH position closed: 0.01 at 2,999"))

    // Filled whole while the positions don't read: the fill is taken, not a false «didn't finish».
    r = await run { s in s.position = 0; s.positionsDown = true; return filled("0.01") }
    #expect(r.sent == ["close sell", "cancel 2,3"] && !r.failed)

    // No one to fill it (a thin book): nothing cancelled, the position keeps its stop.
    r = await run { _ in noMatch }
    #expect(r.sent == ["close sell"] && r.failed)
    #expect(r.text.contains("still open") && r.text.contains("stop at 2,940"))

    // A short closes with a buy, and keeps its stop above.
    r = await run(position: Decimal(string: "-0.01")!, orders: [stop(5, "3060", side: "B")]) { _ in noMatch }
    #expect(r.sent == ["close buy"] && r.failed)
    #expect(r.text.contains("stop at 3,060"))

    // A part filled: the rest stays under the stop.
    r = await run { s in s.position = Decimal(string: "0.005")!; return filled("0.005") }
    #expect(r.sent == ["close sell"] && r.failed)
    #expect(r.text.contains("0.005 of 0.01 ETH closed") && r.text.contains("stop at 2,940"))

    // The reply is lost: the position read afterwards decides, and it is still open.
    r = await run()
    #expect(r.sent == ["close sell"] && r.failed)
    #expect(r.text.contains("still open") && r.text.contains("stop at 2,940"))

    // Lost, and the positions don't read either: it says so, and the stop is where it was.
    r = await run { s in s.positionsDown = true; return nil }
    #expect(r.sent == ["close sell"] && r.failed)
    #expect(r.text.contains("can't be read") && r.text.contains("stop at 2,940"))

    // The signer turns the close down: nothing is sent, and the refusal stays a refusal.
    r = await run(refuses: true)
    #expect(r.sent == [] && r.failed && r.text == "declined")

    // The stop filled first: the close is refused as reduce-only, the position is flat, the take that's left is cancelled.
    r = await run { s in
        s.position = 0
        s.orders.removeAll { ($0["oid"] as? UInt64) == 2 }
        return #"{"status":"ok","response":{"type":"order","data":{"statuses":[{"error":"Reduce only order would increase position. asset=0"}]}}}"#
    }
    #expect(r.sent == ["close sell", "cancel 3"] && !r.failed)
    #expect(r.text.contains("ETH position is closed") && r.text.contains("filled first"))

    // Only a limit entry waiting: it is cancelled, nothing to close.
    let limit: [String: Any] = ["coin": "ETH", "oid": 4, "isTrigger": false, "limitPx": "2900", "orderType": "Limit", "reduceOnly": false, "side": "B", "sz": "0.01"]
    r = await run(position: 0, orders: [limit])
    #expect(r.sent == ["cancel 4"] && !r.failed)
    #expect(r.text.contains("there was no position"))

    // The emergency close: no stop on the exchange, the close doesn't fill, restoreStop puts the stop back — once.
    r = await run(orders: [], close: { _ in noMatch }) { e in
        let unfinished = await #expect(throws: ExecutionError.self) { try await e.close(asset: "ETH") }
        #expect(unfinished?.message.contains("no stop on the exchange") == true)
        let market = try #require(try await Hyperliquid.markets()["ETH"])
        let first = await e.restoreStop(market, stop: 2940)
        return first + " | " + (await e.restoreStop(market, stop: 2940))
    }
    #expect(r.sent == ["close sell", "sl"] && !r.failed)
    #expect(r.text == "Its stop at 2,940 is back on the exchange now. | Its stop at 2,940 is on the exchange.")
}

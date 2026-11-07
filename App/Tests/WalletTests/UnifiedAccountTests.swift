import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
@testable import TradeExecution
import Testing
import WalletCore

/// A Hyperliquid account in `mode` (userAbstraction), shaped like a unified one read live on 2026-11-05: the perp state
/// holds only what positions use, the spot USDC is the whole account and its `hold` what positions and orders keep.
/// /exchange writes down each action's type and answers ok.
final class HLModeStub: URLProtocol {
    nonisolated(unsafe) static var mode = "unifiedAccount"
    nonisolated(unsafe) static var hold = "55.8"
    nonisolated(unsafe) static var sent: [String] = []
    static let lock = NSLock()

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "api.hyperliquid.xyz" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var data = Data()
        if let stream = request.httpBodyStream {
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 { data.append(buffer, count: n) }
            stream.close()
        }
        let body = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
        let reply: String = Self.lock.withLock {
            if request.url?.path == "/exchange" {
                Self.sent.append((body["action"] as? [String: Any])?["type"] as? String ?? "?")
                return #"{"status":"ok","response":{"type":"default"}}"#
            }
            switch body["type"] as? String {
            case "userAbstraction": return "\"\(Self.mode)\""
            case "clearinghouseState":
                return body["dex"] == nil ? #"{"marginSummary":{"accountValue":"55.8"},"withdrawable":"0.0","assetPositions":[]}"#
                    : #"{"marginSummary":{"accountValue":"30"},"withdrawable":"14","assetPositions":[]}"#
            case "spotClearinghouseState": return #"{"balances":[{"coin":"USDC","token":0,"total":"484.57","hold":"\#(Self.hold)"}]}"#
            case "spotMeta":
                return #"{"tokens":[{"name":"USDC","index":0,"szDecimals":8},{"name":"HYPE","index":150,"szDecimals":2}],"#
                    + #""universe":[{"tokens":[150,0],"index":107,"name":"@107"}]}"#
            case "allMids": return #"{"@107":"40"}"#
            case "maxBuilderFee": return "0"
            default: return "null"
            }
        }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(reply.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

extension FakeHyperliquid {
    /// A unified account (or portfolio margin) reads its money from the spot USDC — the whole account, less what positions
    /// hold — not «$0 free on perp», counts no HIP-3 balance beside it, and a spot buy larger than it moves nothing
    /// (no usdClassTransfer) and says what's free. A Manual account reads as before.
    @Test func unifiedAccountReadsOneBalanceAndMovesNothing() async throws {
        URLProtocol.registerClass(HLModeStub.self)
        defer { URLProtocol.unregisterClass(HLModeStub.self) }
        let user = "0x0000000000000000000000000000000000000001"
        for mode in ["unifiedAccount", "portfolioMargin"] {
            HLModeStub.lock.withLock { HLModeStub.mode = mode; HLModeStub.sent = [] }
            let b = try await Hyperliquid.balance(user: user)
            #expect(b.accountValue == Decimal(string: "484.57")! && b.withdrawable == Decimal(string: "428.77")! && b.spotUSDC == 0 && b.unified, "\(mode)")
            #expect(try await Hyperliquid.dexBalance(user: user, dex: "xyz") == (0, 0))
            // The portfolio doesn't list the same USDC again as a spot token.
            #expect(await Holdings.hyperliquidSpot(user: user)?.map(\.contract) == [])
        }

        let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("unified-\(UUID().uuidString).jsonl"))
        let e = Executor(wallet: TradeWallet(solana: "", evm: user), signer: ZeroSigner(), journal: journal) { _ in }
        let error = await #expect(throws: ExecutionError.self) { try await e.hlSpot(buy: true, symbol: "HYPE", usdc: 500, amount: nil) }
        #expect(error?.message == "428.77 USDC free on Hyperliquid: 500 is more. Nothing was signed.")
        #expect(HLModeStub.lock.withLock { HLModeStub.sent }.isEmpty)

        for mode in ["disabled", "default"] {
            HLModeStub.lock.withLock { HLModeStub.mode = mode }
            let b = try await Hyperliquid.balance(user: user)
            #expect(b.accountValue == Decimal(string: "55.8")! && b.withdrawable == 0 && b.spotUSDC == Decimal(string: "484.57")! && !b.unified, "\(mode)")
            #expect(try await Hyperliquid.dexBalance(user: user, dex: "xyz") == (30, 14))
            #expect(await Holdings.hyperliquidSpot(user: user)?.map(\.contract) == ["USDC"])
        }
    }

    /// Portfolio margin's spot USDC `hold` goes negative by what can be borrowed against HYPE or BTC (read live
    /// 2026-11-05: total 5.9M, hold −10.4M). That loan room isn't free money: «free» never reads above the account.
    @Test func portfolioMarginNegativeHoldIsNotFree() async throws {
        URLProtocol.registerClass(HLModeStub.self)
        defer { URLProtocol.unregisterClass(HLModeStub.self); HLModeStub.lock.withLock { HLModeStub.hold = "55.8"; HLModeStub.mode = "unifiedAccount" } }
        HLModeStub.lock.withLock { HLModeStub.mode = "portfolioMargin"; HLModeStub.hold = "-10" }
        let b = try await Hyperliquid.balance(user: "0x0000000000000000000000000000000000000001")
        #expect(b.accountValue == Decimal(string: "484.57")! && b.withdrawable == Decimal(string: "484.57")!)
    }
}

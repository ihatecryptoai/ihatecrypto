import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
@testable import TradeExecution
import Testing
import WalletCore

/// A Hyperliquid for a send to another account: userRole answers `roles` in turn as (status, body), the last one again
/// after; the sender has 100 USDC free; /exchange writes down each usdSend's amount and answers ok.
final class HLSendStub: URLProtocol {
    nonisolated(unsafe) static var roles: [(Int, String)] = []
    nonisolated(unsafe) static var asked = 0
    nonisolated(unsafe) static var sent: [String] = []
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
        let (status, reply): (Int, String) = Self.lock.withLock {
            if request.url?.path == "/exchange" {
                Self.sent.append((body["action"] as? [String: Any])?["amount"] as? String ?? "?")
                return (200, #"{"status":"ok","response":{"type":"default"}}"#)
            }
            switch body["type"] as? String {
            case "userRole":
                Self.asked += 1
                return Self.roles.count > 1 ? Self.roles.removeFirst() : Self.roles.first ?? (200, "{}")
            case "clearinghouseState": return (200, #"{"marginSummary":{"accountValue":"100"},"withdrawable":"100","assetPositions":[]}"#)
            case "spotClearinghouseState": return (200, #"{"balances":[]}"#)
            default: return (200, "null")
            }
        }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(reply.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// Signs a user action (the usdSend) with zeros; nothing else is asked of it here.
private struct UserSigner: TradeSigner {
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { EVMSignature(r: Data(count: 32), s: Data(count: 32), v: 27) }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError("no") }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError("no") }
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64, to: String, data: Data,
                 recipients: [String]) async throws -> Data { throw ExecutionError("no") }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError("no") }
}

extension FakeHyperliquid {
    /// userRole is read strictly: «missing» is a new account, any other role (one added later too) isn't, and no role — none
    /// at all, one that isn't text, a page that isn't JSON, an error status — is an error, not «not new».
    @Test func userRoleIsReadStrictly() async throws {
        URLProtocol.registerClass(HLSendStub.self)
        defer { URLProtocol.unregisterClass(HLSendStub.self) }
        func isNew(_ status: Int, _ body: String) async throws -> Bool {
            HLSendStub.lock.withLock { HLSendStub.roles = [(status, body)] }
            return try await Hyperliquid.isNewAccount("0x00000000000000000000000000000000000a1e70")
        }
        #expect(try await isNew(200, #"{"role":"missing"}"#))
        for role in ["user", "agent", "vault", "subAccount", "builder"] { #expect(try await isNew(200, #"{"role":"\#(role)"}"#) == false, "\(role)") }
        for (status, body) in [(200, "{}"), (200, #"{"role":42}"#), (200, "<html>"), (422, "Failed to deserialize"), (429, "")] {
            await #expect(throws: (any Error).self, "\(status) \(body)") { try await isNew(status, body) }
        }
    }

    /// A send to another Hyperliquid account asks whether it's new (a new one keeps 1 USDC of the first transfer). With no
    /// answer twice, a second apart, nothing is signed, sent or written down, and the chat says so; one answer is enough.
    @Test func sendStopsWhenHyperliquidWontSayTheAccountIsNew() async throws {
        URLProtocol.registerClass(HLSendStub.self)
        defer { URLProtocol.unregisterClass(HLSendStub.self) }
        func send(_ roles: [(Int, String)]) async -> (text: String, sent: [String], asked: Int, journal: Int) {
            HLSendStub.lock.withLock { HLSendStub.roles = roles; HLSendStub.sent = []; HLSendStub.asked = 0 }
            let journal = Journal(url: FileManager.default.temporaryDirectory.appendingPathComponent("hlsend-\(UUID().uuidString).jsonl"))
            let e = Executor(wallet: TradeWallet(solana: "", evm: "0x0000000000000000000000000000000000000001"), signer: UserSigner(), journal: journal) { _ in }
            let text: String
            do { text = try await e.sendUSDC(5, to: .hyperliquid("0x00000000000000000000000000000000000A1E70"), label: "Alex") } catch { text = error.localizedDescription }
            return HLSendStub.lock.withLock { (text, HLSendStub.sent, HLSendStub.asked, (try? journal.entries().count) ?? -1) }
        }
        let busy = await send([(429, "")])
        #expect(busy.text.contains("nothing was sent") && busy.sent.isEmpty && busy.journal == 0 && busy.asked == 2, "\(busy)")
        let blank = await send([(200, "{}")])
        #expect(blank.text.contains("nothing was sent") && blank.sent.isEmpty && blank.journal == 0, "\(blank)")
        let new = await send([(429, ""), (200, #"{"role":"missing"}"#)])
        #expect(new.sent == ["6"] && new.text.contains("plus 1 USDC") && new.asked == 2, "\(new)")
        let open = await send([(200, #"{"role":"user"}"#)])
        #expect(open.sent == ["5"] && !open.text.contains("plus") && open.asked == 1, "\(open)")
    }
}

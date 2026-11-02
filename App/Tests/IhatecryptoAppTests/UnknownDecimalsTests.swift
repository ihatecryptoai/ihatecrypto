@testable import IhatecryptoApp
import EVMChain
import Foundation
import IhatecryptoCore
import Testing
@testable import TradeExecution

/// A Base node that names its token «USDC» and answers decimals() with `decimals` (nil: a revert); the coin's stand-in
/// addresses have no code, so «0x», as a real node answers. Every other method is one it doesn't serve, so the review
/// falls back to a plain eth_call of the transaction, which goes through.
nonisolated final class DecimalsNode: URLProtocol {
    nonisolated(unsafe) static var decimals: String?

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "decimals.stub" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var body = Data()
        if let stream = request.httpBodyStream {   // URLProtocol gets the body as a stream, not httpBody
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 { body.append(buffer, count: n) }
            stream.close()
        }
        let j = (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]
        let call = (j["params"] as? [Any])?.first as? [String: Any]
        let data = (call?["to"] as? String).map { [EVMSimulation.native, UniswapV4.native].contains($0.lowercased()) } == true ? "none" : call?["data"] as? String
        let word = { (hex: String) in String(repeating: "0", count: 64 - hex.count) + hex }
        let result: String? = switch (j["method"] as? String, data) {
        case ("eth_call", "0x95d89b41"): "0x" + word("20") + word("4") + "55534443" + String(repeating: "0", count: 56)
        case ("eth_call", "0x313ce567"): Self.decimals.map { "0x" + word($0) }
        case ("eth_call", _): "0x"
        case ("eth_estimateGas", _): "0xea60"
        default: nil
        }
        let reply = result.map { #"{"jsonrpc":"2.0","id":1,"result":"\#($0)"}"# }
            ?? (j["method"] as? String == "eth_call" ? #"{"jsonrpc":"2.0","id":1,"error":{"code":3,"message":"execution reverted"}}"#
                                                      : #"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"method not supported"}}"#)
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(reply.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// A site's approve of 5,000,000 USDC (5·10¹² units) as the signing window shows it. Without decimals() the amount
/// can't be shown, so it isn't signed; 18 was guessed before, which showed «0.000005 USDC». Hardhat's public accounts.
@Suite(.serialized) struct UnknownDecimals {
    let usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
    let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    let spender = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"

    func review(decimals: String?, to: String? = nil, value: UInt64 = 0, data: Data? = nil) async throws -> EVMReview {
        URLProtocol.registerClass(DecimalsNode.self)
        defer { URLProtocol.unregisterClass(DecimalsNode.self) }
        DecimalsNode.decimals = decimals
        return await Browser.evmReview(to: to ?? usdc, value: value, data: try data ?? ERC20.approve(spender: spender, amount: 5_000_000_000_000),
                                       from: me, rpc: EVMRPC(url: URL(string: "https://decimals.stub")!, chainId: 8453))
    }

    @Test func anApproveWhoseTokenDoesntAnswerDecimalsIsRefused() async throws {
        let r = try await review(decimals: nil)
        #expect(r.lines[0].contains("an unknown amount of USDC (5000000000000 of its smallest units)"), "\(r.lines)")
        #expect(!r.lines[0].contains("0.000005"))
        #expect(r.problems.first == "USDC (0x8335…2913) didn't answer decimals(), so the amount is unknown. ihatecrypto doesn't sign an amount it can't show — if the network was slow, ask the site to send it again.",
                "\(r.problems)")
    }

    /// 2⁶³ trapped Int(UInt64) and stopped the app; past 36 a Decimal can't hold the amount.
    @Test func decimalsNoTokenHasAreRefusedNotATrap() async throws {
        for d in ["8000000000000000", "25", "ffffffffffffffffffffffffffffffff"] {
            let r = try await review(decimals: d)
            #expect(r.lines[0].contains("an unknown amount"), "\(d): \(r.lines)")
            #expect(r.problems.contains { $0.contains("didn't answer decimals()") }, "\(d)")
        }
    }

    @Test func anApproveWithDecimalsShowsItsAmount() async throws {
        let r = try await review(decimals: "6")
        #expect(r.lines[0] == "Allow \(spender.lowercased()) to spend up to \(Fmt.num(5_000_000, maxFraction: 6)) USDC.", "\(r.lines)")
        #expect(!r.problems.contains { $0.contains("decimals") }, "\(r.problems)")
    }

    /// The coin has no decimals() to ask: Uniswap v4's 0x0 (and KyberSwap's 0xeeee…) is ETH, 18, not a refusal.
    @Test func aSwapOfTheCoinIsntAskedForDecimals() async throws {
        let key = UniswapV4.PoolKey(currency0: UniswapV4.native, currency1: usdc, fee: 500, tickSpacing: 10, hooks: UniswapV4.native)
        let swap = try UniswapV4.execute(key, zeroForOne: true, amountIn: 1_000_000_000_000_000, amountOutMinimum: 3_000_000, deadline: 2_000_000_000)
        let r = try await review(decimals: "6", to: try #require(UniswapV4.on(8453)).router, value: 1_000_000_000_000_000, data: swap)
        #expect(r.lines[0] == "Uniswap v4 swap: 0.001 ETH for at least 3 USDC, paid to this wallet.", "\(r.lines)")
        #expect(!r.problems.contains { $0.contains("decimals") }, "\(r.problems)")
    }
}

/// The approvals board: an allowance of a token that doesn't answer decimals() says so, never 18's guess.
@Test func anApprovalWithoutDecimalsSaysTheAmountIsUnknown() {
    func row(_ decimals: Int?, _ units: Decimal) -> String {
        TokenApproval(chain: "base", token: "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", symbol: "USDC", decimals: decimals,
                      spender: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8", spenderName: nil, amount: units, permit2: false).shown
    }
    #expect(row(nil, 5_000_000_000_000) == "amount unknown: decimals() didn't answer")
    #expect(row(6, 5_000_000_000_000) == Fmt.num(5_000_000, maxFraction: 6))
    #expect(row(nil, Decimal(sign: .plus, exponent: 30, significand: 1)) == "unlimited")
}

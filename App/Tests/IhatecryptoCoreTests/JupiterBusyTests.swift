import Foundation
@testable import IhatecryptoCore
import Testing
import WalletCore

// Jupiter's keyless limit (5 requests in 10 s per IP, Nov 2026) answers the 6th with 429 and a JSON error. That answer
// is «Jupiter didn't answer», never «no such token» (TokenResearch.get, B-2 of the 2026-11-05 research).

@Test func aThrottledOrDownJupiterIsAskedAgainAndAnEmptyListIsNo() throws {
    for status in [429, 503] {
        let e = #expect(throws: RPCRetry.Again.self) { try TokenResearch.answer(Data(#"{"code":429}"#.utf8), status: status) }
        #expect(e?.again == true)
    }
    // A refusal a second ask won't change is final, and still no answer.
    let final = #expect(throws: RPCRetry.Again.self) { try TokenResearch.answer(Data(), status: 400) }
    #expect(final?.again == false)
    // 200 and nothing found: Jupiter answered no.
    let empty = try TokenResearch.answer(Data("[]".utf8), status: 200)
    #expect(TokenResearch.solanaMint(empty, symbol: "BONK") == nil)
    let bonk = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
    let found = try TokenResearch.answer(Data(#"[{"id": "\#(bonk)", "symbol": "Bonk", "isVerified": true, "liquidity": 1000}]"#.utf8), status: 200)
    #expect(TokenResearch.solanaMint(found, symbol: "BONK") == bonk)
}

@Test func jupiterStillThrottledAfterTheRetriesSaysItDidntAnswer() async throws {
    let url = URL(string: "https://api.jup.ag/tokens/v2/search?query=BONK")!
    do {
        _ = try await RPCRetry.run(delays: []) {
            (Data(#"{"code":429,"message":"Too many requests"}"#.utf8), HTTPURLResponse(url: url, statusCode: 429, httpVersion: nil, headerFields: nil)!)
        } answer: { data, status in try TokenResearch.answer(data, status: status) }
        Issue.record("a 429 came back as an answer")
    } catch let e as WalletError {
        #expect(e.message.contains("didn't answer"))
        #expect(!e.message.contains(";"))
    }
}

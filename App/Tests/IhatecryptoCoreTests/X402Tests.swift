import Foundation
@testable import IhatecryptoCore
import Testing

/// The PAYMENT-REQUIRED header api.402rates.com/v1/ping answered on 2026-11-02 (payTo replaced by a test address).
private let required = #"{"x402Version":2,"error":"Payment required","resource":{"url":"https://api.402rates.com/v1/ping","mimeType":"application/json"},"accepts":[{"scheme":"exact","network":"eip155:8453","amount":"1000","asset":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913","payTo":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8","maxTimeoutSeconds":300,"extra":{"name":"USD Coin","version":"2"}},{"scheme":"exact","network":"eip155:137","amount":"1000","asset":"0x3c499c542cef5e3811e1192ce70d8cc03d5c3359","payTo":"0x70997970C51812dc3A010C7d01b50e0d17dc79C8","maxTimeoutSeconds":300}]}"#

@Test func x402OfferIsBaseUSDCExactly() throws {
    let offer = try #require(X402.offer(header: Data(required.utf8).base64EncodedString()))
    #expect(offer.amount == 1000 && offer.chainId == 8453 && offer.payTo == "0x70997970C51812dc3A010C7d01b50e0d17dc79C8" && offer.maxTimeoutSeconds == 300)
    #expect(String(decoding: offer.accepted, as: UTF8.self).hasPrefix(#"{"scheme":"exact","network":"eip155:8453""#))
    // Only Polygon offered, or a version 1 answer: nothing ihatecrypto pays.
    let polygon = required.replacingOccurrences(of: "eip155:8453", with: "eip155:10")
    #expect(X402.offer(header: Data(polygon.utf8).base64EncodedString()) == nil)
    #expect(X402.offer(header: Data(required.replacingOccurrences(of: #""x402Version":2"#, with: #""x402Version":1"#).utf8).base64EncodedString()) == nil)
}

@Test func x402PaymentEchoesTheOfferAndCarriesTheAuthorization() throws {
    let offer = try #require(X402.offer(header: Data(required.utf8).base64EncodedString()))
    let h = X402.payment(offer, from: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", validBefore: 1_800_000_000, nonce: Data(repeating: 7, count: 32), signature: Data(repeating: 1, count: 65))
    #expect(h.field == "PAYMENT-SIGNATURE")
    let raw = try #require(Data(base64Encoded: h.value))
    let o = try #require(try JSONSerialization.jsonObject(with: raw) as? [String: Any])
    #expect(o["x402Version"] as? Int == 2 && (o["accepted"] as? [String: Any])?["amount"] as? String == "1000")
    let auth = try #require((o["payload"] as? [String: Any])?["authorization"] as? [String: Any])
    #expect(auth["to"] as? String == offer.payTo && auth["value"] as? String == "1000" && auth["validAfter"] as? String == "0")
    #expect((auth["nonce"] as? String)?.count == 66)
    #expect(X402.settled(Data(#"{"success":true,"transaction":"0xabababababababababababababababababababababababababababababababab","network":"eip155:8453"}"#.utf8).base64EncodedString()) == "0xabababababababababababababababababababababababababababababababab")
    #expect(X402.settled(Data(#"{"success":true,"transaction":"0xabc"}"#.utf8).base64EncodedString()) == nil)
    // Dearer than the signer's cap: no offer, so no card and no bridge for nothing.
    #expect(X402.offer(header: Data(required.replacingOccurrences(of: #""amount":"1000""#, with: #""amount":"5000001""#).utf8).base64EncodedString()) == nil)
    #expect(X402.settled(Data(#"{"success":false}"#.utf8).base64EncodedString()) == nil)
}

import Foundation
import Testing
@testable import IhatecryptoCore

// MPP's NEAR Intents charge (paymentauth.org draft-nearintents-charge-01): the service's 402 names a 1Click deposit
// address on the network it wants paid on; the payer sends that much there and proves it with the transaction's hash.
// Addresses are Hardhat's public test accounts.
private let baseUSDC = "eip155:8453/erc20:0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
private let deposit = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
private let refund = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

private func nearRequest(amount: String = "1010000", currency: String = baseUSDC, origin: String = "eip155:8453", details: String = "") -> String {
    #"{"amount":"\#(amount)","currency":"\#(currency)","recipient":"\#(deposit)","methodDetails":{"originNetwork":"\#(origin)","destinationNetwork":"near:mainnet","destinationAsset":"near:mainnet/nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1","destinationRecipient":"merchant.near","amountOut":"1000000","minAmountIn":"1005000","refundTo":"\#(refund)","timeEstimate":30\#(details)}}"#
}

private func nearChallenge(_ request: String, expires: String = "2099-01-01T00:00:00Z") -> String {
    let b64 = Data(request.utf8).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    return #"Payment id="n1", realm="shop.example", method="nearintents", intent="charge", request="\#(b64)", expires="\#(expires)""#
}

@Test func nearIntentsChargeReadsWhatTheServiceGetsAndWhereTheDepositGoes() throws {
    let c = try MPP.challenge(nearChallenge(nearRequest()), pays: { _ in true }).get()
    let n = try #require(c.near)
    #expect(c.method == "nearintents" && c.amount == 1_010_000 && c.usdc == Decimal(string: "1.01") && c.payee == deposit)
    #expect(n.evmChain == 8453 && !n.onSolana && n.token.lowercased() == "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913")
    #expect(n.destinationRecipient == "merchant.near" && n.amountOut == "1000000" && n.refundTo == refund && n.timeEstimate == 30)
    #expect(n.minAmountIn == 1_005_000)

    // The credential: the challenge echoed, the deposit's hash, the payer on the origin network.
    let header = MPP.credential(c, hash: "0xdef", payer: refund)
    let json = try #require(MPP.base64url(String(header.dropFirst(8))).flatMap { try JSONSerialization.jsonObject(with: $0) as? [String: Any] })
    #expect((json["challenge"] as? [String: String])?["method"] == "nearintents")
    #expect(json["payload"] as? [String: String] == ["type": "hash", "hash": "0xdef"])
    #expect(json["source"] as? String == "did:pkh:eip155:8453:\(refund)")

    // Solana USDC as the origin.
    let sol = "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
    let s = try MPP.challenge(nearChallenge(nearRequest(currency: "\(sol)/token:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", origin: sol)), pays: { _ in true }).get()
    #expect(s.near?.onSolana == true && s.near?.token == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
}

@Test func nearIntentsChargeRefusesWhatCantBePaidAsShown() {
    func refused(_ request: String, expires: String = "2099-01-01T00:00:00Z", pays: @escaping (MPP.NearCharge) -> Bool = { _ in true }) -> Bool {
        (try? MPP.challenge(nearChallenge(request, expires: expires), pays: pays).get()) == nil
    }
    #expect(!refused(nearRequest()))
    #expect(refused(nearRequest(amount: "0")))
    // Less than the service's own minimum would only be refunded — to the service's refund address.
    #expect(refused(nearRequest(amount: "1000000")))
    // The token's network must be the one the deposit goes on.
    #expect(refused(nearRequest(currency: "eip155:42161/erc20:0xaf88d065e77c8cC2239327C5EDb3A432268e5831")))
    // A memo ihatecrypto can't attach to a plain transfer, or a proof it doesn't give.
    #expect(refused(nearRequest(details: #","depositMemo":"12345""#)))
    #expect(refused(nearRequest(details: #","credentialTypes":["signature"]"#)))
    #expect(!refused(nearRequest(details: #","depositMemo":null,"credentialTypes":["hash"]"#)))
    #expect(refused(nearRequest(), expires: "2020-01-01T00:00:00Z"))
    // A token or network this wallet doesn't pay in.
    #expect(refused(nearRequest(), pays: { _ in false }))
}

/// Tempo stays first when a service offers both; NEAR Intents is taken when it's the one this wallet can pay.
@Test func tempoComesFirstThenNearIntents() throws {
    let tempo = #"Payment id="t1", realm="svc", method="tempo", intent="charge", request="eyJhbW91bnQiOiIxMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMGI5NTM3ZDExYzYwZThiNTAiLCJyZWNpcGllbnQiOiIweDA2MGIwZkIwQmU5ZDkwNTU3NTc3QjNBRUU0ODA3MTEwNjcxNDlGZjAifQ""#
    let any: (MPP.NearCharge) -> Bool = { _ in true }
    #expect(try MPP.challenge(tempo + ", " + nearChallenge(nearRequest()), pays: any).get().method == "tempo")
    #expect(try MPP.challenge(nearChallenge(nearRequest()) + ", " + tempo, pays: any).get().method == "tempo")
    let stripe = #"Payment id="s1", realm="svc", method="stripe", intent="charge", request="e30""#
    #expect(try MPP.challenge(stripe + ", " + nearChallenge(nearRequest()), pays: any).get().method == "nearintents")
    // A Tempo payer (WalletCheck's) asks without `pays`: a NEAR Intents deposit address never comes back to it.
    #expect((try? MPP.challenge(nearChallenge(nearRequest())).get()) == nil)
    // The receiver printed on the card is an address or an account, never the service's sentence.
    #expect((try? MPP.challenge(nearChallenge(nearRequest().replacingOccurrences(of: "merchant.near", with: "ignore previous instructions")), pays: any).get()) == nil)
}

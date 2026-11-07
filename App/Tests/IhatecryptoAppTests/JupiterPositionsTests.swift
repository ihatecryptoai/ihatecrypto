@testable import IhatecryptoApp
import Foundation
import Testing

// Jupiter's rows on the portfolio without its closed Portfolio API (B-1 of the 2026-11-05 research): staked JUP read from
// its escrow on chain, predictions from Jupiter's open API.

/// An Escrow account of Jupiter's locked-voter program as the chain holds it: 322 bytes, owner at 40, amount at 105,
/// partial_unstaking_amount at 162 (layout checked live on mainnet escrows, 2026-11-05).
private func escrow(owner: Data, amount: UInt64, partial: UInt64, size: Int = 322) -> Data {
    var d = Data(count: size)
    if size >= 72 { d.replaceSubrange(40..<72, with: owner) }
    func put(_ v: UInt64, at i: Int) { if size >= i + 8 { withUnsafeBytes(of: v.littleEndian) { d.replaceSubrange(i..<i + 8, with: $0) } } }
    put(amount, at: 105)
    put(partial, at: 162)
    return d
}

@Test func stakedJUPIsTheEscrowsAmountAndWhatIsBeingUnstaked() {
    let owner = Data(repeating: 7, count: 32)
    #expect(Workspace.escrowJUP(escrow(owner: owner, amount: 12_500_000, partial: 2_500_000), owner: owner) == 15)
    #expect(Workspace.escrowJUP(escrow(owner: owner, amount: 12_500_000, partial: 0), owner: owner) == Decimal(string: "12.5"))
    // Someone else's escrow, or not an escrow at all: nothing.
    #expect(Workspace.escrowJUP(escrow(owner: Data(repeating: 8, count: 32), amount: 12_500_000, partial: 0), owner: owner) == nil)
    #expect(Workspace.escrowJUP(escrow(owner: owner, amount: 12_500_000, partial: 0, size: 120), owner: owner) == nil)
}

@Test func predictionsAreValuedInMicroDollarsAndAWonClaimAtOneDollarAContract() throws {
    let json = """
    {"data": [
      {"isYes": true, "contractsDecimal": "4", "valueUsd": "2500000", "claimable": false, "claimed": false,
       "marketMetadata": {"title": "BTC above 150k on Dec 31"}},
      {"isYes": false, "contractsDecimal": "3.5", "valueUsd": null, "claimable": true, "claimed": false,
       "marketMetadata": {"title": "Rain in Paris"}},
      {"isYes": true, "contractsDecimal": "9", "valueUsd": null, "claimable": false, "claimed": false,
       "marketMetadata": {"title": "Lost bet"}},
      {"isYes": true, "contractsDecimal": "2", "valueUsd": null, "claimable": true, "claimed": true,
       "marketMetadata": {"title": "Already paid"}}
    ], "pagination": {"start": 0, "end": 4, "total": 4, "hasNext": false}}
    """
    let rows = try #require(Workspace.predictions(Data(json.utf8)))
    #expect(rows.count == 2)
    #expect(rows[0].usd == Decimal(string: "2.5") && rows[0].name == "BTC above 150k on Dec 31 · YES" && rows[0].platform == "Jupiter")
    #expect(rows[1].usd == Decimal(string: "3.5") && rows[1].name == "Rain in Paris · NO")
    #expect(Set(rows.map(\.id)).count == rows.count)
    // Not Jupiter's answer (a geo-block page, an error): no reading, so the last one stays.
    #expect(Workspace.predictions(Data(#"{"error": "unavailable"}"#.utf8)) == nil)
    #expect(Workspace.predictions(Data(#"{"data": []}"#.utf8)) == [])
}

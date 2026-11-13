import Foundation
import SignerAPI
import SolanaChain
import Testing
import TxGuard
import WalletCore

/// The card's limits at the signer: what the guard reads leaving a transfer fits the card, and the same transfer with a
/// doubled amount or another approved payee doesn't — the gaps the 2026-11-13 attack drill left to the prompt alone.
@Test func capHoldsATransferToItsCard() throws {
    let me = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    let them = Base58.encode(Data(repeating: 7, count: 32)), other = Base58.encode(Data(repeating: 9, count: 32))
    func flows(to owner: String, units: UInt64) throws -> [(asset: String, units: Decimal, to: String?)] {
        let wire = SolanaTransaction.unsigned(try SPL.transferMessage(payer: me, to: owner, mint: usdc, amount: units, decimals: 6, blockhash: me))
        let m = try SolanaMessage.parse(transaction: wire)
        let owners = [try SPL.associatedAccount(owner: me, mint: usdc): me, try SPL.associatedAccount(owner: owner, mint: usdc): owner]
        let v = SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: me, recipients: [them, other]), tokenOwners: owners)
        #expect(v.ok)
        return v.outflows.map { ($0.asset, Decimal($0.units), $0.to) }
    }
    let cap = SignerCap(asset: usdc, units: 500_000 as UInt64, payee: them, card: "0.5 USDC to Them")
    #expect(cap.refusal(try flows(to: them, units: 500_000)) == nil)
    #expect(cap.refusal(try flows(to: them, units: 499_999)) == nil)
    #expect(cap.refusal(try flows(to: them, units: 1_000_000))?.contains("at most 500000") == true)
    #expect(cap.refusal(try flows(to: other, units: 500_000))?.contains(other) == true)
}

@Test func capHoldsASwapAndOtherCoins() {
    let usdc = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
    let swap = SignerCap(asset: usdc.lowercased(), units: Decimal(100_000), payee: nil, card: "0.1 USDC → SOL")
    #expect(swap.refusal([(usdc, 100_000, nil)]) == nil)
    #expect(swap.refusal([(usdc, 100_000, "0x" + String(repeating: "ab", count: 20))]) != nil)
    #expect(swap.refusal([(usdc, 100_000, nil), ("native", 1, "0x" + String(repeating: "ab", count: 20))])?.contains("moves only") == true)
    #expect(swap.refusal([]) != nil)
    let send = SignerCap(asset: "native", units: UInt64(5), payee: "0xABCDEF0000000000000000000000000000000001", card: "5 wei")
    #expect(send.refusal([("native", 5, "0xabcdef0000000000000000000000000000000001")]) == nil)
    #expect(SignerCap(asset: "SOL", units: "lots", payee: nil, card: "?").refusal([]) != nil)
}

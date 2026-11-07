import ExecutionCore
@testable import IhatecryptoApp
import Testing

/// «Check before trying again: <links>» after a move with no outcome. A swap with no gas of ours waits in the journal under
/// our signature, the second, which no explorer finds (only the sponsor's, the first): its link is the wallet's page.
@Test func anUnsettledSponsoredSwapLinksTheWalletsPage() {
    let sponsored = Journal.Entry(chain: "solana", id: "ourSig", kind: .signed, note: "Jupiter USDC→SOL, fee paid by Jupiter; Jupiter order r1, valid until block 9")
    #expect(Workspace.whereToLook(sponsored, solana: "Wallet111") == "https://solscan.io/account/Wallet111")
    let own = Journal.Entry(chain: "solana", id: "sig", kind: .signed, note: "Jupiter USDC→SOL; valid until block 9")
    #expect(Workspace.whereToLook(own, solana: "Wallet111") == "https://solscan.io/tx/sig")
    // What went through (Journal.wentThrough) already carries the sponsor's signature: that transaction itself.
    let went = Journal.Entry(chain: "solana", id: "sponsorSig", kind: .confirmed, note: sponsored.note)
    #expect(Workspace.whereToLook(went, solana: "Wallet111") == "https://solscan.io/tx/sponsorSig")
}

@testable import IhatecryptoApp
import Foundation
import SolanaChain
import Testing

/// A site's Solana transaction as the signing sheet shows it: each balance of ours after the simulation. A token account
/// the transaction closes comes back from the node as null, (0, nil) once parsed; it held the tokens before, so they all
/// leave. Public test addresses only («abandon … about» and JUP's mint).
private let me = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm"
private let jup = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN"
private let rent: UInt64 = 2_039_280
private let held: UInt64 = 1_250_000_000   // 1,250 JUP, 6 decimals

private func jupAccount() throws -> String { try SPL.associatedAccount(owner: me, mint: jup) }

private func review(_ b: Browser.Balances) -> TxReview {
    var r = TxReview(wire: Data(), accounts: [], tokenOwners: [:], unknownPrograms: [], programNames: [], checks: [], problems: b.problems)
    r.deltas = b.deltas
    r.closed = b.closed
    r.rentBack = b.rentBack
    return r
}

@Test func closedTokenAccountCountsAsEmpty() throws {
    let account = try jupAccount()
    let b = Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (jup, held, 6, rent)],
                             after: [me: (12_034_280, nil), account: (0, nil)])
    #expect(b.problems.isEmpty)
    #expect(b.deltas.first { $0.mint == jup }?.amount == -1250)
    #expect(b.closed.map { b.deltas[$0].mint } == [jup])
    #expect(b.rentBack == Decimal(rent) / 1_000_000_000)
}

@Test func rentBackFromAClosedAccountIsNotAGain() throws {
    let account = try jupAccount()
    let r = review(Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (jup, held, 6, rent)],
                                    after: [me: (12_034_280, nil), account: (0, nil)]))
    let sol = try #require(r.deltas.first { $0.symbol == "SOL" })
    #expect(sol.amount == Decimal(string: "0.00203428"))
    #expect(r.feeLike(sol))
    #expect(r.summary == "You send 1,250 JUP")
    // Jev reads the same rows in words.
    #expect(r.changes.contains("JUP: −1,250 (account closes)"))
}

@Test func drainedWalletShowsAllItsSOL() {
    let b = Browser.balances(owner: me, solBefore: 5_000_000, touched: [:], after: [me: (0, nil)])
    #expect(b.problems.isEmpty)
    #expect(b.deltas.first?.amount == Decimal(string: "-0.005"))
}

@Test func accountMissingFromTheSimulationRefuses() throws {
    let account = try jupAccount()
    let noToken = Browser.balances(owner: me, solBefore: 5_000_000, touched: [account: (jup, held, 6, rent)], after: [me: (5_000_000, nil)])
    #expect(noToken.problems.contains { $0.contains("balance of JUP:") })
    #expect(noToken.deltas.allSatisfy { $0.mint != jup })
    let noWallet = Browser.balances(owner: me, solBefore: 5_000_000, touched: [account: (jup, held, 6, rent)], after: [account: (rent, held)])
    #expect(noWallet.problems.contains { $0.contains("SOL") })
}

@Test func untouchedAccountShowsNoRow() throws {
    let account = try jupAccount()
    let b = Browser.balances(owner: me, solBefore: 5_000_000, touched: [account: (jup, held, 6, rent)],
                             after: [me: (4_995_000, nil), account: (rent, held)])
    #expect(b.deltas.map(\.symbol) == ["SOL"])
    #expect(b.closed.isEmpty && b.rentBack == 0)
}

@Test func unreadableAccountIsNotCalledClosed() throws {
    // Still there (it holds its rent) but not readable as a token account: shown as emptied, the safe side, never «closes».
    let account = try jupAccount()
    let b = Browser.balances(owner: me, solBefore: 5_000_000, touched: [account: (jup, held, 6, rent)],
                             after: [me: (4_995_000, nil), account: (rent, nil)])
    #expect(b.deltas.first { $0.mint == jup }?.amount == -1250)
    #expect(b.closed.isEmpty && b.rentBack == 0)
}

@Test func hugeRawBalanceDoesNotTrap() throws {
    let account = try jupAccount()
    let b = Browser.balances(owner: me, solBefore: 5_000_000, touched: [account: (jup, .max, 6, rent)],
                             after: [me: (5_000_000, nil), account: (0, nil)])
    #expect(b.deltas.first { $0.mint == jup }?.amount == -(Decimal(UInt64.max) / 1_000_000))
}

@Test func solGainBeyondTheRentStaysAGain() throws {
    let account = try jupAccount()
    let r = review(Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (jup, held, 6, rent)],
                                    after: [me: (510_000_000, nil), account: (0, nil)]))
    let sol = try #require(r.deltas.first { $0.symbol == "SOL" })
    #expect(!r.feeLike(sol))
    #expect(r.summary == "You send 1,250 JUP and get 0.5 SOL")
}

@Test func solLossBesideAClosedAccountIsTheFee() throws {
    // The rent went to someone else: the wallet only pays the fee, and the JUP row still says the account closes.
    let account = try jupAccount()
    let r = review(Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (jup, held, 6, rent)],
                                    after: [me: (9_995_000, nil), account: (0, nil)]))
    #expect(r.feeLike(try #require(r.deltas.first { $0.symbol == "SOL" })))
    #expect(r.summary == "You send 1,250 JUP")
    #expect(r.changes.contains("JUP: −1,250 (account closes)"))
}

@Test func wrappedSOLIsNotTheWalletsSOL() throws {
    // Unwrapping 1 wSOL closes its account: the rent is only what it held beyond the wrapped SOL, and the wallet's own
    // SOL row (the same mint underneath) is neither «closes» nor rent.
    let wsol = "So11111111111111111111111111111111111111112"
    let account = try SPL.associatedAccount(owner: me, mint: wsol)
    let r = review(Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (wsol, 1_000_000_000, 9, 1_000_000_000 + rent)],
                                    after: [me: (1_012_034_280, nil), account: (0, nil)]))
    #expect(r.closed.map { r.deltas[$0].symbol } == ["wSOL"])
    #expect(r.rentBack == Decimal(rent) / 1_000_000_000)
    #expect(!r.feeLike(r.deltas[0]))
    #expect(r.summary == "You send 1 wSOL and get 1.002034 SOL")
    #expect(r.changes == ["SOL: +1.00203428", "wSOL: −1 (account closes)"])
    // A small wSOL outflow is a send, not the network fee.
    let small = review(Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (wsol, 1_000_000_000, 9, 1_000_000_000 + rent)],
                                        after: [me: (9_995_000, nil), account: (990_000_000 + rent, 990_000_000)]))
    #expect(small.summary == "You send 0.01 wSOL")
}

@Test func onlyTheAccountThatClosesSaysSo() throws {
    // Two JUP accounts: the empty one closes, the other loses 1,000 of its 1,250 — that row isn't «all of it».
    let account = try jupAccount(), spare = "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw"
    let r = review(Browser.balances(owner: me, solBefore: 10_000_000, touched: [account: (jup, held, 6, rent), spare: (jup, 0, 6, rent)],
                                    after: [me: (12_034_280, nil), account: (rent, 250_000_000), spare: (0, nil)]))
    #expect(r.deltas.first { $0.mint == jup }?.amount == -1000)
    #expect(r.closed.isEmpty && r.rentBack == Decimal(rent) / 1_000_000_000)
    #expect(!r.changes.contains { $0.contains("account closes") })
}

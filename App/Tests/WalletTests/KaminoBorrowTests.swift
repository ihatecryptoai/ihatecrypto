@testable import DeFiKamino
import ExecutionCore
import Foundation
import IhatecryptoCore
@testable import TradeExecution
import Testing
import TxGuard

/// A loan on Kamino's Backpack market, worked out before anything is signed: 0.05 SPCX at $150 ($7.50) put up, borrowed
/// against at most 30% of it, liquidated at 50%.
@Test func kaminoLoanMathKeepsTheHealthAndNamesTheLiquidationPrice() {
    let d = { (s: String) in Decimal(string: s)! }
    let spcx = Kamino.Risk(maxLTV: d("0.3"), liquidationLTV: d("0.5"), borrowFactor: d("1.5"))
    // A first loan: nothing there yet.
    let first = Executor.kaminoAfter(nil, adding: d("0.05"), held: 0, price: 150, risk: spcx, usdc: 1, borrowFactor: 1)
    #expect(first.collateral == d("7.5"))
    #expect(first.available == d("2.25"))
    #expect(first.health == d("3.75"))
    // Liquidation when 0.05 SPCX × p × 0.5 = $1: p = $40.
    #expect(first.liquidation == 40)
    // More on the same position: its own limits plus the new stock's.
    let loan = Kamino.Loan(collateral: [.init(reserve: "r", symbol: "SPCX", mint: "m", tokens: d("0.05"), valueUSD: d("7.5"))],
                           debts: [.init(reserve: "u", symbol: "USDC", tokens: 1)], depositUSD: d("7.5"), debtUSD: 1,
                           borrowLimitUSD: d("2.25"), liquidationLimitUSD: d("3.75"), adjustedDebtUSD: 1)
    #expect(loan.health == d("3.75"))
    let more = Executor.kaminoAfter(loan, adding: d("0.05"), held: d("0.05"), price: 150, risk: spcx, usdc: 2, borrowFactor: 1)
    #expect(more.available == d("3.5"))
    #expect(more.debt == 3)
    #expect(more.health == d("2.5"))
    #expect(more.liquidation == 60)
    // A borrow ihatecrypto would refuse: past 1.5.
    let deep = Executor.kaminoAfter(nil, adding: d("0.05"), held: 0, price: 150, risk: spcx, usdc: d("2.6"), borrowFactor: 1)
    #expect(deep.health! < Executor.minimumHealth)
    // What may leave with $1 owed: what's left must carry it at 30%, 1% to spare.
    #expect(Executor.kaminoFreeCollateral(loan, maxLTV: d("0.3"), liquidationLTV: d("0.5"), priceUSD: 150) == d("1.25") / d("0.3") / 150 * d("0.99"))
}

/// A loan against SOL on Kamino's main market (its numbers on 2026-11-05: borrow up to 74%, liquidation at 80%): 0.5 SOL
/// at $96 put up, 20 USDC borrowed. The position's own liquidation price agrees with the card's; a SOL deposit's rent
/// is what left beyond the SOL itself.
@Test func kaminoLoanAgainstSOLNamesItsLiquidationPriceAndRent() {
    let d = { (s: String) in Decimal(string: s)! }
    let sol = Kamino.Risk(maxLTV: d("0.74"), liquidationLTV: d("0.8"), borrowFactor: d("1.25"))
    let first = Executor.kaminoAfter(nil, adding: d("0.5"), held: 0, price: 96, risk: sol, usdc: 20, borrowFactor: 1)
    #expect(first.collateral == 48)
    #expect(first.available == d("35.52"))
    #expect(first.health == d("1.92"))
    // 0.5 SOL × p × 0.8 = $20: p = $50.
    #expect(first.liquidation == 50)
    let loan = Kamino.Loan(collateral: [.init(reserve: "r", symbol: "SOL", mint: "So11111111111111111111111111111111111111112", tokens: d("0.5"), valueUSD: 48)],
                           debts: [.init(reserve: "u", symbol: "USDC", tokens: 20)], depositUSD: 48, debtUSD: 20,
                           borrowLimitUSD: d("35.52"), liquidationLimitUSD: d("38.4"), adjustedDebtUSD: 20)
    #expect(loan.liquidationPrice == 50)
    // What may leave with 20 USDC owed: Kamino's 74% limit would free $20.97 and leave a health factor of 1.08; the 1.5
    // line frees $10.50 (38.4 − 8.4 = 30 = 1.5 × 20), 1% to spare.
    #expect(Executor.kaminoFreeCollateral(loan, maxLTV: d("0.74"), liquidationLTV: d("0.8"), priceUSD: 96) == d("10.5") / 96 * d("0.99"))
    // No debt, or two coins up: no single price to name.
    #expect(Kamino.Loan(collateral: loan.collateral, debts: [], depositUSD: 48, debtUSD: 0, borrowLimitUSD: d("35.52"),
                        liquidationLimitUSD: d("38.4"), adjustedDebtUSD: 0).liquidationPrice == nil)
    #expect(Kamino.Loan(collateral: loan.collateral + loan.collateral, debts: loan.debts, depositUSD: 96, debtUSD: 20, borrowLimitUSD: 71,
                        liquidationLimitUSD: d("76.8"), adjustedDebtUSD: 20).liquidationPrice == nil)
    // A first 0.1 SOL deposit's simulation (2026-11-05): 0.13837 SOL left the wallet, 0.03837 of it rent and fee.
    #expect(Executor.kaminoRentSpent(["SOL: -0.13837 (fee and rent for new accounts)"], deposited: d("0.1")) == d("0.03837"))
    #expect(Executor.kaminoRentSpent(["USDC: -1000000 (raw units)", "SOL: -0.0298 (fee and rent for new accounts)"], deposited: 0) == d("0.0298"))
    #expect(Kamino.market(for: "SOL") == Kamino.mainMarket)
}

@Test func backpackStocksAreTheSameInTheChatAndOnKamino() {
    #expect(Set(WalletCommand.kaminoStockMints.keys) == Set(Kamino.backpackStocks.keys))
    #expect(WalletCommand.kaminoStockMints.keys.allSatisfy { Kamino.market(for: $0) == Kamino.backpackMarket })
    #expect(Kamino.market(for: "USDC") == Kamino.mainMarket)
}

/// A borrow or a withdrawal brings no more than the card said (the simulation's ceiling), and a withdrawal right after
/// a borrow on the same market waits a minute: Kamino's API, which the 1.5 line reads, lags the borrow.
@Test func kaminoPayoutsHaveACeilingAndAWithdrawalWaitsOutABorrow() async throws {
    // 1 USDC asked, 1% either way: ten times it is refused.
    #expect(SolanaGuard.balances([.init(label: "USDC", before: 0, after: 1_000_000, atLeast: 990_000, atMost: 1_010_000)]).ok)
    #expect(!SolanaGuard.balances([.init(label: "USDC", before: 0, after: 10_000_000, atLeast: 990_000, atMost: 1_010_000)]).ok)
    #expect(SolanaGuard.balances([.init(label: "USDC", before: 0, after: 10_000_000, atLeast: 990_000)]).ok)
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("kamino-journal-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    let e = Executor(wallet: TradeWallet(solana: "", evm: ""), signer: ForkSigner(key: Data(count: 32), address: "", batches: false), journal: journal) { _ in }
    #expect(await !e.kaminoBorrowedJustNow(on: Kamino.mainMarket))
    try journal.append(.init(at: Date().addingTimeInterval(-90), chain: "solana", id: "a", kind: .signed, note: "Kamino borrow 1 USDC on the main market; valid until block 1"))
    #expect(await !e.kaminoBorrowedJustNow(on: Kamino.mainMarket))
    try journal.append(.init(chain: "solana", id: "b", kind: .signed, note: "Kamino borrow 1 USDC on the main market; valid until block 2"))
    #expect(await e.kaminoBorrowedJustNow(on: Kamino.mainMarket))
    #expect(await !e.kaminoBorrowedJustNow(on: Kamino.backpackMarket))
}

/// «Repay» finds the loan by what it stands on: SOL on the main market, a Backpack stock on the Backpack one, and with
/// nothing named the Backpack market first (a wallet with both loans pays back the one it names, not always Backpack's).
@Test func repayLooksForTheLoanOnTheMarketOfItsCollateral() {
    #expect(Kamino.markets(against: "SOL") == [Kamino.mainMarket])
    #expect(Kamino.markets(against: "SPCX") == [Kamino.backpackMarket])
    #expect(Kamino.markets(against: nil) == [Kamino.backpackMarket, Kamino.mainMarket])
}

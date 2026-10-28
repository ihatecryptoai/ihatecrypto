@testable import DeFiKamino
import Foundation
import IhatecryptoCore
@testable import TradeExecution
import Testing

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
    #expect(Executor.kaminoFreeCollateral(loan, maxLTV: d("0.3"), priceUSD: 150) == d("1.25") / d("0.3") / 150 * d("0.99"))
}

@Test func backpackStocksAreTheSameInTheChatAndOnKamino() {
    #expect(Set(WalletCommand.kaminoStockMints.keys) == Set(Kamino.backpackStocks.keys))
    #expect(WalletCommand.kaminoStockMints.keys.allSatisfy { Kamino.market(for: $0) == Kamino.backpackMarket })
    #expect(Kamino.market(for: "USDC") == Kamino.mainMarket)
}

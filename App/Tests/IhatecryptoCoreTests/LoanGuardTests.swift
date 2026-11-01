import Foundation
@testable import IhatecryptoCore
import Testing

@Test func loanGuardFormsParseInEnglishAndRussian() {
    let d = { Decimal(string: $0)! }
    #expect(LoanGuardCommand.parse("guard my loan on kamino") == .set(loan: "kamino", network: nil, below: nil))
    #expect(LoanGuardCommand.parse("guard my loan on aave on arbitrum below 1.3") == .set(loan: "aave", network: "arbitrum", below: d("1.3")))
    #expect(LoanGuardCommand.parse("guard my loan on Aave") == .set(loan: "aave", network: nil, below: nil))
    #expect(LoanGuardCommand.parse("следи за займом на камино") == .set(loan: "kamino", network: nil, below: nil))
    #expect(LoanGuardCommand.parse("не дай ликвидировать мой займ на aave в арбитрум, если health ниже 1,4")
        == .set(loan: "aave", network: "arbitrum", below: d("1.4")))
    #expect(LoanGuardCommand.parse("защити займ") == .set(loan: nil, network: nil, below: nil))
    #expect(LoanGuardCommand.parse("stop guarding my loans") == .clear)
    #expect(LoanGuardCommand.parse("убери защиту займа") == .clear)
    // A level that isn't a health factor is left to the default; neighbours of the form aren't read as it.
    #expect(LoanGuardCommand.parse("guard my loan on kamino below 50") == .set(loan: "kamino", network: nil, below: nil))
    #expect(LoanGuardCommand.parse("repay all USDC on kamino") == nil)
    #expect(LoanGuardCommand.parse("borrow 2 USDC on kamino against 0.05 SPCX") == nil)
    #expect(LoanGuardCommand.parse("every hour: check: Aave health factor on Base; warn under 1.6") == nil)
    // Asking to stop, asking about it, or a trade that mentions liquidation never starts one.
    #expect(LoanGuardCommand.parse("перестань следить за займом") == .clear)
    #expect(LoanGuardCommand.parse("don't guard my loan anymore") == .clear)
    #expect(LoanGuardCommand.parse("how does the loan guard work?") == nil)
    #expect(LoanGuardCommand.parse("long ETH 5x, don't let it get liquidated") == nil)
    #expect(LoanGuardCommand.parse("when SOL is below 150: guard my loan") == nil)
    #expect(LoanGuardCommand.parse("следи за ценой SOL") == nil)
    #expect(LoanGuardCommand.parse("защити кошелёк") == nil)
    // «до» is the target they want, not the line.
    #expect(LoanGuardCommand.parse("следи за займом, гаси до 1.6, если ниже 1.3") == .set(loan: nil, network: nil, below: d("1.3")))
}

@Test func loanGuardKeepsAGapAndReArmsHalfway() {
    let d = { Decimal(string: $0)! }
    let g = LoanGuard(loan: "aave", network: nil, below: d("1.6"))
    #expect(g.target == d("1.8") && g.rearm == d("1.7") && g.network == "base")
    let usual = LoanGuard(loan: "kamino")
    #expect(usual.target == d("1.5") && usual.rearm == d("1.375"))
}

@Test func loanGuardRepaysBackToTheTarget() {
    let d = { Decimal(string: $0)! }
    // Aave: $100 of collateral at 0.8, $64 owed → health 1.25; back at 1.5 the debt is 100 × 0.8 / 1.5 = 53.33.
    let health = AaveHealth.of(collateral: 100, debt: 64, threshold: d("0.8"))
    let r = LoanGuard.repay(debt: 64, health: health, target: d("1.5"))
    #expect(r == d("10.67"))
    #expect(100 * d("0.8") / (64 - r) >= d("1.5"))
    #expect(LoanGuard.repay(debt: 10, health: 2, target: d("1.5")) == 0)
    #expect(LoanGuard.repay(debt: 10, health: d("0.2"), target: d("1.5")) <= 10)
    #expect(LoanGuard(loan: "aave").repayCommand(d("10.67")) == "repay 10.67 USDC on Aave on base")
    #expect(LoanGuard(loan: "kamino", network: "base").network == nil)
    #expect(LoanGuard(loan: "kamino").repayCommand(d("3.5")) == "repay 3.5 USDC on kamino")
    // The repay it prepares is the move the router already knows.
    #expect(WalletCommand.parse(LoanGuard(loan: "aave", network: "arbitrum").repayCommand(d("10.67"))) == .aaveRepay(d("10.67"), network: "arbitrum"))
    #expect(WalletCommand.parse(LoanGuard(loan: "kamino").repayCommand(d("3.5"))) == .kaminoRepay(d("3.5")))
}

/// The forms as the router writes them land on the guard, in runCanonical's order, and its neighbours don't.
@Test func loanGuardFormsReadAsTheGuard() {
    #expect(read("guard my loan on kamino") == .loanGuard(.set(loan: "kamino", network: nil, below: nil)))
    #expect(read("guard my loan on aave on polygon below 1.4") == .loanGuard(.set(loan: "aave", network: "polygon", below: Decimal(string: "1.4"))))
    #expect(read("guard my loan") == .loanGuard(.set(loan: nil, network: nil, below: nil)))
    #expect(read("stop guarding my loans") == .loanGuard(.clear))
    #expect(read("repay all USDC on kamino") == .wallet(.kaminoRepay(nil)))
    #expect(read("when ETH is below 2680: long ETH leverage 3x") != .loanGuard(.clear))
}

/// Aave's health as AaveAccount counts it (Execution can't be imported here).
private enum AaveHealth {
    static func of(collateral: Decimal, debt: Decimal, threshold: Decimal) -> Decimal { collateral * threshold / debt }
}

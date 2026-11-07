@testable import DeFiJupiterLend
@testable import IhatecryptoApp
import Foundation
import Testing

/// Jupiter Lend's positions as the portfolio counts them (Workspace.loadSolana, lent, lendUSD).
@Test func jupiterLendCountsEachCoinAtItsOwnDecimals() {
    let d = [JupiterLend.Position(vault: "jlUSDC", assets: 5_001_510, shares: 1, coin: "USDC", decimals: 6),
             JupiterLend.Position(vault: "jlWSOL", assets: 1_500_000_000, shares: 1, coin: "WSOL", decimals: 9)].map(LendDeposit.init)
    #expect(d.map(\.coin) == ["USDC", "SOL"])
    // Was 1505.00151: the SOL vault counted as USDC at 10^6.
    #expect(d.amount(of: "USDC") == Decimal(string: "5.00151")!)
    #expect(d.amount(of: "SOL") == Decimal(string: "1.5")!)
    #expect(d.usd { ["USDC": 1, "SOL": 100][$0] } == Decimal(string: "155.00151")!)
    // A coin without a price is nothing, as on its tile; never its raw units.
    #expect(d.usd { $0 == "USDC" ? 1 : nil } == Decimal(string: "5.00151")!)
}

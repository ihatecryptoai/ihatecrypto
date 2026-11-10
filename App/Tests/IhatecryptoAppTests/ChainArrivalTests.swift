@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import Testing

/// «withdraw 1 USDC from Jupiter then deposit 1 USDC into Jupiter» deposited 0.1 (2026-11-10): the deposit read the
/// balance before the withdrawal showed, and «up to a dollar short» let it spend the tenth that was there.
@Test func aStepAfterAWithdrawalWaitsForItAndNeverSpendsASmallPart() {
    // What comes out of a vault lands on Solana like a bridge's money: the step after it waits for the balance.
    #expect(Workspace.arrives(.lendWithdraw("USDC", 1)) == "solana")
    #expect(Workspace.arrives(.kamino(deposit: false, symbol: "USDC", amount: 1)) == "solana")
    #expect(Workspace.arrives(.lendWithdraw("SOL", 1)) == nil && Workspace.arrives(.lendDeposit("USDC", 1)) == nil)
    let d = { (s: String) in Decimal(string: s)! }
    // A fee's worth short spends what is there: cents on 100, under a dollar on 5 (Ethereum).
    #expect(Workspace.fitsArrival(asked: 100, there: d("99.93")) && Workspace.fitsArrival(asked: 5, there: d("4.1")))
    #expect(Workspace.fitsArrival(asked: 1000, there: 985))
    // Never a small part of the amount, and nothing when the money is all there.
    #expect(!Workspace.fitsArrival(asked: 1, there: d("0.107186")) && !Workspace.fitsArrival(asked: 2, there: d("1.1")))
    #expect(!Workspace.fitsArrival(asked: 100, there: 97) && !Workspace.fitsArrival(asked: 5, there: 5) && !Workspace.fitsArrival(asked: 5, there: 6))
}

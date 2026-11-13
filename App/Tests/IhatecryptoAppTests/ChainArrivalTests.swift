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


/// The wait for what a move brought to Solana: over with the step's amount there, and for a step that names none, or
/// asks for more than the move brought, once the finalized balance equals a confirmed one that stopped moving.
@Test func theWaitForADepositEndsWhenTheBalanceShowsIt() {
    let d = { (s: String) in Decimal(string: s)! }
    func shown(_ need: Decimal?, _ finalized: String, _ confirmed: String?, steady: Int, entry: String = "0.107") -> Bool {
        Workspace.depositShown(need: need, finalized: d(finalized), confirmed: confirmed.map(d), steady: steady, entry: d(entry))
    }
    // «withdraw 1 then deposit 1»: the amount is there.
    #expect(!shown(1, "0.107", "1.107", steady: 5) && shown(1, "1.107", "1.107", steady: 0) && shown(1, "1", nil, steady: 0))
    // «…then move all»: no amount, so it waits for the confirmed balance, seen coming and read the same three times running.
    #expect(!shown(nil, "0.107", "1.107", steady: 9) && !shown(nil, "1.107", "1.107", steady: 2) && shown(nil, "1.107", "1.107", steady: 3))
    // Nothing seen coming (it had landed already, or a node behind still answers the old balance): eight reads, not three.
    #expect(!shown(nil, "0.107", "0.107", steady: 3) && !shown(nil, "0.107", "0.107", steady: 7) && shown(nil, "0.107", "0.107", steady: 8))
    #expect(!shown(50, "6", "6", steady: 7, entry: "6") && shown(50, "6", "6", steady: 8, entry: "6"))
    // «withdraw 1 then buy for 50» with 6 coming: over when the 6 show, not after two minutes.
    #expect(!shown(50, "5", "6", steady: 9, entry: "5") && shown(50, "6", "6", steady: 3, entry: "5"))
    // Money an earlier step spent is confirmed before it is final: 20 final, 15 counted is not «caught up».
    #expect(!shown(nil, "20", "15", steady: 9, entry: "20") && shown(nil, "15", "15", steady: 8, entry: "20"))
    // No confirmed read: only the step's own amount ends it.
    #expect(!shown(50, "6", nil, steady: 9))
    // Which steps wait: one that spends Solana's USDC, and a spread of the free USDC wherever it is; never another coin's deposit.
    #expect(Workspace.spendsOnSolana(.lendDeposit("USDC", nil)) && Workspace.spendsOnSolana(.solanaTo("base", nil)))
    #expect(Workspace.spendsOnSolana(.allocate("USDC", maxShare: nil)) && !Workspace.spendsOnSolana(.allocate("USDC", maxShare: nil, from: "base")))
    #expect(!Workspace.spendsOnSolana(.lendDeposit("SOL", 2)) && !Workspace.spendsOnSolana(.kamino(deposit: true, symbol: "SOL", amount: 1)))
    #expect(!Workspace.spendsOnSolana(.lendWithdraw("USDC", 1)))
}

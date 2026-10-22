@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import SolanaChain
import Testing

/// USDC topped up on Solana from a browser wallet, into a wallet with almost no SOL: the same message brings
/// `TopUpOrder.solanaFeeTopUp` SOL along, so the first move there can pay its fees. Public test addresses only.
@Test func solanaUSDCTopUpBringsSOLForFees() throws {
    // «abandon … about» accounts, as WalletTests use them.
    let from = "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw", me = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm"
    let order = TopUpOrder(network: .solana, coin: .usdc, amount: 20, recipient: me)
    let message = try SolanaMessage.parse(message: TopUpPage.usdcWithFees(order, units: 20_000_000, from: from,
                                                                          blockhash: "11111111111111111111111111111111"))
    #expect(message.requiredSignatures == 1 && message.staticKeys.first == from)
    let programs = message.instructions.map { message.staticKeys[$0.programIndex] }
    #expect(programs == ["ComputeBudget111111111111111111111111111111", SPL.associatedTokenProgram, SPL.tokenProgram, "11111111111111111111111111111111"])
    // The USDC goes into this wallet's own token account, TransferChecked with 6 decimals.
    let usdc = message.instructions[2]
    #expect(usdc.data.first == 12 && usdc.data.last == 6)
    #expect(message.staticKeys[usdc.accounts[2]] == (try SPL.associatedAccount(owner: me, mint: order.network.usdc)))
    // The SOL: a system transfer of exactly the top-up, from the payer to this wallet.
    let sol = message.instructions[3]
    #expect(sol.accounts.map { message.staticKeys[$0] } == [from, me])
    #expect(sol.data == Data([2, 0, 0, 0]) + withUnsafeBytes(of: TopUpOrder.solanaFeeTopUp.littleEndian) { Data($0) })
}

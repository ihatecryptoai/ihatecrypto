@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import Testing

/// Research E-7, variant B: a move that brings money to this wallet's address on a network where someone else's EIP-7702
/// contract runs it says so on its card; Confirm stays. Moves elsewhere, or out of that network, say nothing.
@Test func foreignDelegationWarnsOnMovesIntoItsNetwork() {
    let delegates = ["base": "0x8a67b5020ee254ef48e3b6a04927f39baf7e408a"]
    let into = Workspace.delegationWarning(.bridge(from: "solana", to: "base", 20), delegates)
    #expect(into == "Your Base address runs someone else's contract (0x8a67…408a), set up outside ihatecrypto. Money arriving there can be taken by it. Confirm only if you set it up yourself, in MetaMask for example.")
    #expect(Workspace.delegationWarning(.solanaTo("base", 5), delegates) != nil)
    #expect(Workspace.delegationWarning(.buyOn("base", token: "AERO", usdc: 5), delegates) != nil)
    #expect(Workspace.delegationWarning(.bridge(from: "solana", to: "arbitrum", 20), delegates) == nil)
    #expect(Workspace.delegationWarning(.baseToSolana(5), delegates) == nil)
    #expect(Workspace.delegationWarning(.aave(deposit: true, amount: 5, network: "base"), delegates) != nil)
    #expect(Workspace.delegationWarning(.aaveBorrow(1, network: "base"), delegates) != nil)
    #expect(Workspace.delegationWarning(.bridge(from: "solana", to: "base", 20), [:]) == nil)
}

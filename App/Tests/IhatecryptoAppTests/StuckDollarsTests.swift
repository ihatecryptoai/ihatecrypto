@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import Testing
import TradeExecution

/// Where dollars landed without the coin fees are paid in, and what the chat offers there (Workspace.stuck).
private func stuck(solana: Decimal = 0, sol: Decimal = 0, hyperliquid: Decimal = 0, on held: [String: (usdc: Decimal, gas: Decimal)] = [:]) -> [StuckDollars] {
    Workspace.stuck(solanaUSDC: solana, solanaSOL: sol, hyperliquidFree: hyperliquid, say: { en, _ in en }) { held[$0.key] ?? (0, 0) }
}

@Test func solanaDollarsWithoutSOLGetJupitersSwap() {
    let ten = stuck(solana: 10)
    #expect(ten.map(\.key) == ["solana"])
    #expect(ten[0].ways.map(\.command) == ["buy SOL for 5 USDC"])
    // Under Jupiter's $5: only adding SOL works, and that's what the chat asks.
    #expect(stuck(solana: 3)[0].ways.isEmpty)
    // With SOL for fees nothing is stuck.
    #expect(stuck(solana: 10, sol: Decimal(string: "0.01")!).isEmpty)
}

@Test func evmDollarsWithoutGasGetTheirWayOut() {
    // Base pays by signature: to Solana, or its gas brought back over Solana.
    let base = stuck(on: ["base": (5, 0)])
    #expect(base.map(\.key) == ["base"])
    #expect(base[0].ways.map(\.command) == ["move 5 USDC from base to Solana", "add $1.00 of gas on base"])
    // BNB Chain's dollar can't pay by signature, but BNB Chain pays the gas for its transfer (MegaFuel): the same two ways.
    let bnb = stuck(on: ["bnb": (5, 0)])
    #expect(bnb.map(\.key) == ["bnb"] && bnb[0].coin == "BNB")
    #expect(bnb[0].ways.map(\.command) == ["move 5 USDC from bnb to Solana", "add $1.00 of gas on bnb"])
    // HyperEVM has neither: the chat asks for HYPE.
    let hyperevm = stuck(on: ["hyperevm": (5, 0)])
    #expect(hyperevm.map(\.key) == ["hyperevm"] && hyperevm[0].ways.isEmpty)
    // Hyperliquid's balance or Solana's USDC and SOL bring the gas by themselves: nothing to offer.
    #expect(stuck(hyperliquid: 10, on: ["bnb": (5, 0)]).isEmpty)
    #expect(stuck(solana: 5, sol: Decimal(string: "0.01")!, on: ["bnb": (5, 0), "base": (5, 0)]).isEmpty)
    // Gas there already, or under a dollar there: nothing either.
    #expect(stuck(on: ["bnb": (5, Decimal(string: "0.001")!), "base": (Decimal(string: "0.5")!, 0)]).isEmpty)
}

import Foundation
@testable import IhatecryptoApp
import IhatecryptoCore
import Testing

/// Our 0.05% rides on Hyperliquid orders only: a withdrawal or an HLP move there is «none to us».
@Test func theReceiptSaysOurFeeOnlyOnHyperliquidOrders() {
    #expect(Receipt.ours(.hlSpotBuy(symbol: "HYPE", usdc: 5)))
    #expect(Receipt.ours(.hlSpotSell(symbol: "HYPE", amount: 1)))
    #expect(Receipt.ours(.swapOn("hyperliquid", from: "USDC", to: "HYPE", amount: 5)))
    #expect(!Receipt.ours(.swapOn("base", from: "USDC", to: "ETH", amount: 5)))
    #expect(!Receipt.ours(.withdrawToSolana(5)))
    #expect(!Receipt.ours(nil))
}

/// «FROM · BASE» is you paying; a protocol or a vault paying out is money coming back.
@Test func onlyANetworkIsYouPaying() {
    #expect(Receipt.networks.contains("BASE") && Receipt.networks.contains("SOLANA"))
    #expect(!Receipt.networks.contains("KAMINO LEND") && !Receipt.networks.contains("HLP VAULT") && !Receipt.networks.contains("AGENT WALLET"))
}

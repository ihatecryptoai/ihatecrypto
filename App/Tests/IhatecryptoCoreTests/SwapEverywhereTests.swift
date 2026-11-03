import Foundation
@testable import IhatecryptoCore
import Testing

/// Any token for any other on every network ihatecrypto has (2026-10-08): Solana through Jupiter, Hyperliquid's spot book,
/// Robinhood Chain, the EVM networks, and across networks through Relay — each read back from its own summary.
@Test func anyTokenForAnyOtherOnEveryNetwork() {
    let bonk = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
    // Solana: two tokens straight; USDC on a side stays a buy or a sale.
    #expect(WalletCommand.parse("swap 0.1 SOL for BONK on solana") == .swapOn("solana", from: "SOL", to: "BONK", amount: Decimal(string: "0.1")!))
    #expect(WalletCommand.parse("поменяй 0.1 SOL на BONK на солане") == .swapOn("solana", from: "SOL", to: "BONK", amount: Decimal(string: "0.1")!))
    #expect(WalletCommand.parse("swap 1 JUP for \(bonk) on solana") == .swapOn("solana", from: "JUP", to: bonk, amount: 1))
    #expect(WalletCommand.parse("swap 5 USDC for BONK on solana") == .buy(.bySymbol("BONK"), usdc: 5))
    #expect(WalletCommand.parse("swap all JUP for USDC on solana") == .sell(.bySymbol("JUP"), amount: nil))
    // Hyperliquid's spot book: a sale and a buy; USDC on a side is one order.
    #expect(WalletCommand.parse("swap 1 HYPE for PURR on hyperliquid spot") == .swapOn("hyperliquid", from: "HYPE", to: "PURR", amount: 1))
    #expect(WalletCommand.parse("swap 20 USDC for HYPE on hyperliquid spot") == .hlSpotBuy(symbol: "HYPE", usdc: 20))
    // What the book can't swap still parses as a swap there; hlSpotSwap turns it down before the sale.
    #expect(WalletCommand.parse("swap 1 HYPE for HYPE on hyperliquid spot") == .swapOn("hyperliquid", from: "HYPE", to: "HYPE", amount: 1))
    #expect(WalletCommand.parse("поменяй 1 HYPE на HYPE на хайперликвиде") == .swapOn("hyperliquid", from: "HYPE", to: "HYPE", amount: 1))
    #expect(WalletCommand.parse("swap 12 USDT for USDT0 on hyperliquid") == .swapOn("hyperliquid", from: "USDT", to: "USDT0", amount: 12))
    #expect(WalletCommand.parse("swap 1 HYPE for USDC.e on hyperliquid spot") == .swapOn("hyperliquid", from: "HYPE", to: "USDC.E", amount: 1))
    #expect(WalletCommand.parse("обменяй 1 HYPE на USDC.e на хайперликвиде") == .swapOn("hyperliquid", from: "HYPE", to: "USDC.E", amount: 1))
    #expect(WalletCommand.parse("поменяй 1 HYPE на USDC на хайперликвиде") == .hlSpotSell(symbol: "HYPE", amount: 1))
    // Robinhood Chain: Stock Tokens for one another.
    #expect(WalletCommand.parse("swap 1 TSLA for NVDA on robinhood") == .swapOn("robinhood", from: "TSLA", to: "NVDA", amount: 1))
    #expect(WalletCommand.parse("поменяй 1 TSLA на NVDA на робингуде") == .swapOn("robinhood", from: "TSLA", to: "NVDA", amount: 1))
    // NEAR through NEAR Intents: any of its tokens for another there, and to or from any network; NEAR for USDC and
    // back stays 1Click's sale and buy.
    #expect(WalletCommand.parse("swap 2 NEAR for USDC on near") == .sellNear(amount: 2))
    #expect(WalletCommand.parse("swap 5 USDC for NEAR on near") == .buyNear(usdc: 5))
    #expect(WalletCommand.parse("swap 1 NEAR for AURORA on near") == .swapOn("near", from: "NEAR", to: "AURORA", amount: 1))
    #expect(WalletCommand.parse("поменяй 1 NEAR на AURORA на неар") == .swapOn("near", from: "NEAR", to: "AURORA", amount: 1))
    #expect(WalletCommand.parse("swap 1 NEAR for SOL on solana") == .crossSwap(from: "NEAR", on: "near", to: "SOL", at: "solana", amount: 1))
    #expect(WalletCommand.parse("swap 0.1 SOL on solana for NEAR on near") == .crossSwap(from: "SOL", on: "solana", to: "NEAR", at: "near", amount: Decimal(string: "0.1")!))
    // The EVM networks as before, and across networks to and from the new ones.
    #expect(WalletCommand.parse("swap 0.01 ETH for AERO on base") == .swapOn("base", from: "ETH", to: "AERO", amount: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("swap 0.1 SOL for CAKE on bnb") == .crossSwap(from: "SOL", on: "solana", to: "CAKE", at: "bnb", amount: Decimal(string: "0.1")!))
    #expect(WalletCommand.parse("swap 1 OP on optimism for ETH on base") == .crossSwap(from: "OP", on: "optimism", to: "ETH", at: "base", amount: 1))
    #expect(WalletCommand.parse("swap all UBTC on hyperevm for SOL on solana") == .crossSwap(from: "UBTC", on: "hyperevm", to: "SOL", at: "solana", amount: nil))
    for c in [WalletCommand.swapOn("solana", from: "SOL", to: "BONK", amount: 1), .swapOn("solana", from: "JUP", to: bonk, amount: 2),
              .swapOn("hyperliquid", from: "HYPE", to: "PURR", amount: 1), .swapOn("robinhood", from: "TSLA", to: "NVDA", amount: nil),
              .crossSwap(from: "SOL", on: "solana", to: "CAKE", at: "bnb", amount: 1), .crossSwap(from: "OP", on: "optimism", to: "UBTC", at: "hyperevm", amount: 1),
              .swapOn("near", from: "NEAR", to: "AURORA", amount: 1), .crossSwap(from: "NEAR", on: "near", to: "SOL", at: "solana", amount: 1),
              .crossSwap(from: "SOL", on: "solana", to: "NEAR", at: "near", amount: 1)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

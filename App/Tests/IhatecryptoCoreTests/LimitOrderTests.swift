import Foundation
@testable import IhatecryptoCore
import Testing

private func d(_ s: String) -> Decimal { Decimal(string: s)! }

@Test func limitOrdersReadInEnglish() {
    #expect(WalletCommand.parse("limit sell 0.1 SOL at 200") == .limitOrder(sell: true, coin: "SOL", on: "solana", price: 200, amount: d("0.1"), usdcOn: "solana", days: nil))
    #expect(WalletCommand.parse("limit buy SOL at 120 for 20 USDC") == .limitOrder(sell: false, coin: "SOL", on: "solana", price: 120, amount: 20, usdcOn: "solana", days: nil))
    #expect(WalletCommand.parse("limit buy SOL at $120.5 for $20 for 3 days") == .limitOrder(sell: false, coin: "SOL", on: "solana", price: d("120.5"), amount: 20, usdcOn: "solana", days: 3))
    #expect(WalletCommand.parse("limit sell 1 NEAR at 3.5 for USDC on base") == .limitOrder(sell: true, coin: "NEAR", on: "near", price: d("3.5"), amount: 1, usdcOn: "base", days: nil))
    #expect(WalletCommand.parse("limit buy NEAR at 2 for 5 USDC") == .limitOrder(sell: false, coin: "NEAR", on: "near", price: 2, amount: 5, usdcOn: "solana", days: nil))
    #expect(WalletCommand.parse("limit buy ETH on arbitrum at 3000 for 10 USDC on arbitrum for a week") == .limitOrder(sell: false, coin: "ETH", on: "arbitrum", price: 3000, amount: 10, usdcOn: "arbitrum", days: 7))
    #expect(WalletCommand.parse("limit sell 0.1 SOL privately at 200") == .limitOrder(sell: true, coin: "SOL", on: "solana", price: 200, amount: d("0.1"), usdcOn: "solana", days: nil))
    #expect(WalletCommand.parse("show limit orders") == .limitOrders)
    #expect(WalletCommand.parse("my limit orders") == .limitOrders)
    #expect(WalletCommand.parse("cancel limit orders") == .limitCancel(nil))
    #expect(WalletCommand.parse("cancel limit orders SOL") == .limitCancel("SOL"))
}

@Test func limitOrdersReadInRussian() {
    #expect(WalletCommand.parse("лимитка: продай 0,1 SOL по 200") == .limitOrder(sell: true, coin: "SOL", on: "solana", price: 200, amount: d("0.1"), usdcOn: "solana", days: nil))
    #expect(WalletCommand.parse("лимитный ордер: купи SOL по 120 на 20 USDC на 3 дня") == .limitOrder(sell: false, coin: "SOL", on: "solana", price: 120, amount: 20, usdcOn: "solana", days: 3))
    #expect(WalletCommand.parse("лимиткой купи сол по цене 120 на 20 долларов") == .limitOrder(sell: false, coin: "SOL", on: "solana", price: 120, amount: 20, usdcOn: "solana", days: nil))
    #expect(WalletCommand.parse("лимитка: продай 1 NEAR по 3,5 за USDC в бейсе") == .limitOrder(sell: true, coin: "NEAR", on: "near", price: d("3.5"), amount: 1, usdcOn: "base", days: nil))
    #expect(WalletCommand.parse("мои лимитки") == .limitOrders)
    #expect(WalletCommand.parse("покажи лимитные ордера") == .limitOrders)
    #expect(WalletCommand.parse("отмени лимитки") == .limitCancel(nil))
    #expect(WalletCommand.parse("отмени лимитные ордера по SOL") == .limitCancel("SOL"))
}

@Test func aLimitOrderWithoutItsPriceOrAmountIsNothing() {
    // The chat asks rather than resting an order at a guessed price or size.
    #expect(WalletCommand.parse("limit sell SOL at 200") == nil)
    #expect(WalletCommand.parse("limit buy SOL for 20 USDC") == nil)
    #expect(WalletCommand.parse("limit buy SOL at 120") == nil)
    #expect(WalletCommand.parse("limit buy USDC at 1 for 5 USDC") == nil)
    #expect(WalletCommand.parse("limit sell 0.1 SOL at 200 for 90 days") == nil)
    // One «limit order» is Hyperliquid's entry waiting to fill: a position command, never 1Click's orders.
    #expect(WalletCommand.parse("cancel the limit order") == nil)
    #expect(WalletCommand.parse("отмени лимитку") == nil)
    #expect(WalletCommand.parse("cancel limit orders id order_EfzzwVuXY") == .limitCancel("id:order_EfzzwVuXY"))
    // A plan's entry price and an agent wallet's daily limit are not limit orders.
    #expect(WalletCommand.parse("long ETH stop 2% limit 2650") == nil)
    #expect(WalletCommand.parse("long SOL 5 USDC stop 1% limit order at 120") == nil)
    #expect(WalletCommand.parse("создай кошелёк агента с лимитом 20$ в день") == .agentCreate(dailyUSD: 20))
    // Without «limit» a buy at a price stays the buy it always was.
    #expect(WalletCommand.parse("buy SOL for 20 USDC") == .buy(.bySymbol("SOL"), usdc: 20))
}

@Test func limitOrdersWriteBackToThemselves() {
    for c: WalletCommand in [.limitOrder(sell: true, coin: "SOL", on: "solana", price: 200, amount: d("0.1"), usdcOn: "base", days: 3),
                             .limitOrder(sell: false, coin: "NEAR", on: "near", price: d("2.25"), amount: 5, usdcOn: "solana", days: nil),
                             .limitOrder(sell: false, coin: "ETH", on: "arbitrum", price: 3000, amount: 10, usdcOn: "arbitrum", days: 7),
                             .limitOrders, .limitCancel(nil), .limitCancel("SOL"), .limitCancel("id:order_Ab1-x")] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

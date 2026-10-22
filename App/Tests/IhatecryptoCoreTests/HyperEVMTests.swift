import Foundation
@testable import IhatecryptoCore
import Testing

/// HyperEVM (Hyperliquid's EVM, chain 999) as a network: its money in and out, its gas, sends there — never mistaken for
/// the Hyperliquid trading account, in English and in Russian.
@Test func hyperEVMIsAPlaceOfItsOwn() {
    #expect(WalletCommand.parse("move 2 USDC from Solana to HyperEVM") == .solanaTo("hyperevm", 2))
    #expect(WalletCommand.parse("переведи 2 USDC из соланы на хайперевм") == .solanaTo("hyperevm", 2))
    #expect(WalletCommand.parse("move all USDC from HyperEVM to Solana") == .evmToSolana("hyperevm", nil))
    #expect(WalletCommand.parse("move 3 USDC from base to hyperevm") == .bridge(from: "base", to: "hyperevm", 3))
    #expect(WalletCommand.parse("move 3 USDC from hyperevm to hyperliquid") == .bridge(from: "hyperevm", to: "hyperliquid", 3))
    #expect(WalletCommand.parse("add gas on hyperevm") == .gas("hyperevm", usd: nil))
    #expect(WalletCommand.parse("send 5 USDC to Mom on hyperevm") == .sendTo(name: "Mom", amount: 5, network: "hyperevm"))
    #expect(WalletCommand.parse("move 2 USDC from Solana to Hyperliquid") != .solanaTo("hyperevm", 2))
    #expect(ModuleNetwork(place: "hyperevm") == .hyperevm && ModuleNetwork(place: "hyperliquid") == .hyperliquid)
    #expect(AddressBookCommand.parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as Shop on HyperEVM")
            == .save(address: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", name: "Shop", network: "hyperevm"))
    for c in [WalletCommand.solanaTo("hyperevm", 2), .evmToSolana("hyperevm", 1), .bridge(from: "arbitrum", to: "hyperevm", 4), .gas("hyperevm", usd: 1)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

/// Swaps on HyperEVM and Optimism (2026-10-07): a token there for USDC, for another, HYPE as HyperEVM's own coin.
@Test func swapsOnOptimismAndHyperEVM() {
    #expect(WalletCommand.parse("buy UBTC on hyperevm for 5 USDC") == .buyOn("hyperevm", token: "UBTC", usdc: 5))
    #expect(WalletCommand.parse("купи OP на оптимизме на 5 USDC") == .buyOn("optimism", token: "OP", usdc: 5))
    #expect(WalletCommand.parse("sell all OP on optimism") == .sellOn("optimism", token: "OP", amount: nil))
    #expect(WalletCommand.parse("swap 5 USDC for OP on optimism") == .buyOn("optimism", token: "OP", usdc: 5, from: "optimism"))
    #expect(WalletCommand.parse("swap 0.001 ETH for OP on optimism") == .swapOn("optimism", from: "ETH", to: "OP", amount: Decimal(string: "0.001")!))
    // HYPE is HyperEVM's coin: for USDC or back it stays a swap, not a buy that looks HYPE up as a token.
    #expect(WalletCommand.parse("swap 5 USDC for HYPE on hyperevm") == .swapOn("hyperevm", from: "USDC", to: "HYPE", amount: 5))
    #expect(WalletCommand.parse("swap 5 USDC for UBTC on hyperevm") == .swapOn("hyperevm", from: "USDC", to: "UBTC", amount: 5))
    #expect(WalletCommand.parse("поменяй 0.1 HYPE на UBTC на хайперевм") == .swapOn("hyperevm", from: "HYPE", to: "UBTC", amount: Decimal(string: "0.1")!))
    for c in [WalletCommand.buyOn("hyperevm", token: "UBTC", usdc: 5), .swapOn("optimism", from: "ETH", to: "OP", amount: 1),
              .swapOn("hyperevm", from: "USDC", to: "HYPE", amount: 5)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

/// Optimism, the first network added on demand (2026-10-06): moves to and from it, gas, sends, CCTP by name.
@Test func optimismIsAPlaceOfItsOwn() {
    #expect(WalletCommand.parse("move 2 USDC from Solana to Optimism") == .solanaTo("optimism", 2))
    #expect(WalletCommand.parse("переведи 2 USDC из соланы на оптимизм") == .solanaTo("optimism", 2))
    #expect(WalletCommand.parse("move all USDC from Optimism to Solana") == .evmToSolana("optimism", nil))
    #expect(WalletCommand.parse("move 100 USDC from Optimism to Base via CCTP") == .cctp(from: "optimism", to: "base", 100))
    #expect(WalletCommand.parse("add gas on optimism") == .gas("optimism", usd: nil))
    #expect(WalletCommand.parse("send 5 USDC to Mom on optimism") == .sendTo(name: "Mom", amount: 5, network: "optimism"))
    #expect(ModuleNetwork(place: "optimism") == .optimism)
    #expect(AddressBookCommand.parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as Shop on Optimism")
            == .save(address: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", name: "Shop", network: "optimism"))
    for c in [WalletCommand.solanaTo("optimism", 2), .evmToSolana("optimism", 1), .bridge(from: "optimism", to: "arbitrum", 4), .gas("optimism", usd: 1)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

/// A money move the wallet parser can't take — a network ihatecrypto doesn't have, a swap it can't place — is never read
/// as a trade plan of the ticker it names (route-check, 2026-10-07: «move 5 USDC from Solana to BNB» was «long BNB»).
@Test func anUnknownMoneyMoveIsNotATradePlan() {
    let markets: Set<String> = ["ETH", "BNB", "CAKE", "OP", "HYPE", "SOL"]
    for line in ["move 5 USDC from Solana to BNB", "move 5 USDC from Solana to BSC", "buy CAKE on bnb for 5 USDC",
                 "swap 0.01 BNB for USDT on bsc", "переведи 10 USDC на бнб", "купи CAKE на бнб чейне на 5$", "send 5 USDT to Binance on bsc"] {
        #expect(IntentParser.apply(line, to: TradeIntent(), markets: markets) == nil, "\(line)")
    }
    // Plans still read: a long, a stop moved.
    #expect(IntentParser.apply("long BNB 5 USDC", to: TradeIntent(), markets: markets)?.asset == "BNB")
    #expect(IntentParser.apply("move stop to 3%", to: TradeIntent(), markets: markets) != nil)
}

/// A tiny coin amount keeps four significant digits (2026-10-08: 1 USDC of UBTC showed «0.000011», and the card's rate
/// worked out from it read $90,909 instead of ≈ $83,600); dollars and ordinary amounts look as before.
@Test func aTinyCoinAmountKeepsItsDigits() {
    #expect(Fmt.num(Decimal(string: "0.00001196")!, maxFraction: 6) == "0.00001196")
    #expect(Fmt.num(Decimal(string: "0.0000119612")!, maxFraction: 6) == "0.00001196")
    #expect(Fmt.num(Decimal(string: "8.1535351")!, maxFraction: 6) == "8.153535")
    #expect(Fmt.num(Decimal(string: "0.054737")!, maxFraction: 6) == "0.054737")
    #expect(Fmt.num(Decimal(string: "0.00012345678")!, maxFraction: 8) == "0.00012346")
    #expect(Fmt.num(Decimal(string: "0.004")!) == "0")
    #expect(Fmt.num(0, maxFraction: 6) == "0")
}

/// The chat's «Move 11 USDC to Hyperliquid» card (enableAgent on an account Hyperliquid hasn't opened yet): a move it can run.
@Test func openingHyperliquidCardIsAMove() {
    #expect(WalletCommand.parse("move 11 USDC from Solana to Hyperliquid") == .solanaTo("hyperliquid", 11))
    #expect(WalletCommand.parse("переведи 11 USDC с соланы на хайперликвид") == .solanaTo("hyperliquid", 11))
    #expect(WalletCommand.parse(WalletCommand.solanaTo("hyperliquid", 11).summary) == .solanaTo("hyperliquid", 11))
    // HyperEVM is still its own network, never the trading account.
    #expect(WalletCommand.parse("move 2 USDC from Solana to HyperEVM") == .solanaTo("hyperevm", 2))
}

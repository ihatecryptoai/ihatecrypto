import Foundation
import Testing
@testable import IhatecryptoCore

private func d(_ s: String) -> Decimal { Decimal(string: s)! }
private let base = FundingPlan.Place(key: "base", name: "Base", usdc: 100)

@Test func fundingTakesSolanaThenEVMThenHyperliquidThroughSolana() throws {
    let legs = try #require(FundingPlan.legs(short: 300, target: "robinhood", targetName: "Robinhood", solana: 9, evm: [base], hyperliquid: 250, near: 0, nearPrice: nil))
    #expect(legs == ["move 100 USDC from Base to Robinhood", "withdraw 194 USDC to Solana", "move 201 USDC from Solana to Robinhood"])
    // Each leg is a command the chat reads back as the right move.
    #expect(WalletCommand.parse(legs[0]) == .bridge(from: "base", to: "robinhood", 100))
    #expect(WalletCommand.parse(legs[1]) == .withdrawToSolana(194))
    #expect(WalletCommand.parse(legs[2]) == .solanaTo("robinhood", 201))
}

@Test func fundingForSolanaLandsEachSourceThereAndSellsNEARLast() throws {
    let legs = try #require(FundingPlan.legs(short: 50, target: "solana", targetName: "Solana", solana: 0,
                                             evm: [FundingPlan.Place(key: "base", name: "Base", usdc: 20)], hyperliquid: 40, near: 0, nearPrice: nil))
    #expect(legs == ["move 20 USDC from Base to Solana", "withdraw 32 USDC to Solana"])
    let near = try #require(FundingPlan.legs(short: 10, target: "solana", targetName: "Solana", solana: 0, evm: [], hyperliquid: 0, near: 5, nearPrice: 3))
    #expect(near == ["sell 3.74 NEAR"] && WalletCommand.parse(near[0]) == .sellNear(amount: d("3.74")))
    // Not enough anywhere: no plan, the move's own check says so; under $2 a place isn't worth a leg.
    #expect(FundingPlan.legs(short: 500, target: "solana", targetName: "Solana", solana: 0, evm: [base], hyperliquid: 40, near: 0, nearPrice: nil) == nil)
    #expect(FundingPlan.legs(short: 1, target: "solana", targetName: "Solana", solana: 0, evm: [FundingPlan.Place(key: "base", name: "Base", usdc: d("1.5"))],
                             hyperliquid: 0, near: 0, nearPrice: nil) == nil)
}

/// A buy run again as a chain's last step reads back from its own words, thousands and a contract included.
@Test func aBuyByContractReadsBackFromItsSummary() {
    let c = WalletCommand.buyStock(symbol: "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", usdc: 1722)
    #expect(WalletCommand.parse(c.summary) == c, "\(c.summary) → \(String(describing: WalletCommand.parse(c.summary)))")
}

/// Cents count: a leg rounded down left this plan a cent short (1721.109778 USDC more on Robinhood).
@Test func fundingCoversTheLastCent() throws {
    let legs = try #require(FundingPlan.legs(short: Decimal(string: "1721.109778")!, target: "robinhood", targetName: "Robinhood",
                                             solana: Decimal(string: "9.008635")!, evm: [FundingPlan.Place(key: "base", name: "Base", usdc: Decimal(string: "1712.67116")!)],
                                             hyperliquid: Decimal(string: "4.007817")!, near: 0, nearPrice: nil))
    #expect(legs == ["move 1712.67 USDC from Base to Robinhood", "withdraw 2.44 USDC to Solana", "move 9.44 USDC from Solana to Robinhood"])
}

/// Solana's leg names what arrives (fee on top): its whole balance asked to arrive could never be paid.
@Test func solanaLegLeavesRoomForTheFee() throws {
    let legs = try #require(FundingPlan.legs(short: 50, target: "robinhood", targetName: "Robinhood", solana: Decimal(string: "34.823554")!,
                                             evm: [FundingPlan.Place(key: "base", name: "Base", usdc: Decimal(string: "21.5")!)], hyperliquid: 0, near: 0, nearPrice: nil))
    #expect(legs == ["move 17.18 USDC from Base to Robinhood", "move 33.82 USDC from Solana to Robinhood"])
    // A cent on Solana isn't a leg.
    #expect(FundingPlan.legs(short: 5, target: "robinhood", targetName: "Robinhood", solana: Decimal(string: "1.2")!, evm: [base], hyperliquid: 0, near: 0, nearPrice: nil)
        == ["move 6 USDC from Base to Robinhood"])
}

/// A loan guard's top-up goes through NEAR Intents only: no Hyperliquid leg, no network NEAR Intents doesn't pay on, and
/// every leg reads back as a NEAR Intents move.
@Test func aGuardsTopUpGoesThroughNEARIntentsOnly() throws {
    let intents: Set<String> = ["base", "arbitrum", "ethereum"]
    let polygon = FundingPlan.Place(key: "polygon", name: "Polygon", usdc: 100)
    let toSolana = try #require(FundingPlan.legs(short: d("0.3"), target: "solana", targetName: "Solana", solana: 0, evm: [polygon, base],
                                                 hyperliquid: 250, near: 0, nearPrice: nil, intents: intents))
    #expect(toSolana == ["move 1.3 USDC from Base to Solana via NEAR Intents"])
    #expect(WalletCommand.parse(toSolana[0]) == .intents(from: "base", to: "solana", d("1.3")))
    // Only Hyperliquid has it: not through NEAR Intents, so no plan.
    #expect(FundingPlan.legs(short: 5, target: "solana", targetName: "Solana", solana: 0, evm: [], hyperliquid: 250, near: 0, nearPrice: nil, intents: intents) == nil)
    // An Aave loan on Base: Solana's USDC crosses through NEAR Intents too.
    let toBase = try #require(FundingPlan.legs(short: 5, target: "base", targetName: "Base", solana: 20, evm: [], hyperliquid: 0, near: 0, nearPrice: nil, intents: intents))
    #expect(toBase == ["move 6 USDC from Solana to Base via NEAR Intents"])
    #expect(WalletCommand.parse(toBase[0]) == .intents(from: "solana", to: "base", 6))
}

@Test func aMoveViaNEARIntentsIsReadInEnglishAndRussian() {
    #expect(WalletCommand.parse("move 2 USDC from Base to Solana via NEAR Intents") == .intents(from: "base", to: "solana", 2))
    #expect(WalletCommand.parse("переведи 2 USDC с бейза на солану через NEAR Intents") == .intents(from: "base", to: "solana", 2))
    #expect(WalletCommand.parse("перекинь 3 USDC из соланы в арбитрум через неар интентс") == .intents(from: "solana", to: "arbitrum", 3))
    #expect(WalletCommand.parse("move 2 USDC from Base to Solana via NEAR Intents")?.summary == "move 2 USDC from Base to Solana via NEAR Intents")
    // Without the name it's the usual move, whichever bridge lands more.
    #expect(WalletCommand.parse("move 2 USDC from Base to Solana") != .intents(from: "base", to: "solana", 2))
}

@Test func viaNEARIntentsOnlyWhere1ClickPaysUSDC() {
    // Polygon isn't one of them: asked about, never quietly sent over Relay.
    #expect(WalletCommand.parse("move 2 USDC from Polygon to Solana via NEAR Intents") == nil)
    // A deposit into Aave there isn't a bare move.
    #expect(WalletCommand.parse("переведи 5 USDC с бейза в aave на арбитрум через NEAR Intents") != .intents(from: "base", to: "arbitrum", 5))
    // The last leg into an EVM target sends the whole pool, so what arrives still covers the repay after 1Click's cents.
    let legs = FundingPlan.legs(short: 5, target: "base", targetName: "Base", solana: 20, evm: [], hyperliquid: 0, near: 0, nearPrice: nil,
                                intents: ["base", "arbitrum", "ethereum"])
    #expect(legs == ["move 6 USDC from Solana to Base via NEAR Intents"])
}

/// A person between the two networks makes it a send to them, which a move «via NEAR Intents» never is (it pays our
/// own address): nothing is read, and the chat asks.
@Test func aPersonBetweenTheNetworksIsNotAMoveToOurselves() {
    #expect(WalletCommand.parse("отправь 5 USDC с бейза маме на солане через NEAR Intents") == nil)
    #expect(WalletCommand.parse("send 5 USDC from Base to Mom on Solana via NEAR Intents") != .intents(from: "base", to: "solana", 5))
    #expect(WalletCommand.parse("move 5 USDC from my Base wallet to Solana via NEAR Intents") == .intents(from: "base", to: "solana", 5))
    #expect(WalletCommand.parse("переведи 5 USDC с бейза на солану через NEAR Intents") == .intents(from: "base", to: "solana", 5))
}

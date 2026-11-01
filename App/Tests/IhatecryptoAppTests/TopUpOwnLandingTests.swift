@testable import IhatecryptoApp
import ExecutionCore
import Foundation
import Testing

// Money ihatecrypto's own bridge brings is neither «💸 Received» in the chat, nor money in for the P/L (flows.json), nor
// a top-up landing on the Top up board (Workspace.ownMoney, Receive.swift).

private let now = Date(timeIntervalSince1970: 1_800_000_000)
private func sent(_ chain: String, _ id: String, _ note: String, minutesAgo: Double = 1) -> Journal.Entry {
    Journal.Entry(at: now.addingTimeInterval(-minutesAgo * 60), chain: chain, id: id, kind: .signed, note: note)
}
private func ended(_ chain: String, _ id: String, _ kind: Journal.Entry.Kind, minutesAgo: Double) -> Journal.Entry {
    Journal.Entry(at: now.addingTimeInterval(-minutesAgo * 60), chain: chain, id: id, kind: kind, note: "success")
}
private func own(_ network: String, _ journal: [Journal.Entry]) -> Bool { Workspace.ownMoney(on: network, journal: journal, now: now) != nil }

@Test func noBridgesMeansMoneyFromOutside() {
    #expect(!own("solana", []))
    #expect(!own("base", [Journal.Entry(at: now, chain: "solana", id: "sig", kind: .signed, note: "Jupiter USDC→SOL")]))
}

@Test func aTransactionOfOurOwnThereCountsForTwoMinutes() {
    let swap = { (minutes: Double) in [Journal.Entry(at: now.addingTimeInterval(-minutes * 60), chain: "solana", id: "sig", kind: .confirmed, note: "Jupiter USDC→SOL")] }
    #expect(own("solana", swap(1)) && !own("solana", swap(3)))
}

@Test func anOpenBridgeCountsWhereItLandsOnly() {
    let relay = [sent("relay", "r1", "Solana USDC→chain 8453, tx 5abc")]
    #expect(own("base", relay))
    #expect(!own("arbitrum", relay) && !own("solana", relay))
    let cctp = [sent("cctp", "6:0x1", "Base USDC→Arbitrum 5 (CCTP), tx 0x1", minutesAgo: 20)]
    #expect(own("arbitrum", cctp) && !own("base", cctp))
    #expect(own("base", [sent("1click", "dep", "USDC solana→base 5 USDC", minutesAgo: 12)]))
    #expect(own("solana", [sent("layerzero", "0x2", "Arbitrum USDT→Solana 5 (USDT0), tx 0x2", minutesAgo: 90)]))
    #expect(own("hyperliquid", [sent("relay", "r2", "Solana USDC→Hyperliquid, tx 5def")]))
}

@Test func aReceivingAddressIsMoneyFromOutside() {
    #expect(!own("solana", [sent("1click", "dep", "Receive: Sui USDC→Solana USDC 50")]))
}

@Test func aBridgeThatEndedCountsForTenMinutes() {
    let open = sent("relay", "r1", "Base USDC→Solana 5, tx 0xab", minutesAgo: 30)
    #expect(own("solana", [open, ended("relay", "r1", .settled, minutesAgo: 5)]))
    #expect(!own("solana", [open, ended("relay", "r1", .settled, minutesAgo: 20)]))
    // Refunded: the money comes back where it left.
    #expect(own("base", [open, ended("relay", "r1", .failed, minutesAgo: 2)]))
    #expect(!own("solana", [open, ended("relay", "r1", .failed, minutesAgo: 2)]))
    // Never sent (expired): nothing lands anywhere.
    #expect(!own("base", [open, ended("relay", "r1", .expired, minutesAgo: 2)]))
}

@Test func aBridgeOpenForADayIsOld() {
    #expect(!own("base", [sent("relay", "r1", "Solana USDC→chain 8453, tx 5abc", minutesAgo: 25 * 60)]))
}

@Test func aSwapOnOneNetworkLandsThere() {
    let swap = [sent("relay", "r1", "Base ETH→USDC, tx 0xcd")]
    #expect(own("base", swap) && !own("solana", swap))
}

@Test func aNoteWithoutADirectionCountsEverywhere() {
    #expect(own("solana", [sent("relay", "r1", "bridge, tx 5abc")]) && own("base", [sent("relay", "r1", "bridge, tx 5abc")]))
}

@Test func relayChainIdsAreWholeNumbers() {
    let optimism = [sent("relay", "r1", "Solana USDC→chain 10, tx 5abc")]
    #expect(own("optimism", optimism) && !own("ethereum", optimism))
    // An amount after a network's name isn't a chain id.
    #expect(!own("optimism", [sent("relay", "r2", "Solana USDC→Robinhood Chain 10, tx 5abc")]))
}

@Test func notesAsTheJournalWritesThem() {
    // What follows «, » is about the move, not where it lands.
    let bnb = [sent("relay", "r1", "BNB Chain USDC→Solana 5, BNB Chain paying the gas, tx 0xab")]
    #expect(own("solana", bnb) && !own("bnb", bnb))
    // 1Click's own words after the amount: the last network named is where it lands.
    let usdt0 = [sent("1click", "dep", "USDT0→USDT0 5 arbitrum → optimism")]
    #expect(own("optimism", usdt0) && !own("arbitrum", usdt0))
    #expect(own("solana", [sent("1click", "dep", "NEAR→SOL 1 near → solana")]))
    // Bought for a NEAR account, paid from Solana: nothing lands on Solana.
    #expect(!own("solana", [sent("1click", "dep", "USDC→NEAR 5 from Solana to example.near")]))
    #expect(own("solana", [sent("relay", "r2", "Base USDC→Solana 1,234.5, tx 0xcd")]))
}

@Test func aPayeeIsNotWhereItLands() {
    // Paid out to someone else: nothing of ours lands, not even where it started.
    #expect(!own("solana", [sent("1click", "dep", "Solana USDC→0.5 ETH to Mom")]))
    #expect(own("base", [sent("1click", "dep", "NEAR→USDC 5 to 0xabc on Base")]))
    #expect(own("solana", [sent("1click", "dep", "NEAR→USDC 5 to abc on Solana")]) && !own("base", [sent("1click", "dep", "NEAR→USDC 5 to abc on Solana")]))
    let across = [sent("1click", "dep", "SOL→ETH 1 solana → base")]
    #expect(own("base", across) && !own("solana", across))
}

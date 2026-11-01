import ExecutionCore
import Foundation
import Testing

@Test func journalKeepsOrderAndFindsUnresolvedSends() throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("journal-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    try journal.append(.init(chain: "solana", id: "sigA", kind: .signed, note: "swap"))
    try journal.append(.init(chain: "solana", id: "sigB", kind: .signed, note: "bridge deposit"))
    try journal.append(.init(chain: "solana", id: "sigA", kind: .confirmed))
    try journal.append(.init(chain: "1click", id: "depositX", kind: .signed))

    #expect(try journal.entries().count == 4)
    #expect(try journal.unresolved().map(\.id) == ["sigB", "depositX"])
    let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
    #expect((attributes[.posixPermissions] as? Int) == 0o600)
}

@Test func aSendWrittenNowIsFoundFromTheSecondItStarted() throws {
    // The app asks which sends since a move started have no outcome (Execution.swift unsettled); the journal keeps whole
    // seconds, so the start is floored the same way or a send in that second is missed.
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("journal-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    let started = Date()
    try journal.append(.init(chain: "base", id: "0xabc", kind: .signed, note: "Relay deposit"))
    let from = Date(timeIntervalSince1970: started.timeIntervalSince1970.rounded(.down))
    #expect(try journal.unresolved().filter { $0.at >= from }.map(\.id) == ["0xabc"])
}

@Test func aMoveThatWentThroughIsFoundButItsStepsThatMoveNoMoneyAreNot() throws {
    // A swap confirmed, then the balance read after it failed (Wallet.swift convert, a node's 429): the app says what
    // went through and offers no «Try again» (Execution.swift handleWallet). An approve, a Permit2 allowance, a cleared
    // allowance or a revoke on its own, and on Hyperliquid anything but a fill or a transfer (leverage, fee approval,
    // referral code, a cancel, a stop resting on the book) move no money: a refusal after them stays one the user may retry.
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("journal-\(UUID().uuidString).jsonl")
    defer { try? FileManager.default.removeItem(at: url) }
    let journal = Journal(url: url)
    let steps: [(String, String, Journal.Entry.Kind, String)] = [
        ("solana", "swapSig", .signed, "Jupiter 3 USDC→USDT"), ("solana", "swapSig", .confirmed, ""),
        ("base", "0xapprove", .signed, "Approve exactly 3 USDC to Uniswap"), ("base", "0xapprove", .confirmed, ""),
        ("base", "0xbatch", .signed, "Approve exactly 3 USDC to Uniswap + Swap 3 USDC for WETH"), ("base", "0xbatch", .confirmed, ""),
        ("hyperliquid", "1", .signed, "referral code IHATECRYPTO"), ("hyperliquid", "1", .confirmed, "ok"),
        ("hyperliquid", "2", .signed, "leverage"), ("hyperliquid", "2", .confirmed, "ok"),
        ("hyperliquid", "3", .signed, "approve ihatecrypto's fee 0.05%"), ("hyperliquid", "3", .confirmed, "approve ihatecrypto's fee 0.05%: ok"),
        ("relay", "req", .signed, "Base USDC→Solana 3, tx 0xbatch"), ("relay", "req", .settled, "success"),
        ("solana", "failedSig", .signed, "send 1 USDC"), ("solana", "failedSig", .failed, "custom program error"),
        ("base", "0xpermit", .signed, "Permit2: let Uniswap's router take exactly 3 USDC, for 30 minutes"), ("base", "0xpermit", .confirmed, ""),
        ("arbitrum", "0xclear", .signed, "Clear the USDT0 bridge's old allowance"), ("arbitrum", "0xclear", .confirmed, ""),
        ("base", "0xrevoke", .signed, "Revoke 0x1234…abcd's approval of 0x5678…ef01 on Base"), ("base", "0xrevoke", .confirmed, ""),
        ("hyperliquid", "4", .signed, "cancel limit"), ("hyperliquid", "4", .confirmed, "ok"),
        ("hyperliquid", "5", .signed, "stop and take-profit"), ("hyperliquid", "5", .confirmed, "resting on the book"),
        ("hyperliquid", "6", .signed, "spot buy HYPE/USDC"), ("hyperliquid", "6", .confirmed, "filled 0.5 at 40"),
        ("hyperliquid", "7", .signed, "usdClassTransfer perp → spot 20"), ("hyperliquid", "7", .confirmed, "ok"),
        ("hyperliquid", "8", .signed, "HLP deposit"), ("hyperliquid", "8", .confirmed, "ok"),
        // A swap with no gas of ours: the journal keys it by our signature, the explorer knows it by the sponsor's.
        ("solana", "ourSig", .signed, "Jupiter 1 USDC→SOL; Jupiter order r1"), ("solana", "ourSig", .confirmed, "transaction sponsorSig"),
        ("solana", "pendingSig", .signed, "send 2 USDC"),
        ("near", "nearHash", .confirmed, "send 1 NEAR to Automation"),
    ]
    for (chain, id, kind, note) in steps { try journal.append(.init(chain: chain, id: id, kind: kind, note: note)) }

    let went = try journal.wentThrough()
    #expect(went.map(\.id) == ["swapSig", "0xbatch", "req", "6", "7", "8", "sponsorSig", "nearHash"])
    // Each with what it was signed as: a Hyperliquid action or a Relay order is named by it.
    #expect(went.first { $0.id == "req" }?.note == "Base USDC→Solana 3, tx 0xbatch")
    #expect(went.first { $0.id == "6" }?.note == "spot buy HYPE/USDC")
    #expect(try journal.unresolved().map(\.id) == ["pendingSig"])
}

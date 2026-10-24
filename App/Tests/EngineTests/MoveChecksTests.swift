import CWinSec
@testable import DesktopEngine
import Foundation
import Testing

/// What the Mac checks before a move's card (MoveChecks.swift): dust is the verified small tokens, a move the wallet can't
/// pay is said before any card, a saved address written out is its name, and the first send to someone warns.
@Suite(.serialized) struct MoveChecksTests {
    /// Sweep dust sells the verified tokens worth $0.20 up to the limit on Base, Arbitrum and Polygon, the most first — never
    /// the gas coin, USDC itself, an unverified token or one on another network.
    @Test func dustIsTheVerifiedSmallTokens() {
        let base = PortfolioView.Network(key: "base", name: "Base", usd: 0, rows: [
            Engine.row("USDC", 3, usd: 3),
            Engine.row("ETH", Decimal(string: "0.001")!, usd: 3, chain: "base", contract: "native"),
            Engine.row("AERO", 2, usd: Decimal(string: "1.4")!, chain: "base", contract: "0x940181a94A35A4569E4529A3CDfB74e38FD98631", verified: true),
            Engine.row("FAKE", 9, usd: 2, chain: "base", contract: "0x000000000000000000000000000000000000c0de", verified: false),
            Engine.row("DUST", 1, usd: Decimal(string: "0.1")!, chain: "base", contract: "0x000000000000000000000000000000000000beef", verified: true),
            Engine.row("BIG", 1, usd: 40, chain: "base", contract: "0x000000000000000000000000000000000000f00d", verified: true),
        ])
        let arbitrum = PortfolioView.Network(key: "arbitrum", name: "Arbitrum", usd: 0, rows: [
            Engine.row("ARB", 4, usd: Decimal(string: "3.2")!, chain: "arbitrum", contract: "0x912ce59144191c1204e64559fe8253a0e49e6548", verified: true),
        ])
        let solana = PortfolioView.Network(key: "solana", name: "Solana", usd: 0, rows: [
            Engine.row("JUP", 3, usd: 2, chain: "solana", contract: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", verified: true),
        ])
        let dust = Engine.dust(in: [base, arbitrum, solana], under: nil)
        #expect(dust.map(\.symbol) == ["ARB", "AERO"])
        #expect(Engine.dust(in: [base, arbitrum], under: 2).map(\.symbol) == ["AERO"])
    }

    private func walletEngine() throws -> EngineTests.Engine {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        return engine
    }

    /// The first balance read: the one `createWallet` starts may still be running, and a second `refresh` returns at once.
    private func read(_ engine: EngineTests.Engine) throws {
        for _ in 0..<30 {
            // A refresh slower than the helper waits (a node not answering under load) is tried again.
            if ((try? engine.call("refresh"))?.state?["portfolio"] as? [String: Any])?["updatedAt"] != nil { return }
        }
        Issue.record("the balances were never read")
    }

    /// An empty wallet asked to put 5 USDC into Jupiter Lend hears what there is and the ways on, and no card is drawn;
    /// one asked to sweep dust hears there's none; neither says the move «isn't in the Windows app».
    @Test(.enabled(if: winsec_is_stub() == 1)) func aShortfallIsSaidBeforeACard() throws {
        let engine = try walletEngine()
        try read(engine)
        let deposit = try engine.call("send", ["text": "deposit 5 USDC into jupiter lend"])
        let said = EngineTests.chat(deposit.state).last ?? ""
        #expect(deposit.state?["card"] == nil || deposit.state?["card"] is NSNull, "\(said)")
        #expect(said.contains("On Solana there's 0 USDC") && said.contains("asks 5"), "\(said)")
        let cards = (deposit.state?["options"] as? [[String: Any]] ?? []).compactMap { $0["title"] as? String }
        #expect(cards.contains("Top up first"), "\(cards)")

        let dust = try engine.call("send", ["text": "sweep dust"])
        let line = EngineTests.chat(dust.state).last ?? ""
        #expect(line.contains("no dust to sweep") && !line.contains("isn't in the Windows app"), "\(line)")
        #expect(dust.state?["card"] == nil || dust.state?["card"] is NSNull)
    }

    /// «send 2 USDC to 0x…» with that address saved as Mom: the guard doesn't stop it, the card goes to Mom by name, and,
    /// the first time, it says to try a small amount first.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aSavedAddressIsItsNameAndAFirstSendWarns() throws {
        // No wallet, so no balances arrive to say the send is short before its card (Chains.transferShortfall).
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
        let saved = try engine.call("saveAddress", ["name": "Mom", "network": "base", "address": mom])
        #expect(saved.answer["ok"] as? Bool == true, "\(saved.answer)")
        let sent = try engine.call("send", ["text": "send 2 USDC to \(mom) on base"])
        let card = sent.state?["card"] as? [String: Any]
        #expect(card != nil, "\(EngineTests.chat(sent.state).last ?? "")")
        #expect((card?["title"] as? String)?.contains("Mom") == true, "\(card ?? [:])")
        let warnings = card?["warnings"] as? [String] ?? []
        #expect(warnings.contains { $0.contains("First transfer to this address") }, "\(warnings)")
        // A caution, not a warning: Confirm keeps its plain name.
        #expect(card?["confirm"] as? String == "Confirm")
    }

    /// A move between the user's own places is drawn as its route: from, through the bridge, to — each a step.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aWithdrawalIsDrawnAsItsRoute() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        // Hardhat's #0, watched, asked before its balances land (they take long): the route doesn't wait for them. Read
        // first and dry on Hyperliquid, it's said instead of a card.
        try engine.call("watch", ["solana": "7cVfgArCheMR6Cs4t6vz5rfnqd56vZq4ndaBrY5xkxXy", "evm": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"])
        let r = try engine.call("send", ["text": "withdraw 1 USDC to solana"])
        guard let id = (r.state?["card"] as? [String: Any])?["id"] as? String else {
            #expect(EngineTests.chat(r.state).last?.contains("Hyperliquid") == true, "\(EngineTests.chat(r.state).last ?? "")")
            return
        }
        let board = (r.state?["boards"] as? [[String: Any]] ?? []).first { $0["id"] as? String == id }
        let labels = (board?["steps"] as? [[String: Any]] ?? []).compactMap { $0["label"] as? String }
        #expect(labels == ["FROM · HYPERLIQUID", "BRIDGE · RELAY", "TO · SOLANA"], "\(labels)")
    }
}

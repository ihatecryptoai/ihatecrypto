@testable import DesktopEngine
@testable import EVMChain
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// Consent's wave 6 (audit5 S2, S3, G11): a card the second check doubts confirms only with its tick, the engine holding
/// it; a site's request goes with its page, its canvas or a lock; a site's transaction names its tokens.
/// In-process, public test data only, nothing goes to the network or the signer.
@MainActor @Suite(.serialized) struct Consent7Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("consent7-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    // Hardhat's public account #3 and Base's USDC.
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
    static let en: (String, String) -> String = { a, _ in a }

    private var engine: Engine { Self.engine }

    private func fresh() {
        engine.settings.language = "en"
        engine.cancelPending()
        engine.declineSite("test")
    }

    private func waitForSite() async throws -> SiteAskView {
        for _ in 0..<200 {
            if let v = engine.siteAsk?.view { return v }
            try await Task.sleep(for: .milliseconds(5))
        }
        throw WalletErrorText("the site's request never came")
    }

    @Test func aDoubtedCardConfirmsOnlyWithItsTick() async {
        fresh()
        engine.show(.address(SignerAddress(name: "Mom", network: "base", address: Self.mom)), title: "Save “Mom” to Addresses")
        engine.card?.ticks = [Engine.tradeTick("The plan risks $50, the message named $5.", say: Self.en)]
        let id = engine.card?.id
        // Without the tick the engine holds the card, whatever the button sent.
        await engine.confirm(card: id, ticks: [])
        #expect(engine.card?.id == id && engine.pending != nil && engine.runningBoard == nil)
        #expect(engine.messages.last?.text.contains("I understand the second check's warning and execute anyway") == true)
        await engine.confirm(card: id, ticks: ["other"])
        #expect(engine.card?.id == id && engine.runningBoard == nil)
        #expect(engine.untickedOnCard(["doubt"]) == nil)
        engine.cancelPending()
        // A card with nothing to tick confirms as before (the check is only for what it carries).
        engine.show(.address(SignerAddress(name: "Mom", network: "base", address: Self.mom)), title: "Save “Mom” to Addresses")
        #expect(engine.untickedOnCard([]) == nil)
        engine.cancelPending()
    }

    @Test func aSitesRequestGoesWithItsPageAndItsCanvas() async throws {
        fresh()
        var asked = Task { await engine.ask("jup.ag", origin: "https://jup.ag", site: .known, connect: true, title: "jup.ag asks to connect", lines: []) }
        var site = try await waitForSite()
        // Another page going away leaves it; its own page going away declines it, and that's on its block.
        engine.siteGone("https://app.hyperliquid.xyz")
        #expect(engine.siteAsk?.view.id == site.id)
        engine.siteGone("https://jup.ag/")
        #expect(await asked.value == false)
        #expect(engine.siteAsk == nil && engine.boards.first { $0.id == site.id }?.state == "cancelled")
        #expect(engine.boards.first { $0.id == site.id }?.lines.last?.contains("The page closed before an answer") == true)
        // The canvas it's on is deleted: declined too, its ticks were only there.
        asked = Task { await engine.ask("jup.ag", origin: "https://jup.ag", site: .known, title: "jup.ag asks to sign", lines: []) }
        site = try await waitForSite()
        engine.declineSite(onCanvas: [])
        #expect(engine.siteAsk?.view.id == site.id)
        engine.declineSite(onCanvas: engine.boards)
        #expect(await asked.value == false && engine.siteAsk == nil)
    }

    @Test func aSitesTransactionNamesItsTokens() async {
        let token: (String) async -> (symbol: String, decimals: Int) = { _ in ("USDC", 6) }
        // 1.5 USDC to Mom: «Transfer 1.5 USDC to 0x…», never raw units.
        var word = Data(repeating: 0, count: 32)
        word[29] = 0x16; word[30] = 0xE3; word[31] = 0x60
        let transfer = await engine.callLine(.transfer(to: Self.mom, amount: word), to: Self.usdc, value: 0, from: Self.mom, chainId: 8453, native: "ETH", token: token)
        #expect(transfer == "Transfer 1.5 USDC to \(Self.mom).", "\(transfer)")
        let unlimited = await engine.callLine(.approve(spender: Self.mom, amount: Data(repeating: 0xff, count: 32)), to: Self.usdc, value: 0, from: Self.mom,
                                              chainId: 8453, native: "ETH", token: token)
        #expect(unlimited == "Allow \(Self.mom) to spend up to unlimited USDC.", "\(unlimited)")
        let coins = await engine.callLine(.plain, to: Self.mom, value: 500_000_000_000_000_000, from: Self.mom, chainId: 8453, native: "ETH", token: token)
        #expect(coins == "Transfer 0.5 ETH to \(Self.mom).", "\(coins)")
    }
}

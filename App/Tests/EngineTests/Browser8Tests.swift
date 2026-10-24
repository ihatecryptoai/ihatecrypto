@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// The browser's wave 7 (audit6 G19 and its trivials): a site's Solana transaction reads as the Mac's sheet — what you
/// send and get, fee-sized SOL said as the fee, the programs with Solana's own on one line, the checks folded — and the
/// toolbar's Disconnect takes the wallet away whatever wallet is on. In-process, public test data only, no network.
@MainActor @Suite(.serialized) struct Browser8Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("browser8-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    static let en: (String, String) -> String = { a, _ in a }
    static let usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    static let solMint = "So11111111111111111111111111111111111111112"

    @Test func aSolanaTransactionLeadsWithWhatYouSendAndGet() {
        let fee = (symbol: "SOL", mint: Self.solMint, amount: Decimal(string: "-0.00204")!)
        let usdc = (symbol: "USDC", mint: Self.usdcMint, amount: Decimal(-5))
        // Fee-sized SOL beside a real move is the fee, not a send; Kamino's two programs are Kamino.
        #expect(Engine.solanaLead([fee, usdc], programs: ["Compute Budget", "Kamino Vault", "Kamino Farms"], say: Self.en) == "You send 5 USDC to Kamino")
        #expect(Engine.solanaLead([usdc, (symbol: "SOL", mint: Self.solMint, amount: Decimal(string: "0.02")!)], programs: ["Jupiter"], say: Self.en)
                == "You send 5 USDC and get 0.02 SOL")
        #expect(Engine.solanaLead([(symbol: "USDC", mint: Self.usdcMint, amount: 3)], programs: [], say: Self.en) == "You get 3 USDC")
        #expect(Engine.solanaLead([(symbol: "SOL", mint: Self.solMint, amount: 0)], programs: ["System"], say: Self.en) == "Only the network fee")
        #expect(Engine.solanaLead([], programs: [], say: Self.en) == nil)
        // A lone SOL move is a send, whatever its size; only beside another change is small SOL the fee.
        #expect(Engine.feeLike("SOL", Decimal(string: "-0.01")!, count: 2) && !Engine.feeLike("SOL", Decimal(string: "-0.01")!, count: 1))
        #expect(!Engine.feeLike("SOL", Decimal(string: "-0.2")!, count: 2) && !Engine.feeLike("USDC", -1, count: 2))
    }

    @Test func itsProgramsReadWithSolanasOwnOnOneLine() {
        let line = Engine.callsLine([(name: "System", known: true), (name: "Kamino Vault", known: true), (name: "7xKX…sAsU", known: false),
                                     (name: "Compute Budget", known: true)], say: Self.en)
        #expect(line == "It calls: Kamino Vault · 7xKX…sAsU (not on ihatecrypto's list) · and Solana's own: System, Compute Budget", "\(line)")
    }

    @Test func theCardCarriesTheLeadBalancesChecksAndNoteAndItsBlockSaysThem() async throws {
        let engine = Self.engine
        engine.settings.language = "en"
        engine.declineSite("test")
        var review = Engine.SiteReview(lines: ["It calls: Kamino Vault"], changes: [], warnings: [], problems: [])
        review.lead = "You send 5 USDC to Kamino"
        review.balances = [SiteBalance(symbol: "USDC", amount: "−5", up: false, usd: "$5.00", note: nil),
                           SiteBalance(symbol: "SOL", amount: "−0.00204", up: false, usd: nil, note: "network fee and rent for new accounts")]
        review.checks = ["Every instruction is for a program on the list."]
        let asked = Task { await engine.ask("app.kamino.finance", origin: "https://app.kamino.finance", site: .known, title: "app.kamino.finance asks to sign a transaction",
                                            lines: review.lines, review: review, note: "The site gets the signed transaction and sends it itself.") }
        var view: SiteAskView?
        for _ in 0..<200 where view == nil { view = engine.siteAsk?.view; if view == nil { try await Task.sleep(for: .milliseconds(5)) } }
        let shown = try #require(view)
        #expect(shown.lead == "You send 5 USDC to Kamino" && shown.balances.count == 2 && shown.checks.count == 1)
        #expect(shown.note?.hasPrefix("The site gets") == true && shown.canvas == engine.canvasID)
        let said = engine.boards.first { $0.id == shown.id }?.steps?.first?.lines ?? []
        #expect(said.first == "You send 5 USDC to Kamino" && said.contains("USDC −5 ≈ $5.00") && said.contains("SOL −0.00204 · network fee and rent for new accounts"), "\(said)")
        engine.declineSite("test")
        #expect(await asked.value == false)
    }

    @Test func disconnectTakesTheWalletAwayWithNoWalletLedgerOrAutomationInTheWay() async throws {
        let engine = Self.engine
        // No wallet at all here (no signer in this test): every other request is refused first; Disconnect isn't.
        #expect(engine.activeWallet == nil)
        engine.sites.solana.insert("https://jup.ag")
        engine.sites.evm.insert("https://jup.ag")
        engine.sites.chain["https://jup.ag"] = 8453
        #expect(try await engine.dapp("disconnect", params: #"{"everywhere":true}"#, origin: "https://jup.ag") == "null")
        #expect(!engine.sites.solana.contains("https://jup.ag") && !engine.sites.evm.contains("https://jup.ag") && engine.sites.chain["https://jup.ag"] == nil)
        // A page's own Solana disconnect leaves its EVM connection alone.
        engine.sites.solana.insert("https://jup.ag")
        engine.sites.evm.insert("https://jup.ag")
        _ = try await engine.dapp("disconnect", params: "{}", origin: "https://jup.ag")
        #expect(!engine.sites.solana.contains("https://jup.ag") && engine.sites.evm.contains("https://jup.ag"))
        engine.sites.evm.remove("https://jup.ag")
        // Anything else still needs a wallet.
        await #expect(throws: (any Error).self) { try await engine.dapp("connect", params: "{}", origin: "https://jup.ag") }
    }
}

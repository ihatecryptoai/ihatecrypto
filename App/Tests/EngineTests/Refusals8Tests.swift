@testable import DesktopEngine
import Foundation
@testable import IhatecryptoCore
import SignerAPI
import Testing
@testable import TradeExecution

/// After a stop by the guard, the signer or a scam check nothing is offered around it (the Mac's a3f7edb); a chain's
/// paid first step names its price; a saved name is found as the Mac's savedEntry finds it. In-process, by hand.
@MainActor @Suite(.serialized) struct Refusals8Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("refusals8-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }
    // Public test data only: Hardhat's accounts #3 and #1, the multisig a made-up address of zeros and ones.
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let phone = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    static let safe = "0x1111111111111111111111111111111111111111"

    @Test func aSignersRefusalIsAStopThatNothingIsOfferedAround() throws {
        let refused = LinkTradeSigner.refused(SignerReply(ok: false, error: "Windows Hello was declined."))
        #expect(refused.localizedDescription == "Windows Hello was declined.")
        #expect(!Engine.explainsAfter(refused))
        #expect(!Engine.explainsAfter(GuardStop("The guard stopped the swap: a program off the list.")))
        #expect(!Engine.explainsAfter(CostlyMove(lost: 2, paid: 10, what: "Relay")))
        // A move that only lacked money or a route gets the way that works.
        #expect(Engine.explainsAfter(ExecutionError("Only 3 USDC on Base.")))
        // Every refusal the link says is a stop, a method added later included.
        let source = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Sources").appendingPathComponent("Desktop").appendingPathComponent("Engine").appendingPathComponent("LinkTradeSigner.swift")
        let text = try String(contentsOf: source, encoding: .utf8)
        #expect(text.contains("throw Self.refused(reply)") && !text.contains("throw ExecutionError"))
    }

    @Test func aFailedChainStepStopsTheStepsAfterIt() async {
        engine.settings.language = "en"
        // No wallet in this engine: the buy, the second step, fails at its move; the chart after it never runs.
        try? await engine.runChain([.said("clear the plan"), .wallet(.buy(SpotAsset.jitoSOL, usdc: 1)), .said("chart SOL")])
        let said = engine.messages.filter { !$0.fromUser }.map(\.text)
        #expect(said.contains { $0.hasPrefix("Failed: Add a wallet first.") }, "\(said)")
        #expect(said.contains { $0.contains("didn’t go through, so the steps after it don’t run") }, "\(said)")
        #expect(!engine.boards.contains { $0.kind == "chart" })
    }

    @Test func aChainsPaidFirstStepNamesItsPrice() {
        engine.settings.language = "en"
        let pay = WalletCommand.pay(method: "GET", url: "https://api.example.com/answer", body: nil)
        #expect(engine.chainLine(.wallet(pay)) == pay.summary)
        let challenge = MPP.Challenge(params: [:], amount: 10_000, payee: Self.phone, memo: nil, expires: nil)
        engine.payment = Engine.PaymentQuote(command: pay, challenge: challenge, ask: "the answer", service: "Example")
        #expect(engine.chainLine(.wallet(pay)) == "pay 0.01 USDC to Example")
        engine.payment = nil
    }

    @Test func aSavedNameIsFoundAsTheMacFindsIt() {
        engine.book = [SignerAddress(name: "Automation", network: "solana", address: "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", memo: ""),
                       SignerAddress(name: "Automation", network: "base", address: Self.phone, memo: ""),
                       SignerAddress(name: "Mom Base", network: "base", address: Self.mom, memo: "")]
        // Hyperliquid is EVM: the Base entry, not the Solana one that can't receive there.
        #expect(engine.entry("Automation", network: "hyperliquid")?.address == Self.phone)
        // A name that contains the word.
        #expect(engine.entry("mom", network: "base")?.address == Self.mom)
        // The multisig, only on a network it's made on.
        engine.multisigs = [Multisig(address: Self.safe, keys: [], threshold: 1, salt: 0, networks: ["base"])]
        #expect(engine.entry("the multisig", network: "base")?.address == Self.safe)
        #expect(engine.entry("the multisig", network: "arbitrum") == nil)
        engine.multisigs = []
        engine.book = []
    }
}

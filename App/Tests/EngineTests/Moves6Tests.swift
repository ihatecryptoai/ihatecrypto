@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing
@testable import TradeExecution

/// Money moves as the Mac makes them (wave 5, audit4 B): a key wallet moves money, an automation task's send to a saved
/// name is a transfer, a trade confirmed late gets fresh numbers instead of a «Done», the second check says when it
/// couldn't run, a send with no outcome is «unknown», «Send anyway» takes a little more than the loss it named.
/// In-process, balances set by hand; no model is reachable (OpenRouter with no key), nothing goes to a signer.
@MainActor @Suite(.serialized) struct Moves6Tests {
    static let engine: Engine = {
        // In-process suites share one store folder (Store.folder is read once): set it only if no suite did.
        if ProcessInfo.processInfo.environment["IHATECRYPTO_ENGINE_DIR"] == nil {
            setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("moves6-\(UUID().uuidString)").path)
        }
        let e = Engine(out: Outbox(silent: true))
        // No model: a review or an explanation never reaches a real one.
        e.settings.provider = "openrouter"
        e.settings.language = "en"
        return e
    }()

    // Public test data only: Hardhat's accounts.
    static let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"

    private var engine: Engine { Self.engine }

    private func reset() {
        // The folder is shared in-process, and a suite may remove its own when it ends (TaskSealTests).
        try? FileManager.default.createDirectory(at: Store.folder, withIntermediateDirectories: true)
        engine.status = nil
        engine.pending = nil
        engine.card = nil
        engine.book = []
        engine.portfolio = PortfolioView()
    }

    private func plan() -> (TradeIntent, Executor.Prepared, Market) {
        let market = Market(name: "ETH", markPx: 2000, szDecimals: 4, maxLeverage: 25)
        var intent = TradeIntent()
        intent.asset = "ETH"
        intent.side = .long
        let sizing = RiskCalc.size(riskUSD: 1, stopPct: 2, takePct: 4, leverage: 3, side: .long, market: market)
        let p = Executor.Prepared(intent: intent, market: market, sizing: sizing, leverage: 3, bridge: nil, changes: [], steps: ["Long 0.01 ETH"],
                                  preparedAt: Date().addingTimeInterval(-60))
        return (intent, p, market)
    }

    @Test func aKeyWalletMovesMoneyWithTheOtherChainEmpty() async throws {
        reset()
        var s = SignerReply(ok: true)
        s.hasWallet = true
        s.evmAddress = Self.me
        s.wallets = [SignerWallet(id: "key", label: "Key", kind: "evm", solana: nil, evm: Self.me, near: nil)]
        s.activeWallet = "key"
        engine.status = s
        let wallet = await (try engine.executor()).wallet
        #expect(wallet.evm == Self.me && wallet.solana.isEmpty)
        // A Solana move from it is said before its card, not failed at Confirm.
        #expect(engine.missingKey(.stake(1)) != nil)
        reset()
        #expect(throws: (any Error).self) { try engine.executor() }
    }

    @Test func anAutomationSendToASavedNameIsATransfer() async throws {
        reset()
        engine.book = [SignerAddress(name: "Mom", network: "base", address: Self.mom)]
        let task = ScheduledTask(steps: ["send 5 USDC to Mom", "chart SOL"], next: Date(), every: nil)
        let steps = try #require(await engine.automationSteps(task))
        guard case .transfer(let entry, let amount) = try #require(steps.first) else { Issue.record("not a transfer: \(steps)"); return }
        #expect(entry.name == "Mom" && entry.network == "base" && amount == 5)
        guard case .said("chart SOL") = steps[1] else { Issue.record("a chart runs by its words: \(steps)"); return }
        // A contract call is never carried out unseen: it draws its card.
        let call = ScheduledTask(steps: ["call base \(Self.mom) deposit(uint256) 1"], next: Date(), every: nil)
        if WalletCommand.parse(call.steps[0]) != nil {
            let s = try #require(await engine.automationSteps(call))
            guard case .said = try #require(s.first) else { Issue.record("a call went as a move: \(s)"); return }
        }
        // A name not saved stops the run, said.
        let unknown = ScheduledTask(steps: ["send 5 USDC to Stranger"], next: Date(), every: nil)
        #expect(await engine.automationSteps(unknown) == nil)
    }

    @Test func aTradeConfirmedLateGetsFreshNumbersNotADone() async throws {
        reset()
        let (_, p, _) = plan()
        engine.show(.trade(p), title: "Plan · long ETH")
        let id = try #require(engine.card?.id)
        let before = engine.lastMove?.at
        await engine.confirm(card: id)
        #expect(!engine.boards.contains { $0.id == id && $0.state == "done" }, "\(engine.boards.map { ($0.id == id, $0.state ?? "") })")
        #expect(engine.lastMove?.at == before)
        #expect(engine.messages.contains { !$0.fromUser && $0.text.hasPrefix("Numbers refreshed") })
        engine.cancelPending()
    }

    @Test func theSecondCheckSaysWhenItCouldNotRun() async throws {
        reset()
        let (intent, p, market) = plan()
        engine.settings.secondCheck = true
        guard case .notChecked(let why) = await engine.reviewPlan(intent, p) else { Issue.record("a check with no model passed"); return }
        #expect(why == "The second check couldn't run: connect a model in Settings › Sign-ins. Only code checks apply.")
        #expect(Engine.notChecking("timed out", ru: false) == "The second check couldn't run: timed out. Only code checks apply.")
        // The route says it, rather than «All correct».
        let e = Plan.evaluate(intent, market: market)
        let blocks = Plan.blocks(intent, e, engine.planLive(market: market, notes: []))
        func second(_ route: (steps: [Board.Step], footer: String)) -> Board.Step? { route.steps.first { $0.label == "SECOND CHECK" } }
        let unchecked = second(engine.tradeRoute(intent, e, blocks, market: market, prepared: p, review: nil, reviewed: true, notChecked: why))
        #expect(unchecked?.title == "Not checking" && unchecked?.lines == [why] && unchecked?.status == "info")
        let doubt = second(engine.tradeRoute(intent, e, blocks, market: market, prepared: p, review: "It risks $5, not $1.", reviewed: true))
        #expect(doubt?.title == "Has doubts" && doubt?.status == "attention")
        let fine = second(engine.tradeRoute(intent, e, blocks, market: market, prepared: p, review: nil, reviewed: true))
        #expect(fine?.title == "All correct")
    }

    @Test func aSendWithNoOutcomeIsUnknownNotFailed() async throws {
        reset()
        let entry = SignerAddress(name: "Mom", network: "base", address: Self.mom)
        engine.show(.transfer(entry, 1), title: "Send 1 USDC to Mom")
        let id = try #require(engine.card?.id)
        // Something went out a moment ago and nothing came back for it; the move itself then fails.
        try engine.journal.append(.init(chain: "base", id: "0xabc", kind: .signed, note: "test"))
        await engine.confirm(card: id)
        #expect(engine.boards.first { $0.id == id }?.state == "unknown")
        #expect(engine.messages.last { !$0.fromUser }?.text.hasPrefix("Sent, status unknown") == true)
        try engine.journal.append(.init(chain: "base", id: "0xabc", kind: .failed, note: "test"))
    }

    @Test func sendAnywayTakesALittleMoreThanTheLossItNamed() {
        #expect(Engine.acceptedLoss(2) == Decimal(string: "2.55")!)
        #expect(Engine.acceptedLoss(0) == Decimal(string: "0.05")!)
    }

    @Test func allIsTheAmountHeld() {
        reset()
        engine.portfolio = PortfolioView(networks: [PortfolioView.Network(key: "solana", name: "Solana", usd: 20, rows: [Engine.row("USDC", 20, usd: 20)])], updatedAt: Date())
        let steps = engine.routeSteps(.lendDeposit("USDC", nil)) ?? []
        #expect(steps.first?.title == "20 USDC", "\(steps.map(\.title))")
        #expect(steps.first?.lines.first?.hasPrefix("all available") == true, "\(steps.first?.lines ?? [])")
        // Not read yet: the words stay.
        engine.portfolio = PortfolioView()
        #expect(engine.routeSteps(.lendDeposit("USDC", nil))?.first?.title == "all USDC")
    }

    @Test func aLimitWaitingCountsAsOpen() throws {
        reset()
        let json = #"[{"coin":"ETH","oid":1,"isTrigger":false,"limitPx":"2400","sz":"0.01","side":"B","orderType":"Limit","reduceOnly":false,"children":[]}]"#
        guard let orders = try? JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(json.utf8)) else { return }
        engine.positions = []
        engine.hlOrders = orders
        #expect(engine.targetCoin(for: "cancel orders") == "ETH")
        #expect(engine.limitWaitingLine == "Limit waiting: long ETH at 2,400")
        engine.hlOrders = []
        #expect(engine.targetCoin(for: "cancel orders") == nil && engine.limitWaitingLine == nil)
    }
}

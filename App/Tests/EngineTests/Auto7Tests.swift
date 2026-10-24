@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing
@testable import TradeExecution

/// Wave 6's automation and money paths (audit5): in-process, with a wallet of public test addresses and nothing sent to
/// the network.
@MainActor @Suite(.serialized) struct Auto7Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("auto7-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()
    private var engine: Engine { Self.engine }

    /// The wallet that's on: a phrase wallet, or a key for one chain only. «abandon … about»'s and Hardhat's addresses.
    private func wallet(_ kind: String = "phrase") {
        engine.settings.language = "en"
        var s = SignerReply(ok: true)
        let w = SignerWallet(id: "1", label: "Main", kind: kind, solana: kind == "evm" ? nil : "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk",
                             evm: kind == "solana" ? nil : "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", near: nil)
        s.wallets = [w]
        s.activeWallet = "1"
        (s.solanaAddress, s.evmAddress) = (w.solana, w.evm)
        engine.status = s
    }

    /// S1: a scheduled step that names no amount never runs as «all of it» on the automation wallet, where nobody may be
    /// at the computer: the run stops before anything moves and says so (the Mac's run asks «How much?» and stops).
    @Test func anAutomationStepWithoutItsAmountStopsTheRun() async {
        wallet()
        for step in ["buy SOL", "deposit USDC into Kamino", "stake SOL"] {
            #expect(WalletCommand.parse(step)?.amountQuestion != nil, "\(step)")
            let steps = await engine.automationSteps(ScheduledTask(steps: [step], next: Date(), every: .days(1)))
            #expect(steps == nil, "«\(step)» would run as \(String(describing: steps))")
            #expect(engine.messages.last?.text.contains("doesn't say how much") == true, "\(engine.messages.last?.text ?? "")")
        }
        // A step after a good one stops the run too: nothing of it runs.
        #expect(await engine.automationSteps(ScheduledTask(steps: ["buy SOL for 5 USDC", "stake SOL"], next: Date(), every: .days(1))) == nil)
        // With its amount it goes on.
        #expect(await engine.automationSteps(ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date(), every: .days(1)))?.count == 1)
    }

    /// G1: a pay step of a chain or a scheduled run has no card's quote; its price is asked when it runs, so it's refused
    /// for what the service is (here one that refuses the payment's proof), never «the price is gone».
    @Test func aPayStepAsksItsPriceWhenItRuns() async throws {
        wallet()
        engine.payment = nil
        let host = try #require(MPP.refusesProof.sorted().first)
        let pay = WalletCommand.pay(method: "GET", url: "https://\(host)/v1/x", body: nil)
        do {
            _ = try await engine.pay(pay, try engine.executor())
            Issue.record("paid without a price")
        } catch {
            #expect(error.localizedDescription.contains("refuses its proof"), "\(error.localizedDescription)")
        }
    }

    /// B1: a key wallet is refused only what its chain can't sign (the Mac's missingKey).
    @Test func aKeyWalletIsRefusedOnlyWhatItsChainCantSign() throws {
        let sol = try #require(SpotAsset.all.first { $0.symbol == "SOL" })
        wallet("evm")
        #expect(engine.missingKey(.stopSchedules) == nil)
        #expect(engine.missingKey(.swapOn("base", from: "USDC", to: "ETH", amount: 5)) == nil)
        #expect(engine.missingKey(.buy(sol, usdc: 5)) != nil)
        wallet("solana")
        #expect(engine.missingKey(.jupiterDCA(sol, usdc: 100, orders: 2, every: 86_400)) == nil)
        #expect(engine.missingKey(.buy(sol, usdc: 5)) == nil)
        #expect(engine.missingKey(.bridge(from: "base", to: "arbitrum", 5)) != nil)
        wallet()
        #expect(engine.missingKey(.bridge(from: "base", to: "arbitrum", 5)) == nil)
    }

    /// B11: a recurring buy of a coin off the fixed list keeps its mint, so each run buys that token and never another one
    /// trading under its ticker; one on the list keeps its name.
    @Test func aRecurringBuyKeepsItsToken() throws {
        // SAMO's public mint, not on the fixed list.
        let samo = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU"
        guard !SpotAsset.all.contains(where: { $0.mint == samo }) else { return }
        let step = Engine.recurringStep(.buy(SpotAsset(symbol: "SAMO", mint: samo, decimals: 9, token2022: false), usdc: 2))
        #expect(step == "buy token \(samo) for 2 USDC")
        guard case .buy(let a, let u)? = WalletCommand.parse(step) else { Issue.record("«\(step)» doesn't read back as a buy"); return }
        #expect(a.mint == samo && u == 2)
        let sol = try #require(SpotAsset.all.first { $0.symbol == "SOL" })
        #expect(Engine.recurringStep(.buy(sol, usdc: 5)) == WalletCommand.buy(sol, usdc: 5).summary)
    }
}

/// In TaskSealTests' suite, which runs one at a time: these read and write the same task files.
extension TaskSealTests {
    /// R3, S5: recurring-buy marks follow their wallet's tasks; a removed wallet's tasks and marks go — wallet «1» too, as
    /// the signer gives «1» to the next wallet made into an empty list.
    @MainActor @Test func recurringMarksAndARemovedWalletsTasksGoWithTheirWallet() throws {
        try FileManager.default.createDirectory(at: Store.folder, withIntermediateDirectories: true)
        let engine = Auto7Tests.engine
        defer { WalletScope.tasks = nil }
        let task = ScheduledTask(steps: ["buy SOL for 5 USDC"], next: Date().addingTimeInterval(3600), every: .days(1))
        WalletScope.tasks = SignerWallet.firstID
        engine.tasks = [task]
        engine.recurringBuys = [task.id]
        // Another wallet: none of the first one's marks.
        WalletScope.tasks = "94"
        #expect(engine.recurringBuys.isEmpty)
        WalletScope.tasks = SignerWallet.firstID
        #expect(engine.recurringBuys == [task.id])
        // «1» removed with the list left empty (it still reads as the wallet that's on): its tasks and marks are gone.
        engine.walletRemoved(SignerWallet.firstID)
        #expect(engine.tasks.isEmpty && engine.recurringBuys.isEmpty)
        #expect(TaskSeal.load().tasks.isEmpty)
    }
}

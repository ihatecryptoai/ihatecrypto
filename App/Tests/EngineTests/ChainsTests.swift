import CWinSec
@testable import DesktopEngine
import Foundation
import IhatecryptoCore
import Testing

/// Chains as the Mac runs them (Tasks.swift runSteps): a step that shows something runs before the move's card, a move
/// without its amount asks «How much?» and holds the steps after it, a step after a move spends what arrived, and a send
/// no balance covers is said before any card.
@Suite(.serialized) struct ChainsTests {
    @Test func aChartStepThenABuyChains() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let sent = try engine.call("send", ["text": "chart SOL then buy SOL for 10 USDC"])
        let chat = EngineTests.chat(sent.state)
        // The buy's own card, as when it's asked alone.
        let card = sent.state?["card"] as? [String: Any]
        #expect((card?["title"] as? String)?.contains("SOL") == true, "\(chat.suffix(3))")
        #expect(!chat.contains { $0.contains("can’t do") || $0.contains("can't do") }, "\(chat)")
        // The chart went first: its line (or why there's none now) before the card's.
        let chart = chat.lastIndex { $0.contains("chart") || $0.contains("No chart") }
        let move = chat.lastIndex { $0.hasPrefix("Understood:") }
        #expect(chart != nil && move != nil && chart! < move!, "\(chat)")
    }

    @Test func aStepWithoutAnAmountAsksAndHoldsTheRest() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let asked = try engine.call("send", ["text": "buy SOL then chart SOL"])
        let said = EngineTests.chat(asked.state)
        #expect(said.last?.contains("For how many USDC should I buy SOL?") == true, "\(said.suffix(2))")
        #expect(asked.state?["card"] == nil || asked.state?["card"] is NSNull)
        // The chart waits behind the question.
        #expect(!said.contains { $0.contains("chart is on the canvas") || $0.contains("No chart") }, "\(said)")

        let answered = try engine.call("send", ["text": "10"])
        let card = answered.state?["card"] as? [String: Any]
        #expect(card?["title"] as? String == "2 steps, one Confirm", "\(EngineTests.chat(answered.state).suffix(2))")
        let lines = card?["lines"] as? [String] ?? []
        #expect(lines.contains("1. buy SOL for 10 USDC") && lines.contains("2. chart SOL"), "\(lines)")
        // Still nothing of the chart: it runs once the card is confirmed.
        #expect(!EngineTests.chat(answered.state).contains { $0.contains("chart is on the canvas") || $0.contains("No chart") })
    }

    /// A send inside a chain is drawn from its own network with that network's balance, then to the saved name.
    @Test(.enabled(if: winsec_is_stub() == 1)) func aChainsTransferIsDrawnFromItsNetwork() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        // Hardhat's public account #3.
        let saved = try engine.call("saveAddress", ["name": "Mom", "network": "base", "address": "0x90F79bf6EB2c4f870365E785982E1f101E93b906"])
        #expect(saved.answer["ok"] as? Bool == true, "\(saved.answer)")
        let sent = try engine.call("send", ["text": "send 1 USDC to Mom then chart SOL"])
        let card = try #require(sent.state?["card"] as? [String: Any], "\(EngineTests.chat(sent.state).suffix(2))")
        let board = (sent.state?["boards"] as? [[String: Any]])?.first { $0["id"] as? String == card["id"] as? String }
        let labels = (board?["steps"] as? [[String: Any]] ?? []).compactMap { $0["label"] as? String }
        #expect(labels.first == "FROM · BASE" && labels.last == "THEN" && labels.count == 3, "\(labels)")
    }

    @Test func theNextStepIsFittedToWhatArrived() {
        let lend = WalletCommand.lendDeposit("USDC", 100)
        // Cents short after a bridge: the step spends what arrived, rounded down to the cent.
        #expect(Engine.fitted(lend, asked: 100, there: Decimal(string: "99.437")!, dollars: true) == .lendDeposit("USDC", Decimal(string: "99.43")!))
        // More than 2 % (and a dollar) short is left for the step to refuse; enough is left as asked.
        #expect(Engine.fitted(lend, asked: 100, there: 97, dollars: true) == nil)
        #expect(Engine.fitted(lend, asked: 100, there: 100, dollars: true) == nil)
        #expect(Engine.fitted(lend, asked: 100, there: 0, dollars: true) == nil)
        // A small move gets a dollar of slack.
        let move = WalletCommand.solanaTo("arbitrum", 5)
        #expect(Engine.fitted(move, asked: 5, there: Decimal(string: "4.2")!, dollars: true) == .solanaTo("arbitrum", Decimal(string: "4.2")!))
        // A token a swap gave: within 2 % only.
        let send = WalletCommand.sendToken(name: "Bybit", token: "USDT", amount: 750, network: "solana")
        let fitted = Engine.fitted(send, asked: 750, there: Decimal(string: "749.8")!, dollars: false)
        #expect(fitted == .sendToken(name: "Bybit", token: "USDT", amount: Decimal(string: "749.8")!, network: "solana"), "\(String(describing: fitted))")
        #expect(Engine.fitted(.sendToken(name: "Bybit", token: "USDT", amount: 10, network: "solana"), asked: 10, there: Decimal(string: "9.5")!, dollars: false) == nil)
        // Where a step's money lands and comes from: what each step waits for.
        #expect(Engine.arrives(.baseToSolana(5)) == "solana")
        #expect(Engine.spends(.lendDeposit("USDC", 5)) == "solana")
        #expect(Engine.source(.bridge(from: "base", to: "arbitrum", 5)) == "base")
        #expect(Engine.solanaUSDC(.kamino(deposit: true, symbol: "USDC", amount: 5)) == 5)
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func aSendToASavedNameIsRefusedBeforeItsCardWhenShort() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        try engine.call("createWallet", ["test": false])
        try engine.call("phraseSaved")
        // Every network read: a read that failed leaves the send to the move (its network would read as empty). Under a
        // loaded run a node may not answer at all, and then there's nothing to check here.
        var read = false
        for _ in 0..<30 where !read {
            // A refresh that took longer than the helper waits (a node not answering under load) is a read that didn't come.
            let portfolio = (try? engine.call("refresh"))?.state?["portfolio"] as? [String: Any]
            read = portfolio?["updatedAt"] != nil && (portfolio?["errors"] as? [String] ?? []).isEmpty
        }
        guard read else { return }
        // Hardhat's public account #3.
        let saved = try engine.call("saveAddress", ["name": "Mom", "network": "base", "address": "0x90F79bf6EB2c4f870365E785982E1f101E93b906"])
        #expect(saved.answer["ok"] as? Bool == true, "\(saved.answer)")

        let sent = try engine.call("send", ["text": "send 100 USDC to Mom"])
        let said = EngineTests.chat(sent.state).last ?? ""
        #expect(sent.state?["card"] == nil || sent.state?["card"] is NSNull, "\(said)")
        #expect(said.contains("less than 100") && said.contains("nothing was prepared"), "\(said)")

        // A chain's first step is read against the balances the same way, before any card; the step after it doesn't run.
        let chained = try engine.call("send", ["text": "send 100 USDC to Mom then chart SOL"])
        let line = EngineTests.chat(chained.state).last ?? ""
        #expect(chained.state?["card"] == nil || chained.state?["card"] is NSNull, "\(line)")
        #expect(line.contains("less than 100"), "\(line)")
    }
}

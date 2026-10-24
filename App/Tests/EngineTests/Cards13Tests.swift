@testable import DesktopEngine
import Foundation
import WalletCore
import IhatecryptoCore
import SignerAPI
import Testing
@testable import TradeExecution

/// Wave 12's M1: before Confirm a card says what the Mac's says — each route's own footer, ⚠ on a lookalike ticker, a
/// multisig's balance before → after, a chain drawn as its first move's route with the rest «then», and the
/// one-transaction note. In-process: this test's own store folder, balances set by hand, nothing sent.
@MainActor @Suite(.serialized) struct Cards13Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards13-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    // Public test data only: Hardhat's addresses, Base's USDC contract.
    static let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    static let phone = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    static let fake = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let baseUSDC = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"

    private var engine: Engine { Self.engine }

    private func wallet(_ networks: [String: [PortfolioView.Row]] = [:], ru: Bool = false) {
        engine.settings.language = ru ? "ru" : "en"
        engine.portfolio = PortfolioView(networks: networks.map { PortfolioView.Network(key: $0.key, name: WalletCommand.placeName($0.key), usd: 0, rows: $0.value) },
                                         updatedAt: Date())
        engine.wayQuotes = nil
        engine.routeWay = nil
        engine.status = nil
    }

    private func usdc(_ x: Decimal) -> PortfolioView.Row { Engine.row("USDC", x, usd: x) }

    @Test func eachRouteHasTheMacsFooter() throws {
        // A buy with the USDC already on Base: Uniswap there, not Relay's ways from Solana.
        wallet(["base": [usdc(20)], "solana": [usdc(50)]])
        #expect(engine.routeNote(.buyOn("base", token: "AERO", usdc: 5)) == "Two Base transactions: an exact approve and the Uniswap swap, which may only pay this wallet. Press Confirm.")
        // A stock by its contract, with the USDG already there.
        wallet(["robinhood": [Engine.row("USDG", 20, usd: 20)]])
        let stock = engine.routeNote(.buyStock(symbol: Self.fake, usdc: 10)) ?? ""
        #expect(stock.hasPrefix("Two signatures on Robinhood Chain") && stock.contains("taken by its contract"), "\(stock)")
        #expect(engine.routeNote(.buyStock(symbol: "TSLA", usdc: 10))?.contains("Robinhood's registered contract") == true)
        #expect(engine.routeNote(.buyStock(symbol: Self.fake, usdc: nil))?.contains("registered") == false)
        // Into NEAR: the 1Click deposit, never «Relay's solver».
        wallet(["base": [usdc(20)]])
        let near = engine.routeNote(.crossSwap(from: "USDC", on: "base", to: "NEAR", at: "near", amount: 5)) ?? ""
        #expect(near.contains("1Click") && !near.contains("Relay"), "\(near)")
        let relay = engine.routeNote(.crossSwap(from: "USDC", on: "base", to: "SOL", at: "solana", amount: 5)) ?? ""
        #expect(relay.contains("Relay's solver") && relay.contains("exact approve to Relay"), "\(relay)")
        // Tempo's own exchange until Relay's swap is the quote taken.
        wallet(["tempo": [usdc(5)]])
        #expect(engine.routeNote(.swapOn("tempo", from: "USDC", to: "pathUSD", amount: 1))?.contains("Tempo's own exchange") == true)
        // A Russian footer for a Russian chat.
        wallet(["base": [usdc(20)]], ru: true)
        #expect(engine.routeNote(.buyOn("base", token: "AERO", usdc: 5))?.hasPrefix("Две транзакции в Base") == true)
    }

    @Test func aPaidServiceNamesThePayeeAndWhereTheMoneyComesFrom() throws {
        let request = #"{"amount":"10000","currency":"0x20c000000000000000000000b9537d11c60e8b50","recipient":"\#(Self.phone)"}"#
        let b64 = Data(request.utf8).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        let challenge = try MPP.challenge(#"Payment id="a1", realm="svc.example", method="tempo", intent="charge", request="\#(b64)""#).get()
        let pay = WalletCommand.pay(method: "GET", url: "https://svc.example/price", body: nil)
        let ends = AddressCheck.ends(challenge.payee)
        wallet(["solana": [usdc(50)], "tempo": [usdc(0)]])
        engine.payment = Engine.PaymentQuote(command: pay, challenge: challenge, ask: "", service: "Example")
        let short = engine.routeNote(pay) ?? ""
        #expect(short.contains("svc.example named (\(ends.start) … \(ends.end))") && short.contains("from Solana over Relay"), "\(short)")
        wallet(["tempo": [usdc(5)]])
        #expect(engine.routeNote(pay)?.contains("from Solana") == false)
        engine.payment = nil
    }

    @Test func aMultisigSendShowsItsBalanceBeforeAndAfter() {
        wallet(["base": [usdc(20)]])
        let keys = [Multisig.Key(name: "This computer", address: Self.me.lowercased(), kind: "pc"), Multisig.Key(name: "Ledger", address: Self.phone.lowercased(), kind: "ledger")]
        let send = WalletCommand.multisigSend(token: "USDC", amount: 5, to: "Mom", network: "base")
        engine.multisigPayment = MultisigPayment(command: send, multisig: Multisig(address: Self.mom.lowercased(), keys: keys, threshold: 2, salt: 1, networks: ["base"]),
                                                 net: "base", to: SignerAddress(name: "Mom", network: "base", address: Self.mom), symbol: "USDC", token: nil,
                                                 amount: 5, units: 5_000_000, threshold: 2, signers: keys, held: Decimal(string: "12.3412")!)
        let money = engine.routeSteps(send)?.last?.money
        #expect(money?.place == "Multisig" && money?.was == "12.34" && money?.after == "7.34", "\(String(describing: money))")
        #expect(engine.routeNote(send)?.contains("money only to Mom") == true)
        engine.multisigPayment = nil
    }

    @Test func aSavedAddressSaysWhenItsYourOwnWalletAndEthereumsGas() {
        wallet()
        var s = SignerReply(ok: true)
        s.wallets = [SignerWallet(id: "1", label: "Main", kind: "phrase", solana: nil, evm: Self.me, near: nil)]
        engine.status = s
        #expect(engine.transferNote(SignerAddress(name: "Main", network: "base", address: Self.me), 5) == "To your own wallet «Main», one transfer on Base. Press Confirm.")
        let eth = engine.transferNote(SignerAddress(name: "Mom", network: "ethereum", address: Self.mom), 5)
        #expect(eth.hasPrefix("Only this saved address may receive") && eth.contains("the fee is in ETH"), "\(eth)")
        engine.status = nil
    }

    @Test func anAddressToSaveSaysWhereItCameFromAndSaves() {
        wallet()
        let entry = SignerAddress(name: "Mom", network: "base", address: Self.mom)
        engine.show(.address(entry), title: "Save “Mom” to Addresses", lines: ["Base: \(Self.mom)"])
        let note = engine.boards.first { $0.id == engine.card?.id }?.note ?? ""
        #expect(note.hasPrefix("Taken from your own message and checked for Base") && engine.card?.confirm == "Save", "\(note)")
        engine.cancelPending()
    }

    @Test func aLookalikeTickerGetsItsWarning() {
        wallet()
        ContractNames.known[Self.fake.lowercased()] = "USDC"
        ContractNames.known[Self.baseUSDC.lowercased()] = "USDC"
        #expect(engine.tokenName(Self.fake, on: "base") == "⚠ USDC (0x3C44…93BC)")
        #expect(engine.tokenName(Self.baseUSDC, on: "base") == "USDC (0x8335…2913)")
        #expect(engine.tokenName("aero", on: "base") == "AERO")
        // The card's title names it so too, never the raw contract.
        wallet(["base": [usdc(20)]])
        let buy = WalletCommand.buyOn("base", token: Self.fake, usdc: 5)
        // Pinned by hand: show() would ask for its quotes.
        let id = engine.pin(Board(id: UUID().uuidString, kind: "move", title: buy.summary, lines: [], steps: [], state: "waiting"))
        engine.noteRoute(id, .wallet(buy))
        let title = engine.boards.first { $0.id == id }?.title ?? ""
        #expect(title.contains("⚠ USDC (0x3C44…93BC)") && !title.contains(Self.fake), "\(title)")
        engine.boards.removeAll { $0.id == id }
    }

    @Test func aChainIsItsFirstMovesRouteThenTheRestPale() {
        wallet(["solana": [usdc(50)], "base": [usdc(0)]])
        let first = WalletCommand.solanaTo("base", 10), buy = WalletCommand.buyOn("base", token: "AERO", usdc: 5)
        let steps = engine.steps(for: .chain([.wallet(first), .wallet(buy)]), title: "", lines: [])
        let head = engine.routeSteps(first) ?? []
        #expect(!head.isEmpty && Array(steps.prefix(head.count)).map(\.label) == head.map(\.label))
        let rest = steps.dropFirst(head.count)
        #expect(!rest.isEmpty && rest.allSatisfy { $0.status == "then" }, "\(rest.map { ($0.label, $0.status ?? "") })")
        // Base has the money by then: no start, bridge or approve of its own.
        #expect(!rest.contains { ["FROM", "BRIDGE", "APPROVE"].contains(where: $0.label.hasPrefix) }, "\(rest.map(\.label))")
        // And it's paid there, by what the move before leaves (the Mac's `ahead`), not by Solana's ways.
        #expect(rest.map(\.label) == ["BUY · UNISWAP ON BASE"], "\(rest.map(\.label))")
        #expect(engine.usdcFree(on: "base") == 0, "the drawing leaves the balances as they are")
        // Under it, the first move's footer, then the chain's own words from its card (never its numbered list).
        let order = "They run in order; the first that fails stops the rest. Windows Hello asks for each move that signs."
        engine.show(.chain([.wallet(first), .wallet(buy)]), title: "2 steps, one Confirm", lines: ["1. move 10 USDC to Base", "2. buy AERO on Base for 5 USDC", order])
        let note = engine.boards.first { $0.id == engine.card?.id }?.note ?? ""
        #expect(note.hasSuffix(order) && !note.contains("1. move") && note.count > order.count, "\(note)")
        engine.cancelPending()
    }

    @Test func theOneTransactionSwitchIsSaidUnderAnApprove() {
        wallet(["base": [usdc(20)]])
        // Settings › Wallet's switch, as the person sets it (mirrored in memory only, Modules.swift).
        engine.settings.batches = true
        defer { engine.settings.batches = nil }
        #expect(Executor.batchesOn)
        let note = engine.routeNote(.buyOn("base", token: "AERO", usdc: 5)) ?? ""
        #expect(note.hasSuffix("go out as one transaction under one Windows Hello."), "\(note)")
        // No approve, no such line.
        #expect(engine.routeNote(.multisigCreate(threshold: 2, of: 2, keys: [], networks: ["base"]))?.contains("as one transaction") == false)
    }
}

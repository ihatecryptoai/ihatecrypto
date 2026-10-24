@testable import DesktopEngine
import Foundation
import WalletCore
import IhatecryptoCore
import EVMChain
import Testing
@testable import TradeExecution

/// Wave 13's cards: an Aave or Morpho deposit's bridge for gas drawn as the executor runs it, a paid request's dollars
/// named from where they really come, a chain's first move quoted and named, «all» of a token by contract read as the
/// number held, and the approvals board drawn before its read is back. In-process: this test's own store folder,
/// balances set by hand, nothing sent.
@MainActor @Suite(.serialized) struct Cards14Tests {
    static let engine: Engine = {
        setEnv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards14-\(UUID().uuidString)").path)
        return Engine(out: Outbox(silent: true))
    }()

    // Public test data only: Hardhat's addresses.
    static let phone = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    static let token = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"

    private var engine: Engine { Self.engine }

    private func wallet(_ networks: [String: [PortfolioView.Row]] = [:]) {
        engine.settings.language = "en"
        engine.portfolio = PortfolioView(networks: networks.map { PortfolioView.Network(key: $0.key, name: WalletCommand.placeName($0.key), usd: 0, rows: $0.value) },
                                         updatedAt: Date())
        engine.wayQuotes = nil
        engine.status = nil
    }

    private func usdc(_ x: Decimal) -> PortfolioView.Row { Engine.row("USDC", x, usd: x) }
    private func eth(_ x: Decimal, on net: String) -> PortfolioView.Row { Engine.row("ETH", x, chain: net, contract: "native") }

    @Test func aDepositWithNoGasThereShowsItsBridgeFromSolana() {
        // The USDC on Arbitrum, no ETH: ensureEVM bridges half a dollar and gas from Solana first, and the card says so.
        wallet(["solana": [usdc(50)], "arbitrum": [usdc(20)]])
        let lend = WalletCommand.aave(deposit: true, amount: 5, network: "arbitrum")
        let dry = engine.routeSteps(lend) ?? []
        #expect(dry.prefix(2).map(\.label) == ["FROM · SOLANA", "BRIDGE · RELAY"], "\(dry.map(\.label))")
        #expect(dry.first?.title == "0.5 USDC" && dry.first?.lines.first?.hasPrefix("20 already on Arbitrum") == true, "\(dry.first.map { [$0.title] + $0.lines } ?? [])")
        #expect(dry[1].lines == ["+ ETH for gas · ≈ 2 s"], "\(dry[1].lines)")
        #expect(engine.routeNote(lend)?.hasPrefix("What's missing on Arbitrum comes from Solana over Relay, with ETH for gas") == true)
        // With the gas there too: the money is already there.
        wallet(["solana": [usdc(50)], "arbitrum": [usdc(20), eth(Decimal(string: "0.01")!, on: "arbitrum")]])
        #expect(engine.routeSteps(lend)?.first?.label == "ON ARBITRUM")
        #expect(engine.routeNote(lend)?.hasPrefix("Two Arbitrum transactions") == true)
        // Base, as the Mac's aaveRoute: what Solana sends, the gas coming along, and its own footer.
        wallet(["solana": [usdc(50)], "base": [usdc(20)]])
        let morpho = WalletCommand.morpho(deposit: true, amount: 5, network: "base")
        let base = engine.routeSteps(morpho) ?? []
        #expect(base.map(\.label) == ["FROM · SOLANA", "BRIDGE · RELAY", "GAS · COMES ALONG", "APPROVE · EXACT", "MORPHO · GAUNTLET USDC PRIME"], "\(base.map(\.label))")
        #expect(base.first?.lines == ["sends ≈ 0.64 USDC"] && base[1].title == "0.5 USDC → Base", "\(base.prefix(2).map { [$0.title] + $0.lines })")
        #expect(engine.routeNote(morpho)?.hasPrefix("One Solana signature for the bridge, then two on Base") == true)
        wallet(["base": [usdc(20), eth(Decimal(string: "0.001")!, on: "base")]])
        #expect(engine.routeSteps(morpho)?.first?.label == "ON BASE")
        #expect(engine.routeNote(morpho)?.hasPrefix("Two on Base") == true)
    }

    @Test func aPaidRequestNamesTheNetworkItsDollarsComeFrom() throws {
        let request = #"{"amount":"10000","currency":"0x20c000000000000000000000b9537d11c60e8b50","recipient":"\#(Self.phone)"}"#
        let b64 = Data(request.utf8).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        let challenge = try MPP.challenge(#"Payment id="a1", realm="svc.example", method="tempo", intent="charge", request="\#(b64)""#).get()
        let pay = WalletCommand.pay(method: "GET", url: "https://svc.example/price", body: nil)
        defer { engine.payment = nil }
        // Solana can't pay and Base can: ensureEVM sends it from Base, a dollar over for Relay.
        wallet(["solana": [usdc(0)], "base": [usdc(20)], "tempo": [usdc(0)]])
        engine.payment = Engine.PaymentQuote(command: pay, challenge: challenge, ask: "", service: "Example")
        let first = engine.routeSteps(pay)?.first
        #expect(first?.label == "FROM · BASE → TEMPO" && first?.lines.last == "20 USDC on Base", "\(first.map { [$0.label, $0.title] + $0.lines } ?? [])")
        #expect(first?.money?.place == "Base", "\(String(describing: first?.money))")
        #expect(engine.routeNote(pay)?.contains("what's short comes from Base over Relay first") == true)
        // Solana holds enough: it pays, as before.
        wallet(["solana": [usdc(50)], "base": [usdc(20)], "tempo": [usdc(0)]])
        #expect(engine.routeSteps(pay)?.first?.label == "FROM · SOLANA → TEMPO")
        #expect(engine.routeNote(pay)?.contains("from Solana over Relay") == true)
    }

    @Test func allOfATokenByContractReadsAsTheNumberHeld() {
        wallet(["base": [Engine.row("AI", Decimal(string: "12.5")!, usd: 3, chain: "base", contract: Self.token.lowercased())]])
        let steps = engine.routeSteps(.sellOn("base", token: Self.token, amount: nil)) ?? []
        let from = steps.first { $0.label.hasPrefix("FROM") }, approve = steps.first { $0.label.hasPrefix("APPROVE") }
        #expect(from?.title == "12.5 AI (0x3C44…93BC)", "\(steps.map { ($0.label, $0.title) })")
        #expect(approve == nil || approve?.title == "12.5 AI (0x3C44…93BC)", "\(String(describing: approve?.title))")
    }

    @Test func aChainIsDrawnAgainWholeWhenItsFirstMoveIsQuoted() {
        wallet(["base": [usdc(20)]])
        let first = WalletCommand.crossSwap(from: "USDC", on: "base", to: "SOL", at: "solana", amount: 5), then = WalletCommand.buyOn("base", token: "AERO", usdc: 2)
        let pending = Pending.chain([.wallet(first), .wallet(then)])
        // Pinned by hand: show() would ask for its quotes.
        let id = engine.pin(Board(id: UUID().uuidString, kind: "move", title: "2 steps", lines: [], steps: engine.steps(for: pending, title: "", lines: []), state: "waiting"))
        engine.pending = pending
        engine.card = Card(id: id, title: "2 steps", confirm: "Confirm")
        defer { engine.pending = nil; engine.card = nil; engine.boards.removeAll { $0.id == id } }
        let quoting = { (engine.boards.first { $0.id == id }?.steps ?? []).contains { s in s.title.contains("quoting…") || s.lines.contains { $0.contains("quoting…") } } }
        #expect(quoting(), "\(engine.boards.first { $0.id == id }?.steps?.map { [$0.title] + $0.lines } ?? [])")
        engine.wayQuotes = (first, [RouteEdit.Way(way: "relay", gets: "≈ 0.031 SOL", worth: "")])
        engine.redrawRoute(id, first)
        let steps = engine.boards.first { $0.id == id }?.steps ?? []
        #expect(!quoting() && steps.contains { $0.status == "then" }, "\(steps.map { ($0.label, $0.title, $0.status ?? "") })")
    }

    @Test func theApprovalsBoardSaysItReadsBeforeTheReadIsBack() {
        wallet()
        let id = engine.readingApprovals()
        defer { engine.boards.removeAll { $0.id == id } }
        #expect(engine.boards.first { $0.id == id }?.data?.approvals?.loading == true)
        engine.fillApprovals(id, [], unread: ["base"])
        let b = engine.boards.first { $0.id == id }
        #expect(b?.data?.approvals?.loading == nil && b?.lines == ["No token approvals."] && b?.data?.approvals?.unread == ["Base"], "\(String(describing: b?.lines))")
    }
}

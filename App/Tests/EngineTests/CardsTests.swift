@testable import DesktopEngine
@testable import EVMChain
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing
@testable import TradeExecution

/// The Mac's route for each move Windows drew as one card (MoveRoutes.swift): each route's steps in order, a quote on the
/// step it names, and the cautions under the card. In-process: the engine's store is this test's own folder, set before
/// anything else reads it; the balances are set by hand, nothing goes to the network.
@MainActor @Suite(.serialized) struct CardsTests {
    static let engine: Engine = {
        setenv("IHATECRYPTO_ENGINE_DIR", FileManager.default.temporaryDirectory.appendingPathComponent("cards-\(UUID().uuidString)").path, 1)
        return Engine(out: Outbox())
    }()

    // Public test data only: Hardhat's addresses, a token's public contract, XRP's account zero.
    static let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906"
    static let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
    static let phone = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    static let aero = "0x940181a94A35A4569E4529A3CDfB74e38FD98631"
    static let xrpZero = "rrrrrrrrrrrrrrrrrrrrrhoLvTp"

    private var engine: Engine { Self.engine }

    /// A fresh wallet: these balances, nothing waiting, English.
    private func wallet(_ networks: [String: [PortfolioView.Row]] = [:], book: [SignerAddress] = []) {
        engine.settings.language = "en"
        engine.portfolio = PortfolioView(networks: networks.map { PortfolioView.Network(key: $0.key, name: WalletCommand.placeName($0.key), usd: 0, rows: $0.value) },
                                         updatedAt: Date())
        engine.book = book
        engine.wayQuotes = nil
        engine.routeWay = nil
        engine.markets = ["xyz:TSLA": Market(name: "xyz:TSLA", markPx: 250, szDecimals: 3, maxLeverage: 10),
                          "NEAR": Market(name: "NEAR", markPx: Decimal(string: "2.5")!, szDecimals: 1, maxLeverage: 10)]
    }

    private func usdc(_ x: Decimal) -> PortfolioView.Row { Engine.row("USDC", x, usd: x) }
    private func labels(_ c: WalletCommand) -> [String] { engine.routeSteps(c)?.map(\.label) ?? [] }
    private func step(_ c: WalletCommand, _ label: String) -> Board.Step? { engine.routeSteps(c)?.first { $0.label == label } }
    private func quoted(_ c: WalletCommand, _ ways: [(String, String, String)]) {
        engine.wayQuotes = (c, ways.map { RouteEdit.Way(way: $0.0, gets: $0.1, worth: $0.2) })
    }

    @Test func aStockBuyBridgesApprovesAndTakesTheBestWay() {
        wallet(["solana": [usdc(50)], "robinhood": [Engine.row("USDG", 0, usd: 0)]])
        let buy = WalletCommand.buyStock(symbol: "TSLA", usdc: 10)
        #expect(labels(buy) == ["FROM · SOLANA", "BRIDGE · RELAY", "APPROVE · EXACT", "BUY · COMPARING"])
        quoted(buy, [("v4", "0.04 TSLA", "$9.95"), ("relay", "0.039 TSLA", "$9.80")])
        let steps = engine.routeSteps(buy) ?? []
        #expect(steps.map(\.label) == ["FROM · SOLANA", "BRIDGE · RELAY", "APPROVE · EXACT", "BUY · UNISWAP V4 · BEST"])
        #expect(steps[2].lines.first?.contains("Permit2") == true, "\(steps[2])")
        #expect(steps[3].lines.first == "≈ 0.04 TSLA · Relay swap ≈ 0.039 TSLA", "\(steps[3])")
        #expect(steps[3].money?.after == "≈ 0.04", "\(String(describing: steps[3].money))")
        // The USDG already there pays: no bridge.
        wallet(["solana": [usdc(50)], "robinhood": [Engine.row("USDG", 10, usd: 10)]])
        #expect(labels(buy) == ["FROM · ROBINHOOD CHAIN", "APPROVE · EXACT", "BUY · COMPARING"])
    }

    @Test func aStockSaleIsApprovedSoldAndLandsAsUSDG() {
        wallet(["robinhood": [Engine.row("TSLA", 1, usd: 250)]])
        let sale = WalletCommand.sellStock(symbol: "TSLA", amount: nil)
        #expect(labels(sale) == ["APPROVE · EXACT", "SELL · UNISWAP", "TO · USDG"])
        #expect(engine.routeSteps(sale)?.last?.title == "≈ 250 USDG")
        quoted(sale, [("v3", "249.1 USDG", "$249.10")])
        #expect(labels(sale) == ["APPROVE · EXACT", "SELL · UNISWAP V3 · BEST", "TO · USDG"])
        #expect(engine.routeSteps(sale)?.last?.title == "≈ 249.1 USDG")
    }

    @Test func anEVMTokenIsBoughtHereOrFromSolanaTheWayThatLeavesMore() {
        let buy = WalletCommand.buyOn("base", token: "AERO", usdc: 5)
        // The USDC on Base pays: approve and Uniswap there.
        wallet(["base": [usdc(20)], "solana": [usdc(50)]])
        #expect(labels(buy) == ["FROM · BASE", "APPROVE · EXACT", "BUY · UNISWAP ON BASE"])
        quoted(buy, [("here", "3.2 AERO", "$4.98")])
        #expect(step(buy, "BUY · UNISWAP ON BASE")?.lines == ["≈ 3.2 AERO"])
        #expect(step(buy, "BUY · UNISWAP ON BASE")?.money?.after == "≈ 3.2")
        // None on Base: from Solana, the two ways compared, and once quoted the better one with its approve.
        wallet(["base": [usdc(0)], "solana": [usdc(50)]])
        #expect(labels(buy) == ["FROM · SOLANA", "RELAY OR UNISWAP · COMPARING", "BUY · ON BASE"])
        quoted(buy, [("uniswap", "3.1 AERO", "$4.90"), ("relay", "3.0 AERO", "$4.80")])
        #expect(labels(buy) == ["FROM · SOLANA", "RELAY + UNISWAP · BEST NOW", "APPROVE · EXACT", "BUY · ON BASE"])
        #expect(step(buy, "RELAY + UNISWAP · BEST NOW")?.lines == ["$0.10 more than Relay swap"])
        #expect(step(buy, "BUY · ON BASE")?.lines == ["≈ 3.1 AERO arrives"])
        // Picked in the step's details: Relay's swap, no approve.
        engine.routeWay = ("base:aero", "relay")
        #expect(labels(buy) == ["FROM · SOLANA", "RELAY SWAP", "BUY · ON BASE"])
        // Sold back to Solana, or for USDC that stays.
        wallet(["base": [usdc(0), Engine.row("AERO", 4, usd: 6, chain: "base", contract: Self.aero, verified: true)]])
        #expect(labels(.sellOn("base", token: "AERO", amount: 2)) == ["FROM · BASE", "RELAY OR UNISWAP · COMPARING", "TO · SOLANA"])
        #expect(labels(.sellOn("base", token: "AERO", amount: 2, keep: true)) == ["FROM · BASE", "APPROVE · EXACT", "TO · USDC ON BASE"])
        #expect(engine.routeSteps(.sellOn("base", token: "AERO", amount: 2))?.first?.money?.after == "2")
    }

    @Test func swapsOffSolanaAndAcrossNetworksHaveTheirRoutes() {
        wallet(["base": [usdc(20), Engine.row("ETH", Decimal(string: "0.01")!, usd: 30, chain: "base", contract: "native")], "solana": [usdc(50)]])
        // The network's own coin goes in as itself; a token is approved exactly first.
        #expect(labels(.swapOn("base", from: "ETH", to: Self.aero, amount: Decimal(string: "0.001")!)) == ["FROM · BASE", "SWAP · UNISWAP ON BASE"])
        let pair = WalletCommand.swapOn("base", from: "USDC", to: "AERO", amount: 5)
        #expect(labels(pair) == ["FROM · BASE", "APPROVE · EXACT", "SWAP · UNISWAP ON BASE"])
        quoted(pair, [("Uniswap v3", "3.2 AERO", "")])
        #expect(labels(pair).last == "SWAP · UNISWAP V3 ON BASE" && step(pair, "SWAP · UNISWAP V3 ON BASE")?.lines == ["≈ 3.2 AERO"])
        #expect(labels(.swapOn("near", from: "NEAR", to: "USDC", amount: 1)) == ["FROM · NEAR", "SWAP · NEAR INTENTS", "TO · NEAR"])
        #expect(labels(.swapOn("hyperliquid", from: "HYPE", to: "PURR", amount: 1)) == ["FROM · SPOT BALANCE", "SELL · SPOT ORDER", "BUY · SPOT ORDER"])
        let tempo = WalletCommand.swapOn("tempo", from: "USDC", to: "PATHUSD", amount: 1)
        #expect(labels(tempo) == ["FROM · TEMPO", "APPROVE · EXACT", "SWAP · TEMPO'S EXCHANGE"])
        quoted(tempo, [("Relay's swap", "0.99 USDT0", "")])
        #expect(labels(tempo).last == "SWAP · RELAY ON TEMPO")
        // Across: Relay's swap, an approve for a token on an EVM network; NEAR's side through NEAR Intents.
        #expect(labels(.crossSwap(from: "USDC", on: "solana", to: "ETH", at: "base", amount: 5)) == ["FROM · SOLANA", "SWAP ACROSS · RELAY", "TO · BASE"])
        #expect(labels(.crossSwap(from: "USDC", on: "base", to: "USDC", at: "arbitrum", amount: 5)) == ["FROM · BASE", "APPROVE · EXACT", "SWAP ACROSS · RELAY", "TO · ARBITRUM"])
        #expect(labels(.crossSwap(from: "NEAR", on: "near", to: "USDC", at: "solana", amount: 1)) == ["FROM · NEAR", "SWAP ACROSS · NEAR INTENTS", "TO · SOLANA"])
        #expect(labels(.revokeApproval("base", token: Self.aero, spender: Self.phone, permit2: false)) == ["TOKEN · BASE", "REVOKE · TO ZERO"])
    }

    @Test func nearMovesAndCoinsSentThroughNEARIntents() throws {
        let friend = SignerAddress(name: "Friend", network: "near", address: String(repeating: "a", count: 64))
        let exchange = SignerAddress(name: "Exchange", network: "xrp", address: Self.xrpZero)
        wallet(["solana": [usdc(50)], "near": [Engine.row("NEAR", 3, usd: Decimal(string: "7.5")!)]], book: [friend, exchange])
        #expect(labels(.buyNear(usdc: 5)) == ["FROM · SOLANA", "SWAP · NEAR INTENTS", "TO · NEAR"])
        #expect(engine.routeSteps(.buyNear(usdc: 5))?.last?.title == "≈ 2 NEAR")
        #expect(labels(.sellNear(amount: 1)) == ["FROM · NEAR", "SWAP · NEAR INTENTS", "ARRIVES · ≈ 30–40 S", "TO · SOLANA"])
        #expect(labels(.sendNear(name: "Friend", amount: Decimal(string: "0.5")!)) == ["FROM · NEAR", "TO · SAVED ADDRESS"])
        #expect(labels(.closeNear(name: "Friend")) == ["FROM · NEAR", "TO · SAVED ADDRESS"])
        #expect(engine.sendCautions(.sendNear(name: "Friend", amount: 1)).first?.hasPrefix("First transfer to this address") == true)

        // XRP to an exchange's address with no destination tag: the card says the money would be lost.
        let xrp = WalletCommand.sendToken(name: "Exchange", token: "XRP", amount: 20)
        #expect(labels(xrp) == ["FROM · SOLANA", "SWAP · NEAR INTENTS", "TO · SAVED ADDRESS"])
        #expect(engine.sendCautions(xrp) == [try #require(engine.xrpTagWarning(exchange))])
        quoted(xrp, [("Arbitrum", "≈ 41.2 USDC for $40.00 of it · ≈ 30 s", "")])
        #expect(labels(xrp).first == "FROM · ARBITRUM" && step(xrp, "FROM · ARBITRUM")?.lines == ["≈ 41.2 USDC for $40.00 of it · ≈ 30 s · the rest comes back"])
        // With its tag saved as the memo: only the first-transfer caution.
        var tagged = exchange
        tagged.memo = "12345"
        wallet(["solana": [usdc(50)]], book: [tagged])
        #expect(engine.sendCautions(xrp).count == 1 && engine.sendCautions(xrp).first?.hasPrefix("First transfer to this address") == true, "\(engine.sendCautions(xrp))")
    }

    @Test func aPaymentAMultisigAndTheBridgesHaveTheirSteps() throws {
        let request = #"{"amount":"10000","currency":"0x20c000000000000000000000b9537d11c60e8b50","recipient":"\#(Self.phone)"}"#
        let b64 = Data(request.utf8).base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
        let challenge = try MPP.challenge(#"Payment id="a1", realm="svc.example", method="tempo", intent="charge", request="\#(b64)""#).get()
        let pay = WalletCommand.pay(method: "GET", url: "https://svc.example/price", body: nil)
        wallet(["solana": [usdc(50)], "tempo": [usdc(0)]])
        engine.payment = Engine.PaymentQuote(command: pay, challenge: challenge, ask: "", service: "Example")
        #expect(labels(pay) == ["FROM · SOLANA → TEMPO", "PAY · MPP", "TO · EXAMPLE"])
        wallet(["tempo": [usdc(5)]])
        #expect(labels(pay) == ["FROM · TEMPO", "PAY · MPP", "TO · EXAMPLE"])
        engine.payment = nil

        let keys = [Multisig.Key(name: "This computer", address: Self.me.lowercased(), kind: "pc"), Multisig.Key(name: "Ledger", address: Self.phone.lowercased(), kind: "ledger")]
        engine.multisigDraft = MultisigDraft(keys: keys, threshold: 2, networks: ["base", "arbitrum"], salt: 1)
        #expect(labels(.multisigCreate(threshold: 2, of: 2, keys: [], networks: ["base", "arbitrum"])) == ["CHECK · KEYS", "MULTISIG · BASE", "MULTISIG · ARBITRUM"])
        engine.multisigDraft = nil
        let send = WalletCommand.multisigSend(token: "USDC", amount: 5, to: "Mom", network: "base")
        engine.multisigPayment = MultisigPayment(command: send, multisig: Multisig(address: Self.mom.lowercased(), keys: keys, threshold: 2, salt: 1, networks: ["base"]),
                                                 net: "base", to: SignerAddress(name: "Mom", network: "base", address: Self.mom), symbol: "USDC", token: nil,
                                                 amount: 5, units: 5_000_000, threshold: 2, signers: keys)
        #expect(labels(send) == ["SIGN · THIS COMPUTER", "SIGN · LEDGER", "TO · MOM"])
        engine.multisigPayment = nil

        wallet(["base": [usdc(20)], "arbitrum": [Engine.row("USDT", 10, usd: 10)]])
        #expect(labels(.cctp(from: "base", to: "arbitrum", 5)) == ["FROM · BASE", "BURN & MINT · CIRCLE'S CCTP", "TO · ARBITRUM"])
        #expect(labels(.usdt(from: "arbitrum", to: "polygon", 5, layerZero: false)) == ["FROM · ARBITRUM", "BRIDGE · RELAY", "TO · POLYGON"])
        let lz = WalletCommand.usdt(from: "arbitrum", to: "polygon", 5, layerZero: true)
        #expect(labels(lz) == ["FROM · ARBITRUM", "BRIDGE · LAYERZERO", "TO · POLYGON"])
        quoted(lz, [("layerzero", "4.99 USDT · $0.01 · ≈ 2 min", "$4.98")])
        #expect(step(lz, "BRIDGE · LAYERZERO")?.title == "4.99 USDT" && engine.routeSteps(lz)?.last?.title == "4.99 USDT")
        // A bridge between EVM networks names the one it goes by once quoted, and who the approve lets take the money.
        let bridge = WalletCommand.bridge(from: "base", to: "arbitrum", 5)
        quoted(bridge, [("cctp", "4.99 USDC · $0.01 · ≈ 1 min", "$4.99"), ("relay", "4.97 USDC · $0.03 · ≈ 4 s", "$4.97")])
        let steps = engine.routeSteps(bridge) ?? []
        #expect(steps.map(\.label) == ["FROM · BASE", "APPROVE · EXACT", "BRIDGE · CCTP", "TO · ARBITRUM"])
        #expect(steps[1].lines.first?.contains("Circle's CCTP") == true && steps[2].lines.first == "CCTP\t$0.01 · ≈ 1 min" && steps[3].title == "4.99 USDC")
    }

    @Test func aaveDebtShowsTheHealthFactorAndTheLiquidationPrice() throws {
        wallet(["base": [usdc(20), Engine.row("ETH", Decimal(string: "0.01")!, usd: 30, chain: "base", contract: "native")]])
        let borrow = WalletCommand.aaveBorrow(5)
        #expect(labels(borrow) == ["COLLATERAL · AAVE", "BORROW · AAVE", "TO · BASE"])
        let ways = [("health", "∞ → 2.1", ""), ("liquidation", "ETH ≤ $1,500", ""), ("collateral", "$20.00", "")]
        quoted(borrow, ways)
        #expect(step(borrow, "BORROW · AAVE")?.lines == ["health factor ∞ → 2.1"] && step(borrow, "COLLATERAL · AAVE")?.title == "$20.00")
        #expect(labels(.aaveRepay(5)) == ["FROM · BASE", "APPROVE · EXACT", "REPAY · AAVE"])
        #expect(labels(.aaveCollateral(Decimal(string: "0.005")!)) == ["FROM · BASE", "WRAP · WETH", "COLLATERAL · AAVE"])

        // The liquidation price is said on the card, and a new quote replaces it.
        engine.show(.wallet(borrow), title: borrow.summary)
        let id = try #require(engine.card?.id)
        #expect(engine.boards.first { $0.id == id }?.steps?.map(\.label) == ["COLLATERAL · AAVE", "BORROW · AAVE", "TO · BASE"])
        engine.liquidationCaution(id, ways.map { RouteEdit.Way(way: $0.0, gets: $0.1, worth: $0.2) })
        engine.liquidationCaution(id, [RouteEdit.Way(way: "liquidation", gets: "ETH ≤ $1,400", worth: "")])
        let said = engine.card?.warnings.filter { $0.hasPrefix("Liquidation if") } ?? []
        #expect(said == ["Liquidation if ETH ≤ $1,400: Aave sells your collateral at a penalty. Keep an eye on it, or borrow less."], "\(said)")
        #expect(engine.boards.first { $0.id == id }?.lines.filter { $0.contains("Liquidation if") }.count == 1)
        engine.cancelPending()
    }

    @Test func rewardsATransferAndACloseAreDrawnStepByStep() {
        wallet(["base": [usdc(2)], "solana": [usdc(50)]])
        let reward = Merkl.Reward(chainId: 8453, token: Self.aero, symbol: "AERO", decimals: 18, amount: 2_000_000_000_000_000_000, claimed: 0,
                                  proofs: [], root: "", price: Decimal(string: "1.5")!)
        #expect(engine.claimSteps([reward], .claimRewards).map(\.label) == ["CLAIM · MERKL ON BASE", "TO · BASE"])
        #expect(engine.claimSteps([reward], .sweepRewards(minUSD: nil, into: nil)).map(\.label) == ["CLAIM · MERKL ON BASE", "TO · BASE", "SELL · ON BASE"])

        let mom = SignerAddress(name: "Mom", network: "base", address: Self.mom)
        let transfer = engine.steps(for: .transfer(mom, 5), title: "Send 5 USDC to Mom", lines: [])
        #expect(transfer.map(\.label) == ["FROM · BASE", "TO · SAVED ADDRESS"])
        #expect(transfer.first?.lines == ["3 USDC short: it comes from Solana first"])

        engine.positions = [HyperliquidAccount.Position(coin: "ETH", size: Decimal(string: "0.5")!, entry: 2000, unrealizedPnl: 25, liquidation: nil)]
        let close = engine.steps(for: .position(.close(1), coin: "ETH"), title: "close ETH", lines: [])
        #expect(close.map(\.label) == ["CLOSE · ETH LONG", "TO · HYPERLIQUID"] && close.first?.title.hasPrefix("+$25") == true, "\(close)")
        #expect(engine.steps(for: .position(.stopToBreakeven, coin: "ETH"), title: "stop", lines: []).map(\.label) == ["HYPERLIQUID"])
        engine.positions = []
    }

    @Test func tokensByContractAndStockTokensSayWhatTheyAre() {
        wallet()
        #expect(engine.sendCautions(.buyOn("base", token: Self.aero, usdc: 5)) == [engine.byContract])
        #expect(engine.sendCautions(.swapOn("base", from: "USDC", to: Self.aero, amount: 5)) == [engine.byContract])
        #expect(engine.sendCautions(.buyStock(symbol: Self.aero, usdc: 5)) == [engine.byContract])
        #expect(engine.sendCautions(.buyStock(symbol: "TSLA", usdc: 5)).first?.hasPrefix("Robinhood Stock Tokens give economic exposure, not the shares") == true)
        // A ticker, a sale, a Solana swap: nothing to say.
        #expect(engine.sendCautions(.buyOn("base", token: "AERO", usdc: 5)).isEmpty && engine.sendCautions(.sellOn("base", token: Self.aero, amount: 1)).isEmpty)
        #expect(engine.sendCautions(.swapOn("solana", from: "USDC", to: "JUP", amount: 5)).isEmpty)
    }

    @Test func kaminosRentOffersJupiterLendBesideSendAnyway() {
        wallet()
        let instead = engine.rentFreeInstead(.wallet(.kamino(deposit: true, symbol: "USDC", amount: 5)))
        #expect(instead.map(\.title) == ["Jupiter Lend instead · no rent"])
        #expect(instead.first.flatMap { WalletCommand.parse($0.command) } == .lendDeposit("USDC", 5))
        #expect(engine.rentFreeInstead(.wallet(.lendDeposit("USDC", 5))).isEmpty)
    }

    @Test func theRoutesSpeakRussian() {
        wallet(["base": [usdc(20)]])
        engine.settings.language = "ru"
        #expect(labels(.cctp(from: "base", to: "arbitrum", 5)) == ["ОТКУДА · BASE", "СЖЕЧЬ И ВЫПУСТИТЬ · CCTP CIRCLE", "КУДА · ARBITRUM"])
        #expect(engine.sendCautions(.buyOn("base", token: Self.aero, usdc: 5)).first?.hasPrefix("Токен по контракту") == true)
        engine.settings.language = "en"
    }

    @Test func aQuotesAmountIsReadFromOurOwnWording() {
        #expect(Engine.quotedAmount("≈ 2.41 AERO") == Decimal(string: "2.41"))
        #expect(Engine.quotedAmount("1,234.5 USDC · 0.3% impact") == Decimal(string: "1234.5"))
        #expect(Engine.quotedAmount(nil) == nil && Engine.quotedAmount("quoting…") == nil)
    }
}

import Foundation
@testable import IhatecryptoCore
import Testing

// The app is in English; the parsers read English as well as Russian. Other languages go through the model,
// which rewrites them into these same English forms (ChatRouter.forms).

@Test func englishTradePhrases() throws {
    let a = try #require(IntentParser.apply("long ETH risk $10 stop 3% take 6% leverage 2x", to: TradeIntent()))
    #expect(a.side == .long && a.asset == "ETH" && a.riskUSD == 10 && a.stopPct == 3 && a.takePct == 6 && a.leverage == 2)

    let b = try #require(IntentParser.apply("short oil on a 2% pullback by the book", to: TradeIntent(), markets: ["BTC", "xyz:CL"]))
    #expect(b.side == .short && b.asset == "xyz:CL" && b.pullbackPct == 2 && b.autoRules == true)

    let c = try #require(IntentParser.apply("long BTC take 2R take in parts trailing 2%", to: TradeIntent(), markets: ["BTC"]))
    #expect(c.takeR == 2 && c.partialTake == true && c.trailingPct == 2)
}

@Test func englishPositionCommands() {
    #expect(PositionCommand.parse("move stop to breakeven") == .stopToBreakeven)
    #expect(PositionCommand.parse("stop at 93.4") == .stopTo(Decimal(string: "93.4")!))
    #expect(PositionCommand.parse("take at 88") == .takeTo(88))
    #expect(PositionCommand.parse("close half") == .close(Decimal(string: "0.5")!))
    #expect(PositionCommand.parse("close") == .close(1))
    #expect(PositionCommand.parse("cancel the limit order") == .cancelOrders)
    #expect(PositionCommand.parse("take closer") == nil)
}

@Test func englishWalletCommands() {
    let sol = SpotAsset.all[0], btc = SpotAsset.all[1]
    #expect(WalletCommand.parse("withdraw 1 USDC to Solana") == .withdrawToSolana(1))
    #expect(WalletCommand.parse("lend 1 USDC") == .lendDeposit("USDC", 1))
    #expect(WalletCommand.parse("withdraw USDC from the vault") == .lendWithdraw("USDC", nil))
    #expect(WalletCommand.parse("deposit 50 USDC to kamino") == .kamino(deposit: true, symbol: "USDC", amount: 50))
    #expect(WalletCommand.parse("stake 0.01 SOL") == .stake(Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("unstake 0.005 JitoSOL") == .unstake(Decimal(string: "0.005")!))
    #expect(WalletCommand.parse("where to keep USDC") == .yields("USDC"))
    #expect(WalletCommand.parse("deposit 5 USDC to HLP") == .hlp(deposit: true, amount: 5))
    #expect(WalletCommand.parse("spread USDC across the best rates, max 70% per protocol") == .allocate("USDC", maxShare: 70))
    // A limit: at most that much of the free USDC.
    #expect(WalletCommand.parse("spread 2 USDC across the best rates") == .allocate("USDC", maxShare: nil, amount: 2))
    #expect(WalletCommand.parse("разложи 2 USDC по лучшим ставкам, не больше 70% в один") == .allocate("USDC", maxShare: 70, amount: 2))
    #expect(WalletCommand.parse("spread 0.5 SOL across the best rates") == .allocate("SOL", maxShare: nil, amount: Decimal(string: "0.5")))
    // «а без kamino можно?»: a spread leaves protocols out, and its summary reads back the same.
    let without = WalletCommand.allocate("USDC", maxShare: 34, amount: 900, without: ["kamino", "aave"])
    #expect(WalletCommand.parse("spread 900 USDC across the best rates max 34% per protocol without kamino, aave") == without)
    #expect(WalletCommand.parse(without.summary) == without)
    #expect(WalletCommand.parse("разложи 900 USDC по лучшим ставкам без камино") == .allocate("USDC", maxShare: nil, amount: 900, without: ["kamino"]))
    #expect(WalletCommand.parse("spread 2 USDC across the best rates")?.summary == "spread 2 USDC across the best rates")
    #expect(WalletCommand.parse("buy SOL for 2 USDC") == .buy(sol, usdc: 2))
    #expect(WalletCommand.parse("buy SOL for 2 USDC every week") == .schedule(sol, usdc: 2, days: 7))
    #expect(WalletCommand.parse("buy BTC for 5 USDC every day") == .schedule(btc, usdc: 5, days: 1))
    #expect(WalletCommand.parse("stop recurring buys") == .stopSchedules)
    #expect(WalletCommand.parse("sell 0.1 SOL") == .sell(sol, amount: Decimal(string: "0.1")!))
    #expect(WalletCommand.parse("купи неар на 3 бакса") == .buyNear(usdc: 3))
    #expect(WalletCommand.parse("buy NEAR for 5 USDC") == .buyNear(usdc: 5))
    #expect(WalletCommand.parse("купи NEAR") == .buyNear(usdc: nil))
    #expect(WalletCommand.parse("buy 2 NEAR") == .buyNear(usdc: nil))   // coins, not dollars: asks
    #expect(WalletCommand.parse("buy SOL near 150") != .buyNear(usdc: nil))
    #expect(WalletCommand.parse("buy ETH near 2500 for 3 USDC") == nil || WalletCommand.parse("buy ETH near 2500 for 3 USDC") != .buyNear(usdc: 3))
    // Russian keeps working.
    #expect(WalletCommand.parse("выведи 1 USDC в Solana") == .withdrawToSolana(1))
}

@Test func everyWithdrawalTakesAnAmount() {
    // «выведи 3 USDC из камино» drew «only everything can come out»: the forms said no N.
    #expect(WalletCommand.parse("withdraw 3 USDC from kamino") == .kamino(deposit: false, symbol: "USDC", amount: 3))
    #expect(WalletCommand.parse("выведи 3 USDC из камино") == .kamino(deposit: false, symbol: "USDC", amount: 3))
    #expect(WalletCommand.parse("withdraw 3 USDC from HLP") == .hlp(deposit: false, amount: 3))
    #expect(WalletCommand.parse("withdraw 3 USDC from Aave on Arbitrum") == .aave(deposit: false, amount: 3, network: "arbitrum"))
    #expect(WalletCommand.parse("withdraw 3 USDC from Morpho") == .morpho(deposit: false, amount: 3, network: "base"))
    #expect(WalletCommand.parse("withdraw 3 USDC from the vault") == .lendWithdraw("USDC", 3))
    for form in ["withdraw [N] <USDC|SOL> from kamino", "withdraw [N] USDC from HLP", "withdraw [N] USDC from Aave", "withdraw [N] USDC from Morpho"] {
        #expect(ChatRouter.forms.contains(form), "\(form)")
    }
}

/// The classifier's second reading gets only the sections of the forms its command is written in.
@Test func aCommandsOwnSectionsOfTheForms() {
    let lend = ChatRouter.forms(for: "lend 15 USDC")
    #expect(lend.contains("EARNING:") && lend.contains("Jupiter Lend") && !lend.contains("TRADE PLAN"))
    #expect(ChatRouter.forms(for: "long oil 5 USDC").hasPrefix("TRADE PLAN"))
    // Every step of a chain, and what a schedule runs.
    let chain = ChatRouter.forms(for: "withdraw 10 USDC to Solana then deposit 10 USDC to kamino")
    #expect(chain.contains("EARNING:") && chain.contains("MOVING MONEY"))
    #expect(ChatRouter.forms(for: "in 1 week: rebalance USDC").contains("rebalance"))
    // A verb no form starts with: all of them, rather than nothing to judge by.
    #expect(ChatRouter.forms(for: "frobnicate 5 USDC") == ChatRouter.forms)
}

@Test func routerFormsCoverTheParsers() {
    // Every form the model is told to write must parse locally, in the order the app tries the parsers.
    let ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", markets: Set<String> = ["BTC", "ETH", "SOL", "xyz:CL", "xyz:TSLA"]
    let commands = ["withdraw 1 USDC to Solana", "lend 1 USDC", "stake 0.1 SOL", "where to keep SOL", "stop recurring buys",
                    "buy token \(ai) for 2 USDC", "sell 3 tokens \(ai)", "buy Tesla stock for 3 USDC", "sell TSLA stock",
                    "move USDG from Robinhood to Solana", "buy NEAR for 5 USDC", "top up", "deposit 5 USDC to HLP",
                    "lend 2 USDC on Aave", "deposit 2 USDC into Morpho", "withdraw USDC from Morpho",
                    "spread USDC across the best rates max 50% per protocol", "rebalance USDC max 50% per protocol",
                    "move 3 USDC from Solana to Robinhood", "move 2 USDC from Solana to Base", "move 1 USDC from Solana to Arbitrum"]
    for command in commands { #expect(WalletCommand.parse(command) != nil, "\(command)") }
    #expect(WalletCommand.parse("sell NEAR") == .sellNear(amount: nil))
    #expect(WalletCommand.parse("sell 0.5 NEAR") == .sellNear(amount: Decimal(string: "0.5")!))
    #expect(WalletCommand.parse("продай неар") == .sellNear(amount: nil))
    #expect(WalletCommand.parse("продай 0,3 неар") == .sellNear(amount: Decimal(string: "0.3")!))
    #expect(WalletCommand.parse("sell ETH near 2500") != .sellNear(amount: nil))
    #expect(ChatRouter.forms.contains("sell [N] NEAR"))
    #expect(TokenCommand.parse("watch token \(ai) on robinhood") == .favorite(address: ai, chain: "robinhood"))
    #expect(TokenCommand.parse("remove token AI from favorites") == .unfavorite(symbol: "AI"))
    #expect(ChartCommand.parse("chart ETH week", markets: markets) == .chart(coin: "ETH", range: .week))
    #expect(ChartCommand.parse("what's moving") == .movers)
    #expect(AlertCommand.parse("alert me when ETH is above 4000", markets: markets) != nil)
    #expect(PositionCommand.parse("close half") == .close(Decimal(string: "0.5")!))
    #expect(IntentParser.apply("short oil risk $1 by the book", to: TradeIntent(), markets: markets)?.asset == "xyz:CL")
    #expect(ChatRouter.forms.contains("buy token <0xCONTRACT> for N USDC"))
    #expect(PnLCommand.parse("profit and loss week for oil", markets: markets) == PnLCommand(period: .week, coin: "xyz:CL"))
    // Native NEAR and SOL to saved addresses, and closing the NEAR account.
    #expect(WalletCommand.parse("send 0.05 NEAR to Bybit") == .sendNear(name: "Bybit", amount: Decimal(string: "0.05")!))
    #expect(WalletCommand.parse("send all NEAR to Bybit") == .sendNear(name: "Bybit", amount: nil))
    #expect(WalletCommand.parse("close the NEAR account into Bybit") == .closeNear(name: "Bybit"))
    #expect(WalletCommand.parse("send 0.001 SOL to Old wallet") == .sendSol(name: "Old wallet", amount: Decimal(string: "0.001")!))
    #expect(ChatRouter.forms.contains("send N SOL to <NAME from the address book>"))
}

@Test func nearAndSolTransfersParse() {
    #expect(WalletCommand.parse("отправь 0.05 NEAR на Bybit") == .sendNear(name: "Bybit", amount: Decimal(string: "0.05")!))
    #expect(WalletCommand.parse("отправь весь NEAR на Мой кошелёк") == .sendNear(name: "Мой кошелёк", amount: nil))
    #expect(WalletCommand.parse("закрой NEAR-аккаунт и переведи всё на Bybit") == .closeNear(name: "Bybit"))
    #expect(WalletCommand.parse("отправь 0,00002 SOL на старый кошелёк") == .sendSol(name: "старый кошелёк", amount: Decimal(string: "0.00002")!))
    // Never guessed: no number and no «all» is not a NEAR transfer; SOL into a vault is a deposit.
    #expect(WalletCommand.parse("send NEAR to Bybit") != .sendNear(name: "Bybit", amount: nil))
    #expect(WalletCommand.parse("send 1 SOL to the vault") != .sendSol(name: "the vault", amount: 1))
    // The other NEAR phrases keep working.
    #expect(WalletCommand.parse("продай весь NEAR") == .sellNear(amount: nil))
    #expect(WalletCommand.parse("send 5 USDC to Bybit") == .sendTo(name: "Bybit", amount: 5))
    // A NEAR account in a save command, and the router never inventing one.
    let implicit = String(repeating: "ab", count: 32)
    #expect(AddressBookCommand.parse("save \(implicit) as Bybit on near") == .save(address: implicit, name: "Bybit", network: "near"))
    #expect(AddressBookCommand.parse("save alice.near as Alice") == .save(address: "alice.near", name: "Alice", network: "near"))
    #expect(!ChatRouter.addressesAllowed("save \(implicit) as X on near", userText: "сохрани кошелёк", known: []))
}

@Test func movesKeepTheirDirection() {
    // «из соланы» is out of Solana: it must never become a withdrawal to Solana (the bug in the user's screenshot).
    #expect(WalletCommand.parse("переведи 3 USDC из соланы в робингуд") == .solanaTo("robinhood", 3))
    #expect(WalletCommand.parse("move 3 USDC from Solana to Robinhood") == .solanaTo("robinhood", 3))
    #expect(WalletCommand.parse("закинь 2 USDC с соланы на base") == .solanaTo("base", 2))
    #expect(WalletCommand.parse("move USDC from Solana to Arbitrum") == .solanaTo("arbitrum", nil))
    #expect(WalletCommand.parse("выведи 1 USDC в Solana") == .withdrawToSolana(1))
    #expect(WalletCommand.parse("move 2 USDC from Base to Solana") == .baseToSolana(2))
    #expect(WalletCommand.parse("верни USDG с робингуда на солану") == .robinhoodToSolana(nil))
    #expect(WalletCommand.solanaTo("robinhood", nil).amountQuestion != nil)
}

@Test func routerNeverTakesAnAddressFromTheModel() throws {
    let ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", other = "0x1111111111111111111111111111111111111111"
    #expect(ChatRouter.addressesAllowed("buy token \(ai) for 2 USDC", userText: "купи \(ai) на 2$", known: []))
    #expect(ChatRouter.addressesAllowed("buy token \(ai) for 2 USDC", userText: "купи AI на 2$", known: [ai.lowercased()]))
    #expect(!ChatRouter.addressesAllowed("buy token \(other) for 2 USDC", userText: "купи AI на 2$", known: [ai]))
    #expect(ChatRouter.addressesAllowed("long ETH risk $1", userText: "лонг эфир", known: []))
    // A 0x one only whole: never the first 40 hex of a longer one they wrote, nor a longer one round their 40.
    let hash = "0x9f2c4e1ab3d57e8f0c1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70", sui = "0xa1b2c3d4e5f60718293a4b5c6d7e8f9011223344556677889900aabbccddeeff"
    #expect(ChatRouter.addresses("tx \(hash)?") == [hash])
    #expect(!ChatRouter.addressesAllowed("buy token \(hash.prefix(42)) for 2 USDC", userText: "что с транзакцией \(hash)? купи этот токен на 2$", known: []))
    #expect(!ChatRouter.addressesAllowed("save \(sui.prefix(42))000000000000000000000000 as Bob on sui", userText: "сохрани \(sui) как Bob на sui", known: []))
    #expect(ChatRouter.addressesAllowed("save \(sui) as Bob on sui", userText: "сохрани \(sui) как Bob на sui", known: []))
    let vault = "0x1A996cb54bb95462040408C06122D45D6Cdb6096", to = "0x000000000000000000000000" + "aaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbb"
    let said = "call base \(vault) depositForBurn(uint256,uint32,bytes32,address) 1000000 6 \(to) \(other)"
    #expect(ChatRouter.addressesAllowed(said, userText: said, known: []))
    #expect(!ChatRouter.addressesAllowed(said.replacingOccurrences(of: "bbbbbbbbbbbbbbbbbbbbbbbb", with: "cccccccccccccccccccccccc"), userText: said, known: []))
    #expect(!ChatRouter.addressesAllowed("buy token \(ai)a for 2 USDC", userText: "купи \(ai) на 2$", known: []))
    #expect(!ChatRouter.addressesAllowed("buy token \(ai) for 2 USDC", userText: "купи \(ai)a на 2$", known: []))
    #expect(ChatRouter.addressesAllowed("buy token \(ai) for 2 USDC", userText: "купи https://basescan.org/token/\(ai)#code на 2$", known: []))
    #expect(try ChatRouter.decode(Data(#"{"route":"command","command":"lend 1 USDC"}"#.utf8)) == .command("lend 1 USDC"))
    #expect(try ChatRouter.decode(Data(#"{"route":"chat","reply":"Hi!"}"#.utf8)) == .chat("Hi!"))
    #expect(try ChatRouter.decode(Data(#"{"route":"question"}"#.utf8)) == .question)   // an older reply still reads
    #expect(throws: (any Error).self) { try ChatRouter.decode(Data(#"{"route":"command","command":""}"#.utf8)) }
}

@Test func takeProfitChoices() {
    #expect(TakeStyle.asksForChoice("поставь тейк, как советуют лучшие инвесторы"))
    #expect(TakeStyle.asksForChoice("set the take profit like the best investors do"))
    #expect(TakeStyle.asksForChoice("take profit options"))
    #expect(!TakeStyle.asksForChoice("long ETH take 6%"))
    var plan = TradeIntent(); plan.takePct = 6
    let partial = TakeStyle.partial.apply(to: plan)
    #expect(partial.takePct == nil && partial.takeR == 2 && partial.partialTake == true && partial.trailingPct == nil)
    let trend = TakeStyle.trend.apply(to: plan)
    #expect(trend.trailingPct == 2 && trend.partialTake == true)
    #expect(TakeStyle.classic.payoff(risk: Decimal(string: "0.25")!) == "+$0.50")
    #expect(TakeStyle.partial.payoff(risk: Decimal(string: "0.25")!) == "+$0.38")
}

@Test func weakParsesGoToTheModel() {
    #expect(IntentParser.hasTradeWords("long ETH risk $10"))
    #expect(IntentParser.hasTradeWords("шорт нефти как учат"))
    #expect(IntentParser.hasTradeWords("SOL 3x"))
    #expect(!IntentParser.hasTradeWords("compra SOL por 1 USDC"))
    #expect(!IntentParser.hasTradeWords("kaufe ETH für 5 USDC"))
}

@Test func yieldsForBothAssets() {
    #expect(WalletCommand.parse("where to keep USDC and SOL") == .yields(nil))
    #expect(WalletCommand.parse("где лучше держать SOL") == .yields("SOL"))
}

@Test func namedAssetForPositionCommands() {
    let markets: Set<String> = ["BTC", "ETH", "xyz:CL"]
    #expect(IntentParser.namedAsset("close ETH", markets: markets) == "ETH")
    #expect(IntentParser.namedAsset("закрой нефть", markets: markets) == "xyz:CL")
    #expect(IntentParser.namedAsset("close", markets: markets) == nil)
    #expect(IntentParser.namedAsset("close half", markets: markets) == nil)
}

@Test func questionsAreNotOrders() {
    #expect(IntentParser.isQuestion("а где наш шорт по нефти?"))
    #expect(IntentParser.isQuestion("what happened to my ETH long"))
    #expect(IntentParser.isQuestion("сколько я заработал на нефти"))
    #expect(!IntentParser.isQuestion("шорт нефти как учат"))
    #expect(!IntentParser.isQuestion("long ETH risk $10"))
    #expect(!IntentParser.isQuestion("поставь тейк как советуют лучшие инвесторы"))
}

@Test func everydayWordsAreNotMarkets() {
    let markets: Set<String> = ["ETH", "SOL", "MOVE", "NOT", "xyz:NOW", "xyz:CL"]
    #expect(IntentParser.namedAsset("what is open right now?", markets: markets) == nil)
    #expect(IntentParser.namedAsset("move stop to breakeven", markets: markets) == nil)
    #expect(IntentParser.namedAsset("close NOW", markets: markets) == "xyz:NOW")
    #expect(IntentParser.namedAsset("long sol risk $5", markets: markets) == "SOL")
    #expect(IntentParser.namedAsset("short MOVE", markets: markets) == "MOVE")
    #expect(IntentParser.namedAsset("а где наш шорт по нефти?", markets: markets) == "xyz:CL")
}

@Test func trailingAnOpenPosition() {
    #expect(PositionCommand.parse("trail 2%") == .trail(2))
    #expect(PositionCommand.parse("трейлинг стоп 1,5%") == .trail(Decimal(string: "1.5")!))
    #expect(PositionCommand.parse("long ETH by the book trailing 2%") == nil)
}

@Test func aaveCommands() {
    #expect(WalletCommand.parse("lend 2 USDC on Aave") == .aave(deposit: true, amount: 2))
    #expect(WalletCommand.parse("положи 1,5 usdc в aave на base") == .aave(deposit: true, amount: Decimal(string: "1.5")!))
    #expect(WalletCommand.parse("buy USDC on Base and put it into Aave") == .aave(deposit: true, amount: nil))
    #expect(WalletCommand.parse("withdraw USDC from Aave") == .aave(deposit: false, amount: nil))
    // Rewards to collect, not a withdrawal or the rates: «claim … from Aave», «claim yield».
    for text in ["claim rewards", "claim my rewards from Morpho", "claim rewards from Aave", "claim yield", "забери награды", "собери мои награды", "клейм наград", "получи бонусы"] {
        #expect(WalletCommand.parse(text) == .claimRewards, "\(text)")
    }
    #expect(WalletCommand.parse("сними 1 usdc из aave") != .claimRewards)
    #expect(WalletCommand.parse("сними 1 usdc из aave") == .aave(deposit: false, amount: 1))
    #expect(WalletCommand.parse("what do I have on Aave?") == nil)
    #expect(WalletCommand.parse("move USDC from Base to Solana") == .baseToSolana(nil))
    #expect(WalletCommand.parse("верни 1 usdc с base в солану") == .baseToSolana(1))
    #expect(WalletCommand.parse("withdraw 3 USDC to Solana") == .withdrawToSolana(3))
}

@Test func addressBookFromChat() {
    let evm = "0x742d35Cc6634C0532925a3b844Bc454e4438f44e", sol = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    #expect(AddressBookCommand.parse("save \(evm) as Bybit on Arbitrum") == .save(address: evm, name: "Bybit", network: "arbitrum"))
    #expect(AddressBookCommand.parse("сохрани \(sol) как Binance депозит") == .save(address: sol, name: "Binance депозит", network: nil))
    #expect(AddressBookCommand.parse("remember \(evm) as “Mom” on base.") == .save(address: evm, name: "Mom", network: "base"))
    #expect(AddressBookCommand.parse("add Bybit to favorites") == .favorite(name: "Bybit", on: true))
    #expect(AddressBookCommand.parse("добавь bybit в избранное") == .favorite(name: "bybit", on: true))
    #expect(AddressBookCommand.parse("remove Mom from my favourites") == .favorite(name: "Mom", on: false))
    // Without a name nothing is saved; an address alone or a send is not this command.
    #expect(AddressBookCommand.parse("save \(evm)") == nil)
    #expect(AddressBookCommand.parse("send 5 USDC to \(evm)") == nil)
    #expect(AddressBookCommand.parse("long ETH risk $5") == nil)
}

@Test func priceAlerts() {
    let m: Set<String> = ["ETH", "BTC", "SOL", "xyz:CL"]
    #expect(AlertCommand.parse("alert me when ETH is above 2,800", markets: m) == .set(coin: "ETH", price: 2800, above: true))
    #expect(AlertCommand.parse("скажи когда нефть упадёт ниже 90,5", markets: m) == .set(coin: "xyz:CL", price: Decimal(string: "90.5")!, above: false))
    #expect(AlertCommand.parse("tell me when BTC hits 70000", markets: m) == .set(coin: "BTC", price: 70000, above: nil))
    #expect(AlertCommand.parse("cancel all alerts", markets: m) == .clear)
    #expect(AlertCommand.parse("long ETH risk $5 stop 2%", markets: m) == nil)
    #expect(AlertCommand.parse("notify me about ETH", markets: m) == nil)
    let a = PriceAlert(coin: "ETH", above: false, price: 2600)
    #expect(a.fires(at: 2599) && a.fires(at: 2600) && !a.fires(at: 2601))
    // With steps after the colon: the level before it, and the numbers of the steps don't become the level.
    #expect(AlertCommand.parse("when ETH is below 2,680: long ETH risk $0.28 stop 2.1% take 4.2% leverage 3x", markets: m)
            == .set(coin: "ETH", price: 2680, above: false, steps: ["long ETH risk $0.28 stop 2.1% take 4.2% leverage 3x"]))
    #expect(AlertCommand.parse("when SOL is above 250: sell SOL then lend USDC", markets: m)
            == .set(coin: "SOL", price: 250, above: true, steps: ["sell SOL", "lend USDC"]))
    #expect(AlertCommand.parse("when my portfolio grows: long ETH", markets: m) == nil)
    #expect(ChatRouter.parses("when ETH is below 2680: long ETH leverage 3x", markets: m))
    // Alerts kept before steps existed still load.
    let old = try! JSONDecoder().decode(PriceAlert.self, from: Data(#"{"id":"\#(UUID())","coin":"ETH","above":true,"price":1,"created":0}"#.utf8))
    #expect(old.steps == nil)
}

@Test func sendToSavedNames() {
    #expect(WalletCommand.parse("send 5 USDC to Bybit") == .sendTo(name: "Bybit", amount: 5))
    #expect(WalletCommand.parse("отправь 2,5 usdc на Binance депозит") == .sendTo(name: "Binance депозит", amount: Decimal(string: "2.5")!))
    #expect(WalletCommand.parse("send 3 USDC to Solana") == .withdrawToSolana(3))
    #expect(WalletCommand.parse("send 0.5 USDC to My Base wallet") == .sendTo(name: "My Base wallet", amount: Decimal(string: "0.5")!))
    #expect(WalletCommand.parse("send 1 USDC to 0x742d35Cc6634C0532925a3b844Bc454e4438f44e") != .sendTo(name: "0x742d35Cc6634C0532925a3b844Bc454e4438f44e", amount: 1))
}

@Test func morphoCommandsReadInBothLanguages() {
    #expect(WalletCommand.parse("deposit 2 USDC into Morpho") == .morpho(deposit: true, amount: 2))
    #expect(WalletCommand.parse("положи 2 USDC в морфо") == .morpho(deposit: true, amount: 2))
    #expect(WalletCommand.parse("withdraw USDC from Morpho") == .morpho(deposit: false, amount: nil))
    #expect(WalletCommand.parse("выведи из морфо") == .morpho(deposit: false, amount: nil))
    #expect(WalletCommand.parse("lend 2 USDC on Aave") == .aave(deposit: true, amount: 2))
}

@Test func lendingCommandsNameTheirNetwork() {
    #expect(WalletCommand.parse("lend 2 USDC on Aave on Arbitrum") == .aave(deposit: true, amount: 2, network: "arbitrum"))
    #expect(WalletCommand.parse("положи 3 usdc в aave на полигоне") == .aave(deposit: true, amount: 3, network: "polygon"))
    #expect(WalletCommand.parse("deposit 50 USDC into Morpho on Ethereum") == .morpho(deposit: true, amount: 50, network: "ethereum"))
    #expect(WalletCommand.parse("выведи всё из morpho на арбитруме") == .morpho(deposit: false, amount: nil, network: "arbitrum"))
    // No network named: Base, as before.
    #expect(WalletCommand.parse("lend 2 USDC on Aave") == .aave(deposit: true, amount: 2, network: "base"))
    // From Solana into Aave on a network is a deposit there, not an idle transfer.
    #expect(WalletCommand.parse("переведи 2 usdc из соланы в aave на арбитрум") == .aave(deposit: true, amount: 2, network: "arbitrum"))
    // Idle USDC to and from the new networks.
    #expect(WalletCommand.parse("move 3 USDC from Solana to Polygon") == .solanaTo("polygon", 3))
    #expect(WalletCommand.parse("move 5 USDC from Polygon to Solana") == .evmToSolana("polygon", 5))
    #expect(WalletCommand.parse("верни usdc с эфириума в солану") == .evmToSolana("ethereum", nil))
    #expect(WalletCommand.aave(deposit: true, amount: nil, network: "polygon").amountQuestion == "How much USDC into Aave on Polygon?")
    #expect(WalletCommand.morpho(deposit: false, amount: 1, network: "ethereum").summary == "withdraw 1 USDC from Morpho on Ethereum to Solana")
}

@Test func sendToOnTheNetworkNamed() {
    #expect(WalletCommand.parse("send 2 USDC to main wallet on ethereum") == .sendTo(name: "main wallet", amount: 2, network: "ethereum"))
    #expect(WalletCommand.parse("переведи 2 USDC на main wallet по эфир сети") == .sendTo(name: "main wallet", amount: 2, network: "ethereum"))
    #expect(WalletCommand.parse("отправь 5 usdc на Маму в сети полигон") == .sendTo(name: "Маму", amount: 5, network: "polygon"))
    // A network inside the name stays in it.
    #expect(WalletCommand.parse("send 0.5 USDC to My Base wallet") == .sendTo(name: "My Base wallet", amount: Decimal(string: "0.5")!))
}

/// A vault deposit funded from another EVM network runs as these two steps (Workspace.fundedFromElsewhere): both must read back.
@Test func aVaultDepositFundedFromAnotherNetworkReadsBack() {
    for c in [WalletCommand.aave(deposit: true, amount: 300, network: "arbitrum"), .morpho(deposit: true, amount: 200, network: "base"),
              .morpho(deposit: true, amount: 50, network: "ethereum"), .lendDeposit("USDC", 500), .kamino(deposit: true, symbol: "USDC", amount: 500)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
    #expect(WalletCommand.parse("move 301 USDC from Ethereum to Arbitrum") == .bridge(from: "ethereum", to: "arbitrum", 301))
    #expect(ChatRouter.steps("move 301 USDC from Ethereum to Arbitrum then deposit 300 USDC into Aave on Arbitrum")?.count == 2)
    #expect(WalletCommand.parse("move 501 USDC from Ethereum to Solana") == .evmToSolana("ethereum", 501))
}

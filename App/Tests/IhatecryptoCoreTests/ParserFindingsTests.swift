import Foundation
@testable import IhatecryptoCore
import Testing

/// Phrases a review found read wrong (C2–C13, M4, R1–R9), and the normal forms beside them that must keep reading the same.
private func d(_ s: String) -> Decimal { Decimal(string: s)! }
private let sol = SpotAsset.all[0]

@Test func thousandsAndLeadingDotsReadAsWritten() {   // C4
    #expect(PositionCommand.parse("move stop to 2,500") == .stopTo(2500))
    #expect(PositionCommand.parse("stop at 2500") == .stopTo(2500))
    #expect(PositionCommand.parse("take profit at 3,100.5") == .takeTo(d("3100.5")))
    #expect(PositionCommand.parse("закрой 33,333%") == .close(d("0.33333")))
    #expect(WalletCommand.parse("send 1,500 USDC to Mom") == .sendTo(name: "Mom", amount: 1500))
    #expect(WalletCommand.parse("move 1,234.56 USDC from Base to Solana") == .baseToSolana(d("1234.56")))
    #expect(WalletCommand.parse("deposit .5 USDC to kamino") == .kamino(deposit: true, symbol: "USDC", amount: d("0.5")))
    let plan = IntentParser.apply("long ETH 1,000 USDC limit 2,650", to: TradeIntent())
    #expect(plan?.amountUSDC == 1000 && plan?.entryPx == 2650)
    #expect(IntentParser.apply("long ETH risk $1,000", to: TradeIntent())?.riskUSD == 1000)
    // A decimal comma stays one.
    #expect(WalletCommand.parse("deposit 1,5 USDC to kamino") == .kamino(deposit: true, symbol: "USDC", amount: d("1.5")))
    #expect(WalletCommand.parse("положи 0,500 SOL в хранилище") == .lendDeposit("SOL", d("0.5")))
    #expect(WalletCommand.parse("buy SOL for 5 USDC") == .buy(sol, usdc: 5))
}

@Test func solOnSolanaToANameIsASendNotAWithdrawal() {   // C2
    #expect(WalletCommand.parse("send 0.5 SOL on solana to New wallet") == .sendSol(name: "New wallet", amount: d("0.5")))
    #expect(WalletCommand.parse("send 0.5 SOL on solana to Bybit") == .sendSol(name: "Bybit", amount: d("0.5")))
    #expect(WalletCommand.parse("отправь 0.5 SOL в солане на Bybit") == .sendSol(name: "Bybit", amount: d("0.5")))
    #expect(WalletCommand.parse("send 0.01 SOL to Bybit") == .sendSol(name: "Bybit", amount: d("0.01")))
    // The catch-all wants a word for taking money out, or Solana as where it goes.
    #expect(WalletCommand.parse("send SOL on solana to New wallet") == nil)
    #expect(WalletCommand.parse("send 3 USDC to Solana") == .withdrawToSolana(3))
    #expect(WalletCommand.parse("withdraw 3 USDC to Solana") == .withdrawToSolana(3))
    #expect(WalletCommand.parse("верни всё в солану") == .withdrawToSolana(nil))
    #expect(WalletCommand.parse("move 2 USDC from Hyperliquid to Solana") == .withdrawToSolana(2))
}

@Test func aSaleInDollarsIsNeverSellAll() {   // C3
    let m: Set<String> = ["BTC", "ETH", "SOL"]
    for text in ["продай сол на 10 баксов", "sell SOL for 10 USDC", "sell BONK on solana for 10 USDC", "sell DEGEN on base for 10 USDC",
                 "sell TSLA stock for 10 USDC", "sell NEAR for 10 USDC", "sell 0x742d35Cc6634C0532925a3b844Bc454e4438f44e for $5"] {
        #expect(WalletCommand.parse(text) == nil, "\(text)")
    }
    // Unparsed all the way: the model is asked again, and no long of 10 USDC is drawn.
    #expect(IntentParser.apply("sell SOL for 10 USDC", to: TradeIntent(), markets: m) == nil)
    #expect(!ChatRouter.parses("sell SOL for 10 USDC", markets: m))
    #expect(WalletCommand.parse("sell 2 TSLA stock for 10 USDC") == .sellStock(symbol: "TSLA", amount: 2))
    #expect(WalletCommand.parse("sell TSLA stock") == .sellStock(symbol: "TSLA", amount: nil))
    #expect(WalletCommand.parse("sell SOL") == .sell(sol, amount: nil))
    #expect(WalletCommand.parse("sell 0.1 SOL") == .sell(sol, amount: d("0.1")))
    #expect(WalletCommand.parse("sell all DEGEN on base") == .sellOn("base", token: "DEGEN", amount: nil))
    #expect(WalletCommand.parse("sell 1000 BONK on solana") == .sell(.bySymbol("BONK"), amount: 1000))
    #expect(IntentParser.apply("short ETH risk $5", to: TradeIntent(), markets: m)?.side == .short)
}

@Test func aNamedCountIsNeverAll() {
    let c = "0x742d35Cc6634C0532925a3b844Bc454e4438f44e", m = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    let m7 = "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr"   // a mint that starts with a digit
    // A bare count is the count, English and Russian: without «tokens» the sale was all of it.
    #expect(WalletCommand.parse("sell 100 \(c)") == .sellStock(symbol: c, amount: 100))
    #expect(WalletCommand.parse("продай 100 \(c)") == .sellStock(symbol: c, amount: 100))
    #expect(WalletCommand.parse("продай 1,5 \(c)") == .sellStock(symbol: c, amount: d("1.5")))
    #expect(WalletCommand.parse("sell 1,000 \(c)") == .sellStock(symbol: c, amount: 1000))
    #expect(WalletCommand.parse("sell .5 \(c)") == .sellStock(symbol: c, amount: d("0.5")))
    #expect(WalletCommand.parse("sell my 100 \(c)") == .sellStock(symbol: c, amount: 100))
    #expect(WalletCommand.parse("dump 100 \(c)") == .sellStock(symbol: c, amount: 100))
    #expect(WalletCommand.parse("sell 1500 \(m)") == .sell(.byMint(m), amount: 1500))
    #expect(WalletCommand.parse("продай 1500 \(m)") == .sell(.byMint(m), amount: 1500))
    #expect(WalletCommand.parse("sell 3 \(m7)") == .sell(.byMint(m7), amount: 3))
    #expect(WalletCommand.parse("продай 2 теслы") == .sellStock(symbol: "TSLA", amount: 2))
    // The forms the chat writes keep reading the same.
    #expect(WalletCommand.parse("sell 100 tokens \(c)") == .sellStock(symbol: c, amount: 100))
    #expect(WalletCommand.parse("продай 100 штук \(c)") == .sellStock(symbol: c, amount: 100))
    #expect(WalletCommand.parse("sell 1500 tokens \(m)") == .sell(.byMint(m), amount: 1500))
    // A number or a share it can't place: refused, never «all».
    for t in ["sell \(c) 100", "sell 50% of \(c)", "продай половину \(c)", "sell half of \(m)", "sell 1k \(c)", "продай \(m) 3",
              "sell \(m7) 3", "sell \(c) for 10 USDC", "sell $10 of \(c)",
              // Dollars after a space, numbers it would read in part, percents and shares in words, numbers in words.
              "sell 100 $ of \(c)", "продай 100 $ \(c)", "sell 100 $ worth of \(m)", "sell 100 $ of tesla stock",
              "продай 1 000 \(c)", "продай 1\u{a0}000 \(c)", "sell 1 000 tokens \(c)", "sell 1 500 TSLA stock",
              "продай 5 тысяч \(c)", "sell 1 million \(c)", "продай 2 млн \(m)", "sell 1/2 of \(c)",
              "sell 50 percent of \(c)", "продай 50 процентов \(c)", "продай 50 процентов теслы",
              "sell a quarter of \(c)", "sell a third of \(m)", "продай треть \(c)", "продай четверть \(m)", "продай часть \(c)",
              "sell ten \(c)", "продай десять \(c)"] {
        #expect(WalletCommand.parse(t) == nil, "\(t)")
    }
    // «All» said, or no number at all: still all.
    #expect(WalletCommand.parse("sell \(c)") == .sellStock(symbol: c, amount: nil))
    #expect(WalletCommand.parse("sell all \(c)") == .sellStock(symbol: c, amount: nil))
    #expect(WalletCommand.parse("продай весь \(c)") == .sellStock(symbol: c, amount: nil))
    #expect(WalletCommand.parse("sell all \(m)") == .sell(.byMint(m), amount: nil))
    #expect(WalletCommand.parse("sell tokens \(m)") == .sell(.byMint(m), amount: nil))
    #expect(WalletCommand.parse("sell \(m7)") == .sell(.byMint(m7), amount: nil))
    #expect(WalletCommand.parse("sell TSLA stock") == .sellStock(symbol: "TSLA", amount: nil))
}

@Test func hiddenAmountsAreDotsButPercentagesStay() {
    #expect(Fmt.dots("$1,234.56") == "•••")
    #expect(Fmt.dots("−$1.20 · +4.2%") == "••• · +4.2%")
    #expect(Fmt.dots("0.1583 · gas") == "••• · gas")
    #expect(Fmt.dots("worth $2.39 now · earning 5.97%") == "worth ••• now · earning 5.97%")
    #expect(Fmt.dots("no price") == "no price")
}

@Test func aBuyInDollarsIsNeverALong() {
    let m: Set<String> = ["BTC", "ETH", "SOL"]
    // The model left out «on solana»: the plan's parser took 750 USDC as a trade's margin and drew a 5× ETH long.
    #expect(IntentParser.apply("buy USDT for 750 USDC", to: TradeIntent(), markets: m) == nil)
    #expect(IntentParser.apply("купи USDT на 750 баксов", to: TradeIntent(), markets: m) == nil)
    #expect(!ChatRouter.parses("buy USDT for 750 USDC then send all USDT on solana to Bybit USDT", markets: m))
    #expect(ChatRouter.parses("buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit USDT", markets: m))
    #expect(WalletCommand.parse("send 750 USDT on solana to Bybit USDT")?.with(amount: 749.8) == .sendToken(name: "Bybit USDT", token: "USDT", amount: 749.8, network: "solana"))
    // A trade still says so.
    #expect(IntentParser.apply("long ETH 750 USDC", to: TradeIntent(), markets: m)?.amountUSDC == 750)
    // The network's own coin bought or sold for USDC is the swap the forms have for it: Uniswap's list has no ETH.
    #expect(WalletCommand.parse("buy ETH on arbitrum for 10 USDC") == .swapOn("arbitrum", from: "USDC", to: "ETH", amount: 10))
    #expect(WalletCommand.parse("sell 0.01 ETH on base") == .swapOn("base", from: "ETH", to: "USDC", amount: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("buy POL on polygon for 2 USDC") == .swapOn("polygon", from: "USDC", to: "POL", amount: 2))
    #expect(WalletCommand.parse("buy ETH on bnb for 5 USDC") == .buyOn("bnb", token: "ETH", usdc: 5, from: nil))   // a token there
    #expect(WalletCommand.parse("buy ETH on base") == .buyOn("base", token: "ETH", usdc: nil, from: nil))          // asks how much
    // Jupiter Lend takes USDT both ways, as the brief now says.
    #expect(WalletCommand.parse("lend 100 USDT") == .lendDeposit("USDT", 100))
    #expect(WalletCommand.parse("withdraw 100 USDT from the vault") == .lendWithdraw("USDT", 100))
    // Whatever else slips into the plan's parser, the second reader is told what the code makes of it.
    #expect(ChatRouter.planReading("get USDT with 750 USDC", plan: nil, markets: m).contains("becomes: long ETH 750 USDC"))
    #expect(ChatRouter.planReading("buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit", plan: nil, markets: m).isEmpty)
    // What the app reads before the plan is never described as one: a note naming SOL, a position's order.
    #expect(ChatRouter.planReading("write to folder UITest: вернуться к SOL в пятницу", plan: nil, markets: m).isEmpty)
    #expect(ChatRouter.planReading("close ETH", plan: nil, markets: m).isEmpty)
    #expect(ChatRouter.planReading("long SOL 5 USDC", plan: nil, markets: m).contains("becomes: long SOL 5 USDC"))
}

@Test func aQuestionOrADontNeverClosesThePosition() {   // C5
    for text in ["не закрывай позицию", "don't close it", "should I close ETH?", "закрыть?", "what if I close half?", "do not move the stop to 2500"] {
        #expect(PositionCommand.parse(text) == nil, "\(text)")
    }
    #expect(PositionCommand.parse("закрой половину") == .close(d("0.5")))
    #expect(PositionCommand.parse("close ETH") == .close(1))
    #expect(PositionCommand.parse("стоп в безубыток") == .stopToBreakeven)
    #expect(PositionCommand.parse("отмени ордер") == .cancelOrders)
}

@Test func aQuestionOrANegationIsNoYesOrNo() {   // C6, R4
    for text in ["ok?", "да?", "confirm?", "sure?", "let it go", "да ну", "да ну его", "да не", "no don't", "¿sí?"] {
        #expect(!ChatWords.confirms(text), "\(text)")
    }
    for text in ["не отменяй", "don't cancel", "отменить?", "не отменяй, давай"] { #expect(!ChatWords.declines(text), "\(text)") }
    for text in ["да", "давай", "do it", "yes please", "ok", "ну давай", "let's go"] { #expect(ChatWords.confirms(text), "\(text)") }
    for text in ["нет", "не надо", "не, передумал", "cancel", "never mind"] { #expect(ChatWords.declines(text), "\(text)") }
}

@Test func anAnswerInAnotherUnitDoesNotFillTheQuestion() {   // C8, R1, R2, R3
    #expect(WalletCommand.amountAnswer("$5", for: .stake(nil)) == nil)
    #expect(WalletCommand.amountAnswer("на 5 долларов", for: .stake(nil)) == nil)
    #expect(WalletCommand.amountAnswer("$0.05", for: .stake(nil)) == nil)
    #expect(WalletCommand.amountAnswer("5 usdc", for: .kamino(deposit: true, symbol: "SOL", amount: nil)) == nil)
    #expect(WalletCommand.amountAnswer("1 sol", for: .lendDeposit("USDC", nil)) == nil)
    #expect(WalletCommand.amountAnswer("$5 sol", for: .stake(nil)) == nil)
    #expect(WalletCommand.amountAnswer("0.0000001", for: .lendDeposit("USDC", nil)) == nil)
    #expect(WalletCommand.amountAnswer("0.0000000001", for: .stake(nil)) == nil)
    #expect(WalletCommand.amountAnswer("1,000", for: .lendDeposit("USDC", nil)) == 1000)
    #expect(WalletCommand.amountAnswer("5", for: .stake(nil)) == 5)
    #expect(WalletCommand.amountAnswer("0.5 SOL", for: .stake(nil)) == d("0.5"))
    #expect(WalletCommand.amountAnswer("0.000000001", for: .stake(nil)) == d("0.000000001"))
    #expect(WalletCommand.amountAnswer("0.5 сол", for: .lendDeposit("SOL", nil)) == d("0.5"))
    #expect(WalletCommand.amountAnswer("$5", for: .buy(sol, usdc: nil)) == 5)
    #expect(WalletCommand.amountAnswer("да, 2", for: .aave(deposit: true, amount: nil)) == 2)
    #expect(WalletCommand.amountAnswer("10 usdc", for: .solanaTo("base", nil)) == 10)
    #expect(WalletCommand.amountAnswer("5 баксов", for: .hlp(deposit: true, amount: nil)) == 5)
    // The old reader, which the app still calls, reads numbers the same way.
    #expect(WalletCommand.amountAnswer("1,5 USDC") == d("1.5"))
    #expect(WalletCommand.amountAnswer("1,000") == 1000)
    #expect(WalletCommand.amountAnswer(".5") == d("0.5"))
}

@Test func eachPercentGoesToItsOwnWord() {   // C12
    let a = IntentParser.apply("long ETH by the book on a 2% pullback trailing 3%", to: TradeIntent())
    #expect(a?.pullbackPct == 2 && a?.trailingPct == 3)
    let b = IntentParser.apply("long ETH take in parts on a 2% pullback", to: TradeIntent())
    #expect(b?.takePct == nil && b?.pullbackPct == 2 && b?.partialTake == true)
    let c = IntentParser.apply("long ETH risk $1 stop 2% pullback 3%", to: TradeIntent())
    #expect(c?.stopPct == 2 && c?.pullbackPct == 3)
    #expect(IntentParser.apply("тейк на 6%", to: TradeIntent())?.takePct == 6)
    #expect(IntentParser.apply("take profit 6%", to: TradeIntent())?.takePct == 6)
    #expect(IntentParser.apply("тейк-профит 4%", to: TradeIntent())?.takePct == 4)
    #expect(IntentParser.apply("вход на откате 2%", to: TradeIntent())?.pullbackPct == 2)
    #expect(IntentParser.apply("pullback of 1.5%", to: TradeIntent())?.pullbackPct == d("1.5"))
    // The router form reads back to the same plan.
    var i = TradeIntent()
    i.asset = "SOL"; i.riskUSD = 1; i.stopPct = 2; i.pullbackPct = 3; i.trailingPct = 1; i.partialTake = true
    #expect(IntentParser.apply(i.command, to: TradeIntent(), markets: ["SOL"]) == i)
}

@Test func theNetworkAfterFromIsWhereMoneyLeaves() {   // C13
    #expect(WalletCommand.parse("переведи 5 USDC с базы на солану") == .baseToSolana(5))
    #expect(WalletCommand.parse("переведи 5 USDC с бейза на солану") == .baseToSolana(5))
    #expect(WalletCommand.parse("move 5 USDC to Base from Solana") == nil)
    #expect(WalletCommand.parse("move 5 USDC to Robinhood from Solana") == nil)
    #expect(WalletCommand.parse("move 5 USDC to Arbitrum from Solana") == nil)
    #expect(WalletCommand.parse("move 5 USDC from Base to Solana") == .baseToSolana(5))
    #expect(WalletCommand.parse("move 5 USDC to Solana from Base") == .baseToSolana(5))
    #expect(WalletCommand.parse("move 5 USDC from Solana to Base") == .solanaTo("base", 5))
    #expect(WalletCommand.parse("верни USDG с робингуда на солану") == .robinhoodToSolana(nil))
}

@Test func everyZeroDaysIsNoSchedule() {   // M4
    #expect(WalletCommand.parse("buy SOL for 5 USDC every 0 days") == nil)
    #expect(WalletCommand.parse("покупай SOL на 5 USDC каждые 0 дней") == nil)
    #expect(!ChatRouter.parses("buy SOL for 5 USDC every 0 days", markets: ["SOL"]))
    #expect(WalletCommand.parse("buy SOL for 5 USDC every 3 days") == .schedule(sol, usdc: 5, days: 3))
    // One stored before the parser refused it moves a day at a time instead of spinning forever.
    let start = Date(timeIntervalSince1970: 1_800_000_000)
    var s = Schedule(symbol: "SOL", usdc: 5, days: 0, next: start)
    s.advance(now: start)
    #expect(s.next == start.addingTimeInterval(86_400))
}

@Test func aBareAmountDoesNotSwitchTheLanguage() {   // R9
    #expect(ChatLanguage.auto.isRussian(recentUserMessages: ["положи USDC в jupiter", "1 sol"]))
    #expect(ChatLanguage.auto.isRussian(recentUserMessages: ["положи USDC в jupiter", "10 usdc"]))
    #expect(ChatLanguage.auto.isRussian(recentUserMessages: ["купи BONK", "0.5 BONK"]))
    #expect(ChatLanguage.auto.name(recentUserMessages: ["застейкай сол", "$5"]) == "Russian")
    #expect(!ChatLanguage.auto.isRussian(recentUserMessages: ["deposit USDC to jupiter", "1 sol"]))
    #expect(!ChatLanguage.auto.isRussian(recentUserMessages: ["положи USDC в jupiter", "buy 5 SOL"]))
}

/// «Продай 2 акции теслы» sells 2: the form the model writes has a place for the count, and no count is all of it.
@Test func aStockSaleKeepsItsCount() {
    #expect(ChatRouter.forms.contains("sell [N] <TICKER> stock"))
    #expect(WalletCommand.parse("sell 2 TSLA stock") == .sellStock(symbol: "TSLA", amount: 2))
    #expect(WalletCommand.parse("sell 0.5 SPCX stock") == .sellStock(symbol: "SPCX", amount: d("0.5")))
    #expect(WalletCommand.parse("sell 2 Tesla shares") == .sellStock(symbol: "TSLA", amount: 2))
    #expect(WalletCommand.parse("продай 2 акции теслы") == .sellStock(symbol: "TSLA", amount: 2))
    #expect(WalletCommand.parse("продай 0,5 акции SpaceX") == .sellStock(symbol: "SPCX", amount: d("0.5")))
    #expect(WalletCommand.parse("sell TSLA stock") == .sellStock(symbol: "TSLA", amount: nil))
    #expect(WalletCommand.parse("продай все акции теслы") == .sellStock(symbol: "TSLA", amount: nil))
    // A sale in dollars has no form: never read as that many tokens, which the cap at what's held made all of them.
    for t in ["sell $50 of TSLA stock", "sell $50 worth of TSLA stock", "sell 50 worth of Tesla shares", "продай акции теслы на $50",
              "продай на 50 долларов акций теслы", "sell 100 worth of 0x742d35Cc6634C0532925a3b844Bc454e4438f44e"] {
        #expect(WalletCommand.parse(t) == nil, "\(t)")
    }
}

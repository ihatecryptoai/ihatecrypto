import Foundation
@testable import IhatecryptoCore
import LLMCore
import Testing

// The chat as an agent: the model looks things up, then answers with a block to show and cards to go on.

@Test func agentStepsReadLeniently() throws {
    #expect(try ChatRouter.step(Data(#"{"route":"lookup","lookups":[{"tool":"Token","arg":" B5AN "},{"tool":"portfolio"}]}"#.utf8))
            == .lookups([.init("token", "B5AN"), .init("portfolio")]))
    let done = try ChatRouter.step(Data(#"""
    {"route":"chat","reply":"Горячий мемкоин.","show":"chart token B5AN on solana",
     "options":[{"title":"Следить","comment":"цена в избранном","command":"watch token B5AN on solana","recommended":"true"},
                {"title":"","command":"chart ETH"},{"title":"No command"},{"title":"Keep it","command":"new canvas"},{"title":"Rates","command":"where to keep USDC"}]}
    """#.utf8))
    guard case .done(let r) = done else { Issue.record("not an answer"); return }
    #expect(r.route == .chat("Горячий мемкоин."))
    #expect(r.show == "chart token B5AN on solana")
    #expect(r.options == [.init(title: "Следить", comment: "цена в избранном", command: "watch token B5AN on solana", recommended: true),
                          .init(title: "Rates", comment: "", command: "where to keep USDC")])
    #expect(throws: (any Error).self) { try ChatRouter.step(Data(#"{"route":"lookup","lookups":[]}"#.utf8)) }
    // A chart offered as a card opens with the answer.
    let promoted = try ChatRouter.step(Data(#"{"route":"chat","reply":"Hot.","options":[{"title":"Watch","command":"watch token B5AN"},{"title":"Chart","command":"chart token B5AN on solana"}]}"#.utf8))
    #expect(promoted == .done(ChatRouter.Reading(route: .chat("Hot."), show: "chart token B5AN on solana",
                                                 options: [.init(title: "Watch", comment: "", command: "watch token B5AN")])))
}

// A question's answers: one with a command does it, one without goes back to the model as the reply — never one with
// an address, since a picked answer is read as the user's own words. A chat answer's cards all need a command.
@Test func aQuestionsAnswersMayGoBackToTheModel() throws {
    let asked = try ChatRouter.step(Data(#"""
    {"route":"clarify","reply":"На какой срок деньги свободны?",
     "options":[{"title":"Месяц","comment":"можно в волт с локом"},{"title":"Нужны в любой момент","command":"lend 100 USDC","recommended":true},
                {"title":"На 0x1111111111111111111111111111111111111111"}]}
    """#.utf8))
    guard case .done(let r) = asked else { Issue.record("not an answer"); return }
    #expect(r.options == [.init(title: "Месяц", comment: "можно в волт с локом", command: ""),
                          .init(title: "Нужны в любой момент", comment: "", command: "lend 100 USDC", recommended: true)])
}

@Test func onlyTheFirstObjectOfAReplyCounts() {
    let reply = #"{"route":"lookup","lookups":[{"tool":"token","arg":"a{b}"}]}{"route":"chat","reply":"a guess"}"#
    #expect(ChatRouter.firstObject(in: "ok: " + reply) == #"{"route":"lookup","lookups":[{"tool":"token","arg":"a{b}"}]}"#)
    #expect(ChatRouter.firstObject(in: "no json") == nil)
}

/// A scripted model: its replies in order, and what it was sent.
private final class Script: LLMClient, @unchecked Sendable {
    var replies: [String]
    var sent: [[LLMMessage]] = []
    init(_ replies: [String]) { self.replies = replies }
    func complete(_ request: LLMRequest) async throws -> String {
        sent.append(request.messages)
        return replies.removeFirst()
    }
}

@Test func theAgentLooksUpThenAnswers() async throws {
    let model = Script([
        #"{"route":"lookup","lookups":[{"tool":"token","arg":"2FB5"}]}"#,
        #"{"route":"chat","reply":"POKEMON, hot.","show":"chart token B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6 on solana"}"#,
    ])
    let reading = try await ChatRouter.route("расскажи про токен 2FB5", context: .init(), client: model, model: "m") { l in
        "Result for \(l.tool): mint B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    }
    #expect(reading.route == .chat("POKEMON, hot."))
    #expect(reading.lookups == [.init("token", "2FB5")])
    #expect(reading.found == ["B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"])
    // The second call carries the model's lookup and its result.
    #expect(model.sent[1].count == 4)
    #expect(model.sent[1][3].text.contains("mint B5AN"))
}

@Test func theAgentStopsLookingAfterItsRounds() async {
    let model = Script(Array(repeating: #"{"route":"lookup","lookups":[{"tool":"portfolio"}]}"#, count: ChatRouter.rounds + 1))
    await #expect(throws: (any Error).self) { try await ChatRouter.route("?", context: .init(), client: model, model: "m") }
    #expect(model.sent.last?.last?.text.contains("No more lookups") == true)
}

@Test func stepsRunNowAndEachMustParse() {
    let m: Set<String> = ["ETH", "SOL"]
    #expect(ChatRouter.steps("withdraw 5 USDC to Solana then deposit 5 USDC to kamino") == ["withdraw 5 USDC to Solana", "deposit 5 USDC to kamino"])
    #expect(ChatRouter.steps("lend 1 USDC") == nil)
    #expect(ChatRouter.steps("in 1 week: sell SOL then lend USDC") == nil)   // a scheduled task keeps its steps
    #expect(ChatRouter.parses("withdraw 5 USDC to Solana Then deposit 5 USDC to kamino", markets: m))
    #expect(!ChatRouter.parses("withdraw 5 USDC to Solana then write me a trading bot", markets: m))
    #expect(ChatRouter.unparsed("lend 1 USDC then write me a trading bot", markets: m) == "write me a trading bot")
    #expect(ChatRouter.forms.contains("COMMAND then COMMAND"))
}

@Test func aCommandNoParserReadsGoesBackToTheModel() async throws {
    let model = Script([
        #"{"route":"command","command":"write me a trading bot"}"#,
        #"{"route":"clarify","reply":"Бота ihatecrypto не пишет."}"#,
    ])
    let reading = try await ChatRouter.route("напиши бота", context: .init(), client: model, model: "m", markets: ["ETH"])
    #expect(reading.route == .clarify("Бота ihatecrypto не пишет."))
    #expect(model.sent[1].last?.text.contains("no form reads it") == true)
    // Without the markets nothing is checked: the command comes back as the model wrote it.
    let unchecked = try await ChatRouter.route("напиши бота", context: .init(), client: Script([#"{"route":"command","command":"write me a trading bot"}"#]), model: "m")
    #expect(unchecked.route == .command("write me a trading bot"))
}

@Test func tokenCardsAndWatchingParse() {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    #expect(TokenCommand.parse("chart token \(mint) on solana") == .show(address: mint, chain: "solana"))
    #expect(TokenCommand.parse("show token 0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18 on robinhood")
            == .show(address: "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", chain: "robinhood"))
    #expect(TokenCommand.parse("watch token \(mint) on solana") == .favorite(address: mint, chain: "solana"))
    #expect(ChatRouter.forms.contains("chart token <CONTRACT or MINT> on"))
    #expect(ChatRouter.parses("chart token \(mint) on solana", markets: []))
    #expect(ChatRouter.parses("lend 5 USDC", markets: []))
    #expect(!ChatRouter.parses("write me a trading bot", markets: []))
}

@Test func aMintCopiedFromALinkIsRepaired() {
    // «%2F» of a link copied along with the mint.
    #expect(TokenResearch.repairs("2FB5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6").first == "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6")
    #expect(TokenResearch.repairs("B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6") == ["B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"])
    #expect(TokenResearch.repairs("tooshort").isEmpty)
}

@Test func jupiterFactsSayWhatMatters() throws {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    let json = """
    [{"id": "\(mint)", "name": "POKÉMON CARDS", "symbol": "POKEMON", "usdPrice": 0.00036, "mcap": 360198, "fdv": 360198, "liquidity": 43138,
      "holderCount": 1359, "launchpad": "stonkfun", "createdAt": "2026-09-06T04:36:17Z", "graduatedAt": "2026-09-06T04:46:19Z",
      "stats1h": {"priceChange": 158.9}, "stats24h": {"priceChange": 88.2, "buyVolume": 101421, "sellVolume": 89812, "buyOrganicVolume": 7626,
      "sellOrganicVolume": 11995, "numBuys": 681, "numSells": 460, "numTraders": 266},
      "audit": {"mintAuthorityDisabled": true, "freezeAuthorityDisabled": false, "topHoldersPercentage": 21.19, "devBalancePercentage": 1.5},
      "organicScoreLabel": "low", "tags": ["unknown", "token-2022"]}]
    """
    let now = ISO8601DateFormatter().date(from: "2026-09-29T04:36:17Z")!
    let facts = try #require(TokenResearch.jupiterFacts(Data(json.utf8), mint: mint, now: now))
    #expect(facts.contains("POKÉMON CARDS (POKEMON), mint \(mint), chain id solana"))
    #expect(facts.contains("not verified"))
    #expect(facts.contains("1h +158.9%"))
    #expect(facts.contains("organic (real users, not bots) $19.6K"))
    #expect(facts.contains("Created 23 days ago on the stonkfun launchpad"))
    #expect(facts.contains("mint authority off"))
    #expect(facts.contains("FREEZE AUTHORITY ON"))
    #expect(TokenResearch.jupiterFacts(Data(json.utf8), mint: "other", now: now) == nil)
}

@Test func theTokenLookupCarriesTheMonthTheCardDraws() throws {
    // 180 four-hour closes from Sep 4 00:00 UTC: 0.60 falling to 0.40 by the middle, back up to 0.44.
    let start = 1_788_480_000.0
    let points = (0..<180).map { (i: Int) -> TokenMarket.Point in
        let close: Double = i < 90 ? 0.60 - Double(i) * 0.2 / 89 : 0.40 + Double(i - 90) * 0.04 / 89
        return TokenMarket.Point(time: Date(timeIntervalSince1970: start + Double(i) * 14_400), close: Decimal(close))
    }
    let utc = TimeZone(identifier: "UTC")!
    let facts = try #require(TokenResearch.chartFacts(points, range: .month, of: "The pool", timeZone: utc) as String?)
    #expect(facts.hasPrefix("The pool over 30 days: $0.6 → $0.44 (-26.7%)"))
    #expect(facts.contains("low $0.4 on Sep 18"))
    #expect(facts.contains("high $0.6 on Sep 4"))
    // A close a day, the last one the price now.
    let closes = facts.components(separatedBy: "Closes: ")[1].components(separatedBy: ", ")
    #expect(closes.count == 30 && closes.last == "Oct 3 $0.44.")
    #expect(TokenResearch.chartFacts(Array(points.prefix(96)), range: .day, of: "It", timeZone: utc)?.contains("low $0.4 on 20:00") == true)
    #expect(TokenResearch.chartFacts([], range: .month, of: "The pool") == nil)
}

@Test func aSearchPutsTheDeepestPoolFirst() throws {
    let json = """
    {"pairs": [
      {"chainId": "base", "baseToken": {"address": "0xfake", "symbol": "BONK", "name": "Bonk"}, "priceUsd": "0.1", "liquidity": {"usd": 5000}},
      {"chainId": "solana", "baseToken": {"address": "DezX", "symbol": "Bonk", "name": "Bonk"}, "priceUsd": "0.00002", "liquidity": {"usd": 400000},
       "priceChange": {"h24": -3.2}},
      {"chainId": "solana", "baseToken": {"address": "DezX", "symbol": "Bonk", "name": "Bonk"}, "priceUsd": "0.00002", "liquidity": {"usd": 90000}}
    ]}
    """
    let (facts, top) = try #require(TokenResearch.searchFacts(Data(json.utf8), query: "BONK"))
    #expect(top == "DezX")
    let lines = facts.components(separatedBy: "\n")
    #expect(lines.count == 3)
    #expect(lines[1].hasPrefix("Bonk (Bonk) on chain id solana, contract DezX"))
    #expect(lines[1].contains("liquidity $400K, 24h -3.2%"))
    #expect(lines[2].contains("0xfake"))
}

@Test func anySolanaTokenByItsMint() {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    #expect(WalletCommand.parse("buy token \(mint) for 2 USDC") == .buy(.byMint(mint), usdc: 2))
    #expect(WalletCommand.parse("купи \(mint) на $3") == .buy(.byMint(mint), usdc: 3))
    #expect(WalletCommand.parse("sell 1500 tokens \(mint)") == .sell(.byMint(mint), amount: 1500))
    #expect(WalletCommand.parse("sell tokens \(mint)") == .sell(.byMint(mint), amount: nil))
    #expect(!SpotAsset.byMint(mint).resolved && !SpotAsset.byMint(mint).verified)
    // The fixed coins stay what they were.
    #expect(WalletCommand.parse("buy SOL for 2 USDC") == .buy(SpotAsset.all[0], usdc: 2))
    #expect(WalletCommand.parse("buy token \(SpotAsset.all[3].mint) for 2 USDC") != .buy(.byMint(SpotAsset.all[3].mint), usdc: 2))
    #expect(ChatRouter.forms.contains("buy token <MINT> for N USDC"))
}

@Test func buyingByMintKnowsItsRisks() throws {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    let json = """
    [{"id": "\(mint)", "name": "POKÉMON CARDS", "symbol": "POKEMON", "decimals": 6, "tokenProgram": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
      "liquidity": 43138, "audit": {"mintAuthorityDisabled": true, "freezeAuthorityDisabled": false}, "tags": ["unknown"]}]
    """
    let t = try #require(TokenResearch.solanaToken(Data(json.utf8), mint: mint))
    #expect(t.decimals == 6 && t.token2022 && !t.verified)
    #expect(t.warnings().first?.contains("freeze") == true)
    #expect(t.warnings().contains { $0.contains("thin") })
    #expect(!t.warnings().contains { $0.contains("print") })
    #expect(t.warnings(russian: true).first?.hasPrefix("создатель может заморозить") == true)
    #expect(t.warnings(russian: true).count == t.warnings().count)
}

@Test func aPastedAddressIsLookedUpBeforeTheModelReads() async throws {
    #expect(ChatRouter.pastedAddress("расскажи про 2FB5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6") == "2FB5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6")
    #expect(ChatRouter.pastedAddress("купи 0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18 на $2") == "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18")
    #expect(ChatRouter.pastedAddress("what is the difference between staking and lending") == nil)
    let model = Script([#"{"route":"chat","reply":"POKEMON, hot."}"#])
    let reading = try await ChatRouter.route("что за токен B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6", context: .init(), client: model, model: "m") { l in
        "facts about \(l.arg)"
    }
    #expect(reading.lookups == [.init("token", "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6")])
    #expect(model.sent.count == 1)
    #expect(model.sent[0][1].text.contains("Already looked up for you — token B5AN"))
}

@Test func tokensByTickerOnSolanaBaseAndArbitrum() {
    // Solana: a ticker is found among Jupiter's verified tokens when it runs; the fixed coins stay themselves.
    #expect(WalletCommand.parse("buy BONK on solana for 5 USDC") == .buy(.bySymbol("BONK"), usdc: 5))
    #expect(WalletCommand.parse("buy $WIF on solana for 2 USDC") == .buy(.bySymbol("WIF"), usdc: 2))
    #expect(WalletCommand.parse("sell 1000 BONK on solana") == .sell(.bySymbol("BONK"), amount: 1000))
    #expect(WalletCommand.parse("buy SOL on solana for 2 USDC") == .buy(SpotAsset.all[0], usdc: 2))
    #expect(!SpotAsset.bySymbol("BONK").resolved && SpotAsset.bySymbol("jup") == SpotAsset.all[3])
    // Base and Arbitrum: by ticker or by contract, both ways.
    let aero = "0x940181a94A35A4569E4529A3CDfB74e38FD98631"
    #expect(WalletCommand.parse("buy AERO on base for 5 USDC") == .buyOn("base", token: "AERO", usdc: 5))
    #expect(WalletCommand.parse("купи ARB на арбитруме на 5$") == .buyOn("arbitrum", token: "ARB", usdc: 5))
    #expect(WalletCommand.parse("купи AAVE на полигоне на 10 usdc") == .buyOn("polygon", token: "AAVE", usdc: 10))
    #expect(WalletCommand.parse("buy UNI on ethereum for 50 USDC") == .buyOn("ethereum", token: "UNI", usdc: 50))
    #expect(WalletCommand.parse("sell all LINK on polygon") == .sellOn("polygon", token: "LINK", amount: nil))
    #expect(WalletCommand.parse("sell all DEGEN on base") == .sellOn("base", token: "DEGEN", amount: nil))
    #expect(WalletCommand.parse("sell 12.5 AERO on base") == .sellOn("base", token: "AERO", amount: Decimal(string: "12.5")!))
    #expect(WalletCommand.parse("buy token \(aero) on base for 3 USDC") == .buyOn("base", token: aero, usdc: 3))
    #expect(WalletCommand.parse("sell tokens \(aero) on base") == .sellOn("base", token: aero, amount: nil))
    #expect(WalletCommand.parse("buy AERO on base")?.amountQuestion == "For how many USDC should I buy AERO on Base?")
    // Where the money comes from, and a sale that keeps the USDC there; the summaries read back the same.
    let fromSolana = WalletCommand.buyOn("base", token: "AERO", usdc: 5, from: "solana")
    #expect(WalletCommand.parse("buy AERO on base for 5 USDC from solana") == fromSolana)
    #expect(WalletCommand.parse(fromSolana.summary) == fromSolana)
    #expect(WalletCommand.parse("купи AERO на base на 5$ с арбитрума") == .buyOn("base", token: "AERO", usdc: 5, from: "arbitrum"))
    #expect(WalletCommand.parse("buy AERO on base for 5 USDC from base") == .buyOn("base", token: "AERO", usdc: 5, from: "base"))
    let keep = WalletCommand.sellOn("base", token: "AERO", amount: 3, keep: true)
    #expect(WalletCommand.parse("sell 3 AERO on base and keep the USDC there") == keep)
    #expect(WalletCommand.parse(keep.summary) == keep)
    #expect(WalletCommand.parse("продай 3 AERO на base, USDC оставь там") == keep)
    #expect(WalletCommand.parse(WalletCommand.sellOn("base", token: "AERO", amount: 3).summary) == .sellOn("base", token: "AERO", amount: 3))
    // Not these: a contract without a network stays Robinhood's; moving USDC is a bridge.
    #expect(WalletCommand.parse("buy token \(aero) for 3 USDC") == .buyStock(symbol: aero, usdc: 3))
    #expect(WalletCommand.parse("move 2 USDC from Solana to Base") == .solanaTo("base", 2))
    #expect(ChatRouter.forms.contains("buy <TICKER> on <base|arbitrum|polygon|ethereum|optimism|bnb|hyperevm> for N USDC") && ChatRouter.forms.contains("buy <TICKER> on solana for N USDC"))
}

/// Ondo's tokenized stocks and USDY are Solana tokens by ticker (2026-10-29): the model writes them in this form, English
/// or Russian, and a stock with no «Ondo» stays Robinhood's.
@Test func ondoStocksAndUSDYAreSolanaTokens() {
    #expect(WalletCommand.parse("buy NVDAon on solana for 5 USDC") == .buy(.bySymbol("NVDAon"), usdc: 5))
    #expect(WalletCommand.parse("купи SPYon на солане на 5$") == .buy(.bySymbol("SPYon"), usdc: 5))
    #expect(WalletCommand.parse("sell all TSLAon on solana") == .sell(.bySymbol("TSLAon"), amount: nil))
    #expect(WalletCommand.parse("продай 0.01 NVDAon на солане") == .sell(.bySymbol("NVDAon"), amount: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("buy USDY on solana for 10 USDC") == .buy(.bySymbol("USDY"), usdc: 10))
    #expect(WalletCommand.parse("купи USDY на солане на 10 usdc") == .buy(.bySymbol("USDY"), usdc: 10))
    #expect(WalletCommand.parse("buy Nvidia stock for 5 USDC") == .buyStock(symbol: "NVDA", usdc: 5))
    #expect(ChatRouter.forms.contains("«купи акции Nvidia от Ondo на 5$» = buy NVDAon on solana for 5 USDC"))
    // Ondo's own mints (its stocks end in «ondo») get its terms on the card; USDC doesn't.
    #expect(Ondo.issued("gEGtLTPNQ7jcg25zTetkbmF7teoDLcrfTnQfmn2ondo") && Ondo.issued(Ondo.usdy) && !Ondo.issued(SpotAsset.usdc.mint))
}

@Test func aTickerFindsJupitersVerifiedTokenNotItsNamesakes() throws {
    let json = """
    [{"id": "Fake1", "symbol": "WIF", "liquidity": 900000000, "tags": ["unknown"]},
     {"id": "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", "symbol": "$WIF", "isVerified": true, "liquidity": 6858079},
     {"id": "Small", "symbol": "$WIF", "isVerified": true, "liquidity": 100}]
    """
    #expect(TokenResearch.solanaMint(Data(json.utf8), symbol: "WIF") == "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm")
    #expect(TokenResearch.solanaMint(Data(json.utf8), symbol: "BONK") == nil)
}

@Test func moneyInTheTradeSizesIt() throws {
    // «увеличь до 5 USDC» on a 3× plan by the book: a $5 margin, not a $5 risk.
    var m = Market(name: "SOL", markPx: 120, szDecimals: 2, maxLeverage: 20)
    m.atrPct = 2
    var i = try #require(IntentParser.apply("long SOL leverage 3x by the book", to: TradeIntent(), markets: ["SOL"]))
    i = try #require(IntentParser.apply("5 USDC", to: i, markets: ["SOL"]))
    #expect(i.amountUSDC == 5 && i.riskUSD == nil)
    let r = Plan.resolve(i, market: m, equity: 30, policy: RiskPolicy()).intent
    let s = try #require(Plan.evaluate(r, market: m).sizing)
    #expect(r.amountUSDC == nil)
    #expect(s.margin <= 5 && s.margin >= Decimal(string: "4.5")!)   // SOL trades in 0.01 steps
    // A risk named with it keeps «N USDC» as the amount to send.
    let both = try #require(IntentParser.apply("risk $1", to: i, markets: ["SOL"]))
    #expect(Plan.resolve(both, market: m, equity: 30, policy: RiskPolicy()).intent.amountUSDC == 5)
}

/// Code's check of the plan's money: a plan built by the rules never trips it, a number in the wrong role does.
@Test func aPlanNeverHoldsMoreThanItsCommandNamed() throws {
    let markets = [Market(name: "BTC", markPx: 84_000, szDecimals: 5, maxLeverage: 40), Market(name: "ETH", markPx: 2_680, szDecimals: 4, maxLeverage: 25),
                   Market(name: "SOL", markPx: Decimal(string: "152.4")!, szDecimals: 2, maxLeverage: 20), Market(name: "kPEPE", markPx: Decimal(string: "0.0113")!, szDecimals: 0, maxLeverage: 10),
                   Market(name: "xyz:CL", markPx: Decimal(string: "71.35")!, szDecimals: 3, maxLeverage: 20)]
    var plans = 0
    for var m in markets {
        m.atrPct = 2
        for money in [3, 5, 8, 10, 20, 50, 100, 1000] as [Decimal] {
            for lev in [nil, 1, 2, 3, 5, 10] as [Decimal?] {
                for stop in [nil, Decimal(string: "0.5")!, 1, 3, 10] as [Decimal?] {
                    for equity in [nil, 30, 500, 20_000] as [Decimal?] {
                        for byRisk in [false, true] {
                            var i = TradeIntent()
                            (i.asset, i.leverage, i.stopPct, i.autoRules) = (m.name, lev, stop, true)
                            if byRisk { i.riskUSD = money / 10 } else { i.amountUSDC = money }
                            let r = Plan.resolve(i, market: m, equity: equity, policy: RiskPolicy()).intent
                            guard let s = Plan.evaluate(r, market: m).sizing else { continue }
                            plans += 1
                            #expect(Plan.moneyProblem(named: i, sizing: s, market: m) == nil, "\(m.name) \(money) lev \(lev ?? 0) stop \(stop ?? 0) equity \(equity ?? 0) risk \(byRisk): margin \(s.margin) loss \(s.lossAtStopUSD)")
                        }
                    }
                }
            }
        }
    }
    #expect(plans > 1_000)
    // «5 USDC» taken as a $5 risk: at a 2% stop that is $250 of position and $50 of margin at 5×.
    let sol = markets[2]
    var named = TradeIntent()
    (named.asset, named.amountUSDC, named.leverage, named.stopPct) = ("SOL", 5, 5, 2)
    var wrong = named
    (wrong.amountUSDC, wrong.riskUSD) = (nil, 5)
    let asRisk = try #require(Plan.evaluate(wrong, market: sol).sizing)
    #expect(Plan.moneyProblem(named: named, sizing: asRisk, market: sol)?.contains("more than the 5 USDC") == true)
    // Ten times the risk named.
    var risk = TradeIntent()
    (risk.asset, risk.riskUSD, risk.leverage, risk.stopPct) = ("SOL", 2, 3, 2)
    var tenfold = risk
    tenfold.riskUSD = 20
    let big = try #require(Plan.evaluate(tenfold, market: sol).sizing)
    #expect(Plan.moneyProblem(named: risk, sizing: big, market: sol)?.contains("more than the $2 risk") == true)
}

@Test func aSmallAccountReachesTheMinimumOrder() throws {
    // $29 at 1% sizes HYPE under $10: the rules raise the risk to the least that reaches it, within the limit.
    var m = Market(name: "HYPE", markPx: 88, szDecimals: 2, maxLeverage: 10)
    m.atrPct = 2.2
    let i = try #require(IntentParser.apply("long HYPE leverage 3x by the book", to: TradeIntent(), markets: ["HYPE"]))
    let policy = RiskPolicy(), r = Plan.resolve(i, market: m, equity: 29, policy: policy).intent
    let s = try #require(Plan.evaluate(r, market: m).sizing)
    #expect(s.notional >= 10 && s.notional < 11)
    #expect(Plan.evaluate(r, market: m).problem == nil)
    // Past the limit it stays as the rules said, and the plan says why it can't be placed.
    let tiny = Plan.resolve(i, market: m, equity: 5, policy: policy).intent
    #expect(Plan.evaluate(tiny, market: m).problem?.contains("$10 minimum") == true)
}

@Test func moneyInTheTradeLowersTheRulesLeverageToFitTheLimit() throws {
    // «make it 8 bucks» on a short by the book: 8 × 5× × 2.9% = $1.16 breaks a 3.5% limit of $29; 4× fits.
    var m = Market(name: "SOL", markPx: 120, szDecimals: 2, maxLeverage: 20)
    m.atrPct = Decimal(string: "1.9")!
    var policy = RiskPolicy()
    policy.maxRiskPct = Decimal(string: "3.5")!
    policy.maxLeverage = 5
    let i = try #require(IntentParser.apply("short SOL by the book 8 USDC", to: TradeIntent(), markets: ["SOL"]))
    let r = Plan.resolve(i, market: m, equity: 29, policy: policy).intent
    #expect(r.leverage == 4)
    #expect(try #require(r.riskUSD) <= Decimal(string: "1.01")!)
    // Leverage the user named stays.
    let named = try #require(IntentParser.apply("leverage 5x", to: i, markets: ["SOL"]))
    #expect(Plan.resolve(named, market: m, equity: 29, policy: policy).intent.leverage == 5)
}

@Test func aMarketHyperliquidDoesntHaveDoesntParse() {
    let m: Set<String> = ["ETH", "SOL", "kPEPE", "DOGE", "xyz:CL", "xyz:TSLA"]
    // PEPE trades by the thousand; it used to leave the plan on ETH.
    #expect(IntentParser.apply("long PEPE by the book", to: TradeIntent(), markets: m)?.asset == "kPEPE")
    #expect(ChatRouter.parses("long PEPE by the book", markets: m))
    #expect(IntentParser.apply("long KPEPE 4 USDC leverage 2x", to: TradeIntent(), markets: m)?.asset == "kPEPE")
    #expect(IntentParser.namedAsset("kPEPE", markets: m) == "kPEPE" && IntentParser.namedAsset("лонг kpepe", markets: m) == "kPEPE")
    #expect(!ChatRouter.parses("long FOOBAR by the book", markets: m))
    #expect(ChatRouter.parses("long oil leverage 2x", markets: m) && ChatRouter.parses("short TSLA 5 USDC", markets: m))
    #expect(ChatRouter.parses("long 5 USDC", markets: m) && ChatRouter.parses("short by the book", markets: m))
}

@Test func aChainOfPlanChangesIsOneAndTwoPlansDontChain() {
    let m: Set<String> = ["ETH", "BTC", "SOL"]
    #expect(ChatRouter.planChange("6 USDC then leverage 4x", markets: m) == "6 USDC leverage 4x")
    #expect(ChatRouter.planChange("long ETH 5 USDC then alert me when ETH is below 2600", markets: m) == nil)
    #expect(!ChatRouter.parses("long ETH then short BTC", markets: m))
    #expect(ChatRouter.parses("withdraw 2 USDC to Solana then long ETH 2 USDC", markets: m))
    #expect(AlertCommand.parse("alert me when ETH is at 2600", markets: m) == .set(coin: "ETH", price: 2600, above: nil))
}

@Test func aSecondReadingSendsAMisreadBackOnceThenAsks() async throws {
    // «увеличь до 5 USDC» read as risk $5: the checker disagrees, the model fixes it.
    let model = Script([#"{"route":"command","command":"risk $5"}"#, #"{"route":"command","command":"5 USDC"}"#])
    let fixed = try await ChatRouter.route("увеличь до 5 USDC", context: .init(), client: model, model: "m",
                                           check: { $0 == "risk $5" ? "they asked for 5 USDC in the trade, not a $5 risk" : nil })
    #expect(fixed.route == .command("5 USDC") && fixed.corrected != nil && fixed.doubt == nil)
    #expect(model.sent[1].last?.text.contains("not a $5 risk") == true)
    // Still disagreeing after that: the model asks the user, with each reading as a card, rather than a third guess.
    let risk = #"{"route":"command","command":"risk $5"}"#
    let question = Script([risk, risk, #"{"route":"clarify","reply":"5 USDC в сделку или риск $5?","options":[{"title":"5 USDC в сделку","comment":"маржа","command":"5 USDC","recommended":true},{"title":"Риск $5","comment":"потеря на стопе","command":"risk $5"}]}"#])
    let asked = try await ChatRouter.route("увеличь до 5 USDC", context: .init(), client: question, model: "m", check: { _ in "not what they asked" })
    #expect(asked.route == .clarify("5 USDC в сделку или риск $5?") && asked.options.map(\.command) == ["5 USDC", "risk $5"] && asked.doubt == nil)
    #expect(question.sent[2].last?.text.contains("Don't guess with their money") == true)
    // A model that writes the same command a third time: nothing runs, the user decides.
    let stubborn = try await ChatRouter.route("увеличь до 5 USDC", context: .init(), client: Script([risk, risk, risk]),
                                              model: "m", check: { _ in "not what they asked" })
    #expect(stubborn.doubt == "not what they asked")
}

@Test func aCommandNoFormReadsGoesBackWithItsForms() async throws {
    // «buy USDT for 750 USDC»: the network left out. The model hears which forms its verb has, and writes one of them.
    let model = Script([#"{"route":"command","command":"buy USDT for 750 USDC then send 750 USDT on solana to Bybit"}"#,
                        #"{"route":"command","command":"buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit"}"#])
    let r = try await ChatRouter.route("переведи 750 из USDC в USDT на солане и отправь на bybit", context: .init(), client: model, model: "m", markets: ["ETH"])
    #expect(r.route == .command("buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit"))
    let told = model.sent[1].last?.text ?? ""
    #expect(told.contains("«buy USDT for 750 USDC» can't run") && told.contains("buy <TICKER> on solana for N USDC"))
}

@Test func whatTheUserLeftOutTheRulesFill() throws {
    // «open a short on btc, risk half a buck»: the stop by the book, the rules' leverage, the $0.50 risk kept.
    var m = Market(name: "BTC", markPx: 84000, szDecimals: 5, maxLeverage: 40)
    m.atrPct = 1
    let i = try #require(IntentParser.apply("short BTC risk $0.50", to: TradeIntent(), markets: ["BTC"])).started
    #expect(i.autoRules == true)
    let r = Plan.resolve(i, market: m, equity: 29, policy: RiskPolicy()).intent
    #expect(r.riskUSD == Decimal(string: "0.5") && r.stopPct != nil && r.leverage == RiskPolicy().maxLeverage)
    // Both named: nothing else is filled but the leverage.
    let both = try #require(IntentParser.apply("short BTC risk $0.50 stop 1.2%", to: TradeIntent(), markets: ["BTC"])).started
    #expect(both.autoRules == nil && Plan.resolve(both, market: m, equity: 29, policy: RiskPolicy()).intent.leverage == RiskPolicy().maxLeverage)
}

@Test func leverageAboveWhatTheSignerTakesIsRefusedInThePlan() throws {
    // BTC allows 40×, the signer refuses over 20× (HLGuard): a 25× plan says so before any money moves to Hyperliquid.
    let m = Market(name: "BTC", markPx: 84000, szDecimals: 5, maxLeverage: 40)
    var i = TradeIntent()
    (i.asset, i.riskUSD, i.stopPct, i.leverage) = ("BTC", 1, 1, 25)
    #expect(Plan.evaluate(i, market: m).problem == "Leverage for BTC: 1× to 20×.")
    i.leverage = 20
    #expect(Plan.evaluate(i, market: m).problem?.hasPrefix("Leverage") != true)
}

@Test func theMinimumOrderNeverSpendsMoreThanNamed() throws {
    // «нефть лонг на 4 доллара x2» is an $8 position: under the $10 minimum, said so, not quietly $5 of margin.
    var m = Market(name: "xyz:CL", markPx: 91, szDecimals: 3, maxLeverage: 20)
    m.atrPct = 1.8
    let i = try #require(IntentParser.apply("long oil 4 USDC leverage 2x", to: TradeIntent(), markets: ["xyz:CL"])).started
    let r = Plan.resolve(i, market: m, equity: 29, policy: RiskPolicy()).intent
    #expect(Plan.evaluate(r, market: m).problem?.contains("$10 minimum") == true)
    // At 3× the same 4 USDC reaches it.
    let three = try #require(IntentParser.apply("leverage 3x", to: i, markets: ["xyz:CL"]))
    let r3 = Plan.resolve(three, market: m, equity: 29, policy: RiskPolicy()).intent
    #expect(Plan.evaluate(r3, market: m).problem == nil)
}

@Test func spanishIsReadAndConfirmsLikeTheOthers() {
    #expect(ChatLanguage.auto.name(recentUserMessages: ["compra sol por 3 dólares"]) == "Spanish")
    #expect(ChatLanguage.auto.name(recentUserMessages: ["largo en bitcoin con apalancamiento 3"]) == "Spanish")
    #expect(ChatLanguage.auto.name(recentUserMessages: ["long btc 3x"]) == "English")
    #expect(ChatLanguage.auto.name(recentUserMessages: ["лонг эфира"]) == "Russian")
    #expect(ChatLanguage.en.name(recentUserMessages: ["compra sol"]) == "English")
    for yes in ["sí", "Sí, hazlo", "dale", "si", "ok", "Ок", "vale!", "do it", "делай", "да"] { #expect(ChatWords.confirms(yes), "\(yes)") }
    for notYes in ["si quieres compra sol", "ok but lower the risk", "long eth"] { #expect(!ChatWords.confirms(notYes), "\(notYes)") }
    // A model that writes the cancel or confirm route as a command means the route.
    #expect((try? ChatRouter.decode(Data(#"{"route": "command", "command": "cancel"}"#.utf8))) == .cancel)
    #expect((try? ChatRouter.decode(Data(#"{"route": "command", "command": "Confirm"}"#.utf8))) == .confirm)
    // A command with the card's description glued on reads as the command alone; a short one with a dash stays whole.
    let glued = "spread 800 USDC from Base across the best rates — Jupiter Lend 501 USDC at 4.55%, Morpho · Base 299 USDC at 4.46%, taking 800.1 from Base (moves first, straight: 501.2 Base → Solana); at most 50% per protocol"
    #expect(ChatRouter.commandOnly(glued) == "spread 800 USDC from Base across the best rates")
    #expect(ChatRouter.commandOnly("pay POST https://x.y/search {\"q\":\"tesla — news\"}") == "pay POST https://x.y/search {\"q\":\"tesla — news\"}")
    // A confirmation is the whole message: with a change in it, the model reads it and the card waits again.
    for yes in ["да, давай", "давай делай", "ок, погнали", "yes, do it now", "go ahead", "let's do it", "sí, hazlo ya", "De acuerdo!"] { #expect(ChatWords.confirms(yes), "\(yes)") }
    for notYes in ["давай на 30", "да, но без камино", "yes but 50", "go long eth", "sign it with 30 USDC", "да сделай 200", "давай лучше в морфо", "ok тогда 200"] {
        #expect(!ChatWords.confirms(notYes), "\(notYes)")
    }
    for no in ["no", "No gracias", "cancela", "déjalo", "нет", "cancel"] { #expect(ChatWords.declines(no), "\(no)") }
    for no in ["стоп, передумал", "не, передумал", "нет, отмени", "отмени это", "nevermind", "cancel that", "forget it, I changed my mind", "no, cancel it"] {
        #expect(ChatWords.declines(no), "\(no)")
    }
    for notNo in ["не на арбитруме", "нет, лучше 150", "не надо в kamino", "no, make it 50", "stop at 2600", "cancel alerts", "давай"] {
        #expect(!ChatWords.declines(notNo), "\(notNo)")
    }
}

@Test func aStoppedPlanGetsTheOneChangeThatPasses() throws {
    var m = Market(name: "SOL", markPx: 120, szDecimals: 2, maxLeverage: 20)
    m.atrPct = 2
    var policy = RiskPolicy()
    policy.maxRiskPct = Decimal(string: "3.5")!
    // An $9.5 order: the least risk that reaches the $10 minimum.
    let small = try #require(IntentParser.apply("long SOL risk $0.3 stop 3% leverage 2x", to: TradeIntent(), markets: ["SOL"]))
    let up = try #require(Plan.fix(small, market: m, equity: 29, policy: policy, openRisk: nil, available: 26))
    #expect(up.command.hasPrefix("risk $") && up.sizing.notional >= 10 && up.sizing.notional < 11)
    // Money named, «3 USDC at 2×» is $6: the least money that reaches it.
    let money = try #require(IntentParser.apply("long SOL 3 USDC leverage 2x", to: TradeIntent(), markets: ["SOL"])).started
    let more = try #require(Plan.fix(money, market: m, equity: 29, policy: policy, openRisk: nil, available: 26))
    #expect(more.command.hasSuffix("USDC") && more.sizing.notional >= 10)
    // Risk $5 of $29 breaks 3.5%: the largest risk that fits.
    let big = try #require(IntentParser.apply("long SOL risk $5 stop 3%", to: TradeIntent(), markets: ["SOL"]))
    let down = try #require(Plan.fix(big, market: m, equity: 29, policy: policy, openRisk: nil, available: 26))
    #expect(down.command.hasPrefix("risk $") && down.sizing.lossAtStopUSD <= Decimal(string: "1.02")!)
    // Nothing fits $3 of money: no fix.
    #expect(Plan.fix(big, market: m, equity: 3, policy: policy, openRisk: nil, available: 1) == nil)
}

@Test func aLongChainFitsAndAnUnreadableReplyGetsASecondTry() async throws {
    // Three token charts by contract ran past a flat 200 characters.
    let a = "0xab093def657f15df31b33922a95e047add645b29"
    let chain = "chart token \(a) on robinhood then chart token \(a) on robinhood then chart token \(a) on robinhood"
    #expect(chain.count > 200 && ChatRouter.maxCommand(chain) >= chain.count)
    // Not JSON, then fine: the model is told what's wrong and answers again.
    let model = Script(["Sure! I'll show the chart.", #"{"route":"command","command":"chart SOL"}"#])
    let reading = try await ChatRouter.route("покажи чарт сола", context: .init(), client: model, model: "m")
    #expect(reading.route == .command("chart SOL"))
    #expect(model.sent[1].last?.text.contains("doesn't fit the format") == true)
    // Twice unreadable: an Unreadable error, nothing run.
    await #expect(throws: ChatRouter.Unreadable.self) {
        try await ChatRouter.route("?", context: .init(), client: Script(Array(repeating: "no json", count: ChatRouter.rounds + 1)), model: "m")
    }
}

@Test func aVerifiedTokenIsFoundByItsNameAndAFakeTickerIsNot() throws {
    let data = Data("""
    [{"id": "SPXmint", "symbol": "SPX", "name": "SPX6900 (Wormhole)", "isVerified": true, "liquidity": 3000000},
     {"id": "FAKEmint", "symbol": "SPX6900", "name": "SPX6900 real", "isVerified": false, "liquidity": 50000000}]
    """.utf8)
    #expect(TokenResearch.solanaMint(data, symbol: "spx6900") == "SPXmint")
    #expect(TokenResearch.solanaMint(data, symbol: "$SPX") == "SPXmint")
    #expect(TokenResearch.solanaMint(data, symbol: "spx6900 real") == nil)
}

/// Only the Confirm button carries out a waiting move: a typed «yes» in any language is answered, never signed.
@Test func typedApprovalNeverCarriesOutAMove() {
    for yes in ["да", "делай", "do it", "yes, do it", "ok", "sí, hazlo", "погнали", "confirm"] {
        #expect(ChatWords.typedApproval(yes, pressed: false, waiting: true), "\(yes)")
        #expect(!ChatWords.typedApproval(yes, pressed: true, waiting: true), "\(yes) pressed")   // the button's own «do it»
        #expect(!ChatWords.typedApproval(yes, pressed: false, waiting: false), "\(yes) nothing waiting")
    }
    // Not an approval: a question, a change, a refusal go on to the model or the parser.
    for other in ["да?", "давай на 30", "не делай", "cancel", "what's the fee"] {
        #expect(!ChatWords.typedApproval(other, pressed: false, waiting: true), "\(other)")
    }
}

/// ↑ walks back through what was sent, ↓ forward and out to an empty field; a field with its own text keeps its arrows.
@Test func upAndDownWalkWhatWasSent() {
    let history = SentHistory.list(["buy SOL for 5 USDC", "do it", "do it", "show my P/L", "show my P/L", "да", "send 1 USDC to Mom"])
    #expect(history == ["buy SOL for 5 USDC", "show my P/L", "send 1 USDC to Mom"])
    var at: Int? = nil, draft = ""
    func press(_ up: Bool) -> Bool {
        guard let next = SentHistory.step(history, at: at, draft: draft, up: up) else { return false }
        (at, draft) = next
        return true
    }
    #expect(press(true) && draft == "send 1 USDC to Mom")
    #expect(press(true) && draft == "show my P/L")
    #expect(press(true) && draft == "buy SOL for 5 USDC")
    #expect(press(true) && draft == "buy SOL for 5 USDC")   // the oldest stays
    #expect(press(false) && draft == "show my P/L")
    #expect(press(false) && draft == "send 1 USDC to Mom")
    #expect(press(false) && draft == "" && at == nil)        // past the newest: an empty field
    #expect(!press(false))                                    // ↓ with nothing brought back isn't history's
    // An edited message is the user's own text now: the arrows move the caret in it.
    #expect(press(true) && draft == "send 1 USDC to Mom")
    draft = "send 2 USDC to Mom"
    #expect(!press(true) && !press(false))
    #expect(SentHistory.step([], at: nil, draft: "", up: true) != nil)   // nothing sent yet: handled, nothing changes
}

/// «$SOL» is a token once closed by a space or a mark; «$5» is dollars; what's sent reads «SOL».
@Test func dollarMentionsNameTokens() {
    #expect(TokenMentions.find("buy $SOL for 2 USDC").map(\.ticker) == ["SOL"])
    #expect(TokenMentions.find("swap $sol, then $eth.").map(\.ticker) == ["sol", "eth"])
    #expect(TokenMentions.find("buy $SO").isEmpty)              // still being typed
    #expect(TokenMentions.find("pay $5 now").isEmpty)
    #expect(TokenMentions.find("a$SOL ").isEmpty)
    let text = "buy $SOL for $5 then sell $eth"
    #expect(text[TokenMentions.find(text)[0].range] == "$SOL")
    #expect(TokenMentions.plain(text) == "buy SOL for $5 then sell eth")
    #expect(TokenMentions.plain("купи $JUP на $3") == "купи JUP на $3")
    // A contract or a mint after «$» is the token too; what's sent is the address as written.
    let evm = "0x4200000000000000000000000000000000000006", mint = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN"
    #expect(TokenMentions.find("buy $\(evm) on base").map(\.ticker) == [evm])
    #expect(TokenMentions.find("buy $\(mint), now").map(\.ticker) == [mint])
    #expect(TokenMentions.addresses("swap $SOL for $\(mint) ") == [mint])
    #expect(TokenMentions.plain("buy $\(evm)") == "buy \(evm)")
    #expect(!TokenMentions.isAddress("SOL") && TokenMentions.isAddress(evm))
}

/// «save vitalik.eth as Vitalik» saves a name to resolve; a look-alike letter makes it no name, and a token or a buy
/// never reads a name as an address.
@Test func addressBookTakesENSAndSNSNamesOnlyToSave() {
    #expect(AddressBookCommand.parse("save vitalik.eth as Vitalik") == .save(address: "vitalik.eth", name: "Vitalik", network: "ethereum"))
    #expect(AddressBookCommand.parse("сохрани Vitalik.eth как Виталик на base") == .save(address: "vitalik.eth", name: "Виталик", network: "base"))
    #expect(AddressBookCommand.parse("save toly.sol as Toly") == .save(address: "toly.sol", name: "Toly", network: "solana"))
    // A Cyrillic «і» in «vіtalik.eth»: not a name at all, so nothing to save.
    #expect(AddressBookCommand.parse("save vіtalik.eth as Vitalik") == nil)
    #expect(AddressBookCommand.domain(in: "pay sub.name.eth now") == "sub.name.eth")
    #expect(AddressBookCommand.address(in: "add token uniswap.eth to favorites") == nil)
}

/// One token for another on an EVM network, straight: English and Russian, a contract, all of it; USDC on a side is the
/// buy or the sale there, with the money on that network. The summaries read back the same.
@Test func swapOneTokenForAnotherOnANetwork() {
    let aero = "0x940181a94A35A4569E4529A3CDfB74e38FD98631"
    let eth = WalletCommand.swapOn("base", from: "ETH", to: "AERO", amount: Decimal(string: "0.01")!)
    #expect(WalletCommand.parse("swap 0.01 ETH for AERO on base") == eth)
    #expect(WalletCommand.parse("swap 0.01 ETH to AERO on Base.") == eth)
    #expect(WalletCommand.parse("поменяй 0.01 ETH на AERO на бейсе") == eth)
    #expect(WalletCommand.parse("обменяй 0,01 ETH на AERO в base") == eth)
    #expect(WalletCommand.parse(eth.summary) == eth && eth.summary == "swap 0.01 ETH for AERO on Base")
    let all = WalletCommand.swapOn("ethereum", from: "PEPE", to: "ETH", amount: nil)
    #expect(WalletCommand.parse("swap all PEPE for ETH on ethereum") == all)
    #expect(WalletCommand.parse("обменяй весь PEPE на ETH в эфириуме") == all)
    #expect(WalletCommand.parse(all.summary) == all)
    let contract = WalletCommand.swapOn("base", from: "DEGEN", to: aero, amount: 1500)
    #expect(WalletCommand.parse("swap 1500 DEGEN for \(aero) on base") == contract)
    #expect(WalletCommand.parse(contract.summary) == contract)
    #expect(WalletCommand.parse("swap 2 POL for AAVE on polygon") == .swapOn("polygon", from: "POL", to: "AAVE", amount: 2))
    // USDC on a side: the buy or the sale, paid and kept on that network.
    #expect(WalletCommand.parse("swap 5 USDC for AERO on base") == .buyOn("base", token: "AERO", usdc: 5, from: "base"))
    #expect(WalletCommand.parse("swap all AERO for USDC on base") == .sellOn("base", token: "AERO", amount: nil, keep: true))
    // Not a swap on an EVM network: Solana has its own forms, and a swap needs its network named.
    #expect(WalletCommand.parse("swap 0.01 ETH for AERO") == nil)
}

/// A NEAR sale names the network its USDC lands on: Solana by default, Base, Arbitrum or Ethereum when said; the
/// summaries read back the same.
@Test func sellNearForUSDCOnANetwork() {
    #expect(WalletCommand.parse("sell NEAR") == .sellNear(amount: nil))
    #expect(WalletCommand.parse("sell NEAR for USDC on base") == .sellNear(amount: nil, to: "base"))
    #expect(WalletCommand.parse("продай 0,5 NEAR в USDC на арбитрум") == .sellNear(amount: Decimal(string: "0.5")!, to: "arbitrum"))
    #expect(WalletCommand.parse("sell 2 NEAR for USDC on ethereum") == .sellNear(amount: 2, to: "ethereum"))
    for c in [WalletCommand.sellNear(amount: nil), .sellNear(amount: 2, to: "base"), .sellNear(amount: Decimal(string: "0.5")!, to: "arbitrum")] {
        #expect(WalletCommand.parse(c.summary) == c)
    }
    #expect(WalletCommand.sellNear(amount: 2, to: "base").summary == "sell 2 NEAR from your NEAR account for USDC on Base")
}

/// Bridged USDC (USDC.e) on Polygon and Arbitrum: named so in the portfolio (both contracts call themselves «USDC»), and
/// «swap all USDC.e for USDC on polygon» is the sale that keeps Circle's USDC there.
@Test func bridgedUSDCIsNamedAndSwapped() {
    #expect(WalletCommand.parse("swap all USDC.e for USDC on polygon") == .sellOn("polygon", token: "USDC.e", amount: nil, keep: true))
    #expect(WalletCommand.parse("swap 12.5 USDC.e for USDC on arbitrum") == .sellOn("arbitrum", token: "USDC.e", amount: Decimal(string: "12.5")!, keep: true))
    let item: [String: Any] = ["value": "2500000", "token": ["address_hash": "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174", "decimals": "6", "symbol": "USDC"]]
    #expect(Holdings.evmTokens("polygon", [item], skip: []).first?.symbol == "USDC.e")
    #expect(Holdings.evmTokens("ethereum", [item], skip: []).first?.symbol == "USDC")   // that address means nothing there
}

/// «Sweep dust»: the limit when said, $5 when not; a talk about small things isn't one; the summary reads back the same.
@Test func sweepDustIsACommand() {
    #expect(WalletCommand.parse("sweep dust") == .sweepDust(under: nil))
    #expect(WalletCommand.parse("собери пыль до $3") == .sweepDust(under: 3))
    #expect(WalletCommand.parse("convert small balances to USDC") == .sweepDust(under: nil))
    #expect(WalletCommand.parse("собери мелочь в USDC") == .sweepDust(under: nil))
    #expect(WalletCommand.parse("это мелочь, не волнуйся") == nil)
    for c in [WalletCommand.sweepDust(under: nil), .sweepDust(under: Decimal(string: "2.5")!)] { #expect(WalletCommand.parse(c.summary) == c) }
}

/// The approvals board and a revoke: English and Russian, Permit2's said, the summary reads back the same.
@Test func approvalsAreShownAndRevoked() {
    #expect(WalletCommand.parse("show approvals") == .showApprovals)
    #expect(WalletCommand.parse("мои разрешения") == .showApprovals)
    #expect(WalletCommand.parse("кому я дал разрешения на мои токены?") == .showApprovals)
    let token = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", spender = "0x2626664c2603336e57b271c5c0b26f421741e481"
    let revoke = WalletCommand.revokeApproval("base", token: token, spender: spender, permit2: false)
    #expect(WalletCommand.parse("revoke the approval of \(token) to \(spender) on base") == revoke)
    #expect(WalletCommand.parse("отзови разрешение \(token) для \(spender) на base") == revoke)
    let viaPermit2 = WalletCommand.revokeApproval("arbitrum", token: token, spender: spender, permit2: true)
    for c in [revoke, viaPermit2] { #expect(WalletCommand.parse(c.summary) == c) }
}

/// Hyperliquid's spot book: a buy for dollars, a sale of coins or all of it; a leveraged trade isn't spot; summaries read back.
@Test func hyperliquidSpotIsParsed() {
    #expect(WalletCommand.parse("buy HYPE on hyperliquid spot for 10 USDC") == .hlSpotBuy(symbol: "HYPE", usdc: 10))
    #expect(WalletCommand.parse("купи HYPE на споте хайперликвида за 15$") == .hlSpotBuy(symbol: "HYPE", usdc: 15))
    #expect(WalletCommand.parse("sell all HYPE spot") == .hlSpotSell(symbol: "HYPE", amount: nil))
    #expect(WalletCommand.parse("sell 2.5 HYPE on hyperliquid spot") == .hlSpotSell(symbol: "HYPE", amount: Decimal(string: "2.5")!))
    #expect(WalletCommand.parse("long HYPE spot price 30") != .hlSpotBuy(symbol: "HYPE", usdc: nil))
    for c in [WalletCommand.hlSpotBuy(symbol: "HYPE", usdc: 12), .hlSpotSell(symbol: "PURR", amount: nil), .hlSpotSell(symbol: "HYPE", amount: 3)] {
        #expect(WalletCommand.parse(c.summary) == c)
    }
}

@Test func crossNetworkSwapIsParsed() {
    #expect(WalletCommand.parse("swap 0.1 SOL on Solana for ETH on Arbitrum") == .crossSwap(from: "SOL", on: "solana", to: "ETH", at: "arbitrum", amount: Decimal(string: "0.1")!))
    #expect(WalletCommand.parse("обменяй 0.1 SOL на ETH в арбитруме") == .crossSwap(from: "SOL", on: "solana", to: "ETH", at: "arbitrum", amount: Decimal(string: "0.1")!))
    #expect(WalletCommand.parse("swap all AERO on base for SOL on solana") == .crossSwap(from: "AERO", on: "base", to: "SOL", at: "solana", amount: nil))
    #expect(WalletCommand.parse("поменяй 5 AERO с бейса на ARB в арбитруме") == .crossSwap(from: "AERO", on: "base", to: "ARB", at: "arbitrum", amount: 5))
    // One network is a swap there; USDC for USDC is a bridge; ETH with no network named stays the swap on that network.
    #expect(WalletCommand.parse("swap 0.01 ETH for AERO on base") == .swapOn("base", from: "ETH", to: "AERO", amount: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("поменяй 0.01 ETH на AERO на бейсе") == .swapOn("base", from: "ETH", to: "AERO", amount: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("swap 5 USDC on base for USDC on arbitrum") == .bridge(from: "base", to: "arbitrum", 5))
    // USDC on one side is a buy or a sale, but the network's own coin isn't on Uniswap's list: ETH ↔ USDC stays a swap.
    #expect(WalletCommand.parse("swap 2 USDC for AERO on base") == .buyOn("base", token: "AERO", usdc: 2, from: "base"))
    #expect(WalletCommand.parse("swap 2 USDC for ETH on base") == .swapOn("base", from: "USDC", to: "ETH", amount: 2))
    #expect(WalletCommand.parse("swap 0.001 ETH for USDC on arbitrum") == .swapOn("arbitrum", from: "ETH", to: "USDC", amount: Decimal(string: "0.001")!))
    #expect(WalletCommand.parse("swap 2 USDC for POL on polygon") == .swapOn("polygon", from: "USDC", to: "POL", amount: 2))
    for c in [WalletCommand.crossSwap(from: "SOL", on: "solana", to: "ETH", at: "arbitrum", amount: Decimal(string: "0.01")!),
              .crossSwap(from: "AERO", on: "base", to: "SOL", at: "solana", amount: nil),
              .crossSwap(from: "USDC", on: "robinhood", to: "ETH", at: "base", amount: 3)] {
        #expect(WalletCommand.parse(c.summary) == c)
    }
}

@Test func sendsToNetworksOnlyNearIntentsReachesAreParsed() {
    // «All» of a network's own coin: everything but the fee (Send.swift), no number needed.
    #expect(WalletCommand.parse("send all SOL to Bybit") == .sendToken(name: "Bybit", token: "SOL", amount: nil, network: nil))
    #expect(WalletCommand.parse("переведи весь ETH на base на Bybit") == .sendToken(name: "Bybit", token: "ETH", amount: nil, network: "base"))
    #expect(WalletCommand.parse("send 0.0005 BTC to Binance") == .sendToken(name: "Binance", token: "BTC", amount: Decimal(string: "0.0005")!, network: nil))
    #expect(WalletCommand.parse("send 20 USDT on tron to Binance") == .sendToken(name: "Binance", token: "USDT", amount: 20, network: "tron"))
    #expect(WalletCommand.parse("send 20 USDC on sui to Sui wallet") == .sendToken(name: "Sui wallet", token: "USDC", amount: 20, network: "sui"))
    #expect(WalletCommand.parse("отправь 30 XRP на Bitstamp") == .sendToken(name: "Bitstamp", token: "XRP", amount: 30, network: nil))
    #expect(AddressBookCommand.parse("save bc1quc8z9y2dmpe2qztdw4zh0ucr7g5p8gw6x3c8ph as Binance BTC")
            == .save(address: "bc1quc8z9y2dmpe2qztdw4zh0ucr7g5p8gw6x3c8ph", name: "Binance BTC", network: "bitcoin"))
    #expect(AddressBookCommand.parse("save TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t as Binance USDT on tron")
            == .save(address: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t", name: "Binance USDT", network: "tron"))
    #expect(AddressBookCommand.parse("save rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh as Bitstamp XRP")
            == .save(address: "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh", name: "Bitstamp XRP", network: "xrp"))
    // A 0x address keeps its own network, a ticker in its name notwithstanding.
    #expect(AddressBookCommand.parse("save 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC as BTC vault on base")
            == .save(address: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC", name: "BTC vault", network: "base"))
}

@Test func receivingFromNetworksWithoutAWalletIsParsed() {
    #expect(WalletCommand.parse("receive 50 USDC from Sui") == .receiveAcross(network: "sui", token: "USDC", amount: 50))
    #expect(WalletCommand.parse("получи 0.01 BTC с биткоина") == .receiveAcross(network: "bitcoin", token: "BTC", amount: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("top up 20 from tron") == .receiveAcross(network: "tron", token: "USDT", amount: 20))
    #expect(WalletCommand.parse("receive SUI from sui") == .receiveAcross(network: "sui", token: "SUI", amount: nil))
    #expect(WalletCommand.parse("receive SUI from sui")?.amountQuestion != nil)
    // Our own networks keep their forms.
    #expect(WalletCommand.parse("bring 5 USDC from base to solana") != .receiveAcross(network: "base", token: "USDC", amount: 5))
    #expect(WalletCommand.parse("how do I top up") == .receive)
    for c in [WalletCommand.receiveAcross(network: "sui", token: "USDC", amount: 50), .receiveAcross(network: "xrp", token: "XRP", amount: 30),
              .receiveAcross(network: "dogecoin", token: "DOGE", amount: 100)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

@Test func cctpIsAskedForByName() {
    #expect(WalletCommand.parse("move 500 USDC from base to arbitrum via cctp") == .cctp(from: "base", to: "arbitrum", 500))
    #expect(WalletCommand.parse("переведи 100 USDC с арбитрума на полигон через circle") == .cctp(from: "arbitrum", to: "polygon", 100))
    #expect(WalletCommand.parse("move 500 USDC from base to arbitrum") == .bridge(from: "base", to: "arbitrum", 500))
    #expect(WalletCommand.parse("move 5 USDC from base to robinhood via cctp") == .bridge(from: "base", to: "robinhood", 5))
    let c = WalletCommand.cctp(from: "ethereum", to: "base", 250)
    #expect(WalletCommand.parse(c.summary) == c)
}

@Test func tempoDollarSwapsAreParsed() {
    #expect(WalletCommand.parse("swap 5 USDC for pathUSD on tempo") == .swapOn("tempo", from: "USDC", to: "pathUSD", amount: 5))
    #expect(WalletCommand.parse("поменяй весь OUSD на USDC в темпо") == .swapOn("tempo", from: "OUSD", to: "USDC", amount: nil))
    #expect(WalletCommand.parse("swap 5 USDC for AERO on base") == .buyOn("base", token: "AERO", usdc: 5, from: "base"))
    let c = WalletCommand.swapOn("tempo", from: "USDC", to: "OUSD", amount: 2)
    #expect(WalletCommand.parse(c.summary) == c)
}

@Test func aaveBorrowingIsParsed() {
    #expect(WalletCommand.parse("borrow 10 USDC on aave") == .aaveBorrow(10, network: "base"))
    #expect(WalletCommand.parse("займи 10 USDC в aave на арбитруме") == .aaveBorrow(10, network: "arbitrum"))
    #expect(WalletCommand.parse("repay all USDC on aave") == .aaveRepay(nil, network: "base"))
    #expect(WalletCommand.parse("погаси долг на aave") == .aaveRepay(nil, network: "base"))
    #expect(WalletCommand.parse("repay 3 USDC on aave on base") == .aaveRepay(3, network: "base"))
    #expect(WalletCommand.parse("supply 0.01 ETH as collateral on aave on base") == .aaveCollateral(Decimal(string: "0.01")!, network: "base"))
    #expect(WalletCommand.parse("положи 0.01 ETH в залог на aave") == .aaveCollateral(Decimal(string: "0.01")!, network: "base"))
    // Lending isn't borrowing.
    #expect(WalletCommand.parse("lend 10 USDC on aave") == .aave(deposit: true, amount: 10, network: "base"))
    for c in [WalletCommand.aaveBorrow(5, network: "base"), .aaveRepay(nil, network: "arbitrum"), .aaveRepay(2, network: "base"),
              .aaveCollateral(Decimal(string: "0.02")!, network: "base")] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

/// Kamino's Backpack market: USDC borrowed against Backpack's tokenized stocks without selling them, paid back, the
/// stock put up and taken back — in English and Russian, by ticker or company. Aave keeps its own words.
@Test func kaminoBorrowingAgainstStocksIsParsed() {
    let five = Decimal(string: "0.05")!
    #expect(WalletCommand.parse("borrow 2 USDC on kamino against 0.05 SPCX") == .kaminoBorrow(2, against: "SPCX", adding: five))
    #expect(WalletCommand.parse("borrow 2 USDC on kamino against SPCX") == .kaminoBorrow(2, against: "SPCX"))
    #expect(WalletCommand.parse("borrow 1 USDC on kamino") == .kaminoBorrow(1))
    #expect(WalletCommand.parse("займи 2 USDC под 0.05 SPCX на камино") == .kaminoBorrow(2, against: "SPCX", adding: five))
    #expect(WalletCommand.parse("займи 3 доллара под мои акции SpaceX, не продавая") == .kaminoBorrow(3, against: "SPCX"))
    #expect(WalletCommand.parse("borrow 4 USDC against my Micron shares") == .kaminoBorrow(4, against: "MU"))
    #expect(WalletCommand.parse("займи 2 под 0.01 MU") == .kaminoBorrow(2, against: "MU", adding: Decimal(string: "0.01")!))
    #expect(WalletCommand.parse("repay all USDC on kamino") == .kaminoRepay(nil))
    #expect(WalletCommand.parse("погаси долг на камино") == .kaminoRepay(nil))
    #expect(WalletCommand.parse("repay 1.5 USDC on kamino") == .kaminoRepay(Decimal(string: "1.5")!))
    #expect(WalletCommand.parse("deposit 0.05 SPCX to kamino") == .kamino(deposit: true, symbol: "SPCX", amount: five))
    #expect(WalletCommand.parse("withdraw SPCX from kamino") == .kamino(deposit: false, symbol: "SPCX", amount: nil))
    #expect(WalletCommand.parse("выведи 0.02 SNDK из камино") == .kamino(deposit: false, symbol: "SNDK", amount: Decimal(string: "0.02")!))
    #expect(WalletCommand.parse("займи 1 USDC под 0.05 спейса на камино") == .kaminoBorrow(1, against: "SPCX", adding: five))
    // «займёт» is time, not a loan.
    #expect(WalletCommand.parse("сколько займёт покупка акций SpaceX?") != .kaminoBorrow(nil, against: "SPCX"))
    // Aave's loans and Kamino's USDC stay what they were.
    #expect(WalletCommand.parse("borrow 10 USDC on aave") == .aaveBorrow(10, network: "base"))
    #expect(WalletCommand.parse("deposit 5 USDC to kamino") == .kamino(deposit: true, symbol: "USDC", amount: 5))
    #expect(WalletCommand.kaminoBorrow(nil, against: "SPCX").amountQuestion == "How many USDC to borrow on Kamino against SPCX?")
    for c in [WalletCommand.kaminoBorrow(2, against: "SPCX", adding: five), .kaminoBorrow(1), .kaminoBorrow(3, against: "MU"),
              .kaminoRepay(nil), .kaminoRepay(2)] {
        #expect(WalletCommand.parse(c.summary) == c, "\(c.summary)")
    }
}

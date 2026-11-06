import CryptoKit
import Foundation
@testable import IhatecryptoCore
import Testing
import WalletCore

private func d(_ s: String) -> Decimal { Decimal(string: s)! }
private let eth = Market(name: "ETH", markPx: 2000, szDecimals: 4, maxLeverage: 25)

// MARK: Parser

@Test func parsesFullTradePhrase() throws {
    let i = try #require(IntentParser.apply("лонг ETH риск $10 стоп 3% тейк 6% плечо 2x", to: TradeIntent()))
    #expect(i.side == .long)
    #expect(i.asset == "ETH")
    #expect(i.riskUSD == 10)
    #expect(i.stopPct == 3)
    #expect(i.takePct == 6)
    #expect(i.leverage == 2)
    #expect(i.amountUSDC == nil)
}

@Test func parsesRussianVariants() throws {
    let text = "Шорт $BTC, риск на сделку — не больше 15$, стоп-лосс 2,5%, 3х"
    let i = try #require(IntentParser.apply(text, to: TradeIntent()))
    #expect(i.side == .short)
    #expect(i.asset == "BTC")
    #expect(i.riskUSD == 15)
    #expect(i.stopPct == d("2.5"))
    #expect(i.leverage == 3)
}

@Test func percentRiskIsNotDollars() {
    #expect(IntentParser.apply("риск 5%", to: TradeIntent())?.riskUSD == nil)
}

@Test func bareTickerMustBeAKnownMarket() throws {
    let i = try #require(IntentParser.apply("лонг hype", to: TradeIntent(), markets: ["HYPE", "ETH"]))
    #expect(i.asset == "HYPE")
    #expect(IntentParser.apply("лонг hype", to: TradeIntent(), markets: ["ETH"])?.asset == "ETH")
}

@Test func vagueEditIsLeftForTheModel() {
    #expect(IntentParser.apply("тейк ближе", to: TradeIntent()) == nil)
    #expect(IntentParser.apply("привет", to: TradeIntent()) == nil)
}

@Test func explicitSourceAmount() {
    #expect(IntentParser.apply("возьми 100 USDC", to: TradeIntent())?.amountUSDC == 100)
}

// MARK: Seed guard

private let twelve = "abandon ability able about above absent absorb abstract absurd abuse access accident"

@Test func seedGuardCatchesPlainNumberedAndPrefixed() {
    #expect(SeedGuard.containsMnemonic(twelve))
    let numbered = twelve.split(separator: " ").enumerated().map { "\($0.offset + 1). \($0.element)" }.joined(separator: "\n")
    #expect(SeedGuard.containsMnemonic(numbered))
    #expect(SeedGuard.containsMnemonic("вот мои слова: " + twelve))
}

@Test func seedGuardIgnoresOrdinaryText() {
    #expect(!SeedGuard.containsMnemonic("лонг ETH риск $10 стоп 3% тейк 6% плечо 2x"))
    let eleven = twelve.split(separator: " ").dropLast().joined(separator: " ")
    #expect(!SeedGuard.containsMnemonic(eleven))
}

// Hardhat's account #0: a public test key.
private let hardhatKey = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

@Test func seedGuardCatchesPrivateKeys() throws {
    let secret = Data(repeating: 7, count: 32)
    let pair = secret + (try Curve25519.Signing.PrivateKey(rawRepresentation: secret)).publicKey.rawRepresentation
    #expect(SeedGuard.containsPrivateKey("import this " + Base58.encode(pair)))
    #expect(SeedGuard.containsPrivateKey("[" + pair.map { String($0) }.joined(separator: ", ") + "]"))
    #expect(SeedGuard.containsPrivateKey("my private key " + hardhatKey))
    #expect(SeedGuard.containsPrivateKey("вот ключ: 0x" + hardhatKey))
}

@Test func seedGuardLetsSignaturesHashesAndAccountsThrough() {
    // 64 bytes that aren't a keypair: a Solana transaction's signature.
    #expect(!SeedGuard.containsPrivateKey("what did " + Base58.encode(Data((0..<64).map { UInt8($0) })) + " do?"))
    // 64 hex digits with no word for a key: a NEAR account, a transaction hash.
    #expect(!SeedGuard.containsPrivateKey("send 1 NEAR to " + hardhatKey))
    #expect(!SeedGuard.containsPrivateKey("what is 0x" + hardhatKey))
}

// MARK: Risk

@Test func longSizingKeepsLossAtStopWithinRisk() {
    let s = RiskCalc.size(riskUSD: 10, stopPct: 3, takePct: 6, leverage: 2, side: .long, market: eth)
    // per coin: 2000·0.03 + (2000 + 1940)·0.00095 (Hyperliquid 0.045% + ihatecrypto 0.05%) = 63.743 → 10 / 63.743 = 0.15688… → 0.1568
    #expect(s.size == d("0.1568"))
    #expect(s.stopPrice == 1940)
    #expect(s.takePrice == 2120)
    #expect(s.margin == d("156.8"))
    #expect(s.builderFeesUSD == d("0.1568") * 3940 * d("0.0005"))
    #expect(s.lossAtStopUSD <= 10)
    #expect(s.lossAtStopUSD > d("9.9"))
    #expect(s.liquidationPrice < s.stopPrice)
}

@Test func shortSizingMirrorsLong() {
    let s = RiskCalc.size(riskUSD: 10, stopPct: 3, takePct: nil, leverage: 2, side: .short, market: eth)
    #expect(s.stopPrice == 2060)
    #expect(s.lossAtStopUSD <= 10)
    #expect(s.liquidationPrice > s.stopPrice)
}

// MARK: Plan

private func intent(risk: String = "10", stop: String = "3", lev: String? = nil) -> TradeIntent {
    var i = TradeIntent()
    i.riskUSD = d(risk)
    i.stopPct = d(stop)
    i.leverage = lev.map(d)
    return i
}

@Test func missingRiskAndStopAskForInput() {
    let e = Plan.evaluate(TradeIntent(), market: eth)
    #expect(e.missing == ["risk in $", "stop in %"])
    #expect(e.sizing == nil)
}

@Test func leverageAboveMarketMaxIsBlocked() {
    // ETH allows 25×; the signer takes at most 20× (HLGuard), so the plan says 20×.
    #expect(Plan.evaluate(intent(lev: "50"), market: eth).problem?.contains("20×") == true)
}

@Test func liquidationBeforeStopIsBlocked() {
    // 20× on a 4% stop: liquidation (~3% away) comes first.
    #expect(Plan.evaluate(intent(stop: "4", lev: "20"), market: eth).problem?.contains("Liquidation") == true)
}

@Test func tinyNotionalIsBlocked() {
    #expect(Plan.evaluate(intent(risk: "0.2", stop: "3"), market: eth).problem?.contains("$10") == true)
}

@Test func bridgeSkippedWhenHyperCoreCoversMargin() {
    let e = Plan.evaluate(intent(lev: "2"), market: eth)
    #expect(Plan.bridgeNeed(intent(lev: "2"), e, hlWithdrawable: 500) == nil)
    #expect(Plan.bridgeNeed(intent(lev: "2"), e, hlWithdrawable: 100) == d("57.39")) // 156.8 margin + 0.58… fees − 100, rounded up
}

@Test func aLimitBelowThePriceIsFundedForTheMarginHeldAtThePrice() {
    // Hyperliquid holds a buy limit's margin at the current price when that's higher: 5% above the entry, 5% more margin.
    let e = Plan.evaluate(intent(lev: "2"), market: eth), s = e.sizing!
    let atEntry = Plan.bridgeNeed(intent(lev: "2"), e, hlWithdrawable: 0)!
    let atMark = Plan.bridgeNeed(intent(lev: "2"), e, hlWithdrawable: 0, mark: s.entry * d("1.05"))!
    #expect(atMark - atEntry >= s.margin * d("0.05") - d("0.01"))
    #expect(Plan.bridgeNeed(intent(lev: "2"), e, hlWithdrawable: 0, mark: s.entry * d("0.95")) == atEntry) // a price below the entry adds nothing
}

@Test func blockedPositionDoesNotQuoteTheBridge() {
    let e = Plan.evaluate(intent(lev: "25"), market: eth)
    #expect(Plan.bridgeNeed(intent(lev: "25"), e, hlWithdrawable: nil) == nil)
    #expect(Plan.blocks(intent(lev: "25"), e, Live())[1].status == .needsInput)
}

// Solana short, Base holds it: the plan's money comes from Base, not a «Short by» on Solana (the 50 USDC long of 2026-09-30).
@Test func sourceIsTheFundingNetwork() {
    var live = Live()
    live.solanaAddressSet = true
    live.solanaUSDC = 25
    live.from = ("Base", 3000)
    live.quote = BridgeQuote(provider: "Relay", amountIn: 50, reserve: 0, amountOut: Decimal(string: "49.97")!, time: "")
    var i = intent()
    i.amountUSDC = 50
    let source = Plan.blocks(i, Plan.evaluate(i, market: eth), live)[0]
    #expect(source.title == "USDC · Base" && source.status == .ready)
    live.from = nil
    #expect(Plan.blocks(i, Plan.evaluate(i, market: eth), live)[0].status == .blocked)
}

@Test func unknownBalanceIsNoDataNotZero() {
    var live = Live()
    live.solanaAddressSet = true
    let source = Plan.blocks(intent(), Plan.evaluate(intent(), market: eth), live)[0]
    #expect(source.status == .noData)
}

// MARK: Model edit (any provider)

import LLMCore

private struct StubModel: LLMClient {
    let reply: String
    func complete(_ request: LLMRequest) async throws -> String {
        #expect(request.wantsJSON)
        #expect(!request.messages[1].text.contains("0x"))
        return reply
    }
}

@Test func modelEditAcceptsFencedJSONFromAnyProvider() async throws {
    var current = TradeIntent()
    current.takePct = 6
    let reply = "```json\n{\"intent\": {\"asset\": \"ETH\", \"side\": \"long\", \"takePct\": 4}, \"reply\": \"Тейк: 6% → 4%.\"}\n```"
    let (next, answer) = try await ChatModel.edit(current, message: "тейк ближе", client: StubModel(reply: reply), model: "m")
    #expect(next?.takePct == 4)
    #expect(answer == "Тейк: 6% → 4%.")
}

@Test func relativeWordingGoesToTheModelWhole() {
    #expect(IntentParser.needsModel("тейк ближе и плечо 3x"))
    #expect(IntentParser.needsModel("плечо поменьше"))
    #expect(!IntentParser.needsModel("лонг ETH риск $10 стоп 3% тейк 6% плечо 2x"))
}

@Test func rulesFillStopTakeAndRiskWhenAsked() throws {
    var i = try #require(IntentParser.apply("шорт ETH, тейк и стоп как учат", to: TradeIntent()))
    #expect(i.side == .short && i.autoRules == true && i.stopPct == nil)
    var m = eth
    m.atrPct = 2
    let (r, notes) = Plan.resolve(i, market: m, equity: 1000, policy: RiskPolicy())
    #expect(r.stopPct == 3)          // 1.5 × ATR 2%
    #expect(r.takePct == 6)          // 2R
    #expect(r.riskUSD == 10)         // 1% of $1000
    #expect(notes.count == 5 && r.partialTake == true && r.leverage == 5) // stop, take, leverage, half at 1R, risk
    let exits = try #require(IntentParser.apply("тейк частями, трейлинг 2%", to: TradeIntent()))
    #expect(exits.partialTake == true && exits.trailingPct == 2)
    i = try #require(IntentParser.apply("стоп 2% тейк 3R", to: i))
    #expect(i.takeR == 3 && i.takePct == nil)
    #expect(Plan.resolve(i, market: m, equity: 1000, policy: RiskPolicy()).intent.takePct == 6)
    #expect(IntentParser.apply("тейк 1:2", to: TradeIntent())?.takeR == 2)
}

@Test func riskRulesBlockTooMuchCapitalAndWarnAboutTheEntry() throws {
    var i = TradeIntent()
    i.riskUSD = 10; i.stopPct = 3; i.takePct = 3; i.leverage = 2
    let pumped = Market(name: "ETH", markPx: 2140, szDecimals: 4, maxLeverage: 25, prevDayPx: 2000, funding: Decimal(string: "0.0001"))
    let s = try #require(Plan.evaluate(i, market: pumped).sizing)
    let small = RiskRules.check(i, s, market: pumped, equity: 200, policy: RiskPolicy())
    #expect(small.contains { $0.level == .block })                       // $10 of $200 is 5% > 2%
    let fine = RiskRules.check(i, s, market: pumped, equity: 1000, policy: RiskPolicy())
    #expect(!fine.contains { $0.level == .block })
    #expect(fine.contains { $0.level == .warn && $0.text.contains("+7%") })       // long after +7% in a day
    #expect(fine.contains { $0.level == .warn && $0.text.hasPrefix("Reward/risk") }) // 1:1 below 1:1.5
    #expect(fine.contains { $0.level == .warn && $0.text.hasPrefix("Funding") })     // 0.01%/h ≈ 88% a year, longs pay
}

@Test func positionCommandsAreRecognised() {
    #expect(PositionCommand.parse("стоп в безубыток") == .stopToBreakeven)
    #expect(PositionCommand.parse("перенеси стоп на 2661") == .stopTo(2661))
    #expect(PositionCommand.parse("закрой половину") == .close(d("0.5")))
    #expect(PositionCommand.parse("закрой 25%") == .close(d("0.25")))
    #expect(PositionCommand.parse("закрой позицию") == .close(1))
    #expect(PositionCommand.parse("отмени лимит") == .cancelOrders)
    #expect(PositionCommand.parse("стоп 3%") == nil)            // a plan edit, not a command
    #expect(PositionCommand.parse("лонг ETH риск $10") == nil)
}

@Test func limitEntryWaitsForAPullback() throws {
    var i = try #require(IntentParser.apply("лонг ETH риск $10 стоп 3% тейк 6% на откате 2%", to: TradeIntent()))
    #expect(i.pullbackPct == 2 && i.entryPx == nil)
    let (r, notes) = Plan.resolve(i, market: eth, equity: 1000, policy: RiskPolicy())
    #expect(r.entryPx == 1960 && notes.first?.contains("pullback") == true)
    let s = try #require(Plan.evaluate(r, market: eth).sizing)
    #expect(s.entry == 1960 && s.stopPrice == d("1901.2"))
    i = try #require(IntentParser.apply("лимит 2050", to: i))
    #expect(i.entryPx == 2050 && i.pullbackPct == nil)
    #expect(Plan.evaluate(i, market: eth).problem?.contains("above the market") == true) // a long limit above the mark fills at once
    #expect(IntentParser.apply("по рынку", to: i)?.entryPx == nil)
}

@Test func openPositionsCountTowardsTheTotalRisk() throws {
    var i = TradeIntent()
    i.riskUSD = 20; i.stopPct = 3
    let s = try #require(Plan.evaluate(i, market: eth).sizing)
    #expect(RiskRules.check(i, s, market: eth, equity: 1000, policy: RiskPolicy(), openRisk: 45).contains { $0.level == .block })  // 4.5% + 2% > 6%
    #expect(!RiskRules.check(i, s, market: eth, equity: 1000, policy: RiskPolicy(), openRisk: 20).contains { $0.level == .block })
    #expect(RiskRules.check(i, s, market: eth, equity: 1000, policy: RiskPolicy(), openRisk: nil).contains { $0.text.contains("no stop") })
}

@Test func moneyCommandsAreRecognised() {
    #expect(WalletCommand.parse("выведи 2 USDC в Solana") == .withdrawToSolana(2))
    #expect(WalletCommand.parse("верни всё в солану") == .withdrawToSolana(nil))
    #expect(WalletCommand.parse("положи 1,5 USDC в хранилище") == .lendDeposit("USDC", d("1.5")))
    #expect(WalletCommand.parse("сними из хранилища") == .lendWithdraw("USDC", nil))
    #expect(WalletCommand.parse("положи 0.005 SOL в хранилище") == .lendDeposit("SOL", d("0.005")))
    #expect(WalletCommand.parse("сними SOL из хранилища") == .lendWithdraw("SOL", nil))
    #expect(WalletCommand.parse("положи 50 USDC в kamino") == .kamino(deposit: true, symbol: "USDC", amount: 50))
    #expect(WalletCommand.parse("застейкай 0.01 SOL") == .stake(d("0.01")))
    #expect(WalletCommand.parse("анстейк") == .unstake(nil))
    #expect(WalletCommand.parse("где лучше держать USDC?") == .yields("USDC"))
    #expect(WalletCommand.parse("какая доходность") == .yields(nil))
    #expect(WalletCommand.parse("разложи USDC по лучшим ставкам") == .allocate("USDC", maxShare: nil))
    #expect(WalletCommand.parse("распредели SOL по протоколам, не больше 70% в один") == .allocate("SOL", maxShare: 70))
    // Where the money comes from: only that network's («закинь 5 долларов из бейз» took Solana's 1.69 too).
    #expect(WalletCommand.parse("spread 5 USDC from Base across the best rates") == .allocate("USDC", maxShare: nil, amount: 5, from: "base"))
    #expect(WalletCommand.parse("spread 5 USDC from Solana across the best rates") == .allocate("USDC", maxShare: nil, amount: 5, from: "solana"))
    #expect(WalletCommand.parse("разложи 1000 USDC с базы по лучшим ставкам") == .allocate("USDC", maxShare: nil, amount: 1000, from: "base"))
    #expect(WalletCommand.parse("spread 5 USDC from Base across the best rates")?.summary == "spread 5 USDC from Base across the best rates")
    #expect(WalletCommand.parse("переложи в лучшие ставки") == .rebalance(maxShare: nil))
    #expect(WalletCommand.parse("rebalance my USDC, max 40% per protocol") == .rebalance(maxShare: 40))
    #expect(WalletCommand.parse("перебалансируй депозиты") == .rebalance(maxShare: nil))
    #expect(WalletCommand.parse("переложи 5 USDC в Kamino") != .rebalance(maxShare: nil))
    #expect(WalletCommand.parse("лонг ETH риск $1 стоп 3%, из Solana 1.5 USDC") == nil)   // a plan, not a transfer
}

@Test func guardianMovesToBreakevenAfterThePartialTakeAndTrails() {
    var r = ExitRules(openedSize: d("0.012"), entry: 2000, long: true, breakevenAfterPartial: true, trailingPct: 2)
    #expect(Guardian.step(&r, size: d("0.012"), stop: 1940, mark: 2010) == .stopTo(d("1969.8"))) // 2% under the best price beats the 3% stop
    #expect(Guardian.step(&r, size: d("0.006"), stop: d("1969.8"), mark: 2020) == .stopToBreakeven) // half taken
    #expect(Guardian.step(&r, size: d("0.006"), stop: 2002, mark: 2020) == nil)          // trail 1979.6 would loosen
    #expect(Guardian.step(&r, size: d("0.006"), stop: 2002, mark: 2100) == .stopTo(2058)) // peak 2100 − 2%
    #expect(Guardian.step(&r, size: d("0.006"), stop: 2058, mark: 2080) == nil)          // price fell back: stop stays
    var s = ExitRules(openedSize: -1, entry: 2000, long: false, breakevenAfterPartial: false, trailingPct: 2)
    #expect(Guardian.step(&s, size: -1, stop: 2060, mark: 1900) == .stopTo(1938))         // short trails from below
}

@Test func spotCommandsUseVerifiedMints() {
    #expect(WalletCommand.parse("купи SOL на 2 USDC") == .buy(SpotAsset.all[0], usdc: 2))
    #expect(WalletCommand.parse("купи биткоин на $5") == .buy(SpotAsset.all[1], usdc: 5))
    #expect(WalletCommand.parse("продай 0,005 SOL") == .sell(SpotAsset.all[0], amount: d("0.005")))
    #expect(WalletCommand.parse("продай весь эфир") == .sell(SpotAsset.all[2], amount: nil))
    #expect(WalletCommand.parse("купи PEPE на 5 USDC") == nil)          // not on the verified list
    #expect(SpotAsset.all[1].mint == "cbbtcf3aa214zXHbiAZQwf4122FBYbraNdFqgw4iMij")
}

@Test func regularPurchasesParseAndAdvancePastAPause() {
    #expect(WalletCommand.parse("покупай SOL на 10 USDC каждую неделю") == .schedule(SpotAsset.all[0], usdc: 10, days: 7))
    #expect(WalletCommand.parse("покупай биткоин на $5 каждый день") == .schedule(SpotAsset.all[1], usdc: 5, days: 1))
    #expect(WalletCommand.parse("покупай ETH на 20 USDC каждые 3 дня") == .schedule(SpotAsset.all[2], usdc: 20, days: 3))
    #expect(WalletCommand.parse("останови покупки") == .stopSchedules)
    #expect(WalletCommand.parse("купи SOL на 2 USDC") == .buy(SpotAsset.all[0], usdc: 2))   // one purchase, not a schedule
    let start = Date(timeIntervalSince1970: 0)
    var s = Schedule(symbol: "SOL", usdc: 10, days: 7, next: start)
    s.advance(now: start.addingTimeInterval(20 * 86_400))                                  // app closed for 20 days
    #expect(s.next == start.addingTimeInterval(21 * 86_400))                                // one catch-up, then weekly
}

@Test func lookalikeSitesAreFlagged() {
    #expect(SiteCheck.check(host: "jup.ag") == .known)
    #expect(SiteCheck.check(host: "app.kamino.finance") == .known)
    #expect(SiteCheck.check(host: "jupp.ag") == .lookalike("jup.ag"))
    #expect(SiteCheck.check(host: "raydlum.io") == .lookalike("raydium.io"))
    #expect(SiteCheck.check(host: "example.com") == .unknown)
    #expect(SiteCheck.check(host: "") == .local)
    // A known name inside another domain, a dash for its dot, or letters from another alphabet (punycode).
    #expect(SiteCheck.check(host: "jup.ag.claim-airdrop.com") == .lookalike("jup.ag"))
    #expect(SiteCheck.check(host: "jup-ag.com") == .lookalike("jup.ag"))
    #expect(SiteCheck.check(host: "hyperliquid-app.xyz") == .lookalike("app.hyperliquid.xyz"))
    #expect(SiteCheck.check(host: "hyperliquid.exchange") == .lookalike("app.hyperliquid.xyz"))
    #expect(SiteCheck.check(host: "hyperliquid.xyz.evil.io") == .lookalike("app.hyperliquid.xyz"))
    #expect(SiteCheck.check(host: "kamino-finance.app") == .lookalike("kamino.finance"))
    #expect(SiteCheck.check(host: "xn--jp-zka.ag") != .unknown)
    #expect(SiteCheck.check(host: "station.jup.ag") == .known)
    #expect(SiteCheck.check(host: "app.hyperliquid.xyz") == .known)
}

@Test func phishingListBlocksADomainAndEverythingUnderIt() {
    let blocked: Set = ["polymaraket.com", "evil.github.io"], allowed: Set = ["relay.walletconnect.org"]
    #expect(SiteCheck.listed(host: "polymaraket.com", blocked: blocked, allowed: allowed))
    #expect(SiteCheck.listed(host: "app.Polymaraket.com", blocked: blocked, allowed: allowed))
    #expect(SiteCheck.listed(host: "evil.github.io", blocked: blocked, allowed: allowed))
    #expect(!SiteCheck.listed(host: "github.io", blocked: blocked, allowed: allowed))   // not the domain above an entry
    #expect(!SiteCheck.listed(host: "jup.ag", blocked: blocked, allowed: allowed))
    #expect(!SiteCheck.listed(host: "relay.walletconnect.org", blocked: blocked.union(["walletconnect.org"]), allowed: allowed))
    #expect(!SiteCheck.listed(host: "", blocked: blocked, allowed: allowed))
}

@Test func riskFollowedByACommaIsStillRead() throws {
    // Found live: «риск $1, стоп…» lost the risk because the comma looked like a decimal separator.
    #expect(IntentParser.apply("шорт нефти риск $1, стоп и тейк как учат", to: TradeIntent(), markets: ["xyz:CL", "ETH"])?.riskUSD == 1)
    #expect(IntentParser.apply("риск 1,5$ стоп 3%", to: TradeIntent())?.riskUSD == d("1.5"))
    #expect(IntentParser.apply("шорт нефти риск $1", to: TradeIntent(), markets: ["xyz:CL", "ETH"])?.asset == "xyz:CL")
    #expect(IntentParser.apply("лонг золото риск $2", to: TradeIntent(), markets: ["xyz:GOLD", "ETH"])?.asset == "xyz:GOLD")
    #expect(IntentParser.apply("лонг TSLA риск $2", to: TradeIntent(), markets: ["xyz:TSLA", "ETH"])?.asset == "xyz:TSLA")
    #expect(IntentParser.apply("лонг ETH риск $2", to: TradeIntent(), markets: ["xyz:ETH", "ETH"])?.asset == "ETH")   // main exchange first
}

@Test func hyperliquidMarketsFallIntoGroups() {
    func m(_ name: String, _ dex: String?) -> Market { Market(name: name, markPx: 1, szDecimals: 2, maxLeverage: 10, dex: dex) }
    #expect(m("ETH", nil).group == .crypto)
    #expect(m("xyz:CL", "xyz").group == .commodities)
    #expect(m("xyz:GOLD", "xyz").group == .commodities)
    #expect(m("xyz:SP500", "xyz").group == .indices)
    #expect(m("xyz:TSLA", "xyz").group == .stocks)
    #expect(IntentParser.pretty("xyz:CL") == "WTI oil (xyz:CL)" && IntentParser.pretty("ETH") == "ETH")
}

@Test func blockedPlanSuggestsARiskThatFits() {
    var i = TradeIntent()
    i.asset = "ETH"; i.side = .long; i.riskUSD = 1; i.stopPct = Decimal(string: "1.5"); i.takePct = 3; i.leverage = 5
    // $13.56 capital, 2% rule → at most $0.27; the suggestion must pass the rules itself.
    let r = Plan.fittingRisk(i, market: eth, equity: Decimal(string: "13.56"), policy: RiskPolicy(), openRisk: 0, available: 10)
    #expect(r != nil && r! <= Decimal(string: "0.27")! && r! > Decimal(string: "0.15")!)
    // Already within the rules: nothing to suggest.
    i.riskUSD = Decimal(string: "0.2")
    #expect(Plan.fittingRisk(i, market: eth, equity: Decimal(string: "13.56"), policy: RiskPolicy(), openRisk: 0, available: 10) == nil)
    // So small that the $10 minimum can't be met: no suggestion.
    i.riskUSD = 1
    #expect(Plan.fittingRisk(i, market: eth, equity: 2, policy: RiskPolicy(), openRisk: 0, available: 10) == nil)
}

@Test func aRouteStepEditsTheAmountTheCommandHolds() {
    let buy = WalletCommand.buyOn("base", token: "AERO", usdc: 2)
    #expect(buy.editableAmount == 2)
    #expect(buy.with(amount: 3).editableAmount == 3)
    #expect(WalletCommand.aave(deposit: true, amount: 5, network: "arbitrum").with(amount: 7) == .aave(deposit: true, amount: 7, network: "arbitrum"))
    #expect(WalletCommand.sendSol(name: "Test", amount: 1).editableAmount == nil)   // a transfer's amount isn't edited from a card
}

// «All there is» when more is asked than held (shortChoices): the same move for what's there, said as words the chat
// reads back as that move — the card's command goes through the parser again.
@Test func allThereIsReadsBackAsTheSameMove() {
    let sol = SpotAsset(symbol: "SOL", mint: "So11111111111111111111111111111111111111112", decimals: 9)
    let moves: [WalletCommand] = [
        .buy(sol, usdc: 150),
        .swapOn("base", from: "ETH", to: "AERO", amount: 1),
        .crossSwap(from: "SOL", on: "solana", to: "ETH", at: "arbitrum", amount: 2),
        .buyOn("base", token: "AERO", usdc: 50, from: "base"),
    ]
    for move in moves {
        let all = move.with(amount: Decimal(string: "0.5")!)
        #expect(all != move)
        #expect(WalletCommand.parse(all.summary) == all, "\(all.summary)")
    }
}

// A price alert tied to a plan prepares it again from `command`: every field must come back as it was.
@Test func planCommandReadsBackToTheSamePlan() throws {
    let markets: Set<String> = ["SOL", "ETH", "xyz:CL"]
    var a = TradeIntent(); a.asset = "SOL"; a.riskUSD = d("0.295"); a.stopPct = 3; a.takePct = 6; a.leverage = 2
    var b = TradeIntent(); b.asset = "xyz:CL"; b.side = .short; b.takeR = d("2.5"); b.autoRules = true; b.pullbackPct = d("1.5")
    b.partialTake = true; b.trailingPct = 2; b.amountUSDC = 12
    var c = TradeIntent(); c.asset = "ETH"; c.riskUSD = 5; c.stopPct = d("2.1"); c.entryPx = d("2650.5"); c.leverage = 3
    for plan in [a, b, c] {
        #expect(IntentParser.apply(plan.command, to: TradeIntent(), markets: markets) == plan, "\(plan.command)")
    }
    #expect(a.command == "long SOL risk $0.295 stop 3% take 6% leverage 2x")
}

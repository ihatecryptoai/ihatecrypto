import Foundation
import HyperliquidSigning
import IhatecryptoCore
@testable import TradeExecution
import Testing
import WalletCore

@Test func exchangeRepliesAreReadStrictly() {
    func reply(_ json: String) -> HLReply { HLReply.parse(Data(json.utf8)) }
    #expect(reply(#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"0.012","avgPx":"2680.5","oid":1}}]}}}"#)
        == .filled(size: Decimal(string: "0.012")!, avgPx: Decimal(string: "2680.5")!))
    #expect(reply(#"{"status":"ok","response":{"type":"default"}}"#) == .ok)
    #expect(reply(#"{"status":"ok","response":{"type":"order","data":{"statuses":["waitingForTrigger",{"error":"Order has invalid price."}]}}}"#).isError)
    #expect(reply(#"{"status":"err","response":"User or API Wallet does not exist."}"#).isError)
    #expect(reply("not json").isError)
}

@Test func stopAndTakeFollowTheActualFill() throws {
    let long = Executor.protectionPrices(entry: 2000, stopPct: 3, takePct: 6, long: true, szDecimals: 4)
    #expect(long.stop == 1940 && long.take == 2120)
    let short = Executor.protectionPrices(entry: 2000, stopPct: 3, takePct: nil, long: false, szDecimals: 4)
    #expect(short.stop == 2060 && short.take == nil)

    // Market triggers close on the opposite side, reduce-only, and may fill up to 10% past the trigger.
    let action = try Executor.protectionAction(asset: 1, long: true, size: Decimal(string: "0.012")!, stop: 1940, take: 2120, szDecimals: 4)
    let json = action.json as! [String: Any]
    let orders = json["orders"] as! [[String: Any]]
    #expect(json["grouping"] as? String == "positionTpsl" && orders.count == 2)
    #expect(orders.allSatisfy { $0["b"] as? Bool == false && $0["r"] as? Bool == true })
    #expect(orders[0]["p"] as? String == "1746")
}

@Test func riskAtStopsUsesTheNearestStopAndFlagsAMissingOne() throws {
    let orders = try JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(#"""
    [{"coin":"ETH","oid":1,"isTrigger":true,"triggerPx":"1940","orderType":"Stop Market"},
     {"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"1960","orderType":"Stop Market"},
     {"coin":"ETH","oid":3,"isTrigger":true,"triggerPx":"2200","orderType":"Take Profit Market"}]
    """#.utf8))
    let long = HyperliquidAccount.Position(coin: "ETH", size: Decimal(string: "0.5")!, entry: 2000, unrealizedPnl: 0, liquidation: nil)
    #expect(HyperliquidAccount.riskAtStops(positions: [long], orders: orders) == 20)   // (2000 − 1960) × 0.5
    let naked = HyperliquidAccount.Position(coin: "BTC", size: -1, entry: 80000, unrealizedPnl: 0, liquidation: nil)
    #expect(HyperliquidAccount.riskAtStops(positions: [long, naked], orders: orders) == nil)
}

@Test func restingEntriesCountAtTheirStops() throws {
    // Four limit entries at 2% each counted for nothing before: filled, they'd be 8% at the stops against the 6% rule.
    let orders = try JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(#"""
    [{"coin":"ETH","oid":1,"isTrigger":false,"limitPx":"2000","sz":"0.5","side":"B","orderType":"Limit","reduceOnly":false,
      "children":[{"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"1960","orderType":"Stop Market","reduceOnly":true},
                  {"coin":"ETH","oid":3,"isTrigger":true,"triggerPx":"2100","orderType":"Take Profit Market","reduceOnly":true}]},
     {"coin":"SOL","oid":4,"isTrigger":false,"limitPx":"100","sz":"2","side":"A","orderType":"Limit","reduceOnly":false,"children":[]},
     {"coin":"SOL","oid":5,"isTrigger":true,"triggerPx":"105","orderType":"Stop Market","reduceOnly":true},
     {"coin":"BTC","oid":6,"isTrigger":false,"limitPx":"90000","sz":"0.01","side":"A","orderType":"Limit","reduceOnly":true}]
    """#.utf8))
    // ETH (2000 − 1960) × 0.5 = 20, SOL short (105 − 100) × 2 = 10; BTC is a reduce-only close, not an entry.
    #expect(HyperliquidAccount.riskAtStops(positions: [], orders: orders) == 30)
    let naked = try JSONDecoder().decode([HyperliquidAccount.Order].self, from: Data(#"""
    [{"coin":"ETH","oid":1,"isTrigger":false,"limitPx":"2000","sz":"0.5","side":"B","orderType":"Limit"}]
    """#.utf8))
    #expect(HyperliquidAccount.riskAtStops(positions: [], orders: naked) == nil)
}

@Test func aWiderStopIsHeldToThePerTradeRule() {
    let policy = RiskPolicy()   // 2% per trade
    // Long 0.01 BTC at 120000 with the stop at 118000; capital $1000. To 90000: −$300 and fees, 30%: refused.
    let wide = Executor.stopBreaksRule(long: true, entry: 120_000, size: Decimal(string: "0.01")!, stop: 90_000, current: 118_000, equity: 1000, policy: policy)
    #expect(wide.map { $0 > 300 } == true)
    // Tightening, a stop in profit, a first stop, unknown capital: never refused.
    #expect(Executor.stopBreaksRule(long: true, entry: 120_000, size: Decimal(string: "0.01")!, stop: 119_000, current: 118_000, equity: 1000, policy: policy) == nil)
    #expect(Executor.stopBreaksRule(long: false, entry: 120_000, size: Decimal(string: "0.01")!, stop: 121_000, current: 122_000, equity: 1000, policy: policy) == nil)
    #expect(Executor.stopBreaksRule(long: true, entry: 120_000, size: Decimal(string: "0.01")!, stop: 90_000, current: nil, equity: 1000, policy: policy) == nil)
    #expect(Executor.stopBreaksRule(long: true, entry: 120_000, size: Decimal(string: "0.01")!, stop: 90_000, current: 118_000, equity: nil, policy: policy) == nil)
    // 117000 is −$31.07 with fees: 3.1% of $1000 is refused, 1.6% of $2000 is within the rule.
    #expect(Executor.stopBreaksRule(long: true, entry: 120_000, size: Decimal(string: "0.01")!, stop: 117_000, current: 118_000, equity: 1000, policy: policy) != nil)
    #expect(Executor.stopBreaksRule(long: true, entry: 120_000, size: Decimal(string: "0.01")!, stop: 117_000, current: 118_000, equity: 2000, policy: policy) == nil)
}

@Test func gasTopUpTargetIsAWholeNumberOfWei() {
    // $1 at $3000 an ETH, 80% of it: 266,666,666,666,666 wei, not the 0 an unrounded quotient read as.
    #expect(Executor.gasExpected(usd: 1, price: 3000) == 266_666_666_666_666)
    #expect(Executor.gasExpected(usd: 2, price: Decimal(string: "2456.37")!) > 600_000_000_000_000)
    #expect(Executor.gasExpected(usd: 1, price: 0) == 1)
}

@Test func kaminoWithdrawAllAsksForTokensNotDollars() {
    // 1.3 SOL worth $200 at ≈ $153.85: ask ≈ 1.313 SOL (1% over), not 202.
    let sol = Executor.kaminoAll(valueUSD: 200, priceUSD: Decimal(string: "153.846153")!)
    #expect(sol > Decimal(string: "1.312")! && sol < Decimal(string: "1.314")!)
    #expect(Executor.kaminoAll(valueUSD: 100, priceUSD: 1) == 101)   // a stablecoin: as before
}

@Test func rentIsReadFromTheSimulationLines() {
    // The Kamino rule refuses a first deposit whose rent is over a tenth of the amount; it reads the SOL line.
    #expect(Executor.solSpent(["USDC: -1000000 (raw units)", "SOL: −0.029794 (fees and rent for new accounts)"]) == Decimal(string: "0.029794"))
    #expect(Executor.solSpent(["SOL: −1,250.5 (fees and rent for new accounts)"]) == Decimal(string: "1250.5"))
    #expect(Executor.solSpent(["SOL: 0.001479 (fees and rent for new accounts)"]) == 0)   // SOL came back, nothing spent
    #expect(Executor.solSpent([]) == 0)
}

@Test func aPickedSwapWayOnlyNarrowsTheQuotedOnes() {
    let relay = Executor.SwapWay(name: "relay", out: 100, worth: 1.90, line: "")
    let bridge = Executor.SwapWay(name: "bridge", out: 101, worth: 1.95, line: "", venue: "KyberSwap")
    #expect(Executor.pick([relay, bridge], prefer: nil)?.name == "bridge")        // whichever leaves more
    #expect(Executor.pick([relay, bridge], prefer: "relay")?.name == "relay")     // the one picked, even if it leaves less
    #expect(Executor.pick([bridge], prefer: "relay") == nil)                      // picked but unquoted: nothing is signed
    // The card names the exchange after the bridge, and a pick of the bridge's way finds it whichever exchange it is.
    #expect(Executor.wayKey(bridge) == "bridge:KyberSwap" && Executor.wayKey(relay) == "relay")
    #expect(Executor.pick([relay, bridge], prefer: "bridge")?.venue == "KyberSwap")
    #expect(Executor.pick([], prefer: nil) == nil)
}

// Relay's swap losing past its cap asks once: «Send anyway» takes its loss as an impact, which passes the impact stop on
// the same loss too — two separate answers had asked in turn forever.
@Test func relaysLossAsksOnceForBothStops() throws {
    try Executor.holdsRelayLoss(in: 100, out: 98)                     // 2%: under its 3% cap
    do {
        try Executor.holdsRelayLoss(in: 100, out: 90)
        Issue.record("a 10% loss went without asking")
    } catch let stop as CostlyMove {
        let impact = try #require(stop.impact)
        #expect(impact == Decimal(string: "0.1"))
        try Executor.$impactAccepted.withValue(min(1, impact * Decimal(string: "1.25")! + Decimal(string: "0.005")!)) {
            try Executor.holdsRelayLoss(in: 100, out: 90)
            try Executor.checkImpact(impact, paid: 100, amount: 100, symbol: "USDC", venue: "Relay's swap")
        }
    }
}

// Confirm takes the way the card showed: Uniswap's versions are one; another exchange or way stops before anything is signed.
@Test func aWayOtherThanTheCardsStopsBeforeSigning() throws {
    try Executor.holdsShownWay("bridge:Uniswap v4", shown: "bridge:Uniswap v3")
    try Executor.holdsShownWay("here:KyberSwap", shown: nil)            // no card: a scheduled buy
    #expect(throws: ExecutionError.self) { try Executor.holdsShownWay("bridge:Uniswap v3", shown: "bridge:KyberSwap") }
    #expect(throws: ExecutionError.self) { try Executor.holdsShownWay("relay", shown: "bridge:KyberSwap") }
    #expect(throws: ExecutionError.self) { try Executor.holdsShownWay("bridge:KyberSwap", shown: "here:KyberSwap") }
}

// Solana short: the first network holding the amount and a dime for Relay; Ethereum last (its gas costs dollars), never out.
@Test func tradeFundingPicksANetworkThatHoldsIt() {
    let held: [String: Decimal] = ["base": 3000, "arbitrum": 60, "ethereum": 9000]
    #expect(EVMNetwork.funding(50, held: { held[$0] })?.key == "base")
    #expect(EVMNetwork.funding(3000, held: { held[$0] })?.key == "ethereum")
    #expect(EVMNetwork.funding(10_000, held: { held[$0] }) == nil)
    #expect(EVMNetwork.funding(59, held: { ["arbitrum": 60, "ethereum": 9000][$0] })?.key == "arbitrum")
    // Ethereum is asked last, never left out: a big balance there pays when nothing cheaper holds enough.
    #expect(EVMNetwork.funding(100, held: { ["ethereum": 9000][$0] })?.key == "ethereum")
    #expect(EVMNetwork.funding(100, held: { ["ethereum": 9000, "tempo": 500][$0] })?.key == "tempo")
}

@Test func aSpreadTakesIdleUSDCWhereItIsAndPullsOnlyWhatSolanaLacks() {
    // The live case: Morpho on Base from Base's own USDC, Jupiter Lend from Solana's 17.77 plus the rest pulled over.
    let pulls = Executor.pullToSolana(slices: [(nil, Decimal(string: "1493.52")!), ("base", Decimal(string: "1494.52")!)],
                                      idle: [("base", 2960), ("robinhood", Decimal(string: "10.27")!)], onSolana: Decimal(string: "17.77")!)
    #expect(pulls.map(\.network) == ["base", "robinhood"])
    #expect(pulls[0].usdc == Decimal(string: "1465.48")! && pulls[1].usdc == Decimal(string: "10.27")!)
    // Enough on Solana already: nothing moves; an EVM slice alone never pulls its own network's money away.
    #expect(Executor.pullToSolana(slices: [(nil, 10)], idle: [("base", 100)], onSolana: 20).isEmpty)
    #expect(Executor.pullToSolana(slices: [("base", 80)], idle: [("base", 100)], onSolana: 0).isEmpty)
    // Short by a few cents: under a bridge's half dollar, so no bridge; the slice takes what is there.
    #expect(Executor.pullToSolana(slices: [(nil, Decimal(string: "20.2")!)], idle: [("base", 100)], onSolana: 20).isEmpty)
}

@Test func aSpreadMovesMoneyStraightFromWhereItLies() {
    // The live case: 18,587 USDC on Ethereum, 1 on Base, none on Solana; Jupiter Lend 500 and Morpho on Base 500. Base's
    // gap comes straight from Ethereum, not Ethereum → Solana → Base.
    let money = Executor.spreadMoves(slices: [(nil, 500), ("base", 500)], idle: [("base", 1), ("ethereum", Decimal(string: "18587.73")!)], onSolana: 0)
    #expect(money.moves.map(\.from) == ["ethereum", "ethereum"] && money.moves.map(\.to) == ["base", "solana"])
    #expect(money.moves.map(\.usdc) == [Decimal(string: "499.1")!, Decimal(string: "500.1")!])
    #expect(money.drawn.map(\.network) == ["base", "ethereum"] && money.drawn.last?.usdc == Decimal(string: "999.2")!)
    // A slice on the network that holds the money moves nothing; Solana's leftover fills an EVM slice no network can.
    #expect(Executor.spreadMoves(slices: [("ethereum", 500)], idle: [("ethereum", 1000)], onSolana: 0).moves.isEmpty)
    let fromSolana = Executor.spreadMoves(slices: [("base", 50)], idle: [], onSolana: 80)
    #expect(fromSolana.moves.count == 1 && fromSolana.moves[0].from == "solana" && fromSolana.moves[0].usdc == 50)
}

@Test func aNodeIsAskedAgainOnlyWhenItsAnswerMayChange() async throws {
    let ok = HTTPURLResponse(url: URL(string: "https://node")!, statusCode: 200, httpVersion: nil, headerFields: nil)!
    let busy = HTTPURLResponse(url: URL(string: "https://node")!, statusCode: 429, httpVersion: nil, headerFields: nil)!
    func reader(_ data: Data, _ status: Int) throws -> String {
        let text = String(decoding: data, as: UTF8.self)
        if text.hasPrefix("error: ") { throw RPCRetry.Again(status == 429 || RPCRetry.transient(text), WalletError(text)) }
        return text
    }
    // A throttle, a dropped connection, then the answer.
    var script: [Result<(Data, URLResponse), Error>] = [.success((Data("error: busy".utf8), busy)), .failure(URLError(.networkConnectionLost)),
                                                        .success((Data("0x1".utf8), ok))]
    #expect(try await RPCRetry.run(delays: [.zero, .zero, .zero], { try script.removeFirst().get() }, answer: reader) == "0x1")
    // A revert is final at once, though its text says «try again».
    var asked = 0
    await #expect(throws: WalletError.self) {
        try await RPCRetry.run(delays: [.zero, .zero], { asked += 1; return (Data("error: execution reverted: try again later".utf8), ok) }, answer: reader)
    }
    #expect(asked == 1)
    // A node that stays down: every retry, then its error.
    asked = 0
    await #expect(throws: WalletError.self) {
        try await RPCRetry.run(delays: [.zero, .zero], { asked += 1; return (Data("error: over rate limit".utf8), ok) }, answer: reader)
    }
    #expect(asked == 3)
    // A node out of its quota (1rpc's «You've reached the usage limit…» with HTTP 200) is asked again — on the next node.
    #expect(RPCRetry.transient("You've reached the usage limit for your current plan. To continue … upgrade here: https://www.1rpc.io/#pricing"))
    #expect(RPCRetry.transient("daily request quota exceeded"))
    #expect(!RPCRetry.transient("execution reverted: ERC20: transfer amount exceeds allowance"))
}

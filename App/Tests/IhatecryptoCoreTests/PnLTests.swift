import Foundation
@testable import IhatecryptoCore
import Testing

private let now = Date(timeIntervalSince1970: 1_800_000_000)
private func ago(_ days: Double) -> Date { now.addingTimeInterval(-days * 86_400) }
private func fill(_ coin: String, _ side: String, _ sz: Decimal, start: Decimal, pnl: Decimal = 0, fee: Decimal, _ days: Double) -> TradeHistory.Fill {
    TradeHistory.Fill(coin: coin, px: 100, sz: sz, side: side, time: ago(days), startPosition: start, dir: "", closedPnl: pnl, fee: fee)
}

// An oil short 10 days ago (+2 − fees), an ETH long 3 days ago (−1 − fees), an ETH long open now.
private let fills = [
    fill("xyz:CL", "A", 1, start: 0, fee: Decimal(string: "0.1")!, 10.5),
    fill("xyz:CL", "B", 1, start: -1, pnl: 2, fee: Decimal(string: "0.1")!, 10),
    fill("ETH", "B", 1, start: 0, fee: Decimal(string: "0.05")!, 3.5),
    fill("ETH", "A", 1, start: 1, pnl: -1, fee: Decimal(string: "0.05")!, 3),
    fill("ETH", "B", 2, start: 0, fee: Decimal(string: "0.02")!, 0.5),
]
private let funding = [PnL.Funding(time: ago(10.2), coin: "xyz:CL", usdc: Decimal(string: "-0.01")!),
                       PnL.Funding(time: ago(0.2), coin: "ETH", usdc: Decimal(string: "0.03")!)]

@Test func pnlSplitsPeriodsAssetsAndFees() {
    let week = PnL.report(.week, now: now, fills: fills, funding: funding, unrealized: ["ETH": Decimal(string: "0.4")!], equity: [], flows: [])
    #expect(week.realized == Decimal(string: "-1.12")!)          // −1 − 0.05 − 0.05 − 0.02: the open's fee counts too
    #expect(week.funding == Decimal(string: "0.03")!)
    #expect(week.trading == Decimal(string: "-1.09")!)
    #expect(week.unrealized == Decimal(string: "0.4")!)
    #expect(week.closedTrades == 1 && week.wins == 0 && week.worst?.coin == "ETH")
    #expect(week.change == nil)                                   // no equity history

    let all = PnL.report(.all, now: now, fills: fills, funding: funding, unrealized: [:], equity: [], flows: [])
    #expect(all.realized == Decimal(string: "0.68")!)             // 2 − 0.2 − 1.12
    #expect(all.closedTrades == 2 && all.wins == 1 && all.best?.coin == "xyz:CL" && all.best?.realized == Decimal(string: "1.8")!)
    #expect(all.byAsset.first?.coin == "xyz:CL")                  // biggest move first

    let oil = PnL.report(.all, coin: "xyz:CL", now: now, fills: fills, funding: funding, unrealized: ["ETH": 5], equity: [], flows: [])
    #expect(oil.realized == Decimal(string: "1.8")! && oil.funding == Decimal(string: "-0.01")! && oil.unrealized == 0)
    #expect(oil.change == nil)                                    // the whole wallet's change isn't one asset's
}

@Test func pnlPortfolioChangeLeavesOutMoneyMovedInAndOut() {
    let equity: [(at: Date, usd: Decimal)] = [(ago(40), 5), (ago(6), 10), (ago(2), 45), (now, 44)]
    let flows = [PnL.Flow(at: ago(4), usd: 31, note: "received", wallet: "w"),
                 PnL.Flow(at: ago(1), usd: -2, note: "sent", wallet: "w"),
                 PnL.Flow(at: ago(20), usd: 100, note: "before the week", wallet: "w")]
    let week = PnL.report(.week, now: now, fills: [], funding: [], unrealized: [:], equity: equity, flows: flows)
    #expect(week.change == 5 && week.flows == 29 && week.changeSince == ago(6))   // 44 − 10 − (31 − 2)

    // History shorter than the period: measured from the oldest point kept, and it says so.
    let month = PnL.report(.month, now: now, fills: [], funding: [], unrealized: [:], equity: Array(equity.dropFirst()), flows: flows)
    #expect(month.changeSince == ago(6) && month.change == 5)
    #expect(PnL.facts(month).contains { $0.hasPrefix("Portfolio 30 days: +$5") })
}

@Test func pnlPhrasesParse() {
    let markets: Set<String> = ["ETH", "xyz:CL"]
    #expect(PnLCommand.parse("какой у меня P/L") == PnLCommand(period: .all, coin: nil))
    #expect(PnLCommand.parse("pnl за неделю") == PnLCommand(period: .week, coin: nil))
    #expect(PnLCommand.parse("сколько я заработал на нефти", markets: markets) == PnLCommand(period: .all, coin: "xyz:CL"))
    #expect(PnLCommand.parse("profit and loss month for ETH", markets: markets) == PnLCommand(period: .month, coin: "ETH"))
    #expect(PnLCommand.parse("profit and loss today") == PnLCommand(period: .day, coin: nil))
    // A ticker nothing knows is still the one asked about; a period word isn't one.
    #expect(PnLCommand.parse("pnl по aero") == PnLCommand(period: .all, coin: "AERO"))
    #expect(PnLCommand.parse("profit and loss for the week") == PnLCommand(period: .week, coin: nil))
    #expect(PnLCommand.parse("long ETH risk $5") == nil)
    #expect(PnLCommand.parse("plan the trade") == nil)
}

@Test func fundingRowsParse() {
    let json = Data(#"[{"time":1790323200066,"delta":{"type":"funding","coin":"xyz:CL","usdc":"-0.000062","szi":"-0.369"}}]"#.utf8)
    #expect(TradeHistory.funding(json) == [PnL.Funding(time: Date(timeIntervalSince1970: 1_790_323_200.066), coin: "xyz:CL", usdc: Decimal(string: "-0.000062")!)])
}

@Test func spotCostIsTheAverageAndASaleTakesItsShare() throws {
    var basis = CostBasis()
    basis.bought(100, for: 10)   // $0.10 each
    basis.bought(100, for: 30)   // $0.30 each → average $0.20
    #expect(basis.usd == 40)
    let up = try #require(basis.pnl(held: 200, price: Decimal(string: "0.25")!))
    #expect(up.usd == 10 && up.pct == 25)
    basis.sold(50)               // a quarter out: a quarter of the cost goes with it
    #expect(basis.amount == 150 && basis.usd == 30)
    // Tokens that arrived from elsewhere don't count: only the 150 with a known cost.
    let down = try #require(basis.pnl(held: 400, price: Decimal(string: "0.1")!))
    #expect(down.usd == -15 && down.pct == -50)
    basis.sold(1000)
    #expect(basis.amount == 0 && basis.usd == 0 && basis.pnl(held: 5, price: 1) == nil)
}

// Blockscout's shape: SPCX bought for 20 USDG in one transaction, a third sold for 8, then 1 SPCX sent in by a friend.
@Test func costBasisReplaysTheWalletsTransfers() {
    let me = "0xC0CE", spcx = "0x4A0E", pool = "0xpool"
    func t(_ token: String, _ symbol: String, _ value: String, from: String, to: String) -> [String: Any] {
        ["token": ["address_hash": token, "symbol": symbol, "decimals": "6"], "total": ["value": value], "from": ["hash": from], "to": ["hash": to]]
    }
    let txs = [[t(spcx, "SPCX", "3000000", from: pool, to: me), t("0xusdg", "USDG", "20000000", from: me, to: pool)],
               [t(spcx, "SPCX", "1000000", from: me, to: pool), t("0xusdg", "USDG", "8000000", from: pool, to: me)],
               [t(spcx, "SPCX", "1000000", from: "0xfriend", to: me)]]
    let changes = CostBasis.replay(wallet: me.lowercased(), contract: spcx.lowercased(), transactions: txs)
    #expect(changes.map(\.tokens) == [3, -1, 1] && changes.map(\.paid) == [20, nil, nil])
    let unpriced = CostBasis.fold(changes, held: 3) { _ in nil }
    #expect(unpriced.amount == 2)                                      // the gift has no price: no cost
    #expect(unpriced.usd == Decimal(20) * 2 / 3)                       // the sale took a third of the cost
    #expect(unpriced.pnl(held: 3, price: 10)?.usd == 20 - Decimal(20) * 2 / 3)   // P/L only on the 2 with a cost
    // Priced at $9 the day it came: counted at that, and marked as come in unpaid.
    let priced = CostBasis.fold(changes, held: 3) { _ in 9 }
    #expect(priced.amount == 3 && priced.usd == Decimal(20) * 2 / 3 + 9 && priced.received == 1 && priced.receivedUSD == 9)
}

// What was held before the oldest change the chain gave counts at the price that day; gas and sends take their share,
// and a buy for dollars stays a buy.
@Test func foldCountsWhatWasThereBeforeAndWhatCameUnpaid() {
    let day = { (d: Double) in Date(timeIntervalSince1970: d) }
    let changes = [CostBasis.Change(at: day(100), tokens: 1, paid: 3000),     // bought 1 ETH for $3,000
                   CostBasis.Change(at: day(200), tokens: -1),                // sent half away
                   CostBasis.Change(at: day(300), tokens: 1)]                 // bridged in, unpaid
    #expect(CostBasis.unpaid(changes) == [day(100), day(300)])
    let basis = CostBasis.fold(changes, held: 2) { $0 == day(100) ? 2000 : $0 == day(300) ? 4000 : nil }
    // 1 there before at $2,000 + 1 bought for $3,000; half out: 1 for $2,500 (0.5 of it unpaid, $1,000); 1 in at $4,000.
    #expect(basis.amount == 2 && basis.usd == 6500)
    #expect(basis.received == Decimal(string: "1.5") && basis.receivedUSD == 5000)
    #expect(basis.since == day(100))
    var empty = CostBasis.fold([], held: 5) { _ in 1 }
    #expect(empty.amount == 0)
    empty.came(2, worth: 10)
    empty.sold(2)
    #expect(empty.received == nil && empty.since == nil)
}

// getTransaction's shape (jsonParsed): the wallet buys BONK for USDC, then for SOL (fee paid by it), gets some sent in,
// sells part; a failed transaction counts for nothing. The cost is the USDC, then the SOL at its price then.
@Test func solanaCostFromTheWalletsOwnTransactions() throws {
    let me = "6nod592sTfEWD3VSVPdQndLMVBCNmMc6ngt7MyGBK21j", bonk = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
    let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    func bal(_ mint: String, _ raw: String, _ decimals: Int, owner: String = me) -> [String: Any] {
        ["mint": mint, "owner": owner, "uiTokenAmount": ["amount": raw, "decimals": decimals]]
    }
    func tx(_ time: Int, pre: [[String: Any]], post: [[String: Any]], lamports: (Int, Int) = (5_000_000_000, 5_000_000_000), err: Any = NSNull()) -> [String: Any] {
        ["blockTime": time,
         "meta": ["err": err, "fee": 5000, "preBalances": [lamports.0], "postBalances": [lamports.1],
                  "preTokenBalances": pre, "postTokenBalances": post],
         "transaction": ["message": ["accountKeys": [["pubkey": me, "signer": true]]]]]
    }
    let txs = [
        tx(1_000, pre: [bal(usdc, "10000000", 6)], post: [bal(usdc, "0", 6), bal(bonk, "100000", 5)], lamports: (5_000_000_000, 4_999_995_000)),
        tx(2_000, pre: [bal(bonk, "100000", 5)], post: [bal(bonk, "200000", 5)], lamports: (4_999_995_000, 4_899_990_000)),
        tx(3_000, pre: [bal(bonk, "200000", 5)], post: [bal(bonk, "300000", 5)]),
        tx(4_000, pre: [bal(bonk, "300000", 5)], post: [bal(bonk, "250000", 5)]),
        tx(5_000, pre: [bal(usdc, "10000000", 6)], post: [bal(usdc, "0", 6), bal(bonk, "999999999", 5)], err: ["InstructionError": [0, "Custom"]]),
    ]
    let moves = txs.compactMap { CostBasis.SolanaMove($0, owner: me) }
    #expect(moves.count == 4 && moves[1].sol == Decimal(string: "-0.1"))
    let changes = CostBasis.replaySolana(mint: bonk, moves: moves) { $0 == Date(timeIntervalSince1970: 2_000) ? 150 : nil }
    #expect(changes.map(\.paid) == [10, 15, nil, nil])
    let basis = CostBasis.fold(changes, held: Decimal(string: "2.5")!) { _ in nil }
    // 1 BONK for $10, 1 for 0.1 SOL × $150 = $15; 1 sent in (no price, no cost); 0.5 out takes a quarter of the 2 with a cost.
    #expect(basis.amount == Decimal(string: "1.5") && basis.usd == Decimal(string: "18.75"))
    #expect(basis.since == Date(timeIntervalSince1970: 1_000) && basis.price == Decimal(string: "12.5"))
    #expect(CostBasis.replaySolana(mint: usdc, moves: moves) { _ in 150 }.allSatisfy { $0.tokens < 0 || $0.paid == nil })
    // SOL itself: the wallet's lamports (the fee given back), here 0.1 out to buy BONK.
    #expect(CostBasis.replaySolana(mint: CostBasis.wsol, moves: moves) { _ in 150 }.filter { $0.tokens < 0 }.map(\.tokens) == [Decimal(string: "-0.1")!])
}

@Test func priceEffectIgnoresMoneyMovedInOrOut() {
    let before = ["sol": PnL.Mark(amount: 2, price: 100), "perp:ETH": PnL.Mark(amount: -1, price: 3000), "gone": PnL.Mark(amount: 5, price: 1)]
    // SOL up $10 a coin, a top-up tripled it, the short gained $50, a sold coin dropped out, a new coin came in.
    let after = ["sol": PnL.Mark(amount: 6, price: 110), "perp:ETH": PnL.Mark(amount: -1, price: 2950), "new": PnL.Mark(amount: 1000, price: 1)]
    #expect(PnL.priceEffect(from: before, to: after) == 70)     // 2 × 10 + (−1) × (−50); the top-up and the new coin count nothing
    #expect(PnL.priceEffect(from: [:], to: after) == 0)
}

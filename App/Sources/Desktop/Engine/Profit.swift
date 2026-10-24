import Foundation
import IhatecryptoCore

/// «какой у меня P/L», «pnl за неделю», «сколько я заработал на нефти»: the shared PnL report — Hyperliquid's fills and
/// funding, the open positions, and the portfolio's change without the money moved in or out, from the points and flows
/// the engine keeps (equity.json, flows.json), as the Mac keeps them.
extension Engine {
    nonisolated struct EquityPoint: Codable { var at: Date; var usd: Decimal; var wallet: String }

    /// A point of the portfolio's value, at most one in ten minutes, a year kept.
    func recordEquity(_ usd: Decimal, wallet: String) {
        var points = Store.load([EquityPoint].self, "equity.json") ?? []
        if let last = points.last(where: { $0.wallet == wallet }), Date().timeIntervalSince(last.at) < 600 { return }
        points.append(EquityPoint(at: Date(), usd: usd, wallet: wallet))
        points.removeAll { Date().timeIntervalSince($0.at) > 365 * 86_400 }
        Store.save(points, "equity.json")
    }

    /// The week the sparkline draws, for the wallet on screen: its points of the last 7 days, at most 200 (the oldest
    /// kept), and the value now when the balances are in. A watched or newly opened wallet gets its own line at once.
    func loadEquity() {
        guard let me = readWallet else { portfolio.equity = []; portfolio.change7d = nil; return }
        let wallet = me.solana.isEmpty ? me.evm : me.solana
        let since = Date().addingTimeInterval(-7 * 86_400)
        let week = (Store.load([EquityPoint].self, "equity.json") ?? []).filter { $0.wallet == wallet && $0.at >= since }.sorted { $0.at < $1.at }
        var points = Self.thinned(week.map { PortfolioView.Point(at: $0.at, usd: $0.usd) }, to: portfolio.updatedAt == nil ? 200 : 199)
        if let now = portfolio.updatedAt { points.append(.init(at: now, usd: ruleEquity ?? 0)) }
        portfolio.equity = points
        portfolio.change7d = points.count > 2 ? points[points.count - 1].usd - points[0].usd : nil
    }

    /// Every n-th point so that at most `limit` are left, the first always among them.
    nonisolated static func thinned<T>(_ points: [T], to limit: Int) -> [T] {
        guard points.count > limit, limit > 0 else { return points }
        let step = Int((Double(points.count) / Double(limit)).rounded(.up))
        return stride(from: 0, to: points.count, by: step).map { points[$0] }
    }

    /// Money that came in (+) or went out (−), so the portfolio's change counts only what prices and trading did.
    func recordFlow(_ usd: Decimal, _ note: String) {
        guard let wallet = solanaAddress, usd != 0 else { return }
        var flows = Store.load([PnL.Flow].self, "flows.json") ?? []
        flows.append(PnL.Flow(at: Date(), usd: usd, note: note, wallet: wallet))
        Store.save(flows, "flows.json")
    }

    func pnlReport(_ command: PnLCommand) async -> PnL.Report? {
        guard let me = evmAddress, let wallet = solanaAddress else { return nil }
        async let fillsCall = TradeHistory.fills(user: me)
        async let fundingCall = TradeHistory.funding(user: me)
        let fills = (try? await fillsCall) ?? [], funding = (try? await fundingCall) ?? []
        // Another wallet since the request: its own fills, not the last one's against its positions.
        guard evmAddress == me else { return await pnlReport(command) }
        let unrealized = Dictionary(positions.map { ($0.coin, $0.unrealizedPnl) }, uniquingKeysWith: +)
        let points = (Store.load([EquityPoint].self, "equity.json") ?? []).filter { $0.wallet == wallet }.map { (at: $0.at, usd: $0.usd) }
            + (portfolio.updatedAt != nil ? [(at: Date(), usd: ruleEquity ?? 0)] : [])
        let flows = (Store.load([PnL.Flow].self, "flows.json") ?? []).filter { $0.wallet == wallet }
        return PnL.report(command.period, coin: command.coin, fills: fills, funding: funding, unrealized: unrealized, equity: points, flows: flows)
    }

    func showPnL(_ command: PnLCommand, note: String?) async {
        let note = note ?? say("P/L · from Hyperliquid and your portfolio", "P/L · из Hyperliquid и твоего портфеля")
        guard let r = await pnlReport(command) else { reply(say("Add a wallet first.", "Сначала добавь кошелёк."), note: note); return }
        // What the wallet holds of the asset asked about, on each network.
        var held: [String] = []
        if let coin = command.coin {
            let symbol = (coin.split(separator: ":").last.map(String.init) ?? coin).uppercased()
            for n in portfolio.networks {
                for row in n.rows where row.symbol.uppercased() == symbol {
                    held.append("\(row.amount) \(row.symbol) " + say("on", "в") + " \(n.name)" + (row.usd.map { " (\(Fmt.usd($0)))" } ?? ""))
                }
            }
            // In no period at all, as the Mac's (PnLBoard.traded over every period): a trade last month still counts today.
            func traded(_ r: PnL.Report) -> Bool { r.realized != 0 || r.funding != 0 || r.unrealized != 0 || !r.byAsset.isEmpty }
            var ever = traded(r)
            if !ever, command.period != .all, let all = await pnlReport(PnLCommand(period: .all, coin: coin)) { ever = traded(all) }
            if held.isEmpty, !ever {
                let name = IntentParser.pretty(coin)
                reply(say("No \(name) in the wallet on any network, and no Hyperliquid trades in it: nothing to count.",
                          "\(name) нет в кошельке ни в одной сети, и сделок по нему на Hyperliquid не было — считать нечего."), note: note)
                return
            }
        }
        let period = russian ? (["day": "24 ч", "week": "7 дней", "month": "30 дней", "all": "всё время"][r.period.rawValue] ?? r.period.title) : r.period.title
        var lines: [String]
        if russian {
            lines = ["P/L за \(period)\(r.coin.map { " по \(IntentParser.pretty($0))" } ?? ""): торговля \(signedUSD(r.trading)) (реализовано после комиссий \(signedUSD(r.realized)), фандинг \(signedUSD(r.funding))), закрыто сделок \(r.closedTrades), в плюсе \(r.wins); открытые позиции сейчас \(signedUSD(r.unrealized))."]
            if let c = r.change, let since = r.changeSince {
                lines.append("Портфель: \(signedUSD(c)) с \(Fmt.date(since)), без учёта \(signedUSD(r.flows)) пополнений и выводов.")
            }
            if let b = r.best, b.realized > 0 { lines.append("Лучшая сделка: \(b.long ? "лонг" : "шорт") \(IntentParser.pretty(b.coin)) \(signedUSD(b.realized)).") }
            if let w = r.worst, w.realized < 0 { lines.append("Худшая сделка: \(w.long ? "лонг" : "шорт") \(IntentParser.pretty(w.coin)) \(signedUSD(w.realized)).") }
        } else {
            lines = PnL.facts(r, pretty: IntentParser.pretty)
        }
        if !held.isEmpty { lines.append(say("Held now: ", "Сейчас в кошельке: ") + held.joined(separator: ", ") + ".") }
        pin("result", "P/L · " + period + (r.coin.map { " · " + IntentParser.pretty($0) } ?? ""), lines: lines, data: await pnlCard(command).map { BlockData(pnl: $0) })
        reply(lines.joined(separator: " "), note: note)
    }

    func signedUSD(_ d: Decimal) -> String { d < 0 ? "−$\(Fmt.num(-d))" : "+$\(Fmt.num(d))" }

    /// For the model's answers about money: the week and all time.
    func pnlFacts() async -> [String] {
        var out: [String] = []
        for period in [PnL.Period.week, .all] {
            if let r = await pnlReport(PnLCommand(period: period, coin: nil)) { out += PnL.facts(r, pretty: IntentParser.pretty) }
        }
        return out
    }
}

import Foundation
import IhatecryptoCore
import TradeExecution

/// ihatecrypto's guardian, as the Mac's (Execution.swift guardPositions, ExitRules.duties): the exit rules the exchange
/// can't hold by itself — the stop to breakeven once the first half is taken, a trailing stop that only tightens — kept
/// per position, run after each refresh while the app is open or in the tray, and said under the position.
extension Engine {
    /// The rules by coin, kept per Hyperliquid account: they live exactly as long as their position.
    var exitRules: [String: ExitRules] {
        get { (Store.load([String: [String: ExitRules]].self, "exit-rules.json") ?? [:])[evmAddress?.lowercased() ?? ""] ?? [:] }
        set {
            guard let key = evmAddress?.lowercased() else { return }
            var all = Store.load([String: [String: ExitRules]].self, "exit-rules.json") ?? [:]
            all[key] = newValue.isEmpty ? nil : newValue
            Store.save(all, "exit-rules.json")
        }
    }

    /// After a plan opened: its partial take and trailing stop become rules; rules left from an earlier position in the
    /// same coin never steer this one.
    func keepExitRules(_ opened: Executor.Opened, intent: TradeIntent) {
        exitRules[opened.asset] = Self.exitRules(size: opened.size, entry: opened.entry, pending: opened.pending, partialTake: opened.partialTake, intent: intent)
    }

    /// The rules a plan's fill leaves for the guardian, or nil when the exchange's stop and take are all there is.
    nonisolated static func exitRules(size: Decimal, entry: Decimal, pending: Bool, partialTake: Decimal?, intent: TradeIntent) -> ExitRules? {
        let partial = partialTake != nil || (pending && intent.partialTake == true)
        guard partial || intent.trailingPct != nil else { return nil }
        var rules = ExitRules(openedSize: size, entry: entry, long: intent.side == .long, breakevenAfterPartial: partial, trailingPct: intent.trailingPct)
        rules.partialOrderPending = pending && intent.partialTake == true
        return rules
    }

    /// «trail 2% on ETH»: the app keeps the stop, nothing is placed on the exchange for it.
    func trail(_ coin: String, _ pct: Decimal) -> String {
        guard let p = positions.first(where: { $0.coin == coin }) else {
            return say("There is no open \(IntentParser.pretty(coin)) position to trail.", "Открытой позиции \(IntentParser.pretty(coin)) нет — тянуть нечего.")
        }
        var rules = exitRules[coin] ?? ExitRules(openedSize: abs(p.size), entry: p.entry, long: p.size > 0, breakevenAfterPartial: false, trailingPct: nil)
        rules.trailingPct = pct
        exitRules[coin] = rules
        Task { await guardPositions() }
        return say("ihatecrypto now keeps the \(IntentParser.pretty(coin)) stop \(Fmt.pct(pct)) behind the best price and only ever tightens it — while ihatecrypto is open or in the tray.",
                   "Теперь ihatecrypto держит стоп \(IntentParser.pretty(coin)) в \(Fmt.pct(pct)) от лучшей цены и только подтягивает его — пока ihatecrypto открыт или в трее.")
    }

    /// One pass over the rules: a limit that filled gets its half at 1R, then each position's stop moves if a rule says so.
    func guardPositions() async {
        guard !watching, activeWallet?.evm != nil, running == nil, signing == 0, !guarding else { return }
        var rules = exitRules
        guard !rules.isEmpty else { return }
        guarding = true
        defer { guarding = false }
        // On a Ledger, stops move by themselves only through the trading key it approved: without it every move would
        // wait for the device's buttons. The stop and take on the exchange protect the position meanwhile.
        if ledgerActive, !agentActive {
            if !ledgerGuardSaid {
                ledgerGuardSaid = true
                notify(.positions, say("Trailing waits for the trading key", "Трейлинг ждёт торговый ключ"),
                       say("On a Ledger, stops move by themselves once its trading key is approved — say “approve trading key”. The stop on the exchange protects the position meanwhile.",
                           "На Ledger стопы двигаются сами, когда одобрен торговый ключ — скажи «approve trading key». Пока позицию защищает стоп на бирже."))
            }
            return
        }
        for coin in rules.keys where !positions.contains(where: { $0.coin == coin }) && !hlOrders.contains(where: { $0.coin == coin && !$0.isTrigger }) {
            rules[coin] = nil
        }
        exitRules = rules
        for (coin, var r) in rules {
            guard let p = positions.first(where: { $0.coin == coin }), let mark = markets[coin]?.markPx else { continue }
            if r.partialOrderPending {
                // The limit filled: the half at 1R goes on now, sized from the real fill.
                r.partialOrderPending = false
                r.openedSize = abs(p.size)
                r.entry = p.entry
                r.peak = p.entry
                exitRules[coin] = r
                _ = try? await executor().addPartialTake(asset: coin)
                continue
            }
            let stop = hlOrders.first { $0.coin == coin && $0.isTrigger && ($0.orderType ?? "").contains("Stop") }?.triggerPx
            let action = Guardian.step(&r, size: p.size, stop: stop, mark: mark)
            if exitRules[coin] != r { exitRules[coin] = r }
            let name = IntentParser.pretty(coin)
            do {
                switch action {
                case .stopToBreakeven?:
                    _ = try await executor().manage(.stopToBreakeven, asset: coin, policy: policy, equity: ruleEquity)
                    let title = say("Partial profit taken on \(name)", "Частичная прибыль по \(name)"), body = say("Stop moved to breakeven.", "Стоп перенесён в безубыток.")
                    reply("\(title). \(body)", note: "guardian")
                    notify(.positions, title, body)
                case .stopTo(let price)?:
                    _ = try await executor().manage(.stopTo(price), asset: coin, policy: policy, equity: ruleEquity)
                    // Moves in a row are one record, a line each (the Mac's): a trend would otherwise fill the History.
                    recordAutomatic(say("Trailing stop on \(name)", "Трейлинг-стоп по \(name)"),
                                    "\(Fmt.time(Date())) · " + say("stop moved to \(Fmt.price(price))", "стоп передвинут на \(Fmt.price(price))"), coin: coin, merging: true)
                case nil: break
                }
                if action != nil { Self.guardRefused[coin] = nil }
            } catch {
                // Once per reason (it runs every 10 s): said again only when the reason changes or after a move went through.
                // The reason without its numbers: the unopened trading key's says when it asks again, new every ten minutes.
                let why = error.localizedDescription, reason = Self.reasonOnly(why)
                if Self.guardRefused[coin] != reason {
                    Self.guardRefused[coin] = reason
                    reply(say("The guardian couldn't move the \(name) stop: ", "Не получилось передвинуть стоп \(name): ") + why, note: "guardian")
                }
            }
        }
    }

    /// The last reason the guardian couldn't move a coin's stop, said once.
    static var guardRefused: [String: String] = [:]

    /// «… ask again from 10:42 AM, …» and «… from 10:52 AM, …» are one reason: the words without the times and amounts.
    nonisolated static func reasonOnly(_ why: String) -> String { String(why.filter { !$0.isNumber }) }

    /// A position gone while ihatecrypto wasn't closing it: its stop, take or a hand elsewhere closed it. Said once, in the
    /// chat and as a notification, as the Mac's closedByExchange.
    func noteClosedPositions(before: [String]) async {
        guard running == nil, let user = evmAddress else { return }
        for coin in Set(before).subtracting(positions.map(\.coin)) {
            exitRules[coin] = nil
            guard let close = try? await HyperliquidAccount.lastClose(user, coin: coin), Date().timeIntervalSince(close.time) < 600 else { continue }
            let text = say("\(coin) position closed outside ihatecrypto (stop, take-profit or by hand) at \(Fmt.price(close.price)): \(close.pnl >= 0 ? "profit" : "loss") \(Fmt.usd(abs(close.pnl))).",
                           "Позиция \(coin) закрылась вне ihatecrypto (стоп, тейк или вручную) по \(Fmt.price(close.price)): \(close.pnl >= 0 ? "прибыль" : "убыток") \(Fmt.usd(abs(close.pnl))).")
            reply(text, note: "exchange")
            notify(.positions, coin, text)
        }
    }

    /// The position as the Mac's card says it: «0.05 at $2,412.50 · stop $2,300 · take $2,600», and the guardian's line.
    func positionViews() -> [Snapshot.Position] {
        let rules = exitRules
        let words = { (en: String, ru: String) in self.say(en, ru) }
        return positions.map { p in
            .init(coin: IntentParser.pretty(p.coin), side: p.size > 0 ? "long" : "short", size: Fmt.num(abs(p.size), maxFraction: 6),
                  entry: Fmt.price(p.entry), pnl: Fmt.usd(p.unrealizedPnl), liquidation: p.liquidation.map(Fmt.price),
                  summary: Self.summary(coin: p.coin, size: p.size, entry: p.entry, orders: hlOrders, say: words),
                  guardian: rules[p.coin].map { Self.duties($0, say: words).joined(separator: " ") },
                  ruler: positionRuler(p, rules: rules[p.coin]))
        }
    }

    /// «0.05 at $2,412.50 · stop $2,300 · take $2,500 and $2,600»: the position's stop and takes on the exchange, the
    /// nearest take first (a partial one fills before the full one), as the Mac's positionSummary.
    nonisolated static func summary(coin: String, size: Decimal, entry: Decimal, orders: [HyperliquidAccount.Order], say: (String, String) -> String) -> String {
        let triggers = orders.filter { $0.coin == coin && $0.isTrigger }
        let stop = triggers.first { ($0.orderType ?? "").contains("Stop") }?.triggerPx
        let takes = triggers.filter { ($0.orderType ?? "").contains("Take") }.compactMap(\.triggerPx).sorted { size > 0 ? $0 < $1 : $0 > $1 }
        return say("\(Fmt.num(abs(size), maxFraction: 6)) at \(Fmt.price(entry)) · stop \(stop.map(Fmt.price) ?? "none")",
                   "\(Fmt.num(abs(size), maxFraction: 6)) по \(Fmt.price(entry)) · стоп \(stop.map(Fmt.price) ?? "нет")")
            + (takes.isEmpty ? "" : say(" · take ", " · тейк ") + takes.map(Fmt.price).joined(separator: say(" and ", " и ")))
    }

    /// What the guardian does for this position, one line each (ExitRules.duties).
    nonisolated static func duties(_ r: ExitRules, say: (String, String) -> String) -> [String] {
        var out: [String] = []
        if r.partialOrderPending { out.append(say("After entry, places half at 1R.", "После входа ставит половину на 1R.")) }
        if r.breakevenAfterPartial {
            out.append(r.movedToBreakeven ? say("Stop is already at breakeven (\(Fmt.price(r.entry))).", "Стоп уже в безубытке (\(Fmt.price(r.entry))).")
                       : say("After the first half, moves the stop to breakeven (\(Fmt.price(r.entry))).", "После первой половины переносит стоп в безубыток (\(Fmt.price(r.entry)))."))
        }
        if let t = r.trailingPct {
            out.append(say("Trailing \(Fmt.pct(t)): the stop follows the best price, now \(Fmt.price(r.peak)).",
                           "Трейлинг \(Fmt.pct(t)): стоп идёт за лучшей ценой, сейчас \(Fmt.price(r.peak))."))
        }
        return out.isEmpty ? [say("Stop and take-profit sit on the exchange; ihatecrypto doesn't move anything.", "Стоп и тейк стоят на бирже; ihatecrypto ничего не двигает.")] : out
    }
}

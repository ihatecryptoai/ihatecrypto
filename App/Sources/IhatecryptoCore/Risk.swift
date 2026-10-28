import Foundation

/// The user's money rules. Defaults follow the common teaching for crypto perps: risk 1–2% of capital per
/// trade, reward at least 1.5–2 times the risk, a stop outside ordinary volatility, moderate leverage.
public struct RiskPolicy: Codable, Sendable, Equatable {
    /// Hard limit: a trade may lose at most this share of capital at its stop.
    public var maxRiskPct: Decimal = 2
    /// Risk used when the user asks for "by the book" and names no amount.
    public var defaultRiskPct: Decimal = 1
    public var minRewardRisk: Decimal = Decimal(string: "1.5")!
    /// Take as a multiple of the stop distance when the user asks for the rules.
    public var defaultRewardRisk: Decimal = 2
    public var maxLeverage: Decimal = 5
    /// Stop "by volatility": this many 4-hour ATRs.
    public var atrStopMultiple: Decimal = Decimal(string: "1.5")!
    /// All open positions together may lose at most this share of capital at their stops ("portfolio heat").
    public var maxOpenRiskPct: Decimal = 6

    public init() {}
}

public struct Advice: Sendable, Equatable {
    public enum Level: Sendable { case ok, warn, block }
    public let level: Level
    public let text: String

    public init(_ level: Level, _ text: String) {
        self.level = level
        self.text = text
    }
}

public enum RiskRules {
    /// Checks a sized plan against the policy and the market. `block` stops execution; `warn` is shown.
    /// `openRisk`: what the positions already open lose at their stops; nil when a position has no stop.
    public static func check(_ i: TradeIntent, _ s: Sizing, market m: Market, equity: Decimal?, policy p: RiskPolicy,
                             openRisk: Decimal? = 0) -> [Advice] {
        var out: [Advice] = []
        let long = i.side == .long

        if let equity, equity > 0 {
            if let open = openRisk {
                let total = (open + s.lossAtStopUSD) / equity * 100
                if open > 0, total > p.maxOpenRiskPct {
                    out.append(Advice(.block, "With open positions, −\(Fmt.usd(open + s.lossAtStopUSD)) at the stops: \(Fmt.pct(total.rounded(1, .plain))) of capital. Rule: all positions together at most \(Fmt.pct(p.maxOpenRiskPct))."))
                } else if open > 0 {
                    out.append(Advice(.ok, "All positions together: \(Fmt.pct(total.rounded(1, .plain))) of capital at the stops, limit \(Fmt.pct(p.maxOpenRiskPct))."))
                }
            } else {
                out.append(Advice(.warn, "An open position has no stop: total risk is unlimited."))
            }
        }

        // Risk per trade as a share of capital.
        if let equity, equity > 0 {
            let pct = s.lossAtStopUSD / equity * 100
            if pct > p.maxRiskPct {
                let allowed = (equity * p.maxRiskPct / 100).rounded(2, .down)
                out.append(Advice(.block, "At the stop −\(Fmt.usd(s.lossAtStopUSD)): \(Fmt.pct(pct.rounded(1, .plain))) of \(Fmt.usd(equity)) capital. Rule: at most \(Fmt.pct(p.maxRiskPct)) per trade, i.e. up to \(Fmt.usd(allowed)). Lower the risk or top up; the limit is in Settings."))
            } else {
                out.append(Advice(.ok, "Risk \(Fmt.pct(pct.rounded(1, .plain))) of capital, within \(Fmt.pct(p.maxRiskPct))."))
            }
            if s.margin > equity / 2 {
                out.append(Advice(.warn, "Margin takes \(Fmt.pct((s.margin / equity * 100).rounded(0, .plain))) of capital: little is left for other trades."))
            }
        } else {
            out.append(Advice(.warn, "Capital unknown: no balance, so risk in % isn't checked."))
        }

        // Reward to risk. A hedge has no take and no bet on direction: the stop, the trend and the entry are beside the point.
        if i.isHedge {
            out.append(Advice(.ok, "Hedge: no take-profit, the stop only guards the margin; net of the coins it covers, the trade costs its fees."))
        } else if let profit = s.profitAtTakeUSD, s.lossAtStopUSD > 0 {
            let r = (profit / s.lossAtStopUSD).rounded(1, .plain)
            out.append(r < p.minRewardRisk
                ? Advice(.warn, "Reward/risk 1:\(Fmt.num(r)): the take-profit is close. The usual minimum is 1:\(Fmt.num(p.defaultRewardRisk)), “take \(Fmt.num(p.defaultRewardRisk))R”.")
                : Advice(.ok, "Reward/risk 1:\(Fmt.num(r))."))
        } else {
            out.append(Advice(.warn, "No take-profit. By the 1:\(Fmt.num(p.defaultRewardRisk)) rule: “take \(Fmt.num(p.defaultRewardRisk))R”."))
        }

        // The stop against ordinary noise.
        if !i.isHedge, let atr = m.atrPct, let stop = i.stopPct, stop < atr {
            out.append(Advice(.warn, "Stop \(Fmt.pct(stop)) is below the average 4-hour move of \(m.name) (ATR \(Fmt.pct(atr.rounded(2, .plain)))): normal noise can hit it. “Stop by the book” sets \(Fmt.num(p.atrStopMultiple)) ATR."))
        }

        // Leverage and liquidation distance.
        let lev = i.leverage ?? 1
        if lev > p.maxLeverage {
            out.append(Advice(.warn, "Leverage \(Fmt.num(lev))× is above \(Fmt.num(p.maxLeverage))×: risk comes from the stop, but liquidation is closer and mistakes cost more."))
        }
        let stopDistance = abs(s.entry - s.stopPrice)
        let liqDistance = abs(s.entry - s.liquidationPrice)
        if !i.isHedge, stopDistance > 0, liqDistance < stopDistance * 2 {
            out.append(Advice(.warn, "Liquidation (\(Fmt.price(s.liquidationPrice))) is less than two stops from entry: a slipping stop could reach it. Lower the leverage."))
        }

        // Entry point: the last day's move and the cost of holding.
        if !i.isHedge, let prev = m.prevDayPx, prev > 0 {
            let change = ((m.markPx - prev) / prev * 100).rounded(1, .plain)
            if long, change >= 5 {
                out.append(Advice(i.entryPx == nil ? .warn : .ok, i.entryPx == nil
                    ? "Not the best entry: \(m.name) +\(Fmt.pct(change)) in 24h. A sharp rise is often followed by a pullback: “pullback 2%” sets a limit below."
                    : "\(m.name) +\(Fmt.pct(change)) in 24h, so the entry is a limit below the market."))
            } else if !long, change <= -5 {
                out.append(Advice(i.entryPx == nil ? .warn : .ok, i.entryPx == nil
                    ? "Not the best entry: \(m.name) \(Fmt.pct(change)) in 24h. A sharp drop is often followed by a bounce: “pullback 2%” sets a limit above."
                    : "\(m.name) \(Fmt.pct(change)) in 24h, so the entry is a limit above the market."))
            } else if long ? change <= -5 : change >= 5 {
                out.append(Advice(.warn, "Trade against the 24h move (\(change > 0 ? "+" : "")\(Fmt.pct(change))): catching a reversal is riskier than following the trend."))
            } else {
                out.append(Advice(.ok, "24h: \(change > 0 ? "+" : "")\(Fmt.pct(change)), no sharp move before entry."))
            }
        }
        if let funding = m.funding {
            // Hyperliquid funding is hourly; longs pay when it is positive.
            let pays = long ? funding > 0 : funding < 0
            let yearly = (abs(funding) * 24 * 365 * 100).rounded(0, .plain)
            if pays, yearly >= 30 {
                out.append(Advice(.warn, "Funding: \(long ? "longs" : "shorts") pay ≈ \(Fmt.pct(yearly)) a year, ≈ \(Fmt.usd((s.notional * abs(funding) * 24).rounded(2, .up))) a day for this position."))
            }
        }
        return out
    }
}

extension Plan {
    /// Fills what the user left to the rules ("by the book", "take 2R"): stop from volatility, take from the stop,
    /// risk from capital. Returns the concrete intent and one line per filled value.
    /// `spot`: what the wallet holds of the plan's coin, which sizes a hedge; nil when not read.
    public static func resolve(_ i: TradeIntent, market: Market?, equity: Decimal?, policy p: RiskPolicy,
                               spot: Decimal? = nil) -> (intent: TradeIntent, notes: [String]) {
        if i.isHedge { return hedge(i, market: market, spot: spot) }
        var r = i
        var notes: [String] = []
        let auto = i.autoRules == true
        if let pullback = i.pullbackPct, let m = market {
            r.entryPx = m.markPx * (1 - (i.side == .long ? 1 : -1) * pullback / 100)
            notes.append("Limit entry on a \(Fmt.pct(pullback)) pullback: \(Fmt.price(r.entryPx!)) vs. \(Fmt.price(m.markPx)) now.")
        }
        if r.stopPct == nil, auto, let atr = market?.atrPct {
            r.stopPct = (atr * p.atrStopMultiple).rounded(1, .up)
            notes.append("Stop by volatility: \(Fmt.num(p.atrStopMultiple)) × 4h ATR (\(Fmt.pct(atr.rounded(2, .plain)))) ≈ \(Fmt.pct(r.stopPct!)).")
        }
        if r.takePct == nil, let stop = r.stopPct, let multiple = i.takeR ?? (auto ? p.defaultRewardRisk : nil) {
            r.takePct = (stop * multiple).rounded(2, .plain)
            notes.append("Take-profit \(Fmt.num(multiple))R: \(Fmt.num(multiple)) × stop = \(Fmt.pct(r.takePct!)).")
        }
        // Leverage left out is the rules' cap even with the rest named: at 1× «risk $0.5 stop 1.2%» needed $39 of margin.
        if r.leverage == nil, let m = market {
            // Risk comes from the stop, not the leverage: the rules' ceiling just keeps margin small.
            r.leverage = min(p.maxLeverage, Decimal(m.maxLeverage))
            notes.append("Leverage \(Fmt.num(r.leverage!))×, the rules' cap; the stop sets the loss, leverage only shrinks the margin.")
        }
        if r.partialTake == nil, auto {
            r.partialTake = true
            notes.append("Exit in parts: half at 1R, then stop to breakeven, the rest runs to the take-profit.")
        }
        // «5 USDC» and no risk named: the money in the trade is its margin, so the size and the risk follow from it and
        // the stop, and the bridge brings what that margin and the fees need.
        if r.riskUSD == nil, let amount = i.amountUSDC, amount > 0, let stop = r.stopPct {
            var lev = r.leverage ?? 1
            // What a dollar of position loses at the stop, fees on both legs included, so the margin is the money.
            let perDollar = stop / 100 + 2 * RiskCalc.tradeFee
            // Leverage the rules chose is lowered until that money's risk fits the limit: «8 USDC» at 5× would break it.
            if i.leverage == nil, let equity, equity > 0 {
                let fit = (equity * p.maxRiskPct / 100 / (amount * perDollar)).rounded(0, .down)
                if fit >= 1, fit < lev {
                    lev = fit
                    r.leverage = fit
                    notes.append("Leverage \(Fmt.num(fit))× keeps \(Fmt.num(amount)) USDC in the trade within the \(Fmt.pct(p.maxRiskPct)) risk limit.")
                }
            }
            r.riskUSD = (amount * lev * perDollar).rounded(2, .down)
            r.amountUSDC = nil
            notes.append("\(Fmt.num(amount)) USDC in the trade at \(Fmt.num(lev))× with a \(Fmt.pct(stop)) stop risks \(Fmt.usd(r.riskUSD!)).")
        }
        if r.riskUSD == nil, auto, let equity, equity > 0 {
            r.riskUSD = (equity * p.defaultRiskPct / 100).rounded(2, .down)
            notes.append("Risk \(Fmt.pct(p.defaultRiskPct)) of \(Fmt.usd(equity)) capital = \(Fmt.usd(r.riskUSD!)).")
        }
        // A small account's share sizes under Hyperliquid's $10 minimum order: raise the risk to the least that reaches
        // it while the rule's limit allows, rather than draw a plan that can't be placed.
        if i.riskUSD == nil, let risk = r.riskUSD, let stop = r.stopPct, let m = market, m.markPx > 0, let equity, equity > 0 {
            let entry = r.entryPx ?? m.markPx
            let stopPx = entry * (1 - (r.side == .long ? 1 : -1) * stop / 100)
            let perCoin = entry * stop / 100 + (entry + stopPx) * RiskCalc.tradeFee
            let least = ((minNotional / entry).rounded(m.szDecimals, .up) * perCoin).rounded(2, .up)
            // Money the user named caps it: «4 USDC at 2×» is $8, and raising it to $10 would spend more than they said.
            let withinMoney = i.amountUSDC.map { minNotional / (r.leverage ?? 1) <= $0 } ?? true
            if least > risk, least <= equity * p.maxRiskPct / 100, withinMoney {
                r.riskUSD = least
                notes.append("Risk raised to \(Fmt.usd(least)) so the order reaches Hyperliquid's $10 minimum.")
            }
        }
        return (r, notes)
    }

    /// A hedge: a short of the coins held (all, a share, or the number named, never more than there is), at 2× unless
    /// named, with a stop that only guards the margin and nothing else a trade has.
    static func hedge(_ i: TradeIntent, market: Market?, spot: Decimal?) -> (intent: TradeIntent, notes: [String]) {
        var r = TradeIntent()
        r.asset = i.asset
        r.side = .short
        r.leverage = i.leverage
        r.stopPct = i.stopPct
        // Still a hedge while the balance isn't read: `hedge` stays nil and the plan waits for it.
        r.hedgeShare = i.hedgeShare ?? 1
        var notes: [String] = []
        if let spot {
            let asked = i.hedge ?? spot * (i.hedgeShare ?? 1)
            r.hedge = min(asked, spot)
            if asked > spot { notes.append("The wallet holds \(Fmt.num(spot, maxFraction: 6)) \(IntentParser.pretty(i.asset)): the hedge covers that, not \(Fmt.num(asked, maxFraction: 6)).") }
        }
        guard let m = market else { return (r, notes) }
        if r.leverage == nil {
            r.leverage = min(Hedge.leverage, Decimal(m.maxLeverage))
            notes.append("Leverage \(Fmt.num(r.leverage!))× for a hedge: margin is \(Fmt.pct((100 / r.leverage!).rounded(0, .plain))) of the position, liquidation far above.")
        }
        if r.stopPct == nil {
            r.stopPct = Hedge.guardStop(leverage: r.leverage!, market: m)
            notes.append("Stop \(Fmt.pct(r.stopPct!)) above the entry, four fifths of the way to liquidation: it only keeps the margin.")
        }
        return (r, notes)
    }

    static func risk(_ i: TradeIntent, _ e: Evaluation, _ live: Live) -> Block {
        let label = "Risk management"
        guard let s = e.sizing, let m = live.market else {
            return Block(id: "risk", label: label, title: "Rules", lines: live.notes + ["Checks appear once the position is sized."], status: .info)
        }
        let advice = RiskRules.check(i, s, market: m, equity: live.equity, policy: live.policy, openRisk: live.openRisk)
        let mark: [Advice.Level: String] = [.ok: "✓", .warn: "⚠", .block: "✗"]
        var lines = live.notes
        if i.isHedge {
            lines.append("Net of the coins it covers: what the short loses, they gain, and back. The trade costs ≈ \(Fmt.usd(s.feesUSD)) in fees, plus funding while it's open.")
        } else if let equity = live.equity, equity > 0 {
            var result = "If the stop hits: −\(Fmt.usd(s.lossAtStopUSD)) (−\(Fmt.pct((s.lossAtStopUSD / equity * 100).rounded(1, .plain))) of capital)."
            if let profit = s.profitAtTakeUSD {
                result += " If the take-profit hits: +\(Fmt.usd(profit)) (+\(Fmt.pct((profit / equity * 100).rounded(1, .plain))))."
            }
            lines.append(result)
        }
        lines += advice.map { "\(mark[$0.level]!) \($0.text)" }
        let status: Block.Status = advice.contains { $0.level == .block } ? .blocked : advice.contains { $0.level == .warn } ? .warning : .ready
        return Block(id: "risk", label: label, title: status == .blocked ? "Rule broken" : status == .warning ? "Warnings" : "Rules met",
                     lines: lines, status: status)
    }
}

extension Plan {
    /// For a plan the rules stop: the largest risk, in whole cents and below what was asked, that passes every rule,
    /// the $10 minimum and fits the money available. nil when nothing smaller works either.
    /// A stopped plan's one change that passes every rule, the $10 minimum and the money, as the chat's form («risk $0.32»,
    /// «5 USDC») with the trade it makes. Too small: the least risk — or money, when the user named the money — that reaches
    /// the minimum order. Too big: the largest that fits. nil when no such change is found.
    public static func fix(_ raw: TradeIntent, market m: Market, equity: Decimal?, policy: RiskPolicy,
                           openRisk: Decimal?, available: Decimal?) -> (command: String, sizing: Sizing)? {
        func passes(_ j: TradeIntent) -> Sizing? {
            let r = resolve(j, market: m, equity: equity, policy: policy).intent
            let e = evaluate(r, market: m)
            guard e.problem == nil, let s = e.sizing, s.notional >= minNotional else { return nil }
            if let available, s.margin + s.feesUSD > available { return nil }
            return RiskRules.check(r, s, market: m, equity: equity, policy: policy, openRisk: openRisk).contains { $0.level == .block } ? nil : s
        }
        let resolved = resolve(raw, market: m, equity: equity, policy: policy).intent
        guard let s0 = evaluate(resolved, market: m).sizing, passes(raw) == nil else { return nil }
        let byMoney = raw.amountUSDC != nil && raw.riskUSD == nil
        let tooSmall = s0.notional < minNotional
        // ponytail: walks 3% steps up or 5% down (at most 60); a closed form would need fees and size steps inverted.
        if byMoney, let money = raw.amountUSDC {
            var a = tooSmall ? (minNotional / (resolved.leverage ?? 1)).rounded(1, .up) : money
            for _ in 0..<60 where a > 0 {
                var j = raw
                j.amountUSDC = a
                if let s = passes(j) { return a == money ? nil : ("\(Fmt.num(a)) USDC", s) }
                a = (tooSmall ? a * Decimal(string: "1.03")! : a * Decimal(string: "0.95")!).rounded(1, tooSmall ? .up : .down)
            }
            return nil
        }
        guard let risk = resolved.riskUSD else { return nil }
        var r = tooSmall ? (risk * minNotional / max(s0.notional, Decimal(string: "0.01")!)).rounded(2, .up) : risk
        for _ in 0..<60 where r > 0 {
            var j = raw
            j.riskUSD = r
            if let s = passes(j) { return r == risk ? nil : ("risk $\(Fmt.num(r))", s) }
            r = (tooSmall ? r * Decimal(string: "1.03")! : r * Decimal(string: "0.95")!).rounded(2, tooSmall ? .up : .down)
        }
        return nil
    }

    public static func fittingRisk(_ i: TradeIntent, market m: Market, equity: Decimal?, policy: RiskPolicy,
                                   openRisk: Decimal?, available: Decimal?) -> Decimal? {
        guard let asked = i.riskUSD else { return nil }
        var r = asked
        if let equity, equity > 0 {
            r = min(r, equity * policy.maxRiskPct / 100)
            if let open = openRisk, open > 0 { r = min(r, equity * policy.maxOpenRiskPct / 100 - open) }
        }
        r = r.rounded(2, .down)
        // ponytail: steps down 5% at a time; a closed form would need the fee and size-step rounding inverted.
        for _ in 0..<80 where r > 0 {
            var j = i
            j.riskUSD = r
            let e = evaluate(j, market: m)
            guard let s = e.sizing, s.notional >= minNotional else { return nil }
            let fits = available.map { s.margin + s.feesUSD <= $0 } ?? true
            let rules = RiskRules.check(j, s, market: m, equity: equity, policy: policy, openRisk: openRisk)
            if e.problem == nil, fits, !rules.contains(where: { $0.level == .block }) { return r < asked ? r : nil }
            r = (r * Decimal(string: "0.95")!).rounded(2, .down)
        }
        return nil
    }
}

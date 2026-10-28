import Foundation
import IhatecryptoCore
import TradeExecution

/// What a plan still needs, and what «by the book» would make of a trade: the chat asks for the missing risk and stop
/// with these numbers, and answers «should I short oil now?» with them.
extension Workspace {
    /// What stops Execute for lack of details: nil when the plan has a risk and a stop (its own or by the book).
    var missingDetails: String? {
        guard intent != TradeIntent(), !managesPosition else { return nil }
        let r = resolved.intent
        // A hedge is sized by the coins held: what it waits for is their balance, not a risk.
        let parts = [r.isHedge ? r.hedge == nil ? "the balance of \(r.asset)" : nil : r.riskUSD == nil ? "the risk in $" : nil,
                     r.stopPct == nil ? "a stop" : nil].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " and ")
    }

    /// The trade by the book (stop from 4h volatility, take at 2R, 1 % of capital at risk) in numbers, and what the
    /// rules say about entering it now: the day's move, funding, liquidation, the risk limits.
    func bookPlan(asset: String, side: Side) -> (line: String, advice: [Advice])? {
        guard var m = markets[asset] else { return nil }
        m.atrPct = atr[asset]
        var trial = TradeIntent()
        trial.asset = asset
        trial.side = side
        trial.autoRules = true
        let r = Plan.resolve(trial, market: m, equity: equity, policy: policy).intent
        guard let s = Plan.evaluate(r, market: m).sizing, let stop = r.stopPct else { return nil }
        var line = "risk \(Fmt.usd(s.lossAtStopUSD)), stop \(Fmt.pct(stop)) at \(Fmt.price(s.stopPrice))"
        if let take = r.takePct, let tp = s.takePrice, let p = s.profitAtTakeUSD {
            line += ", take \(Fmt.pct(take)) at \(Fmt.price(tp)) → +\(Fmt.usd(p))"
        }
        let advice = RiskRules.check(r, s, market: m, equity: equity, policy: policy,
                                     openRisk: HyperliquidAccount.riskAtStops(positions: positions, orders: orders, spot: shortsCovered))
        return (line, advice)
    }

    /// The plan checked item by item after every message about it, the way the canvas checks it: market, direction,
    /// risk, stop, take, entry, size, money, the rules and the result, then the one next step. What's missing carries
    /// its by-the-book value. Lines start with ✓ ✗ ⚠ · and the chat draws them as a checklist card.
    var planChecklist: String {
        guard intent != TradeIntent(), !managesPosition else { return "" }
        let r = resolved.intent, sizing = evaluation.sizing
        var trial = intent
        trial.autoRules = true
        let book = Plan.resolve(trial, market: market, equity: equity, policy: policy).intent
        func share(_ v: Decimal) -> String { equity.map { $0 > 0 ? " (\(Fmt.pct((v / $0 * 100).rounded(1, .plain))) of capital)" : "" } ?? "" }
        var out: [String] = []

        guard let m = market else {
            return "✗ Market: \(IntentParser.pretty(r.asset)) has no price yet\n· Direction: \(r.side == .long ? "long" : "short")\n· The rest is checked once the price loads."
        }
        let entry = r.entryPx ?? m.markPx
        var marketLine = "✓ Market: \(IntentParser.pretty(r.asset)) \(Fmt.price(m.markPx))"
        if let prev = m.prevDayPx, prev > 0 { marketLine += ", 24h \(signedPct((m.markPx - prev) / prev * 100))" }
        out.append(marketLine)
        if r.isHedge {
            let held = spotHoldings?[r.asset]
            out.append(r.hedge.map { "✓ Hedge: short \(Fmt.num(sizing?.size ?? $0, maxFraction: m.szDecimals)) \(m.name)" + (held.map { " of the \(Fmt.num($0, maxFraction: 6)) held" } ?? "") }
                       ?? "✗ Hedge: what the wallet holds of \(m.name) isn't read yet")
        } else {
            out.append("✓ Direction: \(r.side == .long ? "long" : "short")")
        }
        if r.isHedge {
        } else if let risk = r.riskUSD {
            let from = intent.riskUSD != nil ? "" : intent.amountUSDC.map { " from \(Fmt.num($0)) USDC in the trade" } ?? " by the book"
            out.append("✓ Risk: \(Fmt.usd(risk))\(share(risk))\(from)")
        } else {
            out.append("✗ Risk: not set" + (book.riskUSD.map { " — by the book \(Fmt.usd($0)), \(Fmt.pct(policy.defaultRiskPct)) of capital" } ?? ""))
        }
        if let stop = r.stopPct {
            let direction: Decimal = r.side == .short ? 1 : -1
            let price = sizing?.stopPrice ?? entry * (1 + direction * stop / 100)
            out.append("✓ Stop: \(Fmt.pct(stop)) at \(Fmt.price(price))\(r.isHedge ? intent.stopPct == nil ? ", only guards the margin from liquidation" : ", a rise past it ends the hedge" : intent.stopPct == nil ? " by the book" : "")")
        } else {
            out.append("✗ Stop: not set" + (book.stopPct.map { " — by the book \(Fmt.pct($0)), \(Fmt.num(policy.atrStopMultiple)) × 4h volatility" } ?? ""))
        }
        if r.isHedge {
            out.append("✓ Take: none, the hedge stays until you close it")
        } else if let take = r.takePct {
            out.append("✓ Take: \(Fmt.pct(take))" + (sizing?.takePrice.map { " at \(Fmt.price($0))" } ?? "") + (r.partialTake == true ? ", half at 1R" : ""))
        } else {
            out.append("· Take: none — by the book \(Fmt.num(policy.defaultRewardRisk))R")
        }
        out.append(r.entryPx.map { "✓ Entry: limit \(Fmt.price($0))" + (r.pullbackPct.map { " (\(Fmt.pct($0)) pullback)" } ?? "") } ?? "✓ Entry: at market")
        if let s = sizing {
            out.append("\(evaluation.problem == nil ? "✓" : "✗") Size: \(Fmt.usd(s.notional)) · \(Fmt.num(r.leverage ?? 1))× · margin \(Fmt.usd(s.margin))"
                       + (evaluation.problem.map { ". \($0)" } ?? ""))
            // Where the margin comes from: Hyperliquid's free balance, topped up over the bridge from Solana, or straight from
            // the EVM network that has the USDC when Solana is short.
            let bridge = blocks.first { $0.id == "bridge" }, source = blocks.first { $0.id == "source" }
            let free = hl.map { Fmt.usd($0.withdrawable) } ?? "—"
            if source?.status == .blocked {
                out.append("✗ Money: \(source?.lines.last.map { "\($0.dropLast()) on \(fundingNet?.name ?? "Solana")." } ?? "not enough USDC on Solana")")
            } else if let bridge, bridge.status == .ready || bridge.status == .warning, let q = quote {
                out.append("✓ Money: \(free) free on Hyperliquid + \(Fmt.num(q.amountIn)) USDC from \(fundingNet?.name ?? "Solana") over \(q.provider) (≈ \(Fmt.num(q.amountOut)) arrive)"
                           + (fundingNet == nil ? "" : ", straight: Solana has only \(Fmt.num(solana?.usdc ?? 0))"))
                // The bridge's warning (it costs more than the take earns) belongs next to the money.
                if bridge.status == .warning, let costly = bridge.lines.first(where: { $0.hasPrefix("The bridge costs more") }) { out.append("⚠ Money: " + costly) }
            } else if let bridge, bridge.status == .blocked, let why = bridge.lines.first {
                out.append("✗ Money: \(why)")
            } else if bridge?.status == .noData {
                out.append("· Money: \(free) free on Hyperliquid; the bridge quote from Solana is loading")
            } else if bridge?.status == .info {
                out.append("✓ Money: \(free) free on Hyperliquid covers the margin")
            }
            let advice = RiskRules.check(r, s, market: m, equity: equity, policy: policy,
                                         openRisk: HyperliquidAccount.riskAtStops(positions: positions, orders: orders, spot: shortsCovered))
            let problems = advice.filter { $0.level != .ok }
            out += problems.map { "\($0.level == .block ? "✗" : "⚠") Rules: \($0.text)" }
            if problems.isEmpty { out.append("✓ Rules: all met") }
            var result = r.isHedge
                ? "✓ Result: a fall in \(m.name) is covered, a rise given up; ≈ \(Fmt.usd(s.feesUSD)) fees + funding"
                : "✓ Result: at the stop −\(Fmt.usd(s.lossAtStopUSD))\(share(s.lossAtStopUSD))"
            if !r.isHedge, let p = s.profitAtTakeUSD, s.lossAtStopUSD > 0 {
                result += ", at the take +\(Fmt.usd(p)), 1:\(Fmt.num((p / s.lossAtStopUSD).rounded(1, .plain)))"
            }
            out.append(result)
        } else {
            out.append("· Size, money and rules: checked once the risk and the stop are set")
        }

        let next: String
        if r.isHedge, r.hedge == nil {
            next = "The hedge's size comes once the wallet's balances are read."
        } else if missingDetails != nil {
            next = "Tap “By the book” or say your own, e.g. “risk $1 stop 3%”."
        } else if out.contains(where: { $0.hasPrefix("✗") }) {
            next = "Fix the ✗ above — for example a smaller risk or a wider stop — and I'll check again."
        } else {
            next = "Ready: press Execute — you'll see the simulation before anything is signed."
        }
        return out.joined(separator: "\n") + "\n" + next
    }
}

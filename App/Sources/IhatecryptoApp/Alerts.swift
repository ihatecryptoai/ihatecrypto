import IhatecryptoCore
import SignerAPI
import SwiftUI
import WalletCore

extension Workspace {
    /// «alert me when ETH is above 2,800», «cancel alerts».
    func alertCommand(_ command: AlertCommand, note: String? = nil) {
        switch command {
        case .clear:
            let n = alerts.count
            alerts = []
            reply(n == 0 ? "There were no price alerts." : "Price alerts cleared (\(n)).", note: note)
        case .set(let coin, let price, let above, let steps):
            let name = IntentParser.pretty(coin)
            if let unknown = steps.first(where: { !knows($0) }) {
                reply(say("I can set the alert, but ihatecrypto can’t do “\(unknown)” yet — nothing was set.",
                          "Алерт поставить могу, но «\(unknown)» ihatecrypto пока не умеет — ничего не поставлено."), note: note)
                return
            }
            guard let mark = markets[coin]?.markPx else { reply("No price for \(name) right now — try again in a moment.", note: note); return }
            let up = above ?? (price > mark)
            if up ? mark >= price : mark <= price {
                reply("\(name) is already \(up ? "above" : "below") \(Fmt.price(price)): now \(Fmt.price(mark)).", note: note)
                return
            }
            var alert = PriceAlert(coin: coin, above: up, price: price, steps: steps.isEmpty ? nil : steps)
            if SignerClient.untrusted { alert.untrusted = true }
            // «add it to the plan»: a step that is this canvas's plan (its market and side) ties the alert to the plan.
            // It becomes the plan's first step, one per plan: a new level replaces the old.
            let tied = intent != TradeIntent() && !managesPosition && steps.contains { planStep($0).map { $0.asset == intent.asset && $0.side == intent.side } == true }
            if tied {
                alert.plan = canvasID
                alerts.removeAll { $0.plan == canvasID }
            }
            alerts.append(alert)
            if !tied { withAnimation(StudioView.spring) { showAlerts = true } }
            UserDefaults.standard.set("plan", forKey: "canvasMode")
            let away = ((price - mark) / mark * 100).rounded(1, .plain)
            if tied {
                reply(say("🔔 Added to the plan as its first step: when \(name) is \(up ? "above" : "below") \(Fmt.price(price)) (now \(Fmt.price(mark)), \(Fmt.num(abs(away)))% away), this plan comes back to you to confirm, with any changes you make to it until then.",
                          "🔔 Добавил в план первым шагом: когда \(name) будет \(up ? "выше" : "ниже") \(Fmt.price(price)) (сейчас \(Fmt.price(mark)), \(Fmt.num(abs(away)))% до уровня), этот план придёт тебе на подтверждение — со всеми правками, что внесёшь до тех пор."), note: note)
                return
            }
            let then = steps.isEmpty ? "" : say(" Then I’ll prepare “\(steps.joined(separator: ", then "))” and send it to you to confirm.",
                                               " Тогда подготовлю «\(steps.joined(separator: ", потом "))» и пришлю на подтверждение.")
            reply(say("🔔 On the canvas: \(name) \(up ? "above" : "below") \(Fmt.price(price)) — now \(Fmt.price(mark)), \(Fmt.num(abs(away)))% \(up ? "higher" : "lower").",
                      "🔔 На канвасе: \(name) \(up ? "выше" : "ниже") \(Fmt.price(price)) — сейчас \(Fmt.price(mark)), на \(Fmt.num(abs(away)))% \(up ? "выше" : "ниже").") + then, note: note)
        }
    }

    /// A step that is a trade plan («long SOL risk $0.3 …»), read as that plan; nil for any other command.
    func planStep(_ step: String) -> TradeIntent? {
        let known = Set(markets.keys)
        return IntentParser.startsPlan(step, markets: known) ? IntentParser.apply(step, to: TradeIntent(), markets: known) : nil
    }

    /// The alert tied to this canvas's plan while the plan is still that trade (its market and side).
    var planAlert: PriceAlert? {
        guard intent != TradeIntent() else { return nil }
        return alerts.first { a in a.plan == canvasID && (a.steps ?? []).contains { planStep($0).map { $0.asset == intent.asset && $0.side == intent.side } == true } }
    }

    /// The plan's alert as its first route card: the level, how far the price is, and what happens when it gets there.
    var planAlertStep: ActivityBubble? {
        guard let a = planAlert else { return nil }
        let name = IntentParser.pretty(a.coin).components(separatedBy: " (").first ?? a.coin
        var lines: [String] = []
        if let mark = markets[a.coin]?.markPx {
            lines = ["now\t\(Fmt.price(mark))", "away\t\(Fmt.num(((a.price - mark) / mark * 100).rounded(1, .plain)))%"]
        }
        lines.append("then this plan comes to you to confirm")
        return ActivityBubble(kind: "step", label: "WHEN · PRICE \(a.above ? "RISES" : "FALLS") TO", title: "\(name) \(Fmt.price(a.price))", lines: lines,
                              status: "watching", asset: a.coin, logo: TokenLogo.name(forAsset: a.coin) ?? "hype", fill: .check)
    }

    /// Runs with every refresh: an alert that fires sends one notification and is done.
    func checkAlerts() {
        for a in alerts {
            guard a.paused != true, let mark = markets[a.coin]?.markPx, a.fires(at: mark) else { continue }
            alerts.removeAll { $0.id == a.id }
            // Its steps run as a task due now: on their own canvas once nothing else is going on, and a move or a plan
            // waits there for Confirm with a notification (runDueTask).
            if var steps = a.steps {
                // Tied to a plan: that plan as it is on its canvas now, edits included — unless it's no longer that trade.
                if let id = a.plan, let plan = id == canvasID ? intent : canvases.first(where: { $0.id == id })?.intent {
                    steps = steps.map { step in planStep(step).map { $0.asset == plan.asset && $0.side == plan.side } == true ? plan.command : step }
                }
                let because = say("\(IntentParser.pretty(a.coin)) went \(a.above ? "above" : "below") \(Fmt.price(a.price))",
                                  "\(IntentParser.pretty(a.coin)) \(a.above ? "выше" : "ниже") \(Fmt.price(a.price))")
                var task = ScheduledTask(steps: steps, next: Date(), every: nil, because: because)
                task.untrusted = a.untrusted
                // As the alert promised: its moves come to the user to confirm, never off the automation wallet by themselves.
                task.fromActive = true
                tasks.append(task)
                continue
            }
            let text = say("\(IntentParser.pretty(a.coin)) is \(a.above ? "above" : "below") \(Fmt.price(a.price)): now \(Fmt.price(mark)).",
                           "\(IntentParser.pretty(a.coin)) \(a.above ? "выше" : "ниже") \(Fmt.price(a.price)): сейчас \(Fmt.price(mark)).")
            notify(.alerts, "🔔 \(IntentParser.pretty(a.coin))", text)
            reply(text + say(" The alert is done; set another any time.", " Алерт сработал; поставь новый, когда захочешь."), note: "price alert")
        }
    }
}

extension PriceAlert {
    /// Its steps read as the task it becomes when it fires (checkAlerts): from the wallet that's on, each move confirmed.
    var task: ScheduledTask {
        var t = ScheduledTask(steps: steps ?? [], next: .distantFuture, every: nil)
        t.fromActive = true
        t.untrusted = untrusted
        return t
    }
}

/// The alerts as a board on the canvas, each one as the blocks the agent built it from: the level, every step it
/// prepares when the price gets there, and the notification that asks for the user's Confirm, arrows between them.
struct AlertsBoard: View {
    @Environment(Workspace.self) private var ws

    private struct Block: Identifiable {
        let id: String
        var coin: String?
        var icon: String?
        let label: String, title: String
        var line: String?
        var remove: (() -> Void)?
    }

    var body: some View {
        if ws.alerts.contains(where: { $0.id != ws.planAlert?.id }) { board }
    }

    private var board: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Image(systemName: "bell.badge.fill").foregroundStyle(.orange)
                Text("Price alerts").font(.ui(15, .bold))
                Spacer()
                Button { withAnimation(StudioView.spring) { ws.showAlerts = false } } label: { Image(systemName: "xmark") }
                    .buttonStyle(.hoverIcon).help("Hide the board; the alerts stay")
            }
            // One tied to this canvas's plan is drawn as that plan's first step instead.
            ForEach(ws.alerts.filter { $0.id != ws.planAlert?.id }) { chain(blocks($0)) }
            Text("Checked every 10 s while ihatecrypto is open or in the menu bar. Nothing moves until you confirm it.")
                .font(.ui(10)).foregroundStyle(.tertiary)
        }
        .padding(16)
        .frame(width: 470, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 24).fill(Brand.card).shadow(color: .black.opacity(0.06), radius: 14, y: 8))
        .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(Brand.cardBorder))
        .transition(.opacity)
    }

    private func blocks(_ a: PriceAlert) -> [Block] {
        let name = IntentParser.pretty(a.coin).components(separatedBy: " (").first ?? a.coin
        let now = (ws.markets[a.coin]?.markPx).map { "now \(Fmt.price($0)) · \(Fmt.num(abs(((a.price - $0) / $0 * 100).rounded(1, .plain))))% away" }
        let markets = Set(ws.markets.keys)
        let steps = (a.steps ?? []).enumerated().map { k, step -> Block in
            // A trade plan reads as the plan it becomes; anything else as the command it is.
            if IntentParser.startsPlan(step, markets: markets), let t = IntentParser.apply(step, to: TradeIntent(), markets: markets) {
                let rest = t.changes(from: TradeIntent()).filter { !$0.hasPrefix("market") && $0 != "long" && $0 != "short" && !$0.hasPrefix("leverage") }
                return Block(id: "\(a.id)-\(k)", coin: t.asset, label: "THEN · PLAN ON HYPERLIQUID",
                             title: "\(t.side == .long ? "Long" : "Short") \(IntentParser.pretty(t.asset))\(t.leverage.map { " ×" + Fmt.num($0) } ?? "")",
                             line: rest.isEmpty ? nil : rest.joined(separator: " · "))
            }
            return Block(id: "\(a.id)-\(k)", icon: "bolt.fill", label: "THEN", title: ScheduleCanvas.readable(step))
        }
        return [Block(id: "\(a.id)-when", coin: a.coin, label: "WHEN · PRICE", title: "\(name) \(a.above ? "above" : "below") \(Fmt.price(a.price))",
                      line: now) { withAnimation(.snappy) { ws.alerts.removeAll { $0.id == a.id } } }]
            + steps
            + [Block(id: "\(a.id)-you", icon: "bell.and.waves.left.and.right", label: "YOU · NOTIFICATION",
                     title: steps.isEmpty ? "A notification" : "A notification, then your Confirm",
                     line: steps.isEmpty ? nil : "Prepared on its own canvas; nothing moves without \(Presence.name).")]
    }

    private func chain(_ blocks: [Block]) -> some View {
        VStack(spacing: 4) {
            ForEach(Array(blocks.enumerated()), id: \.element.id) { i, b in
                if i > 0 { Image(systemName: "arrow.down").font(.system(size: 13, weight: .semibold)).foregroundStyle(.secondary).accessibilityHidden(true) }
                block(b, first: i == 0)
            }
        }
    }

    /// The level is solid, what follows it is still to come: a dashed edge.
    private func block(_ b: Block, first: Bool) -> some View {
        let shape = RoundedRectangle(cornerRadius: 16, style: .continuous)
        return HStack(spacing: 12) {
            Group {
                if let coin = b.coin { AssetBadge(coin: coin, size: 32, dark: true) }
                else { Image(systemName: b.icon ?? "bolt.fill").font(.system(size: 14, weight: .semibold)).foregroundStyle(.orange) }
            }
            .frame(width: 36, height: 36)
            .background(RoundedRectangle(cornerRadius: 11).fill(Color.orange.opacity(b.coin == nil ? 0.12 : 0)))
            VStack(alignment: .leading, spacing: 2) {
                Text(b.label).font(.ui(9.5, .heavy)).tracking(0.7).foregroundStyle(.secondary).lineLimit(1)
                Text(b.title).font(.ui(13, .semibold)).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                if let line = b.line {
                    Text(line).font(.ui(11)).monospacedDigit().foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 4)
            if let remove = b.remove {
                Button(action: remove) { Image(systemName: "trash") }.buttonStyle(.hoverIcon).help("Remove the alert")
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(shape.fill(first ? Color.orange.opacity(0.08) : Color.clear))
        .overlay(shape.strokeBorder(first ? Color.orange.opacity(0.45) : Color.secondary.opacity(0.35),
                                    style: StrokeStyle(lineWidth: 1.2, dash: first ? [] : [5, 4])))
        .accessibilityElement(children: .combine)
    }
}

/// Alerts waiting in the portfolio frame, each with its distance to the price and a way to drop it.
struct AlertsCard: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        if !ws.alerts.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Label("Price alerts", systemImage: "bell.badge").font(.ui(10, .semibold))
                ForEach(ws.alerts) { a in
                    HStack(spacing: 6) {
                        AssetBadge(coin: a.coin, size: 18)
                        Text("\(IntentParser.pretty(a.coin).components(separatedBy: " (").first ?? a.coin) \(a.above ? "≥" : "≤") \(Fmt.price(a.price))")
                            .font(.ui(10, .semibold)).lineLimit(1)
                        Spacer(minLength: 4)
                        if let mark = ws.markets[a.coin]?.markPx {
                            Text("\(Fmt.num(((a.price - mark) / mark * 100).rounded(1, .plain)))%").font(.ui(10)).monospacedDigit().foregroundStyle(.secondary)
                        }
                        Button { withAnimation(.snappy) { ws.alerts.removeAll { $0.id == a.id } } } label: { Image(systemName: "xmark") }
                            .buttonStyle(.hoverIcon).font(.ui(10)).help("Remove the alert")
                    }
                }
            }
            .padding(11)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 16).strokeBorder(Brand.cardBorder))
        }
    }
}

/// The portfolio's value over time: a point every ten minutes, kept for 30 days (equity.json).
nonisolated struct EquityPoint: Codable, Equatable {
    let at: Date
    let usd: Decimal
    /// The wallet it was measured for (P/L); nil in points kept before wallets were told apart.
    var wallet: String? = nil
}

extension Workspace {
    /// Only when both main balances are in: a half-loaded portfolio would draw a false dip.
    func recordEquity() {
        guard solana != nil, hl != nil, let e = equity, e > 0 else { return }
        if let last = equityHistory.last, Date().timeIntervalSince(last.at) < 600 { return }
        let cutoff = Date().addingTimeInterval(-30 * 86_400)
        equityHistory = equityHistory.filter { $0.at > cutoff } + [EquityPoint(at: Date(), usd: e, wallet: pnlWallet)]
    }

    /// The last `days` of history plus the value now. «Now» is when the balances were read: stamped with the clock, the
    /// series was new on every pass and the sparkline redrew each time.
    func equitySeries(days: Double = 7) -> [EquityPoint] {
        let since = Date().addingTimeInterval(-days * 86_400)
        return equityHistory.filter { $0.at >= since } + (equity.map { [EquityPoint(at: updatedAt ?? Date(), usd: $0)] } ?? [])
    }
}

/// A small line of the portfolio's value; green when it grew over the period, red when it fell.
struct EquitySparkline: View {
    let points: [EquityPoint]
    var height: CGFloat = 26

    var body: some View {
        let values = points.map { NSDecimalNumber(decimal: $0.usd).doubleValue }
        let up = (values.last ?? 0) >= (values.first ?? 0)
        Canvas { context, size in
            guard values.count > 1, let lo = values.min(), let hi = values.max() else { return }
            let span = max(hi - lo, max(hi * 0.002, 0.01))
            let start = points.first!.at.timeIntervalSince1970, total = max(points.last!.at.timeIntervalSince1970 - start, 1)
            var line = Path()
            for (i, p) in points.enumerated() {
                let pt = CGPoint(x: size.width * (p.at.timeIntervalSince1970 - start) / total, y: size.height * (1 - (values[i] - lo) / span))
                i == 0 ? line.move(to: pt) : line.addLine(to: pt)
            }
            var area = line
            area.addLine(to: CGPoint(x: size.width, y: size.height))
            area.addLine(to: CGPoint(x: 0, y: size.height))
            area.closeSubpath()
            let color = up ? Brand.profit : Brand.loss
            context.fill(area, with: .linearGradient(Gradient(colors: [color.opacity(0.25), color.opacity(0)]),
                                                     startPoint: .zero, endPoint: CGPoint(x: 0, y: size.height)))
            context.stroke(line, with: .color(color), style: StrokeStyle(lineWidth: 1.6, lineCap: .round, lineJoin: .round))
        }
        .frame(height: height)
        .accessibilityLabel("Portfolio value over the week")
    }
}

import Foundation
import IhatecryptoCore

/// «Tell me when ETH is above 4000», and «when ETH is below 2,680: long ETH leverage 3x»: the level is checked against
/// Hyperliquid's mark every 10 s while ihatecrypto is open. A plain alert says so once and is done; one with steps
/// runs them as a task due now, so a move among them becomes a card that waits for Confirm, never a signature by itself.
extension Engine {
    func alertText(_ a: PriceAlert) -> String {
        "\(IntentParser.pretty(a.coin)) \(a.above ? say("above", "выше") : say("below", "ниже")) \(Fmt.price(a.price))"
            + (a.steps.map { ": " + $0.joined(separator: " then ") } ?? "")
    }

    func alert(_ command: AlertCommand, note: String?) async {
        switch command {
        case .clear:
            let n = alerts.count
            alerts = []
            reply(n == 0 ? say("There were no price alerts.", "Ценовых оповещений не было.") : say("Price alerts cleared (\(n)).", "Ценовые оповещения сняты (\(n))."), note: note)
        case .set(let coin, let price, let above, let steps):
            if markets.isEmpty { markets = (try? await Hyperliquid.markets()) ?? [:] }
            guard let mark = markets[coin]?.markPx else {
                // No markets at all: Hyperliquid didn't answer, which isn't «no market» (the Mac: «No price … right now»).
                reply(markets.isEmpty
                      ? say("No price for \(IntentParser.pretty(coin)) right now — Hyperliquid didn't answer. Nothing was set; try again in a moment.",
                            "Цены \(IntentParser.pretty(coin)) сейчас нет — Hyperliquid не ответил. Оповещение не поставлено, попробуй через минуту.")
                      : say("Hyperliquid has no market for \(coin), so there's no price to watch.", "На Hyperliquid нет рынка \(coin) — нечего отслеживать."), note: note)
                return
            }
            // A step must be something a chain runs (Chains.knows, the Mac's Alerts.swift): it fires as a task due now.
            if let bad = steps.first(where: { !knows($0) }) {
                reply(say("When it fires, “\(bad)” isn't something the Windows app can do yet. Nothing was set.", "«\(bad)» Windows-версия пока не умеет. Оповещение не поставлено."), note: note)
                return
            }
            var a = PriceAlert(coin: coin, above: above ?? (price > mark), price: price, steps: steps.isEmpty ? nil : steps)
            if a.fires(at: mark) {
                reply(say("\(IntentParser.pretty(coin)) is already at \(Fmt.price(mark)): that level is passed.", "\(IntentParser.pretty(coin)) уже \(Fmt.price(mark)): этот уровень пройден."), note: note)
                return
            }
            // «Add it to the plan»: a step that is this canvas's plan (its market and side) ties the alert to the plan.
            // It becomes the plan's first step, one per plan: a new level replaces the old.
            let managed = tradedPlan == intent || positions.contains { $0.coin == intent.asset }
            if intent != TradeIntent(), !managed, samePlan(steps, intent), let id = UUID(uuidString: canvasID) {
                a.plan = id
                alerts.removeAll { $0.plan == id }
                alerts.append(a)
                if let step = planAlertStep {
                    updateBoard(planBoard) { $0.steps = [step] + ($0.steps ?? []).filter { $0.status != "watching" } }
                }
                let name = IntentParser.pretty(coin).components(separatedBy: " (").first ?? coin
                let away = Fmt.num(abs(((price - mark) / mark * 100).rounded(1, .plain)))
                reply(say("🔔 Added to the plan as its first step: when \(name) is \(a.above ? "above" : "below") \(Fmt.price(price)) (now \(Fmt.price(mark)), \(away)% away), this plan comes back to you to confirm, with any changes you make to it until then.",
                          "🔔 Добавил в план первым шагом: когда \(name) будет \(a.above ? "выше" : "ниже") \(Fmt.price(price)) (сейчас \(Fmt.price(mark)), \(away)% до уровня), этот план придёт тебе на подтверждение — со всеми правками, что внесёшь до тех пор."), note: note)
                return
            }
            alerts.append(a)
            pin(Board(kind: "alerts", title: "alerts"))
            reply(say("Set: \(alertText(a)) (now \(Fmt.price(mark))). It's checked every 10 s while ihatecrypto is open",
                      "Поставил: \(alertText(a)) (сейчас \(Fmt.price(mark))). Проверяю каждые 10 с, пока ihatecrypto открыт")
                  + (a.steps == nil ? "." : say("; when it fires, its moves come as cards for Confirm.", "; когда сработает, действия придут карточками на подтверждение.")), note: note)
        }
    }

    /// The alerts the new marks crossed: said in the chat, and their steps read.
    func checkAlerts() async {
        let due = alerts.filter { a in markets[a.coin].map { a.fires(at: $0.markPx) } ?? false }
        guard !due.isEmpty else { return }
        alerts.removeAll { a in due.contains { $0.id == a.id } }
        for a in due {
            // Its steps run as a task due now, as on the Mac: once nothing else is going on (never over a card waiting
            // for Confirm), from the wallet that's on, each move waiting for Confirm — never off the automation wallet.
            if var steps = a.steps {
                // Tied to a plan: that plan as it is now on its own canvas, edits included (the open one's live, another's as
                // saved, the Mac's CanvasSession) — unless it's no longer that trade.
                if let id = a.plan, let plan = id.uuidString == canvasID ? intent : canvases.first(where: { $0.id == id.uuidString })?.intent,
                   plan != TradeIntent() {
                    steps = steps.map { step in planStep(step).map { $0.asset == plan.asset && $0.side == plan.side } == true ? plan.command : step }
                }
                var task = ScheduledTask(steps: steps, next: Date(), every: nil,
                                         because: "\(IntentParser.pretty(a.coin)) \(a.above ? say("went above", "выше") : say("went below", "ниже")) \(Fmt.price(a.price))")
                task.fromActive = true
                tasks.append(task)
                continue
            }
            let text = "🔔 " + alertText(a) + say(" — now ", " — сейчас ") + Fmt.price(markets[a.coin]?.markPx ?? a.price)
            notify(.alerts, say("Price alert", "Ценовое оповещение"), text)
            reply(text + say(" The alert is done; set another any time.", " Оповещение сработало; поставь новое, когда захочешь."), note: "price alert")
        }
        await runDueTasks()
    }

    /// A step that is a trade plan («long SOL risk $0.3 …»), read as that plan; nil for any other command.
    func planStep(_ step: String) -> TradeIntent? {
        let known = Set(markets.keys)
        return IntentParser.startsPlan(step, markets: known) ? IntentParser.apply(step, to: TradeIntent(), markets: known) : nil
    }

    /// Whether one of the steps is this trade (its market and side).
    func samePlan(_ steps: [String]?, _ plan: TradeIntent) -> Bool {
        (steps ?? []).contains { planStep($0).map { $0.asset == plan.asset && $0.side == plan.side } == true }
    }

    /// The alert tied to this canvas's plan while the plan is still that trade: drawn as its first step, not in the
    /// alerts block, and gone with «clear the plan».
    var planAlert: PriceAlert? {
        guard intent != TradeIntent(), let id = UUID(uuidString: canvasID) else { return nil }
        return alerts.first { $0.plan == id && samePlan($0.steps, intent) }
    }

    /// The plan's alert as its first route step: the level, how far the price is, and what happens when it gets there.
    var planAlertStep: Board.Step? {
        guard let a = planAlert else { return nil }
        let name = IntentParser.pretty(a.coin).components(separatedBy: " (").first ?? a.coin
        var lines: [String] = []
        if let mark = markets[a.coin]?.markPx, mark > 0 {
            lines = [say("now", "сейчас") + "\t" + Fmt.price(mark),
                     say("away", "до уровня") + "\t" + Fmt.num(((a.price - mark) / mark * 100).rounded(1, .plain)) + "%"]
        }
        lines.append(say("then this plan comes to you to confirm", "потом этот план придёт тебе на подтверждение"))
        // The label in English as the route's other steps (it picks the card's picture); the lines in the reply language.
        return Board.Step(label: "WHEN · PRICE \(a.above ? "RISES" : "FALLS") TO",
                          title: "\(name) \(Fmt.price(a.price))", lines: lines, status: "watching")
    }

    /// For the second check: a plan waiting for its alert enters at the price then, not today's — without this line the
    /// check reads «long below 117.44» against today's entry and cries mismatch.
    func planAlertFacts() -> [String] {
        guard let a = planAlert else { return [] }
        return ["It waits for a price alert: when \(intent.asset) \(a.above ? "rises" : "falls") to \(Fmt.price(a.price)), the plan comes to the user to "
                + "confirm and enters at the market price then (≈ \(Fmt.price(a.price))); the entry above is today's price, for sizing."]
    }

    /// The alerts' prices every 10 s while there are any, as the Mac checks them; the block's «now · away» follows.
    static var watchingAlerts = false
    func watchAlerts() {
        guard !Self.watchingAlerts else { return }
        Self.watchingAlerts = true
        Task {
            while true {
                try? await Task.sleep(for: .seconds(10))
                // Exit rules too, as the Mac's watch(): a trailing stop or the move to breakeven lags 10 s, not a minute.
                guard !alerts.isEmpty || !exitRules.isEmpty, let m = try? await Hyperliquid.markets() else { continue }
                markets = m
                await checkAlerts()
                await guardNow()
                changed()
            }
        }
    }
}

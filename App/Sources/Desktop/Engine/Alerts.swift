import Foundation
import IhatecryptoCore

/// «Tell me when ETH is above 4000», and «when ETH is below 2,680: long ETH leverage 3x»: the level is checked against
/// Hyperliquid's mark at every refresh while ihatecrypto is open. A plain alert says so once and is done; one with steps
/// reads them as if typed, so a move among them becomes a card that waits for Confirm, never a signature by itself.
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
                reply(say("Hyperliquid has no market for \(coin), so there's no price to watch.", "На Hyperliquid нет рынка \(coin) — нечего отслеживать."), note: note)
                return
            }
            // A step must be something this app runs: a money command, a trade or another alert.
            if let bad = steps.first(where: { WalletCommand.parse($0) == nil && IntentParser.apply($0, to: TradeIntent(), markets: Set(markets.keys)) == nil && PositionCommand.parse($0) == nil }) {
                reply(say("When it fires, “\(bad)” isn't something the Windows app can do yet. Nothing was set.", "«\(bad)» Windows-версия пока не умеет. Оповещение не поставлено."), note: note)
                return
            }
            let a = PriceAlert(coin: coin, above: above ?? (price > mark), price: price, steps: steps.isEmpty ? nil : steps)
            if a.fires(at: mark) {
                reply(say("\(IntentParser.pretty(coin)) is already at \(Fmt.price(mark)): that level is passed.", "\(IntentParser.pretty(coin)) уже \(Fmt.price(mark)): этот уровень пройден."), note: note)
                return
            }
            alerts.append(a)
            reply(say("Set: \(alertText(a)) (now \(Fmt.price(mark))). It's checked every minute while ihatecrypto is open",
                      "Поставил: \(alertText(a)) (сейчас \(Fmt.price(mark))). Проверяю раз в минуту, пока ihatecrypto открыт")
                  + (a.steps == nil ? "." : say("; when it fires, its moves come as cards for Confirm.", "; когда сработает, действия придут карточками на подтверждение.")), note: note)
        }
    }

    /// The alerts the new marks crossed: said in the chat, and their steps read.
    func checkAlerts() async {
        let due = alerts.filter { a in markets[a.coin].map { a.fires(at: $0.markPx) } ?? false }
        guard !due.isEmpty else { return }
        alerts.removeAll { a in due.contains { $0.id == a.id } }
        for a in due {
            let text = "🔔 " + alertText(a) + say(" — now ", " — сейчас ") + Fmt.price(markets[a.coin]?.markPx ?? a.price)
            out.event("notify", ["title": say("Price alert", "Ценовое оповещение"), "body": text])
            reply(text, note: "price alert")
            for step in a.steps ?? [] where !(await runCanonical(step, userText: step)) {
                reply(say("“\(step)” didn't run.", "«\(step)» не выполнилось."), note: "price alert")
            }
        }
    }
}

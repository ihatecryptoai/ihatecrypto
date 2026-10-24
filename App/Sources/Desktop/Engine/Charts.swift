import Foundation
import IhatecryptoCore
import WalletCore

/// «chart ETH week», «what's moving»: the shared ChartCommand, drawn in the chat (the Mac draws it on the canvas).
extension Engine {
    /// `quiet`: the movers' board only, without its list in the chat (the model's answer that opened it says it).
    func chart(_ command: ChartCommand, note: String?, quiet: Bool = false) async {
        switch command {
        case .movers:
            if markets.isEmpty { markets = (try? await Hyperliquid.markets()) ?? [:] }
            let moving = markets.values.filter { $0.change24h != nil && $0.volume24h > 1_000_000 }.sorted { ($0.change24h ?? 0) > ($1.change24h ?? 0) }
            guard !moving.isEmpty else { reply(say("Markets aren't loaded yet.", "Рынки ещё не загрузились."), note: note); return }
            func line(_ m: Market) -> String { "\(IntentParser.pretty(m.name)) \(Fmt.price(m.markPx)) \(signed(m.change24h ?? 0))" }
            func mover(_ m: Market) -> BlockData.MoversCard.Mover {
                .init(coin: m.name, name: IntentParser.pretty(m.name).components(separatedBy: " (").first ?? m.name, price: Fmt.price(m.markPx), change: m.change24h ?? 0)
            }
            pin("result", say("Movers · 24 h", "Движение · 24 ч"), lines: moving.prefix(6).map(line) + ["—"] + moving.suffix(6).reversed().map(line),
                data: BlockData(movers: .init(up: moving.prefix(6).map(mover), down: moving.suffix(6).reversed().map(mover))))
            if quiet { return }
            reply(say("Up the most today: ", "Сильнее всего выросли: ") + moving.prefix(6).map(line).joined(separator: ", ")
                  + say(".\nDown the most: ", ".\nСильнее всего упали: ") + moving.suffix(6).reversed().map(line).joined(separator: ", ") + ".", note: note ?? "Hyperliquid")
        case .chart(let coin, let range):
            do {
                let line = try await chartLine(coin, range: range)
                let first = line.closes.first.map { Fmt.price(Decimal($0)) } ?? "", last = line.closes.last.map { Fmt.price(Decimal($0)) } ?? ""
                var m = Message(fromUser: false, text: "\(IntentParser.pretty(coin)) · \(range.title): \(first) → \(last) (\(line.change))", note: note ?? "Hyperliquid")
                placeChart(line)
                m.text += say(" — the chart is on the canvas.", " — график на холсте.")
                messages.append(m)
                saveChat()
                changed()
            } catch {
                reply(say("No chart for \(IntentParser.pretty(coin)) right now: ", "Графика \(IntentParser.pretty(coin)) сейчас нет: ") + error.localizedDescription, note: note)
            }
        }
    }

    /// A canvas has one chart, as the Mac's chart place: a new one — another coin or range — takes the place of the
    /// newest chart there, drawn anew and on top; charts drawn before stay as they were.
    @discardableResult
    func placeChart(_ line: ChartLine) -> String {
        if let old = boards.first(where: { $0.kind == "chart" }) {
            updateBoard(old.id) { $0.chart = line; $0.title = line.title; $0.at = Date(); $0.request = lastRequest }
            if let i = boards.firstIndex(where: { $0.id == old.id }) { boards.insert(boards.remove(at: i), at: 0) }
            return old.id
        }
        var b = Board(kind: "chart", title: line.title, chart: line)
        b.request = lastRequest
        return pin(b)
    }

    func signed(_ pct: Decimal) -> String { (pct >= 0 ? "+" : "") + Fmt.num(pct, maxFraction: 2) + "%" }
}

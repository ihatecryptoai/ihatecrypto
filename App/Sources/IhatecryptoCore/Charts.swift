import Foundation
import LLMCore

extension Hyperliquid {
    public struct Candle: Sendable, Equatable, Codable {
        public let time: Date
        public let open: Decimal
        public let high: Decimal
        public let low: Decimal
        public let close: Decimal

        public init(time: Date, open: Decimal, high: Decimal, low: Decimal, close: Decimal) {
            self.time = time; self.open = open; self.high = high; self.low = low; self.close = close
        }
    }

    /// Candles for a chart, oldest first. HIP-3 markets («xyz:CL») are asked by their full name.
    public static func candles(coin: String, range: ChartCommand.Range, now: Date = Date()) async throws -> [Candle] {
        let end = Int(now.timeIntervalSince1970 * 1000)
        let body: [String: Any] = ["type": "candleSnapshot", "req": ["coin": coin, "interval": range.interval, "startTime": end - range.span, "endTime": end]]
        return try parseCandles(try await HTTP.post(info, json: body))
    }

    /// Candles of any interval («1h», «4h», «1d») between two times, oldest first; Hyperliquid gives at most 5,000.
    public static func candles(coin: String, interval: String, from: Date, to: Date = Date()) async throws -> [Candle] {
        let body: [String: Any] = ["type": "candleSnapshot", "req": ["coin": coin, "interval": interval,
                                                                       "startTime": Int(from.timeIntervalSince1970 * 1000), "endTime": Int(to.timeIntervalSince1970 * 1000)]]
        return try parseCandles(try await HTTP.post(info, json: body))
    }

    public static func parseCandles(_ data: Data) throws -> [Candle] {
        struct Row: Decodable { let t: Int64; let o: String; let h: String; let l: String; let c: String }
        return try JSONDecoder().decode([Row].self, from: data).map {
            Candle(time: Date(timeIntervalSince1970: TimeInterval($0.t) / 1000), open: try dec($0.o), high: try dec($0.h), low: try dec($0.l), close: try dec($0.c))
        }
        .sorted { $0.time < $1.time }
    }
}

/// «chart ETH», «график нефти за неделю», «покажи эфир»: a price chart of one market on the canvas.
public enum ChartCommand: Equatable, Sendable {
    public enum Range: String, CaseIterable, Sendable, Codable {
        case day, week, month

        /// Hyperliquid candle interval and how far back, ms: about 100–170 bars each.
        public var interval: String { switch self { case .day: "15m"; case .week: "1h"; case .month: "4h" } }
        public var span: Int { switch self { case .day: 86_400_000; case .week: 7 * 86_400_000; case .month: 30 * 86_400_000 } }
        public var title: String { switch self { case .day: "24 h"; case .week: "7 days"; case .month: "30 days" } }
    }

    case chart(coin: String, range: Range)
    /// «what's moving», «что растёт»: the biggest movers of the day.
    case movers

    public static func parse(_ text: String, markets: Set<String> = []) -> ChartCommand? {
        let q = text.lowercased()
        if q.contains(/\bmovers?\b|what'?s\s+(?:moving|pumping|dumping|hot)|top\s+(?:gainers|losers|coins)|что\s+(?:растёт|растет|падает|движется|качает)|лидеры\s+(?:роста|падения)|топ\s+(?:монет|движени|рост)/) {
            return .movers
        }
        guard q.contains(/\bcharts?\b|\bgraph\b|график|свеч|candles?|^\s*(?:show|покажи)\b|how'?s\s+\S+\s+(?:doing|looking)|как\s+там\b/),
              let coin = IntentParser.namedAsset(text, markets: markets) else { return nil }
        let range: Range = q.contains(/month|месяц|30\s*(?:d|дн)/) ? .month : q.contains(/week|недел|7\s*(?:d|дн)/) ? .week : .day
        return .chart(coin: coin, range: range)
    }
}

/// Where the money is, as shares of the whole: for the portfolio ring.
public enum PortfolioSplit {
    public struct Slice: Equatable, Sendable {
        public let name: String
        public let usd: Decimal
        public let share: Decimal
    }

    /// Positive parts only, largest first; shares sum to 100 (the last one takes the rounding).
    public static func slices(_ parts: [(String, Decimal)]) -> [Slice] {
        let positive = parts.filter { $0.1 > 0 }.sorted { $0.1 > $1.1 }
        let total = positive.reduce(0) { $0 + $1.1 }
        guard total > 0 else { return [] }
        var out: [Slice] = []
        var used: Decimal = 0
        for (i, p) in positive.enumerated() {
            let share = i == positive.count - 1 ? 100 - used : (p.1 / total * 100).rounded(1, .plain)
            used += share
            out.append(Slice(name: p.0, usd: p.1, share: share))
        }
        return out
    }

    /// The portfolio's network cards, in order: Solana, Hyperliquid and Robinhood Chain first (the user's priority),
    /// each when its account is there or it holds money; then the others by money, the most first. One of the others
    /// holding $100 or less is folded under «Other networks» instead of getting a card.
    public static func networkCards(_ held: [ModuleNetwork: Decimal], present: Set<ModuleNetwork>) -> (cards: [ModuleNetwork], folded: [ModuleNetwork]) {
        let pinned: [ModuleNetwork] = [.solana, .hyperliquid, .robinhood], cent = Decimal(string: "0.01")!
        let rest = held.filter { !pinned.contains($0.key) && $0.value >= cent }
            .sorted { $0.value != $1.value ? $0.value > $1.value : $0.key.rawValue < $1.key.rawValue }
        return (pinned.filter { present.contains($0) || (held[$0] ?? 0) >= cent } + rest.filter { $0.value > 100 }.map(\.key),
                rest.filter { $0.value <= 100 }.map(\.key))
    }
}

/// Zooming a canvas drawn at `pan + point·scale` about a screen point, so that point stays under the fingers.
/// Plain Doubles: CoreGraphics types in this module's API crash the Release build's cross-module optimizer
/// (Swift 6.3.3, «DESERIALIZATION FAILURE … CGPoint … x»).
public enum CanvasZoom {
    public static func pan(keeping point: (x: Double, y: Double), from pan: (x: Double, y: Double), by k: Double) -> (x: Double, y: Double) {
        (point.x - (point.x - pan.x) * k, point.y - (point.y - pan.y) * k)
    }
}

/// A two-finger pan's path, to be drawn once a frame. A trackpad sends ≈ 240 scroll events a second while the fingers
/// move and its momentum ≈ 120 after, neither on the display's beat, so moving on each event gave a 120 Hz frame one, two
/// or three steps (recorded 2026-10-28; at speed a momentum event is 100 pt). The events' own times jitter too (6.7 ms,
/// then 1.6), so a frame reads the path averaged over `window`, `lag` before it: replaying the recorded gestures, frames
/// with a double step went from 1195 to 12, for 19 ms more behind the fingers. The lag leaves an event 8 ms to reach the
/// main thread (20.8 ms left 4, and a busy moment brought the steps back).
public struct ScrollTrail {
    public static let window = 1.0 / 60, lag = 1.0 / 40
    private var marks: [(t: Double, x: Double, y: Double)] = []
    /// Everything added so far.
    public private(set) var sum = (x: 0.0, y: 0.0)
    public init() {}

    /// The newest event's time.
    public var last: Double? { marks.last?.t }

    public mutating func add(_ d: (x: Double, y: Double), at t: Double) {
        let last = marks.last?.t ?? -.infinity
        // After a pause (and at the start) a step plays over a window like the ones in a run, not at once.
        if t - last > Self.window * 1.5 { marks.append((t - Self.window, sum.x, sum.y)) }
        sum = (sum.x + d.x, sum.y + d.y)
        marks.append((max(t, last), sum.x, sum.y))
        if marks.count > 16 { marks.removeFirst(marks.count - 16) }
    }

    /// Forgets the path: what's left of it is no longer played.
    public mutating func clear() { marks.removeAll() }

    /// Where a frame shown at `t` draws the scroll.
    public func frame(at t: Double) -> (x: Double, y: Double) {
        var x = 0.0, y = 0.0
        for k in 0..<8 {
            let p = at(t - Self.lag + Self.window * ((Double(k) + 0.5) / 8 - 0.5))
            x += p.x
            y += p.y
        }
        return (x / 8, y / 8)
    }

    /// Where the scroll was at `t`, between the two events around it.
    private func at(_ t: Double) -> (x: Double, y: Double) {
        guard let last = marks.last, t < last.t else { return sum }
        guard let i = marks.lastIndex(where: { $0.t <= t }) else { return (marks[0].x, marks[0].y) }
        let a = marks[i], b = marks[i + 1], k = b.t > a.t ? (t - a.t) / (b.t - a.t) : 1
        return (a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k)
    }
}

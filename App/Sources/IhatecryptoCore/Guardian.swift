import Foundation

/// Exit rules the exchange cannot hold by itself: after the first partial take the stop goes to breakeven,
/// and a trailing stop follows the best price. They run while the app is open; decisions are pure so they
/// are tested apart from the exchange.
public struct ExitRules: Codable, Equatable, Sendable {
    /// Size when the position opened; a smaller size later means the first take filled.
    public var openedSize: Decimal
    public var entry: Decimal
    public var long: Bool
    /// Move the stop to breakeven once part of the position is taken.
    public var breakevenAfterPartial: Bool
    /// Trailing distance in % of the best price; nil = no trailing.
    public var trailingPct: Decimal?
    /// Best price seen since the position opened.
    public var peak: Decimal
    public var movedToBreakeven = false
    /// A limit entry with a take in parts: the half at 1R is placed once the entry fills.
    public var partialOrderPending = false

    public init(openedSize: Decimal, entry: Decimal, long: Bool, breakevenAfterPartial: Bool, trailingPct: Decimal?) {
        self.openedSize = openedSize
        self.entry = entry
        self.long = long
        self.breakevenAfterPartial = breakevenAfterPartial
        self.trailingPct = trailingPct
        self.peak = entry
    }
}

public enum Guardian {
    public enum Action: Equatable, Sendable {
        case stopToBreakeven
        case stopTo(Decimal)
    }

    /// What to do now for one position. `stop` is the stop on the exchange, `mark` the current price.
    /// Updates `rules` (peak, breakeven flag) and returns at most one stop move.
    public static func step(_ rules: inout ExitRules, size: Decimal, stop: Decimal?, mark: Decimal) -> Action? {
        rules.peak = rules.long ? max(rules.peak, mark) : min(rules.peak, mark)
        if rules.breakevenAfterPartial, !rules.movedToBreakeven, abs(size) < abs(rules.openedSize) {
            rules.movedToBreakeven = true
            return .stopToBreakeven
        }
        guard let pct = rules.trailingPct, let stop else { return nil }
        let trail = rules.peak * (1 - (rules.long ? 1 : -1) * pct / 100)
        // Only tighten, never loosen, and only by a meaningful step (a tenth of the distance) to spare signatures.
        let step = rules.peak * pct / 1000
        let better = rules.long ? trail > stop + step : trail < stop - step
        // A trailing stop must also stay on the right side of the price, or it would fire at once.
        let valid = rules.long ? trail < mark : trail > mark
        return better && valid ? .stopTo(trail) : nil
    }

    /// The half at 1R's tries on one coin: failures in a row, and when the next try may go.
    public struct PartialTries: Equatable, Sendable {
        public var failures = 0
        public var next = Date.distantPast
        public init(failures: Int = 0, next: Date = .distantPast) {
            self.failures = failures
            self.next = next
        }
    }

    /// What a try at the half at 1R leaves. `placed`: true on the book (or skipped, under $10), false waiting for the
    /// limit's full fill, nil failed (`refused`: a declined signature). Returns whether the pending flag stays, the tries
    /// for the next pass, and whether the chat hears of it: the first failure and the end, not every try. A failure goes
    /// again in 1, then 2 minutes; the third, or a declined signature, ends it. The stop and take on the exchange protect
    /// the position either way.
    public static func afterPartialTry(_ tries: PartialTries, placed: Bool?, refused: Bool = false, now: Date = Date())
        -> (pending: Bool, tries: PartialTries, tell: Bool) {
        guard let placed else {
            let failures = tries.failures + 1
            if refused || failures >= 3 { return (false, PartialTries(failures: failures), true) }
            return (true, PartialTries(failures: failures, next: now.addingTimeInterval(TimeInterval(60 * failures))), failures == 1)
        }
        // Waiting keeps the count: reads that lag and fail by turns still come to an end.
        return (!placed, PartialTries(failures: placed ? 0 : tries.failures), false)
    }
}

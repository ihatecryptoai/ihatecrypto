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
    /// The entry is this TWAP on the exchange. Its stop firing doesn't stop it: the guardian does, and closes what it
    /// bought after, so it never builds a position without a stop.
    public var twapId: UInt64?
    /// The TWAP plan's loss at the stop: past 1.5 × it the guardian stops the TWAP (Executor.guardTwap).
    public var twapRisk: Decimal?

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
    /// Breakeven is done once the exchange shows it, never on a flag set before the try: a failed move is asked for again.
    public static func step(_ rules: inout ExitRules, size: Decimal, stop: Decimal?, mark: Decimal) -> Action? {
        rules.peak = rules.long ? max(rules.peak, mark) : min(rules.peak, mark)
        if rules.breakevenAfterPartial, !rules.movedToBreakeven, abs(size) < abs(rules.openedSize) {
            // A stop at the entry or better: moved by hand, or by a try whose check failed after it went on.
            if let stop, rules.long ? stop >= rules.entry : stop <= rules.entry { rules.movedToBreakeven = true }
            // Not while the price is on the wrong side of breakeven: the stop would fire at once.
            else if rules.long ? mark > breakeven(entry: rules.entry, long: true) : mark < breakeven(entry: rules.entry, long: false) {
                return .stopToBreakeven
            }
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

    /// The stop that loses nothing: the entry plus both sides' fees (Executor.moveStop places exactly this).
    public static func breakeven(entry: Decimal, long: Bool) -> Decimal { entry + (long ? 1 : -1) * entry * 2 * RiskCalc.tradeFee }

    /// After `failures` failed stop moves in a row: 1, 2, 4, 8, then every 15 minutes. Each try may ask for Touch ID.
    public static func retryDelay(after failures: Int) -> TimeInterval { min(60 * pow(2, Double(max(failures, 1) - 1)), 900) }

    /// What a try at a stop move leaves: no tries once it went through; a failure waits `retryDelay` and is said only
    /// when it starts a series (`tell`): the tries after it are quiet, and the move that lands says so.
    public static func afterStopTry(_ tries: Tries, ok: Bool, now: Date = Date()) -> (tries: Tries?, tell: Bool) {
        guard !ok else { return (nil, false) }
        let failures = tries.failures + 1
        return (Tries(failures: failures, next: now.addingTimeInterval(retryDelay(after: failures))), failures == 1)
    }

    /// A guardian's tries on one coin (the half at 1R, a stop move): failures in a row, and when the next try may go.
    public struct Tries: Equatable, Sendable {
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
    public static func afterPartialTry(_ tries: Tries, placed: Bool?, refused: Bool = false, now: Date = Date())
        -> (pending: Bool, tries: Tries, tell: Bool) {
        guard let placed else {
            let failures = tries.failures + 1
            if refused || failures >= 3 { return (false, Tries(failures: failures), true) }
            return (true, Tries(failures: failures, next: now.addingTimeInterval(TimeInterval(60 * failures))), failures == 1)
        }
        // Waiting keeps the count: reads that lag and fail by turns still come to an end.
        return (!placed, Tries(failures: placed ? 0 : tries.failures), false)
    }
}

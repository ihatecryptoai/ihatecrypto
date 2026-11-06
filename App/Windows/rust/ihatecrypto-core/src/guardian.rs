//! Exit rules the exchange cannot hold by itself (`Guardian.swift`): after the first partial take the stop goes to
//! breakeven, and a trailing stop follows the best price. They run while the app is open; decisions are pure so they
//! are tested apart from the exchange.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::foundation::{Date, adding, distant_past, number};
use crate::plan::RiskCalc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitRules {
    /// Size when the position opened; a smaller size later means the first take filled.
    #[serde(with = "number")]
    pub opened_size: Decimal,
    #[serde(with = "number")]
    pub entry: Decimal,
    pub long: bool,
    /// Move the stop to breakeven once part of the position is taken.
    pub breakeven_after_partial: bool,
    /// Trailing distance in % of the best price; nil = no trailing.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub trailing_pct: Option<Decimal>,
    /// Best price seen since the position opened.
    #[serde(with = "number")]
    pub peak: Decimal,
    pub moved_to_breakeven: bool,
    /// A limit entry with a take in parts: the half at 1R is placed once the entry fills.
    pub partial_order_pending: bool,
    /// The entry is this TWAP on the exchange. Its stop firing doesn't stop it: the guardian does, and closes what it
    /// bought after, so it never builds a position without a stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twap_id: Option<u64>,
    /// The TWAP plan's loss at the stop: past 1.5 × it the guardian stops the TWAP (Executor.guardTwap).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub twap_risk: Option<Decimal>,
}

impl ExitRules {
    pub fn new(opened_size: Decimal, entry: Decimal, long: bool, breakeven_after_partial: bool, trailing_pct: Option<Decimal>) -> Self {
        Self {
            opened_size, entry, long, breakeven_after_partial, trailing_pct, peak: entry, moved_to_breakeven: false,
            partial_order_pending: false, twap_id: None, twap_risk: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    StopToBreakeven,
    StopTo(Decimal),
}

/// What to do now for one position. `stop` is the stop on the exchange, `mark` the current price.
/// Updates `rules` (peak, breakeven flag) and returns at most one stop move.
/// Breakeven is done once the exchange shows it, never on a flag set before the try: a failed move is asked for again.
pub fn step(rules: &mut ExitRules, size: Decimal, stop: Option<Decimal>, mark: Decimal) -> Option<Action> {
    rules.peak = if rules.long { rules.peak.max(mark) } else { rules.peak.min(mark) };
    if rules.breakeven_after_partial && !rules.moved_to_breakeven && size.abs() < rules.opened_size.abs() {
        // A stop at the entry or better: moved by hand, or by a try whose check failed after it went on.
        if stop.is_some_and(|s| if rules.long { s >= rules.entry } else { s <= rules.entry }) {
            rules.moved_to_breakeven = true;
        } else if if rules.long { mark > breakeven(rules.entry, true) } else { mark < breakeven(rules.entry, false) } {
            // Not while the price is on the wrong side of breakeven: the stop would fire at once.
            return Some(Action::StopToBreakeven);
        }
    }
    let (pct, stop) = (rules.trailing_pct?, stop?);
    let dir = if rules.long { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
    // Checked: prices and percents come from the exchange and the chat, and rust_decimal panics on overflow.
    let trail = rules.peak.checked_mul(Decimal::ONE.checked_sub(dir.checked_mul(pct)?.checked_div(Decimal::ONE_HUNDRED)?)?)?;
    // Only tighten, never loosen, and only by a meaningful step (a tenth of the distance) to spare signatures.
    let step = rules.peak.checked_mul(pct)?.checked_div(Decimal::ONE_THOUSAND)?;
    let better = if rules.long { trail > stop.checked_add(step)? } else { trail < stop.checked_sub(step)? };
    // A trailing stop must also stay on the right side of the price, or it would fire at once.
    let valid = if rules.long { trail < mark } else { trail > mark };
    (better && valid).then_some(Action::StopTo(trail))
}

/// The stop that loses nothing: the entry plus both sides' fees (Executor.moveStop places exactly this).
/// An entry past what the fees can be added to (never a real price) is its own breakeven.
pub fn breakeven(entry: Decimal, long: bool) -> Decimal {
    let fees = entry.checked_mul(Decimal::TWO).and_then(|x| x.checked_mul(RiskCalc::trade_fee()));
    let side = if long { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
    fees.and_then(|f| entry.checked_add(side * f)).unwrap_or(entry)
}

/// After `failures` failed stop moves in a row: 1, 2, 4, 8, then every 15 minutes, in seconds. Each try may ask for
/// Touch ID.
pub fn retry_delay(failures: i64) -> f64 { (60.0 * 2f64.powi((failures.max(1) - 1).min(64) as i32)).min(900.0) }

/// A guardian's tries on one coin (the half at 1R, a stop move): failures in a row, and when the next try may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tries {
    pub failures: i64,
    pub next: Date,
}

impl Default for Tries {
    fn default() -> Self { Self { failures: 0, next: distant_past() } }
}

/// What a try at a stop move leaves: no tries once it went through; a failure waits `retry_delay` and is said only
/// when it starts a series (`tell`): the tries after it are quiet, and the move that lands says so.
pub fn after_stop_try(tries: Tries, ok: bool, now: Date) -> (Option<Tries>, bool) {
    if ok { return (None, false) }
    let failures = tries.failures + 1;
    (Some(Tries { failures, next: adding(now, retry_delay(failures)) }), failures == 1)
}

/// What a try at the half at 1R leaves. `placed`: true on the book (or skipped, under $10), false waiting for the
/// limit's full fill, nil failed (`refused`: a declined signature). Returns whether the pending flag stays, the tries
/// for the next pass, and whether the chat hears of it: the first failure and the end, not every try. A failure goes
/// again in 1, then 2 minutes; the third, or a declined signature, ends it. The stop and take on the exchange protect
/// the position either way.
pub fn after_partial_try(tries: Tries, placed: Option<bool>, refused: bool, now: Date) -> (bool, Tries, bool) {
    let Some(placed) = placed else {
        let failures = tries.failures + 1;
        if refused || failures >= 3 { return (false, Tries { failures, ..Tries::default() }, true) }
        return (true, Tries { failures, next: adding(now, 60.0 * failures as f64) }, failures == 1);
    };
    // Waiting keeps the count: reads that lag and fail by turns still come to an end.
    (!placed, Tries { failures: if placed { 0 } else { tries.failures }, ..Tries::default() }, false)
}

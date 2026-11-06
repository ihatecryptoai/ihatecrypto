use super::d;
use crate::foundation::{adding, unix};
use crate::guardian::*;

fn to(s: &str) -> Option<Action> { Some(Action::StopTo(d(s))) }

#[test]
fn guardian_moves_to_breakeven_after_the_partial_take_and_trails() {
    let mut r = ExitRules::new(d("0.012"), d("2000"), true, true, Some(d("2")));
    assert_eq!(step(&mut r, d("0.012"), Some(d("1940")), d("2010")), to("1969.8"));   // 2% under the best price beats the 3% stop
    assert_eq!(step(&mut r, d("0.006"), Some(d("1969.8")), d("2020")), Some(Action::StopToBreakeven));   // half taken
    assert_eq!(step(&mut r, d("0.006"), Some(d("2002")), d("2020")), None);   // trail 1979.6 would loosen
    assert_eq!(step(&mut r, d("0.006"), Some(d("2002")), d("2100")), to("2058"));   // peak 2100 − 2%
    assert_eq!(step(&mut r, d("0.006"), Some(d("2058")), d("2080")), None);   // price fell back: stop stays
    let mut s = ExitRules::new(d("-1"), d("2000"), false, false, Some(d("2")));
    assert_eq!(step(&mut s, d("-1"), Some(d("2060")), d("1900")), to("1938"));   // short trails from below
}

/// Breakeven is done when the exchange shows it, never on the guardian's say-so before the try: a failed move is asked
/// for again, not while the price is under breakeven, and a stop moved by hand counts.
#[test]
fn guardian_asks_for_breakeven_until_the_exchange_has_it() {
    let mut r = ExitRules::new(d("0.012"), d("2000"), true, true, None);
    assert_eq!(step(&mut r, d("0.006"), Some(d("1940")), d("2020")), Some(Action::StopToBreakeven));
    assert!(!r.moved_to_breakeven);   // not until it's on the exchange
    assert_eq!(step(&mut r, d("0.006"), Some(d("1940")), d("2020")), Some(Action::StopToBreakeven));   // the move failed: asked again
    assert_eq!(step(&mut r, d("0.006"), Some(d("1940")), d("2003")), None);   // under 2003.8 it would fire at once
    assert_eq!(step(&mut r, d("0.006"), Some(d("1940")), d("1990")), None);
    assert!(step(&mut r, d("0.006"), Some(d("2003.8")), d("2020")).is_none() && r.moved_to_breakeven);
    let mut s = ExitRules::new(d("-1"), d("2000"), false, true, None);
    assert!(step(&mut s, d("-0.5"), Some(d("1999")), d("1980")).is_none() && s.moved_to_breakeven);   // moved by hand earlier
    let mut t = ExitRules::new(d("-1"), d("2000"), false, true, None);
    assert_eq!(step(&mut t, d("-0.5"), Some(d("2060")), d("1998")), None);   // a short: over 1996.2 it would fire
    assert!(step(&mut t, d("-0.5"), Some(d("2060")), d("1990")) == Some(Action::StopToBreakeven) && !t.moved_to_breakeven);
    assert!(breakeven(d("2000"), true) == d("2003.8") && breakeven(d("2000"), false) == d("1996.2"));
    // Breakeven waiting under its price still lets the trailing stop tighten.
    let mut u = ExitRules::new(d("0.012"), d("2000"), true, true, Some(d("2")));
    assert_eq!(step(&mut u, d("0.012"), Some(d("1900")), d("2040")), to("1999.2"));
    assert!(step(&mut u, d("0.006"), Some(d("1900")), d("2003")) == to("1999.2") && !u.moved_to_breakeven);
    assert_eq!([1, 2, 4, 5, 9].map(retry_delay), [60.0, 120.0, 480.0, 900.0, 900.0]);
}

/// A failed stop move is said once, then tried again quietly after a growing pause; one that lands ends the series.
#[test]
fn stop_move_tries_say_the_first_failure_only() {
    let now = unix(1_000_000.0);
    let (tries, tell) = after_stop_try(Tries::default(), false, now);
    assert!(tell && tries == Some(Tries { failures: 1, next: adding(now, 60.0) }));
    let (tries, tell) = after_stop_try(tries.unwrap(), false, now);
    assert!(!tell && tries == Some(Tries { failures: 2, next: adding(now, 120.0) }));
    let (tries, tell) = after_stop_try(tries.unwrap(), true, now);
    assert!(!tell && tries.is_none());
}

/// The half at 1R after a limit fills: the pending flag goes only once it's on; a failure goes again in 1, then 2
/// minutes, and the third one, or a declined signature, ends it. The chat hears the first failure and the end.
#[test]
fn partial_take_tries_three_times_and_says_the_first_and_the_last() {
    let now = unix(1_000_000.0);
    // Waiting for the full fill: the flag stays, nothing said, no failure counted.
    let (pending, tries, tell) = after_partial_try(Tries::default(), Some(false), false, now);
    assert!(pending && !tell && tries == Tries::default());
    let (pending, tries, tell) = after_partial_try(Tries::default(), None, false, now);
    assert!(pending && tell && tries == Tries { failures: 1, next: adding(now, 60.0) });
    // Waiting in between keeps the count: reads that lag and fail by turns still come to an end.
    let (pending, between, tell) = after_partial_try(tries, Some(false), false, now);
    assert!(pending && !tell && between.failures == 1);
    let (pending, tries, tell) = after_partial_try(between, None, false, now);
    assert!(pending && !tell && tries == Tries { failures: 2, next: adding(now, 120.0) });
    let (pending, _, tell) = after_partial_try(tries, None, false, now);
    assert!(!pending && tell);
    // A declined signature ends it at once.
    let (pending, _, tell) = after_partial_try(Tries::default(), None, true, now);
    assert!(!pending && tell);
    // On after a failure: the flag goes, nothing more said here (the executor's own line tells it).
    let (pending, _, tell) = after_partial_try(Tries { failures: 1, next: now }, Some(true), false, now);
    assert!(!pending && !tell);
}

/// The rules as exit-rules.json keeps them.
#[test]
fn exit_rules_keep_swifts_shape() {
    let mut r = ExitRules::new(d("0.012"), d("2000"), true, true, Some(d("2")));
    r.twap_id = Some(7);
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json, serde_json::json!({"openedSize": 0.012, "entry": 2000, "long": true, "breakevenAfterPartial": true, "trailingPct": 2,
                                        "peak": 2000, "movedToBreakeven": false, "partialOrderPending": false, "twapId": 7}));
    assert_eq!(serde_json::from_value::<ExitRules>(json).unwrap(), r);
}

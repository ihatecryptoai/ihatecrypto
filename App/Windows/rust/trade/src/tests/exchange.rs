//! The executor against FakeHL: closing (ExecutorCloseTests), limit entries that fill at once (LimitFillTests), the half
//! at 1R (PartialTakeTests), a trailing stop (TrailingStopTests) and TWAP entries (TwapExecutionTests).

use std::collections::HashSet;

use chrono::Utc;
use execution::trade_signer::TradeError;
use ihatecrypto_core::foundation::text;
use ihatecrypto_core::intent::{IntentParser, TradeIntent};
use ihatecrypto_core::plan::{Market, Plan};
use ihatecrypto_core::position_command::PositionCommand;
use ihatecrypto_core::risk::RiskPolicy;
use rust_decimal::Decimal;
use serde_json::{Value, json};

use super::d;
use super::fake_hl::*;
use crate::Error;
use crate::executor::{Opened, PartialTake, Prepared, TwapWatch, hl_markets};

fn market() -> Market {
    let mut m = Market::new("ETH", d("3000"), 4, 25);
    m.index = 0;
    m
}

/// A plan from the chat's words, as `prepare` hands it to `execute` (ETH at 3000 in FakeHL, so a limit at 2990 waits).
fn plan(phrase: &str, leverage: i64) -> Prepared {
    let markets: HashSet<String> = ["ETH".to_string()].into();
    let intent = IntentParser::apply(phrase, &TradeIntent::default(), &markets).unwrap_or_else(|| panic!("{phrase}"));
    let evaluation = Plan::evaluate(&intent, Some(&market()));
    assert!(evaluation.problem.is_none(), "{phrase}: {:?}", evaluation.problem);
    Prepared::new(intent, market(), evaluation.sizing.unwrap(), leverage, None, vec![], vec![], Utc::now())
}

struct Outcome {
    sent: Vec<String>,
    opened: Option<Opened>,
    text: String,
    error: Option<Error>,
    reported: Vec<String>,
    protections: Vec<Value>,
    orders: Vec<Value>,
}

/// `execute` against FakeHL as `setup` leaves it: what went to the exchange, and the Opened or the error's words.
async fn execute(p: &Prepared, signer: ZeroSigner, setup: impl FnOnce(&mut State)) -> Outcome {
    let fake = Fake::start(setup).await;
    let result = fake.executor(signer).execute(p).await;
    let s = fake.state();
    Outcome {
        sent: s.sent.clone(), opened: result.as_ref().ok().cloned(), text: result.as_ref().err().map(Error::to_string).unwrap_or_default(),
        error: result.err(), reported: s.reported.clone(), protections: s.protections.clone(), orders: s.orders.clone(),
    }
}

/// The fill of the plan's whole size: the position opens, and with `armed` its group's stop and take are listed.
fn fills_at_once(p: &Prepared, armed: bool, reply: Option<&str>) -> impl FnOnce(&mut State) + use<> {
    let (size, reply) = (p.sizing.size, reply.map(str::to_string));
    move |s| {
        s.entry = Box::new(move |s| {
            s.position = size;
            if armed { s.orders = vec![stop(7, "2900.3"), take(8, "3169.4")] }
            Some(reply.clone().unwrap_or_else(|| filled_at(&text(size), "2988")))
        })
    }
}

/// The plan's limit waiting on the book.
fn book(p: &Prepared) -> Value {
    json!({"coin": "ETH", "oid": 9, "isTrigger": false, "limitPx": "2990", "orderType": "Limit", "reduceOnly": false, "side": "B", "sz": text(p.sizing.size)})
}

const PHRASES: [&str; 2] = ["long ETH limit 2990 risk $0.5 stop 3% take 6% leverage 2x", "лонг ETH лимит 2990 риск $0.5 стоп 3% тейк 6% плечо 2"];

fn sent(list: &[&str]) -> Vec<String> { list.iter().map(|s| s.to_string()).collect() }

// MARK: Close (ExecutorCloseTests)

/// «close ETH» on FakeHL: what went to the exchange, the words, and whether it failed.
async fn run_close(position: &str, orders: Vec<Value>, refuses: bool, close: impl FnMut(&mut State) -> Option<String> + Send + 'static)
    -> (Vec<String>, String, bool) {
    let fake = Fake::start(|s| {
        s.position = d(position);
        s.orders = orders;
        s.close = Box::new(close);
    }).await;
    let result = fake.executor(ZeroSigner { refuses, ..Default::default() }).close("ETH").await;
    (fake.sent(), result.as_ref().map_or_else(Error::to_string, String::clone), result.is_err())
}

/// The stop and take stay on the exchange until the position is flat. A close that doesn't fill, fills in part or loses
/// its reply leaves what is open under its stop; after the emergency close, restore_stop puts one back once.
#[tokio::test]
async fn close_keeps_the_stop_until_the_close_fills() {
    let guarded = || vec![stop(2, "2940"), take(3, "3120")];
    // Filled: the close goes first, its stop and take are cancelled after it.
    let r = run_close("0.01", guarded(), false, |s| { s.position = Decimal::ZERO; Some(filled("0.01")) }).await;
    assert!(r.0 == sent(&["close sell", "cancel 2,3"]) && !r.2, "{r:?}");
    assert!(r.1.starts_with("ETH position closed: 0.01 at 2,999"), "{}", r.1);

    // Filled whole while the positions don't read: the fill is taken, not a false «didn't finish».
    let r = run_close("0.01", guarded(), false, |s| { s.position = Decimal::ZERO; s.positions_down = true; Some(filled("0.01")) }).await;
    assert!(r.0 == sent(&["close sell", "cancel 2,3"]) && !r.2, "{r:?}");

    // No one to fill it (a thin book): nothing cancelled, the position keeps its stop.
    let r = run_close("0.01", guarded(), false, |_| Some(NO_MATCH.into())).await;
    assert!(r.0 == sent(&["close sell"]) && r.2 && r.1.contains("still open") && r.1.contains("stop at 2,940"), "{r:?}");

    // A short closes with a buy, and keeps its stop above.
    let r = run_close("-0.01", vec![stop_side(5, "3060", "B")], false, |_| Some(NO_MATCH.into())).await;
    assert!(r.0 == sent(&["close buy"]) && r.2 && r.1.contains("stop at 3,060"), "{r:?}");

    // A part filled: the rest stays under the stop.
    let r = run_close("0.01", guarded(), false, |s| { s.position = d("0.005"); Some(filled("0.005")) }).await;
    assert!(r.0 == sent(&["close sell"]) && r.2, "{r:?}");
    assert!(r.1.contains("0.005 of 0.01 ETH closed") && r.1.contains("stop at 2,940") && !r.1.contains("position closed"), "{}", r.1);

    // A part filled with no stop on the exchange: it says so, not that a stop stays.
    let r = run_close("0.01", vec![], false, |s| { s.position = d("0.005"); Some(filled("0.005")) }).await;
    assert!(r.0 == sent(&["close sell"]) && r.2 && r.1.contains("0.005 is still open") && r.1.contains("no stop on the exchange"), "{r:?}");

    // Two stops on a long: the message names the nearer one, the higher.
    let r = run_close("0.01", vec![stop(2, "2900"), stop(6, "2940"), take(3, "3120")], false, |_| Some(NO_MATCH.into())).await;
    assert!(r.2 && r.1.contains("stop at 2,940"), "{r:?}");

    // The reply is lost: the position read afterwards decides, and it is still open.
    let r = run_close("0.01", guarded(), false, |_| None).await;
    assert!(r.0 == sent(&["close sell"]) && r.2 && r.1.contains("still open") && r.1.contains("stop at 2,940"), "{r:?}");

    // Lost, and the positions don't read either: it says so, and the stop is where it was.
    let r = run_close("0.01", guarded(), false, |s| { s.positions_down = true; None }).await;
    assert!(r.0 == sent(&["close sell"]) && r.2 && r.1.contains("can't be read") && r.1.contains("stop at 2,940"), "{r:?}");

    // The signer turns the close down: nothing is sent, and the refusal stays a refusal.
    let r = run_close("0.01", guarded(), true, |_| None).await;
    assert!(r.0.is_empty() && r.2 && r.1 == "declined", "{r:?}");

    // The stop filled first: the close is refused as reduce-only, the position is flat, the take that's left is cancelled.
    let r = run_close("0.01", guarded(), false, |s| {
        s.position = Decimal::ZERO;
        s.orders.retain(|o| o["oid"] != 2);
        Some(r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"error":"Reduce only order would increase position. asset=0"}]}}}"#.into())
    }).await;
    assert!(r.0 == sent(&["close sell", "cancel 3"]) && !r.2 && r.1.contains("ETH position is closed") && r.1.contains("filled first"), "{r:?}");

    // Only a limit entry waiting: it is cancelled, nothing to close.
    let limit = json!({"coin": "ETH", "oid": 4, "isTrigger": false, "limitPx": "2900", "orderType": "Limit", "reduceOnly": false, "side": "B", "sz": "0.01"});
    let r = run_close("0", vec![limit.clone()], false, |_| None).await;
    assert!(r.0 == sent(&["cancel 4"]) && !r.2 && r.1.contains("there was no position"), "{r:?}");

    // A position and a limit entry waiting: the entry goes first (it could open it again), the stop and take after the fill.
    let r = run_close("0.01", [vec![limit], guarded()].concat(), false, |s| { s.position = Decimal::ZERO; Some(filled("0.01")) }).await;
    assert!(r.0 == sent(&["cancel 4", "close sell", "cancel 2,3"]) && !r.2, "{r:?}");

    // No position, its stop and take left over: they go in one cancel.
    let r = run_close("0", guarded(), false, |_| None).await;
    assert!(r.0 == sent(&["cancel 2,3"]) && !r.2 && r.1 == "ETH orders cancelled (2); there was no position.", "{r:?}");

    // The emergency close: no stop on the exchange, the close doesn't fill, restore_stop puts the stop back — once.
    let fake = Fake::start(|s| { s.position = d("0.01"); s.close = Box::new(|_| Some(NO_MATCH.into())) }).await;
    let e = fake.executor(ZeroSigner::default());
    let unfinished = e.close("ETH").await.unwrap_err();
    assert!(matches!(&unfinished, Error::Execution(m) if m.contains("no stop on the exchange")), "{unfinished}");
    let market = hl_markets().await.unwrap().remove("ETH").unwrap();
    let both = format!("{} | {}", e.restore_stop(&market, d("2940")).await, e.restore_stop(&market, d("2940")).await);
    assert_eq!(fake.sent(), sent(&["close sell", "sl"]));
    assert_eq!(both, "Its stop at 2,940 is back on the exchange now. | Its stop at 2,940 is on the exchange.");
}

// MARK: Limit entries (LimitFillTests)

/// The price was already past the limit at Confirm: it fills at once, and the group's stop and take are listed — nothing
/// more is sent.
#[tokio::test]
async fn limit_filled_at_once_keeps_its_armed_stop() {
    for phrase in PHRASES {
        let p = plan(phrase, 2);
        assert!(p.intent.entry_px == Some(d("2990")) && p.intent.leverage == Some(Decimal::TWO));
        let r = execute(&p, ZeroSigner::default(), fills_at_once(&p, true, None)).await;
        let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
        assert_eq!(r.sent, sent(&["leverage", "limit sl+tp"]));
        assert!(!opened.pending && opened.size == p.sizing.size && opened.entry == d("2988") && opened.stop == d("2900.3") && opened.take == Some(d("3169.4")));
    }
}

/// «take in parts» on a limit that fills at once: the half at 1R goes on as on a market entry.
#[tokio::test]
async fn limit_filled_at_once_gets_its_partial_take() {
    let p = plan("long ETH limit 2990 risk $2 stop 3% take 6% in parts leverage 2x", 2);
    let r = execute(&p, ZeroSigner::default(), fills_at_once(&p, true, None)).await;
    let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
    assert_eq!(r.sent, sent(&["leverage", "limit sl+tp", "tp"]));
    assert_eq!(opened.partial_take, Some(d("3075.7")));   // 2988 + (2988 − 2900.3)
}

/// The half turned down there: the position stays open under its stop and take, and the chat hears why.
#[tokio::test]
async fn limit_filled_at_once_with_its_half_refused() {
    let p = plan("long ETH limit 2990 risk $2 stop 3% take 6% in parts leverage 2x", 2);
    let r = execute(&p, ZeroSigner::default(), |s| { fills_at_once(&p, true, None)(s); s.refusals = 1 }).await;
    let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
    assert!(r.sent == sent(&["leverage", "limit sl+tp", "tp"]) && opened.partial_take.is_none());
    assert!(r.reported.iter().any(|l| l.starts_with("Partial take-profit didn't go through")));
    // Its reply lost: it may be on, and the chat isn't told it failed.
    let lost = execute(&p, ZeroSigner::default(), |s| { fills_at_once(&p, true, None)(s); s.lost = 1 }).await;
    assert!(lost.opened.unwrap().partial_take.is_none() && lost.reported.iter().any(|l| l.contains("reply didn't come, so it may or may not be on")));
}

/// Filled at once, its stop not listed: the stop and take go on the position again, from the limit's prices, for the
/// position's size. The trade stays.
#[tokio::test]
async fn limit_filled_at_once_without_its_stop_gets_it_again() {
    for phrase in PHRASES {
        let p = plan(phrase, 2);
        let r = execute(&p, ZeroSigner::default(), fills_at_once(&p, false, None)).await;
        let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
        assert_eq!(r.sent, sent(&["leverage", "limit sl+tp", "sl+tp"]));
        let sl = &r.protections[0];
        assert!(sl["r"] == true && sl.pointer("/t/trigger/tpsl") == Some(&json!("sl")) && sl.pointer("/t/trigger/triggerPx") == Some(&json!("2900.3")));
        assert_eq!(sl["s"], text(p.sizing.size));
        assert!(!opened.pending && opened.size == p.sizing.size && opened.stop == d("2900.3"));
        assert!(r.reported.iter().any(|l| l.contains("didn't confirm its stop") && l.contains("placing it again")), "{:?}", r.reported);
        assert!(r.reported.iter().any(|l| l.starts_with("On the exchange: stop 2,900.3")), "{:?}", r.reported);
    }
}

/// The stop is turned down again: the position is closed, as a market entry's is.
#[tokio::test]
async fn limit_filled_at_once_stop_refused_again_closes() {
    let p = plan(PHRASES[0], 2);
    let size = text(p.sizing.size);
    let r = execute(&p, ZeroSigner::default(), |s| {
        fills_at_once(&p, false, None)(s);
        s.refusals = 1;
        s.close = Box::new(move |s| { s.position = Decimal::ZERO; Some(filled(&size)) });
    }).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "limit sl+tp", "sl+tp", "close sell"]), "{:?}", r.sent);
    assert!(r.text.starts_with("Position closed: the exchange didn't confirm the stop."), "{}", r.text);
}

/// Turned down, and the close doesn't fill either: the stop goes back on what is open.
#[tokio::test]
async fn limit_filled_at_once_close_fails_puts_the_stop_back() {
    let p = plan(PHRASES[0], 2);
    let r = execute(&p, ZeroSigner::default(), |s| {
        fills_at_once(&p, false, None)(s);
        s.refusals = 1;
        s.close = Box::new(|_| Some(NO_MATCH.into()));
    }).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "limit sl+tp", "sl+tp", "close sell", "sl"]), "{:?}", r.sent);
    assert!(r.text.contains("emergency close failed too") && r.text.contains("Its stop at 2,900.3 is back on the exchange now."), "{}", r.text);
}

/// Filled at once and already gone (the price ran through the stop): it says so, and sends nothing more.
#[tokio::test]
async fn limit_filled_at_once_already_stopped_out() {
    let p = plan(PHRASES[0], 2);
    let size = text(p.sizing.size);
    let r = execute(&p, ZeroSigner::default(), |s| s.entry = Box::new(move |_| Some(filled_at(&size, "2988")))).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "limit sl+tp"]));
    assert!(r.text.contains("no ETH position is open now"), "{}", r.text);
}

/// The entry filled and its stop was turned down in the same reply: that's a position without a stop, never «No
/// position» — also when /info shows the fill a moment late. Without one open it's a plain refusal; with the positions
/// unread it doesn't claim either.
#[tokio::test]
async fn filled_entry_with_a_refused_stop_is_not_no_position() {
    let p = plan(PHRASES[0], 2);
    let mixed = format!(r#"{{"status":"ok","response":{{"type":"order","data":{{"statuses":[{{"filled":{{"totalSz":"{}","avgPx":"2988","oid":9}}}},{{"error":"Trigger order has invalid price."}}]}}}}}}"#,
                        text(p.sizing.size));
    let held = d("0.0051");
    assert_ne!(held, p.sizing.size);
    let m = mixed.clone();
    let r = execute(&p, ZeroSigner::default(), move |s| s.entry = Box::new(move |s| { s.position = held; s.down = 2; Some(m.clone()) })).await;
    let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
    assert_eq!(r.sent, sent(&["leverage", "limit sl+tp", "sl+tp"]));
    assert!(r.protections[0]["s"] == "0.0051" && opened.size == held && opened.entry == d("3000") && opened.stop == d("2900.3"));
    assert!(r.reported.iter().any(|l| l.contains("Trigger order has invalid price") && l.contains("placing it again")));

    let m = mixed.clone();
    let r = execute(&p, ZeroSigner::default(), move |s| s.entry = Box::new(move |_| Some(m.clone()))).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "limit sl+tp"]));
    assert!(r.text.starts_with("Limit rejected: Trigger order has invalid price") && r.text.ends_with("No position."), "{}", r.text);

    let r = execute(&p, ZeroSigner::default(), move |s| s.entry = Box::new(move |s| { s.positions_down = true; Some(mixed.clone()) })).await;
    assert!(r.opened.is_none() && !r.text.contains("No position") && r.text.contains("can't be read"), "{}", r.text);
}

/// The entry rests and its stop was turned down: it would fill with no stop, so it is cancelled, and the chat says so.
#[tokio::test]
async fn resting_entry_with_a_refused_stop_is_cancelled() {
    let p = plan(PHRASES[0], 2);
    let mixed = r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":9}},{"error":"Trigger order has invalid price."}]}}}"#;
    let order = book(&p);
    let r = execute(&p, ZeroSigner::default(), move |s| s.entry = Box::new(move |s| { s.orders = vec![order.clone()]; Some(mixed.into()) })).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "limit sl+tp", "cancel 9"]) && r.orders.is_empty(), "{:?}", r.sent);
    assert!(r.text.contains("Trigger order has invalid price") && r.text.contains("was cancelled; no position"), "{}", r.text);
}

/// Rested, then gone from the book a moment later because it filled: a position like one that filled at once. One that
/// is on the book waits, as before — also when the first reads of the book fail or come back late.
#[tokio::test]
async fn limit_that_filled_after_resting_is_a_position() {
    let p = plan(PHRASES[0], 2);
    let resting = r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"resting":{"oid":9}},"waitingForFill","waitingForFill"]}}}"#;
    let r = execute(&p, ZeroSigner::default(), fills_at_once(&p, true, Some(resting))).await;
    let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
    assert!(r.sent == sent(&["leverage", "limit sl+tp"]) && !opened.pending && opened.size == p.sizing.size);
    for down in [0, 3] {
        let order = book(&p);
        let r = execute(&p, ZeroSigner::default(), move |s| s.entry = Box::new(move |s| { s.orders = vec![order.clone()]; s.down = down; Some(resting.into()) })).await;
        let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
        assert!(r.sent == sent(&["leverage", "limit sl+tp"]) && opened.pending);
    }
}

/// The protection chain the limit now shares, on a market entry: protected, or closed when the stop is refused.
#[tokio::test]
async fn market_entry_stop_refused_still_closes() {
    let p = plan("long ETH risk $0.5 stop 3% take 6% leverage 2x", 2);
    let size = p.sizing.size;
    let opens = move |s: &mut State| s.entry = Box::new(move |s| { s.position = size; Some(filled(&text(size))) });
    let r = execute(&p, ZeroSigner::default(), opens).await;
    let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
    assert!(r.sent == sent(&["leverage", "entry", "sl+tp"]) && opened.stop == d("2909"));   // 3% under the fill at 2999

    let r = execute(&p, ZeroSigner::default(), move |s| {
        opens(s);
        s.refusals = 1;
        s.close = Box::new(move |s| { s.position = Decimal::ZERO; Some(filled(&text(size))) });
    }).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "entry", "sl+tp", "close sell"]));
    assert!(r.text.starts_with("Position closed: the exchange didn't confirm the stop."), "{}", r.text);
}

/// A market entry the signer turns down never went out: the refusal comes out as it is — the guard's, a Ledger declined —
/// never «the reply didn't come», and an ETH position already open isn't taken for its fill. Signed and sent with the
/// reply lost, the positions decide.
#[tokio::test]
async fn a_stop_before_the_entry_is_sent_is_not_a_lost_reply() {
    let p = plan("long ETH risk $0.5 stop 3% take 6% leverage 2x", 2);
    let guard_text = "The guard refused to sign. The order pays a fee to someone other than ihatecrypto.";
    let r = execute(&p, ZeroSigner { refuses_orders: Some(TradeError::Guard(guard_text.into())), ..Default::default() }, |s| s.position = d("0.01")).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage"]) && matches!(r.error, Some(Error::Guard(_))) && r.text == guard_text);

    let declined = TradeError::Device { status: Some(0x6985), message: "You declined it on the Ledger. Nothing was signed.".into() };
    let r = execute(&p, ZeroSigner { refuses_orders: Some(declined), ..Default::default() }, |_| {}).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage"]));
    assert_eq!(r.error, Some(Error::Device { status: Some(0x6985), message: "You declined it on the Ledger. Nothing was signed.".into() }));

    // Sent, the reply lost, nothing opened: said as a lost reply, the money on the balance.
    let r = execute(&p, ZeroSigner::default(), |_| {}).await;
    assert!(r.opened.is_none() && r.sent == sent(&["leverage", "entry"]));
    assert!(r.text.starts_with("The entry's reply didn't come") && r.text.contains("no ETH position is open"), "{}", r.text);

    // Sent, the reply lost, the position open: it gets its stop.
    let size = p.sizing.size;
    let r = execute(&p, ZeroSigner::default(), move |s| s.entry = Box::new(move |s| { s.position = size; None })).await;
    let opened = r.opened.unwrap_or_else(|| panic!("{}", r.text));
    assert!(r.sent == sent(&["leverage", "entry", "sl+tp"]) && opened.size == size && opened.entry == d("3000"));
    assert!(r.reported.iter().any(|l| l.starts_with("The entry's reply didn't come, but the ETH position is open")));
}

// MARK: The half at 1R (PartialTakeTests)

/// `add_partial_take` against FakeHL (an ETH position entered at 3000) as `setup` leaves it.
async fn add_partial_take(refuses: bool, setup: impl FnOnce(&mut State)) -> (Vec<String>, crate::Result<PartialTake>, Vec<String>, Vec<Value>) {
    let fake = Fake::start(setup).await;
    let outcome = fake.executor(ZeroSigner { refuses, ..Default::default() }).add_partial_take("ETH").await;
    let s = fake.state();
    (s.sent.clone(), outcome, s.reported.clone(), s.orders.clone())
}

/// A limit still waiting on the book with its stop and take as children (normalTpsl).
fn resting() -> Value {
    json!({"coin": "ETH", "oid": 9, "isTrigger": false, "limitPx": "2990", "orderType": "Limit", "reduceOnly": false, "side": "B", "sz": "0.017",
           "children": [stop(10, "2940"), take(11, "3120")]})
}

#[tokio::test]
async fn the_half_at_1r() {
    // Partly filled: the stop is still the limit's child, armed only on the full fill. Nothing is signed; it waits.
    let r = add_partial_take(false, |s| { s.position = d("0.003"); s.orders = vec![resting()] }).await;
    assert!(r.1 == Ok(PartialTake::Waiting) && r.0.is_empty());

    // Filled: one reduce-only take at 1R (3000 + (3000 − 2940)) for half the position.
    let r = add_partial_take(false, |s| { s.position = d("0.02"); s.orders = vec![stop(2, "2940"), take(3, "3120")] }).await;
    assert!(r.1 == Ok(PartialTake::Placed(d("3060"))) && r.0 == sent(&["tp"]));
    assert!(r.2.iter().any(|l| l.starts_with("Partial take-profit: 0.01 ETH at 1R → 3,060")), "{:?}", r.2);

    // The half went on but its reply was lost: the next try finds it on the book and sends nothing.
    let r = add_partial_take(false, |s| { s.position = d("0.02"); s.orders = vec![stop(2, "2940")]; s.lost = 1 }).await;
    assert!(r.1.is_err() && r.0 == sent(&["tp"]));
    let on_book = r.3;
    let r = add_partial_take(false, |s| { s.position = d("0.02"); s.orders = on_book }).await;
    assert!(r.1 == Ok(PartialTake::Placed(d("3060"))) && r.0.is_empty());
    // The chat was last told it failed: it hears that the half is on.
    assert!(r.2.iter().any(|l| l.starts_with("Partial take-profit: 0.01 ETH at 1R → 3,060")));

    // Hyperliquid turns the half down: an error to try again on, not a quiet None.
    let r = add_partial_take(false, |s| { s.position = d("0.02"); s.orders = vec![stop(2, "2940")]; s.refusals = 1 }).await;
    assert!(matches!(&r.1, Err(Error::Execution(m)) if m.contains("Trigger order has invalid price")) && r.0 == sent(&["tp"]), "{:?}", r.1);

    // Touch ID declined or the signer refused: a guard stop, which the guardian doesn't ask again.
    let r = add_partial_take(true, |s| { s.position = d("0.02"); s.orders = vec![stop(2, "2940")] }).await;
    assert!(matches!(r.1, Err(Error::Guard(_))) && r.0.is_empty());

    // Half of $15 is under the $10 minimum order: skipped, nothing sent.
    let r = add_partial_take(false, |s| { s.position = d("0.005"); s.orders = vec![stop(2, "2940")] }).await;
    assert!(r.1 == Ok(PartialTake::Skipped) && r.0.is_empty());

    // A position with no stop and no limit waiting (the rest of a partly filled limit was cancelled): it says so.
    let r = add_partial_take(false, |s| s.position = d("0.02")).await;
    assert!(matches!(&r.1, Err(Error::Execution(m)) if m.contains("no stop on the exchange")) && r.0.is_empty());
}

// MARK: Trailing stop (TrailingStopTests)

/// «trail 2%» on an open long puts a reduce-only trailing stop for the whole position on the exchange, then cancels the
/// trailing stop it replaces; the fixed stop and take stay. A stop moved afterwards leaves the trailing stop alone.
#[tokio::test]
async fn trail_places_a_trailing_stop_on_the_exchange() {
    let trailing = json!({"coin": "ETH", "oid": 8, "isTrigger": true, "triggerPx": "2950", "orderType": "Trailing Stop Market", "reduceOnly": true,
                          "side": "A", "sz": "0.01"});
    let setup = |s: &mut State| { s.position = d("0.01"); s.orders = vec![stop(2, "2940"), take(3, "3120"), trailing.clone()] };
    let fake = Fake::start(setup).await;
    let e = fake.executor(ZeroSigner::default());
    let line = e.manage(&PositionCommand::Trail(Decimal::TWO), "ETH", &RiskPolicy::default(), None).await.unwrap();
    assert!(line.starts_with("On the exchange: a trailing stop 2% behind the best price"), "{line}");
    assert_eq!(fake.sent(), sent(&["trailing sell 0.01 2%", "cancel 8"]));

    // A stop moved by hand replaces the fixed stop only.
    fake.reset(setup);
    e.manage(&PositionCommand::StopTo(d("2970")), "ETH", &RiskPolicy::default(), None).await.unwrap();
    assert_eq!(fake.sent(), sent(&["sl", "cancel 2"]));
}

// MARK: TWAP entries (TwapExecutionTests)

const RUNNING: &str = r#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"running":{"twapId":77}}}}}"#;

/// The entry is one TWAP; once its first part shows as a position, the stop and take go on for the whole position (size
/// 0, they grow with it). Refused both ways, the TWAP stops and what it bought is closed.
#[tokio::test]
async fn a_twap_entry_gets_its_stop_for_the_whole_position() {
    let p = plan("long ETH 300 USDC over 2 h stop 2% take 4% risk $6 leverage 1x", 1);
    let starts = |s: &mut State| s.entry = Box::new(|s| { s.position = d("0.0008"); Some(RUNNING.into()) });

    // Started, the first part filled: the stop and take for the whole position, nothing else.
    let r = execute(&p, ZeroSigner::default(), starts).await;
    assert!(r.sent == sent(&["leverage", "twap 120 min", "sl+tp"]) && r.text.is_empty(), "{:?} {}", r.sent, r.text);
    assert_eq!(r.protections.iter().map(|o| o["s"].clone()).collect::<Vec<_>>(), [json!("0"), json!("0")]);
    let opened = r.opened.unwrap();
    assert!(opened.twap_id == Some(77) && opened.size == p.sizing.size && opened.stop == d("2940") && opened.take == Some(d("3120")));

    // Hyperliquid won't start it: nothing bought, nothing else sent.
    let r = execute(&p, ZeroSigner::default(), |s| s.entry = Box::new(|_| {
        Some(r#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"error":"Invalid TWAP duration: 1 min(s)"}}}}"#.into())
    })).await;
    assert!(r.sent == sent(&["leverage", "twap 120 min"]) && r.text.contains("didn't start the TWAP") && r.opened.is_none());

    // The stop is refused both ways: the TWAP stops first, then what it bought is closed.
    let r = execute(&p, ZeroSigner::default(), |s| {
        s.refusals = 2;
        starts(s);
        s.close = Box::new(|s| { s.position = Decimal::ZERO; Some(filled("0.0008")) });
    }).await;
    assert_eq!(r.sent, sent(&["leverage", "twap 120 min", "sl+tp", "sl+tp", "twap cancel 77", "close sell"]));
    assert!(r.text.contains("The TWAP is stopped") && r.opened.is_none(), "{}", r.text);
}

/// «cancel the TWAP» stops a running one by its id and keeps the position's stop; «close» stops it before closing; the
/// guardian's look at it each pass; and a stop moved during it stays one for the whole position.
#[tokio::test]
async fn a_running_twap_stops_by_its_id() {
    let row = json!({"state": {"coin": "ETH", "side": "B", "sz": "0.1", "minutes": 120}, "status": {"status": "activated"}, "twapId": 77});
    // twapHistory is a log: a TWAP that finished has its «activated» row too, and isn't running.
    let done = [json!({"state": {"coin": "ETH"}, "status": {"status": "finished"}, "twapId": 5}),
                json!({"state": {"coin": "ETH"}, "status": {"status": "activated"}, "twapId": 5})];
    let policy = RiskPolicy::default();
    let fake = Fake::start(|s| {
        s.position = d("0.01");
        s.orders = vec![stop(2, "2940"), take(3, "3120")];
        s.twaps = [vec![row.clone()], done.to_vec()].concat();
    }).await;
    let e = fake.executor(ZeroSigner::default());
    let line = e.manage(&PositionCommand::CancelOrders, "ETH", &policy, None).await.unwrap();
    assert_eq!(line, "ETH TWAP stopped (1). What it bought stays open with its stop and take-profit.");
    assert_eq!(fake.sent(), sent(&["twap cancel 77"]));

    fake.reset(|s| {
        s.position = d("0.01");
        s.orders = vec![stop(2, "2940")];
        s.close = Box::new(|s| { s.position = Decimal::ZERO; Some(filled("0.01")) });
        s.twaps = vec![row.clone()];
    });
    e.close("ETH").await.unwrap();
    assert_eq!(fake.sent()[..2], sent(&["twap cancel 77", "close sell"]));

    // The guardian, each pass: with the stop on the exchange and the loss at it within 1.5 × the plan's, nothing is done.
    let whole_stop = json!({"coin": "ETH", "oid": 2, "isTrigger": true, "triggerPx": "2940", "orderType": "Stop Market", "reduceOnly": true,
                            "side": "A", "sz": "0.0", "isPositionTpsl": true});
    let watch = async |risk: Option<&str>, setup: &dyn Fn(&mut State)| {
        fake.reset(|s| { s.twaps = vec![row.clone()]; setup(s) });
        let w = e.guard_twap("ETH", risk.map(d)).await;
        (w, fake.sent())
    };
    let w = watch(None, &|s| { s.position = d("0.01"); s.orders = vec![whole_stop.clone()] }).await;
    assert!(w.0 == Ok(TwapWatch::Running) && w.1.is_empty());
    // Its stop fired and the TWAP bought again: stopped first, then what it bought with no stop is closed.
    let w = watch(None, &|s| { s.position = d("0.004"); s.close = Box::new(|s| { s.position = Decimal::ZERO; Some(filled("0.004")) }) }).await;
    assert_eq!(w.1, sent(&["twap cancel 77", "close sell"]));
    assert!(matches!(&w.0, Ok(TwapWatch::Stopped(l)) if l.starts_with("The ETH TWAP kept buying after its stop: it's stopped. ETH position closed")), "{:?}", w.0);
    // Flat (its stop or take closed it): only the TWAP stops.
    let w = watch(None, &|s| s.position = Decimal::ZERO).await;
    assert!(w.0 == Ok(TwapWatch::Stopped("The ETH TWAP is stopped: its position's stop or take closed it.".into())) && w.1 == sent(&["twap cancel 77"]));
    // Hyperliquid doesn't run it any more: nothing left to watch.
    let w = watch(None, &|s| { s.twaps = vec![]; s.position = d("0.1"); s.orders = vec![whole_stop.clone()] }).await;
    assert!(w.0 == Ok(TwapWatch::Ended) && w.1.is_empty());
    // A TWAP that won't stop throws: nothing is closed, and the guardian keeps watching it.
    for position in ["0", "0.004"] {
        let w = watch(None, &|s| { s.position = d(position); s.twap_cancel_refused = true }).await;
        assert!(matches!(w.0, Err(Error::Execution(_))) && w.1 == sent(&["twap cancel 77"]), "{w:?}");
    }
    // The loss at the stop past 1.5 × the plan's: 0.05 ETH held from 3000 and 0.05 still to buy at 3000, both down to 2940,
    // is $6: over 1.5 × $3, within 1.5 × $5.
    let w = watch(Some("3"), &|s| { s.position = d("0.05"); s.orders = vec![whole_stop.clone()] }).await;
    assert_eq!(w.1, sent(&["twap cancel 77"]));
    assert!(matches!(&w.0, Ok(TwapWatch::Stopped(l)) if l.contains("would lose $6.00, more than 1.5 × the plan's $3.00")), "{:?}", w.0);
    let w = watch(Some("5"), &|s| { s.position = d("0.05"); s.orders = vec![whole_stop.clone()] }).await;
    assert!(w.0 == Ok(TwapWatch::Running) && w.1.is_empty());

    // A stop moved by hand during a TWAP stays one for the whole position (size 0).
    fake.reset(|s| { s.position = d("0.01"); s.orders = vec![whole_stop.clone()] });
    e.manage(&PositionCommand::StopTo(d("2970")), "ETH", &policy, None).await.unwrap();
    assert_eq!(fake.sent(), sent(&["sl", "cancel 2"]));
    assert_eq!(fake.state().protections.iter().map(|o| o["s"].clone()).collect::<Vec<_>>(), [json!("0")]);

    // A wider stop is held to the rule on everything the TWAP builds: 0.01 held and 0.09 still to buy, $100 under the price,
    // is −$10.56 of $100 capital. Without the TWAP the same stop is −$1.06 and goes.
    fake.reset(|s| { s.position = d("0.01"); s.orders = vec![stop(2, "2940")]; s.twaps = vec![row.clone()] });
    let wide = e.manage(&PositionCommand::StopTo(d("2900")), "ETH", &policy, Some(d("100"))).await;
    assert!(matches!(&wide, Err(Error::Execution(m)) if m.contains("−$10.56")) && fake.sent().is_empty(), "{wide:?}");
    fake.reset(|s| { s.position = d("0.01"); s.orders = vec![stop(2, "2940")] });
    e.manage(&PositionCommand::StopTo(d("2900")), "ETH", &policy, Some(d("100"))).await.unwrap();
    assert_eq!(fake.sent(), sent(&["sl", "cancel 2"]));
}

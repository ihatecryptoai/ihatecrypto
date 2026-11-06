//! Portfolio3Tests (the guardian's rules from a fill, the position's line and duties), Rest9Tests and Runs8Tests (one
//! reason whatever time it names), and the rules kept per account, offline in a test folder of their own.

use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Value, json};
use signer_api::{SignerReply, SignerWallet};
use trade::executor::Position;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::snapshot::Snapshot;

fn d(s: &str) -> Decimal { s.parse().expect("a decimal") }
fn en(english: &str, _: &str) -> String { english.into() }

fn engine() -> Rc<Engine> {
    fn draw(_: &Engine) -> Snapshot { unreachable!("the test window is locked") }
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), draw);
    e.st_mut().lock.locked = true;
    e.st_mut().settings.language = Some("en".into());
    e
}

/// A wallet whose Hyperliquid account is `evm` (Hardhat's first account: public test data).
fn signed_in(e: &Engine, evm: &str) {
    let w = SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(), solana: None, evm: Some(evm.into()), near: None, test: None, automation: None };
    e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(evm.into()), wallets: Some(vec![w]), active_wallet: Some("1".into()),
                                           ..SignerReply::default() });
}

const ME: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

#[test]
fn a_plans_fill_leaves_the_guardian_its_rules() {
    let mut intent = TradeIntent { side: Side::Long, ..TradeIntent::default() };
    // The exchange holds the stop and the take: nothing for the guardian.
    assert_eq!(Engine::exit_rules_for(d("1"), d("100"), false, None, &intent), None);
    // A partial take: the stop goes to breakeven after it.
    let partial = Engine::exit_rules_for(d("1"), d("100"), false, Some(d("110")), &intent).expect("rules");
    assert!(partial.breakeven_after_partial && !partial.partial_order_pending && partial.trailing_pct.is_none());
    // A limit entry with a take in parts: the half at 1R waits for the fill.
    intent.partial_take = Some(true);
    assert!(Engine::exit_rules_for(d("1"), d("100"), true, None, &intent).is_some_and(|r| r.partial_order_pending));
    // A trailing stop alone.
    intent.partial_take = None;
    intent.trailing_pct = Some(d("2"));
    let trailing = Engine::exit_rules_for(d("1"), d("100"), false, None, &intent).expect("rules");
    assert!(trailing.trailing_pct == Some(d("2")) && !trailing.breakeven_after_partial && trailing.long);
}

fn orders(v: Value) -> Vec<Order> { v.as_array().expect("a list").iter().map(|o| Order::read(o).expect("an order")).collect() }

#[test]
fn the_position_says_its_stop_takes_and_guardian() {
    let book = orders(json!([
        {"coin": "ETH", "oid": 1, "isTrigger": true, "triggerPx": "2300", "orderType": "Stop Market", "reduceOnly": true},
        {"coin": "ETH", "oid": 2, "isTrigger": true, "triggerPx": "2600", "orderType": "Take Profit Market", "reduceOnly": true},
        {"coin": "ETH", "oid": 3, "isTrigger": true, "triggerPx": "2500", "orderType": "Take Profit Market", "reduceOnly": true},
        {"coin": "BTC", "oid": 4, "isTrigger": true, "triggerPx": "90000", "orderType": "Stop Market", "reduceOnly": true}]));
    let line = Engine::summary("ETH", d("0.05"), d("2412.5"), &book, en);
    assert_eq!(line, "0.05 at 2,412.5 · stop 2,300 · take 2,500 and 2,600");
    // A short's nearest take is the higher one; no stop says so.
    assert_eq!(Engine::summary("ETH", d("-1"), d("2700"), &book, en), "1 at 2,700 · stop 2,300 · take 2,600 and 2,500");
    assert!(Engine::summary("SOL", d("-3"), d("150"), &book, en).ends_with("stop none"));

    let mut rules = ExitRules::new(d("1"), d("100"), true, true, Some(d("2")));
    let said = Engine::duties(&rules, en);
    assert!(said.len() == 2 && said[0].starts_with("After the first half, moves the stop to breakeven") && said[1].starts_with("Trailing 2%"), "{said:?}");
    rules.moved_to_breakeven = true;
    assert!(Engine::duties(&rules, en)[0].starts_with("Stop is already at breakeven"));
    assert_eq!(Engine::duties(&ExitRules::new(d("1"), d("1"), true, false, None), en), ["Stop and take-profit sit on the exchange; ihatecrypto doesn't move anything."]);
}

/// 11:58 AM and 12:08 PM are one reason: the time it asks again goes with its AM/PM.
#[test]
fn a_reason_is_the_same_whatever_time_it_names() {
    let morning = "The trading key wasn't opened, so this order wasn't signed. ihatecrypto asks again from 11:58 AM.";
    let noon = "The trading key wasn't opened, so this order wasn't signed. ihatecrypto asks again from 12:08 PM.";
    assert_eq!(Engine::reason_only(morning), Engine::reason_only(noon));
    assert_ne!(Engine::reason_only(morning), Engine::reason_only("Hyperliquid refused the order: reduce only."));
    let first = "The Hyperliquid trading key … wasn't opened. Orders through it ask again from 10:42 AM, and any move opens it.";
    let later = "The Hyperliquid trading key … wasn't opened. Orders through it ask again from 10:52 AM, and any move opens it.";
    assert_eq!(Engine::reason_only(first), Engine::reason_only(later));
    // Fmt.time's narrow no-break space before AM is a space too.
    assert_eq!(Engine::reason_only("from 9:05\u{202f}PM."), Engine::reason_only("from 10:15\u{202f}AM."));
}

/// Rules are kept per Hyperliquid account in exit-rules.json; a trailing stop the exchange holds leaves none.
#[tokio::test]
async fn the_rules_are_kept_per_account() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let intent = TradeIntent { asset: "ETH".into(), trailing_pct: Some(d("2")), ..TradeIntent::default() };
        let opened = Opened::new("ETH", d("1"), d("100"), d("97"), None);
        // No account: nothing to keep them under.
        e.keep_exit_rules(&opened, &intent);
        assert!(store::load::<AllRules>("exit-rules.json").is_none());
        signed_in(&e, ME);
        e.keep_exit_rules(&opened, &intent);
        assert_eq!(e.exit_rules()["ETH"].trailing_pct, Some(d("2")));
        assert!(store::load::<AllRules>("exit-rules.json").expect("the file").contains_key(&ME.to_lowercase()));
        // On the exchange already: the app moves nothing, and the coin's old rules go.
        e.keep_exit_rules(&Opened { trailing_on_exchange: true, ..opened.clone() }, &intent);
        assert!(e.exit_rules().is_empty());
        assert!(store::load::<AllRules>("exit-rules.json").expect("the file").is_empty());
        // Another account sees none of them.
        e.keep_exit_rules(&opened, &intent);
        signed_in(&e, "0x70997970C51812dc3A010C7d01b50e0d17dc79C8");
        assert!(e.exit_rules().is_empty());
    }).await;
}

/// «trail 2%» keeps the stop by the app, from the position's entry; with no position it says so.
#[tokio::test]
async fn trailing_a_position() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        signed_in(&e, ME);
        assert_eq!(e.trail("ETH", d("2")), "There is no open ETH position to trail.");
        e.st_mut().positions = vec![Position { coin: "ETH".into(), size: d("-0.5"), entry: d("2000"), unrealized_pnl: Decimal::ZERO, liquidation: None }];
        assert!(e.trail("ETH", d("2")).starts_with("ihatecrypto now keeps the ETH stop 2% behind the best price"));
        let r = e.exit_rules()["ETH"].clone();
        assert!(!r.long && r.opened_size == d("0.5") && r.entry == d("2000") && r.trailing_pct == Some(d("2")) && !r.breakeven_after_partial);
    }).await;
}

/// The window's positions: the card's line, the guardian's duties, the ruler and the exchange's name.
#[tokio::test]
async fn the_positions_as_the_window_draws_them() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        signed_in(&e, ME);
        e.set_exit_rules(HashMap::from([("ETH".to_string(), ExitRules::new(d("0.05"), d("2412.5"), true, false, Some(d("2"))))]));
        {
            let mut st = e.st_mut();
            st.positions = vec![
                Position { coin: "ETH".into(), size: d("0.05"), entry: d("2412.5"), unrealized_pnl: d("1.25"), liquidation: Some(d("1900")) },
                Position { coin: "SOL".into(), size: d("-3"), entry: d("150"), unrealized_pnl: d("-0.5"), liquidation: None },
            ];
            st.hl_orders = orders(json!([{"coin": "ETH", "oid": 1, "isTrigger": true, "triggerPx": "2300", "orderType": "Stop Market", "reduceOnly": true}]));
        }
        let views = e.position_views();
        assert_eq!((views[0].side.as_str(), views[0].size.as_str(), views[0].pnl.as_str(), views[0].liquidation.as_deref()), ("long", "0.05", "$1.25", Some("1,900")));
        assert_eq!(views[0].summary.as_deref(), Some("0.05 at 2,412.5 · stop 2,300"));
        assert_eq!(views[0].guardian.as_deref(), Some("Trailing 2%: the stop follows the best price, now 2,412.5."));
        assert_eq!(views[0].ruler.as_ref().and_then(|r| r.guard_line.as_deref()), Some("Guardian is watching"));
        assert_eq!((views[1].side.as_str(), views[1].pnl.as_str(), views[1].guardian.as_deref(), views[1].market.as_deref()), ("short", "−$0.50", None, Some("SOL")));
    }).await;
}

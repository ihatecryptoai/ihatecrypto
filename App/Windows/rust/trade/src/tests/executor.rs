//! ExecutorTests' readers and rules, HedgeRiskTests, and the reply parts of TrailingStopTests and TwapExecutionTests.

use std::collections::HashMap;

use ihatecrypto_core::position_command::PositionCommand;
use ihatecrypto_core::risk::RiskPolicy;
use rust_decimal::Decimal;

use super::d;
use crate::executor::{HlReply, HyperliquidAccount, Position, Twap};
use crate::{CostlyMove, Error, Executor, LOSS_ACCEPTED};

fn reply(json: &str) -> HlReply { HlReply::parse(json.as_bytes()) }

fn position(coin: &str, size: &str, entry: &str) -> Position {
    Position { coin: coin.into(), size: d(size), entry: d(entry), unrealized_pnl: Decimal::ZERO, liquidation: None }
}

#[test]
fn exchange_replies_are_read_strictly() {
    assert_eq!(reply(r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"filled":{"totalSz":"0.012","avgPx":"2680.5","oid":1}}]}}}"#),
               HlReply::Filled { size: d("0.012"), avg_px: d("2680.5") });
    assert_eq!(reply(r#"{"status":"ok","response":{"type":"default"}}"#), HlReply::Ok);
    assert!(reply(r#"{"status":"ok","response":{"type":"order","data":{"statuses":["waitingForTrigger",{"error":"Order has invalid price."}]}}}"#).is_error());
    assert!(reply(r#"{"status":"err","response":"User or API Wallet does not exist."}"#).is_error());
    assert!(reply("not json").is_error());
    // One status, not a list: a trailing stop's, a TWAP's.
    assert_eq!(reply(r#"{"status":"ok","response":{"type":"trailingStop","data":{"oid":77738308}}}"#), HlReply::Ok);
    assert_eq!(reply(r#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"error":"Invalid TWAP duration: 1 min(s)"}}}}"#),
               HlReply::Error("Invalid TWAP duration: 1 min(s)".into()));
    assert_eq!(reply(r#"{"status":"ok","response":{"type":"twapOrder","data":{"status":{"running":{"twapId":77738308}}}}}"#), HlReply::Running(77738308));
    assert_eq!(reply(r#"{"status":"ok","response":{"type":"twapCancel","data":{"status":"success"}}}"#), HlReply::Ok);
}

#[test]
fn stop_and_take_follow_the_actual_fill() {
    assert_eq!(Executor::protection_prices(d("2000"), d("3"), Some(d("6")), true, 4).unwrap(), (d("1940"), Some(d("2120"))));
    assert_eq!(Executor::protection_prices(d("2000"), d("3"), None, false, 4).unwrap(), (d("2060"), None));
    // Market triggers close on the opposite side, reduce-only, and may fill up to 10% past the trigger.
    let json = Executor::protection_action(1, true, d("0.012"), d("1940"), Some(d("2120")), 4).unwrap().to_json();
    let orders = json["orders"].as_array().unwrap();
    assert!(json["grouping"] == "positionTpsl" && orders.len() == 2);
    assert!(orders.iter().all(|o| o["b"] == false && o["r"] == true));
    assert_eq!(orders[0]["p"], "1746");
}

#[test]
fn risk_at_stops_uses_the_nearest_stop_and_flags_a_missing_one() {
    let orders = HyperliquidAccount::parse_orders(br#"
    [{"coin":"ETH","oid":1,"isTrigger":true,"triggerPx":"1940","orderType":"Stop Market"},
     {"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"1960","orderType":"Stop Market"},
     {"coin":"ETH","oid":3,"isTrigger":true,"triggerPx":"2200","orderType":"Take Profit Market"}]"#).unwrap();
    let long = position("ETH", "0.5", "2000");
    assert_eq!(HyperliquidAccount::risk_at_stops(&[long.clone()], &orders, &HashMap::new()), Some(d("20")));   // (2000 − 1960) × 0.5
    let naked = position("BTC", "-1", "80000");
    assert_eq!(HyperliquidAccount::risk_at_stops(&[long, naked], &orders, &HashMap::new()), None);
}

#[test]
fn resting_entries_count_at_their_stops() {
    // Four limit entries at 2% each counted for nothing before: filled, they'd be 8% at the stops against the 6% rule.
    let orders = HyperliquidAccount::parse_orders(br#"
    [{"coin":"ETH","oid":1,"isTrigger":false,"limitPx":"2000","sz":"0.5","side":"B","orderType":"Limit","reduceOnly":false,
      "children":[{"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"1960","orderType":"Stop Market","reduceOnly":true},
                  {"coin":"ETH","oid":3,"isTrigger":true,"triggerPx":"2100","orderType":"Take Profit Market","reduceOnly":true}]},
     {"coin":"SOL","oid":4,"isTrigger":false,"limitPx":"100","sz":"2","side":"A","orderType":"Limit","reduceOnly":false,"children":[]},
     {"coin":"SOL","oid":5,"isTrigger":true,"triggerPx":"105","orderType":"Stop Market","reduceOnly":true},
     {"coin":"BTC","oid":6,"isTrigger":false,"limitPx":"90000","sz":"0.01","side":"A","orderType":"Limit","reduceOnly":true}]"#).unwrap();
    // ETH (2000 − 1960) × 0.5 = 20, SOL short (105 − 100) × 2 = 10; BTC is a reduce-only close, not an entry.
    assert_eq!(HyperliquidAccount::risk_at_stops(&[], &orders, &HashMap::new()), Some(d("30")));
    let naked = HyperliquidAccount::parse_orders(br#"[{"coin":"ETH","oid":1,"isTrigger":false,"limitPx":"2000","sz":"0.5","side":"B","orderType":"Limit"}]"#).unwrap();
    assert_eq!(HyperliquidAccount::risk_at_stops(&[], &naked, &HashMap::new()), None);
}

#[test]
fn a_short_of_coins_held_adds_no_risk_at_its_stop() {
    let orders = HyperliquidAccount::parse_orders(br#"[{"coin":"SOL","oid":1,"isTrigger":true,"triggerPx":"137","orderType":"Stop Market"}]"#).unwrap();
    let short = position("SOL", "-0.2", "100");
    // Alone it loses (137 − 100) × 0.2 at the stop; with the SOL it covers held, nothing; half held, half.
    assert_eq!(HyperliquidAccount::risk_at_stops(&[short.clone()], &orders, &HashMap::new()), Some(d("7.4")));
    assert_eq!(HyperliquidAccount::risk_at_stops(&[short.clone()], &orders, &[("SOL".into(), d("0.25"))].into()), Some(Decimal::ZERO));
    assert_eq!(HyperliquidAccount::risk_at_stops(&[short], &orders, &[("SOL".into(), d("0.1"))].into()), Some(d("3.7")));
    // A long is never covered by coins held: they fall with it.
    let long = position("SOL", "0.2", "100");
    let below = HyperliquidAccount::parse_orders(br#"[{"coin":"SOL","oid":1,"isTrigger":true,"triggerPx":"95","orderType":"Stop Market"}]"#).unwrap();
    assert_eq!(HyperliquidAccount::risk_at_stops(&[long], &below, &[("SOL".into(), Decimal::ONE)].into()), Some(Decimal::ONE));
}

#[test]
fn a_wider_stop_is_held_to_the_per_trade_rule() {
    let policy = RiskPolicy::default();   // 2% per trade
    let rule = |long, stop: &str, current: Option<&str>, equity: Option<&str>| {
        Executor::stop_breaks_rule(long, d("120000"), d("0.01"), d(stop), current.map(d), equity.map(d), &policy)
    };
    // Long 0.01 BTC at 120000 with the stop at 118000; capital $1000. To 90000: −$300 and fees, 30%: refused.
    assert!(rule(true, "90000", Some("118000"), Some("1000")).is_some_and(|l| l > d("300")));
    // Tightening, a stop in profit, a first stop, unknown capital: never refused.
    assert_eq!(rule(true, "119000", Some("118000"), Some("1000")), None);
    assert_eq!(rule(false, "121000", Some("122000"), Some("1000")), None);
    assert_eq!(rule(true, "90000", None, Some("1000")), None);
    assert_eq!(rule(true, "90000", Some("118000"), None), None);
    // 117000 is −$31.07 with fees: 3.1% of $1000 is refused, 1.6% of $2000 is within the rule.
    assert!(rule(true, "117000", Some("118000"), Some("1000")).is_some());
    assert_eq!(rule(true, "117000", Some("118000"), Some("2000")), None);
}

#[test]
fn a_share_below_one_step_is_refused_not_a_full_close() {
    let btc = d("0.00015");   // ≈ $12 at $84k, BTC steps of 0.00001
    for text in ["close 5% BTC", "закрой 5% BTC"] {
        let Some(PositionCommand::Close(share)) = PositionCommand::parse(text) else { panic!("{text} didn't parse") };
        assert!(matches!(Executor::partial_close_size(btc, share, 5, "BTC"), Err(Error::Execution(_))), "{text}");
    }
    for text in ["close half", "закрой половину"] {
        let Some(PositionCommand::Close(share)) = PositionCommand::parse(text) else { panic!("{text} didn't parse") };
        assert_eq!(Executor::partial_close_size(btc, share, 5, "BTC").unwrap(), d("0.00007"), "{text}");
    }
    let third = Decimal::ONE / Decimal::from(3);
    assert_eq!(Executor::partial_close_size(d("-1000"), third, 0, "kPEPE").unwrap(), d("333"));   // a short too
    // A third is 0.333…3: rounded down as it was, a third of 3 steps was none and a third of 0.00015 one step short.
    assert_eq!(Executor::partial_close_size(d("3"), third, 0, "kPEPE").unwrap(), Decimal::ONE);
    assert_eq!(Executor::partial_close_size(btc, third, 5, "BTC").unwrap(), d("0.00005"));
}

/// A running TWAP's parts not yet bought count in the open risk at its coin's stop; with no stop the sum is unknown.
#[test]
fn a_running_twap_counts_in_the_open_risk() {
    let twap = Twap { id: 1, coin: "ETH".into(), size: d("0.1"), is_buy: true };
    let marks: HashMap<String, Decimal> = [("ETH".into(), d("3000"))].into();
    let held = position("ETH", "0.04", "3000");
    let stop = HyperliquidAccount::parse_orders(
        br#"[{"coin":"ETH","oid":2,"isTrigger":true,"triggerPx":"2940","orderType":"Stop Market","reduceOnly":true,"side":"A","sz":"0.0"}]"#).unwrap();
    // 0.06 ETH still to buy, $60 above the stop.
    assert_eq!(HyperliquidAccount::twap_risk(&[twap.clone()], &[held.clone()], &stop, &marks), Some(d("3.6")));
    assert_eq!(HyperliquidAccount::twap_risk(&[twap], &[held], &[], &marks), None);
    assert_eq!(HyperliquidAccount::twap_risk(&[], &[], &[], &marks), Some(Decimal::ZERO));
}

/// A move losing more than its cap is asked about, not refused: it stops with the numbers, and the same move passes once
/// the user takes that loss («Send anyway»).
#[test]
fn a_costly_move_is_asked_not_refused() {
    let (cap, lost) = (d("0.15"), d("0.23"));
    assert!(matches!(Executor::check_loss(lost, Decimal::TWO, cap, "Relay"), Err(Error::Costly(_))));
    Executor::check_loss(d("0.1"), Decimal::TWO, cap, "Relay").unwrap();
    LOSS_ACCEPTED.sync_scope(d("0.34"), || Executor::check_loss(lost, Decimal::TWO, cap, "Relay")).unwrap();
    assert_eq!(CostlyMove::new(lost, Decimal::TWO, "Relay").description(),
               "Relay keeps ≈ $0.23 of $2.00 (12%): more than a move may lose on its own. Nothing was signed.");
}

/// The engine runs a move on its own task: what it awaits must be Send.
#[allow(dead_code, unused_must_use, clippy::let_underscore_future)]
fn the_moves_can_run_on_their_own_task(e: &Executor, p: &crate::executor::Prepared, net: &crate::EvmNetwork, m: &evm::protocols::aave::Market,
                                       v: &evm::protocols::morpho::Vault) {
    fn send<T: Send>(_: T) {}
    send(e.prepare(&p.intent, &RiskPolicy::default(), false, &HashMap::new()));
    send(e.execute(p));
    send(e.close("ETH"));
    send(e.manage(&PositionCommand::CancelOrders, "ETH", &RiskPolicy::default(), None));
    send(e.guard_twap("ETH", None));
    send(e.bridge(net, None, false, None));
    send(e.solana_to_evm(net, Decimal::ONE));
    send(e.top_up_gas(net, Decimal::ONE));
    send(e.aave(true, None, m));
    send(e.aave_borrow(m, Decimal::ONE));
    send(e.morpho(true, None, v));
    send(e.cctp_bridge(net, net, None));
    send(e.evm_places());
    send(e.resolve_transit(chrono::Utc::now()));
}

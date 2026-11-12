//! LimitOrderGuardTests.swift: a limit order's deposit goes only to an order 1Click holds as asked, read back by its id.

use guard::Verdict;
use guard::order::{self, Asked};
use protocols::one_click::{self, Order};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use super::d;

// The test wallet's addresses from the public phrase «abandon … about» would do as well: any two strings stand for «ours».
const ME: &str = "2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj";
const DEPOSIT: &str = "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw";
const SOL: &str = "nep141:sol.omft.near";
const USDC: &str = "nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near";
const NOW: i64 = 1_794_398_400;   // 2026-11-11T12:00:00Z
const ENDS: i64 = NOW + 7 * 86_400;

/// 1Click's answer for a sale of 0.1 SOL at 200 USDC, as GET /v0/orders/{id} gives it (the shape of 2026-11-11).
fn order(change: impl FnOnce(&mut Value)) -> Order {
    let mut a = json!({
        "orderType": "LIMIT", "depositAddress": DEPOSIT, "depositMode": "SIMPLE", "depositType": "ORIGIN_CHAIN", "fillStatus": "AWAITING_DEPOSIT",
        "payoutStatus": "NOT_STARTED", "isPayoutStatusFinal": false, "depositedAmount": "0", "baseAsset": SOL, "quoteAsset": USDC,
        "quantity": "100000000", "side": "SELL", "price": "200", "recipient": ME, "recipientType": "DESTINATION_CHAIN", "refundTo": ME,
        "refundType": "ORIGIN_CHAIN", "confidentiality": "basic", "timeInForce": "GTC", "deadline": "2026-11-18T12:00:00.000Z",
        "createdAt": "2026-11-11T12:00:00.000Z", "appFees": [{"recipient": "ihatecrypto.near", "fee": 5}],
        "swapView": {"swapType": "EXACT_INPUT", "originAsset": SOL, "destinationAsset": USDC, "amountIn": "100000000", "minAmountOut": "20000000"},
        "partialFills": [],
    });
    change(&mut a);
    one_click::parse_order(&json!({"type": "orders", "id": "ord_1", "attributes": a})).unwrap()
}

fn check_with(o: &Order, deposit: &str, most: Decimal, sell: bool) -> Verdict {
    order::check(&o.fields, &Asked {
        sell, base: SOL, quote: USDC, quantity: "100000000", price: d("200"), deposit_address: DEPOSIT, recipient: ME, refund_to: ME, deposit,
        most, fee: one_click::ORDER_FEE, deadline: ENDS, now: NOW,
    })
}

fn check(o: &Order) -> Verdict { check_with(o, "100000000", d("100000000"), true) }

fn set(key: &'static str, v: Value) -> impl FnOnce(&mut Value) { move |a| a[key] = v }

#[test]
fn an_order_held_as_asked_passes() {
    assert_eq!(chrono::DateTime::from_timestamp(NOW, 0).unwrap().to_rfc3339(), "2026-11-11T12:00:00+00:00");
    let v = check(&order(|_| {}));
    assert!(v.ok(), "{:?}", v.problems);
    // 1Click lists its own 20 basis points beside ours.
    let both = check(&order(set("appFees", json!([{"recipient": "ihatecrypto.near", "fee": 5}, {"recipient": "oneclick-fees.near", "fee": 20}]))));
    assert!(both.ok(), "{:?}", both.problems);
}

#[test]
fn the_guard_refuses_an_order_not_as_asked() {
    let scam = "ScamXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1";
    // Someone else's payout, someone else's refund, a public order, a fee that isn't ours, a higher one, a memo.
    assert!(!check(&order(set("recipient", json!(scam)))).ok());
    assert!(!check(&order(set("refundTo", json!(scam)))).ok());
    assert!(!check(&order(set("confidentiality", json!("public")))).ok());
    assert!(!check(&order(set("appFees", json!([{"recipient": "someone.near", "fee": 25}])))).ok());
    assert!(!check(&order(set("appFees", json!([{"recipient": "ihatecrypto.near", "fee": 5}, {"recipient": "a.near", "fee": 15}, {"recipient": "b.near", "fee": 15}])))).ok());
    assert!(!check(&order(set("appFees", json!([{"recipient": "ihatecrypto.near", "fee": 50}])))).ok());
    assert!(!check(&order(set("depositMemo", json!("123")))).ok());
    // The wrong side, price, pair or deposit address.
    assert!(!check(&order(set("side", json!("BUY")))).ok());
    assert!(!check(&order(set("price", json!("150")))).ok());
    assert!(!check(&order(set("baseAsset", json!("nep141:wrap.near")))).ok());
    assert!(!check(&order(set("depositAddress", json!("Other1111111111111111111111111111111111111")))).ok());
    // Resting far longer than asked, or about to run out.
    assert!(!check(&order(set("deadline", json!("2027-01-01T00:00:00.000Z")))).ok());
    assert!(!check(&order(set("deadline", json!("2026-11-11T12:01:00.000Z")))).ok());
    // A deposit other than what the swap view takes, or more than approved.
    assert!(!check_with(&order(|_| {}), "100000001", d("100000000"), true).ok());
    assert!(!check_with(&order(|_| {}), "100000000", d("99999999"), true).ok());
}

#[test]
fn a_buy_deposits_its_swap_views_maximum_within_what_was_asked() {
    // A buy of 0.1 SOL at 200 for at most 20 USDC: the swap view's maxAmountIn is what goes in.
    let buy = order(|a| {
        a["side"] = json!("BUY");
        a["swapView"] = json!({"swapType": "EXACT_OUTPUT", "originAsset": USDC, "destinationAsset": SOL, "maxAmountIn": "19950000", "amountOut": "100000000"});
    });
    assert!(check_with(&buy, "19950000", d("20000000"), false).ok());
    assert!(!check_with(&buy, "19950000", d("19000000"), false).ok());
    assert!(!check_with(&buy, "20000000", d("20000000"), false).ok());
}

#[test]
fn a_price_through_the_market_fills_at_once() {
    assert!(order::fills_at_once(true, d("140"), d("142")));
    assert!(!order::fills_at_once(true, d("200"), d("142")));
    assert!(order::fills_at_once(false, d("150"), d("142")));
    assert!(!order::fills_at_once(false, d("120"), d("142")));
}

#[test]
fn an_orders_fills_add_up() {
    let o = order(|a| {
        a["fillStatus"] = json!("PARTIALLY_FILLED");
        a["partialFills"] = json!([{"id": "f1", "amountIn": "30000000", "amountOut": "6000000", "createdAt": "", "updatedAt": ""},
                                   {"id": "f2", "amountIn": "20000000", "amountOut": "4000000", "createdAt": "", "updatedAt": ""}]);
    });
    assert_eq!(o.get("filledIn"), Some("50000000"));
    assert_eq!(o.get("filledOut"), Some("10000000"));
    assert!(o.is_open());
    assert!(!order(set("fillStatus", json!("FILLED"))).is_open());
}

#[test]
fn an_order_id_stays_one_path_segment() {
    assert_eq!(one_click::order_path("order_Ab1-x.~"), "order_Ab1-x.~");
    assert_eq!(one_click::order_path("a/../b?c"), "a%2F..%2Fb%3Fc");
    assert!(one_click::parse_orders(br#"{"data":[]}"#, 200).unwrap().is_empty());
    assert!(one_click::parse_orders(br#"{"message":"Unauthorized"}"#, 401).is_err());
}

#[test]
fn the_journal_follows_an_order_to_its_payout() {
    use execution::Kind;
    use crate::Executor;
    // Seen after the move's three minutes, the deposit confirms the order; a final payout settles it; a cancel alone doesn't.
    assert_eq!(Executor::limit_journal_step(Kind::Signed, "OPEN", false), Some(Kind::Confirmed));
    assert_eq!(Executor::limit_journal_step(Kind::Signed, "AWAITING_DEPOSIT", false), None);
    assert_eq!(Executor::limit_journal_step(Kind::Confirmed, "PENDING_CANCEL", false), None);
    assert_eq!(Executor::limit_journal_step(Kind::Confirmed, "CANCELED", true), Some(Kind::Settled));
    assert_eq!(order(|a| a["isPayoutStatusFinal"] = json!(true)).get("isPayoutStatusFinal"), Some("true"));
}

// 1Click's quote has the payout's network fee off its output: the price it matches at puts it back (0.7 USDC → SOL,
// 2026-11-12: 0.008365867 SOL out, 121581 lamports fee, $83.67 as quoted, $82.47 matched).
#[test]
fn limit_match_price_puts_the_payout_fee_back() {
    use crate::limit_order::match_price;
    let buy = match_price(false, d("0.7"), "0.008365867", Some("121581"), 9).unwrap();
    assert!((buy - d("82.4747")).abs() < d("0.001"), "{buy}");
    // A sale: 0.012 SOL brings 0.98 USDC and the fee's 0.01 USDC goes back on top.
    assert_eq!(match_price(true, d("0.012"), "0.98", Some("10000"), 6).unwrap().round_dp(6), (d("0.99") / d("0.012")).round_dp(6));
    assert_eq!(match_price(false, Decimal::ONE, "?", None, 9), None);
}

// The buy that filled on the Mac's Dev wallet (2026-11-12), as 1Click's order read: 0.6979 USDC in, 0.008372599 SOL
// matched, 123398 lamports estimated for the payout. 0.008247143 SOL arrived on chain.
#[test]
fn limit_end_takes_the_payout_fee_off() {
    use crate::limit_order::limit_end;
    let o = one_click::parse_order(&json!({"id": "o1", "attributes": {"side": "BUY", "fillStatus": "FILLED", "estimatedWithdrawFee": "123398",
        "partialFills": [{"amountIn": "697900", "amountOut": "8372599"}]}})).unwrap();
    let end = limit_end(&o, false, 9, 6);
    assert_eq!((end.status.as_str(), end.spent, end.got), ("FILLED", d("0.6979"), d("0.008249201")));
    assert!((end.price.unwrap() - d("84.6023")).abs() < d("0.001"));
    let none = limit_end(&one_click::parse_order(&json!({"id": "o2", "attributes": {"side": "SELL", "fillStatus": "EXPIRED"}})).unwrap(), true, 9, 6);
    assert_eq!((none.got, none.price), (Decimal::ZERO, None));
}

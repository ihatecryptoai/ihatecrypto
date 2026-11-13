use std::collections::HashSet;

use rust_decimal::Decimal;

use super::{d, set};
use crate::intent::{IntentParser, Side, TradeIntent};
use crate::plan::*;
use crate::risk::RiskPolicy;

fn eth() -> Market { Market::new("ETH", d("2000"), 4, 25) }

fn intent(risk: &str, stop: &str, lev: Option<&str>) -> TradeIntent {
    TradeIntent { risk_usd: Some(d(risk)), stop_pct: Some(d(stop)), leverage: lev.map(d), ..TradeIntent::default() }
}

fn size(risk: &str, stop: &str, take: Option<&str>, lev: &str, side: Side, m: &Market) -> Sizing {
    RiskCalc::size(d(risk), d(stop), take.map(d), d(lev), side, m, None, None, Decimal::ZERO).unwrap()
}

#[test]
fn long_sizing_keeps_loss_at_stop_within_risk() {
    let s = size("10", "3", Some("6"), "2", Side::Long, &eth());
    // per coin: 2000·0.03 + (2000 + 1940)·0.00095 (Hyperliquid 0.045% + ihatecrypto 0.05%) = 63.743 → 10 / 63.743 = 0.15688… → 0.1568
    assert_eq!(s.size, d("0.1568"));
    assert_eq!(s.stop_price, d("1940"));
    assert_eq!(s.take_price, Some(d("2120")));
    assert_eq!(s.margin, d("156.8"));
    assert_eq!(s.builder_fees_usd, d("0.1568") * d("3940") * d("0.0005"));
    assert!(s.loss_at_stop_usd <= d("10") && s.loss_at_stop_usd > d("9.9"));
    assert!(s.liquidation_price < s.stop_price);
}

#[test]
fn short_sizing_mirrors_long() {
    let s = size("10", "3", None, "2", Side::Short, &eth());
    assert_eq!(s.stop_price, d("2060"));
    assert!(s.loss_at_stop_usd <= d("10"));
    assert!(s.liquidation_price > s.stop_price);
}

#[test]
fn missing_risk_and_stop_ask_for_input() {
    let e = Plan::evaluate(&TradeIntent::default(), Some(&eth()));
    assert_eq!(e.missing, ["risk in $", "stop in %"]);
    assert!(e.sizing.is_none());
}

#[test]
fn checks_that_stop_a_plan() {
    // ETH allows 25×: a 50× plan says the market's top.
    assert!(Plan::evaluate(&intent("10", "3", Some("50")), Some(&eth())).problem.unwrap().contains("25×"));
    // 20× on a 4% stop: liquidation (~3% away) comes first.
    assert!(Plan::evaluate(&intent("10", "4", Some("20")), Some(&eth())).problem.unwrap().contains("Liquidation"));
    assert!(Plan::evaluate(&intent("0.2", "3", None), Some(&eth())).problem.unwrap().contains("$10"));
    // A market past 50×, the signer refuses over 50× (HLGuard): a 60× plan says so before any money moves to Hyperliquid.
    let btc = Market::new("BTC", d("84000"), 5, 60);
    let mut i = TradeIntent { asset: "BTC".into(), ..intent("1", "0.5", Some("60")) };
    assert_eq!(Plan::evaluate(&i, Some(&btc)).problem.as_deref(), Some("Leverage for BTC: 1× to 50×."));
    i.leverage = Some(d("40"));
    assert!(!Plan::evaluate(&i, Some(&btc)).problem.is_some_and(|p| p.starts_with("Leverage")));
    i.leverage = Some(d("2.5"));
    assert_eq!(Plan::evaluate(&i, Some(&btc)).problem.as_deref(), Some("Leverage on Hyperliquid is a whole number: 2× or 3×."));
}

#[test]
fn bridge_need_covers_margin_and_fees() {
    let e = Plan::evaluate(&intent("10", "3", Some("2")), Some(&eth()));
    assert_eq!(Plan::bridge_need(&intent("10", "3", Some("2")), &e, Some(d("500")), None), None);
    assert_eq!(Plan::bridge_need(&intent("10", "3", Some("2")), &e, Some(d("100")), None), Some(d("57.39")));   // 156.8 margin + 0.58… fees − 100, up
    // Hyperliquid holds a buy limit's margin at the current price when that's higher: 5% above the entry, 5% more margin.
    let s = e.sizing.clone().unwrap();
    let at_entry = Plan::bridge_need(&intent("10", "3", Some("2")), &e, Some(Decimal::ZERO), None).unwrap();
    let at_mark = Plan::bridge_need(&intent("10", "3", Some("2")), &e, Some(Decimal::ZERO), Some(s.entry * d("1.05"))).unwrap();
    assert!(at_mark - at_entry >= s.margin * d("0.05") - d("0.01"));
    assert_eq!(Plan::bridge_need(&intent("10", "3", Some("2")), &e, Some(Decimal::ZERO), Some(s.entry * d("0.95"))), Some(at_entry));
    // A blocked position quotes no bridge.
    let blocked = Plan::evaluate(&intent("10", "3", Some("25")), Some(&eth()));
    assert_eq!(Plan::bridge_need(&intent("10", "3", Some("25")), &blocked, None, None), None);
    assert_eq!(Plan::blocks(&intent("10", "3", Some("25")), &blocked, &Live::default())[1].status, BlockStatus::NeedsInput);
}

// Solana short, Base holds it: the plan's money comes from Base, not a «Short by» on Solana (the 50 USDC long of 2026-09-30).
#[test]
fn source_is_the_funding_network() {
    let mut live = Live {
        solana_address_set: true, solana_usdc: Some(d("25")), from: Some(("Base".into(), d("3000"))),
        quote: Some(BridgeQuote::new("Relay", d("50"), Decimal::ZERO, d("49.97"), "", None)), ..Live::default()
    };
    let i = TradeIntent { amount_usdc: Some(d("50")), ..intent("10", "3", None) };
    let source = &Plan::blocks(&i, &Plan::evaluate(&i, Some(&eth())), &live)[0];
    assert!(source.title == "USDC · Base" && source.status == BlockStatus::Ready);
    live.from = None;
    let source = &Plan::blocks(&i, &Plan::evaluate(&i, Some(&eth())), &live)[0];
    assert_eq!(source.status, BlockStatus::Blocked);
    assert_eq!(source.lines, ["Wallet: 25 USDC.", "Send 50 USDC.", "Short by 25 USDC."]);
    // An unknown balance is no data, not zero.
    let live = Live { solana_address_set: true, ..Live::default() };
    assert_eq!(Plan::blocks(&intent("10", "3", None), &Plan::evaluate(&intent("10", "3", None), Some(&eth())), &live)[0].status, BlockStatus::NoData);
}

#[test]
fn hyperliquid_markets_fall_into_groups() {
    let m = |name: &str, dex: Option<&str>| Market { dex: dex.map(String::from), ..Market::new(name, Decimal::ONE, 2, 10) };
    assert_eq!(m("ETH", None).group(), MarketGroup::Crypto);
    assert_eq!(m("xyz:CL", Some("xyz")).group(), MarketGroup::Commodities);
    assert_eq!(m("xyz:GOLD", Some("xyz")).group(), MarketGroup::Commodities);
    assert_eq!(m("xyz:SP500", Some("xyz")).group(), MarketGroup::Indices);
    assert_eq!(m("xyz:TSLA", Some("xyz")).group(), MarketGroup::Stocks);
    assert!(IntentParser::pretty("xyz:CL") == "WTI oil (xyz:CL)" && IntentParser::pretty("ETH") == "ETH");
    let pumped = Market { prev_day_px: Some(d("2000")), ..Market::new("ETH", d("2140"), 4, 25) };
    assert_eq!(pumped.change24h(), Some(d("7")));
}

fn markets() -> HashSet<String> { set(&["ETH", "BTC", "SOL"]) }

fn parse(text: &str, to: &TradeIntent) -> Option<TradeIntent> { IntentParser::apply(text, to, &markets()) }

/// TwapTests: the same rules as one order, plus Hyperliquid's own ($100 at least, 5 minutes at least) and ours: 4 hours
/// at most, nothing but the entry, and the risk counted from parts filling 3% worse (each part's slippage cap).
#[test]
fn a_twap_keeps_the_rules_of_one_order() {
    let eth = Market { index: 1, ..Market::new("ETH", d("3000"), 4, 25) };
    let mut i = parse("long ETH 300 USDC over 2 h stop 2% risk $6 leverage 1x", &TradeIntent::default()).unwrap();
    let e = Plan::evaluate(&i, Some(&eth));
    assert!(e.problem.is_none() && e.sizing.as_ref().is_some_and(|s| s.notional >= d("100")), "{e:?}");
    // $6 at a 2% stop is 0.0914 ETH as one order, 0.0385 as a TWAP: each coin may lose 2% + 3% of 3000 and the fees.
    let one = TradeIntent { twap_minutes: None, ..i.clone() };
    let s = e.sizing.clone().unwrap();
    let single = Plan::evaluate(&one, Some(&eth)).sizing.unwrap();
    assert!(s.size == d("0.0385") && single.size == d("0.0914") && s.stop_price == single.stop_price);
    assert!(s.loss_at_stop_usd <= d("6") && s.loss_at_stop_usd > d("5.9"), "{}", s.loss_at_stop_usd);
    // ihatecrypto's 0.05% only on the exit: a TWAP's parts pay Hyperliquid alone.
    assert_eq!(s.builder_fees_usd, s.size * s.stop_price * RiskCalc::builder_fee());
    assert_eq!(single.builder_fees_usd, single.size * (single.entry + single.stop_price) * RiskCalc::builder_fee());
    // «N USDC» buys what it says: the money's risk counts the 3% too, so the size comes back to it.
    let money = parse("long ETH 300 USDC over 2 h stop 2% leverage 1x", &TradeIntent::default()).unwrap();
    let money = Plan::resolve(&money, Some(&eth), Some(d("100000")), &RiskPolicy::default(), None).0;
    assert!(Plan::evaluate(&money, Some(&eth)).sizing.is_some_and(|s| s.notional > d("295") && s.notional <= d("300")));
    // Started from the chat with no risk named, the rest goes by the rules, and they take no half at 1R off a TWAP.
    let started = parse("long ETH 300 USDC over 2 h stop 2% leverage 1x", &TradeIntent::default()).unwrap().started();
    let ruled = Plan::resolve(&started, Some(&eth), Some(d("100000")), &RiskPolicy::default(), None).0;
    assert!(ruled.auto_rules == Some(true) && ruled.partial_take.is_none() && ruled.take_pct == Some(d("4")));
    assert_eq!(Plan::evaluate(&ruled, Some(&eth)).problem, None);
    let live = Live { market: Some(eth.clone()), ..Live::default() };
    let lines = Plan::position(&i, &e, &live).lines;
    assert!(lines[0].starts_with("TWAP: 0.") && lines[0].contains(" ETH over 2 h, parts at the market every 30 s or more") && lines[0].ends_with("equal sizes."),
            "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("ihatecrypto's 0.05% isn't on a TWAP")));

    i.risk_usd = Some(d("2"));   // $38 of ETH: under the TWAP's $100
    assert!(Plan::evaluate(&i, Some(&eth)).problem.unwrap().starts_with("A TWAP on Hyperliquid is from $100"));
    i.risk_usd = Some(d("6"));
    i.twap_minutes = Some(3);
    assert_eq!(Plan::evaluate(&i, Some(&eth)).problem.as_deref(), Some("A TWAP on Hyperliquid runs 5 minutes at least."));
    // 4 hours at most: a longer one could buy on unwatched after its stop fires. Days are recurring buys.
    i.twap_minutes = Some(240);
    assert_eq!(Plan::evaluate(&i, Some(&eth)).problem, None);
    for minutes in [241, 1440] {
        i.twap_minutes = Some(minutes);
        assert!(Plan::evaluate(&i, Some(&eth)).problem.is_some_and(|p| p.starts_with("A TWAP here runs 4 hours at most") && p.contains("recurring buys")));
    }
    i.twap_minutes = Some(120);
    i.trailing_pct = Some(Decimal::ONE);
    assert!(Plan::evaluate(&i, Some(&eth)).problem.unwrap().starts_with("A TWAP is the whole entry"));
}

/// AgentTests: code's check of the plan's money: a plan built by the rules never trips it, a number in the wrong role does.
#[test]
fn a_plan_never_holds_more_than_its_command_named() {
    let markets = [Market::new("BTC", d("84000"), 5, 40), Market::new("ETH", d("2680"), 4, 25), Market::new("SOL", d("152.4"), 2, 20),
                   Market::new("kPEPE", d("0.0113"), 0, 10), Market::new("xyz:CL", d("71.35"), 3, 20)];
    let mut plans = 0;
    for m in &markets {
        let m = Market { atr_pct: Some(d("2")), ..m.clone() };
        for money in ["3", "5", "8", "10", "20", "50", "100", "1000"] {
            for lev in [None, Some("1"), Some("2"), Some("3"), Some("5"), Some("10")] {
                for stop in [None, Some("0.5"), Some("1"), Some("3"), Some("10")] {
                    for equity in [None, Some("30"), Some("500"), Some("20000")] {
                        for by_risk in [false, true] {
                            let mut i = TradeIntent { asset: m.name.clone(), leverage: lev.map(d), stop_pct: stop.map(d), auto_rules: Some(true), ..TradeIntent::default() };
                            if by_risk { i.risk_usd = Some(d(money) / d("10")) } else { i.amount_usdc = Some(d(money)) }
                            let r = Plan::resolve(&i, Some(&m), equity.map(d), &RiskPolicy::default(), None).0;
                            let Some(s) = Plan::evaluate(&r, Some(&m)).sizing else { continue };
                            plans += 1;
                            assert_eq!(Plan::money_problem(&i, &s, &m), None, "{} {money} lev {lev:?} stop {stop:?} equity {equity:?} risk {by_risk}: margin {} loss {}",
                                       m.name, s.margin, s.loss_at_stop_usd);
                        }
                    }
                }
            }
        }
    }
    assert!(plans > 1_000);
    // «5 USDC» taken as a $5 risk: at a 2% stop that is $250 of position and $50 of margin at 5×.
    let sol = &markets[2];
    let named = TradeIntent { asset: "SOL".into(), amount_usdc: Some(d("5")), leverage: Some(d("5")), stop_pct: Some(d("2")), ..TradeIntent::default() };
    let wrong = TradeIntent { amount_usdc: None, risk_usd: Some(d("5")), ..named.clone() };
    let as_risk = Plan::evaluate(&wrong, Some(sol)).sizing.unwrap();
    assert!(Plan::money_problem(&named, &as_risk, sol).unwrap().contains("more than the 5 USDC"));
    // Ten times the risk named.
    let risk = TradeIntent { asset: "SOL".into(), risk_usd: Some(d("2")), leverage: Some(d("3")), stop_pct: Some(d("2")), ..TradeIntent::default() };
    let tenfold = TradeIntent { risk_usd: Some(d("20")), ..risk.clone() };
    let big = Plan::evaluate(&tenfold, Some(sol)).sizing.unwrap();
    assert!(Plan::money_problem(&risk, &big, sol).unwrap().contains("more than the $2 risk"));
}

/// Numbers no arithmetic holds (a node's garbage, a typo of 30 digits) refuse the plan instead of stopping the engine:
/// rust_decimal panics where Swift's Decimal goes on.
#[test]
fn numbers_past_range_refuse_the_plan() {
    let huge = Market::new("ETH", Decimal::MAX, 4, 25);
    let i = intent("10", "3", Some("2"));
    assert_eq!(Plan::evaluate(&i, Some(&huge)).problem.as_deref(), Some(OUT_OF_RANGE));
    let none = Market::new("ETH", d("2000"), 4, 0);
    assert!(RiskCalc::size(d("10"), d("3"), None, d("1"), Side::Long, &none, None, None, Decimal::ZERO).is_none());
    let big = TradeIntent { risk_usd: Some(Decimal::MAX), ..i.clone() };
    let e = Plan::evaluate(&big, Some(&eth()));
    let _ = Plan::blocks(&big, &e, &Live { market: Some(eth()), equity: Some(Decimal::MAX), ..Live::default() });
    let (r, _) = Plan::resolve(&TradeIntent { amount_usdc: Some(Decimal::MAX), risk_usd: None, ..i.clone() }, Some(&eth()), Some(Decimal::MAX),
                               &RiskPolicy::default(), None);
    let _ = Plan::evaluate(&r, Some(&eth()));
}

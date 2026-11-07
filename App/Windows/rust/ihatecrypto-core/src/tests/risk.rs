use std::collections::HashSet;

use rust_decimal::Decimal;

use super::{d, set};
use crate::intent::{IntentParser, Side, TradeIntent};
use crate::plan::{Market, Plan};
use crate::risk::*;

fn eth() -> Market { Market::new("ETH", d("2000"), 4, 25) }

fn apply(text: &str, to: &TradeIntent, markets: &HashSet<String>) -> TradeIntent { IntentParser::apply(text, to, markets).unwrap_or_else(|| panic!("«{text}»")) }

fn policy() -> RiskPolicy { RiskPolicy::default() }

fn blocks(advice: &[Advice]) -> bool { advice.iter().any(|a| a.level == Level::Block) }

#[test]
fn rules_fill_stop_take_and_risk_when_asked() {
    let none = HashSet::new();
    let mut i = apply("шорт ETH, тейк и стоп как учат", &TradeIntent::default(), &none);
    assert!(i.side == Side::Short && i.auto_rules == Some(true) && i.stop_pct.is_none());
    let m = Market { atr_pct: Some(d("2")), ..eth() };
    let (r, notes) = Plan::resolve(&i, Some(&m), Some(d("1000")), &policy(), None);
    assert_eq!(r.stop_pct, Some(d("3")));    // 1.5 × ATR 2%
    assert_eq!(r.take_pct, Some(d("6")));    // 2R
    assert_eq!(r.risk_usd, Some(d("10")));   // 1% of $1000
    assert!(notes.len() == 5 && r.partial_take == Some(true) && r.leverage == Some(d("5")), "{notes:?}");   // stop, take, leverage, half at 1R, risk
    i = apply("стоп 2% тейк 3R", &i, &none);
    assert!(i.take_r == Some(d("3")) && i.take_pct.is_none());
    assert_eq!(Plan::resolve(&i, Some(&m), Some(d("1000")), &policy(), None).0.take_pct, Some(d("6")));
}

#[test]
fn risk_rules_block_too_much_capital_and_warn_about_the_entry() {
    let i = TradeIntent { risk_usd: Some(d("10")), stop_pct: Some(d("3")), take_pct: Some(d("3")), leverage: Some(d("2")), ..TradeIntent::default() };
    let pumped = Market { prev_day_px: Some(d("2000")), funding: Some(d("0.0001")), ..Market::new("ETH", d("2140"), 4, 25) };
    let s = Plan::evaluate(&i, Some(&pumped)).sizing.unwrap();
    assert!(blocks(&RiskRules::check(&i, &s, &pumped, Some(d("200")), &policy(), Some(Decimal::ZERO))));   // $10 of $200 is 5% > 2%
    let fine = RiskRules::check(&i, &s, &pumped, Some(d("1000")), &policy(), Some(Decimal::ZERO));
    assert!(!blocks(&fine));
    assert!(fine.iter().any(|a| a.level == Level::Warn && a.text.contains("+7%")), "{fine:?}");        // long after +7% in a day
    assert!(fine.iter().any(|a| a.level == Level::Warn && a.text.starts_with("Reward/risk")));        // 1:1 below 1:1.5
    assert!(fine.iter().any(|a| a.level == Level::Warn && a.text.starts_with("Funding")));            // 0.01%/h ≈ 88% a year, longs pay
}

#[test]
fn limit_entry_waits_for_a_pullback() {
    let none = HashSet::new();
    let i = apply("лонг ETH риск $10 стоп 3% тейк 6% на откате 2%", &TradeIntent::default(), &none);
    assert!(i.pullback_pct == Some(d("2")) && i.entry_px.is_none());
    let (r, notes) = Plan::resolve(&i, Some(&eth()), Some(d("1000")), &policy(), None);
    assert!(r.entry_px == Some(d("1960")) && notes[0].contains("pullback"));
    let s = Plan::evaluate(&r, Some(&eth())).sizing.unwrap();
    assert!(s.entry == d("1960") && s.stop_price == d("1901.2"));
    let i = apply("лимит 2050", &i, &none);
    assert!(i.entry_px == Some(d("2050")) && i.pullback_pct.is_none());
    assert!(Plan::evaluate(&i, Some(&eth())).problem.unwrap().contains("above the market"));   // a long limit above the mark fills at once
}

#[test]
fn open_positions_count_towards_the_total_risk() {
    let i = TradeIntent { risk_usd: Some(d("20")), stop_pct: Some(d("3")), ..TradeIntent::default() };
    let s = Plan::evaluate(&i, Some(&eth())).sizing.unwrap();
    assert!(blocks(&RiskRules::check(&i, &s, &eth(), Some(d("1000")), &policy(), Some(d("45")))));   // 4.5% + 2% > 6%
    assert!(!blocks(&RiskRules::check(&i, &s, &eth(), Some(d("1000")), &policy(), Some(d("20")))));
    assert!(RiskRules::check(&i, &s, &eth(), Some(d("1000")), &policy(), None).iter().any(|a| a.text.contains("no stop")));
}

#[test]
fn blocked_plan_suggests_a_risk_that_fits() {
    let mut i = TradeIntent { asset: "ETH".into(), side: Side::Long, risk_usd: Some(Decimal::ONE), stop_pct: Some(d("1.5")), take_pct: Some(d("3")),
                              leverage: Some(d("5")), ..TradeIntent::default() };
    // $13.56 capital, 2% rule → at most $0.27; the suggestion must pass the rules itself.
    let r = Plan::fitting_risk(&i, &eth(), Some(d("13.56")), &policy(), Some(Decimal::ZERO), Some(d("10"))).unwrap();
    assert!(r <= d("0.27") && r > d("0.15"), "{r}");
    // Already within the rules: nothing to suggest.
    i.risk_usd = Some(d("0.2"));
    assert_eq!(Plan::fitting_risk(&i, &eth(), Some(d("13.56")), &policy(), Some(Decimal::ZERO), Some(d("10"))), None);
    // So small that the $10 minimum can't be met: no suggestion.
    i.risk_usd = Some(Decimal::ONE);
    assert_eq!(Plan::fitting_risk(&i, &eth(), Some(d("2")), &policy(), Some(Decimal::ZERO), Some(d("10"))), None);
}

#[test]
fn money_in_the_trade_sizes_it() {
    // «увеличь до 5 USDC» on a 3× plan by the book: a $5 margin, not a $5 risk.
    let sol = set(&["SOL"]);
    let m = Market { atr_pct: Some(d("2")), ..Market::new("SOL", d("120"), 2, 20) };
    let i = apply("5 USDC", &apply("long SOL leverage 3x by the book", &TradeIntent::default(), &sol), &sol);
    assert!(i.amount_usdc == Some(d("5")) && i.risk_usd.is_none());
    let r = Plan::resolve(&i, Some(&m), Some(d("30")), &policy(), None).0;
    let s = Plan::evaluate(&r, Some(&m)).sizing.unwrap();
    assert!(r.amount_usdc.is_none());
    assert!(s.margin <= d("5") && s.margin >= d("4.5"));   // SOL trades in 0.01 steps
    // A risk named with it keeps «N USDC» as the amount to send.
    let both = apply("risk $1", &i, &sol);
    assert_eq!(Plan::resolve(&both, Some(&m), Some(d("30")), &policy(), None).0.amount_usdc, Some(d("5")));
}

#[test]
fn a_small_account_reaches_the_minimum_order() {
    // $29 at 1% sizes HYPE under $10: the rules raise the risk to the least that reaches it, within the limit.
    let hype = set(&["HYPE"]);
    let m = Market { atr_pct: Some(d("2.2")), ..Market::new("HYPE", d("88"), 2, 10) };
    let i = apply("long HYPE leverage 3x by the book", &TradeIntent::default(), &hype);
    let r = Plan::resolve(&i, Some(&m), Some(d("29")), &policy(), None).0;
    let e = Plan::evaluate(&r, Some(&m));
    let s = e.sizing.unwrap();
    assert!(s.notional >= d("10") && s.notional < d("11"), "{}", s.notional);
    assert_eq!(e.problem, None);
    // Past the limit it stays as the rules said, and the plan says why it can't be placed.
    let tiny = Plan::resolve(&i, Some(&m), Some(d("5")), &policy(), None).0;
    assert!(Plan::evaluate(&tiny, Some(&m)).problem.unwrap().contains("$10 minimum"));
}

#[test]
fn money_in_the_trade_lowers_the_rules_leverage_to_fit_the_limit() {
    // «make it 8 bucks» on a short by the book: 8 × 5× × 2.9% = $1.16 breaks a 3.5% limit of $29; 4× fits.
    let sol = set(&["SOL"]);
    let m = Market { atr_pct: Some(d("1.9")), ..Market::new("SOL", d("120"), 2, 20) };
    let policy = RiskPolicy { max_risk_pct: d("3.5"), max_leverage: d("5"), ..RiskPolicy::default() };
    let i = apply("short SOL by the book 8 USDC", &TradeIntent::default(), &sol);
    let r = Plan::resolve(&i, Some(&m), Some(d("29")), &policy, None).0;
    assert_eq!(r.leverage, Some(d("4")));
    assert!(r.risk_usd.unwrap() <= d("1.01"));
    // Leverage the user named stays.
    let named = apply("leverage 5x", &i, &sol);
    assert_eq!(Plan::resolve(&named, Some(&m), Some(d("29")), &policy, None).0.leverage, Some(d("5")));
}

#[test]
fn what_the_user_left_out_the_rules_fill() {
    // «open a short on btc, risk half a buck»: the stop by the book, the rules' leverage, the $0.50 risk kept.
    let btc = set(&["BTC"]);
    let m = Market { atr_pct: Some(Decimal::ONE), ..Market::new("BTC", d("84000"), 5, 40) };
    let i = apply("short BTC risk $0.50", &TradeIntent::default(), &btc).started();
    assert_eq!(i.auto_rules, Some(true));
    let r = Plan::resolve(&i, Some(&m), Some(d("29")), &policy(), None).0;
    assert!(r.risk_usd == Some(d("0.5")) && r.stop_pct.is_some() && r.leverage == Some(policy().max_leverage));
    // Both named: nothing else is filled but the leverage.
    let both = apply("short BTC risk $0.50 stop 1.2%", &TradeIntent::default(), &btc).started();
    assert!(both.auto_rules.is_none() && Plan::resolve(&both, Some(&m), Some(d("29")), &policy(), None).0.leverage == Some(policy().max_leverage));
}

#[test]
fn the_minimum_order_never_spends_more_than_named() {
    // «нефть лонг на 4 доллара x2» is an $8 position: under the $10 minimum, said so, not quietly $5 of margin.
    let cl = set(&["xyz:CL"]);
    let m = Market { atr_pct: Some(d("1.8")), ..Market::new("xyz:CL", d("91"), 3, 20) };
    let i = apply("long oil 4 USDC leverage 2x", &TradeIntent::default(), &cl).started();
    let r = Plan::resolve(&i, Some(&m), Some(d("29")), &policy(), None).0;
    assert!(Plan::evaluate(&r, Some(&m)).problem.unwrap().contains("$10 minimum"));
    // At 3× the same 4 USDC reaches it.
    let three = apply("leverage 3x", &i, &cl);
    let r3 = Plan::resolve(&three, Some(&m), Some(d("29")), &policy(), None).0;
    assert_eq!(Plan::evaluate(&r3, Some(&m)).problem, None);
}

#[test]
fn a_stopped_plan_gets_the_one_change_that_passes() {
    let sol = set(&["SOL"]);
    let m = Market { atr_pct: Some(d("2")), ..Market::new("SOL", d("120"), 2, 20) };
    let policy = RiskPolicy { max_risk_pct: d("3.5"), ..RiskPolicy::default() };
    // An $9.5 order: the least risk that reaches the $10 minimum.
    let small = apply("long SOL risk $0.3 stop 3% leverage 2x", &TradeIntent::default(), &sol);
    let (command, s) = Plan::fix(&small, &m, Some(d("29")), &policy, None, Some(d("26"))).unwrap();
    assert!(command.starts_with("risk $") && s.notional >= d("10") && s.notional < d("11"), "{command}");
    // Money named, «3 USDC at 2×» is $6: the least money that reaches it.
    let money = apply("long SOL 3 USDC leverage 2x", &TradeIntent::default(), &sol).started();
    let (command, s) = Plan::fix(&money, &m, Some(d("29")), &policy, None, Some(d("26"))).unwrap();
    assert!(command.ends_with("USDC") && s.notional >= d("10"), "{command}");
    // Risk $5 of $29 breaks 3.5%: the largest risk that fits.
    let big = apply("long SOL risk $5 stop 3%", &TradeIntent::default(), &sol);
    let (command, s) = Plan::fix(&big, &m, Some(d("29")), &policy, None, Some(d("26"))).unwrap();
    assert!(command.starts_with("risk $") && s.loss_at_stop_usd <= d("1.02"), "{command}");
    // Nothing fits $3 of money: no fix.
    assert_eq!(Plan::fix(&big, &m, Some(d("3")), &policy, None, Some(Decimal::ONE)), None);
}

/// HedgeTests: a hedge is sized by the coins held, at 2× with a stop that only guards the margin.
#[test]
fn a_hedge_is_sized_by_the_coins_held() {
    let markets = set(&["SOL", "ETH", "BTC", "HYPE", "xyz:TSLA", "xyz:NVDA", "xyz:CL"]);
    let sol = Market { prev_day_px: Some(d("110")), funding: Some(d("0.00001")), atr_pct: Some(d("2")), ..Market::new("SOL", d("100"), 2, 20) };
    let hedge = |text: &str| apply(text, &TradeIntent::default(), &markets);
    let (r, notes) = Plan::resolve(&hedge("hedge SOL"), Some(&sol), Some(d("58")), &policy(), Some(d("0.2032")));
    assert!(r.hedge == Some(d("0.2032")) && r.leverage == Some(d("2")) && r.take_pct.is_none() && r.partial_take.is_none());
    // Four fifths of the way to liquidation at 2× on a 20× market: (0.5 − 0.025) / 1.025 × 80 = 37.07.
    assert!(r.stop_pct == Some(d("37")) && notes.len() == 2, "{notes:?}");
    let e = Plan::evaluate(&r, Some(&sol));
    let s = e.sizing.clone().unwrap();
    assert!(e.problem.is_none() && s.size == d("0.2") && s.notional == d("20") && s.margin == d("10") && s.take_price.is_none());
    // The stop is above the entry and below liquidation; what it loses the SOL gains, so only the fees are at risk.
    assert!(s.stop_price == d("137") && s.liquidation_price > d("137") && s.loss_at_stop_usd == s.fees_usd);
    let advice = RiskRules::check(&r, &s, &sol, Some(d("58")), &policy(), Some(Decimal::ZERO));
    assert!(!blocks(&advice));
    assert!(!advice.iter().any(|a| a.text.contains("take-profit is close") || a.text.contains("Liquidation") || a.text.contains("24h")));

    // Half; more than is held is cut to what is held; nothing held, or not read yet.
    assert_eq!(Plan::resolve(&hedge("hedge SOL half"), Some(&sol), Some(d("58")), &policy(), Some(d("0.3"))).0.hedge, Some(d("0.15")));
    let (cut, notes) = Plan::resolve(&hedge("hedge 5 SOL"), Some(&sol), Some(d("58")), &policy(), Some(d("0.2")));
    assert!(cut.hedge == Some(d("0.2")) && notes[0].contains("covers that"));
    let none = Plan::resolve(&hedge("hedge SOL"), Some(&sol), Some(d("58")), &policy(), Some(Decimal::ZERO)).0;
    assert!(Plan::evaluate(&none, Some(&sol)).problem.unwrap().contains("holds no"));
    let unread = Plan::resolve(&hedge("hedge SOL"), Some(&sol), Some(d("58")), &policy(), None).0;
    assert_eq!(Plan::evaluate(&unread, Some(&sol)).missing, ["what the wallet holds of SOL"]);
    // Under Hyperliquid's $10 minimum.
    let small = Plan::resolve(&hedge("hedge SOL"), Some(&sol), Some(d("58")), &policy(), Some(d("0.05"))).0;
    assert!(Plan::evaluate(&small, Some(&sol)).problem.unwrap().contains("$10 minimum"));
    // A market with no leverage (NaN in Swift): no stop and no note for it, rather than a panic.
    let broken = Market { max_leverage: 0, ..sol.clone() };
    let (r, notes) = Plan::resolve(&hedge("hedge SOL leverage 2x"), Some(&broken), Some(d("58")), &policy(), Some(d("0.2")));
    assert!(r.stop_pct.is_none() && notes.is_empty(), "{notes:?}");
}

/// The policy as Swift's synthesized Codable writes it, every key required.
#[test]
fn the_policy_keeps_swifts_shape() {
    let json = serde_json::to_string(&policy()).unwrap();
    assert_eq!(json, r#"{"maxRiskPct":2,"defaultRiskPct":1,"minRewardRisk":1.5,"defaultRewardRisk":2,"maxLeverage":5,"atrStopMultiple":1.5,"maxOpenRiskPct":6}"#);
    assert_eq!(serde_json::from_str::<RiskPolicy>(&json).unwrap(), policy());
    assert!(serde_json::from_str::<RiskPolicy>(r#"{"maxRiskPct":2}"#).is_err());
}

/// Every rule's line has its Russian, with the same numbers and no «;» (the Windows engine's checklist in a Russian chat).
#[test]
fn every_rules_line_has_its_russian() {
    let numbers = |t: &str| {
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        for c in t.chars().chain([' ']) {
            if c.is_ascii_digit() || (!cur.is_empty() && ".,:%$×−+".contains(c)) { cur.push(c) } else if !cur.is_empty() {
                out.push(cur.trim_end_matches(['.', ',', ':']).to_string());
                cur.clear();
            }
        }
        out.sort();
        out
    };
    let i = TradeIntent { risk_usd: Some(d("10")), stop_pct: Some(d("0.5")), take_pct: Some(d("0.5")), leverage: Some(d("10")), ..TradeIntent::default() };
    let pumped = Market { prev_day_px: Some(d("2000")), funding: Some(d("0.0001")), atr_pct: Some(d("1.2")), ..Market::new("ETH", d("2140"), 4, 25) };
    let s = Plan::evaluate(&i, Some(&pumped)).sizing.unwrap();
    let mut all = RiskRules::check(&i, &s, &pumped, Some(d("200")), &policy(), Some(d("5")));
    all.extend(RiskRules::check(&i, &s, &pumped, None, &policy(), None));
    all.extend(RiskRules::check(&i, &s, &pumped, Some(d("100000")), &policy(), Some(Decimal::ZERO)));
    assert!(all.len() >= 10, "{all:?}");
    for a in &all {
        assert!(!a.ru.is_empty() && !a.ru.contains(';') && a.ru.chars().any(|c| ('а'..='я').contains(&c)), "{a:?}");
        assert_eq!(numbers(&a.ru), numbers(&a.text), "{a:?}");
    }
    let fine = RiskRules::check(&TradeIntent { stop_pct: Some(d("3")), take_pct: Some(d("3")), leverage: Some(d("2")), ..i.clone() }, &s, &pumped,
                                Some(d("1000")), &policy(), Some(Decimal::ZERO));
    assert!(fine.iter().any(|a| a.ru.starts_with("Прибыль к риску 1:") && a.ru.contains("тейк слишком близко. Обычный минимум — 1:")), "{fine:?}");
}

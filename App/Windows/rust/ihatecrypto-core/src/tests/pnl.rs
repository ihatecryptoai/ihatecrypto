use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use super::d;
use crate::foundation::{Date, adding, unix};
use crate::pnl::*;
use crate::trade_history::{Fill, parse_funding};

fn now() -> Date { unix(1_800_000_000.0) }
fn ago(days: f64) -> Date { adding(now(), -days * 86_400.0) }
fn markets(m: &[&str]) -> HashSet<String> { super::set(m) }

fn fill(coin: &str, side: &str, sz: &str, start: &str, pnl: &str, fee: &str, days: f64) -> Fill {
    Fill {
        coin: coin.into(), px: d("100"), sz: d(sz), side: side.into(), time: ago(days), start_position: d(start), dir: String::new(),
        closed_pnl: d(pnl), fee: d(fee), hash: None,
    }
}

/// An oil short 10 days ago (+2 − fees), an ETH long 3 days ago (−1 − fees), an ETH long open now.
fn fills() -> Vec<Fill> {
    vec![
        fill("xyz:CL", "A", "1", "0", "0", "0.1", 10.5),
        fill("xyz:CL", "B", "1", "-1", "2", "0.1", 10.0),
        fill("ETH", "B", "1", "0", "0", "0.05", 3.5),
        fill("ETH", "A", "1", "1", "-1", "0.05", 3.0),
        fill("ETH", "B", "2", "0", "0", "0.02", 0.5),
    ]
}

fn funding() -> Vec<Funding> {
    vec![Funding { time: ago(10.2), coin: "xyz:CL".into(), usdc: d("-0.01") }, Funding { time: ago(0.2), coin: "ETH".into(), usdc: d("0.03") }]
}

#[test]
fn pnl_splits_periods_assets_and_fees() {
    let week = report(Period::Week, None, now(), &fills(), &funding(), &HashMap::from([("ETH".into(), d("0.4"))]), &[], &[]);
    assert_eq!(week.realized, d("-1.12")); // −1 − 0.05 − 0.05 − 0.02: the open's fee counts too
    assert_eq!(week.funding, d("0.03"));
    assert_eq!(week.trading(), d("-1.09"));
    assert_eq!(week.unrealized, d("0.4"));
    assert!(week.closed_trades == 1 && week.wins == 0 && week.worst.as_ref().unwrap().coin == "ETH");
    assert_eq!(week.change, None); // no equity history

    let all = report(Period::All, None, now(), &fills(), &funding(), &HashMap::new(), &[], &[]);
    assert_eq!(all.realized, d("0.68")); // 2 − 0.2 − 1.12
    assert!(all.closed_trades == 2 && all.wins == 1);
    assert!(all.best.as_ref().is_some_and(|b| b.coin == "xyz:CL" && b.realized == d("1.8")));
    assert_eq!(all.by_asset[0].coin, "xyz:CL"); // biggest move first

    let oil = report(Period::All, Some("xyz:CL"), now(), &fills(), &funding(), &HashMap::from([("ETH".into(), d("5"))]), &[], &[]);
    assert!(oil.realized == d("1.8") && oil.funding == d("-0.01") && oil.unrealized.is_zero());
    assert_eq!(oil.change, None); // the whole wallet's change isn't one asset's
}

#[test]
fn pnl_portfolio_change_leaves_out_money_moved_in_and_out() {
    let equity = [(ago(40.0), d("5")), (ago(6.0), d("10")), (ago(2.0), d("45")), (now(), d("44"))];
    let flow = |days: f64, usd: &str, note: &str| Flow { at: ago(days), usd: d(usd), note: note.into(), wallet: "w".into() };
    let flows = [flow(4.0, "31", "received"), flow(1.0, "-2", "sent"), flow(20.0, "100", "before the week")];
    let week = report(Period::Week, None, now(), &[], &[], &HashMap::new(), &equity, &flows);
    assert!(week.change == Some(d("5")) && week.flows == d("29") && week.change_since == Some(ago(6.0))); // 44 − 10 − (31 − 2)

    // History shorter than the period: measured from the oldest point kept, and it says so.
    let month = report(Period::Month, None, now(), &[], &[], &HashMap::new(), &equity[1..], &flows);
    assert!(month.change_since == Some(ago(6.0)) && month.change == Some(d("5")));
    assert!(facts(&month, &|s| s.into()).iter().any(|f| f.starts_with("Portfolio 30 days: +$5")));
    assert!(facts(&month, &|s| s.into())[0].starts_with("P/L 30 days: trading +$0 (realized after fees +$0, funding +$0), 0 closed trades"));
}

#[test]
fn pnl_phrases_parse() {
    let m = markets(&["ETH", "xyz:CL"]);
    let none = HashSet::new();
    let cmd = |period, coin: Option<&str>| Some(PnLCommand { period, coin: coin.map(String::from) });
    assert_eq!(PnLCommand::parse("какой у меня P/L", &none), cmd(Period::All, None));
    assert_eq!(PnLCommand::parse("pnl за неделю", &none), cmd(Period::Week, None));
    assert_eq!(PnLCommand::parse("сколько я заработал на нефти", &m), cmd(Period::All, Some("xyz:CL")));
    assert_eq!(PnLCommand::parse("profit and loss month for ETH", &m), cmd(Period::Month, Some("ETH")));
    assert_eq!(PnLCommand::parse("profit and loss today", &none), cmd(Period::Day, None));
    // A ticker nothing knows is still the one asked about; a period word isn't one.
    assert_eq!(PnLCommand::parse("pnl по aero", &none), cmd(Period::All, Some("AERO")));
    assert_eq!(PnLCommand::parse("profit and loss for the week", &none), cmd(Period::Week, None));
    assert_eq!(PnLCommand::parse("long ETH risk $5", &none), None);
    assert_eq!(PnLCommand::parse("plan the trade", &none), None);
    // EnglishCommandTests.
    let english = markets(&["BTC", "ETH", "xyz:CL"]);
    assert_eq!(PnLCommand::parse("profit and loss week for oil", &english), cmd(Period::Week, Some("xyz:CL")));
    assert_eq!(PnLCommand::parse("pnl on base", &none), cmd(Period::All, None));
}

/// FormsMatrixTests' P/L rows: every period word with every kind of asset named.
#[test]
fn pnl_forms_matrix() {
    let m = markets(&["BTC", "ETH", "SOL", "AERO", "MOODENG", "xyz:CL", "xyz:GOLD", "xyz:SPCX", "xyz:TSLA"]);
    for (p, period) in [("", Period::All), (" today", Period::Day), (" week", Period::Week), (" month", Period::Month), (" all", Period::All)] {
        assert_eq!(PnLCommand::parse(&format!("profit and loss{p}"), &m), Some(PnLCommand { period, coin: None }), "{p}");
        for (word, coin) in [("ETH", "ETH"), ("oil", "xyz:CL"), ("SOL", "SOL"), ("MOO", "MOO"), ("gold", "xyz:GOLD"), ("AERO", "AERO"), ("SPCX", "xyz:SPCX")] {
            let c = format!("profit and loss{p} for {word}");
            assert_eq!(PnLCommand::parse(&c, &m), Some(PnLCommand { period, coin: Some(coin.into()) }), "{c}");
        }
    }
}

/// PnLTests: userFunding's rows.
#[test]
fn funding_rows_parse() {
    let json = br#"[{"time":1790323200066,"delta":{"type":"funding","coin":"xyz:CL","usdc":"-0.000062","szi":"-0.369"}}]"#;
    assert_eq!(parse_funding(json), [Funding { time: DateTime::from_timestamp_millis(1_790_323_200_066).unwrap(), coin: "xyz:CL".into(), usdc: d("-0.000062") }]);
    assert!(parse_funding(b"{\"error\": 1}").is_empty());
}

#[test]
fn spot_cost_is_the_average_and_a_sale_takes_its_share() {
    let mut basis = CostBasis::default();
    basis.bought(d("100"), d("10"), Utc::now()); // $0.10 each
    basis.bought(d("100"), d("30"), Utc::now()); // $0.30 each → average $0.20
    assert_eq!(basis.usd, d("40"));
    assert_eq!(basis.pnl(d("200"), d("0.25")), Some((d("10"), d("25"))));
    basis.sold(d("50")); // a quarter out: a quarter of the cost goes with it
    assert!(basis.amount == d("150") && basis.usd == d("30"));
    // Tokens that arrived from elsewhere don't count: only the 150 with a known cost.
    assert_eq!(basis.pnl(d("400"), d("0.1")), Some((d("-15"), d("-50"))));
    basis.sold(d("1000"));
    assert!(basis.amount.is_zero() && basis.usd.is_zero() && basis.pnl(d("5"), d("1")).is_none());
}

/// Blockscout's shape: SPCX bought for 20 USDG in one transaction, a third sold for 8, then 1 SPCX sent in by a friend.
#[test]
fn cost_basis_replays_the_wallets_transfers() {
    let (me, spcx, pool) = ("0xC0CE", "0x4A0E", "0xpool");
    let t = |token: &str, symbol: &str, value: &str, from: &str, to: &str| {
        json!({"token": {"address_hash": token, "symbol": symbol, "decimals": "6"}, "total": {"value": value}, "from": {"hash": from}, "to": {"hash": to}})
    };
    let txs: Vec<Vec<Value>> = vec![
        vec![t(spcx, "SPCX", "3000000", pool, me), t("0xusdg", "USDG", "20000000", me, pool)],
        vec![t(spcx, "SPCX", "1000000", me, pool), t("0xusdg", "USDG", "8000000", pool, me)],
        vec![t(spcx, "SPCX", "1000000", "0xfriend", me)],
    ];
    let changes = CostBasis::replay(&me.to_lowercase(), &spcx.to_lowercase(), &txs);
    assert_eq!(changes.iter().map(|c| c.tokens).collect::<Vec<_>>(), [d("3"), d("-1"), d("1")]);
    assert_eq!(changes.iter().map(|c| c.paid).collect::<Vec<_>>(), [Some(d("20")), None, None]);
    let unpriced = CostBasis::fold(&changes, d("3"), |_| None);
    assert_eq!(unpriced.amount, d("2")); // the gift has no price: no cost
    assert_eq!(unpriced.usd, d("20") * d("2") / d("3")); // the sale took a third of the cost
    assert_eq!(unpriced.pnl(d("3"), d("10")).map(|p| p.0), Some(d("20") - d("20") * d("2") / d("3"))); // P/L only on the 2 with a cost
    // Priced at $9 the day it came: counted at that, and marked as come in unpaid.
    let priced = CostBasis::fold(&changes, d("3"), |_| Some(d("9")));
    assert!(priced.amount == d("3") && priced.usd == d("20") * d("2") / d("3") + d("9") && priced.received == Some(d("1")) && priced.received_usd == Some(d("9")));
}

/// What was held before the oldest change the chain gave counts at the price that day; gas and sends take their share,
/// and a buy for dollars stays a buy.
#[test]
fn fold_counts_what_was_there_before_and_what_came_unpaid() {
    let day = |s: f64| unix(s);
    let changes = [
        Change { at: day(100.0), tokens: d("1"), paid: Some(d("3000")) }, // bought 1 ETH for $3,000
        Change { at: day(200.0), tokens: d("-1"), paid: None },           // sent half away
        Change { at: day(300.0), tokens: d("1"), paid: None },            // bridged in, unpaid
    ];
    assert_eq!(CostBasis::unpaid(&changes), [day(100.0), day(300.0)]);
    let basis = CostBasis::fold(&changes, d("2"), |at| if at == day(100.0) { Some(d("2000")) } else if at == day(300.0) { Some(d("4000")) } else { None });
    // 1 there before at $2,000 + 1 bought for $3,000; half out: 1 for $2,500 (0.5 of it unpaid, $1,000); 1 in at $4,000.
    assert!(basis.amount == d("2") && basis.usd == d("6500"));
    assert!(basis.received == Some(d("1.5")) && basis.received_usd == Some(d("5000")));
    assert_eq!(basis.since, Some(day(100.0)));
    let mut empty = CostBasis::fold(&[], d("5"), |_| Some(d("1")));
    assert!(empty.amount.is_zero());
    empty.came(d("2"), d("10"), Utc::now());
    empty.sold(d("2"));
    assert!(empty.received.is_none() && empty.since.is_none());
}

/// getTransaction's shape (jsonParsed): the wallet buys BONK for USDC, then for SOL (fee paid by it), gets some sent in,
/// sells part; a failed transaction counts for nothing. The cost is the USDC, then the SOL at its price then.
#[test]
fn solana_cost_from_the_wallets_own_transactions() {
    let me = "6nod592sTfEWD3VSVPdQndLMVBCNmMc6ngt7MyGBK21j";
    let (bonk, usdc) = ("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    let bal = |mint: &str, raw: &str, decimals: i64| json!({"mint": mint, "owner": me, "uiTokenAmount": {"amount": raw, "decimals": decimals}});
    let tx = |time: i64, pre: Vec<Value>, post: Vec<Value>, lamports: (i64, i64), err: Value| {
        json!({"blockTime": time,
               "meta": {"err": err, "fee": 5000, "preBalances": [lamports.0], "postBalances": [lamports.1], "preTokenBalances": pre, "postTokenBalances": post},
               "transaction": {"message": {"accountKeys": [{"pubkey": me, "signer": true}]}}})
    };
    let still = (5_000_000_000, 5_000_000_000);
    let txs = [
        tx(1_000, vec![bal(usdc, "10000000", 6)], vec![bal(usdc, "0", 6), bal(bonk, "100000", 5)], (5_000_000_000, 4_999_995_000), Value::Null),
        tx(2_000, vec![bal(bonk, "100000", 5)], vec![bal(bonk, "200000", 5)], (4_999_995_000, 4_899_990_000), Value::Null),
        tx(3_000, vec![bal(bonk, "200000", 5)], vec![bal(bonk, "300000", 5)], still, Value::Null),
        tx(4_000, vec![bal(bonk, "300000", 5)], vec![bal(bonk, "250000", 5)], still, Value::Null),
        tx(5_000, vec![bal(usdc, "10000000", 6)], vec![bal(usdc, "0", 6), bal(bonk, "999999999", 5)], still, json!({"InstructionError": [0, "Custom"]})),
    ];
    let moves: Vec<SolanaMove> = txs.iter().filter_map(|t| SolanaMove::from_tx(t, me)).collect();
    assert!(moves.len() == 4 && moves[1].sol == d("-0.1"));
    let changes = CostBasis::replay_solana(bonk, &moves, |at| (at == unix(2_000.0)).then(|| d("150")));
    assert_eq!(changes.iter().map(|c| c.paid).collect::<Vec<_>>(), [Some(d("10")), Some(d("15")), None, None]);
    let basis = CostBasis::fold(&changes, d("2.5"), |_| None);
    // 1 BONK for $10, 1 for 0.1 SOL × $150 = $15; 1 sent in (no price, no cost); 0.5 out takes a quarter of the 2 with a cost.
    assert!(basis.amount == d("1.5") && basis.usd == d("18.75"));
    assert!(basis.since == Some(unix(1_000.0)) && basis.price() == Some(d("12.5")));
    assert!(CostBasis::replay_solana(usdc, &moves, |_| Some(d("150"))).iter().all(|c| c.tokens < Decimal::ZERO || c.paid.is_none()));
    // SOL itself: the wallet's lamports (the fee given back), here 0.1 out to buy BONK.
    let sol: Vec<Decimal> = CostBasis::replay_solana(CostBasis::WSOL, &moves, |_| Some(d("150"))).iter().filter(|c| c.tokens < Decimal::ZERO).map(|c| c.tokens).collect();
    assert_eq!(sol, [d("-0.1")]);
}

#[test]
fn price_effect_ignores_money_moved_in_or_out() {
    let mark = |amount: &str, price: &str| Mark { amount: d(amount), price: d(price) };
    let before = HashMap::from([("sol".to_string(), mark("2", "100")), ("perp:ETH".into(), mark("-1", "3000")), ("gone".into(), mark("5", "1"))]);
    // SOL up $10 a coin, a top-up tripled it, the short gained $50, a sold coin dropped out, a new coin came in.
    let after = HashMap::from([("sol".to_string(), mark("6", "110")), ("perp:ETH".into(), mark("-1", "2950")), ("new".into(), mark("1000", "1"))]);
    assert_eq!(price_effect(&before, &after), d("70")); // 2 × 10 + (−1) × (−50); the top-up and the new coin count nothing
    assert_eq!(price_effect(&HashMap::new(), &after), d("0"));
}

/// The records the Swift app keeps: a cost basis and a flow read back as `Codable` wrote them.
#[test]
fn records_read_as_swift_wrote_them() {
    let basis: CostBasis = serde_json::from_str(r#"{"amount":1.5,"usd":18.75,"since":-978306200,"receivedUSD":9}"#).unwrap();
    assert!(basis.amount == d("1.5") && basis.since == Some(unix(1_000.0)) && basis.received.is_none() && basis.received_usd == Some(d("9")));
    assert_eq!(serde_json::to_value(&basis).unwrap(), json!({"amount": 1.5, "usd": 18.75, "since": -978306200.0, "receivedUSD": 9}));
    let old: CostBasis = serde_json::from_str(r#"{"amount":2,"usd":40}"#).unwrap();
    assert!(old.since.is_none() && old.received.is_none());
    let flow: Flow = serde_json::from_str(r#"{"at":0,"usd":-2.5,"note":"sent","wallet":"w"}"#).unwrap();
    assert!(flow.at == unix(978_307_200.0) && flow.usd == d("-2.5"));
    assert_eq!(serde_json::to_value(Period::Week).unwrap(), json!("week"));
}

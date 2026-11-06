use rust_decimal::Decimal;

use super::d;
use crate::foundation::unix;
use crate::trade_history::*;

fn fill(coin: &str, px: &str, sz: &str, side: &str, t: f64, start: &str, dir: &str, pnl: &str, fee: &str) -> Fill {
    Fill {
        coin: coin.into(), px: d(px), sz: d(sz), side: side.into(), time: unix(t), start_position: d(start), dir: dir.into(), closed_pnl: d(pnl),
        fee: d(fee), hash: None,
    }
}

#[test]
fn fills_become_position_stories() {
    // The real oil short of 2026-09-25 and the ETH test trade that overlapped nothing.
    let mut fills = vec![
        fill("xyz:CL", "93.417", "0.369", "A", 100.0, "0", "Open Short", "0", "0.003102"),
        fill("xyz:CL", "92.214", "0.369", "B", 200.0, "-0.369", "Close Short", "0.443907", "0.003062"),
        fill("ETH", "2694.4", "0.0058", "B", 300.0, "0", "Open Long", "0", "0.007032"),
        fill("ETH", "2694.2", "0.0058", "A", 400.0, "0.0058", "Close Long", "-0.00116", "0.007031"),
        // Still open: half taken, half running.
        fill("BTC", "60000", "0.0002", "B", 500.0, "0", "Open Long", "0", "0.003"),
        fill("BTC", "61000", "0.0001", "A", 600.0, "0.0002", "Close Long", "0.1", "0.003"),
    ];
    fills.reverse();
    let stories = stories(&fills);
    assert_eq!(stories.iter().map(|s| s.coin.as_str()).collect::<Vec<_>>(), ["BTC", "ETH", "xyz:CL"]);
    let oil = &stories[2];
    assert!(!oil.long && !oil.is_open() && oil.fills.len() == 2);
    assert_eq!(oil.realized(), d("0.443907") - d("0.006164"));
    assert!(stories[0].is_open() && stories[0].long && stories[0].closed().is_none() && stories[0].peak_size() == d("0.0002"));
    assert_eq!(oil.id(), "xyz:CL-100.0");
}

#[test]
fn ledger_in_words() {
    let me = "0x1523aaaa";
    let json = r#"
    [{"time": 1000, "delta": {"type": "send", "user": "0xrelay", "destination": "0x1523AAAA", "sourceDex": "spot", "destinationDex": "", "token": "USDC", "amount": "2.98", "usdcValue": "2.98", "fee": "0.0"}},
     {"time": 2000, "delta": {"type": "send", "user": "0x1523aaaa", "destination": "0x1523aaaa", "sourceDex": "", "destinationDex": "xyz", "token": "USDC", "amount": "1.2", "usdcValue": "1.2", "fee": "0.0"}},
     {"time": 3000, "delta": {"type": "send", "user": "0x1523aaaa", "destination": "0xrelaydepository", "sourceDex": "", "destinationDex": "spot", "token": "USDC", "amount": "1.0", "usdcValue": "1.0", "fee": "0.0"}}]
    "#;
    let items = parse_transfers(json.as_bytes(), me);
    assert_eq!(items.iter().map(|t| t.amount).collect::<Vec<_>>(), [d("-1"), Decimal::ZERO, d("2.98")]);
    assert_eq!(items[1].text, "Moved $1.20 to the xyz exchange");
    assert_eq!(items[2].text, "Received $2.98 (bridge in)");
    assert_eq!(items[0].text, "Sent $1.00 out (bridge to another chain)");
    let more = r#"[{"time": 1, "delta": {"type": "deposit", "usdc": "5"}}, {"time": 2, "delta": {"type": "withdraw", "usdc": "1.5"}},
                   {"time": 3, "delta": {"type": "accountClassTransfer", "usdc": "2", "toPerp": true}},
                   {"time": 4, "delta": {"type": "vaultWithdraw", "netWithdrawnUsd": "3"}}, {"time": 5, "delta": {"type": "rewardsClaim"}},
                   {"time": 6, "delta": {"type": "send", "user": "0x1523aaaa", "destination": "0xother", "usdcValue": "4", "fee": "1"}}]"#;
    let words: Vec<(String, Decimal)> = parse_transfers(more.as_bytes(), me).into_iter().map(|t| (t.text, t.amount)).collect();
    assert_eq!(words, [
        ("Sent $4.00 out (bridge to another chain), fee $1.00".into(), d("-4")), ("rewardsClaim".into(), Decimal::ZERO),
        ("Withdrew $3.00 from a vault".into(), Decimal::ZERO), ("Moved $2.00 from spot to perps".into(), Decimal::ZERO),
        ("Withdrawal $1.50".into(), d("-1.5")), ("Deposit $5.00".into(), d("5")),
    ]);
}

/// userFills as Hyperliquid writes them: numbers as text, the time in milliseconds.
#[test]
fn fills_read_as_hyperliquid_writes_them() {
    let json = r#"[{"coin":"xyz:CL","px":"93.417","sz":"0.369","side":"A","time":1790323200066,"startPosition":"0.0","dir":"Open Short",
                    "closedPnl":"0.0","hash":"0xabc","oid":1,"crossed":true,"fee":"0.003102","tid":2,"feeToken":"USDC"}]"#;
    let fills: Vec<Fill> = serde_json::from_str(json).unwrap();
    assert!(fills[0].px == d("93.417") && fills[0].end_position() == d("-0.369") && fills[0].hash.as_deref() == Some("0xabc"));
    assert_eq!(fills[0].time.timestamp_millis(), 1_790_323_200_066);
    // A number that isn't one reads as 0; a field missing is no fill.
    let odd: Vec<Fill> = serde_json::from_str(&json.replace("\"0.003102\"", "\"n/a\"")).unwrap();
    assert!(odd[0].fee.is_zero());
    assert!(serde_json::from_str::<Vec<Fill>>(&json.replace("\"dir\":\"Open Short\",", "")).is_err());
}

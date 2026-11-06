use chrono::Utc;
use rust_decimal::Decimal;

use super::d;
use crate::foundation::{Date, adding, unix};
use crate::pnl::Funding;
use crate::trade_history::{Fill, stories};
use crate::trade_study::*;

fn t0() -> Date { unix(1_800_000_000.0) }
fn at(hours: f64) -> Date { adding(t0(), hours * 3600.0) }

fn fill(coin: &str, side: &str, sz: &str, px: &str, start: &str, pnl: &str, hours: f64, dir: &str) -> Fill {
    Fill {
        coin: coin.into(), px: d(px), sz: d(sz), side: side.into(), time: at(hours), start_position: d(start), dir: dir.into(), closed_pnl: d(pnl),
        fee: d("1"), hash: None,
    }
}

/// A HYPE long that wins 50 (two fees), an ETH short added to as it went against it that loses 30 and paid 2 of
/// funding, a BTC long that wins 10, and a SOL long still open.
fn fills() -> Vec<Fill> {
    vec![
        fill("HYPE", "B", "10", "20", "0", "0", 0.0, ""), fill("HYPE", "A", "10", "25", "10", "50", 5.0, ""),
        fill("ETH", "A", "1", "3000", "0", "0", 10.0, ""), fill("ETH", "A", "1", "3100", "-1", "0", 12.0, ""),
        fill("ETH", "B", "2", "3065", "-2", "-30", 40.0, ""),
        fill("BTC", "B", "0.1", "60000", "0", "0", 50.0, ""), fill("BTC", "A", "0.1", "60100", "0.1", "10", 51.0, ""),
        fill("SOL", "B", "1", "150", "0", "0", 60.0, ""),
    ]
}

fn funding() -> Vec<Funding> {
    vec![Funding { time: at(20.0), coin: "ETH".into(), usdc: d("-2") }, Funding { time: at(70.0), coin: "ETH".into(), usdc: d("-5") }]
}

#[test]
fn trades_are_measured_after_fees_and_funding() {
    let trades = trades(&stories(&fills()), &funding());
    assert_eq!(trades.iter().map(|t| t.coin.as_str()).collect::<Vec<_>>(), ["HYPE", "ETH", "BTC"]); // closed only, oldest first
    assert_eq!(trades.iter().map(|t| t.net).collect::<Vec<_>>(), [d("48"), d("-35"), d("8")]); // fees 2, 3, 2; ETH's funding while open only
    let eth = &trades[1];
    assert!(!eth.long && eth.adds == 1 && eth.adds_losing == 1); // a short added above its entry: adding to a loser
    assert!(eth.entry == d("3050") && eth.peak_usd == d("6100"));
    let s = stats(&trades);
    assert!(s.count == 3 && s.wins == 2 && s.total == d("21"));
    assert_eq!(s.profit_factor, Some(d("56") / d("35")));
    assert_eq!(s.max_drawdown, d("35"));
    assert_eq!(concentration(&trades), None); // two winners say nothing about luck
    let report = report(&trades, 1, 0, &Utc);
    assert!(report.contains("All: 3 trades, won 67%, net $21.00"), "{report}");
    assert!(report.contains("Added to a loser: 1 trade, won 0%"));
    // A position the history begins inside of has no entry: left out, and said.
    let mut cut_short = vec![fill("BTC", "A", "1", "60000", "1", "5", 1.0, "")];
    cut_short.extend(fills());
    assert_eq!(super::super::trade_study::trades(&stories(&cut_short), &[]).len(), 3);
    assert_eq!(cut(&stories(&cut_short)), 1);
    assert!(report.contains("1 still open, not counted"));
    assert_eq!(super::super::trade_study::report(&[], 0, 0, &Utc), "No closed Hyperliquid positions in this history.");
    // t0 is a Friday in UTC: the HYPE and ETH trades opened then, BTC two days later.
    assert!(report.contains("\nBy the day opened: Sun 1 / $8.00, Fri 2 / $13.00.\n"), "{report}");
    assert!(report.contains("Biggest winners: ") && report.contains("long HYPE ≈ $200.00 at 20, held 5 h → $48.00"), "{report}");
    assert!(report.contains("Biggest losers: ") && report.contains("short ETH ≈ $6,100.00 at 3,050, 1 add, held 30 h → −$35.00"), "{report}");
}

#[test]
fn most_of_the_profit_from_a_few_trades() {
    let trade = |net: i64| Trade {
        coin: "BTC".into(), long: true, opened: t0(), closed: at(1.0), net: Decimal::from(net), fees: Decimal::ZERO, funding: Decimal::ZERO,
        peak_usd: d("100"), entry: d("1"), adds: 0, adds_losing: 0, liquidated: false,
    };
    let trades: Vec<Trade> = [100, 10, 10, 10, -20].into_iter().map(trade).collect();
    assert_eq!(concentration(&trades), Some((1, d("100") / d("130") * d("100"))));
    assert!(report(&trades, 0, 0, &Utc).contains("77% of the profit came from 1 trade."));
    assert!(report(&trades, 0, 0, &Utc).contains("Without the best 1: 4 trades, won 75%, net $10.00"));
}

#[test]
fn a_flip_closes_one_trade_and_opens_the_next() {
    let flip = [fill("ETH", "B", "1", "3000", "0", "0", 0.0, ""), fill("ETH", "A", "3", "3100", "1", "100", 2.0, "Long > Short"),
                fill("ETH", "B", "2", "3050", "-2", "100", 4.0, "")];
    let trades = trades(&stories(&flip), &[]);
    assert_eq!(trades.len(), 2);
    assert!(trades[0].long && trades[0].adds == 0 && trades[0].peak_usd == d("3000"));
    assert!(!trades[1].long && trades[1].entry == d("3100"));
}

#[test]
fn time_held_in_words() {
    assert_eq!(held(0.001), "1 min");
    assert_eq!(held(0.5), "30 min");
    assert_eq!(held(30.0), "30 h");
    assert_eq!(held(47.6), "48 h");
    assert_eq!(held(72.0), "3 d");
}

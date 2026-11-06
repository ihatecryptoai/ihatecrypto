use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;

use super::{d, set};
use crate::charts::*;
use crate::modules::ModuleNetwork;
use crate::network::Hyperliquid;

#[test]
fn chart_phrases() {
    let markets = set(&["ETH", "BTC", "SOL", "xyz:CL", "xyz:GOLD"]);
    let chart = |coin: &str, range| Some(ChartCommand::Chart { coin: coin.into(), range });
    for (phrase, want) in [
        ("chart ETH", chart("ETH", Range::Day)),
        ("график нефти за неделю", chart("xyz:CL", Range::Week)),
        ("покажи эфир", chart("ETH", Range::Day)),
        ("show me BTC for a month", chart("BTC", Range::Month)),
        ("how's SOL doing?", chart("SOL", Range::Day)),
        ("свечи золота", chart("xyz:GOLD", Range::Day)),
        ("what's moving?", Some(ChartCommand::Movers)),
        ("что растёт сегодня", Some(ChartCommand::Movers)),
        ("top gainers", Some(ChartCommand::Movers)),
    ] {
        assert_eq!(ChartCommand::parse(phrase, &markets), want, "«{phrase}»");
    }
    for phrase in ["long ETH risk $5", "сколько стоит нефть", "show my addresses", "покажи адреса", "задепозить"] {
        assert_eq!(ChartCommand::parse(phrase, &set(&["ETH", "xyz:CL"])), None, "«{phrase}»");
    }
}

#[test]
fn candles_from_hyperliquid_json() {
    let json = r#"[{"t":1790000000000,"T":1790000899999,"s":"ETH","i":"15m","o":"2690.1","c":"2688.0","h":"2695.5","l":"2680.2","v":"12.3","n":40},{"t":1789999100000,"T":1789999999999,"s":"ETH","i":"15m","o":"2685.0","c":"2690.1","h":"2692.0","l":"2684.0","v":"8.1","n":22}]"#;
    let candles = Hyperliquid::parse_candles(json.as_bytes()).unwrap();
    assert_eq!(candles.len(), 2);
    assert_eq!(candles[0].close, d("2690.1"));   // sorted oldest first
    assert_eq!(candles[1].high, d("2695.5"));
    assert!(Range::Week.interval() == "1h" && Range::Week.span() / 3_600_000 == 168);
    assert_eq!(Hyperliquid::parse_candles(br#"[{"t":1,"o":"x","h":"1","l":"1","c":"1"}]"#).unwrap_err().0, "Unexpected number format: x");
    // As Codable writes it: the time in seconds since 2001, the prices as numbers.
    let json = serde_json::to_value(&candles[1]).unwrap();
    assert_eq!(json["time"].as_f64(), Some(1_790_000_000.0 - 978_307_200.0));
    assert_eq!(json["open"].to_string(), "2690.1");
    assert_eq!(serde_json::from_value::<Candle>(json).unwrap(), candles[1]);
}

#[test]
fn allocation_shares() {
    let s = PortfolioSplit::slices(&[("Solana", d("2")), ("Hyperliquid", d("10")), ("Jupiter", d("3")), ("Aave", Decimal::ZERO), ("Base", d("-1"))]);
    assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Hyperliquid", "Jupiter", "Solana"]);
    assert_eq!(s.iter().map(|s| s.share).sum::<Decimal>(), d("100"));
    assert_eq!(s[0].share, d("66.7"));
    assert!(PortfolioSplit::slices(&[]).is_empty() && PortfolioSplit::slices(&[("x", Decimal::ZERO)]).is_empty());
}

#[test]
fn portfolio_network_order() {
    use ModuleNetwork::*;
    // The wallet of 2026-10-01: Solana, Hyperliquid at $0 and Robinhood under $100 still lead; Base and Arbitrum follow
    // by money; NEAR and Tempo, under $100, fold; Ethereum with nothing on it is left out.
    let held: HashMap<ModuleNetwork, Decimal> = [(Base, "2685.70"), (Solana, "190.77"), (Arbitrum, "102.94"), (Robinhood, "43.83"), (Near, "0.38"),
                                                 (Tempo, "0.43"), (Hyperliquid, "0"), (Ethereum, "0")].iter().map(|(n, v)| (*n, d(v))).collect();
    let (cards, folded) = PortfolioSplit::network_cards(&held, &HashSet::from([Solana, Hyperliquid]));
    assert_eq!(cards, [Solana, Hyperliquid, Robinhood, Base, Arbitrum]);
    assert_eq!(folded, [Tempo, Near]);
    // A pinned network without an account and without money gets no card; exactly $100 folds.
    let (cards, folded) = PortfolioSplit::network_cards(&HashMap::from([(Robinhood, Decimal::ZERO), (Polygon, d("100"))]), &HashSet::new());
    assert!(cards.is_empty() && folded == [Polygon]);
}

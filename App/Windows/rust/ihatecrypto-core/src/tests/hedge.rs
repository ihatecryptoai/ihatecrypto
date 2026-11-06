use std::collections::HashMap;

use rust_decimal::Decimal;

use super::d;
use crate::hedge::{guard_stop, spot};
use crate::plan::Market;

fn sol() -> Market {
    Market { prev_day_px: Some(d("110")), funding: Some(d("0.00001")), atr_pct: Some(d("2")), ..Market::new("SOL", d("100"), 2, 20) }
}

// pending: hedgeFormsParseInEnglishAndRussian, aHedgeIsAPlanOfItsOwn, aHedgeIsSizedByTheCoinsHeld (IntentParser.apply,
// Plan.resolve, RiskRules: Intent and Plan rounds).
#[test]
fn spot_held_maps_to_its_market() {
    let marks: HashMap<String, Decimal> = [("SOL", "100"), ("ETH", "2500"), ("BTC", "80000"), ("xyz:TSLA", "400"), ("xyz:NVDA", "180"), ("xyz:META", "700"),
                                           ("xyz:GOLD", "4000")].iter().map(|(k, v)| (k.to_string(), d(v))).collect();
    let held = [
        ("SOL", d("1"), None), ("ETH", d("2"), None), ("WETH", d("1"), Some(d("2490"))), ("cbBTC", d("0.1"), Some(d("79500"))), ("TSLA", d("3"), Some(d("401"))),
        ("NVDAon", d("2"), Some(d("181"))), ("BONK", d("1000"), Some(d("0.00002"))), ("USDC", d("50"), Some(d("1"))), ("SOL", d("0"), None),
        // An airdropped «WETH» with no real price, MetaDAO's META, a GOLD memecoin: none of them is that market's coin.
        ("WETH", d("50"), Some(d("0"))), ("META", d("10"), Some(d("0.3"))), ("GOLD", d("5000"), Some(d("0.01"))),
    ];
    let want: HashMap<String, Decimal> = [("SOL", "1"), ("ETH", "3"), ("BTC", "0.1"), ("xyz:TSLA", "3"), ("xyz:NVDA", "2")].iter().map(|(k, v)| (k.to_string(), d(v))).collect();
    assert_eq!(spot(&held, &marks), want);
    assert_eq!(guard_stop(d("2"), &sol()), Some(d("37")));
    assert_eq!(guard_stop(d("1"), &sol()), Some(d("76")));
    // Where Swift gets NaN, no stop rather than a panic.
    assert_eq!(guard_stop(Decimal::ZERO, &sol()), None);   // 0.975 / 1.025 × 80
}

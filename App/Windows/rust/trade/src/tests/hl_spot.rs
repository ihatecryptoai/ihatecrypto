//! OneClickTests' spot cases: a pair from spotMeta's lists, and a spot swap that checks both pairs before the sale
//! (main's 23a6da3).

use serde_json::{Map, Value, json};

use super::fake_hl::ZeroSigner;
use super::g4_fake::spot_hl;
use crate::hl_spot::SpotPair;
use crate::{Error, Executor};

fn objects(v: &Value) -> Vec<&Map<String, Value>> { v.as_array().unwrap().iter().map(|o| o.as_object().unwrap()).collect() }

/// A spot token's USDC pair from spotMeta's lists: asset 10000 + the pair's index, its key in allMids, its size decimals.
#[test]
fn hyperliquid_spot_pair_from_its_lists() {
    let (tokens, universe) = (json!([{"name": "USDC", "index": 0, "szDecimals": 8}, {"name": "PURR", "index": 1, "szDecimals": 0}, {"name": "HYPE", "index": 150, "szDecimals": 2}]),
                              json!([{"tokens": [1, 0], "index": 0, "name": "PURR/USDC"}, {"tokens": [150, 0], "index": 107, "name": "@107"}]));
    let (tokens, universe) = (objects(&tokens), objects(&universe));
    assert_eq!(Executor::spot_pair_in("hype", &tokens, &universe).unwrap(), SpotPair { asset: 10_107, key: "@107".into(), symbol: "HYPE".into(), sz_decimals: 2 });
    assert_eq!(Executor::spot_pair_in("PURR", &tokens, &universe).unwrap().asset, 10_000);
    assert!(Executor::spot_pair_in("DOGE", &tokens, &universe).is_err());
}

/// A spot swap finds both pairs before the sale: a `to` the book can't buy (USDC itself, «USDC.e») or `from` again (USDT
/// is USDT0 there) stops with nothing signed or sent, so Try again can't sell twice. A pair the book has goes on.
#[tokio::test]
async fn spot_swap_checks_both_pairs_before_the_sale() {
    let hl = spot_hl().await;
    let journal = super::fake_hl::journal("spot");
    let e = Executor::new(execution::trade_signer::TradeWallet { solana: String::new(), evm: "0x0".into() }, std::sync::Arc::new(ZeroSigner::default()),
                          journal.clone(), |_| {});
    for (from, to, why) in [("HYPE", "USDC", "USDC has no USDC pair"), ("HYPE", "USDC.e", "USDC.E has no USDC pair"),
                            ("HYPE", "hype", "HYPE for HYPE: the same token"), ("USDT", "USDT0", "USDT0 for USDT0: the same token")] {
        let error = e.hl_spot_swap(from, to, Some(super::d("1"))).await;
        assert!(matches!(&error, Err(Error::Execution(m)) if m.contains(why) && m.ends_with("Nothing was signed.")), "{from} → {to}: {error:?}");
    }
    // HYPE for PURR passes the check and goes on to the spot balance, which this Hyperliquid doesn't answer.
    let next = e.hl_spot_swap("HYPE", "PURR", Some(super::d("1"))).await;
    assert_eq!(next, Err(Error::Execution("Hyperliquid's spot balance didn't come.".into())));
    assert_eq!(hl.sent(), 0);
    assert!(journal.entries().unwrap().is_empty());
}

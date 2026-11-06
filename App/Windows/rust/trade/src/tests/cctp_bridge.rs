//! Circle's fee tiers read into the burn's cap and what lands.

use serde_json::json;

use super::d;
use crate::Executor;

#[test]
fn cctp_caps_its_fee_a_fifth_over_the_fast_tier() {
    let tiers = json!([{"finalityThreshold": 2000, "minimumFee": 0}, {"finalityThreshold": 1000, "minimumFee": 1.3, "forwardFee": {"low": 1000, "med": 2500, "high": 9000}}]);
    // 20 USDC: 1.3 bp is 2,600 units, plus the forward's 2,500: 5,100; the cap 6,120; 19.9949 lands.
    assert_eq!(Executor::cctp_fees(&tiers, 20_000_000), Some((6_120, d("19.9949"))));
    // A fee past what moves isn't a quote.
    assert_eq!(Executor::cctp_fees(&tiers, 2_000), None);
    assert_eq!(Executor::cctp_fees(&json!([{"finalityThreshold": 2000, "minimumFee": 0}]), 20_000_000), None);
}

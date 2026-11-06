//! OneClickTests' offline cases: 1Click's USDC on each place, the edge it needs over Relay.

use protocols::one_click::asset;

use super::d;
use crate::Executor;

#[test]
fn one_click_usdc_on_the_bridged_places() {
    assert_eq!(Executor::one_click_usdc(None), Some(asset::SOLANA_USDC));
    for net in ["base", "arbitrum", "ethereum"] { assert_eq!(Executor::one_click_usdc(Some(net)), asset::usdc_on_evm(net)) }
    assert_eq!(Executor::one_click_usdc(Some("hyperliquid")), Some(asset::HYPER_CORE_USDC));
    assert!(Executor::one_click_usdc(Some("polygon")).is_none() && Executor::one_click_usdc(Some("robinhood")).is_none());
}

/// 1Click is taken over Relay only for an edge worth another Windows Hello: 10 cents, or 0.05% of a bigger move.
#[test]
fn one_click_needs_an_edge_over_relay() {
    assert_eq!(Executor::one_click_edge(d("5")), d("0.10"));
    assert_eq!(Executor::one_click_edge(d("200")), d("0.10"));
    assert_eq!(Executor::one_click_edge(d("1000")), d("0.5"));
}

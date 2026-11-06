//! Swap.swift's offline cases: Batch7702Tests' tolerance and ExecutorTests' ways, Relay's loss and sale, the card's way
//! and the wrap's failure; plus a ticker read from Uniswap's list and checked on a stand-in node, and KyberSwap's router.

use std::sync::Arc;

use execution::trade_signer::TradeWallet;
use net::evm_rpc::EvmRpc;
use rust_decimal::Decimal;
use serde_json::json;

use super::d;
use super::fake_hl::{ZeroSigner, journal};
use super::g4_fake::serve;
use crate::swap::{SwapWay, UniRoute, kyber_route, set_web};
use crate::{CostlyMove, Error, EvmNetwork, Executor, IMPACT_ACCEPTED};

/// The floor under a Uniswap swap follows its price impact: half a percent in a deep pool, up to 3% in a thin one.
#[test]
fn swap_tolerance_follows_price_impact() {
    assert_eq!(UniRoute::tolerance_of(None), d("0.01"));
    assert_eq!(UniRoute::tolerance_of(Some(d("0.0004"))), d("0.005"));
    assert_eq!(UniRoute::tolerance_of(Some(d("0.012"))), d("0.01"));
    assert_eq!(UniRoute::tolerance_of(Some(d("0.03"))), d("0.025"));
    assert_eq!(UniRoute::tolerance_of(Some(d("0.2"))), d("0.03"));
    let mut deep = UniRoute { impact: Some(d("0.0004")), ..UniRoute::of_v3(vec![], 1, Decimal::ONE) };
    assert!(deep.impact_text().is_empty());
    deep.impact = Some(d("0.012"));
    assert_eq!(deep.impact_text(), " · price impact 1.2%");
}

#[test]
fn a_picked_swap_way_only_narrows_the_quoted_ones() {
    let relay = SwapWay::new("relay", d("100"), d("1.90"), "");
    let bridge = SwapWay { venue: "KyberSwap".into(), ..SwapWay::new("bridge", d("101"), d("1.95"), "") };
    let ways = [relay.clone(), bridge.clone()];
    assert_eq!(Executor::pick(&ways, None).unwrap().name, "bridge"); // whichever leaves more
    assert_eq!(Executor::pick(&ways, Some("relay")).unwrap().name, "relay"); // the one picked, even if it leaves less
    assert!(Executor::pick(std::slice::from_ref(&bridge), Some("relay")).is_none()); // picked but unquoted: nothing is signed
    // The card names the exchange after the bridge, and a pick of the bridge's way finds it whichever exchange it is.
    assert!(Executor::way_key(&bridge) == "bridge:KyberSwap" && Executor::way_key(&relay) == "relay");
    assert_eq!(Executor::pick(&ways, Some("bridge")).unwrap().venue, "KyberSwap");
    assert!(Executor::pick(&[], None).is_none());
}

// Relay's swap losing past its cap asks once: «Send anyway» takes its loss as an impact, which passes the impact stop on
// the same loss too — two separate answers had asked in turn forever.
#[test]
fn relays_loss_asks_once_for_both_stops() {
    Executor::holds_relay_loss(d("100"), d("98")).unwrap(); // 2%: under its 3% cap
    let Err(Error::Costly(stop)) = Executor::holds_relay_loss(d("100"), d("90")) else { panic!("a 10% loss went without asking") };
    let impact = stop.impact.unwrap();
    assert_eq!(impact, d("0.1"));
    IMPACT_ACCEPTED.sync_scope(Decimal::ONE.min(impact * d("1.25") + d("0.005")), || {
        Executor::holds_relay_loss(d("100"), d("90")).unwrap();
        Executor::check_impact(impact, d("100"), d("100"), "USDC", "Relay's swap", None).unwrap();
    });
}

// A failure after the coin's wrap says it's the wrapped token now and how it comes back, never «Nothing was signed»; a
// guard's stop stays a guard's stop.
#[test]
fn a_failure_after_the_wrap_says_where_the_coin_is() {
    let back = " 0.001 ETH is WETH in the wallet now: «swap 0.001 WETH for ETH on base» turns it back.";
    let failed = Executor::left_wrapped(&Error::Execution("Approve exactly 0.001 WETH to Relay failed. Nothing was signed.".into()), d("0.001"), "WETH",
                                        EvmNetwork::base());
    assert_eq!(failed, Error::Execution(format!("Approve exactly 0.001 WETH to Relay failed.{back}")));
    let stop = Executor::left_wrapped(&Error::Guard("pays 0xdead".into()), d("0.001"), "WETH", EvmNetwork::base());
    assert_eq!(stop, Error::Guard(format!("pays 0xdead.{back}")));
    // «All» of the coin is its balance less the fees, in wei: the way back names it rounded down, never over what was wrapped.
    let bnb = Executor::left_wrapped(&Error::Wallet("The request timed out.".into()), d("0.012345678987654321"), "WBNB", EvmNetwork::bnb());
    assert!(bnb.to_string().ends_with(" 0.01234567 BNB is WBNB in the wallet now: «swap 0.01234567 WBNB for BNB on bnb» turns it back."), "{bnb}");
}

// Relay's sale with no dollar price for what goes in stops before the swap: read as $0, any loss had passed.
#[test]
fn relays_sale_without_a_price_stops() {
    assert!(matches!(Executor::holds_relay_sale(Decimal::ZERO, d("5"), "XYZ", "Base"), Err(Error::Execution(_))));
    Executor::holds_relay_sale(d("10"), d("9.8"), "AERO", "Base").unwrap(); // 2%: passes
    assert!(matches!(Executor::holds_relay_sale(d("10"), d("9"), "AERO", "Base"), Err(Error::Costly(CostlyMove { .. }))));
}

// What goes in: Relay's valuation first (DefiLlama not asked), else DefiLlama's sure price × the amount, else 0 (stopped).
#[tokio::test]
async fn relays_sale_is_worth_relays_price_else_defillamas_sure_one() {
    let asked = std::sync::atomic::AtomicBool::new(false);
    let worth = Executor::relay_worth_in_by("0.697351", Decimal::ONE, || async {
        asked.store(true, std::sync::atomic::Ordering::Relaxed);
        Some((d("5"), true))
    }).await;
    assert_eq!(worth, d("0.697351"));
    assert!(!asked.load(std::sync::atomic::Ordering::Relaxed));
    for empty in ["", "0", "-1", "NaN"] {
        assert_eq!(Executor::relay_worth_in_by(empty, d("1.5"), || async { Some((d("0.68"), true)) }).await, d("1.02"), "{empty}");
    }
    assert_eq!(Executor::relay_worth_in_by("", d("2"), || async { Some((d("0.68"), false)) }).await, Decimal::ZERO); // unsure
    assert_eq!(Executor::relay_worth_in_by("", d("2"), || async { None }).await, Decimal::ZERO); // no price
}

// Confirm takes the way the card showed: Uniswap's versions are one; another exchange or way stops before anything is signed.
#[test]
fn a_way_other_than_the_cards_stops_before_signing() {
    Executor::holds_shown_way("bridge:Uniswap v4", Some("bridge:Uniswap v3")).unwrap();
    Executor::holds_shown_way("here:KyberSwap", None).unwrap(); // no card: a scheduled buy
    assert_eq!(Executor::holds_shown_way("bridge:Uniswap v3", Some("bridge:KyberSwap")).unwrap_err(),
               Error::Execution("The card showed Relay's bridge + KyberSwap, but Relay's bridge + Uniswap leaves more now. Nothing was signed. Refresh the card \
                                 (or ask again) to see it.".into()));
    assert!(matches!(Executor::holds_shown_way("relay", Some("bridge:KyberSwap")), Err(Error::Execution(_))));
    assert!(matches!(Executor::holds_shown_way("bridge:KyberSwap", Some("here:KyberSwap")), Err(Error::Execution(_))));
}

/// A ticker comes from Uniswap's list and its decimals are read on the chain: a contract that disagrees with the list is
/// never traded. KyberSwap's route counts only from its own router. (Uniswap's list, the node and KyberSwap on one stand-in.)
#[tokio::test]
async fn a_listed_token_is_checked_on_its_contract() {
    let aero = "0x940181a94a35a4569e4529a3cdfb74e38fd98631";
    let decimals = Arc::new(std::sync::atomic::AtomicU64::new(18));
    let on_chain = decimals.clone();
    let (url, asked) = serve(move |path, body| {
        if path.starts_with("/base/api/v1/routes") {
            let router = if path.contains("tokenOut=0xbad") { "0x0000000000000000000000000000000000000bad" } else { "0x6131B5fAe19EA4f9D964eAc0408E4408b66337b5" };
            return Some(json!({"data": {"routerAddress": router, "routeSummary": {"amountOut": "2410000000000000000", "amountInUsd": "2", "amountOutUsd": "1.97",
                                                                                     "gasUsd": "0.01", "gas": "180000"}}}).to_string());
        }
        if body.get("method").is_some() {
            let word = format!("0x{:064x}", on_chain.load(std::sync::atomic::Ordering::Relaxed));
            return Some(json!({"jsonrpc": "2.0", "id": 1, "result": word}).to_string());
        }
        Some(json!({"tokens": [{"chainId": 1, "symbol": "AERO", "address": "0x1111111111111111111111111111111111111111", "decimals": 18},
                               {"chainId": 8453, "symbol": "AERO", "name": "Aerodrome", "address": "0x940181A94A35A4569E4529A3CDfB74e38FD98631", "decimals": 18}]})
            .to_string())
    }).await;
    set_web(Some(url.clone()));
    let net = EvmNetwork { rpc: EvmRpc::pinned(&url, 8453), ..EvmNetwork::base().clone() };
    let e = Executor::new(TradeWallet { solana: String::new(), evm: "0x90f79bf6eb2c4f870365e785982e1f101e93b906".into() }, Arc::new(ZeroSigner::default()),
                          journal("g3"), |_| {});
    let token = e.evm_token("$aero", &net).await.unwrap();
    assert_eq!((token.symbol.as_str(), token.name.as_str(), token.address.as_str(), token.decimals), ("AERO", "Aerodrome", aero, 18));
    decimals.store(6, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(e.evm_token("AERO", &net).await.unwrap_err(), Error::Execution("The AERO contract on Base doesn't match Uniswap's list. Nothing was signed.".into()));

    let route = kyber_route(8453, evm::BASE_USDC, aero, d("2000000")).await.unwrap();
    assert_eq!((route.out, route.gas, route.impact()), (d("2410000000000000000"), 180_000, Some(d("0.015"))));
    assert!(kyber_route(8453, evm::BASE_USDC, "0xbad", d("2000000")).await.unwrap_err().to_string().starts_with("KyberSwap: "));
    set_web(None);
    let paths: Vec<String> = asked.lock().unwrap().iter().map(|(p, _)| p.clone()).collect();
    assert!(paths.iter().any(|p| p == "/base/api/v1/routes?tokenIn=0x833589fcd6edb6e08f4c7c32d4f71b54bda02913&tokenOut=0x940181a94a35a4569e4529a3cdfb74e38fd98631\
                                       &amountIn=2000000"), "{paths:?}");
}

/// The engine runs a move on its own task: every swap the chat can start is a future that may go to another thread.
#[allow(dead_code)]
fn swaps_run_on_any_thread(e: &Executor, net: &EvmNetwork) {
    fn send<T: Send>(_: T) {}
    send(e.buy_on(net, "AERO", Decimal::ONE, None, None, None));
    send(e.sell_on(net, "AERO", None, None, false, None));
    send(e.swap_on(net, "ETH", "AERO", None));
    send(e.swap_quotes(net, "AERO", None, None, None, false));
    send(e.pair_quote(net, "ETH", "AERO", None));
    send(e.buy_stock("TSLA", Decimal::ONE));
    send(e.sell_stock("TSLA", None));
    send(e.stock_quotes("TSLA", None, None));
    send(e.cross_swap("SOL", "solana", "ETH", "base", None, None));
    send(e.cross_quote_text("SOL", "solana", "ETH", "base", None, None));
    send(e.intent_swap("NEAR", "near", "USDC", "solana", None, None));
    send(e.tempo_swap("USDC", "USDT0", None));
}

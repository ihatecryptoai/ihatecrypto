//! The EVM networks' rules (EVMTests, ExecutorTests), the gas top-up's words and the batching switch (Batch7702Tests).

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use evm::U256;
use evm::call::Call;
use execution::trade_signer::TradeWallet;
use rust_decimal::Decimal;

use super::d;
use super::fake_hl::{ZeroSigner, journal};
use crate::{BATCHES_ON, EvmNetwork, Executor};

fn held<'a>(m: &'a HashMap<&str, Decimal>) -> impl Fn(&str) -> Option<Decimal> + 'a { move |k| m.get(k).copied() }

#[test]
fn gas_top_up_target_is_a_whole_number_of_wei() {
    // $1 at $3000 an ETH, 80% of it: 266,666,666,666,666 wei, not the 0 an unrounded quotient read as.
    assert_eq!(Executor::gas_expected(Decimal::ONE, d("3000")), 266_666_666_666_666);
    assert!(Executor::gas_expected(Decimal::TWO, d("2456.37")) > 600_000_000_000_000);
    assert_eq!(Executor::gas_expected(Decimal::ONE, Decimal::ZERO), 1);
}

#[test]
fn gas_top_up_is_said_done_only_when_the_coin_grew() {
    let before: u128 = 420_000_000_000_000;
    assert_eq!(Executor::gas_line(EvmNetwork::base(), before, 690_000_000_000_000, Decimal::ONE), "Topped up gas on Base: 0.00042 → 0.00069 ETH (≈ $1.00).");
    // Not there yet (a node behind): said so, with a wait before another top-up.
    let same = Executor::gas_line(EvmNetwork::base(), before, before, Decimal::ONE);
    assert!(!same.contains("Topped up") && same.contains("hasn't shown up yet") && same.contains("before topping up again"));
    // Less than before: the swap's own fees here took more than it brought, and another swap would too.
    let less = Executor::gas_line(EvmNetwork::ethereum(), before, 390_000_000_000_000, Decimal::ONE);
    assert!(!less.contains("Topped up") && less.contains("0.00042 → 0.00039 ETH") && less.contains("fees") && less.contains("Solana"));
}

/// Solana short: the first network holding the amount and a dime for Relay; Ethereum last (its gas costs dollars), never out.
#[test]
fn trade_funding_picks_a_network_that_holds_it() {
    let h: HashMap<&str, Decimal> = [("base", d("3000")), ("arbitrum", d("60")), ("ethereum", d("9000"))].into();
    let key = |send: &str, m: &HashMap<&str, Decimal>| EvmNetwork::funding(d(send), held(m)).map(|n| n.key);
    assert_eq!(key("50", &h), Some("base"));
    assert_eq!(key("3000", &h), Some("ethereum"));
    assert_eq!(key("10000", &h), None);
    assert_eq!(key("59", &[("arbitrum", d("60")), ("ethereum", d("9000"))].into()), Some("arbitrum"));
    // Ethereum is asked last, never left out: a big balance there pays when nothing cheaper holds enough.
    assert_eq!(key("100", &[("ethereum", d("9000"))].into()), Some("ethereum"));
    assert_eq!(key("100", &[("ethereum", d("9000")), ("tempo", d("500"))].into()), Some("tempo"));
}

/// One rule for where a move's USDC comes from: the card and ensure_evm both read EvmNetwork::source.
#[test]
fn usdc_comes_from_solana_when_it_has_it_else_straight_from_the_network_that_does() {
    let h: HashMap<&str, Decimal> = [("base", d("300")), ("ethereum", d("5000")), ("robinhood", d("400"))].into();
    assert!(EvmNetwork::source(d("249.14"), d("260"), "robinhood", held(&h)).is_none());
    let base = EvmNetwork::source(d("249.14"), d("9.01"), "robinhood", held(&h)).unwrap();
    assert!(base.0.key == "base" && base.1 == d("250.14"));
    // Ethereum last, the target never pays itself, and nobody holding enough leaves it to Solana's own refusal.
    assert_eq!(EvmNetwork::source(d("1000"), Decimal::ZERO, "robinhood", held(&h)).unwrap().0.key, "ethereum");
    let other: HashMap<&str, Decimal> = [("robinhood", d("400")), ("base", d("9000"))].into();
    assert_eq!(EvmNetwork::source(d("350"), Decimal::ZERO, "base", held(&other)).unwrap().0.key, "robinhood");
    assert!(EvmNetwork::source(d("9000"), Decimal::ZERO, "base", held(&h)).is_none());
}

/// BNB Chain's USDC has 18 decimals: the app's millionths cross to its own units and back, and every other network's
/// USDC stays as it was. 20 USDC there is past 64 bits: the transfer word holds it whole.
#[test]
fn bnb_usdc_counts_in_its_own_decimals() {
    assert_eq!(EvmNetwork::bnb().raw(1_000_000), U256::from(1_000_000_000_000_000_000u64));
    assert_eq!(EvmNetwork::bnb().micro("2500000000000000123".parse().unwrap()), 2_500_000);
    assert!(EvmNetwork::base().raw(1_000_000) == U256::from(1_000_000) && EvmNetwork::base().micro(U256::from(1_000_000)) == 1_000_000);
    let me = "0x3535353535353535353535353535353535353535";
    let twenty = evm::erc20::transfer(me, EvmNetwork::bnb().raw(20_000_000)).unwrap();
    assert!(Call::decode(&twenty).summary(56, evm::BNB_USDC, 0).starts_with("send 20 USDC to"));
}

/// Batching waits for the user's switch (Settings › Wallet, off by default): off, approve and call go as before.
#[test]
fn batching_is_the_users_choice_and_off_by_default() {
    let e = Executor::new(TradeWallet { solana: String::new(), evm: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into() },
                          std::sync::Arc::new(ZeroSigner { batches: true, ..Default::default() }), journal("switch"), |_| {});
    assert!(!Executor::batches_on());
    assert!(!e.can_batch(&EvmNetwork::base().rpc));
    BATCHES_ON.store(true, Ordering::Relaxed);
    let (base, robinhood, tempo) = (e.can_batch(&EvmNetwork::base().rpc), e.can_batch(&EvmNetwork::robinhood().rpc), e.can_batch(&EvmNetwork::tempo().rpc));
    BATCHES_ON.store(false, Ordering::Relaxed);
    assert!(base && robinhood && !tempo);
}

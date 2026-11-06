//! GasWaysTests (Engine.stuck, as the Mac's Workspace.stuck) and Rail13Tests' gas offer reading Hyperliquid's perps money.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use rust_decimal::Decimal;

use super::StuckDollars;
use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::place_facts::HlFacts;
use crate::portfolio::{Network, PortfolioView, Row};
use crate::signer_link::SignerLink;
use crate::store;

/// Where dollars landed without the coin fees are paid in, and what the chat offers there.
fn stuck(solana: Decimal, sol: Decimal, hyperliquid: Decimal, held: &[(&str, Decimal, Decimal)]) -> Vec<StuckDollars> {
    let held: HashMap<&str, (Decimal, Decimal)> = held.iter().map(|(k, u, g)| (*k, (*u, *g))).collect();
    Engine::stuck(solana, sol, hyperliquid, |en, _| en.into(), |n| held.get(n.key).copied().unwrap_or_default())
}

fn commands(s: &StuckDollars) -> Vec<&str> { s.ways.iter().map(|w| w.command.as_str()).collect() }
fn keys(s: &[StuckDollars]) -> Vec<&str> { s.iter().map(|s| s.key.as_str()).collect() }

#[test]
fn solana_dollars_without_sol_get_jupiters_swap() {
    let ten = stuck(10.into(), Decimal::ZERO, Decimal::ZERO, &[]);
    assert_eq!(keys(&ten), ["solana"]);
    assert_eq!(commands(&ten[0]), ["buy SOL for 5 USDC"]);
    assert_eq!(ten[0].ways[0].title, "Swap 5 USDC for SOL");
    // Under Jupiter's $5: only adding SOL works, and that's what the chat asks.
    assert!(stuck(3.into(), Decimal::ZERO, Decimal::ZERO, &[])[0].ways.is_empty());
    // With SOL for fees nothing is stuck.
    assert!(stuck(10.into(), Decimal::new(1, 2), Decimal::ZERO, &[]).is_empty());
}

#[test]
fn evm_dollars_without_gas_get_their_way_out() {
    let none = Decimal::ZERO;
    // Base pays by signature: to Solana, or its gas brought back over Solana.
    let base = stuck(none, none, none, &[("base", 5.into(), none)]);
    assert_eq!(keys(&base), ["base"]);
    assert_eq!(commands(&base[0]), ["move 5 USDC from base to Solana", "add $1.00 of gas on base"]);
    assert_eq!(base[0].ways[0].comment, "By signature: Relay pays the gas on Base");
    // BNB Chain's dollar can't pay by signature, but BNB Chain pays the gas for its transfer (MegaFuel): the same two ways.
    let bnb = stuck(none, none, none, &[("bnb", 5.into(), none)]);
    assert!(keys(&bnb) == ["bnb"] && bnb[0].coin == "BNB");
    assert_eq!(commands(&bnb[0]), ["move 5 USDC from bnb to Solana", "add $1.00 of gas on bnb"]);
    // HyperEVM has neither: the chat asks for HYPE.
    let hyperevm = stuck(none, none, none, &[("hyperevm", 5.into(), none)]);
    assert!(keys(&hyperevm) == ["hyperevm"] && hyperevm[0].ways.is_empty());
    // Hyperliquid's balance or Solana's USDC and SOL bring the gas by themselves: nothing to offer.
    assert!(stuck(none, none, 10.into(), &[("bnb", 5.into(), none)]).is_empty());
    assert!(stuck(5.into(), Decimal::new(1, 2), none, &[("bnb", 5.into(), none), ("base", 5.into(), none)]).is_empty());
    // Gas there already, or under a dollar there: nothing either.
    assert!(stuck(none, none, none, &[("bnb", 5.into(), Decimal::new(1, 3)), ("base", Decimal::new(5, 1), none)]).is_empty());
}

fn usdc(amount: i64) -> Row { Row { usd: Some(amount.into()), ..Row::new("USDC", &amount.to_string(), amount.into()) } }

fn network(key: &str, rows: Vec<Row>) -> Network {
    Network { key: key.into(), name: key.into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 }
}

#[tokio::test]
async fn hyperliquids_perps_money_brings_the_gas() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e: Rc<Engine> = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("L1's tests draw no state"));
        e.st_mut().portfolio = PortfolioView { networks: vec![network("base", vec![usdc(5)]), network("hyperliquid", vec![usdc(10)])], ..Default::default() };
        // Spot USDC alone brings no gas (the Mac reads perps withdrawable): Base's dollars are stuck.
        assert_eq!(keys(&e.stuck_dollars()), ["base"]);
        e.st_mut().place_facts.hyperliquid = Some(HlFacts {
            account_value: 10.into(), withdrawable: 10.into(), spot_usdc: Decimal::ZERO, dexes: vec![], hlp_equity: Decimal::ZERO, hlp_locked_until: None,
        });
        assert!(e.stuck_dollars().is_empty());
    }).await;
}

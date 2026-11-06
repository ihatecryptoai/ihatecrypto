//! Contract calls, offline: a balance change as the card says it, the node that simulates, and the refusals said before
//! anything is checked on a network or signed.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use evm::simulation::{NATIVE, Simulation};
use ihatecrypto_core::position_command::{ContractCall, WalletCommand};
use rust_decimal::Decimal;
use signer_api::SignerReply;
use trade::EvmNetwork;
use trade::contract_calls::{ContractCallGuard, ContractTrust};

use super::{CallReview, approve_units, change_line, risk_usd};
use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::portfolio::Network;
use crate::signer_link::SignerLink;
use crate::store;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("drawn while locked"));
    e.st_mut().lock.locked = true;
    e
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

#[test]
fn a_change_is_signed_and_the_simulator_is_the_one_that_answers() {
    assert_eq!(Engine::delta(Decimal::new(15, 1)), "+1.5");
    assert_eq!(Engine::delta(Decimal::new(-25, 2)), "−0.25");
    assert_eq!(Engine::delta(Decimal::ZERO), "0");
    assert_eq!(Engine::delta(Decimal::new(123_456_789, 9)), "+0.12345679");
    // Arbitrum's own node crashes on eth_simulateV1.
    assert_eq!(Engine::simulator(EvmNetwork::arbitrum()).url, "https://arbitrum-one-rpc.publicnode.com");
    assert_eq!(Engine::simulator(EvmNetwork::base()).url, EvmNetwork::base().rpc.url);
}

#[tokio::test]
async fn a_call_that_can_t_be_encoded_is_refused_before_any_check() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let call = |function: &str| WalletCommand::ContractCall(ContractCall {
            network: "base".into(), contract: "0x1a996cb54bb95462040408c06122d45d6cdb6096".into(), function: function.into(), args: vec!["10".into()], value: None,
            approve: None,
        });
        e.prepare_contract_call(&call("deposit(uint256)"), None).await;
        assert_eq!(last(&e), "Contract calls need this wallet's EVM key in secure storage.");
        e.st_mut().status = Some(SignerReply { evm_address: Some("0x9858EfFD232B4033E47d90003D41EC34EcaEda94".into()), has_wallet: Some(true), ..Default::default() });
        e.prepare_contract_call(&call("deposit"), None).await;
        assert_eq!(last(&e), "I won't make this call: «deposit» isn't a function like deposit(uint256,address). Nothing was signed.");
        e.prepare_contract_call(&call("deposit(uint256,address)"), None).await;
        assert_eq!(last(&e), "I won't make this call: deposit takes 2 arguments, 1 given. Nothing was signed.");
        // A network ihatecrypto doesn't move money on: nothing at all.
        let before = e.st().messages().len();
        e.prepare_contract_call(&WalletCommand::ContractCall(ContractCall { network: "fantom".into(), ..match call("x()") { WalletCommand::ContractCall(c) => c, _ => unreachable!() } }), None).await;
        assert_eq!(e.st().messages().len(), before);
        // Confirm without a checked call: nothing is simulated or signed.
        assert!(e.st().call_review.is_none());
    }).await;
}

#[tokio::test]
async fn no_owner_or_no_explorer_adds_nothing_to_the_balances() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        e.set_app_costs(|c| { c.insert("solana:So11111111111111111111111111111111111111112".into(), Default::default()); c.insert("base".into(), Default::default()); });
        let networks = vec![Network { key: "base".into(), name: "Base".into(), usd: Decimal::ONE, rows: vec![], note: None, hidden: vec![], hidden_more: 0 }];
        assert_eq!(e.with_missing_deposits(networks.clone(), "").await, networks);
        assert_eq!(e.with_missing_deposits(networks.clone(), "0x9858EfFD232B4033E47d90003D41EC34EcaEda94").await, networks);
    }).await;
}

const TOKEN: &str = "0x1111111111111111111111111111111111111111";
const SHARES: &str = "0x2222222222222222222222222222222222222222";
const ODD: &str = "0x3333333333333333333333333333333333333333";

#[test]
fn an_exact_approve_of_a_token_without_decimals_is_refused() {
    let (en, ru) = approve_units(TOKEN, "FOO", "Base", Decimal::TEN, None).unwrap_err();
    assert_eq!(en, format!("FOO ({TOKEN}) on Base doesn't answer decimals(), so ihatecrypto can't count an exact approve."));
    assert_eq!(ru, format!("FOO ({TOKEN}) в Base не отвечает на decimals(), а без этого ihatecrypto не посчитает точный approve."));
    assert_eq!(approve_units(TOKEN, "USDC", "Base", Decimal::new(105, 1), Some(6)), Ok(Decimal::from(10_500_000)));
}

#[test]
fn the_cap_refuses_an_unpriced_approve_and_a_token_leaving_without_decimals() {
    let priced = HashMap::from([(TOKEN.to_string(), Decimal::from(2))]);
    // An approve nothing prices is a spend that hasn't happened yet, never $0.
    let (en, ru) = risk_usd(ContractCallGuard::at_risk(&[], Some((SHARES, "FOO", Decimal::TEN)), &priced)).unwrap_err();
    assert_eq!(en, "ihatecrypto can't price the FOO this call would spend or let the contract take, so it can't keep the call under its cap.");
    assert_eq!(ru, "ihatecrypto не может оценить FOO, который вызов потратит или разрешит контракту взять, а значит не может удержать вызов в лимите.");
    // A priced token leaving whose decimals() didn't answer has no amount to count.
    let (en, ru) = risk_usd(ContractCallGuard::at_risk(&[(TOKEN.into(), "BAR".into(), None)], None, &priced)).unwrap_err();
    assert_eq!(en, "ihatecrypto can't read the decimals() of the BAR that would leave, so it can't keep the call under its cap.");
    assert_eq!(ru, "у уходящего BAR не читается decimals(), а значит ihatecrypto не может удержать вызов в лимите.");
    // Counted: what leaves, or the approve when that's more.
    assert_eq!(risk_usd(ContractCallGuard::at_risk(&[(TOKEN.into(), "BAR".into(), Some(Decimal::ONE))], Some((TOKEN, "BAR", Decimal::from(3))), &priced)), Ok(Decimal::from(6)));
}

#[test]
fn a_balance_change_of_a_token_without_decimals_is_refused_and_an_nft_keeps_its_count() {
    assert_eq!(change_line(TOKEN, "USDC", -1_500_000, Some(6), false), Ok("USDC: −1.5".into()));
    assert_eq!(change_line(TOKEN, "PUNK", 2, None, true), Ok("PUNK: +2".into()));
    let (en, ru) = change_line(TOKEN, "FOO", 5, None, false).unwrap_err();
    assert_eq!(en, format!("FOO ({TOKEN}) doesn't answer decimals(), so the card couldn't show how much of it moves."));
    assert_eq!(ru, format!("FOO ({TOKEN}) не отвечает на decimals(), поэтому карточка не может показать, сколько его уходит или приходит."));
}

#[tokio::test]
async fn the_booking_after_the_send_uses_the_tokens_the_card_read() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let call = ContractCall { network: "base".into(), contract: SHARES.into(), function: "deposit(uint256)".into(), args: vec!["5000000".into()], value: None, approve: None };
        let simulation = Simulation {
            changes: BTreeMap::from([(TOKEN.into(), -5_000_000), (SHARES.into(), 2_000_000_000_000_000_000), (ODD.into(), 7), (NATIVE.into(), -1_000)]),
            ..Default::default()
        };
        // Booked from the card's reads, nothing read again: ODD without decimals isn't booked, so the one kind of share gets the dollars.
        let tokens = HashMap::from([(TOKEN.into(), ("USDC".into(), Some(6))), (SHARES.into(), ("fUSDC".into(), Some(18))), (ODD.into(), ("ODD".into(), None))]);
        let review = CallReview {
            command: WalletCommand::ContractCall(call.clone()), call, net: EvmNetwork::base().clone(), data: vec![], value: 0, approve: None, lines: vec![],
            changes: vec![], trust: ContractTrust::default(), warnings: vec![], calls: vec![], simulation, badges: vec![], tokens,
        };
        e.record_call(&review);
        let costs = e.st().app_costs.clone();
        let shares = &costs[&format!("base:{SHARES}")];
        assert_eq!((shares.amount, shares.usd), (Decimal::from(2), Decimal::from(5)));
        assert!(!costs.contains_key(&format!("base:{ODD}")) && !costs.contains_key(&format!("base:{TOKEN}")));
    }).await;
}

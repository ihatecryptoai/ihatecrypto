//! ContractGuardTests (the rights a call hands out, the card's drift, the cap's fail-closed pricing, GoPlus Security's
//! answers), UnknownDecimalsTests' GoPlus cache, and Batch7702Tests' call with coins on Tempo.

use std::collections::HashMap;
use std::sync::Arc;

use evm::simulation::{Grant, Simulation};
use execution::trade_signer::TradeWallet;
use net::evm_rpc::EvmRpc;
use serde_json::{Value, json};
use wallet::evm::keccak;
use wallet::to_hex;

use super::d;
use super::fake_hl::{ZeroSigner, journal};
use super::g4_fake::serve;
use crate::contract_calls::{ContractCallGuard, Risk, ScamList, Verdict};
use crate::{Error, EvmNetwork, Executor};

const ME: &str = "0x1111111111111111111111111111111111111111";
const VAULT: &str = "0x2222222222222222222222222222222222222222";
const EVIL: &str = "0x3333333333333333333333333333333333333333";
const USDC: &str = "0xaf88d065e77c8cc2239327c5edb3a432268e5831";

fn topic(a: &str) -> String { format!("0x{}{}", "0".repeat(24), &a[2..]) }
fn event(s: &str) -> String { format!("0x{}", to_hex(&keccak(s.as_bytes()))) }
fn word(n: u64) -> String { format!("0x{}{n:016x}", "0".repeat(48)) }
fn simulated(logs: Vec<Value>) -> Simulation { Simulation::parse(&json!([{"calls": [{"status": "0x1", "logs": logs}]}]), ME).unwrap() }
fn log(address: &str, topics: [&str; 3], data: &str) -> Value { json!({"address": address, "topics": topics, "data": data}) }

#[test]
fn simulations_show_the_rights_a_wallet_hands_out() {
    let (approval, all) = (event("Approval(address,address,uint256)"), event("ApprovalForAll(address,address,bool)"));
    let exact = simulated(vec![log(USDC, [&approval, &topic(ME), &topic(VAULT)], &word(10_000_000))]);
    assert_eq!(exact.grants, [Grant { kind: "approve".into(), asset: USDC.into(), to: VAULT.into(), unlimited: false }]);
    assert_eq!(ContractCallGuard::grant_problem(&exact.grants, VAULT), None);
    // To someone else, unlimited, over every NFT: refused. A revoke (zero) and someone else's approval give nothing.
    let other = simulated(vec![log(USDC, [&approval, &topic(ME), &topic(EVIL)], &word(1))]);
    assert!(ContractCallGuard::grant_problem(&other.grants, VAULT).is_some());
    let unlimited = simulated(vec![log(USDC, [&approval, &topic(ME), &topic(VAULT)], &format!("0x{}", "f".repeat(64)))]);
    assert!(unlimited.grants.first().is_some_and(|g| g.unlimited) && ContractCallGuard::grant_problem(&unlimited.grants, VAULT).is_some());
    let nfts = simulated(vec![log(EVIL, [&all, &topic(ME), &topic(VAULT)], &word(1))]);
    assert!(ContractCallGuard::grant_problem(&nfts.grants, VAULT).is_some());
    let none = simulated(vec![log(USDC, [&approval, &topic(ME), &topic(EVIL)], &word(0)), log(USDC, [&approval, &topic(EVIL), &topic(ME)], &word(5))]);
    assert!(none.grants.is_empty());
}

#[test]
fn what_is_signed_must_still_do_what_the_card_showed() {
    let (mut card, mut now) = (simulated(vec![]), simulated(vec![]));
    card.changes = [(USDC.to_string(), -10_000_000), (VAULT.to_string(), 8_826_380)].into();
    now.changes = [(USDC.to_string(), -10_000_000), (VAULT.to_string(), 8_820_000)].into();
    assert_eq!(ContractCallGuard::drift(&card, &now), None);
    now.changes.insert(USDC.into(), -11_000_000);
    assert!(ContractCallGuard::drift(&card, &now).is_some());
    now.changes = [(USDC.to_string(), -10_000_000), (VAULT.to_string(), 8_826_380), (EVIL.to_string(), -1)].into();
    assert!(ContractCallGuard::drift(&card, &now).is_some());
    now.changes = card.changes.clone();
    now.grants = vec![Grant { kind: "approve".into(), asset: USDC.into(), to: EVIL.into(), unlimited: false }];
    assert!(ContractCallGuard::drift(&card, &now).is_some());
}

/// The cap for a protocol outside the list: an approve it can't price is refused like an outflow, never counted as $0 —
/// the contract may take the part of it this call leaves unspent later.
#[test]
fn an_approve_it_cant_price_is_refused_like_an_outflow() {
    let priced: HashMap<String, rust_decimal::Decimal> = [(USDC.to_string(), d("1")), (VAULT.to_string(), d("1"))].into();
    let out = |token: &str, symbol: &str, amount: Option<&str>| (token.to_string(), symbol.to_string(), amount.map(d));
    let unpriced = |s: &str| Risk::Unpriced(s.into());
    assert_eq!(ContractCallGuard::at_risk(&[], Some((EVIL, "EVIL", d("1000000"))), &priced), unpriced("EVIL"));
    assert_eq!(ContractCallGuard::at_risk(&[], Some((EVIL, "EVIL", d("0"))), &priced), Risk::Usd(d("0")));   // a revoke
    assert_eq!(ContractCallGuard::at_risk(&[out(USDC, "USDC", Some("10"))], Some((VAULT, "VLT", d("40"))), &priced), Risk::Usd(d("40")));
    assert_eq!(ContractCallGuard::at_risk(&[out(USDC, "USDC", Some("100"))], Some((VAULT, "VLT", d("1"))), &priced), Risk::Usd(d("100")));
    assert_eq!(ContractCallGuard::at_risk(&[out(USDC, "USDC", Some("10")), out(VAULT, "VLT", Some("5"))], None, &priced), Risk::Usd(d("15")));
    assert_eq!(ContractCallGuard::at_risk(&[], Some((EVIL, "usdc", d("30"))), &priced), Risk::Usd(d("30")));   // the dollar list
    assert_eq!(ContractCallGuard::at_risk(&[out(EVIL, "EVIL", Some("1"))], None, &priced), unpriced("EVIL"));
    // An outflow without decimals: priced, it can't be counted; unpriced (an NFT), the price is asked first.
    assert_eq!(ContractCallGuard::at_risk(&[out(USDC, "USDC", None)], None, &priced), Risk::Uncounted("USDC".into()));
    assert_eq!(ContractCallGuard::at_risk(&[out(EVIL, "EVIL", None)], None, &priced), unpriced("EVIL"));
}

/// GoPlus Security's address_security answer, read as ScamList's verdict. Its rate limit comes as HTTP 200 with code
/// 4029: never read as clean. A clean contract shows only contract_address=1.
#[test]
fn go_plus_answers_read_as_a_verdict() {
    let answer = |fields: &[(&str, &str)], code: i64| -> Vec<u8> {
        let mut result: serde_json::Map<String, Value> = ["cybercrime", "money_laundering", "number_of_malicious_contracts_created", "phishing_activities",
            "contract_address", "blacklist_doubt", "stealing_attack", "sanctioned", "mixer", "honeypot_related_address", "data_source"]
            .iter().map(|k| (k.to_string(), json!(if *k == "data_source" { "" } else { "0" }))).collect();
        for (k, v) in fields { result.insert(k.to_string(), json!(v)); }
        json!({"code": code, "message": "ok", "result": result}).to_string().into_bytes()
    };
    let Verdict::Reported(theft) = ScamList::verdict(&answer(&[("stealing_attack", "1"), ("data_source", "SlowMist")], 1)) else { panic!("theft") };
    assert!(theft.en.contains("theft") && theft.en.contains("SlowMist"), "{}", theft.en);
    assert!(theft.ru.contains("кража") && theft.ru.contains("SlowMist"), "{}", theft.ru);
    assert!(!theft.en.contains(';') && !theft.ru.contains(';'));
    let Verdict::Reported(both) = ScamList::verdict(&answer(&[("phishing_activities", "1"), ("blacklist_doubt", "1"), ("data_source", "SlowMist,ScamSniffer")], 1))
    else { panic!("phishing") };
    assert!(both.en.contains("phishing") && both.en.contains("ScamSniffer") && both.ru.contains("фишинг"), "{both:?}");
    assert_eq!(ScamList::verdict(&answer(&[("contract_address", "1")], 1)), Verdict::Clean);
    assert_eq!(ScamList::verdict(&answer(&[], 1)), Verdict::Clean);
    assert!(matches!(ScamList::verdict(&answer(&[("number_of_malicious_contracts_created", "2")], 1)), Verdict::Reported(_)));
    assert_eq!(ScamList::verdict(br#"{"code":4029,"message":"too many requests"}"#), Verdict::Unchecked);
    assert_eq!(ScamList::verdict(&answer(&[("stealing_attack", "1")], 4029)), Verdict::Unchecked);
    assert_eq!(ScamList::verdict(br#"{"code":1,"message":"ok","result":{}}"#), Verdict::Unchecked);
    assert_eq!(ScamList::verdict(b"<html>bad gateway</html>"), Verdict::Unchecked);
    // GoPlus's networks are asked as they are; one it doesn't serve (HyperEVM, Tempo) answers zeros even for a drainer,
    // so Ethereum's labels are asked: they're per address.
    assert!(ScamList::chain(8453) == 8453 && ScamList::chain(42161) == 42161 && ScamList::chain(56) == 56);
    assert!(ScamList::chain(999) == 1 && ScamList::chain(4217) == 1);
}

/// An answer is kept an hour, a silence isn't kept at all; a network GoPlus doesn't serve asks Ethereum's labels.
#[tokio::test]
async fn go_plus_verdicts_are_kept_but_a_silence_is_asked_again() {
    const DRAINER: &str = "0x012f89ff307568f8409bbe379ca92ed41bc4d386";
    const SILENT: &str = "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc";
    let (url, asked) = serve(|path, _| {
        let address = path.split('?').next().unwrap_or("").rsplit('/').next().unwrap_or("").to_lowercase();
        let flags = if address == DRAINER { r#""stealing_attack":"1","data_source":"SlowMist""# } else { r#""stealing_attack":"0","data_source":"""# };
        Some(if address == SILENT { r#"{"code":4029,"message":"too many requests"}"#.into() }
             else { format!(r#"{{"code":1,"message":"ok","result":{{"phishing_activities":"0","contract_address":"0",{flags}}}}}"#) })
    }).await;
    let list = ScamList::at(&url);
    let count = || asked.lock().unwrap().len();
    assert_ne!(list.check(DRAINER, 8453).await, Verdict::Clean);
    assert_ne!(list.check(&DRAINER.to_uppercase().replace("0X", "0x"), 8453).await, Verdict::Clean);
    assert_eq!(count(), 1);
    assert_eq!(list.check(SILENT, 8453).await, Verdict::Unchecked);
    assert_eq!(list.check(SILENT, 8453).await, Verdict::Unchecked);
    assert_eq!(count(), 3);
    assert_eq!(list.check("0x70997970C51812dc3A010C7d01b50e0d17dc79C8", 999).await, Verdict::Clean);
    assert!(asked.lock().unwrap().last().unwrap().0.ends_with("chain_id=1"));
    assert_eq!(list.check("not an address", 8453).await, Verdict::Unchecked);
    assert_eq!(count(), 4);
    // Asked at once, keyed lowercased, each address once.
    let all = list.verdicts([DRAINER.to_uppercase().replace("0X", "0x"), SILENT.into(), DRAINER.into()], 8453).await;
    assert!(all.len() == 2 && matches!(all[DRAINER], Verdict::Reported(_)) && all[SILENT] == Verdict::Unchecked);
}

/// Coins with an approve on a network that pays its fees in a token (Tempo): refused before the approve, nothing
/// signed. Its node is a dead port: an approve that started would fail on reading the allowance, with another message.
#[tokio::test]
async fn contract_call_with_coins_on_tempo_stops_before_the_approve() {
    let e = Executor::new(TradeWallet { solana: String::new(), evm: ME.into() }, Arc::new(ZeroSigner::default()), journal("tempo"), |_| {});
    let dead = EvmNetwork { rpc: EvmRpc::pinned("http://127.0.0.1:9", EvmNetwork::tempo().rpc.chain_id), ..EvmNetwork::tempo().clone() };
    let refused = e.contract_call(&dead, "0x1111111111111111111111111111111111111111", &[1, 2, 3, 4], 1, Some((evm::TEMPO_USDC, d("1"))), "Approve", "Call").await;
    assert!(matches!(&refused, Err(Error::Execution(m)) if m.contains("Coins go only with a single call")), "{refused:?}");
}

//! Payments, offline: what's refused before a service is ever called, and the USDC on Tempo the card counts.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine as _;
use chrono::{TimeDelta, Utc};
use ihatecrypto_core::mpp::NearCharge;
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::scheduled_task::{Every, ScheduledTask};
use rust_decimal::Decimal;
use signer_api::{SignerReply, SignerWallet};

use crate::automation::{RUN_TASK, TASK_RUN};
use crate::engine::{Engine, Pending};
use crate::from_core::MppChallenge;
use crate::outbox::Outbox;
use crate::payments::PaymentQuote;
use crate::portfolio::{Network, Row};
use crate::signer_link::SignerLink;
use crate::store;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("drawn while locked"));
    e.st_mut().lock.locked = true;
    e
}

fn pay(url: &str) -> WalletCommand { WalletCommand::Pay { method: "GET".into(), url: url.into(), body: None } }

#[tokio::test]
async fn a_service_that_refuses_its_proof_or_isn_t_a_web_address_is_never_called() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let refused = |r: crate::payments::PriceAsked| panic!("asked: {r:?}");
        let said = e.ask_price(&WalletCommand::ClaimRewards).await.map(refused).unwrap_err().to_string();
        assert_eq!(said, "That isn't a web address ihatecrypto can call.");
        assert_eq!(e.ask_price(&pay("not a url")).await.map(refused).unwrap_err().to_string(), "That isn't a web address ihatecrypto can call.");
        assert_eq!(e.ask_price(&pay("https://API.aisa.one/v1/search?q=x")).await.map(refused).unwrap_err().to_string(),
                   // ponytail: the host as WHATWG reads it, lowercased; Foundation's URL keeps its case for the words.
                   "api.aisa.one takes the payment and then refuses its proof (checked 2026-10-23), so ihatecrypto doesn't pay it. Ask for another service.");
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(e.ask_price(&pay("https://stableenrich.dev/x")).await.map(refused).unwrap_err().to_string(),
                   "stableenrich.dev берёт оплату, а потом не принимает её подтверждение (проверено 2026-10-23), поэтому ihatecrypto его не оплачивает. Попроси другой сервис.");
        // Confirm with no price shown and no service to ask: nothing is paid.
        assert!(e.st().payment.is_none());
    }).await;
}

#[tokio::test]
async fn tempo_s_usdc_is_read_from_the_balances() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        assert_eq!(e.tempo_usdc(), Decimal::ZERO);
        let network = |key: &str, rows: Vec<Row>| Network { key: key.into(), name: key.into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 };
        e.st_mut().portfolio.networks = vec![network("base", vec![Row::new("USDC", "9", Decimal::from(9))]),
                                             network("tempo", vec![Row::new("pathUSD", "1", Decimal::ONE), Row::new("USDC", "2.5", Decimal::new(25, 1))])];
        assert_eq!(e.tempo_usdc(), Decimal::new(25, 1));
    }).await;
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

/// Example's price for `pay`, asked in a run of `task` (None: in the window).
fn quote(command: &WalletCommand, amount: u64, task: Option<&ScheduledTask>) -> PaymentQuote {
    // Hardhat's account #1, public test data.
    let challenge = MppChallenge { params: HashMap::new(), amount, payee: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".into(), memo: None, expires: None, method: "tempo".into(), near: None };
    PaymentQuote { command: command.clone(), challenge, x402: None, ask: String::new(), service: "Example".into(), probed: false, task: task.map(|t| t.id), receives: None }
}

fn clear_card(e: &Engine) {
    let mut st = e.st_mut();
    st.pending = None;
    st.card = None;
    st.payment = None;
    st.chain_state.after_call = None;
}

/// A task's run on the automation wallet pays a service by itself only up to the price Confirmed for that request in the
/// task: above it, or outside such a run, its own card shows the price and waits, the steps after it held behind it.
#[tokio::test]
async fn a_task_s_run_pays_by_itself_only_up_to_the_price_confirmed_for_it() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        let pay = pay("https://api.example.com/answer");
        let mut task = ScheduledTask::new(vec!["pay GET https://api.example.com/answer".into(), "chart SOL".into()], Utc::now() + TimeDelta::hours(1), Some(Every::Days(1)), None);
        task.approve(60_000, &pay);
        let saved = task.clone();
        e.set_tasks(|t| t.push(saved));
        let rest = ["chart SOL".to_string()];
        // In the window, not a run: no task, so its card waits even at the price Confirmed.
        assert!(e.automation_task(&pay).is_none());
        assert!(!e.pays_by_itself(quote(&pay, 60_000, None), &rest));
        assert!(matches!(&e.st().pending, Some(Pending::Wallet(c)) if *c == pay) && e.st().card.is_some());
        assert_eq!(e.st().chain_state.after_call.as_ref().map(|a| a.rest.clone()), Some(rest.to_vec()));
        clear_card(&e);
        RUN_TASK.scope(Some(task.id), async {
            assert_eq!(e.automation_task(&pay).map(|t| t.id), Some(task.id));
            // Another request than the task's steps: never this task's.
            assert!(e.automation_task(&self::pay("https://api.example.com/other")).is_none());
            // More than Confirmed: its card, saying what its Confirm lets later runs pay.
            assert!(!e.pays_by_itself(quote(&pay, 60_001, Some(&task)), &rest));
            assert!(e.st().card.is_some() && e.st().payment.as_ref().map(|q| q.challenge.amount) == Some(60_001));
            let said = e.st().messages().iter().rev().nth(1).map(|m| m.text.clone()).unwrap_or_default();
            assert_eq!(said, "Example asks 0.060001 USDC. Press Confirm to pay it in USDC on Tempo, or Cancel. After that, this task pays up to 0.060001 USDC for this request by itself. A higher price waits for your Confirm again.");
            clear_card(&e);
            // Up to it: no card, and pay() takes the price.
            assert!(e.pays_by_itself(quote(&pay, 60_000, Some(&task)), &rest));
            assert!(e.st().card.is_none() && e.st().chain_state.after_call.is_none() && e.st().payment.as_ref().map(|q| q.challenge.amount) == Some(60_000));
            assert_eq!(last(&e), "Example asks 0.06 USDC — no more than you Confirmed for this task, so it goes without a card, from the automation wallet.");
        }).await;
    }).await;
}

/// pay() pays only a price shown: with none it asks no service, and the one shown goes to the payment as it is.
#[tokio::test]
async fn pay_pays_only_a_price_shown() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        // Hardhat's account #0.
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into()), ..Default::default() });
        let executor = e.executor().unwrap();
        let pay = pay("https://api.example.com/answer");
        let said = |r: trade::Result<String>| r.map_or_else(|e| e.to_string(), |_| "paid".into());
        assert_eq!(said(e.pay(&pay, &executor).await), "The service's price is gone. Ask for it again.");
        // Another request's price is no price for this one.
        e.st_mut().payment = Some(quote(&self::pay("https://api.example.com/other"), 10_000, None));
        assert_eq!(said(e.pay(&pay, &executor).await), "The service's price is gone. Ask for it again.");
        // The price shown is what's paid: one the fee can't ride on in 64 bits is refused before any call.
        e.st_mut().payment = Some(quote(&pay, u64::MAX, None));
        assert_eq!(said(e.pay(&pay, &executor).await), "The service asks more USDC than exists. Nothing was paid.");
    }).await;
}

// MARK: NEAR Intents and x402 (NearChargeTests, X402Tests and Payments.swift's way of reading a 402)

/// Base's USDC as CAIP-19, the 1Click deposit and the refund address (Hardhat's accounts #1 and #0), as NearChargeTests
/// has them.
const BASE_USDC: &str = "eip155:8453/erc20:0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
const DEPOSIT: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const REFUND: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const NEAR_USDC: &str = "near:mainnet/nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";

fn near_request(currency: &str, origin: &str) -> String {
    format!(r#"{{"amount":"1010000","currency":"{currency}","recipient":"{DEPOSIT}","methodDetails":{{"originNetwork":"{origin}","destinationNetwork":"near:mainnet","destinationAsset":"{NEAR_USDC}","destinationRecipient":"merchant.near","amountOut":"1000000","minAmountIn":"1005000","refundTo":"{REFUND}","timeEstimate":30}}}}"#)
}

fn mpp_header(id: &str, method: &str, request: &str) -> String {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(request);
    format!(r#"Payment id="{id}", realm="shop.example", method="{method}", intent="charge", request="{b64}", expires="2099-01-01T00:00:00Z""#)
}

fn near_header(currency: &str, origin: &str) -> String { mpp_header("n1", "nearintents", &near_request(currency, origin)) }

fn tempo_header() -> String { mpp_header("t1", "tempo", &format!(r#"{{"amount":"1000","currency":"{}","recipient":"{DEPOSIT}"}}"#, ihatecrypto_core::mpp::USDC)) }

/// The PAYMENT-REQUIRED header api.402rates.com/v1/ping answered on 2026-11-02, its payTo Hardhat's account #1 (X402Tests).
fn x402_header() -> String {
    base64::engine::general_purpose::STANDARD.encode(format!(r#"{{"x402Version":2,"error":"Payment required","resource":{{"url":"https://api.402rates.com/v1/ping","mimeType":"application/json"}},"accepts":[{{"scheme":"exact","network":"eip155:8453","amount":"1000","asset":"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913","payTo":"{DEPOSIT}","maxTimeoutSeconds":300,"extra":{{"name":"USD Coin","version":"2"}}}}]}}"#))
}

fn answered_402(www: &str, x402: bool) -> HashMap<String, String> {
    let mut h = HashMap::from([("www-authenticate".to_string(), www.to_string())]);
    if x402 { h.insert("payment-required".into(), x402_header()); }
    h
}

fn charge(origin: &str, token: &str) -> NearCharge {
    NearCharge { origin_network: origin.into(), currency: format!("{origin}/erc20:{token}"), destination_network: "near:mainnet".into(), destination_asset: NEAR_USDC.into(),
                 destination_recipient: "merchant.near".into(), amount_out: "1000000".into(), min_amount_in: 1_005_000, refund_to: REFUND.into(), time_estimate: None }
}

/// A 402 is read as Tempo first, then a NEAR Intents charge in USDC on a network this wallet pays from, then x402 on Base.
#[test]
fn a_402_is_read_as_tempo_then_near_intents_then_x402() {
    let way = |www: &str, x402: bool| Engine::way_asked(&answered_402(www, x402), Utc::now());
    let near = near_header(BASE_USDC, "eip155:8453");
    assert_eq!(way(&format!("{}, {near}", tempo_header()), true).map(|w| w.0.method), Ok("tempo".into()));
    assert_eq!(way(&format!("{near}, {}", tempo_header()), true).map(|w| w.0.method), Ok("tempo".into()));
    let (c, offer) = way(&near, true).unwrap();
    assert_eq!((c.method.as_str(), c.amount, c.payee.as_str(), c.near.map(|n| n.destination_recipient), offer), ("nearintents", 1_010_000, DEPOSIT, Some("merchant.near".into()), None));
    // USDC on Polygon through NEAR Intents isn't paid here: x402 on Base is, its price and payee on the challenge.
    let polygon = near_header("eip155:137/erc20:0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", "eip155:137");
    let (c, offer) = way(&polygon, true).unwrap();
    assert_eq!((c.method.as_str(), c.amount, c.payee.as_str(), c.near.is_none(), offer.map(|o| (o.amount, o.chain_id))), ("x402", 1000, DEPOSIT, true, Some((1000, 8453))));
    assert_eq!(way("", true).map(|w| w.0.method), Ok("x402".into()));
    // Neither: MPP's own reason.
    assert_eq!(way(&polygon, false).unwrap_err().reason,
               "The service wants eip155:137/erc20:0x3c499c542cef5e3811e1192ce70d8cc03d5c3359 on eip155:137 through NEAR Intents. ihatecrypto pays such a charge in USDC on Solana, Base, Arbitrum or Ethereum.");
    assert_eq!(way("", false).unwrap_err().reason, "The service asked for payment without an MPP challenge.");
}

/// A NEAR Intents charge is paid from Solana, Base, Arbitrum or Ethereum, in their USDC only (the Mac's nearOrigin).
#[test]
fn a_near_charge_is_paid_only_in_usdc_where_1click_takes_it() {
    let sol = NearCharge { currency: format!("{}/token:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", NearCharge::SOLANA_MAINNET),
                           ..charge(NearCharge::SOLANA_MAINNET, "") };
    assert_eq!(Engine::near_origin(&sol), Some("solana"));
    let wsol = NearCharge { currency: format!("{}/token:So11111111111111111111111111111111111111112", NearCharge::SOLANA_MAINNET), ..sol };
    assert_eq!(Engine::near_origin(&wsol), None);
    assert_eq!(Engine::near_origin(&charge("eip155:8453", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")), Some("base"));
    assert_eq!(Engine::near_origin(&charge("eip155:42161", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831")), Some("arbitrum"));
    assert_eq!(Engine::near_origin(&charge("eip155:1", "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")), Some("ethereum"));
    // Another token on Base, Base's USDC named on another chain, a network 1Click takes no USDC from here.
    assert_eq!(Engine::near_origin(&charge("eip155:8453", "0x940181a94A35A4569E4529A3CDfB74e38FD98631")), None);
    assert_eq!(Engine::near_origin(&charge("eip155:42161", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")), None);
    assert_eq!(Engine::near_origin(&charge("eip155:137", "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359")), None);
}

/// What the service receives, as 1Click lists the asset: on NEAR, on an EVM network, with its own decimals.
#[test]
fn what_the_service_receives_is_read_from_1clicks_list() {
    let token = |asset: &str, chain: &str, symbol: &str, decimals: i64, contract: &str| protocols::one_click::Token {
        asset_id: asset.into(), blockchain: chain.into(), symbol: symbol.into(), decimals, contract: Some(contract.into()), price: None };
    let near_usdc = "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1";
    let tokens = [token(near_usdc, "near", "USDC", 6, "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1"),
                  token("nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near", "base", "USDC", 6, "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"),
                  token("nep141:wrap.near", "near", "wNEAR", 24, "wrap.near")];
    let gets = |asset: &str, out: &str| Engine::receives_in(&tokens, &NearCharge { destination_asset: asset.into(), amount_out: out.into(), ..charge("eip155:8453", "") });
    assert_eq!(gets(NEAR_USDC, "1000000"), Some(("1 USDC".into(), near_usdc.into())));
    assert_eq!(gets(BASE_USDC, "2500000"), Some(("2.5 USDC".into(), "nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near".into())));
    assert_eq!(gets("near:mainnet/nep141:wrap.near", "2500000000000000000000000").map(|g| g.0), Some("2.5 wNEAR".into()));
    // Not listed, or not a number: nothing is claimed.
    assert_eq!(gets("eip155:42161/erc20:0xaf88d065e77c8cC2239327C5EDb3A432268e5831", "1000000"), None);
    assert_eq!(gets(NEAR_USDC, "a lot"), None);
}

/// A charge asked again is paid only to the same receiver, refund address and output, for no more.
#[test]
fn a_near_charge_asked_again_must_be_the_same_for_no_more() {
    let near = |amount: u64, n: NearCharge| MppChallenge { params: HashMap::new(), amount, payee: DEPOSIT.into(), memo: None, expires: None, method: "nearintents".into(), near: Some(n) };
    let n = charge("eip155:8453", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
    let shown = near(1_010_000, n.clone());
    assert!(Engine::same_charge(&shown, &near(1_010_000, n.clone())));
    // Its deposit address is single-use: a fresh one for a cheaper price is fine.
    assert!(Engine::same_charge(&shown, &MppChallenge { payee: REFUND.into(), ..near(1_000_000, n.clone()) }));
    assert!(!Engine::same_charge(&shown, &near(1_010_001, n.clone())));
    assert!(!Engine::same_charge(&shown, &near(1_010_000, NearCharge { refund_to: DEPOSIT.into(), ..n.clone() })));
    assert!(!Engine::same_charge(&shown, &near(1_010_000, NearCharge { destination_recipient: "other.near".into(), ..n.clone() })));
    assert!(!Engine::same_charge(&shown, &near(1_010_000, NearCharge { amount_out: "999999".into(), ..n.clone() })));
    assert!(!Engine::same_charge(&shown, &MppChallenge { near: None, method: "tempo".into(), ..shown.clone() }));
}

/// What's brought for a price on Base (or the EVM network a charge is paid on): none when it's there, else what's short
/// plus 1Click's ≈ 1% and a cent, at least a dollar.
#[test]
fn what_comes_to_base_first_is_what_s_short_and_at_least_a_dollar() {
    let d = |s: &str| s.parse::<Decimal>().unwrap();
    assert_eq!(Engine::x402_top_up(d("0.001"), d("5")), None);
    assert_eq!(Engine::x402_top_up(d("5"), d("5")), None);
    assert_eq!(Engine::x402_top_up(d("0.001"), Decimal::ZERO), Some(Decimal::ONE));
    assert_eq!(Engine::x402_top_up(d("5"), d("4.99")), Some(Decimal::ONE));
    assert_eq!(Engine::x402_top_up(d("2"), d("0.5")), Some(d("1.54")));
    assert_eq!(Engine::x402_top_up(d("3"), Decimal::ZERO), Some(d("3.07")));
}

fn priced(command: &WalletCommand, www: &str, x402: bool) -> PaymentQuote {
    let (challenge, offer) = Engine::way_asked(&answered_402(www, x402), Utc::now()).unwrap();
    let receives = challenge.near.as_ref().map(|_| ("1 USDC".to_string(), "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1".to_string()));
    PaymentQuote { command: command.clone(), challenge, x402: offer, ask: String::new(), service: "Example".into(), probed: false, task: None, receives }
}

/// x402 and NEAR Intents charges always wait for their own card, even in a task's run at a price Confirmed for it: their
/// card and the chat say which way it pays, and the move's note too.
#[tokio::test]
async fn x402_and_near_intents_charges_always_wait_for_their_card() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(REFUND.into()), ..Default::default() });
        let pay = pay("https://api.example.com/answer");
        let mut task = ScheduledTask::new(vec!["pay GET https://api.example.com/answer".into()], Utc::now() + TimeDelta::hours(1), Some(Every::Days(1)), None);
        task.approve(5_000_000, &pay);
        let saved = task.clone();
        e.set_tasks(|t| t.push(saved));
        let x402 = priced(&pay, "", true);
        let near = priced(&pay, &near_header(BASE_USDC, "eip155:8453"), false);
        assert_eq!((x402.way(), near.way(), quote(&pay, 1000, None).way()), ("x402 · Base", "MPP · NEAR Intents", "MPP · Tempo"));
        RUN_TASK.scope(Some(task.id), async {
            assert!(!e.pays_by_itself(x402, &[]));
            assert_eq!(e.st().card.as_ref().map(|c| c.title.clone()), Some("Pay 0.001 USDC · Example".into()));
            let said = last(&e);
            assert_eq!(said, "Example asks 0.001 USDC by x402 on Base. Press Confirm to sign it, and its facilitator pays the fee, or Cancel. Base's USDC comes from Solana through NEAR Intents first.");
            clear_card(&e);
            assert!(!e.pays_by_itself(near, &[]));
            let card = e.st().card.clone().expect("its card");
            assert_eq!(card.title, "Pay 1.01 USDC · Example · NEAR Intents");
            assert_eq!(card.lines[1], "Example gets 1 USDC at merchant.near. If the payment doesn't settle, a refund comes back to you.");
            let said = last(&e);
            assert!(said.starts_with("Example asks 1.01 USDC on Base through NEAR Intents and gets 1 USDC at merchant.near."), "{said}");
            assert!(!card.lines.iter().chain([&said]).any(|l| l.contains(';')));
        }).await;
    }).await;
}

/// Confirm on a charge this wallet can't pay as shown says so before anything is asked or signed: x402 from the
/// automation wallet or a Ledger, a NEAR Intents charge in a token it doesn't pay, or with no address there.
#[tokio::test]
async fn a_charge_this_wallet_can_t_pay_is_refused_before_anything_is_signed() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(REFUND.into()), ..Default::default() });
        let executor = e.executor().unwrap();
        let pay = pay("https://api.example.com/answer");
        let said = |r: trade::Result<String>| r.map_or_else(|e| e.to_string(), |_| "paid".into());
        e.st_mut().payment = Some(priced(&pay, "", true));
        let automation = SignerWallet { id: "auto".into(), label: "Automation".into(), kind: "phrase".into(), solana: None, evm: Some(DEPOSIT.into()), near: None,
                                        test: None, automation: Some(true) };
        let refused = TASK_RUN.scope(Some(automation), e.pay(&pay, &executor)).await;
        assert_eq!(said(refused), "An x402 payment is signed by this wallet's own key, so it can't run from the automation wallet or a Ledger. Nothing was paid.");
        assert!(e.st().payment.is_some());
        // A NEAR Intents charge in a token this wallet doesn't pay (read as one that does would have it on the card).
        let polygon = near_header("eip155:137/erc20:0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", "eip155:137");
        let challenge = ihatecrypto_core::mpp::challenge_paying(&polygon, Utc::now(), |_| true).unwrap();
        e.st_mut().payment = Some(PaymentQuote { challenge, ..priced(&pay, "", true) });
        e.st_mut().payment.as_mut().unwrap().x402 = None;
        assert_eq!(said(e.pay(&pay, &executor).await), "ihatecrypto doesn't pay this token. Nothing was paid.");
        // USDC on Solana, and this wallet has no Solana address.
        let sol = near_header(&format!("{}/token:EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", NearCharge::SOLANA_MAINNET), NearCharge::SOLANA_MAINNET);
        e.st_mut().payment = Some(priced(&pay, &sol, false));
        assert_eq!(said(e.pay(&pay, &executor).await), "No address on Solana in this wallet.");
    }).await;
}

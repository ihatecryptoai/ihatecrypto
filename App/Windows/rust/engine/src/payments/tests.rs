//! Payments, offline: what's refused before a service is ever called, and the USDC on Tempo the card counts.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::scheduled_task::{Every, ScheduledTask};
use rust_decimal::Decimal;
use signer_api::SignerReply;

use crate::automation::RUN_TASK;
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
    let challenge = MppChallenge { params: HashMap::new(), amount, payee: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".into(), memo: None, expires: None };
    PaymentQuote { command: command.clone(), challenge, ask: String::new(), service: "Example".into(), probed: false, task: task.map(|t| t.id) }
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

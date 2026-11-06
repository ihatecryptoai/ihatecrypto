//! The automation wallet and recurring buys (Auto7Tests, Moves6Tests, Runs8Tests, Live13Tests, Holds10Tests, Cards6Tests):
//! a step without its amount stops a run before anything moves, a send to a saved name is a transfer and a contract call
//! is never carried out unseen, a recurring buy keeps its token by its mint and shows its symbol, what a task bought is
//! owed to the wallet that's on, and the marks follow their wallet. Offline: no signer, no network, no Jupiter.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use ihatecrypto_core::foundation::Uuid;
use ihatecrypto_core::scheduled_task::Every;
use signer_api::{SignerReply, SignerWallet};

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;

/// An engine on the test's own folder, in English; locked, so a state push draws the lock alone. Its settings set the
/// process's switches (`trade::modules`), so the tests that make one take turns.
async fn engine() -> (Rc<Engine>, tokio::sync::MutexGuard<'static, ()>) {
    let turn = crate::modules::SWITCHES.lock().await;
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the test engine stays locked"));
    e.st_mut().lock.locked = true;
    e.set_settings(|s| s.language = Some("en".into()));
    (e, turn)
}

fn wallet(id: &str, label: &str, automation: bool) -> SignerWallet {
    // «abandon … about»'s Solana address and Hardhat's account #0.
    SignerWallet { id: id.into(), label: label.into(), kind: "phrase".into(), solana: Some("HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk".into()),
                   evm: Some("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into()), near: None, test: None, automation: automation.then_some(true) }
}

/// The wallet that's on, «Main», and an automation wallet beside it when asked.
fn wallets(e: &Engine, automation: bool) {
    let mut list = vec![wallet("1", "Main", false)];
    if automation { list.push(wallet("2", "Automation", true)) }
    e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), wallets: Some(list), active_wallet: Some("1".into()), ..Default::default() });
}

fn task(steps: &[&str]) -> ScheduledTask {
    ScheduledTask::new(steps.iter().map(|s| s.to_string()).collect(), Utc::now() + TimeDelta::hours(1), Some(Every::Days(1)), None)
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

const SAMO: &str = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU";

#[test]
fn a_task_says_what_it_does() {
    let t = task(&["move stop to breakeven"]);
    assert!(t.manages_position() && !t.moves_money());
    assert!(!task(&["buy SOL for 5 USDC"]).manages_position());
    assert!(task(&["buy SOL for 5 USDC"]).buys_coins() && task(&["spread USDC across the best rates"]).spreads_usdc());
    assert!(!task(&["check: is SOL above 300?"]).moves_money());
    assert!(task(&["sell SOL"]).sells() && !task(&["buy SOL for 5 USDC"]).sells());
    assert_eq!(split_key("solana:So1:x"), ["solana", "So1:x"]);
    assert_eq!(split_key(":abc"), ["abc"]);
    assert_eq!(split_key("solana:"), ["solana"]);
    assert_eq!(prefix("ÅÅÅ", 2), "ÅÅ");
}

/// B11: a recurring buy of a coin off the fixed list keeps its mint, so each run buys that token and never another one
/// trading under its ticker; one on the list keeps its name.
#[test]
fn a_recurring_buy_keeps_its_token() {
    let samo = SpotAsset { token2022: false, ..SpotAsset::new("SAMO", SAMO, 9) };
    let step = Engine::recurring_step(&WalletCommand::Buy { asset: samo, usdc: Some(Decimal::TWO) });
    assert_eq!(step, format!("buy token {SAMO} for 2 USDC"));
    let Some(WalletCommand::Buy { asset, usdc }) = WalletCommand::parse(&step) else { panic!("«{step}» doesn't read back as a buy") };
    assert!(asset.mint == SAMO && usdc == Some(Decimal::TWO));
    let sol = SpotAsset::all().iter().find(|a| a.symbol == "SOL").unwrap().clone();
    let buy = WalletCommand::Buy { asset: sol, usdc: Some(Decimal::from(5)) };
    assert_eq!(Engine::recurring_step(&buy), buy.summary());
}

#[tokio::test]
async fn a_recurring_buy_by_its_mint_shows_its_symbol() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let step = format!("buy token {SAMO} for 5 USDC");
        assert_eq!(e.shown_step(&step), step);
        // A mint's short form isn't a name.
        e.remember_name(&WalletCommand::Buy { asset: SpotAsset::by_mint(SAMO), usdc: Some(Decimal::from(5)) });
        assert_eq!(e.shown_step(&step), step);
        e.remember_name(&WalletCommand::Buy { asset: SpotAsset::new("SAMO", SAMO, 9), usdc: Some(Decimal::from(5)) });
        assert_eq!(e.shown_step(&step), "buy SAMO for 5 USDC");
        assert_eq!(e.kept_assets()[SAMO], KeptAsset { symbol: "SAMO".into(), decimals: 9, token2022: false });
        // A coin of the fixed list keeps its own words.
        assert_eq!(e.shown_step("buy SOL for 5 USDC"), "buy SOL for 5 USDC");
        e.remember_name(&WalletCommand::Buy { asset: SpotAsset::all()[0].clone(), usdc: None });
        assert!(!e.token_names().contains_key(&SpotAsset::all()[0].mint));
    }).await;
}

/// S1: a scheduled step that names no amount never runs as «all of it» on the automation wallet, where nobody may be at
/// the computer: the run stops before anything moves and says so.
#[tokio::test]
async fn an_automation_step_without_its_amount_stops_the_run() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        wallets(&e, true);
        for step in ["buy SOL", "deposit USDC into Kamino", "stake SOL"] {
            assert!(WalletCommand::parse(step).and_then(|c| c.amount_question()).is_some(), "{step}");
            assert_eq!(e.automation_steps(&task(&[step])).await, None, "«{step}» would run");
            assert!(last(&e).contains("doesn't say how much"), "{}", last(&e));
            assert_eq!(e.st().messages().last().unwrap().note.as_deref(), Some("schedule"));
        }
        // A step after a good one stops the run too: nothing of it runs.
        assert_eq!(e.automation_steps(&task(&["buy SOL for 5 USDC", "stake SOL"])).await, None);
        // With its amount it goes on.
        assert_eq!(e.automation_steps(&task(&["buy SOL for 5 USDC"])).await.map(|s| s.len()), Some(1));
    }).await;
}

#[tokio::test]
async fn an_automation_send_to_a_saved_name_is_a_transfer() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        // Public test data only: Hardhat's account #3.
        let mom = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
        e.st_mut().book = vec![SignerAddress { name: "Mom".into(), network: "base".into(), address: mom.into(), memo: String::new() }];
        let steps = e.automation_steps(&task(&["send 5 USDC to Mom", "chart SOL"])).await.unwrap();
        let ChainStep::Transfer(entry, amount) = &steps[0] else { panic!("not a transfer: {steps:?}") };
        assert!(entry.name == "Mom" && entry.network == "base" && *amount == Decimal::from(5));
        // A chart runs by its words.
        assert_eq!(steps[1], ChainStep::Said("chart SOL".into()));
        // A contract call is never carried out unseen: it draws its card.
        let call = format!("call base {mom} deposit(uint256) 1");
        if WalletCommand::parse(&call).is_some() {
            let s = e.automation_steps(&task(&[&call])).await.unwrap();
            assert!(matches!(s[0], ChainStep::Said(_)), "a call went as a move: {s:?}");
        }
        // A name not saved stops the run, said.
        assert_eq!(e.automation_steps(&task(&["send 5 USDC to Stranger"])).await, None);
        // A task that moves no money is only words.
        assert_eq!(e.automation_steps(&task(&["chart SOL"])).await, Some(vec![ChainStep::Said("chart SOL".into())]));
        // Switched off in Settings › Modules: said, nothing runs.
        e.set_settings(|s| s.modules = Some("protocol.jupiter=0".into()));
        assert_eq!(e.automation_steps(&task(&["buy SOL for 5 USDC"])).await, None);
        assert_eq!(last(&e), "Jupiter is off in Settings › Modules.");
        e.set_settings(|s| s.modules = None);
    }).await;
}

#[tokio::test]
async fn the_automation_wallet_runs_only_what_it_may() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let buy = task(&["buy SOL for 5 USDC"]);
        assert!(e.automation_wallet().is_none() && !e.uses_automation(&buy) && !e.waits_for_switch(&buy));
        wallets(&e, true);
        assert_eq!(e.automation_wallet().unwrap().id, "2");
        assert!(e.uses_automation(&buy) && e.waits_for_switch(&buy));
        // A sale, a task from the debug API and one set to pay from the wallet that's on never run from it.
        assert!(!e.uses_automation(&task(&["sell SOL"])));
        assert!(!e.uses_automation(&ScheduledTask { untrusted: Some(true), ..buy.clone() }));
        assert!(!e.uses_automation(&ScheduledTask { from_active: Some(true), ..buy.clone() }));
        // A position-only task is the automation wallet's too; a check signs nothing, so it never waits.
        assert!(e.waits_for_switch(&task(&["move stop to breakeven"])) && !e.waits_for_switch(&task(&["check: is SOL above 300?"])));
        // Its switch on the Schedule.
        e.set_tasks(|t| t.push(buy.clone()));
        e.set_task(&buy.id.to_string(), false);
        assert_eq!(e.st().tasks[0].paused, Some(true));
        e.set_task(&buy.id.to_string(), true);
        assert_eq!(e.st().tasks[0].paused, None);
        e.set_task("no such task", false);
        // There's one already: said, nothing asked of the signer.
        e.create_automation_wallet(None).await;
        assert_eq!(last(&e), "There's an automation wallet already: «Automation».");
    }).await;
}

#[tokio::test]
async fn what_a_task_bought_is_owed_to_the_wallet_thats_on() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        wallets(&e, true);
        let key = format!("solana:{SAMO}");
        e.owe(&key, Decimal::from(5));
        e.owe(&key, Decimal::from(-2));
        assert_eq!(e.for_main(), HashMap::from([(key.clone(), Decimal::from(3))]));
        assert_eq!(std::fs::read_to_string(store::url("for-main.json")).unwrap(), format!(r#"{{"1":{{"{key}":3}}}}"#));
        // Dollars stay: they're the budget. A key that isn't one is left alone.
        e.owe(&format!("solana:{}", SpotAsset::usdc().mint), Decimal::from(5));
        e.owe("solana", Decimal::ONE);
        assert_eq!(e.for_main().len(), 1);
        // Spent all of it: nothing owed, and the wallet's entry goes.
        e.owe(&key, Decimal::from(-3));
        assert!(e.for_main().is_empty());
        assert_eq!(std::fs::read_to_string(store::url("for-main.json")).unwrap(), "{}");
        // Kept per main wallet.
        e.owe(&key, Decimal::ONE);
        e.st_mut().status.as_mut().unwrap().active_wallet = Some("2".into());
        assert!(e.for_main().is_empty());
        // The automation wallet itself is never paid back to.
        e.send_bought_to_main(&wallet("2", "Automation", true)).await;
    }).await;
}

#[tokio::test]
async fn a_recurring_buy_gets_its_board_and_its_marks_follow_their_wallet() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let t = ScheduledTask::new(vec!["buy SOL for 5 USDC".into()], Utc::now() + TimeDelta::days(7), Some(Every::Days(7)), None);
        e.set_tasks(|ts| ts.push(t.clone()));
        assert!(e.recurring_views().is_empty());
        e.set_recurring_buys(HashSet::from([t.id]));
        let view = e.recurring_views().remove(0);
        assert!(view.symbol == "SOL" && view.usdc == Decimal::from(5) && view.days == 7 && !view.paused && view.solana.is_none());
        let facts = e.recurring_facts("sol");
        assert_eq!(facts[0], "5 USDC → SOL every 7 day(s)");
        assert!(facts[1].starts_with("next run 20") && facts[1].ends_with(" +0000"));
        assert_eq!(facts[2], "USDC on Solana: not read yet");
        // Another wallet: none of the first one's marks.
        e.st_mut().wallet_scope_tasks = Some("94".into());
        assert!(e.recurring_buys().is_empty());
        e.st_mut().wallet_scope_tasks = Some(SignerWallet::FIRST_ID.into());
        assert_eq!(e.recurring_buys(), HashSet::from([t.id]));
        // A marker whose task is gone goes with it; nothing due, nothing runs.
        e.set_recurring_buys(HashSet::from([t.id, Uuid::new()]));
        e.run_recurring_buys().await;
        assert_eq!(e.recurring_buys(), HashSet::from([t.id]));
        // A paused schedule holds them all.
        e.set_settings(|s| s.schedule_paused = Some(true));
        e.set_recurring_buys(HashSet::from([t.id, Uuid::new()]));
        e.run_recurring_buys().await;
        assert_eq!(e.recurring_buys().len(), 2);
        e.set_tasks(Vec::clear);
        assert!(e.recurring_views().is_empty());
        assert_eq!(e.recurring_facts("SOL"), ["stopped: no recurring SOL buy now"]);
    }).await;
}

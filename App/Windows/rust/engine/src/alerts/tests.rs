//! Price alerts (EngineTests' alert kept and cleared, Plan5Tests' plan alert, Chat6Tests' record): set against the mark,
//! refused when passed or when a step isn't something the app runs, tied to the canvas's plan as its first step; at the
//! level a plain one says so once and is done, one with steps becomes a task due now that pays from the wallet that's on.
//! Offline: the marks are set by hand.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use chrono::Utc;
use ihatecrypto_core::alerts::{AlertCommand, PriceAlert};
use ihatecrypto_core::intent::{Side, TradeIntent};
use ihatecrypto_core::plan::Market;
use rust_decimal::Decimal;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

/// An engine on the test's own folder, in English; locked, so a state push draws the lock alone. Its settings set the
/// process's switches (`trade::modules`), so the tests that make one take turns.
async fn engine() -> (Rc<Engine>, tokio::sync::MutexGuard<'static, ()>) {
    let turn = crate::modules::SWITCHES.lock().await;
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the test engine stays locked"));
    e.st_mut().lock.locked = true;
    e.set_settings(|s| s.language = Some("en".into()));
    (e, turn)
}

fn d(n: i64) -> Decimal { Decimal::from(n) }

fn mark(e: &Engine, coin: &str, px: i64) { e.st_mut().markets.insert(coin.into(), Market::new(coin, d(px), 4, 25)); }

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

fn markets(e: &Engine) -> HashSet<String> { e.st().markets.keys().cloned().collect() }

#[tokio::test]
async fn a_price_alert_is_kept_and_cleared() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        e.alert(AlertCommand::Clear, None).await;
        assert_eq!(last(&e), "There were no price alerts.");
        mark(&e, "ETH", 2000);
        mark(&e, "BTC", 100_000);
        let set = AlertCommand::parse("alert me when ETH is above 3000", &markets(&e)).unwrap();
        e.alert(set, Some("note")).await;
        assert_eq!(last(&e), "Set: ETH above 3,000 (now 2,000). It's checked every 10 s while ihatecrypto is open.");
        assert_eq!(e.st().boards()[0].kind, "alerts");
        let kept: Vec<PriceAlert> = store::load("alerts.json").unwrap();
        assert!(kept.len() == 1 && kept[0].above && kept[0].price == d(3000) && kept[0].steps.is_none());
        // No side said: whichever side of the mark the level is on.
        e.alert(AlertCommand::Set { coin: "BTC".into(), price: d(90_000), above: None, steps: vec![] }, None).await;
        assert!(!e.st().alerts[1].above);
        // With steps: a move among them comes as a card when it fires.
        let steps = AlertCommand::parse("when ETH is below 1,900: send 1 USDC to Mom", &markets(&e)).unwrap();
        e.alert(steps, None).await;
        assert_eq!(last(&e), "Set: ETH below 1,900: send 1 USDC to Mom (now 2,000). It's checked every 10 s while ihatecrypto is open. When it fires, its moves come as cards for Confirm.");
        e.alert(AlertCommand::Clear, None).await;
        assert_eq!(last(&e), "Price alerts cleared (3).");
        assert!(e.st().alerts.is_empty() && store::load::<Vec<PriceAlert>>("alerts.json").unwrap().is_empty());
    }).await;
}

#[tokio::test]
async fn an_alert_is_refused_when_passed_unpriced_or_unrunnable() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        mark(&e, "ETH", 2000);
        e.alert(AlertCommand::Set { coin: "ETH".into(), price: d(1000), above: Some(true), steps: vec![] }, None).await;
        assert_eq!(last(&e), "ETH is already at 2,000: that level is passed.");
        e.alert(AlertCommand::Set { coin: "NOPE".into(), price: d(1), above: None, steps: vec![] }, None).await;
        assert_eq!(last(&e), "Hyperliquid has no market for NOPE, so there's no price to watch.");
        e.alert(AlertCommand::Set { coin: "ETH".into(), price: d(1500), above: None, steps: vec!["dance a little".into()] }, None).await;
        assert_eq!(last(&e), "When it fires, “dance a little” isn't something the Windows app can do yet. Nothing was set.");
        e.set_settings(|s| s.language = Some("ru".into()));
        e.alert(AlertCommand::Set { coin: "xyz:CL".into(), price: d(1), above: None, steps: vec![] }, None).await;
        assert_eq!(last(&e), "На Hyperliquid нет рынка xyz:CL — нечего отслеживать.");
        assert!(e.st().alerts.is_empty());
    }).await;
}

#[tokio::test]
async fn a_plan_alert_is_the_plans_first_step() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        mark(&e, "ETH", 2000);
        e.st_mut().intent = TradeIntent { asset: "ETH".into(), side: Side::Long, risk_usd: Some(d(5)), ..TradeIntent::default() };
        let step = "long ETH risk $5".to_string();
        assert!(e.plan_step(&step).is_some_and(|t| t.asset == "ETH" && t.side == Side::Long));
        assert!(e.plan_step("send 1 USDC to Mom").is_none());
        assert!(e.same_plan(Some(std::slice::from_ref(&step)), &e.st().intent.clone()));
        e.alert(AlertCommand::Set { coin: "ETH".into(), price: d(1900), above: None, steps: vec![step.clone()] }, None).await;
        assert_eq!(last(&e), "🔔 Added to the plan as its first step: when ETH is below 1,900 (now 2,000, 5% away), this plan comes back to you to confirm, with any changes you make to it until then.");
        let a = e.plan_alert().unwrap();
        assert_eq!(a.plan.map(|p| p.to_string()), Some(e.st().canvas_id.clone()));
        let s = e.plan_alert_step().unwrap();
        assert_eq!((s.label.as_str(), s.title.as_str(), s.status.as_deref()), ("WHEN · PRICE FALLS TO", "ETH 1,900", Some("watching")));
        assert_eq!(s.lines, ["now\t2,000", "away\t-5%", "then this plan comes to you to confirm"]);
        assert_eq!(e.plan_alert_facts(), ["It waits for a price alert: when ETH falls to 1,900, the plan comes to the user to confirm and enters at the market price then (≈ 1,900); the entry above is today's price, for sizing."]);
        // A new level for the same plan replaces the old one: one alert per plan, and no alerts block.
        e.alert(AlertCommand::Set { coin: "ETH".into(), price: d(1800), above: None, steps: vec![step.clone()] }, None).await;
        assert_eq!(e.st().alerts.len(), 1);
        assert_eq!(e.plan_alert().unwrap().price, d(1800));
        assert!(!e.st().boards().iter().any(|b| b.kind == "alerts"));
        // Traded already: the alert is a plain one.
        let intent = e.st().intent.clone();
        e.st_mut().traded_plan = Some(intent);
        assert!(e.plan_alert().is_some());
        e.alert(AlertCommand::Set { coin: "ETH".into(), price: d(1700), above: None, steps: vec![step] }, None).await;
        assert!(last(&e).starts_with("Set: ETH below 1,700: long ETH risk $5"));
        // No plan on the canvas: none is tied.
        e.st_mut().intent = TradeIntent::default();
        assert!(e.plan_alert().is_none() && e.plan_alert_step().is_none() && e.plan_alert_facts().is_empty());
    }).await;
}

#[tokio::test]
async fn a_crossed_alert_says_so_once_or_runs_its_steps_as_a_task() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        // The run it starts finds the schedule paused: nothing here leaves the test.
        e.set_settings(|s| s.schedule_paused = Some(true));
        mark(&e, "ETH", 3000);
        mark(&e, "BTC", 100_000);
        let plain = PriceAlert::new("ETH", true, d(2500), None);
        let waits = PriceAlert::new("BTC", false, d(50_000), Some(vec!["send 1 USDC to Mom".into()]));
        let steps = PriceAlert::new("ETH", true, d(2900), Some(vec!["send 5 USDC to Mom".into()]));
        // Tied to this canvas's plan: the plan as it is now, edits included.
        let canvas = e.st().canvas_id.clone();
        let tied = PriceAlert { plan: ihatecrypto_core::foundation::Uuid::parse(&canvas), ..PriceAlert::new("ETH", true, d(2800), Some(vec!["long ETH risk $5".into()])) };
        e.st_mut().intent = TradeIntent { asset: "ETH".into(), side: Side::Long, risk_usd: Some(d(7)), ..TradeIntent::default() };
        let want = e.st().intent.command();
        e.set_alerts(|a| a.extend([plain, waits.clone(), steps, tied]));
        e.check_alerts().await;
        assert_eq!(e.st().alerts.iter().map(|a| a.id).collect::<Vec<_>>(), [waits.id]);
        let said = e.st().messages().last().cloned().unwrap();
        assert_eq!(said.text, "🔔 ETH above 2,500 — now 3,000 The alert is done. Set another any time.");
        assert_eq!(said.note.as_deref(), Some("price alert"));
        let record = e.st().history.saved.automatic.last().cloned().unwrap();
        assert_eq!((record.title.as_deref(), record.lines.as_slice()), (Some("Price alert"), ["🔔 ETH above 2,500 — now 3,000".to_string()].as_slice()));
        // Each alert with steps is a task due now, paying from the wallet that's on: never off the automation wallet.
        let tasks = e.st().tasks.clone();
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().all(|t| t.from_active == Some(true) && t.every.is_none() && t.next <= Utc::now()));
        assert_eq!(tasks[0].steps, ["send 5 USDC to Mom"]);
        assert_eq!(tasks[0].because.as_deref(), Some("ETH went above 2,900"));
        assert_eq!(tasks[1].steps, [want]);
        assert!(e.st().scheduled_run.is_some());
        // Nothing crossed: nothing said.
        let before = e.st().messages().len();
        e.check_alerts().await;
        assert_eq!(e.st().messages().len(), before);
    }).await;
}

#[tokio::test]
async fn an_alert_reads_as_its_text() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        assert_eq!(e.alert_text(&PriceAlert::new("xyz:CL", false, d(90), None)), "WTI oil (xyz:CL) below 90");
        assert_eq!(e.alert_text(&PriceAlert::new("ETH", true, d(4000), Some(vec!["sell ETH".into(), "chart ETH".into()]))), "ETH above 4,000: sell ETH then chart ETH");
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(e.alert_text(&PriceAlert::new("BTC", true, d(120_000), None)), "BTC выше 120,000");
    }).await;
}

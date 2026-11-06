//! The Schedule (ScheduleRunsTests, Engine11Tests, Runs8Tests, Live13Tests, Live3Tests, Blocks14Tests): the same task
//! asked twice is found and resumed, a run waits while anything else is going on and one run goes at a time, «Run now»
//! keeps a repeating task's next run, a task's own canvas keeps the Schedule, the History tab lists the runs and the
//! guardian's, the automation wallet's budget says what its tasks take and the gas it lacks. Offline, state set by hand.

use std::rc::Rc;
use std::sync::Arc;

use chrono::{TimeDelta, Utc};
use ihatecrypto_core::alerts::PriceAlert;
use ihatecrypto_core::foundation::Uuid;
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use ihatecrypto_core::scheduled_task::{Every, ScheduledTask};
use rust_decimal::Decimal;
use signer_api::{SignerReply, SignerWallet};
use trade::executor::Position;

use super::*;
use crate::chains::ChainState;
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

fn d(s: &str) -> Decimal { s.parse().unwrap() }

/// «Main» on, and an automation wallet beside it («abandon … about»'s and Hardhat's public addresses).
fn with_automation(e: &Engine) {
    let w = |id: &str, label: &str, automation: Option<bool>| SignerWallet {
        id: id.into(), label: label.into(), kind: "phrase".into(), solana: Some("HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk".into()),
        evm: Some("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into()), near: None, test: None, automation,
    };
    e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), wallets: Some(vec![w("1", "Main", None), w("2", "Automation", Some(true))]),
                                           active_wallet: Some("1".into()), ..Default::default() });
}

/// Every 24 hours from an hour from now: whole hours, so a change of clocks doesn't move a run's count.
fn daily(steps: &[&str]) -> ScheduledTask {
    ScheduledTask::new(steps.iter().map(|s| s.to_string()).collect(), Utc::now() + TimeDelta::hours(1), Some(Every::Hours(24)), None)
}

/// Due a minute ago, as an alert's steps are (they go at once, never waiting for the person to step away).
fn due() -> ScheduledTask {
    ScheduledTask::new(vec!["send 50 USDC to Mom".into()], Utc::now() - TimeDelta::minutes(1), None, Some("SOL went above $1".into()))
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

fn kinds(e: &Engine) -> Vec<String> { e.st().boards().iter().map(|b| b.kind.clone()).collect() }

#[test]
fn gas_is_short_only_under_half_and_rounded_up_to_two_digits() {
    // At least half the target and the floor: nothing.
    assert_eq!(Engine::gas_short(d("0.01"), d("0.005"), d("0.005")), Decimal::ZERO);
    assert_eq!(Engine::gas_short(d("0.003"), d("0.005"), Decimal::ZERO), Decimal::ZERO);
    // Under the floor: up to the target.
    assert_eq!(Engine::gas_short(d("0.001"), d("0.0055"), d("0.005")), d("0.0045"));
    // Two significant digits, rounded up: topped up exactly, a dearer fee next minute asked for dust.
    assert_eq!(Engine::gas_short(Decimal::ZERO, d("0.012345"), Decimal::ZERO), d("0.013"));
    assert_eq!(Engine::gas_short(d("1"), d("150"), Decimal::ZERO), d("149"));
    assert_eq!(Engine::gas_short(d("0.0000001"), d("0.0009"), Decimal::ZERO), d("0.0009"));
}

#[tokio::test]
async fn the_same_task_asked_twice_is_found_and_resumed() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let at_nine = || ScheduledTask::parse("every day at 09:00: buy SOL for 5 USDC", Utc::now()).unwrap();
        let first = at_nine();
        e.schedule(first.clone(), Some("note"));
        let text = e.task_text(&first);
        assert_eq!(last(&e), format!("Scheduled: {text}. It runs while ihatecrypto is open or in the tray. A money move comes as a card for Confirm. To run them by themselves, say «create an automation wallet»."));
        assert!(text.starts_with("buy SOL for 5 USDC · every day · next "));
        assert_eq!(kinds(&e), ["schedule"]);
        e.schedule(at_nine(), None);
        assert_eq!(e.st().tasks.len(), 1);
        assert_eq!(last(&e), format!("That’s already scheduled: buy SOL for 5 USDC — {}. It’s on the canvas.", first.when()));
        // Paused, then asked for again: switched back on, not made twice.
        let id = e.st().tasks[0].id.to_string();
        e.set_task(&id, false);
        assert!(e.task_text(&e.st().tasks[0].clone()).ends_with(" · paused"));
        e.schedule(at_nine(), None);
        assert!(e.st().tasks.len() == 1 && e.st().tasks[0].paused.is_none());
        assert!(last(&e).ends_with("It’s on again."), "{}", last(&e));
        // Without an automation wallet there's nothing to pick where it pays from.
        assert!(!e.can_pick_pays(&e.st().tasks[0].clone()));
        e.set_from_active(&id, true);
        assert_eq!(e.st().tasks[0].from_active, None);
        // A step the app doesn't run: nothing scheduled.
        e.schedule(ScheduledTask::parse("tomorrow: dance a little", Utc::now()).unwrap(), None);
        assert_eq!(last(&e), "“dance a little” isn't something the Windows app can do yet. Nothing was scheduled.");
        e.cancel_tasks(None);
        assert_eq!(last(&e), "Scheduled tasks cancelled (1).");
        e.cancel_tasks(None);
        assert_eq!(last(&e), "Nothing was scheduled.");
    }).await;
}

#[tokio::test]
async fn with_an_automation_wallet_a_task_starts_paused_and_picks_where_it_pays() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        with_automation(&e);
        e.schedule(daily(&["buy SOL for 5 USDC"]), None);
        let t = e.st().tasks[0].clone();
        assert_eq!(t.paused, Some(true));
        assert!(last(&e).ends_with(" Its moves run by themselves from «Automation», no Windows Hello — so it starts paused: switch it on in the Schedule block."));
        // A buy runs from it by itself; the block can switch it to pay from the wallet that's on.
        assert!(e.can_pick_pays(&t) && e.uses_automation(&t));
        e.set_from_active(&t.id.to_string(), true);
        assert!(!e.uses_automation(&e.st().tasks[0].clone()));
        assert_eq!(e.other_payer(&e.st().tasks[0].clone()).from_active, None);
        e.set_from_active(&t.id.to_string(), false);
        assert_eq!(e.other_payer(&e.st().tasks[0].clone()).from_active, Some(true));
        // A top-up of the automation wallet pays from the wallet that's on: never from itself.
        e.schedule(daily(&["send 20 USDC to automation"]), None);
        let top_up = e.st().tasks[1].clone();
        assert!(top_up.from_active == Some(true) && top_up.paused.is_none());
        // A sale waits for Confirm and Windows Hello; it can't pick.
        e.schedule(daily(&["sell SOL"]), None);
        assert!(last(&e).ends_with(" A sale waits for your Confirm and Windows Hello."));
        assert!(!e.can_pick_pays(&e.st().tasks[2].clone()));
        // Untrusted (the debug API's): never.
        assert!(!e.can_pick_pays(&ScheduledTask { untrusted: Some(true), ..t }));
    }).await;
}

#[tokio::test]
async fn the_budget_says_what_the_tasks_take_and_the_gas_they_lack() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let week = Utc::now() + TimeDelta::days(7);
        let buy = daily(&["buy SOL for 5 USDC"]);
        e.set_tasks(|t| t.extend([buy.clone(), daily(&["sell SOL"]), ScheduledTask { from_active: Some(true), ..daily(&["buy SOL for 7 USDC"]) },
                                  ScheduledTask { paused: Some(true), ..daily(&["buy SOL for 9 USDC"]) }]));
        e.st_mut().automation_budget = Some(AutomationBudget { total: d("20"), sol: Some(d("0.001")), ..Default::default() });
        // No automation wallet: nothing of it.
        assert_eq!(e.automation_spend(week), Decimal::ZERO);
        assert!(e.automation_budget_now().is_none());
        with_automation(&e);
        // Only the buys that run on it: 5 USDC a day for 7 days.
        assert_eq!(e.automation_spend(week), d("35"));
        let b = e.automation_budget_now().unwrap();
        assert_eq!((b.weeks, b.short_for_week), (Some(0), Some(d("15"))));
        // 28 runs a step in 4 weeks: 0.005 + 28 × 0.00025 = 0.012 SOL, it has 0.001.
        assert_eq!(b.gas, [BudgetGas { network: "solana".into(), coin: "SOL".into(), amount: d("0.011") }]);
        assert_eq!(e.sol_short(d("0.006")), Decimal::ZERO);
        // The free USDC a spread would pay from: the automation wallet's budget.
        let spread = daily(&["spread USDC across the best rates"]);
        e.st_mut().automation_budget = Some(AutomationBudget { solana: Some(d("3")), evm: [("base".to_string(), d("4"))].into(), ..Default::default() });
        assert_eq!(e.task_free_usdc(&spread), Some(d("7")));
        // From the wallet that's on: none until its balances are read.
        assert_eq!(e.task_free_usdc(&ScheduledTask { from_active: Some(true), ..spread }), None);
        // The schedule paused: nothing is taken.
        e.pause_schedule(true);
        assert_eq!(e.st().settings.schedule_paused, Some(true));
        assert_eq!(e.automation_spend(week), Decimal::ZERO);
        e.pause_schedule(false);
        assert_eq!(e.st().settings.schedule_paused, None);
        // The facts the model reads about an item.
        let id = format!("task:{}", buy.id);
        let facts = e.schedule_item_facts(&id);
        assert_eq!(facts[1..], ["steps: buy SOL for 5 USDC".to_string(), "signs by itself from the automation wallet".into()]);
        assert_eq!(e.schedule_item_facts("task:gone"), ["no longer scheduled"]);
        assert_eq!(e.schedule_item_facts("perp:ETH"), ["closed"]);
        e.st_mut().positions = vec![Position { coin: "ETH".into(), size: d("-0.5"), entry: d("2000"), unrealized_pnl: d("12.5"), liquidation: None }];
        assert_eq!(e.schedule_item_facts("perp:ETH"), ["short 0.5 at 2,000, P/L $12.50; its stop and take are on the exchange"]);
        assert_eq!(e.schedule_title("perp:ETH"), "Short ETH");
        assert_eq!(e.schedule_title(&id), "buy SOL for 5 USDC");
        assert_eq!(e.schedule_title("task:gone"), "Scheduled task");
    }).await;
}

/// R9-3, T12, T18: while held steps run, a «How much?» is open, half-typed text is in the field or the schedule is
/// paused, no scheduled run starts.
#[tokio::test]
async fn nothing_scheduled_starts_while_anything_else_goes_on() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let task = due();
        e.set_tasks(|t| t.push(task.clone()));
        let untouched = |e: &Engine| e.st().card.is_none() && e.st().tasks.first().map(|t| t.next) == Some(task.next) && e.st().messages().is_empty();
        e.st_mut().chain_state.running_held = true;
        e.run_due_tasks().await;
        assert!(untouched(&e), "a scheduled run started in a held run's middle");
        e.st_mut().chain_state = ChainState::default();
        e.st_mut().awaiting_amount = Some(WalletCommand::Buy { asset: SpotAsset::usdc(), usdc: None });
        e.run_due_tasks().await;
        assert!(untouched(&e), "a scheduled run started over a «How much?»");
        e.st_mut().awaiting_amount = None;
        e.st_mut().composing = true;
        e.run_due_tasks().await;
        assert!(untouched(&e), "a scheduled run started over the person's typing");
        e.st_mut().composing = false;
        e.pause_schedule(true);
        e.run_due_tasks().await;
        assert!(untouched(&e), "a paused schedule ran its task");
        // A clock task waits for the person to step away (half an hour at most).
        e.pause_schedule(false);
        e.set_tasks(|t| t[0] = ScheduledTask { because: None, ..task.clone() });
        let away = e.st().user_away();
        if !away {
            e.run_due_tasks().await;
            assert!(untouched(&e), "a clock task ran with the person at the computer");
        }
    }).await;
}

#[tokio::test]
async fn the_scheduled_run_is_one_at_a_time() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        e.start_scheduled_run();
        let first = e.st().scheduled_run.as_ref().map(|r| r.id());
        assert!(first.is_some());
        // A second start while it runs leaves it be: no second run beside it.
        e.start_scheduled_run();
        assert_eq!(e.st().scheduled_run.as_ref().map(|r| r.id()), first);
        e.scheduled_run_ended().await;
        assert!(e.st().scheduled_run.is_none());
    }).await;
}

#[tokio::test]
async fn run_now_keeps_a_repeating_tasks_next_run() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        // The run it starts finds the schedule paused: what «Run now» set up stays to be seen.
        e.pause_schedule(true);
        let task = daily(&["remember: run now"]);
        e.set_tasks(|t| t.push(task.clone()));
        let key = format!("task:{}", task.id);
        e.run_now(&key).await;
        assert_eq!(e.st().kept_next.get(&task.id.to_string()), Some(&task.next));
        assert!(e.st().tasks[0].next <= Utc::now() && e.st().run_now_keys.contains(&key));
        assert!(e.st().scheduled_run.is_none());
        e.run_now("task:nothing").await;
    }).await;
}

#[tokio::test]
async fn a_tasks_canvas_keeps_the_schedule() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let task = daily(&["remember: live13"]);
        e.set_tasks(|t| t.push(task.clone()));
        let home = e.st().canvas_id.clone();
        e.pin(Board::new("schedule", "schedule"));
        let key = format!("task:{}", task.id);
        e.open_schedule(&key).unwrap();
        assert_eq!(e.schedule_key(), Some(key.clone()));
        assert_eq!(kinds(&e), ["scheduleItem", "schedule"]);
        assert_eq!(e.st().canvases[e.st().canvas_index()].title, "remember: live13");
        assert_eq!(e.st().schedule_return.as_deref(), Some(home.as_str()));
        // Opened again: the same canvas, its block on top.
        let item = e.st().canvas_id.clone();
        e.pin(Board::new("result", "on top"));
        e.open_schedule(&key).unwrap();
        assert_eq!(e.st().canvas_id, item);
        assert_eq!(kinds(&e)[0], "scheduleItem");
        // Close: back where it came from, and an item's canvas nothing was written on leaves the list.
        e.close_schedule().unwrap();
        assert!(e.st().canvas_id == home && e.schedule_key().is_none() && e.st().schedule_return.is_none());
        assert!(!e.st().canvases.iter().any(|c| c.id == item));
        // Opened from somewhere the Schedule wasn't, it doesn't come along.
        e.set_boards(Vec::clear);
        e.open_schedule(&key).unwrap();
        assert_eq!(kinds(&e), ["scheduleItem"]);
        e.close_schedule().unwrap();
        assert_eq!(e.open_schedule("nonsense").unwrap_err().0, "Unknown scheduled item «nonsense».");
        e.close_schedule().unwrap();
    }).await;
}

/// D7, D8: a recurring buy's run (no canvas) opens to what it said; a position closed outside, a limit filled and the
/// guardian's moves are runs of the position's item.
#[tokio::test]
async fn the_history_tab_lists_the_runs_and_the_guardians() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        let id = Uuid::new().to_string();
        e.set_schedule_runs(|r| r.push(ScheduleRun { at: Utc::now() - TimeDelta::minutes(5), title: "buy SOL for 5 USDC".into(), result: "done".into(),
                                                     line: Some("Bought 0.03 SOL for 5 USDC.".into()), canvas: None, task: Some(id.clone()), activity: None }));
        e.st_mut().history.automatic("Blocks14", "Blocks14 position closed outside ihatecrypto at 100: profit $1.00.", None, false, false, Some("perp:BLOCKS14".into()));
        e.st_mut().history.automatic("Partial profit taken on LIVE13", "Stop moved to breakeven.", Some("LIVE13".into()), false, false, None);
        e.st_mut().history.automatic("Money arrived", "5 USDC on Base", None, false, false, None);
        let runs = e.recent_runs();
        // Newest first; what ihatecrypto did by itself with no item isn't a run.
        assert_eq!(runs.iter().map(|r| r.task.clone().unwrap()).collect::<Vec<_>>(), ["perp:LIVE13".to_string(), "perp:BLOCKS14".into(), id.clone()]);
        assert!(runs[0].title == "Partial profit taken on LIVE13" && runs[0].line.as_deref() == Some("Stop moved to breakeven.") && runs[0].result == "done");
        assert!(runs[1].line.as_deref().is_some_and(|l| l.contains("closed outside")));
        // Filled only while an item's own block is on the canvas, as the LAST RUNS read them.
        assert!(runs.iter().all(|r| r.activity.is_none()));
        e.pin(Board::new("scheduleItem", &format!("task:{id}")));
        let activity = e.recent_runs()[2].activity.clone().unwrap();
        assert_eq!((activity.kind.as_str(), activity.title.as_deref(), activity.lines.as_slice(), activity.failed),
                   ("automatic", Some("buy SOL for 5 USDC"), ["Bought 0.03 SOL for 5 USDC.".to_string()].as_slice(), false));
        // A view only: the saved runs stay the runs.
        assert_eq!(e.st().schedule_runs.len(), 1);
    }).await;
}

/// Live3Tests: an alert's steps are drawn one block each — a command as it is, a trade plan as the plan it becomes.
#[tokio::test]
async fn an_alerts_steps_are_its_blocks() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let (e, _turn) = engine().await;
        e.st_mut().markets.insert("ETH".into(), Market::new("ETH", d("2000"), 4, 25));
        let a = PriceAlert::new("ETH", true, d("999999"), Some(vec!["send 1 USDC to Mom".into(), "long ETH leverage 3x".into()]));
        let view = e.alert_view(&a);
        assert_eq!((view.when.as_deref(), view.now.as_deref()), (Some("ETH above 999,999"), Some("now 2,000 · 49,900% away")));
        let blocks = view.blocks.unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(!blocks[0].plan && blocks[0].title == "send 1 USDC to Mom" && blocks[0].coin.is_none());
        assert!(blocks[1].plan && blocks[1].title == "Long ETH ×3" && blocks[1].coin.as_deref() == Some("ETH"));
        assert_eq!(view.id, a.id.to_string());
        // No mark yet: no «now».
        assert!(e.alert_view(&PriceAlert::new("BTC", false, d("1"), None)).now.is_none());
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(e.alert_view(&a).now.as_deref(), Some("сейчас 2,000 · до цели 49,900%"));
    }).await;
}

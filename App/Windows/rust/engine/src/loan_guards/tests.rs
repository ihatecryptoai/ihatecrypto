//! Loan guards (LoanGuards.swift, and LoanGuardTests' forms read in runCanonical's order): set, kept and stopped in the
//! chat, «stop guarding» taking the prepared repay too; under the line the repay back to the target is a task due now
//! from the wallet that's on, once per dip, re-armed halfway; the USDC the loan's network lacks comes first through NEAR
//! Intents, and with too little anywhere it repays what there is and says what's missing. Another wallet's guard is
//! never read. Offline: no wallet, the health reads handed in, the balances set by hand.

use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::foundation::{Uuid, decimal};
use ihatecrypto_core::loan_guard::{LoanGuard, LoanGuardCommand};
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use rust_decimal::Decimal;

use crate::engine::{Engine, Request};
use crate::outbox::Outbox;
use crate::portfolio::{Network, PortfolioView, Row};
use crate::signer_link::SignerLink;
use crate::store;

fn d(s: &str) -> Decimal { decimal(s).unwrap() }

/// An engine in English, locked so a push draws the lock alone. The funding legs read the process's module switches,
/// so the tests that set them take turns.
async fn engine() -> (Rc<Engine>, tokio::sync::MutexGuard<'static, ()>) {
    let turn = crate::modules::SWITCHES.lock().await;
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the test engine stays locked"));
    let mut st = e.st_mut();
    st.lock.locked = true;
    st.settings.language = Some("en".into());
    drop(st);
    (e, turn)
}

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

/// USDC on these networks, read now.
fn usdc(e: &Engine, held: &[(&str, &str)]) {
    let networks = held.iter().map(|(key, x)| Network {
        key: (*key).into(), name: WalletCommand::place_name(key), usd: Decimal::ZERO, rows: vec![Row { usd: Some(d(x)), ..Row::new("USDC", x, d(x)) }],
        note: None, hidden: vec![], hidden_more: 0,
    }).collect();
    e.st_mut().portfolio = PortfolioView { networks, updated_at: Some(chrono::Utc::now()), ..PortfolioView::default() };
}

fn guard(e: &Engine, loan: &str, network: Option<&str>) -> Uuid {
    let g = LoanGuard::new(loan, network, LoanGuard::default_below(), LoanGuard::default_target());
    let id = g.id;
    e.set_loan_guards(|list| list.push(g));
    id
}

fn of(e: &Engine, id: Uuid) -> LoanGuard { e.st().loan_guards.iter().find(|g| g.id == id).cloned().unwrap() }

fn tasks(e: &Engine) -> Vec<ScheduledTask> { e.st().tasks.clone() }

async fn local(test: impl std::future::Future<Output = ()>) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(test).await;
}

#[tokio::test]
async fn a_guard_is_set_kept_moved_and_stopped_with_its_repay() {
    local(async {
        let (e, _turn) = engine().await;
        assert!(e.run_canonical("stop guarding my loans", "stop guarding my loans").await);
        assert_eq!(last(&e), "No loan was guarded.");
        // No wallet, so no loan to find: nothing is set.
        assert!(e.run_canonical("guard my loan", "guard my loan").await);
        assert_eq!(last(&e), "There's no loan to guard yet: borrow on Kamino or Aave first, then say “guard my loan”.");
        assert!(e.st().loan_guards.is_empty());

        // The router's forms land on the guard, ahead of a price alert's.
        assert!(e.run_canonical("guard my loan on kamino", "следи за займом на камино").await);
        assert_eq!(last(&e), "Guarding your Kamino loan: under 1.25, a repay back to 1.5 comes to you to confirm.");
        assert_eq!(e.st().boards()[0].kind, "alerts");
        e.loan_guard(LoanGuardCommand::Set { loan: Some("aave".into()), network: Some("arbitrum".into()), below: Some(d("1.3")) }, None).await;
        assert_eq!(last(&e), "Guarding your Aave (Arbitrum) loan: under 1.3, a repay back to 1.5 comes to you to confirm.");
        let kept: Vec<LoanGuard> = store::load("loanguards.json").unwrap();
        assert_eq!(kept.iter().map(|g| (g.loan.as_str(), g.network.as_deref(), g.below)).collect::<Vec<_>>(),
                   [("kamino", None, d("1.25")), ("aave", Some("arbitrum"), d("1.3"))]);

        // A new line for the same loan keeps the guard: one that fired stays fired, so no second repay.
        let id = kept[0].id;
        usdc(&e, &[("solana", "10")]);
        assert!(e.loan_read(id, Some((Some(d("1.2")), d("20")))));
        e.loan_guard(LoanGuardCommand::Set { loan: Some("kamino".into()), network: None, below: Some(d("1.4")) }, None).await;
        let moved = e.st().loan_guards.iter().find(|g| g.loan == "kamino").cloned().unwrap();
        assert!(moved.id == id && !moved.armed && moved.fired.is_some() && moved.below == d("1.4") && moved.target == d("1.6"));
        assert_eq!(e.st().loan_guards.len(), 2);

        // «Stop guarding» takes the repay it prepared too; another wallet's guard stays.
        let other = LoanGuard { owner: Some("another wallet".into()), ..LoanGuard::new("kamino", None, d("1.25"), d("1.5")) };
        e.set_loan_guards(|list| list.push(other));
        e.set_settings(|s| s.language = Some("ru".into()));
        e.loan_guard(LoanGuardCommand::Clear, Some("note")).await;
        assert_eq!(last(&e), "Защита займов снята (2).");
        assert!(tasks(&e).is_empty());
        let left: Vec<LoanGuard> = store::load("loanguards.json").unwrap();
        assert!(left.len() == 1 && left[0].owner.as_deref() == Some("another wallet"));
    }).await;
}

#[tokio::test]
async fn under_its_line_a_guard_prepares_one_repay_and_rearms_halfway() {
    local(async {
        let (e, _turn) = engine().await;
        let id = guard(&e, "kamino", None);
        usdc(&e, &[("solana", "10")]);
        // Over the line nothing happens; the block shows the health.
        assert!(!e.loan_read(id, Some((Some(d("1.3")), d("20")))));
        assert!(tasks(&e).is_empty() && of(&e, id).armed);
        assert_eq!(e.loan_guard_views()[0].now.as_deref(), Some("now 1.3 · liquidation at 1"));
        // Under it: 20 × (1 − 1.2 / 1.5) = 4 USDC back to 1.5, from the wallet that's on, due now, waiting for Confirm.
        assert!(e.loan_read(id, Some((Some(d("1.2")), d("20")))));
        let t = tasks(&e);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].steps, ["repay 4 USDC on kamino"]);
        assert!(t[0].from_active == Some(true) && t[0].every.is_none() && t[0].next <= chrono::Utc::now());
        assert_eq!(t[0].because.as_deref(), Some("Kamino loan health 1.2 is under 1.25 — liquidation is at 1"));
        assert!(!t[0].because.as_deref().unwrap().contains(';'));
        assert!(!of(&e, id).armed && of(&e, id).fired == Some(t[0].id));
        assert_eq!(e.loan_guard_views()[0].now.as_deref(), Some("now 1.2 · liquidation at 1 · repay prepared"));
        // One per dip: lower again, or back over the line but under rearm (1.375), and nothing more.
        assert!(!e.loan_read(id, Some((Some(d("1.1")), d("20")))));
        assert!(!e.loan_read(id, Some((Some(d("1.3")), d("20")))));
        assert!(tasks(&e).len() == 1 && !of(&e, id).armed);
        // A read that failed changes nothing; over rearm, or the debt gone, it's armed again.
        assert!(!e.loan_read(id, None));
        assert!(!e.loan_read(id, Some((Some(d("1.4")), d("20")))));
        assert!(of(&e, id).armed);
        assert!(!e.loan_read(id, Some((None, Decimal::ZERO))));
        assert_eq!(e.loan_guard_views()[0].now.as_deref(), Some("no loan yet · watching"));
        // A guard set through an untrusted way gives its repay that mark.
        e.set_loan_guards(|list| list[0].untrusted = Some(true));
        assert!(e.loan_read(id, Some((Some(d("1.1")), d("20")))));
        assert_eq!(tasks(&e)[1].untrusted, Some(true));
    }).await;
}

#[tokio::test]
async fn usdc_the_loans_network_lacks_comes_through_near_intents_first() {
    local(async {
        let (e, _turn) = engine().await;
        let id = guard(&e, "aave", Some("base"));
        usdc(&e, &[("base", "1"), ("solana", "50")]);
        // $100 at 0.8 against $64 owed is 1.25; at 1.2 the repay back to 1.5 is 64 × 0.2 = 12.80, Base has 1.
        assert!(e.loan_read(id, Some((Some(d("1.2")), d("64")))));
        let steps = tasks(&e)[0].steps.clone();
        let (repay, legs) = steps.split_last().unwrap();
        assert_eq!(repay, "repay 12.8 USDC on Aave on base");
        assert!(!legs.is_empty() && legs.iter().all(|l| l.ends_with(" via NEAR Intents")), "{steps:?}");
    }).await;
}

#[tokio::test]
async fn with_too_little_anywhere_it_repays_what_there_is_and_says_what_is_missing() {
    local(async {
        let (e, _turn) = engine().await;
        let id = guard(&e, "kamino", None);
        usdc(&e, &[("solana", "1.239")]);
        assert!(e.loan_read(id, Some((Some(d("1.2")), d("20")))));
        assert_eq!(tasks(&e)[0].steps, ["repay 1.23 USDC on kamino"]);
        let said = e.st().history.saved.automatic.last().cloned().unwrap();
        assert_eq!(said.title.as_deref(), Some("🛡 Kamino"));
        assert!(said.lines.iter().any(|l| l == "Loan health 1.2: 2.77 USDC more is needed to bring it back to 1.5. Top up USDC."), "{:?}", said.lines);
        // Nothing at all there: said, no repay, and no second word until it re-arms.
        let empty = guard(&e, "aave", Some("polygon"));
        assert!(!e.loan_read(empty, Some((Some(d("1.2")), d("20")))));
        assert!(tasks(&e).len() == 1 && !of(&e, empty).armed);
    }).await;
}

#[tokio::test]
async fn another_wallets_guard_is_never_read_and_the_block_shows_and_drops_guards() {
    local(async {
        let (e, _turn) = engine().await;
        let other = LoanGuard { owner: Some("another wallet".into()), ..LoanGuard::new("kamino", None, d("1.25"), d("1.5")) };
        let id = other.id;
        e.set_loan_guards(|list| list.push(other));
        e.check_loan_guards().await;
        assert!(e.st().guard_health.is_empty() && tasks(&e).is_empty());
        // On the alerts block beside the price alerts: its line, the repay and the notification it leads to.
        let view = e.snapshot().alerts;
        assert_eq!(view.len(), 1);
        let v = &view[0];
        assert!(v.id == id.to_string() && v.coin.as_deref() == Some("kamino") && v.when.as_deref() == Some("Kamino loan under 1.25"));
        assert_eq!(v.now.as_deref(), Some("reading…"));
        assert_eq!(v.blocks.as_ref().unwrap()[0].title, "USDC back to health 1.5");
        // Its trash button is the alert's (unlocked for the request, locked again before any push).
        e.st_mut().lock.locked = false;
        e.handle(Request { op: "removeAlert".into(), wallet: Some(id.to_string()), ..Request::default() }).await;
        e.st_mut().lock.locked = true;
        assert!(e.st().loan_guards.is_empty() && store::load::<Vec<LoanGuard>>("loanguards.json").unwrap().is_empty());
    }).await;
}

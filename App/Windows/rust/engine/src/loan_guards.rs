//! Loan guards (LoanGuards.swift): «guard my loan on kamino|aave … below H», «stop guarding my loans». A guarded loan's
//! health factor (liquidation at 1) is read with every refresh; under its line the repay back to its target is prepared
//! as a task due now, the USDC the loan's network lacks brought first through NEAR Intents, and it waits on its own
//! canvas for Confirm with a notification, like a price alert's steps. A guard never signs. One repay per dip: it stays
//! disarmed until the health is back over `rearm`. Each guard watches the wallet that set it (its Solana address) and is
//! never read for another; the alerts block draws it as a chain beside the price alerts.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use defi::kamino::{self, Kamino, Loan};
use evm::protocols::aave;
use ihatecrypto_core::foundation::{Num, Rounding, Uuid, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::loan_guard::{LoanGuard, LoanGuardCommand};
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use rust_decimal::Decimal;
use trade::Executor;

use crate::canvas::Board;
use crate::engine::Engine;
use crate::notices::Notice;
use crate::snapshot::{Alert, AlertStep};
use crate::store;
use crate::trade_plan::PreviewSigner;

/// The EVM networks NEAR Intents pays USDC on (protocols' `one_click::asset::usdc_on_evm`): the guard's top-up comes
/// only through NEAR Intents (the owner, 2026-11-03), so only from these, Solana and NEAR.
const INTENTS: [&str; 3] = ["base", "arbitrum", "ethereum"];

/// A loan's health and its USDC debt; health None: no debt there.
pub type LoanRead = (Option<Decimal>, Decimal);

impl Engine {
    /// loanguards.json, saved with every change (Swift's didSet).
    pub fn set_loan_guards(&self, change: impl FnOnce(&mut Vec<LoanGuard>)) {
        let mut st = self.st_mut();
        change(&mut st.loan_guards);
        store::save(&st.loan_guards, "loanguards.json");
    }

    /// The wallet that's on's guards: another wallet's loan is never read as this one.
    fn own_guards(&self) -> Vec<LoanGuard> {
        let st = self.st();
        // A wallet without a Solana address (a Ledger's EVM side, a watched EVM address) owns its guards by its EVM one.
        let (sol, evm) = (st.solana_address(), st.evm_address());
        st.loan_guards.iter().filter(|g| g.owner.is_none() || (g.owner.is_some() && (g.owner == sol || g.owner == evm))).cloned().collect()
    }

    /// «guard my loan on kamino», «stop guarding my loans».
    pub async fn loan_guard(&self, command: LoanGuardCommand, note: Option<&str>) {
        let note = note.map(str::to_string);
        match command {
            LoanGuardCommand::Clear => {
                let mine = self.own_guards();
                // A repay one of them already prepared goes too: they asked for no more of it.
                let (ids, fired): (HashSet<Uuid>, HashSet<Uuid>) = (mine.iter().map(|g| g.id).collect(), mine.iter().filter_map(|g| g.fired).collect());
                self.set_tasks(|t| t.retain(|t| !fired.contains(&t.id)));
                self.set_loan_guards(|list| list.retain(|g| !ids.contains(&g.id)));
                let n = mine.len();
                self.reply(&if n == 0 { self.say("No loan was guarded.", "Ни один займ не был под защитой.") }
                           else { self.say(&format!("Stopped guarding loans ({n})."), &format!("Защита займов снята ({n}).")) }, note);
            }
            LoanGuardCommand::Set { loan: named, network, below } => {
                let (below, target) = (below.unwrap_or_else(LoanGuard::default_below), LoanGuard::default_target());
                // The loan named, or the one the wallet has: Kamino's, then Aave's on the first network with a debt.
                let mut guarded = None;
                let kamino_debt = named.is_none() && self.kamino_loan().await.flatten().is_some_and(|l| l.health().is_some());
                if named.as_deref() == Some("kamino") || kamino_debt {
                    guarded = Some(LoanGuard::new("kamino", None, below, target));
                } else if named.as_deref() == Some("aave") && network.is_some() {
                    guarded = Some(LoanGuard::new("aave", network.as_deref(), below, target));
                } else if let Some(reader) = self.reader() {
                    for m in &aave::MARKETS {
                        if reader.aave_account(m).await.ok().and_then(|a| a.health()).is_some() {
                            guarded = Some(LoanGuard::new("aave", Some(m.chain), below, target));
                            break;
                        }
                    }
                    if guarded.is_none() && named.as_deref() == Some("aave") { guarded = Some(LoanGuard::new("aave", Some("base"), below, target)) }
                }
                let Some(mut g) = guarded else {
                    self.reply(&self.say("There's no loan to guard yet: borrow on Kamino or Aave first, then say “guard my loan”.",
                                         "Пока нечего защищать: сначала займи на Kamino или Aave, потом скажи «следи за займом»."), note);
                    return;
                };
                g.owner = { let st = self.st(); st.solana_address().or_else(|| st.evm_address()) };
                // Windows has no debug API, so no message here is untrusted: the Mac's «a test never replaces the user's
                // own guard» has nothing to tell apart. The same guard with a new line: a fired one stays fired, so it
                // can't prepare a second repay.
                let old = self.st().loan_guards.iter().find(|o| o.loan == g.loan && o.network == g.network && o.owner == g.owner).cloned();
                if let Some(old) = old {
                    (g.id, g.armed, g.fired, g.created) = (old.id, old.armed, old.fired, old.created);
                    self.set_loan_guards(|list| list.retain(|x| x.id != old.id));
                }
                let kept = g.clone();
                self.set_loan_guards(|list| list.push(kept));
                // The guard itself is on the alerts block, with its health.
                self.pin(Board::new("alerts", "alerts"));
                if let Some((health, _)) = self.loan_health(&g).await { self.st_mut().guard_health.insert(g.id, health); }
                let (name, b, t) = (self.guard_name(&g), Fmt::num(g.below, 2), Fmt::num(g.target, 2));
                self.reply(&self.say(&format!("Guarding your {name} loan: under {b}, a repay back to {t} comes to you to confirm."),
                                     &format!("Слежу за займом на {name}: ниже {b} пришлю погашение до {t} на подтверждение.")), note);
            }
        }
    }

    /// «Aave (Base)», «Kamino».
    pub fn guard_name(&self, g: &LoanGuard) -> String {
        if g.loan == "aave" { format!("Aave ({})", WalletCommand::place_name(g.network.as_deref().unwrap_or("base"))) } else { "Kamino".into() }
    }

    /// Reads only: the wallet's addresses (or the watched ones) with a signer that refuses, for Aave's account.
    fn reader(&self) -> Option<Arc<Executor>> {
        let wallet = self.st().read_wallet().filter(|w| !w.evm.is_empty())?;
        Some(Executor::new(wallet, Arc::new(PreviewSigner { refusal: "A loan guard only reads.".into() }), self.journal(), |_| {}))
    }

    /// The wallet's Kamino loan on the Backpack market (the Mac's kaminoLoan); Some(None): no position. None: not read.
    async fn kamino_loan(&self) -> Option<Option<Loan>> {
        let sol = self.st().solana_address()?;
        Kamino::default().loan(&sol, kamino::BACKPACK_MARKET).await.ok()
    }

    /// The loan's health and its USDC debt; None when it couldn't be read this time.
    async fn loan_health(&self, g: &LoanGuard) -> Option<LoanRead> {
        if g.loan == "kamino" {
            let Some(loan) = self.kamino_loan().await? else { return Some((None, Decimal::ZERO)) };
            // The token count lags the interest the health already counts (the executor's repay takes the larger too).
            let usdc = loan.debts.iter().find(|d| d.symbol == "USDC").map_or(Decimal::ZERO, |d| d.tokens);
            return Some((loan.health(), usdc.max(loan.debt_usd)));
        }
        // ponytail: one getUserAccountData a check per Aave guard; cache it if guards pile up.
        let m = aave::MARKETS.iter().find(|m| Some(m.chain) == g.network.as_deref())?;
        let a = self.reader()?.aave_account(m).await.ok()?;
        Some((a.health(), a.debt))
    }

    /// With every refresh (dispatch.rs's 30 s loop): each of the wallet's guards read, and one under its line prepares
    /// its repay.
    // ponytail: every 30 s with the balances, where the Mac speeds its loop to 10 s while a guard exists; into
    // watch_alerts' 10 s loop if a dip ever outruns it.
    pub async fn check_loan_guards(&self) {
        let mine = self.own_guards();
        if mine.is_empty() { return }
        let mut fired = false;
        for g in mine {
            let read = self.loan_health(&g).await;
            fired |= self.loan_read(g.id, read);
        }
        if fired { self.start_scheduled_run() }
        self.changed();
    }

    /// One guard's read: the block's health kept, re-armed over `rearm`, and under `below` while armed its repay prepared
    /// as a task due now from the wallet that's on (never the automation wallet), which waits on its own canvas for
    /// Confirm with a notification (run_due_tasks). True when it fired.
    pub fn loan_read(&self, id: Uuid, read: Option<LoanRead>) -> bool {
        let Some((health, debt)) = read else { return false };
        let Some(g) = self.st().loan_guards.iter().find(|g| g.id == id).cloned() else { return false };
        self.st_mut().guard_health.insert(id, health);
        let Some(health) = health.filter(|h| *h < g.rearm()) else {
            if !g.armed { self.set_loan_guards(|list| list.iter_mut().filter(|x| x.id == id).for_each(|x| x.armed = true)) }
            return false;
        };
        if !g.armed || health >= g.below { return false }
        self.set_loan_guards(|list| list.iter_mut().filter(|x| x.id == id).for_each(|x| x.armed = false));
        let (name, h, t) = (self.guard_name(&g), Fmt::num(health, 2), Fmt::num(g.target, 2));
        let mut usdc = LoanGuard::repay(debt, health, g.target);
        // The USDC the place lacks comes first, from wherever it is, every leg through NEAR Intents. With too little
        // anywhere, it repays what the place has and says what's missing.
        let net = if g.loan == "aave" { g.network.clone().unwrap_or_else(|| "base".into()) } else { "solana".into() };
        let have = self.usdc_free(&net).unwrap_or_default();
        let short = (Num::of(usdc) - have).0.unwrap_or_default();
        let mut legs = vec![];
        if short > Decimal::ZERO {
            if let Some(found) = self.funding_legs_via(short, &net, Some(&INTENTS)) { legs = found } else {
                let down = rounded(have, 2, Rounding::Down);
                let missing = Fmt::num((Num::of(usdc) - down).0.unwrap_or_default(), 2);
                usdc = down;
                self.notify(Notice::Alerts, &format!("🛡 {name}"),
                            &self.say(&format!("Loan health {h}: {missing} USDC more is needed to bring it back to {t}. Top up USDC."),
                                      &format!("Здоровье займа {h}: чтобы вернуть его к {t}, не хватает {missing} USDC. Пополни USDC.")), false, false, None, None);
                if usdc <= Decimal::ZERO { return false }
            }
        }
        let b = Fmt::num(g.below, 2);
        let because = self.say(&format!("{name} loan health {h} is under {b} — liquidation is at 1"),
                               &format!("здоровье займа на {name} {h}, ниже {b} — ликвидация при 1"));
        legs.push(g.repay_command(usdc));
        let mut task = ScheduledTask::new(legs, Utc::now(), None, Some(because));
        task.untrusted = g.untrusted;
        // As the guard promised: the repay comes to the user to confirm, never off the automation wallet by itself.
        task.from_active = Some(true);
        let task_id = task.id;
        self.set_loan_guards(|list| list.iter_mut().filter(|x| x.id == id).for_each(|x| x.fired = Some(task_id)));
        self.set_tasks(|t| t.push(task));
        true
    }

    /// Each guard on the alerts block, as the Mac's AlertsBoard draws it: its line and health now, the repay it prepares,
    /// and the notification with Confirm it waits for. The window's `removeAlert` takes one away.
    pub fn loan_guard_views(&self) -> Vec<Alert> {
        let (guards, health) = { let st = self.st(); (st.loan_guards.clone(), st.guard_health.clone()) };
        guards.iter().map(|g| {
            let (name, b, t) = (self.guard_name(g), Fmt::num(g.below, 2), Fmt::num(g.target, 2));
            let when = self.say(&format!("{name} loan under {b}"), &format!("Займ на {name} ниже {b}"));
            let now = match health.get(&g.id) {
                None => self.say("reading…", "читаю…"),
                Some(None) => self.say("no loan yet · watching", "займа пока нет · слежу"),
                Some(Some(h)) => { let h = Fmt::num(*h, 2); self.say(&format!("now {h} · liquidation at 1"), &format!("сейчас {h} · ликвидация при 1")) }
            } + &if g.armed { String::new() } else { self.say(" · repay prepared", " · погашение готово") };
            let then = self.say(&format!("USDC back to health {t}"), &format!("USDC до здоровья {t}"));
            Alert {
                id: g.id.to_string(), text: when.clone(), coin: Some(g.loan.clone()), when: Some(when), now: Some(now), steps: Some(vec![then.clone()]),
                blocks: Some(vec![AlertStep { plan: false, coin: None, title: then,
                                              line: Some(self.say("USDC it lacks comes through NEAR Intents", "Недостающие USDC придут через NEAR Intents")) }]),
            }
        }).collect()
    }
}

#[cfg(test)]
#[path = "loan_guards/tests.rs"]
mod tests;

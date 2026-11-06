//! The Schedule (Schedule.swift): «every week: buy SOL for 10 USDC», «in 2 hours: withdraw 5 USDC to Solana» — the
//! shared ScheduledTask, run by the minute while ihatecrypto runs (in the tray too). With an automation wallet a task the
//! person switched on runs its moves from it by itself; without one, every money move a task makes comes as a card that
//! waits for Confirm, with a system notification. Here: scheduling and cancelling, the runs (one at a time, on a canvas
//! of their own, logged for the History tab), the automation wallet's budget, «Run now», a task's own canvas, and the
//! alerts block's rows. What it keeps between runs («Run now» keys, the window in front, the field holding text, a
//! task's next run put back, the canvas an item's canvas was opened from) is in `State`'s run_now_keys … schedule_return.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use ihatecrypto_core::agent_commands::AgentCheck;
use ihatecrypto_core::alerts::PriceAlert;
use ihatecrypto_core::foundation::{Date, Num, Rounding, number};
use ihatecrypto_core::holdings;
use ihatecrypto_core::intent::{Fmt, IntentParser, Side, TradeIntent};
use ihatecrypto_core::network::Solana;
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use ihatecrypto_core::stablecoins;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use serde::{Deserialize, Serialize};
use trade::EvmNetwork;
use wallet::WalletError;

use crate::alerts::{away, short_name};
use crate::automation::{SOL_FLOOR, TaskFacts, prefix};
use crate::canvas::Board;
use crate::engine::{Engine, Message, State};
use crate::history_tabs::Activity;
use crate::notices::Notice;
use crate::snapshot::{Alert, AlertStep};
use crate::{json, platform, store};

/// One run of a scheduled task, for the Schedule's History tab (schedule-runs.json): when, what, how it ended and the
/// canvas it ran on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleRun {
    #[serde(with = "json::date")]
    pub at: Date,
    pub title: String,
    /// «done», «failed», «unknown» (sent, its outcome not known), or «nothing»: a check whose condition wasn't met.
    pub result: String,
    /// What it said last: the row's tooltip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<String>,
    /// The task's id: its own canvas lists its runs (the Mac's ScheduleItemBoard «LAST RUNS»).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// All it did, as History keeps its canvas's request. Only in the state: never saved, filled while an item's own
    /// block is on the canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
}

/// The automation wallet's budget as the Schedule shows it (the Mac's AutomationBudget): its money on Solana and on each
/// EVM network, how many weeks of its buys that covers, and the gas it lacks.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationBudget {
    /// USDC on Solana, SOL for its fees, the other Solana coins; USDC on each EVM network that has some.
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub solana: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub sol: Option<Decimal>,
    pub tokens: Vec<BudgetCoin>,
    #[serde(with = "json::numbers")]
    pub evm: HashMap<String, Decimal>,
    /// Circle's USDC at its NEAR account.
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub near: Option<Decimal>,
    /// The USDC everywhere; Solana's and the EVM networks' money in dollars, for the Wallet tab's rows.
    #[serde(with = "number")]
    pub total: Decimal,
    #[serde(rename = "solanaUSD", skip_serializing_if = "Option::is_none", with = "number::option")]
    pub solana_usd: Option<Decimal>,
    #[serde(rename = "evmUSD", with = "number")]
    pub evm_usd: Decimal,
    /// Whole weeks of the tasks' buys it covers (None: they buy nothing), and the USDC short for the next 7 days.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weeks: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub short_for_week: Option<Decimal>,
    /// What it lacks for gas: SOL, then each EVM network's coin where its USDC is. Empty: enough.
    pub gas: Vec<BudgetGas>,
}

/// `AutomationBudget.Coin`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetCoin {
    pub symbol: String,
    #[serde(with = "number")]
    pub amount: Decimal,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
}

/// `AutomationBudget.Gas`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BudgetGas {
    pub network: String,
    pub coin: String,
    #[serde(with = "number")]
    pub amount: Decimal,
}

impl State {
    /// Nobody at the computer: no key or mouse for five minutes (winsec reads the whole session's input), or ihatecrypto's
    /// window not in front for that long, as the Mac counts another app in front (main.rs sends `windowFocus`).
    pub fn user_away(&self) -> bool {
        platform::idle_ms() >= 300_000 || (!self.window_front && (chrono::Utc::now() - self.window_left).num_milliseconds() >= 300_000)
    }
}

/// Money read from outside, added up; None past Decimal's range (Swift's NaN).
fn sum(values: impl IntoIterator<Item = Decimal>) -> Option<Decimal> { values.into_iter().fold(Num::of(Decimal::ZERO), |s, v| s + v).0 }

impl Engine {
    pub fn task_text(&self, t: &ScheduledTask) -> String {
        t.steps.iter().map(|s| self.shown_step(s)).collect::<Vec<_>>().join(" then ") + " · " + &t.when()
            + &if t.paused == Some(true) { self.say(" · paused", " · на паузе") } else { String::new() }
    }

    /// The steps must be what this app runs.
    pub fn runnable(&self, step: &str) -> bool {
        if self.knows(step) || AgentCheck::parse(step).is_some() { return true }
        let markets: HashSet<String> = self.st().markets.keys().cloned().collect();
        IntentParser::apply(step, &TradeIntent::default(), &markets).is_some()
    }

    pub fn schedule(&self, task: ScheduledTask, note: Option<&str>) {
        let note = note.map(str::to_string);
        if let Some(bad) = task.steps.iter().find(|s| !self.runnable(s)) {
            self.reply(&self.say(&format!("“{bad}” isn't something the Windows app can do yet. Nothing was scheduled."),
                                 &format!("«{bad}» Windows-версия пока не умеет. Ничего не запланировано.")), note);
            return;
        }
        let what = task.steps.join(&self.say(", then ", ", потом "));
        // The same steps at the same times again (a repeated message, a test run twice) would run them twice.
        let same = self.st().tasks.iter()
            .find(|t| t.steps == task.steps && t.every == task.every && (t.next - task.next).abs() < TimeDelta::seconds(60)).cloned();
        if let Some(same) = same {
            self.pin(Board::new("schedule", "schedule"));
            let when = same.when();
            if same.paused == Some(true) && self.waits_for_switch(&same) {
                self.reply(&self.say(&format!("That’s scheduled and paused: {what} — {when}. It starts only when you switch it on in the Schedule."),
                                     &format!("Это уже запланировано и стоит на паузе: {what} — {when}. Задача начнёт работать, только когда ты включишь её в «Расписании».")), note);
            } else if same.paused == Some(true) {
                self.set_task(&same.id.to_string(), true);
                self.reply(&self.say(&format!("That was scheduled and paused: {what} — {when}. It’s on again."),
                                     &format!("Это уже было запланировано и стояло на паузе: {what} — {when}. Снова включено.")), note);
            } else {
                self.reply(&self.say(&format!("That’s already scheduled: {what} — {when}. It’s on the canvas."), &format!("Это уже запланировано: {what} — {when}. Это на холсте.")), note);
            }
            return;
        }
        let mut task = task;
        let automation = self.automation_wallet();
        // A top-up of the automation wallet pays from the wallet that's on, with Confirm: never from itself.
        if let Some(a) = &automation
            && task.steps.iter().any(|s| matches!(WalletCommand::parse(s),
                Some(WalletCommand::SendTo { name, .. } | WalletCommand::SendToken { name, .. } | WalletCommand::SendSol { name, .. }) if name.to_lowercase() == a.label.to_lowercase()))
        {
            task.from_active = Some(true);
        }
        let paused = self.waits_for_switch(&task);
        if paused { task.paused = Some(true) }
        let saved = task.clone();
        self.set_tasks(|t| t.push(saved));
        self.pin(Board::new("schedule", "schedule"));
        let label = automation.as_ref().map_or("Automation", |a| a.label.as_str());
        let how = if paused {
            self.say(&format!(" Its moves run by themselves from «{label}», no Windows Hello — so it starts paused: switch it on in the Schedule block."),
                     &format!(" Движения пойдут сами с «{label}», без Windows Hello — поэтому задача на паузе: включи её в блоке «Расписание»."))
        } else if task.sells() && automation.is_some() {
            self.say(" A sale waits for your Confirm and Windows Hello.", " Продажа будет ждать «Подтвердить» и Windows Hello.")
        } else if task.moves_money() {
            self.say(" A money move comes as a card for Confirm. To run them by themselves, say «create an automation wallet».",
                     " Перевод придёт карточкой на подтверждение. Чтобы шли сами, скажи «создай кошелёк автоматизации».")
        } else {
            String::new()
        };
        let text = self.task_text(&task);
        self.reply(&(self.say(&format!("Scheduled: {text}. It runs while ihatecrypto is open or in the tray."),
                              &format!("Запланировано: {text}. Выполнится, пока ihatecrypto открыт или в трее.")) + &how), note);
    }

    pub fn cancel_tasks(&self, note: Option<&str>) {
        let n = self.st().tasks.len();
        self.set_tasks(Vec::clear);
        self.reply(&if n == 0 { self.say("Nothing was scheduled.", "Ничего не было запланировано.") }
                   else { self.say(&format!("Scheduled tasks cancelled ({n})."), &format!("Запланированное отменено ({n}).")) }, note.map(str::to_string));
    }

    /// The automation wallet's money, read with the balances: Solana from its node, USDC and the gas coin on every EVM
    /// network from theirs (an explorer lists a new address minutes late).
    pub async fn refresh_automation_budget(&self) {
        let Some(w) = self.automation_wallet() else {
            self.st_mut().automation_budget = None;
            return;
        };
        let mut b = AutomationBudget::default();
        if let Some(owner) = &w.solana && let Ok(read) = Solana::balance(owner, false).await {
            b.solana = Some(read.usdc);
            b.sol = Some(read.sol);
            b.tokens = read.tokens.iter().filter(|t| t.looks_real()).map(|t| BudgetCoin { symbol: t.symbol.clone(), amount: t.amount, usd: t.usd() }).collect();
            let px = self.st().markets.get("SOL").map_or(Decimal::ZERO, |m| m.mark_px);
            b.solana_usd = (Num::of(read.usdc) + Num(sum(b.tokens.iter().filter_map(|c| c.usd))) + Num::of(read.sol) * px).0;
        }
        if let Some(owner) = &w.evm {
            let wei = Decimal::from(1_000_000_000_000_000_000u64);
            for net in EvmNetwork::all() {
                let Ok(raw) = net.usdc_balance(owner).await else { continue };
                if raw == 0 { continue }
                let usdc = Decimal::from(raw) / Decimal::from(1_000_000);
                b.evm.insert(net.key.into(), usdc);
                // Three moves at today's fee (an approve and a swap or a deposit, ≈ 300k gas each); after that a move
                // buys its own gas with the USDC.
                if usdc >= Decimal::ONE && net.fee_token.is_none() && let Ok(held) = net.rpc.balance(owner).await && let Ok((_, max_fee)) = net.rpc.fees().await
                    && let (Some(held), Some(target)) = ((Num(Decimal::from_u128(held)) / wei).0, (Num::of(Decimal::from(900_000)) * Decimal::from(max_fee) / wei).0)
                {
                    let short = Self::gas_short(held, target, Decimal::ZERO);
                    if short > Decimal::ZERO { b.gas.push(BudgetGas { network: net.key.into(), coin: net.gas_coin.into(), amount: short }) }
                }
            }
        }
        if let Some(account) = &w.near
            && let Some(usdc) = stablecoins::on("near").and_then(|c| c.iter().find(|c| c.symbol == "USDC")).map(|c| c.address)
            && let Some(held) = holdings::near(account).await
        {
            b.near = Some(held.iter().find(|h| h.contract == usdc).map_or(Decimal::ZERO, |h| h.amount));
        }
        b.evm_usd = sum(b.evm.values().copied()).unwrap_or_default();
        b.total = sum([b.solana.unwrap_or_default(), b.evm_usd, b.near.unwrap_or_default()]).unwrap_or_default();
        self.st_mut().automation_budget = Some(b);
    }

    /// The USDC a task could spread where it pays from (the Mac's freeUSDC): the automation wallet's budget, or the idle
    /// USDC of the wallet that's on; None until read.
    pub fn task_free_usdc(&self, t: &ScheduledTask) -> Option<Decimal> {
        if self.uses_automation(t) {
            let budget = self.st().automation_budget.clone();
            return budget.and_then(|b| sum(b.solana.into_iter().chain(b.evm.values().copied())));
        }
        let solana = self.usdc_free("solana")?;
        sum(std::iter::once(solana).chain(EvmNetwork::searched().into_iter().filter_map(|n| self.usdc_free(n.key))))
    }

    /// The task paying from the other wallet: the one that's on instead of the automation wallet, or the other way.
    pub fn other_payer(&self, t: &ScheduledTask) -> ScheduledTask {
        let mut o = t.clone();
        o.from_active = if self.uses_automation(t) { Some(true) } else { None };
        o
    }

    /// The budget as read, with what the tasks on now take from it: worked out at every state, so a task switched on or
    /// the schedule paused shows at once.
    pub fn automation_budget_now(&self) -> Option<AutomationBudget> {
        self.automation_wallet()?;
        let mut b = self.st().automation_budget.clone()?;
        let spend = self.automation_spend(Utc::now() + TimeDelta::days(7));
        // Whole weeks: 20 / 3 unrounded read as 0 («not enough for the next run»).
        if spend > Decimal::ZERO { b.weeks = (Num::of(b.total) / spend).rounded(0, Rounding::Down).0.and_then(|w| w.to_i64()) }
        if spend > b.total { b.short_for_week = (Num::of(spend) - b.total).rounded(0, Rounding::Up).0 }
        if let Some(sol) = b.sol {
            let short = self.sol_short(sol);
            if short > Decimal::ZERO { b.gas.insert(0, BudgetGas { network: "solana".into(), coin: "SOL".into(), amount: short }) }
        }
        Some(b)
    }

    /// What the Schedule and the multisig show, read with the balances each minute.
    pub async fn refresh_live(&self) {
        self.refresh_multisigs().await;
        self.refresh_automation_budget().await;
    }

    /// What the tasks on the automation wallet take from its budget by `date`: each one's buys times its runs until then.
    // ponytail: buys with an amount only; a spread or a deposit spends what's there, unpriced (as on the Mac).
    pub fn automation_spend(&self, until: Date) -> Decimal {
        if self.automation_wallet().is_none() || self.st().settings.schedule_paused == Some(true) { return Decimal::ZERO }
        let tasks = self.st().tasks.clone();
        tasks.iter().filter(|t| t.paused != Some(true) && self.uses_automation(t)).fold(Num::of(Decimal::ZERO), |total, t| {
            let each = t.steps.iter().fold(Num::of(Decimal::ZERO), |s, step| match WalletCommand::parse(step) {
                Some(WalletCommand::Buy { usdc: Some(u), .. } | WalletCommand::BuyNear { usdc: Some(u) } | WalletCommand::BuyStock { usdc: Some(u), .. }
                     | WalletCommand::BuyOn { usdc: Some(u), .. }) => s + u,
                _ => s,
            });
            total + each * t.runs(until)
        }).0.unwrap_or_default()
    }

    /// SOL it lacks for the next 4 weeks of its tasks: the floor for the accounts a move opens plus 0.00025 SOL a step a
    /// run (Jupiter's priority fee cap, then sending what it bought on to Main); nothing while it holds half of that.
    // ponytail: every step priced as a Solana swap, as on the Mac.
    pub fn sol_short(&self, held: Decimal) -> Decimal {
        let until = Utc::now() + TimeDelta::days(28);
        let steps: i64 = if self.st().settings.schedule_paused == Some(true) { 0 } else {
            let tasks = self.st().tasks.clone();
            tasks.iter().filter(|t| t.paused != Some(true) && t.moves_money() && self.uses_automation(t))
                .map(|t| t.runs(until).saturating_mul(t.steps.len() as i64)).fold(0i64, i64::saturating_add)
        };
        let target = (Num::of(SOL_FLOOR) + Num::of(Decimal::from(steps)) * Decimal::new(25, 5)).0.unwrap_or(Decimal::MAX);
        Self::gas_short(held, target, SOL_FLOOR)
    }

    /// Nothing while `held` is at least half of `target` and the `floor` under which moves fail, else up to the target,
    /// rounded up to two significant digits (topped up exactly, a dearer fee next minute asked for dust). Swift's
    /// default `floor` is 0.
    pub fn gas_short(held: Decimal, target: Decimal, floor: Decimal) -> Decimal {
        if held >= floor.max(target / Decimal::TWO) { return Decimal::ZERO }
        let Some(x) = (Num::of(target) - held).0 else { return Decimal::ZERO };
        let d = ihatecrypto_core::foundation::double(x);
        if d <= 0.0 { return Decimal::ZERO }
        let scale = (1 - d.log10().floor() as i64).max(0);
        ihatecrypto_core::foundation::rounded(x, u32::try_from(scale).unwrap_or(u32::MAX).min(28), Rounding::Up)
    }

    /// A price alert as its block draws it (the Mac's AlertsBoard): when, how far the price is now, then its steps.
    pub fn alert_view(&self, a: &PriceAlert) -> Alert {
        let name = short_name(&a.coin);
        let px = self.st().markets.get(&a.coin).map(|m| m.mark_px).filter(|px| *px > Decimal::ZERO);
        let now = px.map(|px| {
            let (p, away) = (Fmt::price(px), away(a.price, px));
            self.say(&format!("now {p} · {away}% away"), &format!("сейчас {p} · до цели {away}%"))
        });
        // A trade plan reads as the plan it becomes («THEN · PLAN ON HYPERLIQUID»); anything else as the command it is.
        let known: HashSet<String> = self.st().markets.keys().cloned().collect();
        let blocks = a.steps.clone().unwrap_or_default().into_iter().map(|step| {
            let plan = if IntentParser::starts_plan(&step, &known) { IntentParser::apply(&step, &TradeIntent::default(), &known) } else { None };
            let Some(t) = plan else { return AlertStep { plan: false, coin: None, title: step, line: None } };
            let rest: Vec<String> = t.changes(&TradeIntent::default()).into_iter()
                .filter(|c| !c.starts_with("market") && c != "long" && c != "short" && !c.starts_with("leverage")).collect();
            let side = if t.side == Side::Long { self.say("Long", "Лонг") } else { self.say("Short", "Шорт") };
            AlertStep {
                plan: true, coin: Some(t.asset.clone()),
                title: format!("{side} {}{}", IntentParser::pretty(&t.asset), t.leverage.map(|l| format!(" ×{}", Fmt::num(l, 2))).unwrap_or_default()),
                line: if rest.is_empty() { None } else { Some(rest.join(" · ")) },
            }
        }).collect();
        let side = if a.above { self.say("above", "выше") } else { self.say("below", "ниже") };
        Alert {
            id: a.id.to_string(), text: self.alert_text(a), coin: Some(a.coin.clone()), when: Some(format!("{name} {side} {}", Fmt::price(a.price))), now,
            steps: a.steps.clone(), blocks: Some(blocks),
        }
    }

    /// «Pause schedule»: nothing scheduled runs until it's resumed; each task keeps its own switch and its next time.
    pub fn pause_schedule(&self, on: bool) { self.set_settings(|s| s.schedule_paused = if on { Some(true) } else { None }) }

    /// The History tab: the newest 30 runs first.
    pub fn recent_runs(&self) -> Vec<ScheduleRun> {
        let (mut runs, item) = {
            let st = self.st();
            // The guardian's moves on a position (partial profit, the trailing stop), a position closed outside ihatecrypto
            // and a limit that filled are runs of its item too — the Mac's activities with schedule «perp:…». The
            // guardian's carry their coin, the others the item.
            let automatic = &st.history.saved.automatic;
            let guarded = automatic[automatic.len().saturating_sub(60)..].iter().filter_map(|a| {
                let task = a.schedule.clone().or_else(|| a.coin.as_ref().map(|c| format!("perp:{c}")))?;
                Some(ScheduleRun { at: a.at, title: a.title.clone().unwrap_or_default(), result: if a.failed { "failed" } else { "done" }.into(),
                                   line: a.lines.last().cloned(), canvas: None, task: Some(task), activity: None })
            });
            let saved = &st.schedule_runs;
            let mut runs: Vec<ScheduleRun> = saved[saved.len().saturating_sub(30)..].iter().cloned().chain(guarded).collect();
            runs.sort_by_key(|r| std::cmp::Reverse(r.at));
            runs.truncate(30);
            (runs, st.boards().iter().any(|b| b.kind == "scheduleItem"))
        };
        if !item { return runs }
        // A run's canvas starts with its «Scheduled: …» request, said as the run starts; a run with no canvas (a recurring
        // buy, the guardian) opens to what it said, as the Mac's automatic ActivityCard.
        let asked: Vec<Activity> = if runs.iter().any(|r| r.canvas.is_some()) {
            self.history_activities().into_iter().filter(|a| a.kind == "request" && a.request.is_some()).collect()
        } else { vec![] };
        for r in runs.iter_mut() {
            r.activity = if r.canvas.is_none() {
                Some(Activity { title: Some(r.title.clone()), lines: r.line.clone().into_iter().collect(), failed: r.result == "failed", ..Activity::new("automatic", r.at) })
            } else {
                asked.iter().filter(|a| (a.at - r.at).abs() < TimeDelta::seconds(3)).min_by_key(|a| (a.at - r.at).abs()).cloned()
            };
        }
        runs
    }

    /// The due recurring buys and tasks in a task of their own, beside the 30 s loop (the Mac's scheduledRun, d8cc366): an
    /// unanswered Windows Hello holds only this run, while balances, «closed outside» and the trading-key offer go on. One
    /// run at a time; a call while one runs leaves it be, and the next 30 s picks up what's due then.
    pub fn start_scheduled_run(&self) {
        if self.st().scheduled_run.is_some() { return }
        let me = self.rc();
        let run = tokio::task::spawn_local(async move {
            me.run_due_tasks().await;
            me.st_mut().scheduled_run = None;
        });
        self.st_mut().scheduled_run = Some(run);
    }

    /// `await scheduledRun?.value`: the run under way, if any, has ended.
    // ponytail: polled every 50 ms, as State keeps the run's JoinHandle (one owner); a watch channel if that ever lags.
    async fn scheduled_run_ended(&self) {
        while self.st().scheduled_run.as_ref().is_some_and(|r| !r.is_finished()) { tokio::time::sleep(Duration::from_millis(50)).await }
    }

    /// The first task whose time has come, as the Mac's runDueTask: on a canvas of its own (the last run's makes way), and
    /// back to the canvas the person was on once it's done, unless it needs them (a Confirm, a failure). Only while nothing
    /// else is going on: a card waiting for Confirm, a reply being written, a move running.
    pub async fn run_due_tasks(&self) {
        self.remind_top_up();
        // Recurring buys first: at their time, Windows Hello only, no canvas of their own (automation.rs).
        self.run_recurring_buys().await;
        // As the Mac's runDueTask: nothing else going on — a card, a reply, a move, a «How much?», steps held behind a
        // card or running after it (a scheduled card in their middle could be taken for theirs).
        let busy = {
            let st = self.st();
            st.settings.schedule_paused == Some(true) || st.pending.is_some() || st.card.is_some() || st.running.is_some() || st.thinking
                || st.awaiting_amount.is_some() || st.chain_state.running_held || st.composing
        };
        if busy { return }
        // A recurring buy has its own runs (run_recurring_buys): one waiting for a Ledger stays due there, never a card here.
        let marked = self.recurring_buys();
        let now = Utc::now();
        let Some((i, mut task)) = self.st().tasks.iter().enumerate()
            .find(|(_, t)| t.paused != Some(true) && t.next <= now && !marked.contains(&t.id)).map(|(i, t)| (i, t.clone())) else { return };
        let key = format!("task:{}", task.id);
        // With the person at the computer a clock task waits for them to step away (half an hour at most), so the canvas
        // doesn't change under them. An alert's steps and «Run now» go at once.
        let (asked, away) = { let st = self.st(); (st.run_now_keys.contains(&key) || task.because.is_some(), st.user_away()) };
        if !(asked || away || Utc::now() - task.next > TimeDelta::seconds(1800)) { return }
        self.st_mut().run_now_keys.remove(&key);
        if task.advance(Utc::now()) {
            // «Run now» on a repeating one keeps its next run as it was (the Mac's TaskRun.kept).
            let kept = self.st_mut().kept_next.remove(&task.id.to_string());
            if let Some(kept) = kept && kept > Utc::now() { task.next = kept }
            let saved = task.clone();
            self.set_tasks(|t| t[i] = saved);
        } else {
            self.set_tasks(|t| { t.remove(i); });
        }
        let back = self.st().canvas_id.clone();
        // The runs before left their canvases; what they did stays in the History. One the person wrote on stays.
        let id = task.id.to_string();
        let gone: Vec<String> = {
            let st = self.st();
            let before: HashSet<&String> = st.schedule_runs.iter().filter(|r| r.task.as_ref() == Some(&id)).filter_map(|r| r.canvas.as_ref()).collect();
            st.canvases.iter().filter(|c| c.id != st.canvas_id && before.contains(&c.id) && c.schedule.is_none() && c.messages.iter().filter(|m| m.from_user).count() <= 1)
                .map(|c| c.id.clone()).collect()
        };
        for c in gone { let _ = self.remove_canvas(&c); }
        if self.new_canvas(false).is_err() { return }
        let request = task.because.as_ref().map_or_else(|| self.say("Scheduled: ", "По расписанию: "), |b| format!("{b}: "))
            + &task.steps.iter().map(|s| self.shown_step(s)).collect::<Vec<_>>().join(&self.say(", then ", ", потом "));
        self.st_mut().messages_mut().push(Message::new(true, &request, Some("schedule".into())));
        self.name_canvas(&request);
        let (started, mark, canvas) = { let st = self.st(); (Utc::now(), st.messages().len(), st.canvas_id.clone()) };
        let (mut ran, mut step_failed) = (true, false);
        let check = task.steps.iter().all(|s| AgentCheck::parse(s).is_some());
        let automation = self.automation_wallet().filter(|_| self.uses_automation(&task) && (task.moves_money() || task.manages_position()));
        if check {
            // A check says something only when its condition is met. Its model answers as a message's would: the chat is
            // busy meanwhile, so no canvas switch or message lands in the middle of it (the Mac's routeWithModel).
            {
                let mut st = self.st_mut();
                st.quiet_check = false;
                st.thinking = true;
            }
            self.changed();
            for step in &task.steps { self.run_canonical(step, step).await; }
            self.st_mut().thinking = false;
            self.changed();
            ran = !self.st().quiet_check;
        } else if let Some(automation) = automation {
            if !self.run_on_automation(&task, &automation).await { step_failed = true }
            self.refresh_automation_budget().await;
        } else if task.steps.len() > 1 {
            // A step that can't go on or a first move that can't be paid stops the run before any card: it failed.
            let chained = self.chain(&task.steps, None, Some("schedule"), false).await;
            if !chained || self.st().chain_state.stopped { step_failed = true }
        } else if let Some(step) = task.steps.first() {
            // A spread with nothing free stays quiet too, with a grey mark (the Mac's checkQuiet, Earn.preparePlan):
            // every day's «see the canvas» led to a canvas already gone (the owner, 2026-10-16).
            self.st_mut().quiet_check = false;
            if !self.run_canonical(step, step).await {
                self.reply(&self.say(&format!("“{step}” didn't run."), &format!("«{step}» не выполнилось.")), Some("schedule".into()));
                step_failed = true;
            }
            ran = !self.st().quiet_check;
        }
        self.log_run(&task, started, mark, &canvas, ran, step_failed);
        let failed = self.st().schedule_runs.last().is_some_and(|r| r.result == "failed");
        // A card waits for Confirm the way the Mac's move waits on its canvas; a trade plan does too, and one a rule stops
        // (no card) needs the person just as much (the Mac's runDueTask).
        let (plan, waits, no_card, told) = {
            let st = self.st();
            let plan = st.canvas_id == canvas && st.intent != TradeIntent::default() && st.traded_plan.as_ref() != Some(&st.intent)
                && !st.positions.iter().any(|p| p.coin == st.intent.asset);
            let told = st.messages().iter().skip(mark).rev().find(|m| !m.from_user).map(|m| m.text.clone()).unwrap_or_default();
            (plan, st.card.is_some() || st.pending.is_some() || plan, st.card.is_none(), told)
        };
        // A check with nothing to tell stays quiet: the run is in the History with a grey mark.
        if ran {
            // A check that speaks is a task's run like any other (the Mac's runDueTask), not a price alert.
            let body = if plan && no_card { self.say("The plan is on its canvas, but a rule stops it — open it to fix.", "План на своём холсте, но его останавливает правило — открой и поправь.") }
                       else if waits { self.say("Ready on its canvas — press Confirm.", "Готово на своём холсте — нажми «Подтвердить».") }
                       else { prefix(&told, 200) };
            self.notify(if waits { Notice::Confirm } else { Notice::Moves }, &request, &body, failed, false, None, None);
        }
        // Nothing here for the person: back where they were. «Run now» and an alert's task stay to be seen.
        let back_now = {
            let st = self.st();
            !asked && !waits && !failed && st.canvas_id == canvas && st.messages().iter().filter(|m| m.from_user).count() <= 1 && st.canvases.iter().any(|c| c.id == back)
        };
        if back_now { let _ = self.open_canvas(&back); }
    }

    /// The day before the automation wallet runs short for its buys, once a day: a notification, and with no one at the
    /// computer the Schedule comes up. The wallet that's on signs only with Windows Hello, so a top-up can't go by itself;
    /// asked ahead, a week of buys then runs while the computer is left alone.
    pub fn remind_top_up(&self) {
        let Some(w) = self.automation_wallet() else { return };
        let Some(b) = self.automation_budget_now() else { return };
        let Some(short) = b.short_for_week else { return };
        let need = self.automation_spend(Utc::now() + TimeDelta::days(1));
        let today = Utc::now().format("%Y-%m-%d").to_string();
        if need <= b.total || store::load::<String>("top-up-asked.json").as_deref() == Some(today.as_str()) { return }
        store::save(&today, "top-up-asked.json");
        let main = self.st().active_wallet().map_or_else(|| "Main".into(), |a| a.label);
        let (label, need, has, short) = (&w.label, Fmt::num(need, 2), Fmt::num(b.total, 2), Fmt::num(short, 2));
        self.notify(Notice::Confirm, &self.say(&format!("«{label}» is short for the next buys"), &format!("На «{label}» не хватит на ближайшие покупки")),
                    &self.say(&format!("The buys in the next 24 hours need {need} USDC, but it has {has}. Top it up with {short} USDC for the week from «{main}»: Confirm and Windows Hello once, then they run by themselves."),
                              &format!("Покупкам в ближайшие сутки нужно {need} USDC, на нём {has}. Пополни его на {short} USDC на неделю с «{main}»: один раз «Подтвердить» и Windows Hello, дальше покупки пойдут сами.")),
                    false, false, None, None);
        let away = self.st().user_away();
        if away { self.pin(Board::new("schedule", "schedule")); }
    }

    /// Where a task pays from: the wallet that's on (Confirm and Windows Hello each run) or the automation wallet. Only
    /// a money task typed in this window that sells nothing can pick, and only with an automation wallet.
    pub fn can_pick_pays(&self, t: &ScheduledTask) -> bool { t.moves_money() && t.untrusted != Some(true) && !t.sells() && self.automation_wallet().is_some() }

    pub fn set_from_active(&self, id: &str, on: bool) {
        let found = self.st().tasks.iter().position(|t| t.id.to_string() == id);
        let Some(i) = found.filter(|&i| { let t = self.st().tasks[i].clone(); self.can_pick_pays(&t) }) else { return };
        self.set_tasks(|t| t[i].from_active = if on { Some(true) } else { None });
    }

    /// A run in the log: failed when one of its moves did (or a step didn't run at all), «nothing» when a check had
    /// nothing to tell. The last 100 are kept.
    fn log_run(&self, task: &ScheduledTask, started: Date, mark: usize, canvas: &str, ran: bool, step_failed: bool) {
        let (failed, unknown, line) = {
            let st = self.st();
            let moved = |state: &str| st.boards().iter().any(|b| b.kind == "move" && b.at >= started && b.state.as_deref() == Some(state));
            // Sent with no answer back: not a failure to do again (it may have gone through), not done either.
            let line = if canvas == st.canvas_id { st.messages().iter().skip(mark).rev().find(|m| !m.from_user).map(|m| m.text.clone()) } else { None };
            (step_failed || moved("failed"), moved("unknown"), line)
        };
        let run = ScheduleRun {
            at: started, title: task.steps.join(" then "), result: if failed { "failed" } else if unknown { "unknown" } else if ran { "done" } else { "nothing" }.into(),
            line: line.map(|l| prefix(&l, 300)), canvas: Some(canvas.into()), task: Some(task.id.to_string()), activity: None,
        };
        self.set_schedule_runs(|r| {
            r.push(run);
            let over = r.len().saturating_sub(100);
            r.drain(..over);
        });
    }

    // MARK: A scheduled item's own canvas
    // A task (or an open position) on a canvas of its own, as the Mac's ScheduleItemBoard: made the first time, then the
    // same one; its block on top, the chat on it can change it; Close goes back to the canvas it was opened from.

    /// The open canvas's item: «task:<id>» or «perp:<coin>».
    pub fn schedule_key(&self) -> Option<String> {
        let st = self.st();
        st.canvases.get(st.canvas_index())?.schedule.clone()
    }

    pub fn schedule_title(&self, key: &str) -> String {
        if let Some(coin) = key.strip_prefix("perp:") {
            let long = self.st().positions.iter().find(|p| p.coin == coin).is_none_or(|p| p.size > Decimal::ZERO);
            return format!("{} {}", if long { self.say("Long", "Лонг") } else { self.say("Short", "Шорт") }, IntentParser::pretty(coin));
        }
        // A recurring buy kept by its mint reads as its symbol here too (shown_step), as in its row.
        let steps = self.st().tasks.iter().find(|t| format!("task:{}", t.id) == key).map(|t| t.steps.clone());
        steps.map_or_else(|| self.say("Scheduled task", "Задача по расписанию"), |s| s.iter().map(|x| self.shown_step(x)).collect::<Vec<_>>().join(" → "))
    }

    pub fn open_schedule(&self, key: &str) -> Result<(), WalletError> {
        if !(key.starts_with("task:") || key.starts_with("perp:")) { return Err(WalletError(format!("Unknown scheduled item «{key}»."))) }
        if self.schedule_key().is_none() {
            let id = self.st().canvas_id.clone();
            self.st_mut().schedule_return = Some(id);
        }
        // Opened from the Schedule: it comes along, so the next item is one click away (the Mac keeps its frame open).
        let from_schedule = self.st().boards().iter().any(|b| b.kind == "schedule");
        let existing = self.st().canvases.iter().find(|c| c.schedule.as_deref() == Some(key)).map(|c| c.id.clone());
        if let Some(c) = existing {
            self.open_canvas(&c)?;
        } else {
            self.new_canvas(false)?;
            let title = prefix(&self.schedule_title(key), 60);
            let mut st = self.st_mut();
            let i = st.canvas_index();
            st.canvases[i].title = title;
            st.canvases[i].schedule = Some(key.into());
        }
        let schedule_here = self.st().boards().iter().any(|b| b.kind == "schedule");
        if from_schedule && !schedule_here { self.pin(Board::new("schedule", "schedule")); }
        // Its block on top, put back if it was closed.
        let at = self.st().boards().iter().position(|b| b.kind == "scheduleItem" && b.title == key);
        match at {
            Some(0) => {}
            Some(i) => self.set_boards(|b| {
                let item = b.remove(i);
                b.insert(0, item);
            }),
            None => { self.pin(Board::new("scheduleItem", key)); }
        }
        Ok(())
    }

    /// Back to the canvas it came from (or the newest other one); an item's canvas nothing was written on leaves the list.
    pub fn close_schedule(&self) -> Result<(), WalletError> {
        let Some(key) = self.schedule_key() else { return Ok(()) };
        let (id, written, back, other) = {
            let st = self.st();
            let back = st.schedule_return.clone().filter(|b| st.canvases.iter().any(|c| &c.id == b && c.schedule.is_none()));
            (st.canvas_id.clone(), st.canvases[st.canvas_index()].messages.iter().any(|m| m.from_user), back,
             st.canvases.iter().find(|c| c.schedule.is_none()).map(|c| c.id.clone()))
        };
        if let Some(back) = back { self.open_canvas(&back)? } else if let Some(other) = other { self.open_canvas(&other)? } else { self.new_canvas(false)? }
        self.st_mut().schedule_return = None;
        let still = self.st().canvases.iter().any(|c| c.id == id && c.schedule.as_deref() == Some(key.as_str()));
        if !written && still { self.remove_canvas(&id)? }
        Ok(())
    }

    /// «Run now»: due at once, within the minute's run (sooner if nothing else is going on).
    pub async fn run_now(&self, key: &str) {
        let Some(i) = self.st().tasks.iter().position(|t| format!("task:{}", t.id) == key) else { return };
        let (every, next, id) = { let st = self.st(); (st.tasks[i].every, st.tasks[i].next, st.tasks[i].id) };
        if every.is_some() && next > Utc::now() { self.st_mut().kept_next.insert(id.to_string(), next); }
        self.set_tasks(|t| t[i].next = Utc::now());
        self.st_mut().run_now_keys.insert(key.into());
        // After a run already going (one at a time), then this one.
        self.scheduled_run_ended().await;
        self.start_scheduled_run();
        self.scheduled_run_ended().await;
    }

    /// The item's block in words, for the model: what it is, when, how it signs, how its runs went.
    pub fn schedule_item_facts(&self, key: &str) -> Vec<String> {
        if let Some(coin) = key.strip_prefix("perp:") {
            let Some(p) = self.st().positions.iter().find(|p| p.coin == coin).cloned() else { return vec!["closed".into()] };
            return vec![format!("{} {} at {}, P/L {}; its stop and take are on the exchange", if p.size > Decimal::ZERO { "long" } else { "short" },
                                Fmt::num(p.size.abs(), 6), Fmt::price(p.entry), Fmt::usd(p.unrealized_pnl))];
        }
        let Some(t) = self.st().tasks.iter().find(|t| format!("task:{}", t.id) == key).cloned() else { return vec!["no longer scheduled".into()] };
        let id = t.id.to_string();
        let runs: Vec<String> = {
            let st = self.st();
            let mine: Vec<&ScheduleRun> = st.schedule_runs.iter().filter(|r| r.task.as_ref() == Some(&id)).collect();
            mine[mine.len().saturating_sub(5)..].iter().map(|r| format!("{}: {}", Fmt::date(r.at), r.result)).collect()
        };
        let signs = if self.uses_automation(&t) && t.moves_money() { "signs by itself from the automation wallet" } else { "each move waits for Confirm" };
        [self.task_text(&t), format!("steps: {}", t.steps.join(" then ")), signs.into()].into_iter().chain(runs).collect()
    }
}

#[cfg(test)]
#[path = "schedule/tests.rs"]
mod tests;

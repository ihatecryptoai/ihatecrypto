//! The Schedule (Schedule.swift): scheduled tasks and their runs, the automation wallet's budget, when a run may go.
//! What it keeps between runs («Run now» keys, the window in front, the field holding text, a task's next run put back,
//! the canvas an item's canvas was opened from) is in `State`'s run_now_keys … schedule_return.
//! Layer 2 adds here: start_scheduled_run, running due tasks, run_now, set_task, set_from_active, pause_schedule,
//! open_schedule, close_schedule, task_text, shown_step, recent_runs, the budget (automation_budget_now), top-up-asked.json.

use std::collections::HashMap;

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::engine::State;
use crate::history_tabs::Activity;
use crate::{json, platform};

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

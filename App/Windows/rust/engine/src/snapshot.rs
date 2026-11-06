//! The whole state the window draws, sent after every change (Engine.swift's `Engine.Snapshot`): the window's contract,
//! read by App/Windows/app/ui/*.js — the same keys, nesting and omissions as Swift's JSONEncoder.lines (a nil optional
//! left out, dates in ISO 8601). Layer 2 adds `Engine::snapshot()`, which fills it from every file's views.

use std::collections::HashMap;

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::Serialize;
use signer_api::{SignerAddress, SignerWallet};

use crate::automation::RecurringView;
use crate::blocks::{AddressExtra, ClosedTrade};
use crate::canvas::Board;
use crate::chat_turns::{ModelsView, QuoteView};
use crate::consent::SiteAskView;
use crate::engine::{Card, Message};
use crate::favorites::FavoriteToken;
use crate::folders::{CanvasIcon, FolderView};
use crate::hidden_tokens::TransitView;
use crate::history_moves::{MoveRow, MovesDay};
use crate::history_tabs::Activity;
use crate::json;
use crate::ledger::LedgerConnect;
use crate::local_models::LocalSetupView;
use crate::lock::LockView;
use crate::models::{ModelView, ProviderView};
use crate::modules::ModulesView;
use crate::moves::HistoryLine;
use crate::multisig::{Multisig, MultisigDraft, MultisigKey, MultisigNetHolding};
use crate::nodes::NodesView;
use crate::notices::NoticeView;
use crate::place_facts::PlaceFacts;
use crate::portfolio::PortfolioView;
use crate::route_edit::Edit;
use crate::schedule::{AutomationBudget, ScheduleRun};
use crate::top_up::TopUpView;
use crate::trade_plan::PositionRuler;
use crate::trading_agent::TradingView;
use crate::wallet_ops::StuckView;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub signer: Signer,
    pub wallets: Vec<SignerWallet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    pub addresses: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase: Option<Vec<String>>,
    pub portfolio: PortfolioView,
    pub positions: Vec<Position>,
    pub chat: Vec<Message>,
    /// What ↑↓ in the field walk: the sent messages without confirmations and repeats (the Mac's SentHistory.list).
    pub sent: Vec<String>,
    pub thinking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<SiteAskView>,
    pub options: Vec<OptionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<Running>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub model: ModelView,
    pub providers: Vec<ProviderView>,
    /// The model chosen for each account (the Mac's `ws.model(id)`): Settings' Model field shows and checks it.
    pub model_choices: HashMap<String, String>,
    pub second_check: bool,
    pub jev_open_router: bool,
    pub language: String,
    pub russian: bool,
    pub book: Vec<SignerAddress>,
    pub history: Vec<HistoryLine>,
    pub alerts: Vec<Alert>,
    pub tasks: Vec<TaskView>,
    /// The recurring buys, as their boards draw them (automation.rs recurringViews).
    pub recurring: Vec<RecurringView>,
    #[serde(rename = "jupiterDCA", skip_serializing_if = "Option::is_none")]
    pub jupiter_dca: Option<Board>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation: Option<SignerWallet>,
    pub skills: Vec<SkillView>,
    pub memory: Vec<String>,
    pub favorites: Vec<FavoriteToken>,
    pub canvases: Vec<CanvasRow>,
    pub folders: Vec<FolderView>,
    pub canvas: String,
    pub boards: Vec<Board>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running_board: Option<String>,
    pub multisigs: Vec<Multisig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multisig_draft: Option<MultisigDraft>,
    pub candidates: Vec<MultisigKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger_connect: Option<LedgerConnect>,
    pub starred: Vec<String>,
    pub watching: bool,
    pub watched: HashMap<String, String>,
    #[serde(rename = "automationUSD", skip_serializing_if = "Option::is_none", with = "number::option")]
    pub automation_usd: Option<Decimal>,
    /// The open positions' unrealized P/L together; None with none open.
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub positions_pnl: Option<Decimal>,
    /// «Limit waiting: long ETH at $2,400»: an entry on Hyperliquid's book, under its card (the Mac's pendingEntry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_waiting: Option<String>,
    /// Bridge orders signed and not landed yet (hidden_tokens.rs).
    pub in_transit: Vec<TransitView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    pub model_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_board: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<QuoteView>,
    pub models: ModelsView,
    /// The app's lock (lock.rs): whether a code is set, how soon it locks, a new code to offer.
    pub lock: LockView,
    /// Settings › Modules and Settings › Trading agent (modules.rs, trading_agent.rs).
    pub modules: ModulesView,
    pub trading: TradingView,
    pub nodes: NodesView,
    /// What the waiting move's step lets change (route_edit.rs Edit); None when nothing can.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_edit: Option<Edit>,
    pub schedule_paused: bool,
    pub schedule_runs: Vec<ScheduleRun>,
    pub multisig_held: HashMap<String, Vec<MultisigNetHolding>>,
    /// The open canvas's scheduled item («task:<id>», «perp:<coin>»), when it's that item's own canvas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule_key: Option<String>,
    /// The History block's moves, newest first, and what the latest day did to the money (history_moves.rs).
    pub moves: Vec<MoveRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moves_day: Option<MovesDay>,
    /// Nothing moved yet: the latest three requests as they were (the Mac's HistoryBoard).
    pub recent_requests: Vec<Activity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_budget: Option<AutomationBudget>,
    /// Dollars without the fee coin (wallet_ops.rs), for the onboarding's Top up.
    pub stuck: Vec<StuckView>,
    /// A place's details board beyond the rows: Hyperliquid in full, the deposits' rates (place_facts.rs).
    pub places: PlaceFacts,
    /// Settings › General › Notifications: each kind and whether it's on (notices.rs).
    pub notices: Vec<NoticeView>,
    /// Settings › Background and the onboarding's «Stay in touch»: ihatecrypto starts in the tray at login.
    pub open_at_login: bool,
    /// Top up's balance, gas and the money it watches for (top_up.rs); Settings › General › App language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_up: Option<TopUpView>,
    pub app_language: String,
    pub local_setup: LocalSetupView,
    /// Hyperliquid's trades closed in the last 7 days, newest first (blocks.rs), and the address book's pictures,
    /// categories and notes by «network:name».
    pub recently_closed: Vec<ClosedTrade>,
    pub address_extras: HashMap<String, AddressExtra>,
    /// Add block › Price alert while a trade plan is on the canvas: the alert for that plan (blocks.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_alert_example: Option<String>,
    /// The open position the canvas's plan is on: the chat's position chips show for it alone (chat_turns.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chips_position: Option<String>,
    /// The last full balance of the wallet that's on, shown dimmed until every network answers (the Mac's lastBalance).
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub last_balance: Option<Decimal>,
    /// Settings › About's «Build …» behind ⓘ: when this engine was built (`engine::build()`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// A newer version the updater found (notices.rs updateReady): Settings › About offers Install without a check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_ready: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signer {
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub notes: Vec<String>,
    pub waiting: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_length: Option<i64>,
    /// What the Ledger waits for on its screen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<String>,
}

/// `Snapshot.Option`: a card the last reply offers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OptionView {
    pub title: String,
    pub comment: String,
    pub recommended: bool,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Alert {
    pub id: String,
    pub text: String,
    /// The block's parts (AlertsBoard): the coin, «BTC above $120,000», «now $98,000 · 22.4% away», its THEN steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<String>>,
    /// Each step as its block draws it: a trade plan («THEN · PLAN ON HYPERLIQUID», Long ETH ×3) or a command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<AlertStep>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlertStep {
    pub plan: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillView {
    pub id: String,
    pub name: String,
    pub about: String,
    pub text: String,
    pub built_in: bool,
    pub on: bool,
    /// The Trading study's journal is kept (skills.rs): its «Journal» button opens it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journal: Option<bool>,
}

/// A move running now: its title and what it said so far (also the engine's `running`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Running {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Position {
    pub coin: String,
    pub side: String,
    pub size: String,
    pub entry: String,
    pub pnl: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liquidation: Option<String>,
    /// «0.05 at $2,412.50 · stop $2,300 · take $2,600» and the guardian's line (position_guard.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guardian: Option<String>,
    /// The position board's price ruler and guardian line (trade_plan.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ruler: Option<PositionRuler>,
    /// The exchange's own name, «xyz:TSLA», where `coin` reads «Tesla (xyz:TSLA)»: what a trade is matched by.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub text: String,
    pub paused: bool,
    pub automatic: bool,
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub next: Option<Date>,
    /// Its steps one by one, and how often («3 days»; None: once), for its own canvas's block.
    pub steps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    /// It can pay from the wallet that's on or the automation wallet («Pays from» on its block).
    pub pick: bool,
    /// Whether it moves money (a check or a lookup signs nothing), buys coins, sells; a spread's USDC free where it
    /// pays from (the Mac's task facts: MONEY, ENDS UP IN, PAYS FROM).
    pub moves: bool,
    pub buys: bool,
    pub sells: bool,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub free: Option<Decimal>,
    /// A spread's USDC free on the other payer (the wallet that's on ↔ the automation wallet), and whether the model
    /// wrote it (then it never pays by itself).
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub free_other: Option<Decimal>,
    pub untrusted: bool,
    /// A recurring buy (automation.rs recurringBuys): it runs at its time from the wallet that's on, Windows Hello only.
    pub recurring: bool,
}

/// Tokens the model read with the canvas's last message, and its window when the service or the catalog says. Of
/// `used`: the instructions and the chat's lines; how many lines went of how many; dollars an API key paid.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Context {
    pub used: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<i64>,
    pub instructions: i64,
    pub chat: i64,
    pub sent: i64,
    pub lines: i64,
    pub spent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<f64>,
    pub billed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasRow {
    pub id: String,
    pub title: String,
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub pinned_at: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<CanvasIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Nothing on it yet: the list shows it as the new canvas, kept once written on. `blocks`: something on its canvas.
    pub empty: bool,
    pub blocks: bool,
    /// Its trade plan's coin, drawn as the row's picture (the Mac's AssetBadge); `note`: it was written as a folder note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    pub note: bool,
}

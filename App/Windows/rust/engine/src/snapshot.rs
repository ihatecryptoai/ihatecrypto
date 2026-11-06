//! The whole state the window draws, sent after every change (Engine.swift's `Engine.Snapshot`): the window's contract,
//! read by App/Windows/app/ui/*.js — the same keys, nesting and omissions as Swift's JSONEncoder.lines (a nil optional
//! left out, dates in ISO 8601). Layer 2 adds `Engine::snapshot()`, which fills it from every file's views.

use std::collections::HashMap;

use ihatecrypto_core::chat_language::SentHistory;
use ihatecrypto_core::foundation::{Date, number};
use ihatecrypto_core::intent::TradeIntent;
use ihatecrypto_core::skills::Skill;
use llm::catalog;
use rust_decimal::Decimal;
use serde::Serialize;
use signer_api::{SignerAddress, SignerWallet};

use crate::automation::{RecurringView, TaskFacts};
use crate::blocks::{AddressExtra, ClosedTrade};
use crate::canvas::Board;
use crate::chat_turns::{ModelsView, QuoteView};
use crate::consent::SiteAskView;
use crate::engine::{Card, Engine, Message};
use crate::favorites::FavoriteToken;
use crate::folders::{CanvasIcon, FolderView};
use crate::hidden_tokens::TransitView;
use crate::history_moves::{MoveRow, MovesDay};
use crate::history_tabs::Activity;
use crate::json;
use crate::ledger::{LedgerAccount, LedgerConnect};
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
use crate::skills::journal_kept;
use crate::top_up::{TopUpPage, TopUpView};
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

// MARK: The state drawn (Engine.swift's `snapshot()`)

impl Engine {
    /// Everything the window draws, from every file's views. What's read straight from the state is read under one
    /// borrow, dropped before the views are asked for: each takes its own.
    pub fn snapshot(&self) -> Snapshot {
        let st = self.st();
        let mut addresses = HashMap::new();
        if let Some(s) = st.solana_address() { addresses.insert("solana".to_string(), s); }
        if let Some(e) = st.evm_address() {
            addresses.insert("evm".to_string(), e.clone());
            addresses.insert("hyperliquid".to_string(), e);
        }
        if let Some(n) = st.near_address() { addresses.insert("near".to_string(), n); }
        let status = st.status.as_ref();
        let signer = Signer {
            ready: status.is_some(), error: st.signer_error.clone(), notes: status.map(|s| s.checks.clone()).unwrap_or_default(),
            waiting: st.signing > 0 || st.ledger_prompt.is_some(), session_length: status.and_then(|s| s.session_length), ledger: st.ledger_prompt.clone(),
        };
        let mut wallets = status.and_then(|s| s.wallets.clone()).unwrap_or_default();
        if let Some(l) = st.ledger.as_ref().filter(|l| l.evm.is_some()) { wallets.push(l.wallet()) }
        let active = if st.ledger_active() { Some(LedgerAccount::ID.to_string()) } else { status.and_then(|s| s.active_wallet.clone()) };
        let sent: Vec<String> = st.messages().iter().filter(|m| m.from_user).map(|m| m.text.clone()).collect();
        let options = st.options.iter().map(|o| OptionView { title: o.title.clone(), comment: o.comment.clone(), recommended: o.recommended, command: o.command.clone() }).collect();
        let watched = [("solana", &st.settings.watch_solana), ("evm", &st.settings.watch_evm)].into_iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.to_string(), v))).collect();
        // Hyperliquid's numbers: a sum past Decimal's range shows none instead of panicking (lib.rs).
        let positions_pnl = if st.positions.is_empty() { None } else { st.positions.iter().try_fold(Decimal::ZERO, |sum, p| sum.checked_add(p.unrealized_pnl)) };
        let skills = Skill::built_ins().into_iter().chain(st.added_skills.iter().cloned()).map(|s| SkillView {
            on: !st.skills_off.contains(&s.id), journal: (s.id == Skill::trading_study().id).then(journal_kept),
            id: s.id, name: s.name, about: s.about, text: s.text, built_in: s.built_in,
        }).collect();
        let mut tasks = st.tasks.clone();
        tasks.sort_by_key(|t| t.next);
        let (phrase, portfolio, chat, thinking, draft, card) = (st.phrase.clone(), st.portfolio.clone(), st.messages().clone(), st.thinking, st.draft.clone(), st.card.clone());
        let (site, running) = (st.site_ask.as_ref().map(|a| a.view.clone()), st.running.clone());
        let plan = (st.intent != TradeIntent::default()).then(|| st.intent.command());
        let (jev_open_router, language) = (st.settings.jev_open_router == Some(true), st.settings.language.clone().unwrap_or_else(|| "auto".into()));
        let (russian, book, jupiter_dca, memory, favorites) = (st.app_russian(), st.book.clone(), st.jupiter_dca.clone(), st.memory.clone(), st.favorites.clone());
        let (canvas, running_board, multisigs, multisig_draft) = (st.canvas_id.clone(), st.running_board.clone(), st.multisigs.clone(), st.multisig_draft.clone());
        let (ledger_connect, starred, watching, automation_usd) = (st.ledger_connect.clone(), st.starred.iter().cloned().collect(), st.watching(), st.automation_usd);
        let (in_transit, context, model_running) = (st.in_transit.clone(), st.meters.get(&st.canvas_id).cloned(), st.model_running);
        let agent_step = if st.thinking { st.agent_step.clone() } else { None };
        let (options_message, retry_board) = (st.options_message.clone(), st.retry.as_ref().map(|r| r.board.clone()));
        let (schedule_paused, multisig_held, places) = (st.settings.schedule_paused == Some(true), st.multisig_held.clone(), st.place_facts.clone());
        let (open_at_login, app_language) = (st.settings.open_at_login == Some(true), st.settings.app_language.clone().unwrap_or_else(|| "system".into()));
        let (recently_closed, update_ready, alerts) = (st.recently_closed.clone(), st.available_update.clone(), st.alerts.clone());
        drop(st);

        let marks = self.recurring_buys();
        let automation = self.automation_wallet();
        let tasks = tasks.iter().map(|t| {
            let mark = marks.contains(&t.id);
            TaskView {
                id: t.id.to_string(), text: self.task_text(t), paused: t.paused == Some(true),
                automatic: !mark && self.uses_automation(t) && t.moves_money(), next: Some(t.next),
                steps: t.steps.iter().map(|step| self.shown_step(step)).collect(), every: t.every.map(|e| e.words()),
                pick: !mark && self.can_pick_pays(t), moves: t.moves_money(), buys: t.buys_coins(), sells: t.sells(),
                free: if t.spreads_usdc() { self.task_free_usdc(t) } else { None },
                free_other: if t.spreads_usdc() && automation.is_some() && t.untrusted != Some(true) { self.task_free_usdc(&self.other_payer(t)) } else { None },
                untrusted: t.untrusted == Some(true), recurring: mark,
            }
        }).collect();
        let plan_alert = self.plan_alert().map(|a| a.id);
        let alerts = alerts.iter().filter(|a| Some(a.id) != plan_alert).map(|a| self.alert_view(a)).collect();
        // Swift's Dictionary(_:uniquingKeysWith:) keeping the first.
        let mut model_choices = HashMap::new();
        for p in catalog::all() {
            let id = p.info().id;
            if !model_choices.contains_key(&id) { model_choices.insert(id.clone(), self.model_of(&id)); }
        }
        Snapshot {
            signer, wallets, active, addresses, phrase, portfolio, positions: self.position_views(),
            chat, sent: SentHistory::list(&sent), thinking, draft, card: card.map(|mut c| { c.title = self.with_amounts(&c.title); c }), site, options, running, plan,
            model: self.model_view(), providers: self.provider_views(), model_choices, second_check: self.second_check(), jev_open_router,
            language, russian, book, history: self.history(), alerts, tasks, recurring: self.recurring_views(), jupiter_dca,
            automation_usd: automation.as_ref().and(automation_usd), automation, skills, memory, favorites,
            canvases: self.canvas_rows(), folders: self.folder_views(), canvas, boards: self.with_chart_extras(self.boards_shown()),
            running_board, multisigs, multisig_draft, candidates: self.multisig_candidates(), ledger_connect, starred, watching, watched,
            positions_pnl, limit_waiting: self.limit_waiting_line(), in_transit, context, model_running, agent_step, options_message, retry_board,
            quote: self.quote_view(), models: self.models_view(), lock: self.lock_view(),
            modules: self.modules_view(), trading: self.trading_view(), nodes: self.nodes_view(), step_edit: self.step_edit(),
            schedule_paused, schedule_runs: self.recent_runs(), multisig_held,
            schedule_key: self.schedule_key(), moves: self.history_moves(), moves_day: self.moves_day(), recent_requests: self.recent_requests(),
            automation_budget: self.automation_budget_now(), stuck: self.stuck_view(), places, notices: self.notice_views(), open_at_login,
            top_up: TopUpPage::view(), app_language, local_setup: self.local_setup_view(),
            recently_closed, address_extras: self.address_extras(), plan_alert_example: self.plan_alert_example(), chips_position: self.chips_position(),
            last_balance: self.last_full_balance(), build: crate::engine::build(), update_ready,
        }
    }
}

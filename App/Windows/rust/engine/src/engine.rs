//! The engine and its stored state (Engine.swift): the window (Tauri, App/Windows/app) sends it what the person did, one
//! JSON line at a time, and gets back the whole state to draw after every change. Here: the chat's messages, what waits
//! for Confirm, every stored field (`State`), the computed properties that read them, the setters that save them, one
//! state push per step (`changed`) and the window's `Request`.
//! The window's requests go through dispatch.rs (`handle`, `run`, `watch`); what a state push draws is snapshot.rs's
//! `Engine::snapshot`.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::{Arc, LazyLock};

use evm::protocols::merkl::Reward;
use execution::trade_signer::TradeWallet;
use ihatecrypto_core::alerts::PriceAlert;
use ihatecrypto_core::chat_language::ChatLanguage;
use ihatecrypto_core::foundation::{Date, Uuid, distant_past};
use ihatecrypto_core::holdings::Holding;
use ihatecrypto_core::intent::{Fmt, TradeIntent};
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::pnl::{CostBasis, SolanaMove};
use ihatecrypto_core::position_command::{PositionCommand, WalletCommand};
use ihatecrypto_core::risk::RiskPolicy;
use ihatecrypto_core::scheduled_task::ScheduledTask;
use ihatecrypto_core::skills::Skill;
use llm::usage::ModelFacts;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::{SignerAddress, SignerLimits, SignerReply, SignerWallet};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use zeroize::Zeroizing;
use trade::executor::{Order, Position, Prepared};
use trade::stocks;

use crate::agent::AgentGoal;
use crate::blocks::{AddressExtra, ChartExtras, ClosedTrade};
use crate::browser::{ConnectedSites, SiteAsk};
use crate::canvas::{Board, CanvasDoc};
use crate::chains::ChainState;
use crate::chat_turns::{OfferView, SentContext};
use crate::consent::SiteTick;
use crate::contract_calls::CallReview;
use crate::favorites::FavoriteToken;
use crate::folders::FolderStore;
use crate::from_core::RouterOption;
use crate::hidden_tokens::TransitView;
use crate::history_tabs::HistoryStore;
use crate::ledger::{LedgerAccount, LedgerConnect};
use crate::local_models::LocalSetup;
use crate::lock::LockView;
use crate::move_routes::ContractNames;
use crate::multisig::{Multisig, MultisigDraft, MultisigNetHolding, MultisigPayment};
use crate::outbox::Outbox;
use crate::payments::PaymentQuote;
use crate::place_facts::PlaceFacts;
use crate::portfolio::PortfolioView;
use crate::route_edit::Way;
use crate::schedule::{AutomationBudget, ScheduleRun};
use crate::signer_link::SignerLink;
use crate::snapshot::{Context, Running, Snapshot};
use crate::store::{self, Settings};
use crate::trade_plan::PlanQuote;
use crate::{json, nodes, platform, task_seal};

// MARK: Messages and what waits for Confirm

/// One chat message, as the window shows it and the model reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub from_user: bool,
    pub text: String,
    /// Under the reply, small: which model, which guard, which network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(with = "json::date")]
    pub at: Date,
    /// A price chart drawn under the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<ChartLine>,
    /// What a block the person pointed at told the model with this message, shown over it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// The form the chat's model read a request as (the History's «understood as»).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub understood: Option<String>,
    /// The cards this reply offered and the one picked, a move's steps kept as one card, the blocks a message went with
    /// (chat_turns.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<OfferView>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contexts: Option<Vec<SentContext>>,
}

impl Message {
    /// `Message(fromUser:text:note:)`: a new id, now.
    pub fn new(from_user: bool, text: &str, note: Option<String>) -> Self {
        Message {
            id: Uuid::new().to_string(), from_user, text: text.into(), note, at: chrono::Utc::now(), chart: None, context: None,
            understood: None, options: None, picked: None, steps: None, contexts: None,
        }
    }
}

/// A market's closes over a range, for the window to draw as a line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartLine {
    pub title: String,
    pub closes: Vec<f64>,
    /// Each close's time, seconds since 1970: the chart's time axis and the time at the pointer (the Mac's priceAxes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<Vec<f64>>,
    pub low: String,
    pub high: String,
    pub change: String,
    /// Hyperliquid's name of the coin («ETH», «xyz:TSLA»): a trade plan of it is drawn on this chart (route_edit.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    /// The network a portfolio row opened it from: what's held of the coin is read there (the Mac's ChartState.network).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Read live for the window, never kept (blocks.rs withChartExtras): the market's numbers, levels, what's held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<ChartExtras>,
}

/// What waits for the Confirm button. Only the button carries one out (`confirm`): a typed «yes» never does.
#[derive(Debug, Clone)]
pub enum Pending {
    Wallet(WalletCommand),
    /// USDC to an address-book entry (`send N USDC to <name>`).
    Transfer(SignerAddress, Decimal),
    Position { command: PositionCommand, coin: String },
    Trade(Box<Prepared>),
    Address(SignerAddress),
    /// «withdraw 5 USDC to Solana then deposit 5 USDC to kamino»: one Confirm, the steps in order, the first failure stops them.
    Chain(Vec<ChainStep>),
    /// The first buy of a recurring one: the schedule exists only once this buy is confirmed and goes through.
    Recurring { command: WalletCommand, days: i64 },
    /// «stop 2%» after a trade: that position's stop and take moved on the exchange, the traded plan kept (trade_plan.rs).
    Orders { commands: Vec<PositionCommand>, coin: String, intent: TradeIntent },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChainStep {
    Wallet(WalletCommand),
    Transfer(SignerAddress, Decimal),
    /// A step run by its words once the moves before it are through: a chart, a position, a move asked «How much?» then
    /// (chains.rs).
    Said(String),
}

impl ChainStep {
    pub fn title(&self) -> String {
        match self {
            ChainStep::Wallet(c) => c.summary(),
            ChainStep::Transfer(e, amount) => format!("send {} USDC to {}", Fmt::num(*amount, 2), e.name),
            ChainStep::Said(text) => text.clone(),
        }
    }
}

/// The card the window draws for what's pending: what will happen, in lines, and what to look at first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    pub confirm: String,
    /// What Confirm needs ticked first (a trade the second check doubts, the Mac's acknowledgement; consent.rs).
    pub ticks: Vec<SiteTick>,
    /// A trade past a risk limit the person lets through («Ask me» or «Just warn»): the header says so.
    #[serde(rename = "overLimit", skip_serializing_if = "std::ops::Not::not")]
    pub over_limit: bool,
}

// MARK: The state's labelled pieces (Swift's labelled tuples)

/// What a coin cost, read from the chain by the amount held then.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainCost {
    pub held: Decimal,
    pub cost: CostBasis,
}

/// A caller of `refresh()` waiting for the read it queued: told once read `pass` has landed.
#[derive(Debug)]
pub struct ReadWaiter {
    pub pass: i64,
    pub done: oneshot::Sender<()>,
}

/// The cards offered and the words of the message they answered: a pick runs against those (consent.rs pick).
#[derive(Debug, Clone, PartialEq)]
pub struct OfferedFor {
    pub options: Vec<RouterOption>,
    pub text: String,
}

/// A Solana swap's output as its card showed it, in the token's units, and its step's words (quotes.rs).
#[derive(Debug, Clone, PartialEq)]
pub struct JupiterShown {
    pub command: WalletCommand,
    pub out: u64,
    pub title: String,
    pub route: String,
}

/// The failed move «Try again» lays out, on its block.
#[derive(Debug, Clone, PartialEq)]
pub struct Retry {
    pub board: String,
    pub command: WalletCommand,
}

/// How the last move ended («done», «failed»), for the request it was a step of.
#[derive(Debug, Clone, PartialEq)]
pub struct LastMove {
    pub at: Date,
    pub state: String,
}

/// A card left waiting on a canvas the user switched away from: it waits there a quarter of an hour.
#[derive(Debug, Clone)]
pub struct Parked {
    pub pending: Pending,
    pub card: Card,
    pub intent: TradeIntent,
    pub at: Date,
}

/// The way picked in a route step's details for the waiting trade (route_edit.rs), taken at its Confirm.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteWay {
    pub key: String,
    pub way: String,
}

/// Both ways of the waiting EVM trade as quoted for its step's details, the better first.
#[derive(Debug, Clone, PartialEq)]
pub struct WayQuotes {
    pub command: WalletCommand,
    pub ways: Vec<Way>,
}

/// What Merkl owes for the waiting card's claim or sweep, once read.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimOwed {
    pub board: String,
    pub command: WalletCommand,
    pub owed: Vec<Reward>,
}

/// A move the loss cap stopped (CostlyMove): offered again with that loss accepted («Send anyway»), never by itself.
#[derive(Debug, Clone)]
pub struct Costly {
    /// The option's own id, in its command (`__send anyway <id>`): a button kept on another canvas never sends this move.
    pub id: String,
    pub pending: Pending,
    pub title: String,
    pub lost: Decimal,
}

/// A coin's 4-hour ATR as a share of its price, and when it was read (trade_plan.rs).
#[derive(Debug, Clone, PartialEq)]
pub struct AtrKept {
    pub pct: Decimal,
    pub at: Date,
}

/// The plan's last dry bridge quote, by what it was asked for (trade_plan.rs).
#[derive(Debug, Clone)]
pub struct QuoteKept {
    pub key: String,
    pub quoted: PlanQuote,
    pub at: Date,
}

// MARK: The state

/// Every stored field of Swift's Engine, grouped as Engine.swift groups them, then what the other files kept in their
/// `static var`s. Read with `Engine::st()`, changed with `Engine::st_mut()` or the setters (lib.rs).
pub struct State {
    /// What the person chose (`set_settings`).
    pub settings: Settings,

    // Signer
    pub status: Option<SignerReply>,
    pub signer_error: Option<String>,
    pub book: Vec<SignerAddress>,
    pub signing: i64,
    /// Shown once, right after a new wallet was made, until the person says they checked it; wiped when dropped.
    pub phrase: Option<Zeroizing<Vec<String>>>,

    // Markets and balances
    pub markets: HashMap<String, Market>,
    pub portfolio: PortfolioView,
    pub positions: Vec<Position>,
    /// Hyperliquid's open orders: the positions' stops and takes, a limit waiting (position_guard.rs).
    pub hl_orders: Vec<Order>,
    /// A place's details beyond the portfolio's rows (place_facts.rs).
    pub place_facts: PlaceFacts,
    pub rates_at: Option<Date>,
    pub guarding: bool,
    pub ledger_guard_said: bool,
    /// Held while a TWAP of ours runs (an exit rule with its id): Windows doesn't idle-sleep, so the guardian sees a stop
    /// that fired (the Mac's twapAwake). keep_awake_for_twaps begins and ends it.
    pub twap_awake: bool,
    /// What the automation wallet holds, in dollars, read with the balances; None without it.
    pub automation_usd: Option<Decimal>,
    /// What each canvas's chat filled of its model's window with its last message (the Mac's ContextMeter), this run.
    pub meters: HashMap<String, Context>,
    pub refreshing: bool,
    /// A refresh asked for while one ran (a wallet switch's, a move's): it runs right after, never skipped.
    pub refresh_again: bool,
    /// The reads begun and the callers waiting for one: `refresh()` asked during a read returns once the read it
    /// queued has landed (portfolio.rs).
    pub reads_started: i64,
    pub read_waiters: Vec<ReadWaiter>,
    /// The Hyperliquid account `positions` and `hl_orders` were read for: the guardian acts only on those, never on a
    /// list that was never read (a first request that failed) or read for another wallet.
    pub positions_for: Option<String>,
    /// The last Solana read's tokens, named and priced: a read Jupiter didn't price takes them (the Mac's pricing(from:)).
    pub solana_holdings: Vec<Holding>,
    /// Since this wallet came on screen, kept through a read that failed (the Mac's solana / hl): Solana answered
    /// (priced or not) and Hyperliquid answered.
    pub seen_solana_priced: Option<bool>,
    pub seen_hl: bool,
    pub refreshing_every_minute: bool,
    /// The scheduled buys and tasks running now, beside the 30 s loop (startScheduledRun): one at a time.
    pub scheduled_run: Option<JoinHandle<()>>,
    /// When the user's last move ran: the balance change it made isn't announced as money arriving.
    pub last_own_move: Date,
    /// The wallet the balances on screen were read for.
    pub portfolio_owner: Option<String>,
    /// What each coin cost: read from the chain (by amount held then), recorded by the app's own buys, or first seen.
    pub chain_cost: HashMap<String, ChainCost>,
    /// costs.json, the wallet's own (`set_app_costs`).
    pub app_costs: HashMap<String, CostBasis>,
    /// seen.json (`set_seen_costs`).
    pub seen_costs: HashMap<String, CostBasis>,
    pub costs_loading: bool,
    /// A service's price waiting for Confirm («pay GET https://…»).
    pub payment: Option<PaymentQuote>,
    /// Skills the user added (`set_added_skills`), the ones switched off (`set_skills_off`), the ones loaded in this
    /// chat, and what they asked it to remember (`set_memory`).
    pub added_skills: Vec<Skill>,
    pub skills_off: BTreeSet<String>,
    pub loaded_skills: Vec<String>,
    pub memory: Vec<String>,
    /// `set_favorites`, `set_starred`.
    pub favorites: Vec<FavoriteToken>,
    pub starred: BTreeSet<String>,

    // Canvases: each has its own chat and blocks.
    pub canvases: Vec<CanvasDoc>,
    /// The canvas list's folders and their pictures (folders.rs; `set_folder_store`).
    pub folder_store: FolderStore,
    pub canvas_id: String,

    // Chat
    pub thinking: bool,
    /// The window's first run is on screen: the chat's own offers (new modules, the trading key, gas ways) wait until it
    /// ends, as the Mac's checks of its onboarding step. The window says when it starts and ends.
    pub onboarding: bool,
    pub draft: Option<String>,
    pub pending: Option<Pending>,
    pub card: Option<Card>,
    /// A site's request waiting in its own block, apart from the card (consent.rs).
    pub site_ask: Option<SiteAsk>,
    /// The cards offered and the words of the message they answered: a pick runs against those (Consent.pick).
    pub offered_for: Option<OfferedFor>,
    /// Hyperliquid's trades closed in the last 7 days (blocks.rs).
    pub recently_closed: Vec<ClosedTrade>,
    /// A Solana swap's output as its card showed it (quotes.rs; `set_jupiter_shown`).
    pub jupiter_shown: Option<JupiterShown>,
    /// The last reply's cards (didSet pinOptions: chat_turns.rs's setter).
    pub options: Vec<RouterOption>,
    /// The chat's turn (chat_turns.rs): the model call Stop can cancel, the lookup under way, the reply the live cards
    /// belong to, the failed move «Try again» lays out, when the waiting swap was priced, the other blocks a message goes with.
    pub model_running: bool,
    pub stop_requested: bool,
    pub cancel_model: Option<Box<dyn Fn()>>,
    pub agent_step: Option<String>,
    pub options_message: Option<String>,
    pub retry: Option<Retry>,
    pub quoted_at: Option<Date>,
    /// Bumped each time the waiting move's quotes are asked anew (quotes.rs quote_card, the Mac's quoteRound).
    pub quote_round: i64,
    pub also_attached: Vec<Board>,
    /// The model menu's lists (live-models.json, `set_live_models`): a subscription's and the local servers' models,
    /// whether those answer, sizes, downloads.
    pub live_models: HashMap<String, Vec<String>>,
    pub local_servers: HashMap<String, bool>,
    /// Settings › AI model's model check and installers, while they run (local_models.rs).
    pub local_setup: LocalSetup,
    pub model_sizes: HashMap<String, f64>,
    pub model_downloads: HashMap<String, f64>,
    pub model_facts: HashMap<String, ModelFacts>,
    pub model_facts_asked: Date,
    pub running: Option<Running>,
    /// N5: a NEAR Intents move after Confirm, in the chat's field until the next message (journey.rs).
    pub journey: Option<crate::journey::Journey>,
    /// The block of the move that runs now.
    pub running_board: Option<String>,
    /// The block the message being read is about.
    pub attached: Option<Board>,
    /// Sites the browser connected in this run.
    pub sites: ConnectedSites,
    /// Multisigs made here (multisigs.json, saved where they change), the set-up card's choices, and a payment out of
    /// one waiting for Confirm.
    pub multisigs: Vec<Multisig>,
    pub multisig_draft: Option<MultisigDraft>,
    pub multisig_payment: Option<MultisigPayment>,
    /// A request the model carries on with after its step, and whether the last check found nothing to tell.
    pub agent_goal: Option<AgentGoal>,
    pub quiet_check: bool,
    pub last_move: Option<LastMove>,
    /// A contract call checked and simulated, waiting for Confirm.
    pub call_review: Option<CallReview>,
    /// The Ledger's addresses (ledger.json, `set_ledger`), its connect card, and what it waits for on its screen now.
    pub ledger: Option<LedgerAccount>,
    pub ledger_connect: Option<LedgerConnect>,
    pub ledger_prompt: Option<String>,
    pub awaiting_amount: Option<WalletCommand>,
    /// A chain's «How much?» and the steps it holds (chains.rs).
    pub chain_state: ChainState,
    /// A recurring buy whose two ways were offered: picked again, it goes straight (move_checks.rs).
    pub dca_choice: Option<WalletCommand>,
    /// A new Hyperliquid account's way in, as picked on its cards: NEAR Intents (true) or Relay (false); None not asked yet.
    pub open_via_1click: Option<bool>,
    pub parked: HashMap<String, Parked>,
    pub route_way: Option<RouteWay>,
    /// `set_way_quotes`.
    pub way_quotes: Option<WayQuotes>,
    /// A Solana swap Jupiter can't price (a token it can't name, an amount under one unit): Confirm as always, not held
    /// for a quote that never comes.
    pub unpriced: Option<WalletCommand>,
    /// When the plan's take-profit ways were last offered: the History freezes them only into the request that did.
    pub takes_at: Option<Date>,
    /// What Merkl owes for the waiting card's claim or sweep: that move's route and footer (move_routes.rs quoteRoute).
    pub claim_owed: Option<ClaimOwed>,
    /// The Jupiter DCA orders as last read this session: drawn on every canvas with no plan (earn.rs); None when none run.
    pub jupiter_dca: Option<Board>,
    /// This wallet's open limit orders on 1Click as last read this session (earn.rs); None when none are open.
    pub limit_orders: Option<Board>,
    /// The orders behind that board and the wallet (its Solana address) they're this one's for: what the loop follows.
    pub limit_views: Option<(String, Vec<trade::limit_order::LimitOrderView>)>,
    /// The wallet whose limit orders the loop has read once since launch or the switch to it (earn.rs follow_limit_orders).
    pub limit_followed: Option<String>,
    /// Orders that left the board, until the journal holds their payout as settled: id → ticks left to ask.
    pub limit_payouts: HashMap<String, u32>,
    /// A follow runs: the 30 s loop starts no second one.
    pub limit_following: bool,
    /// The Ledger's Hyperliquid trading key, and whether the account approved ihatecrypto's fee (trading_key.rs).
    pub ledger_agent: Option<SignerReply>,
    pub fee_approved: Option<bool>,
    pub costly: Option<Costly>,
    /// Bridge orders on their way, read with the balances (hidden_tokens.rs).
    pub in_transit: Vec<TransitView>,
    /// The trade plan the chat is building: a perp on Hyperliquid.
    pub intent: TradeIntent,
    /// The plan's board on the canvas, stopped, previewed or the card (trade_plan.rs); the plan of the trade last opened,
    /// which «stop 2%» edits while its position is open.
    pub plan_board: Option<String>,
    pub traded_plan: Option<TradeIntent>,
    /// Price alerts (alerts.json, `set_alerts`), checked against Hyperliquid's marks at every refresh.
    pub alerts: Vec<PriceAlert>,
    /// Loans whose health factor is read with every refresh (loanguards.json, `set_loan_guards`, loan_guards.rs), and
    /// each one's last read for the alerts block: no entry before the first read, None without a debt.
    pub loan_guards: Vec<ihatecrypto_core::loan_guard::LoanGuard>,
    pub guard_health: HashMap<Uuid, Option<Decimal>>,
    /// Scheduled tasks and recurring buys, run by the minute: the wallet that's on has its own, loaded with each word
    /// from the signer (loadWalletState; `set_tasks` seals them).
    pub tasks: Vec<ScheduledTask>,
    /// What each scheduled run did, for the Schedule's History tab (schedule-runs.json, `set_schedule_runs`).
    pub schedule_runs: Vec<ScheduleRun>,
    /// Each multisig's money by network (address → networks), read with the balances.
    pub multisig_held: HashMap<String, Vec<MultisigNetHolding>>,
    /// The automation wallet's money as last read, for the Schedule's budget.
    pub automation_budget: Option<AutomationBudget>,
    /// The app's lock (lock.rs), and whether the signer has said if a code is set since launch.
    pub lock: LockView,
    pub lock_heard_signer: bool,

    // What the other files kept in `static var`s
    /// Alerts.swift: the alerts' prices are watched every 10 s while there are any.
    pub watching_alerts: bool,
    /// Automation.swift: recurring buys told once that they wait for the Ledger.
    pub ledger_wait_notified: HashSet<Uuid>,
    /// Blocks.swift: Robinhood's stock tokens, read once; the address book's pictures, categories and notes.
    pub stock_registry: Option<Vec<stocks::Token>>,
    pub extras_cache: Option<HashMap<String, AddressExtra>>,
    /// Chat.swift's `seen`: the cards the model was shown.
    pub chat_seen: Vec<RouterOption>,
    /// ChatCommands.swift's WalletScope: whose per-wallet records are in memory; None is the first wallet's.
    pub wallet_scope_costs: Option<String>,
    pub wallet_scope_tasks: Option<String>,
    /// HistoryTabs.swift's HistoryStore: the History's own record (history.json).
    pub history: HistoryStore,
    /// Lock.swift: the signing session was ended for time away or a locked session.
    pub session_ended_for_lock: bool,
    /// MoveRoutes.swift: contracts' symbols looked up for a route's labels.
    pub contract_names: ContractNames,
    /// Notices.swift: the positions and limit entries last seen, when the last notice went, the update found.
    pub notice_positions: Option<HashSet<String>>,
    pub notice_entries: HashSet<String>,
    pub notice_last: Date,
    pub available_update: Option<String>,
    /// Payments.swift: services whose price came out different once they saw the request's content.
    pub priced_with_content: HashSet<String>,
    /// PositionGuard.swift: the last reason the guardian couldn't move a coin's stop, said once.
    pub guard_refused: HashMap<String, String>,
    /// Moves.swift: what a save from the chat keeps beside the address once it's saved.
    pub saving_extras: HashMap<String, AddressExtra>,
    /// Schedule.swift: «Run now» keys, the window in front and since when it isn't, the field holding half-typed text,
    /// a repeating task's next run put back after «Run now», the canvas an item's canvas was opened from.
    pub run_now_keys: HashSet<String>,
    pub window_front: bool,
    pub window_left: Date,
    pub composing: bool,
    pub kept_next: HashMap<String, Date>,
    pub schedule_return: Option<String>,
    /// TradePlan.swift: each coin's ATR, and the plan's last bridge quote.
    pub atr_kept: HashMap<String, AtrKept>,
    pub quote_kept: Option<QuoteKept>,
    /// WalletOps.swift: a new wallet is being made.
    pub creating_wallet: bool,
    /// Costs.swift's SolanaMoves: each Solana transaction read once, by signature.
    pub solana_moves: HashMap<String, SolanaMove>,
}

impl State {
    /// Every stored file as the engine starts (Engine.swift's property initializers).
    pub fn load() -> Self {
        let settings: Settings = store::load("settings.json").unwrap_or_default();
        let canvases = crate::canvas::load_canvases();
        let canvas_id = settings.canvas.clone().filter(|id| canvases.iter().any(|c| &c.id == id)).unwrap_or_else(|| canvases[0].id.clone());
        State {
            settings,
            status: None, signer_error: None, book: vec![], signing: 0, phrase: None,
            markets: HashMap::new(), portfolio: PortfolioView::default(), positions: vec![], hl_orders: vec![], place_facts: PlaceFacts::default(),
            rates_at: None, guarding: false, ledger_guard_said: false, twap_awake: false, automation_usd: None, meters: HashMap::new(), refreshing: false,
            refresh_again: false, reads_started: 0, read_waiters: vec![], positions_for: None, solana_holdings: vec![], seen_solana_priced: None,
            seen_hl: false, refreshing_every_minute: false, scheduled_run: None, last_own_move: distant_past(), portfolio_owner: None,
            chain_cost: HashMap::new(),
            app_costs: store::load("costs.json").unwrap_or_default(),
            seen_costs: store::load("seen.json").unwrap_or_default(),
            costs_loading: false, payment: None,
            added_skills: store::load("skills.json").unwrap_or_default(),
            skills_off: store::load::<Vec<String>>("skills-off.json").unwrap_or_default().into_iter().collect(),
            loaded_skills: vec![],
            memory: store::load("memory.json").unwrap_or_default(),
            favorites: store::load("favorites.json").unwrap_or_default(),
            starred: store::load::<Vec<String>>("starred.json").unwrap_or_default().into_iter().collect(),
            canvases, folder_store: store::load("folders.json").unwrap_or_default(), canvas_id,
            thinking: false, onboarding: false, draft: None, pending: None, card: None, site_ask: None, offered_for: None, recently_closed: vec![],
            jupiter_shown: None, options: vec![], model_running: false, stop_requested: false, cancel_model: None, agent_step: None,
            options_message: None, retry: None, quoted_at: None, quote_round: 0, also_attached: vec![],
            live_models: store::load("live-models.json").unwrap_or_default(),
            local_servers: HashMap::new(), local_setup: LocalSetup::default(), model_sizes: HashMap::new(), model_downloads: HashMap::new(),
            model_facts: HashMap::new(), model_facts_asked: distant_past(), running: None, journey: None, running_board: None, attached: None,
            sites: ConnectedSites::default(),
            multisigs: store::load("multisigs.json").unwrap_or_default(),
            multisig_draft: None, multisig_payment: None, agent_goal: None, quiet_check: false, last_move: None, call_review: None,
            ledger: store::load("ledger.json"),
            ledger_connect: None, ledger_prompt: None, awaiting_amount: None, chain_state: ChainState::default(), dca_choice: None,
            open_via_1click: None, parked: HashMap::new(), route_way: None, way_quotes: None, unpriced: None, takes_at: None, claim_owed: None,
            jupiter_dca: None, limit_orders: None, limit_views: None, limit_followed: None, limit_payouts: HashMap::new(), limit_following: false, ledger_agent: None, fee_approved: None, costly: None, in_transit: vec![], intent: TradeIntent::default(),
            plan_board: None, traded_plan: None,
            alerts: store::load("alerts.json").unwrap_or_default(),
            loan_guards: store::load("loanguards.json").unwrap_or_default(), guard_health: HashMap::new(),
            tasks: task_seal::load(task_seal::TASKS, task_seal::KEY).0,
            schedule_runs: store::load("schedule-runs.json").unwrap_or_default(),
            multisig_held: HashMap::new(), automation_budget: None, lock: LockView::default(), lock_heard_signer: false,
            watching_alerts: false, ledger_wait_notified: HashSet::new(), stock_registry: None, extras_cache: None, chat_seen: vec![],
            wallet_scope_costs: None, wallet_scope_tasks: None, history: HistoryStore::load(), session_ended_for_lock: false,
            contract_names: ContractNames::default(), notice_positions: None, notice_entries: HashSet::new(), notice_last: distant_past(),
            available_update: None, priced_with_content: HashSet::new(), guard_refused: HashMap::new(), saving_extras: HashMap::new(),
            run_now_keys: HashSet::new(), window_front: true, window_left: chrono::Utc::now(), composing: false, kept_next: HashMap::new(),
            schedule_return: None, atr_kept: HashMap::new(), quote_kept: None, creating_wallet: false, solana_moves: HashMap::new(),
        }
    }

    // MARK: The open canvas

    pub fn canvas_index(&self) -> usize { self.canvases.iter().position(|c| c.id == self.canvas_id).unwrap_or(0) }

    /// The open canvas's chat.
    pub fn messages(&self) -> &Vec<Message> { &self.canvases[self.canvas_index()].messages }
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        let i = self.canvas_index();
        &mut self.canvases[i].messages
    }

    /// The open canvas's blocks (canvas.rs's `set_boards` saves a change).
    pub fn boards(&self) -> &Vec<Board> { &self.canvases[self.canvas_index()].boards }

    // MARK: The wallet

    /// The Ledger signs now (ledger.rs).
    pub fn ledger_active(&self) -> bool { self.ledger.as_ref().is_some_and(|l| l.active == Some(true) && l.evm.is_some()) }

    /// The wallet that signs now: the connected Ledger when it's on, else the signer's.
    pub fn active_wallet(&self) -> Option<SignerWallet> {
        if self.ledger_active() && let Some(l) = &self.ledger { return Some(l.wallet()) }
        let wallets = self.status.as_ref()?.wallets.as_ref()?;
        let active = self.status.as_ref()?.active_wallet.as_ref();
        wallets.iter().find(|w| Some(&w.id) == active).or(wallets.first()).cloned()
    }

    /// No wallet here, only addresses watched for their balances: they're read, and nothing signs for them.
    pub fn watching(&self) -> bool {
        !self.ledger_active() && self.status.as_ref().and_then(|s| s.has_wallet) != Some(true)
            && (self.settings.watch_solana.is_some() || self.settings.watch_evm.is_some())
    }

    pub fn solana_address(&self) -> Option<String> {
        if self.ledger_active() { self.ledger.as_ref()?.solana.clone() }
        else if self.watching() { self.settings.watch_solana.clone() }
        else { self.status.as_ref()?.solana_address.clone() }
    }

    pub fn evm_address(&self) -> Option<String> {
        if self.ledger_active() { self.ledger.as_ref()?.evm.clone() }
        else if self.watching() { self.settings.watch_evm.clone() }
        else { self.status.as_ref()?.evm_address.clone() }
    }

    pub fn near_address(&self) -> Option<String> { if self.ledger_active() { None } else { self.status.as_ref()?.near_address.clone() } }

    /// The wallet moves are signed for. A key wallet leaves the other chain's address empty, as the Mac's signerWallet:
    /// its balances there show nothing, and a move on that chain says why (missingKey, the Executor's own checks).
    pub fn trade_wallet(&self) -> Option<TradeWallet> {
        let (s, e) = (self.solana_address(), self.evm_address());
        if s.is_none() && e.is_none() { None } else { Some(TradeWallet { solana: s.unwrap_or_default(), evm: e.unwrap_or_default() }) }
    }

    /// The addresses balances are read for: a chain without one ("") isn't read.
    pub fn read_wallet(&self) -> Option<TradeWallet> { self.trade_wallet() }

    pub fn watch_only(&self) -> String {
        self.say("These addresses are only watched: moving money or signing needs a wallet here (Settings › Wallets).",
                 "Эти адреса только для просмотра: чтобы переводить или подписывать, добавь кошелёк (Настройки › Кошельки).")
    }

    // MARK: Language

    /// The person's last four messages, oldest first: what the chat's language follows on «auto».
    fn recent_user_messages(&self) -> Vec<String> {
        let mine: Vec<&Message> = self.messages().iter().filter(|m| m.from_user).collect();
        mine[mine.len().saturating_sub(4)..].iter().map(|m| m.text.clone()).collect()
    }

    pub fn russian(&self) -> bool {
        match self.settings.language.as_deref() {
            Some("ru") => true,
            Some("en" | "es") => false,
            _ => ChatLanguage::Auto.is_russian(&self.recent_user_messages()),
        }
    }

    pub fn say(&self, english: &str, russian: &str) -> String { if self.russian() { russian } else { english }.into() }

    /// The lines a conversation meets most, in Spanish too (the Mac's say with `spanish`): a Spanish chat isn't answered
    /// in English there. Russian first, as the Mac's.
    pub fn say3(&self, english: &str, russian: &str, spanish: &str) -> String {
        if self.russian() { russian } else if self.reply_language() == "Spanish" { spanish } else { english }.into()
    }

    /// The window's own texts (Settings › General › App language), apart from the replies' language: System follows
    /// Windows' language, as the Mac's follows the Mac's.
    pub fn app_russian(&self) -> bool {
        match self.settings.app_language.as_deref() {
            Some("ru") => true,
            Some("en") => false,
            _ => platform::prefers_russian(),
        }
    }

    pub fn reply_language(&self) -> String {
        match self.settings.language.as_deref() {
            Some("ru") => "Russian".into(),
            Some("en") => "English".into(),
            Some("es") => "Spanish".into(),
            _ => ChatLanguage::Auto.name(&self.recent_user_messages()).into(),
        }
    }
}

// MARK: The engine

/// What a state push draws while the app is open: layer 2's `Engine::snapshot`.
pub type Draw = fn(&Engine) -> Snapshot;

/// The desktop app's engine: the window sends it what the person did and gets back the whole state to draw after every
/// change. It reads the chat with the model through ChatRouter, plans and checks moves with the same core and trade as
/// the Mac, and signs only through the signing process, after Confirm.
pub struct Engine {
    me: Weak<Engine>,
    pub(crate) out: Outbox,
    pub(crate) signer: Arc<SignerLink>,
    draw: Draw,
    state: RefCell<State>,
    push_queued: Cell<bool>,
}

impl Engine {
    /// The engine with every stored file read, the settings handed to trade, the own nodes in use and the lock set as
    /// the last run left it, and the open canvas's plan, skills, model and cards back as they were (canvas.rs
    /// `restore_canvas_state`, as Swift's init). Its watch starts: call inside a `LocalSet`.
    pub fn new(out: Outbox, signer: Arc<SignerLink>, draw: Draw) -> Rc<Self> {
        let state = State::load();
        state.settings.mirror();
        nodes::apply_nodes();
        // The window keeps the signer's crash with its own (system.rs write_crash), for the line at the next launch.
        let o = out.clone();
        signer.on_stopped(Arc::new(move |why| o.event("signerCrashed", &HashMap::from([("text", why)]))));
        let engine = Rc::new_cyclic(|me| Engine { me: me.clone(), out, signer, draw, state: RefCell::new(state), push_queued: Cell::new(false) });
        engine.lock_at_launch();
        engine.restore_canvas_state();
        engine
    }

    /// An owned handle, for a task that outlives the call.
    pub fn rc(&self) -> Rc<Self> { self.me.upgrade().expect("the engine outlives its calls") }

    /// The stored fields, read. Never held across an `.await`.
    pub fn st(&self) -> Ref<'_, State> { self.state.borrow() }

    /// The stored fields, changed. Never held across an `.await`, and dropped before another method borrows.
    pub fn st_mut(&self) -> RefMut<'_, State> { self.state.borrow_mut() }

    pub fn say(&self, english: &str, russian: &str) -> String { self.st().say(english, russian) }
    pub fn say3(&self, english: &str, russian: &str, spanish: &str) -> String { self.st().say3(english, russian, spanish) }

    // MARK: State to the window

    /// Something the window draws changed: one state goes out after the current step, however many changes it made.
    pub fn changed(&self) {
        if self.push_queued.replace(true) { return }
        let me = self.rc();
        tokio::task::spawn_local(async move {
            tokio::task::yield_now().await;
            me.push_queued.set(false);
            // Locked: the window gets the lock alone, never the balances, the chat or the history.
            if me.st().lock.locked { me.out.event("state", &me.locked_snapshot()) } else { me.out.event("state", &(me.draw)(&me)) }
        });
    }

    pub fn reply(&self, text: &str, note: Option<String>) {
        self.st_mut().messages_mut().push(Message::new(false, text, note));
        self.save_chat();
        self.changed();
    }

    pub fn save_chat(&self) { self.save_canvases() }

    // MARK: Setters: Swift's didSet

    /// A field kept in its own file: changed, then saved.
    fn kept<T: Serialize + ?Sized>(&self, field: fn(&mut State) -> &mut T, file: &str, change: impl FnOnce(&mut T)) {
        let mut st = self.st_mut();
        let value = field(&mut st);
        change(value);
        store::save(&*value, file);
    }

    pub fn set_settings(&self, change: impl FnOnce(&mut Settings)) {
        {
            let mut st = self.st_mut();
            change(&mut st.settings);
            store::save(&st.settings, "settings.json");
            st.settings.mirror();
        }
        self.changed();
    }

    /// The wallet that's on's own costs.json (`costs-<wallet>.json` past the first).
    pub fn set_app_costs(&self, change: impl FnOnce(&mut HashMap<String, CostBasis>)) {
        let mut st = self.st_mut();
        change(&mut st.app_costs);
        store::save(&st.app_costs, &store::scoped("costs.json", st.wallet_scope_costs.as_deref()));
    }

    pub fn set_seen_costs(&self, change: impl FnOnce(&mut HashMap<String, CostBasis>)) { self.kept(|s| &mut s.seen_costs, "seen.json", change) }
    pub fn set_added_skills(&self, change: impl FnOnce(&mut Vec<Skill>)) { self.kept(|s| &mut s.added_skills, "skills.json", change) }
    /// Saved sorted, as Swift saves `skillsOff.sorted()`.
    pub fn set_skills_off(&self, change: impl FnOnce(&mut BTreeSet<String>)) { self.kept(|s| &mut s.skills_off, "skills-off.json", change) }
    pub fn set_memory(&self, change: impl FnOnce(&mut Vec<String>)) { self.kept(|s| &mut s.memory, "memory.json", change) }
    pub fn set_live_models(&self, change: impl FnOnce(&mut HashMap<String, Vec<String>>)) { self.kept(|s| &mut s.live_models, "live-models.json", change) }
    pub fn set_alerts(&self, change: impl FnOnce(&mut Vec<PriceAlert>)) { self.kept(|s| &mut s.alerts, "alerts.json", change) }
    pub fn set_schedule_runs(&self, change: impl FnOnce(&mut Vec<ScheduleRun>)) { self.kept(|s| &mut s.schedule_runs, "schedule-runs.json", change) }

    pub fn set_favorites(&self, change: impl FnOnce(&mut Vec<FavoriteToken>)) {
        self.kept(|s| &mut s.favorites, "favorites.json", change);
        self.changed();
    }

    pub fn set_starred(&self, change: impl FnOnce(&mut BTreeSet<String>)) {
        self.kept(|s| &mut s.starred, "starred.json", change);
        self.changed();
    }

    pub fn set_folder_store(&self, change: impl FnOnce(&mut FolderStore)) {
        self.kept(|s| &mut s.folder_store, "folders.json", change);
        self.changed();
    }

    /// ledger.json; nil writes `null`, as Swift's encoder does an absent optional at the top.
    pub fn set_ledger(&self, change: impl FnOnce(&mut Option<LedgerAccount>)) {
        self.kept(|s| &mut s.ledger, "ledger.json", change);
        self.changed();
    }

    /// The wallet that's on's tasks, sealed (task_seal.rs).
    pub fn set_tasks(&self, change: impl FnOnce(&mut Vec<ScheduledTask>)) {
        let mut st = self.st_mut();
        change(&mut st.tasks);
        task_seal::save(&st.tasks, &store::scoped(task_seal::TASKS, st.wallet_scope_tasks.as_deref()), task_seal::KEY);
    }

    /// A shown swap marks when it was priced.
    pub fn set_jupiter_shown(&self, shown: Option<JupiterShown>) {
        let mut st = self.st_mut();
        if shown.is_some() { st.quoted_at = Some(chrono::Utc::now()) }
        st.jupiter_shown = shown;
    }

    /// Quoted ways mark when they were priced.
    pub fn set_way_quotes(&self, quotes: Option<WayQuotes>) {
        let mut st = self.st_mut();
        if quotes.as_ref().is_some_and(|q| !q.ways.is_empty()) { st.quoted_at = Some(chrono::Utc::now()) }
        st.way_quotes = quotes;
    }
}

/// The engine program's own build time, «20261024.0831»: Windows has no bundle build number to show.
pub fn build() -> Option<String> {
    static BUILD: LazyLock<Option<String>> = LazyLock::new(|| {
        let at = std::fs::metadata(std::env::current_exe().ok()?).ok()?.modified().ok()?;
        Some(chrono::DateTime::<chrono::Local>::from(at).format("%Y%m%d.%H%M").to_string())
    });
    BUILD.clone()
}

// MARK: Requests from the window

/// «Ask in chat» on something that isn't a block (a coin, a frame): its name and what the model is told about it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct About {
    pub title: String,
    pub about: String,
}

/// One line from the window: an `op` and what it needs. A line that doesn't read as one is skipped (main.swift).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: Option<i64>,
    pub op: String,
    pub text: Option<String>,
    pub index: Option<i64>,
    pub test: Option<bool>,
    pub phrase: Option<String>,
    /// A recovery phrase's BIP-39 passphrase, if its wallet has one.
    pub passphrase: Option<String>,
    pub key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub wallet: Option<String>,
    pub label: Option<String>,
    pub name: Option<String>,
    pub network: Option<String>,
    pub address: Option<String>,
    pub memo: Option<String>,
    pub language: Option<String>,
    pub seconds: Option<i64>,
    pub board: Option<String>,
    pub threshold: Option<i64>,
    pub solana: Option<String>,
    pub evm: Option<String>,
    /// Every block the message is about, the newest last (the chat's chips).
    pub boards: Option<Vec<String>>,
    /// The step of `board` its menu's «Send to chat» attached: that step goes with the message, not the whole block.
    pub step: Option<i64>,
    pub abouts: Option<Vec<About>>,
    /// Settings › Trading agent's rules, and a number (the contract cap in dollars).
    pub policy: Option<RiskPolicy>,
    pub value: Option<f64>,
    /// A site's request approved with these ticks (`siteAnswer`, consent.rs).
    pub ticks: Option<Vec<String>>,
    /// Settings › Wallet › Limits as the person set them (`limits`).
    pub limits: Option<SignerLimits>,
}

impl Request {
    /// A line from standard input, or None when it isn't a request (main.swift skips it).
    pub fn read(line: &str) -> Option<Request> { serde_json::from_str(line).ok() }
}

/// A question that only reads changes nothing: a new state after it would make the browser's toolbar ask again (each
/// state redraws it, and a redraw asks siteState), six times a second, dropping what's being typed.
pub const ANSWERS_ONLY: [&str; 14] = [
    "siteState", "historyTabs", "transitState", "blockCatalog", "hlFunded", "checkAddress", "planMarkets", "windowFocus", "composing", "onboarding",
    "openStudyFiles", "keyAddress", "phraseAccounts", "multisigFees",
];

//! Canvases and their blocks (Canvas.swift): the types kept in canvases.json, and keeping them — the canvases read at
//! launch, the open one's state written into it, saved with the History's copy, a block changed in place.
//! Layer 2: pinning a block, new/open/rename/remove canvas, leaving and switching (a waiting card parked, the chat's
//! state kept with its canvas and back), a stale chart read again, the first words naming a canvas, a block in words
//! for the model, and a move's route as cards (`steps`).

use std::collections::{BTreeMap, HashSet};

use ihatecrypto_core::charts::Range;
use ihatecrypto_core::foundation::{Date, Uuid};
use ihatecrypto_core::intent::{Fmt, TradeIntent};
use ihatecrypto_core::position_command::{PositionCommand, WalletCommand};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use wallet::{Result, WalletError};

use crate::blocks::BlockData;
use crate::engine::{ChainStep, ChartLine, Engine, Message, Pending};
use crate::folders::CanvasIcon;
use crate::from_core::RouterOption;
use crate::json;
use crate::route_edit::TradeView;
use crate::store;
use crate::trade_plan::PlanView;

/// A block on a canvas: a move's route (waiting, running, done), a result the chat made (a chart, the rates, approvals,
/// P/L, a token), or a live block the person added (the portfolio, Receive, the address book…). The newest is on top,
/// and nothing leaves a canvas unless the person closes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    /// «move», «result», «chart», or a live block: portfolio, receive, addresses, history, favorites, alerts, schedule,
    /// positions, multisig.
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<Step>>,
    /// A move's: waiting, running, done, failed, cancelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart: Option<ChartLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<Action>>,
    #[serde(with = "json::date")]
    pub at: Date,
    /// What the route signs and how it's checked, as the Mac's MoveRoute footer: «Why this way?» quotes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A trade plan's levels and numbers, for drawing it on its coin's chart (route_edit.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trade: Option<TradeView>,
    /// A note's card shown on another canvas: the note canvas «Open» goes to (chat_commands.rs folderCommand).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opens: Option<String>,
    /// What a result board draws, as data (blocks.rs); `lines` stay what the model reads about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<BlockData>,
    /// The message that drew it: Refresh on a board left from before asks it again (the Mac's StudioNodeView).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// A trade plan's route extras: what stops it and its fix, the take-profit ways, the market picker (trade_plan.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanView>,
}

impl Board {
    /// `Board(kind:title:)`: a new id, now; the rest set after (`Board { lines, ..Board::new(…) }`).
    pub fn new(kind: &str, title: &str) -> Self {
        Board {
            id: Uuid::new().to_string(), kind: kind.into(), title: title.into(), lines: vec![], steps: None, state: None, chart: None, actions: None,
            at: chrono::Utc::now(), note: None, trade: None, opens: None, data: None, request: None, plan: None,
        }
    }
}

/// One card of a move's route: «BUY · Solana», what it does, and the balance it changes, now → after.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub label: String,
    pub title: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub money: Option<Money>,
    /// «then»: a step after this move, the model's next one (chat_turns.rs boardsShown), drawn pale; None for the move's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Read from the balances on screen when the route was drawn, never from the step's words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Money {
    pub place: String,
    pub symbol: String,
    pub was: String,
    pub after: String,
}

/// A button on a block: it puts the command in the chat field, and the person sends it; it never moves money itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub title: String,
    pub command: String,
}

/// One canvas: its own chat and its own blocks, as on the Mac.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasDoc {
    pub id: String,
    /// Its automatic title (the first message's words); the list shows the newest move's title over it, and the
    /// person's own `name` over both (the Mac's CanvasSession.title and .name).
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub boards: Vec<Board>,
    #[serde(with = "json::date")]
    pub at: Date,
    /// The canvas list's (folders.rs): pinned on top since, its folder, its picture, the model it was talked through.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub pinned_at: Option<Date>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<CanvasIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A scheduled item's own canvas: «task:<id>» or «perp:<coin>» (schedule.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
    /// What its chat was in when the person left it, back when they come back and across a restart (the Mac's
    /// CanvasSession): the trade plan, its board and the plan a trade came from, the skills loaded, the last reply's cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<TradeIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_board: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traded_plan: Option<TradeIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offers: Option<Vec<RouterOption>>,
}

impl CanvasDoc {
    /// `CanvasDoc(title:)`: a new id, now, nothing on it.
    pub fn new(title: &str) -> Self {
        CanvasDoc {
            id: Uuid::new().to_string(), title: title.into(), name: None, messages: vec![], boards: vec![], at: chrono::Utc::now(), pinned_at: None,
            folder: None, icon: None, provider: None, model: None, schedule: None, intent: None, plan_board: None, traded_plan: None, skills: None,
            offers: None,
        }
    }
}

pub const LIVE_BOARDS: [&str; 10] = ["portfolio", "receive", "addresses", "history", "favorites", "alerts", "schedule", "positions", "multisig", "ledger"];

/// Canvases from canvases.json; the chat of a build before canvases becomes the first one.
pub fn load_canvases() -> Vec<CanvasDoc> {
    if let Some(mut saved) = store::load::<Vec<CanvasDoc>>("canvases.json") && !saved.is_empty() {
        // A card can't wait across a restart, and a move that was running then may or may not have gone through.
        for b in saved.iter_mut().flat_map(|c| c.boards.iter_mut()) {
            match b.state.as_deref() {
                Some("waiting") => {
                    b.state = Some("cancelled".into());
                    b.lines.push("ihatecrypto restarted before Confirm: nothing was signed.".into());
                }
                Some("running") => {
                    b.state = Some("failed".into());
                    b.lines.push("ihatecrypto stopped while this ran: check History before doing it again.".into());
                }
                _ => {}
            }
        }
        return saved;
    }
    let old: Vec<Message> = store::load("chat.json").unwrap_or_default();
    let title = old.iter().find(|m| m.from_user).map_or_else(|| "Canvas".to_string(), |m| m.text.graphemes(true).take(40).collect());
    vec![CanvasDoc { messages: old, ..CanvasDoc::new(&title) }]
}

impl Engine {
    /// The open canvas's blocks changed (Swift's `boards` setter): saved, and a state pushed.
    pub fn set_boards(&self, change: impl FnOnce(&mut Vec<Board>)) {
        {
            let mut st = self.st_mut();
            let i = st.canvas_index();
            change(&mut st.canvases[i].boards);
        }
        self.save_canvases();
        self.changed();
    }

    /// A block changed wherever it is, by its id.
    pub fn update_board(&self, id: Option<&str>, change: impl FnOnce(&mut Board)) {
        let Some(id) = id else { return };
        let found = {
            let mut st = self.st_mut();
            match st.canvases.iter_mut().flat_map(|c| c.boards.iter_mut()).find(|b| b.id == id) {
                Some(b) => { change(b); true }
                None => false,
            }
        };
        if found {
            self.save_canvases();
            self.changed();
        }
    }

    /// The canvases saved (each with its last 300 messages), the open one's state and board title written into it first,
    /// and the History's own copy brought up to date.
    pub fn save_canvases(&self) {
        let mut st = self.st_mut();
        st.keep_canvas_state();
        st.keep_board_title();
        // The History keeps its own copy, so nothing said leaves it with a canvas (HistoryStore).
        let st = &mut *st;
        st.history.record(&st.canvases);
        let kept: Vec<CanvasDoc> = st.canvases.iter().map(|c| {
            let mut c = c.clone();
            c.messages.drain(..c.messages.len().saturating_sub(300));
            c
        }).collect();
        store::save(&kept, "canvases.json");
    }
}

impl crate::engine::State {
    /// The open canvas keeps its plan, skills and live cards (saved with canvases.json).
    pub fn keep_canvas_state(&mut self) {
        let i = self.canvas_index();
        let intent = (self.intent != TradeIntent::default()).then(|| self.intent.clone());
        let (plan_board, traded_plan) = (self.plan_board.clone(), self.traded_plan.clone());
        let skills = (!self.loaded_skills.is_empty()).then(|| self.loaded_skills.clone());
        // The last reply's cards while none is picked and nothing was said after them.
        let last = self.canvases[i].messages.last();
        let live = !self.options.is_empty() && last.map(|m| &m.id) == self.options_message.as_ref() && last.and_then(|m| m.picked).is_none();
        let offers = live.then(|| self.options.clone());
        let c = &mut self.canvases[i];
        (c.intent, c.plan_board, c.traded_plan, c.skills, c.offers) = (intent, plan_board, traded_plan, skills, offers);
    }
}

/// The first word of a text, as Swift's `split(separator: " ").first`: empty pieces skipped.
fn first_word(text: &str) -> Option<&str> { text.split(' ').find(|w| !w.is_empty()) }

impl Engine {
    /// A block on top of the canvas. A live block that's already there moves to the top instead of showing twice.
    pub fn pin(&self, board: Board) -> String {
        let mut board = board;
        // A move keeps the request that drew it: Refresh asks it again once it's left unsigned (blocks.rs).
        if board.kind == "move" && board.request.is_none() { board.request = self.last_request() }
        let mut id = board.id.clone();
        self.set_boards(|list| {
            if LIVE_BOARDS.contains(&board.kind.as_str()) && let Some(i) = list.iter().position(|b| b.kind == board.kind) {
                let mut b = list.remove(i);
                b.at = chrono::Utc::now();
                id = b.id.clone();
                list.insert(0, b);
                return;
            }
            // Nothing leaves the canvas (the Mac's EarlierBoards keeps every finished move): no cap.
            list.insert(0, board);
        });
        id
    }

    /// Swift's `pin(_:_:lines:chart:actions:data:)`: a block made from its kind and title, with the request that drew it.
    pub fn pin_kind(&self, kind: &str, title: &str, lines: Vec<String>, chart: Option<ChartLine>, actions: Option<Vec<Action>>, data: Option<BlockData>) {
        // The request that drew it, not a «yes» or «cancel» said after.
        let request = self.last_request();
        self.pin(Board { lines, chart, actions, data, request, ..Board::new(kind, title) });
    }

    // MARK: Canvases

    /// Another canvas may open: nothing is being answered or carried out (the Mac's canSwitchCanvas), so a reply or a
    /// move's outcome can't land on a canvas it wasn't asked on. The chat's own «new canvas» passes `from_chat`.
    pub fn can_switch_canvas(&self) -> bool {
        let st = self.st();
        !st.thinking && st.running.is_none() && st.running_board.is_none()
    }

    pub fn new_canvas(&self, from_chat: bool) -> Result<()> {
        self.leave_canvas(from_chat)?;
        self.forget_unkept();
        let c = CanvasDoc::new(&self.say("New canvas", "Новый холст"));
        {
            let mut st = self.st_mut();
            st.canvas_id = c.id.clone();
            st.canvases.insert(0, c);
        }
        self.after_switch();
        Ok(())
    }

    pub fn open_canvas(&self, id: &str) -> Result<()> {
        {
            let st = self.st();
            if id == st.canvas_id || !st.canvases.iter().any(|c| c.id == id) { return Ok(()) }
        }
        self.leave_canvas(false)?;
        self.forget_unkept();
        self.st_mut().canvas_id = id.into();
        self.after_switch();
        Ok(())
    }

    /// The person's own name for a canvas; an empty one gives the automatic title back (the Mac's renameCanvas).
    pub fn rename_canvas(&self, id: &str, title: &str) {
        let t = title.trim();
        {
            let mut st = self.st_mut();
            let Some(c) = st.canvases.iter_mut().find(|c| c.id == id) else { return };
            c.name = if t.is_empty() { None } else { Some(t.graphemes(true).take(80).collect()) };
        }
        self.save_canvases();
    }

    pub fn remove_canvas(&self, id: &str) -> Result<()> {
        let (boards, was_open) = {
            let st = self.st();
            let Some(c) = st.canvases.iter().find(|c| c.id == id) else { return Ok(()) };
            (c.boards.clone(), id == st.canvas_id)
        };
        self.decline_site_on_canvas(&boards);
        if was_open { self.leave_canvas(false)? }
        // Its plan's price alert goes with it (the Mac's Canvases.swift:361).
        self.set_alerts(|alerts| alerts.retain(|a| !a.plan.is_some_and(|p| p.to_string() == id)));
        let fresh = {
            let mut st = self.st_mut();
            st.canvases.retain(|c| c.id != id);
            // The open one gives way to a new blank canvas, as the Mac's deleteCanvas opens newCanvas(), not another.
            was_open || st.canvases.is_empty()
        };
        if fresh {
            let c = CanvasDoc::new(&self.say("New canvas", "Новый холст"));
            {
                let mut st = self.st_mut();
                st.canvas_id = c.id.clone();
                st.canvases.insert(0, c);
            }
            self.after_switch();
        }
        self.save_canvases();
        Ok(())
    }

    /// A running move stays where it was started; a waiting card stays on its canvas, marked as not confirmed. What the
    /// chat was in stays with the canvas (keep_canvas_state).
    fn leave_canvas(&self, from_chat: bool) -> Result<()> {
        let (busy, thinking, card) = {
            let st = self.st();
            (st.running.is_some() || st.running_board.is_some(), st.thinking, st.card.as_ref().map(|c| c.id.clone()))
        };
        if busy { return Err(WalletError(self.say("A move is running: wait for it to finish.", "Идёт перевод: дождись, пока он закончится."))) }
        if !from_chat && thinking {
            return Err(WalletError(self.say("The chat is still answering: wait for it, or press Stop.", "Чат ещё отвечает: подожди или нажми «Стоп».")));
        }
        if let Some(card) = card && !self.park_pending() {
            let line = self.say("Left without Confirm: nothing was signed.", "Ушёл без «Подтвердить»: ничего не подписано.");
            self.update_board(Some(&card), |b| {
                b.state = Some("cancelled".into());
                b.lines.push(line);
            });
            let mut st = self.st_mut();
            st.pending = None;
            st.card = None;
        }
        self.st_mut().keep_canvas_state();
        self.set_options(vec![]);
        let mut st = self.st_mut();
        st.awaiting_amount = None;
        st.intent = TradeIntent::default();
        st.plan_board = None;
        st.traded_plan = None;
        st.loaded_skills = vec![];
        Ok(())
    }

    fn after_switch(&self) {
        self.restore_canvas_state();
        self.unpark_pending();
        let id = self.st().canvas_id.clone();
        self.set_settings(|s| s.canvas = Some(id));
        self.save_canvases();
        self.changed();
    }

    /// Back on a canvas (or at launch): its plan, skills, model and the last reply's cards as they were; a chart older
    /// than five minutes read again, as the Mac reloads a stale snapshot (Canvases.load).
    pub fn restore_canvas_state(&self) {
        let c = {
            let st = self.st();
            st.canvases[st.canvas_index()].clone()
        };
        {
            let mut st = self.st_mut();
            st.intent = c.intent.clone().unwrap_or_default();
            st.plan_board = c.plan_board.clone().filter(|id| c.boards.iter().any(|b| &b.id == id));
            st.traded_plan = c.traded_plan.clone();
            st.loaded_skills = c.skills.clone().unwrap_or_default();
        }
        if let Some(offers) = &c.offers && let Some(last) = c.messages.last() && !last.from_user && last.picked.is_none() && !offers.is_empty() {
            self.set_options(offers.clone());
        }
        // The model the canvas was talked through, while that account is still connected (C3).
        if let Some(id) = &c.provider && *id != self.provider_id() && let Some(p) = self.provider(id) {
            let info = p.info();
            if info.unavailable.is_none() && (store::credentials().contains_key(id) || info.kind == llm::catalog::Kind::Local) {
                self.set_settings(|s| s.provider = Some(id.clone()));
                if let Some(m) = &c.model { self.set_model(Some(m.as_str()), id) }
            }
        }
        let now = chrono::Utc::now();
        let stale = c.boards.iter().filter(|b| b.kind == "chart" && b.chart.as_ref().is_some_and(|l| l.coin.is_some()) && (now - b.at).num_seconds() > 300);
        for b in stale.take(3) {
            let (me, id) = (self.rc(), b.id.clone());
            tokio::task::spawn_local(async move { me.reload_chart(&id).await });
        }
    }

    /// A chart board drawn again from fresh candles, in its own range; it keeps its place and time on the canvas.
    pub async fn reload_chart(&self, id: &str) {
        let found = self.st().canvases.iter().flat_map(|c| c.boards.iter()).find(|b| b.id == id).cloned();
        let Some(b) = found else { return };
        let Some(coin) = b.chart.as_ref().and_then(|c| c.coin.clone()) else { return };
        let range = Range::ALL.into_iter().find(|r| b.title.ends_with(r.title())).unwrap_or(Range::Day);
        let Ok(mut line) = self.chart_line(&coin, range).await else { return };
        line.network = b.chart.and_then(|c| c.network);
        self.update_board(Some(id), |b| b.chart = Some(line));
    }

    /// The first thing said on a canvas names it, as the Mac's canvas list does.
    pub fn name_canvas(&self, text: &str) {
        let new = self.say("New canvas", "Новый холст");
        let mut st = self.st_mut();
        let i = st.canvas_index();
        let c = &mut st.canvases[i];
        let untitled = c.title == new || c.title == "New canvas" || c.title == "Новый холст";
        if c.messages.iter().filter(|m| m.from_user).count() != 1 || !untitled { return }
        c.title = text.graphemes(true).take(40).collect();
    }

    /// A block in words, as the model gets it: its title, its state, what it shows.
    pub fn block_facts(&self, b: &Board) -> String {
        let live: Vec<String> = match b.kind.as_str() {
            "portfolio" => self.portfolio_facts(),
            "addresses" => self.st().book.iter().map(|e| format!("{} on {}", e.name, e.network)).collect(),
            "alerts" => {
                let alerts = self.st().alerts.clone();
                alerts.iter().map(|a| self.alert_text(a)).collect()
            }
            "schedule" => {
                let tasks = self.st().tasks.clone();
                tasks.iter().map(|t| self.task_text(t)).collect()
            }
            "favorites" => self.st().favorites.iter().map(|f| format!("{} on {}", f.symbol, f.chain)).collect(),
            "multisig" => self.st().multisigs.iter().map(|m| {
                format!("{}: any {} of {} on {}", m.address, m.threshold, m.keys.iter().map(|k| k.name.as_str()).collect::<Vec<_>>().join(", "),
                        m.networks.join(", "))
            }).collect(),
            "history" => self.history().iter().take(10).map(|h| format!("{} ({})", h.note, h.chain)).collect(),
            "scheduleItem" => self.schedule_item_facts(&b.title),
            "recurring" => self.recurring_facts(&b.title),
            "note" => vec![format!("{} {} in folder «{}»", if b.opens.is_none() { "this canvas is" } else { "a card opens" },
                                   if b.lines.len() == 1 { "a note".to_string() } else { format!("{} notes", b.lines.len()) }, b.title)],
            _ => vec![],
        };
        let steps = b.steps.iter().flatten().map(|s| {
            format!("{}: {}", s.label, s.title) + &s.money.as_ref().map_or_else(String::new, |m| format!(" ({} {} → {} {})", m.place, m.was, m.after, m.symbol))
        });
        // A chart: its range in numbers, and the live price now when Hyperliquid has the market.
        let chart = b.chart.as_ref().map(|c| {
            let coin = first_word(&c.title).unwrap_or("");
            let live = self.st().markets.get(coin).map_or_else(String::new, |m| format!(", live price now {}", Fmt::price(m.mark_px)));
            format!("low {}, high {}, {} over the range{live}", c.low, c.high, c.change)
        });
        let shown: Vec<String> = steps.chain(chart).chain(b.lines.iter().cloned()).chain(live).take(14).collect();
        let name = if ["move", "result", "chart"].contains(&b.kind.as_str()) { &b.title } else { &b.kind };
        format!("«{name}»{}: {}", b.state.as_ref().map_or_else(String::new, |s| format!(" [{s}]")), shown.join("; "))
    }

    // MARK: A move's route

    /// The card's steps: one per step of a chain, else the command's own, with the balance it spends.
    pub fn steps(&self, pending: &Pending, title: &str, lines: &[String]) -> Vec<Step> {
        match pending {
            Pending::Chain(list) => self.chain_steps(list),
            Pending::Wallet(c) | Pending::Recurring { command: c, .. } => self.route_steps(c).unwrap_or_else(|| {
                vec![Step { label: self.label(c), title: title.into(), lines: lines.to_vec(), money: self.money(c), status: None }]
            }),
            Pending::Transfer(entry, amount) => self.transfer_steps(entry, *amount),
            Pending::Position { command, coin } => {
                if let PositionCommand::Close(share) = command && let Some(steps) = self.close_steps(*share, coin) { return steps }
                vec![Step { label: "HYPERLIQUID".into(), title: title.into(), lines: lines.to_vec(), money: None, status: None }]
            }
            Pending::Orders { .. } => vec![Step { label: "HYPERLIQUID".into(), title: title.into(), lines: lines.to_vec(), money: None, status: None }],
            Pending::Trade(_) => vec![Step { label: self.say("TRADE · Hyperliquid", "СДЕЛКА · Hyperliquid"), title: title.into(), lines: lines.to_vec(), money: None, status: None }],
            Pending::Address(e) => vec![Step { label: self.say("SAVE · ", "СОХРАНИТЬ · ") + &self.network_name(&e.network), title: title.into(),
                                               lines: lines.to_vec(), money: None, status: None }],
        }
    }

    /// As the Mac draws a chain (Canvases.swift chainSteps): the first move as its whole route, each later one as its
    /// route without its own start and approve, pale «then»; no bridge where a move before brings the money.
    fn chain_steps(&self, list: &[ChainStep]) -> Vec<Step> {
        let mut arrived: HashSet<String> = HashSet::new();
        // And how much: a later step is drawn paid as it will be, by the balances the moves before it leave (the Mac's
        // `ahead`): «buy on Base» after «move 10 to Base» pays on Base, not Solana's ways.
        let mut ahead: BTreeMap<String, Decimal> = BTreeMap::new();
        let later = |steps: Vec<Step>, spends: Option<String>, arrived: &HashSet<String>| -> Vec<Step> {
            let brought = spends.is_some_and(|s| arrived.contains(&s));
            let mut skip = vec!["FROM", "ОТКУДА", "APPROVE", "РАЗРЕШЕНИЕ"];
            if brought { skip.extend(["BRIDGE", "МОСТ", "GAS", "ГАЗ"]) }
            let kept: Vec<Step> = steps.iter().filter(|s| !skip.iter().any(|w| s.label.starts_with(w))).cloned().collect();
            (if kept.is_empty() { steps } else { kept }).into_iter().map(|mut s| { s.status = Some("then".into()); s }).collect()
        };
        let mut out = vec![];
        for (n, step) in list.iter().enumerate() {
            let first = n == 0;
            match step {
                ChainStep::Wallet(c) => {
                    let steps = (if first { self.route_steps(c) } else { self.route_ahead(c, &ahead) })
                        .unwrap_or_else(|| vec![Step { label: self.label(c), title: c.summary(), lines: vec![], money: self.money(c), status: None }]);
                    out.extend(if first { steps } else { later(steps, Self::spends(c), &arrived) });
                    if let Some(to) = Self::arrives(c) { arrived.insert(to); }
                    self.count_ahead(c, &mut ahead);
                }
                // From its own network, with that network's balance, to the saved name (move_routes.rs).
                ChainStep::Transfer(e, amount) => {
                    let steps = self.transfer_steps(e, *amount);
                    out.extend(if first { steps } else { later(steps, Some(e.network.clone()), &arrived) });
                }
                ChainStep::Said(text) => out.push(self.said_step(text)),
            }
        }
        out
    }

    /// What a chain's move leaves at each place for the steps after it: a bridge's dollars leave its source and land
    /// where it delivers; a Solana move spends Solana's.
    fn count_ahead(&self, c: &WalletCommand, ahead: &mut BTreeMap<String, Decimal>) {
        use WalletCommand as W;
        let sent: Option<Option<Decimal>> = match c {
            W::Bridge { amount, .. } | W::Cctp { amount, .. } | W::EvmToSolana(_, amount) | W::SolanaTo(_, amount) | W::BaseToSolana(amount)
            | W::ArbitrumToSolana(amount) | W::RobinhoodToSolana(amount) | W::WithdrawToSolana(amount) => Some(*amount),
            _ => None,
        };
        let bridged = sent.and_then(|sent| {
            let from = Self::source(c)?;
            let moved = sent.or_else(|| self.usdc_free(&from))?;
            Some((from, moved))
        });
        if let Some((from, moved)) = bridged {
            let e = ahead.entry(from).or_default();
            *e = e.saturating_sub(moved);
            if let Some(to) = Self::arrives(c) {
                let e = ahead.entry(to).or_default();
                *e = e.saturating_add(moved);
            }
        } else if let Some(spent) = Self::solana_usdc(c) {
            let e = ahead.entry("solana".into()).or_default();
            *e = e.saturating_sub(spent);
        }
    }

    /// A later step's route, read against the balances the moves before it leave; the portfolio put back after.
    fn route_ahead(&self, c: &WalletCommand, ahead: &BTreeMap<String, Decimal>) -> Option<Vec<Step>> {
        if ahead.is_empty() { return self.route_steps(c) }
        let kept = {
            let mut st = self.st_mut();
            let kept = st.portfolio.clone();
            for (net, delta) in ahead {
                let dollar = if net == "robinhood" { "USDG" } else { "USDC" };
                let networks = &mut st.portfolio.networks;
                if !networks.iter().any(|n| &n.key == net) {
                    networks.push(crate::portfolio::Network { key: net.clone(), name: WalletCommand::place_name(net), usd: Decimal::ZERO, rows: vec![], note: None,
                                                              hidden: vec![], hidden_more: 0 });
                }
                let n = networks.iter_mut().find(|n| &n.key == net).expect("added above");
                if let Some(r) = n.rows.iter_mut().find(|r| r.section == "tokens" && r.symbol == dollar && r.contract.is_none()) {
                    r.value = r.value.saturating_add(*delta).max(Decimal::ZERO);
                } else if *delta > Decimal::ZERO {
                    // Portfolio's row(_:_:usd:) for a dollar: no chain, so no key.
                    let mut row = crate::portfolio::Row::new(dollar, &Fmt::num(*delta, if *delta < Decimal::ONE { 6 } else { 4 }), *delta);
                    row.usd = Some(*delta);
                    n.rows.push(row);
                }
            }
            kept
        };
        let steps = self.route_steps(c);
        self.st_mut().portfolio = kept;
        steps
    }

    fn label(&self, c: &WalletCommand) -> String {
        match c {
            WalletCommand::MultisigCreate { .. } => return self.say("MULTISIG · Safe", "МУЛЬТИСИГ · Safe"),
            WalletCommand::MultisigSend { network, .. } => {
                let net = network.clone().or_else(|| self.st().multisigs.last().and_then(|m| m.networks.first().cloned())).unwrap_or_else(|| "base".into());
                return self.say("PAY · ", "ОПЛАТА · ") + &self.network_name(&net);
            }
            _ => {}
        }
        let verb = first_word(&c.summary()).map_or_else(|| "MOVE".to_string(), str::to_uppercase);
        verb + " · " + &self.network_name(&Self::place(c).unwrap_or_else(|| "solana".into()))
    }

    fn network_name(&self, key: &str) -> String {
        if let Some(n) = self.st().portfolio.networks.iter().find(|n| n.key == key) { return n.name.clone() }
        serde_json::from_value::<wallet::address_check::Network>(serde_json::Value::String(key.into()))
            .map_or_else(|_| WalletCommand::place_name(key), |n| n.title().into())
    }

    /// Where a command's money comes from, as a portfolio network key.
    fn place(c: &WalletCommand) -> Option<String> {
        use WalletCommand as W;
        match c {
            W::BuyOn { place, from, .. } => Some(from.clone().unwrap_or_else(|| place.clone())),
            W::SwapOn { place, .. } | W::SellOn { place, .. } => Some(place.clone()),
            W::Aave { network, .. } | W::Morpho { network, .. } => Some(network.clone()),
            W::SendToken { network, .. } => Some(network.clone().unwrap_or_else(|| "solana".into())),
            W::Hlp { .. } | W::HlSpotBuy { .. } | W::HlSpotSell { .. } => Some("hyperliquid".into()),
            _ => None,
        }
    }

    /// What the command spends, read against the balance on screen: «Solana · 45.97 → 35.97 USDC».
    fn money(&self, c: &WalletCommand) -> Option<Money> {
        use WalletCommand as W;
        match c {
            W::Buy { usdc: Some(u), .. } => self.money_spending(*u, "USDC", "solana"),
            W::Sell { asset, amount: Some(x) } => self.money_spending(*x, &asset.symbol, "solana"),
            W::LendDeposit(s, Some(x)) | W::Kamino { deposit: true, symbol: s, amount: Some(x) } => self.money_spending(*x, s, "solana"),
            W::Stake(Some(x)) => self.money_spending(*x, "SOL", "solana"),
            W::SolanaTo(_, Some(x)) => self.money_spending(*x, "USDC", "solana"),
            W::SendSol { amount, .. } => self.money_spending(*amount, "SOL", "solana"),
            W::Hlp { deposit: true, amount: Some(x) } => self.money_spending(*x, "USDC", "hyperliquid"),
            W::Aave { deposit: true, amount: Some(x), network } | W::Morpho { deposit: true, amount: Some(x), network } => self.money_spending(*x, "USDC", network),
            W::BuyOn { place, usdc: Some(u), from, .. } => self.money_spending(*u, "USDC", from.as_deref().unwrap_or(place)),
            W::SwapOn { place, from, amount: Some(x), .. } => self.money_spending(*x, from, place),
            W::SellOn { place, token, amount: Some(x), .. } => self.money_spending(*x, token, place),
            W::SendToken { token, amount: Some(x), network, .. } => self.money_spending(*x, token, network.as_deref().unwrap_or("solana")),
            _ => None,
        }
    }

    /// Swift's `money(spending:_:on:)`: `amount` of `symbol` leaving `place`, its row now → after.
    pub fn money_spending(&self, amount: Decimal, symbol: &str, place: &str) -> Option<Money> {
        let st = self.st();
        let net = st.portfolio.networks.iter().find(|n| n.key == place)?;
        let row = net.rows.iter().find(|r| r.symbol.to_uppercase() == symbol.to_uppercase())?;
        // The amount is the chat's: past Decimal's range, all of it leaves.
        let after = row.value.checked_sub(amount).unwrap_or(Decimal::ZERO).max(Decimal::ZERO);
        Some(Money { place: net.name.clone(), symbol: row.symbol.clone(), was: Self::amount(row.value, &row.symbol), after: Self::amount(after, &row.symbol) })
    }
}

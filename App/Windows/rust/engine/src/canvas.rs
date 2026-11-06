//! Canvases and their blocks (Canvas.swift): the types kept in canvases.json, and keeping them — the canvases read at
//! launch, the open one's state written into it, saved with the History's copy, a block changed in place.
//! Layer 2 adds here: pin, new/open/rename/remove canvas, leaving and switching (park, unpark, afterSwitch),
//! restore_canvas_state, reload_chart, name_canvas, block_facts and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::{Date, Uuid};
use ihatecrypto_core::intent::TradeIntent;
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

use crate::blocks::BlockData;
use crate::engine::{ChartLine, Engine, Message};
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

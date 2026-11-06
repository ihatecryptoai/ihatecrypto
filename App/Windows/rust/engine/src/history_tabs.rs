//! The History page's three tabs (HistoryTabs.swift), as the Mac's HistoryView: «Agent» — each request with what it was
//! understood as, the cards it had on the canvas and what was said, and the scheduled runs; earlier signature sessions
//! under them; «Positions» — each Hyperliquid position from open to close with its realized P/L and fees; «All» —
//! every request, fill, transfer and signature in one timeline. And `HistoryStore`, the History's own record
//! (history.json), kept apart from the canvases.
//! Layer 2 adds here: record_automatic, history_tabs() (reading the page), recent_requests, freezing a request's canvas.

use std::collections::HashMap;

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::canvas::{CanvasDoc, LIVE_BOARDS};
use crate::folders::shown_title;
use crate::{json, store};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryTabs {
    pub agent: Vec<Activity>,
    pub earlier: Vec<Activity>,
    pub stories: Vec<Story>,
    pub rows: Vec<Row>,
    /// Hyperliquid's history didn't load: why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub hyperliquid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bubble {
    pub kind: String,
    pub label: String,
    pub title: String,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default = "info")]
    pub status: String,
}

fn info() -> String { "info".into() }

/// What the canvas showed once a message was handled (the Mac's snapshotCanvas): that request's title and cards,
/// frozen. No title and no cards: nothing the Mac would freeze; the cards the message drew are its History card.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub bubbles: Vec<Bubble>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    /// «request», «execution» or «automatic».
    pub kind: String,
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub understood: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    #[serde(default)]
    pub bubbles: Vec<Bubble>,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub failed: bool,
    /// The Schedule item it ran for («perp:BTC»), as the Mac's Activity.schedule: that item's LAST RUNS and the
    /// Schedule's History list it, without its coin putting it in a position's story.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<String>,
}

impl Activity {
    /// `Activity(kind:at:)`; the rest set after.
    pub fn new(kind: &str, at: Date) -> Self {
        Activity { kind: kind.into(), at, request: None, understood: None, title: None, coin: None, bubbles: vec![], lines: vec![], failed: false,
                   schedule: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// «open», «close» or «activity».
    pub kind: String,
    #[serde(with = "json::date")]
    pub at: Date,
    pub label: String,
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Story {
    pub coin: String,
    pub long: bool,
    pub open: bool,
    #[serde(with = "json::date")]
    pub opened: Date,
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub closed: Option<Date>,
    #[serde(with = "number")]
    pub realized: Decimal,
    #[serde(with = "number")]
    pub fees: Decimal,
    #[serde(with = "number")]
    pub peak: Decimal,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    #[serde(with = "json::date")]
    pub at: Date,
    /// An icon's name and its tone: «accent», «profit», «loss» or «secondary».
    pub icon: String,
    pub tone: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

// MARK: The History's own record

/// history.json: each canvas's chat and finished blocks, what ihatecrypto did by itself, each request's frozen canvas.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub chats: Vec<CanvasDoc>,
    #[serde(default)]
    pub automatic: Vec<Activity>,
    /// Each request's frozen canvas, by its message's id (None in a record from before they were kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<HashMap<String, Snapshot>>,
}

/// The History's own record, as the Mac keeps its 1,000 activities apart from the canvases: each canvas's chat and its
/// moves copied in as they're said, so a canvas deleted, a chat cleared, a scheduled run's canvas cleaned up or the 300
/// messages a canvas keeps leave the History as it was; and what ihatecrypto did by itself (money arrived, an alert, the
/// guardian, a limit filled) as activities of their own. The first launch with it copies what the canvases hold.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryStore {
    pub saved: Saved,
    /// The page has been read once this run.
    pub read: bool,
}

impl HistoryStore {
    pub const LIMIT: usize = 1000;

    pub fn load() -> Self { HistoryStore { saved: store::load("history.json").unwrap_or_default(), read: false } }

    /// The canvases' chats and finished blocks merged in; true when anything was new or changed.
    pub fn record(&mut self, canvases: &[CanvasDoc]) -> bool {
        let mut changed = false;
        for c in canvases.iter().filter(|c| !c.messages.is_empty() || c.boards.iter().any(|b| !LIVE_BOARDS.contains(&b.kind.as_str()))) {
            // Named as the canvas list names it: the person's name, else its newest move, else its first words.
            let title = shown_title(c, &c.boards);
            let i = match self.saved.chats.iter().position(|s| s.id == c.id) {
                Some(i) => i,
                None => {
                    self.saved.chats.push(CanvasDoc { id: c.id.clone(), at: c.at, ..CanvasDoc::new(&title) });
                    self.saved.chats.len() - 1
                }
            };
            let chat = &mut self.saved.chats[i];
            if chat.title != title { chat.title = title; changed = true }
            let mut at: HashMap<String, usize> = HashMap::new();
            for (n, m) in chat.messages.iter().enumerate() { at.entry(m.id.clone()).or_insert(n); }
            for m in &c.messages {
                match at.get(&m.id) {
                    Some(&n) => if chat.messages[n] != *m { chat.messages[n] = m.clone(); changed = true },
                    None => {
                        at.insert(m.id.clone(), chat.messages.len());
                        chat.messages.push(m.clone());
                        changed = true;
                    }
                }
            }
            for b in c.boards.iter().filter(|b| !LIVE_BOARDS.contains(&b.kind.as_str())) {
                match chat.boards.iter().position(|s| s.id == b.id) {
                    Some(n) => if chat.boards[n] != *b { chat.boards[n] = b.clone(); changed = true },
                    None => { chat.boards.push(b.clone()); changed = true }
                }
            }
        }
        if changed { self.trim(); store::save(&self.saved, "history.json") }
        changed
    }

    /// What ihatecrypto did by itself. `merging`: a trailing stop moving again is one more line on its record, as the
    /// Mac keeps a trend from filling the History.
    pub fn automatic(&mut self, title: &str, line: &str, coin: Option<String>, failed: bool, merging: bool, schedule: Option<String>) {
        match self.saved.automatic.last_mut() {
            Some(last) if merging && last.title.as_deref() == Some(title) => last.lines.push(line.into()),
            _ => self.saved.automatic.push(Activity {
                title: Some(title.into()), coin, lines: vec![line.into()], failed, schedule, ..Activity::new("automatic", chrono::Utc::now())
            }),
        }
        self.trim();
        store::save(&self.saved, "history.json");
    }

    /// The newest 1,000 requests and 1,000 automatic records stay.
    pub fn trim(&mut self) {
        let over = self.saved.automatic.len().saturating_sub(Self::LIMIT);
        self.saved.automatic.drain(..over);
        let mut asked: Vec<Date> = self.saved.chats.iter().flat_map(|c| c.messages.iter().filter(|m| m.from_user).map(|m| m.at)).collect();
        if asked.len() <= Self::LIMIT { return }
        asked.sort();
        let cutoff = asked[asked.len() - Self::LIMIT];
        for c in &mut self.saved.chats {
            c.messages.retain(|m| m.at >= cutoff);
            c.boards.retain(|b| b.at >= cutoff);
        }
        self.saved.chats.retain(|c| !c.messages.is_empty() || !c.boards.is_empty());
        if let Some(shown) = &mut self.saved.shown {
            let ids: std::collections::HashSet<&String> = self.saved.chats.iter().flat_map(|c| c.messages.iter().map(|m| &m.id)).collect();
            shown.retain(|k, _| ids.contains(k));
        }
    }

    /// A request's canvas frozen as it was handled.
    pub fn freeze(&mut self, snapshot: Snapshot, message: &str) {
        self.saved.shown.get_or_insert_with(HashMap::new).insert(message.into(), snapshot);
        store::save(&self.saved, "history.json");
    }
}

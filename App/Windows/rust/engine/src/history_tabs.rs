//! The History page's three tabs (HistoryTabs.swift), as the Mac's HistoryView: «Agent» — each request with what it was
//! understood as, the cards it had on the canvas and what was said, and the scheduled runs; earlier signature sessions
//! under them; «Positions» — each Hyperliquid position from open to close with its realized P/L and fees; «All» —
//! every request, fill, transfer and signature in one timeline. And `HistoryStore`, the History's own record
//! (history.json), kept apart from the canvases.
//! Layer 2: what ihatecrypto did by itself recorded, a request's canvas frozen as it was handled, the activities the
//! History reads from its own record, and the three tabs (`history_tabs`) with Hyperliquid's fills and the journals.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::TimeDelta;
use execution::{Entry, Journal, Kind};
use fancy_regex::Regex;
use ihatecrypto_core::foundation::{Date, number};
use ihatecrypto_core::intent::{Fmt, IntentParser, TradeIntent};
use ihatecrypto_core::trade_history::{self, Fill, Transfer};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::canvas::{Board, CanvasDoc, LIVE_BOARDS};
use crate::engine::{Engine, Message};
use crate::folders::{board_title, shown_title};
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

/// The three tabs as built from plain inputs (`Engine::history_tabs_from`).
pub type Tabs = (Vec<Activity>, Vec<Activity>, Vec<Story>, Vec<Row>);

/// «solana» → «Solana».
fn capitalized(c: &str) -> String {
    let mut chars = c.chars();
    chars.next().map_or_else(String::new, |f| f.to_uppercase().chain(chars).collect())
}

fn bubble(kind: &str, label: &str, title: &str, lines: Vec<String>, status: &str) -> Bubble {
    Bubble { kind: kind.into(), label: label.into(), title: title.into(), lines, status: status.into() }
}

impl Engine {
    /// What ihatecrypto did by itself, kept in the History (the Mac's notify(record:)).
    pub fn record_automatic(&self, title: &str, line: &str, coin: Option<&str>, failed: bool, merging: bool, schedule: Option<&str>) {
        self.st_mut().history.automatic(title, line, coin.map(str::to_string), failed, merging, schedule.map(str::to_string));
    }

    /// A card of the canvas as History keeps it (the Mac's ActivityBubble): each step of a route; rates as up to 8 small
    /// cards, asset · place · APY (its `yield` bubbles); anything else as one card.
    pub fn bubbles(b: &Board) -> Vec<Bubble> {
        let status = match b.state.as_deref() { Some("failed") => "blocked", Some("done") => "done", _ => "info" };
        if let Some(steps) = b.steps.as_ref().filter(|s| !s.is_empty()) {
            return steps.iter().map(|s| bubble("step", &s.label, &s.title, s.lines.iter().take(2).cloned().collect(), status)).collect();
        }
        if let Some(rates) = b.data.as_ref().and_then(|d| d.rates.as_ref()).filter(|r| !r.is_empty()) {
            return rates.iter().take(8).map(|r| bubble("yield", &r.asset, &r.place, vec![format!("{}%", Fmt::num(r.apy, 2))], status)).collect();
        }
        vec![bubble(&b.kind, if b.kind == "chart" { "Chart" } else { "Result" }, &b.title, b.lines.iter().take(2).cloned().collect(), status)]
    }

    /// The canvas as it is once a message is handled, frozen into that request's History card (the Mac's snapshotCanvas
    /// → canvasBubbles), in the Mac's order: the card waiting for Confirm; else the chart while no plan is under way and
    /// this message showed no rates; else the rates it showed; else the take-profit ways it offered; else the plan under
    /// way. Rates and take-profit ways count only for the message that showed them, as the Mac clears both at the next.
    pub fn snapshot_canvas(&self, message: &Message) {
        let since = message.at;
        let snap = {
            let st = self.st();
            let boards = st.boards();
            let plan_under_way = st.intent != TradeIntent::default() && !st.positions.iter().any(|p| p.coin == st.intent.asset);
            let plan = st.plan_board.as_ref().and_then(|id| boards.iter().find(|b| &b.id == id));
            let rates = boards.iter().rev().find(|b| b.kind == "result" && b.data.as_ref().is_some_and(|d| d.rates.is_some()) && b.at >= since);
            let waiting = st.card.as_ref().and_then(|c| boards.iter().find(|b| b.id == c.id).map(|b| (c, b)));
            let chart = boards.iter().rev().find(|b| b.kind == "chart");
            let takes = plan.and_then(|p| p.plan.as_ref()?.takes.as_ref()).filter(|t| !t.is_empty());
            if let Some((c, b)) = waiting {
                Snapshot { title: Some(c.title.clone()), bubbles: Self::bubbles(b) }
            } else if let (false, None, Some(chart)) = (plan_under_way, rates, chart) {
                Snapshot { title: board_title(std::slice::from_ref(chart)), bubbles: Self::bubbles(chart) }
            } else if let Some(rates) = rates {
                Snapshot { title: Some("Rates".into()), bubbles: Self::bubbles(rates) }
            } else if let (true, Some(takes)) = (plan_under_way, takes) && st.takes_at.is_some_and(|t| t >= since) {
                Snapshot { title: Some("Take-profit choices".into()), bubbles: takes.iter().map(|t| {
                    bubble("choice", &t.title, if t.payoff.is_empty() { &t.title } else { &t.payoff }, vec![t.detail.clone()], "info")
                }).collect() }
            } else if let (true, Some(plan)) = (plan_under_way, plan) {
                Snapshot { title: Some(plan.title.clone()), bubbles: Self::bubbles(plan) }
            } else {
                Snapshot::default()
            }
        };
        self.st_mut().history.freeze(snap, &message.id);
    }

    /// What a canvas shows between requests, as the Mac's canvasBubbles picks it once the moves are done: its chart while
    /// no plan or rates are there, else its rates, else its plan. The newest of each. Only for a request recorded before
    /// snapshots were kept.
    pub fn standing(boards: &[Board]) -> Option<&Board> {
        let plan = boards.iter().rev().find(|b| b.kind == "move" && b.plan.is_some());
        let rates = boards.iter().rev().find(|b| b.kind == "result" && b.data.as_ref().is_some_and(|d| d.rates.is_some()));
        if plan.is_none() && rates.is_none() && let Some(chart) = boards.iter().rev().find(|b| b.kind == "chart") { return Some(chart) }
        rates.or(plan)
    }

    /// The History's record read once this run: each save merges the canvases in; the first read of a launch does too, so
    /// the History a build before this one rebuilt from the canvases is all there.
    fn history_read(&self) {
        let mut st = self.st_mut();
        if st.history.read { return }
        st.history.read = true;
        let st = &mut *st;
        st.history.record(&st.canvases);
    }

    /// The chats History reads: its own record.
    pub fn history_chats(&self) -> Vec<CanvasDoc> {
        self.history_read();
        self.st().history.saved.chats.clone()
    }

    /// What the History kept, as activities: each request with the cards made after it (until the next one) and the
    /// replies; each scheduled run, and what ihatecrypto did by itself, on its own. Newest last.
    pub fn history_activities(&self) -> Vec<Activity> {
        self.history_read();
        let st = self.st();
        let saved = &st.history.saved;
        let names: HashSet<String> = st.markets.keys().cloned().collect();
        let mut out = vec![];
        for c in &saved.chats {
            let mut asked: Vec<&Message> = c.messages.iter().filter(|m| m.from_user).collect();
            asked.sort_by_key(|m| m.at);
            for (i, q) in asked.iter().enumerate() {
                let until = asked.get(i + 1).map(|n| n.at);
                let within = |at: Date| at >= q.at && until.is_none_or(|u| at < u);
                let said: Vec<String> = c.messages.iter().filter(|m| !m.from_user && within(m.at)).map(|m| m.text.clone()).collect();
                let kept: Vec<&Board> = c.boards.iter().filter(|b| !LIVE_BOARDS.contains(&b.kind.as_str()) && !["recurring", "scheduleItem"].contains(&b.kind.as_str())).collect();
                let mut made: Vec<Board> = kept.iter().filter(|b| within(b.at)).map(|b| (*b).clone()).collect();
                made.sort_by_key(|b| b.at);
                // What the canvas showed as it was handled (the Mac's snapshotCanvas), frozen then; a record from before
                // snapshots: what a request that drew nothing had standing.
                let snap = saved.shown.as_ref().and_then(|s| s.get(&q.id));
                let shown = if snap.is_none() && made.is_empty() {
                    let mut before: Vec<Board> = kept.iter().filter(|b| b.at < q.at).map(|b| (*b).clone()).collect();
                    before.sort_by_key(|b| b.at);
                    Self::standing(&before).cloned()
                } else {
                    None
                };
                let frozen = snap.map(|s| &s.bubbles).filter(|b| !b.is_empty()).cloned();
                let bubbles = frozen.or_else(|| shown.as_ref().map(Self::bubbles)).unwrap_or_else(|| made.iter().flat_map(Self::bubbles).collect());
                // A scheduled run is a request of its own, «Scheduled: …», with no coin, so it isn't a «Guardian» step of a
                // trade (the Mac's beginActivity(.request, request:, schedule:), Tasks.swift:130).
                let run = q.note.as_deref() == Some("schedule");
                let title = snap.and_then(|s| s.title.clone()).or_else(|| board_title(shown.as_ref().map_or(&made[..], std::slice::from_ref)));
                out.push(Activity {
                    request: Some(q.text.clone()), understood: q.understood.clone(), title,
                    coin: if run { None } else { IntentParser::named_asset(&q.text, &names) }, bubbles, lines: said,
                    failed: run && made.iter().any(|b| b.state.as_deref() == Some("failed")),
                    ..Activity::new("request", q.at)
                });
            }
        }
        // A run whose canvas the History has is that record already (it showed twice).
        let kept: HashSet<&str> = saved.chats.iter().filter(|c| c.messages.iter().any(|m| m.from_user && m.note.as_deref() == Some("schedule")))
            .map(|c| c.id.as_str()).collect();
        for r in st.schedule_runs.iter().filter(|r| !kept.contains(r.canvas.as_deref().unwrap_or(""))) {
            out.push(Activity { title: Some(r.title.clone()), lines: r.line.clone().into_iter().collect(), failed: r.result == "failed", ..Activity::new("automatic", r.at) });
        }
        out.extend(saved.automatic.iter().cloned());
        out.sort_by_key(|a| a.at);
        out
    }

    /// The chat's model read this message as `canonical`: kept with it for the History's «understood as».
    pub fn note_understood(&self, canonical: &str) {
        let mut st = self.st_mut();
        if let Some(m) = st.messages_mut().iter_mut().rev().find(|m| m.from_user) { m.understood = Some(canonical.into()) }
    }

    /// The page's three tabs, as JSON for the window.
    pub async fn history_tabs(&self) -> String {
        let activities = self.history_activities();
        // Signatures from the app and from the terminal tool, which trades from the same wallet.
        let mut files = vec![store::url("journal.jsonl")];
        if let Some(home) = std::env::home_dir() { files.push(home.join(".ihatecrypto").join("journal.jsonl")) }
        let journal = tokio::task::spawn_blocking(move || files.into_iter().flat_map(|f| Journal::new(f).entries().unwrap_or_default()).collect::<Vec<_>>())
            .await.unwrap_or_default();
        let (mut fills, mut transfers, mut problem) = (vec![], vec![], None);
        let user = self.st().evm_address();
        if let Some(user) = &user {
            let (f, t) = tokio::join!(trade_history::fills(user), trade_history::transfers(user));
            let read = f.and_then(|f| {
                fills = f;
                t
            });
            match read {
                Ok(t) => transfers = t,
                Err(e) => problem = Some(self.say("Hyperliquid's history didn't load: ", "История Hyperliquid не загрузилась: ") + &e.0),
            }
        }
        let (agent, earlier, stories, rows) = Self::history_tabs_from(&activities, &journal, &fills, &transfers, self.st().russian());
        let out = HistoryTabs { agent, earlier, stories, rows, error: problem, hyperliquid: user.is_some() };
        json::window(&out).map_or_else(|_| "{}".into(), |b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Swift's static `historyTabs(activities:journal:fills:transfers:ru:)`: the three tabs from plain inputs, so a test
    /// can build them.
    pub fn history_tabs_from(activities: &[Activity], journal: &[Entry], fills: &[Fill], transfers: &[Transfer], ru: bool) -> Tabs {
        let say = |en: &str, r: &str| if ru { r.to_string() } else { en.to_string() };
        let agent: Vec<Activity> = activities.iter().rev().filter(|a| !a.lines.is_empty() || !a.bubbles.is_empty() || a.request.is_some()).cloned().collect();
        let earlier = Self::sessions(journal, activities.first().map(|a| a.at), ru);
        let signed = |v: Decimal| format!("{}{}", if v < Decimal::ZERO { "−" } else { "+" }, Fmt::usd(v.abs()));
        let mut by_coin: HashMap<String, Vec<&Activity>> = HashMap::new();
        for a in activities { if let Some(c) = &a.coin { by_coin.entry(c.to_uppercase()).or_default().push(a) } }
        // Newest first, as TradeHistory gives them and the Mac lists them.
        let stories = trade_history::stories(fills).into_iter().map(|story| {
            let mut events: Vec<Event> = story.fills.iter().map(|f| Event {
                kind: (if f.dir.starts_with("Open") { "open" } else { "close" }).into(), at: f.time, label: f.dir.clone(),
                title: format!("{} {} {}", Fmt::num(f.sz, 6), say("at", "по"), Fmt::price(f.px)),
                lines: vec![if f.closed_pnl.is_zero() { say("fee ", "комиссия ") + &Fmt::usd(f.fee) }
                            else { signed(f.closed_pnl) + &say(" · fee ", " · комиссия ") + &Fmt::usd(f.fee) }],
            }).collect();
            // ihatecrypto's own steps on this position: from 15 min before it opened to 2 min after it closed.
            let coin = IntentParser::pretty(&story.coin).to_uppercase();
            let (opened, closed) = (story.opened() - TimeDelta::seconds(900), story.closed().map(|c| c + TimeDelta::seconds(120)));
            let mine = by_coin.get(&coin).or_else(|| by_coin.get(&story.coin.to_uppercase()));
            events.extend(mine.into_iter().flatten().filter(|a| a.at >= opened && closed.is_none_or(|c| a.at <= c)).map(|a| Event {
                kind: "activity".into(), at: a.at,
                label: if a.kind == "automatic" { say("Guardian", "Страж") } else if a.request.is_some() { say("You asked", "Ты попросил") } else { "ihatecrypto".into() },
                title: a.request.clone().or_else(|| a.title.clone()).unwrap_or_default(), lines: a.lines.last().cloned().into_iter().collect(),
            }));
            events.sort_by_key(|e| e.at);
            Story { coin: IntentParser::pretty(&story.coin), long: story.long, open: story.is_open(), opened: story.opened(), closed: story.closed(),
                    realized: story.realized(), fees: story.fees(), peak: story.peak_size(), events }
        }).collect();
        let mut rows: Vec<Row> = vec![];
        for a in activities {
            if let Some(q) = &a.request {
                rows.push(Row { at: a.at, icon: "person".into(), tone: "accent".into(), text: say(&format!("You: “{q}”"), &format!("Ты: «{q}»")),
                                detail: a.lines.first().cloned(), link: None });
            } else if let Some(t) = &a.title {
                rows.push(Row { at: a.at, icon: (if a.kind == "execution" { "signature" } else { "gear" }).into(), tone: (if a.failed { "loss" } else { "accent" }).into(),
                                text: t.clone(), detail: a.lines.last().cloned(), link: None });
            }
        }
        for f in fills {
            let pnl = if f.closed_pnl.is_zero() { String::new() } else { format!(" · {}", signed(f.closed_pnl)) };
            rows.push(Row {
                at: f.time, icon: (if f.dir.starts_with("Open") { "arrow.up.right.circle" } else { "arrow.down.left.circle" }).into(),
                tone: (if f.closed_pnl < Decimal::ZERO { "loss" } else { "profit" }).into(),
                text: format!("{} {} {} {} {}{pnl}", f.dir, IntentParser::pretty(&f.coin), Fmt::num(f.sz, 6), say("at", "по"), Fmt::price(f.px)),
                detail: Some(say("fee ", "комиссия ") + &Fmt::usd(f.fee)), link: f.hash.as_ref().map(|h| format!("https://app.hyperliquid.xyz/explorer/tx/{h}")),
            });
        }
        for t in transfers {
            let icon = if t.amount > Decimal::ZERO { "arrow.down.circle" } else if t.amount < Decimal::ZERO { "arrow.up.circle" } else { "arrow.left.arrow.right" };
            rows.push(Row { at: t.time, icon: icon.into(), tone: "secondary".into(), text: t.text.clone(), detail: None, link: None });
        }
        for e in journal.iter().filter(|e| matches!(e.kind, Kind::Signed | Kind::Settled | Kind::Failed)) {
            let link = match e.chain.as_str() {
                "solana" => Some(format!("https://solscan.io/tx/{}", e.id)),
                "relay" => Some(format!("https://relay.link/transaction/{}", e.id)),
                "near" => Some(format!("https://nearblocks.io/txns/{}", e.id)),
                _ if e.id.starts_with("0x") && e.id.chars().count() == 66 => trade::EvmNetwork::named(&e.chain).map(|n| format!("{}/tx/{}", n.explorer, e.id)),
                _ => None,
            };
            let what = match e.kind { Kind::Settled => say("Settled", "Завершено"), Kind::Failed => say("Failed", "Не вышло"), _ => say("Signed", "Подписано") };
            let failed = e.kind == Kind::Failed;
            rows.push(Row { at: e.at, icon: (if failed { "xmark.octagon" } else { "signature" }).into(), tone: (if failed { "loss" } else { "secondary" }).into(),
                            text: format!("{what} · {}: {}", capitalized(&e.chain), Self::english(&e.note)), detail: None, link });
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.at));
        rows.truncate(400);
        (agent, earlier, stories, rows)
    }

    /// Signatures grouped into sessions (a pause of 10 minutes starts a new one), newest first; each is an activity whose
    /// bubbles are its signatures with what came of them. `before`: None is Swift's distantFuture.
    pub fn sessions(journal: &[Entry], before: Option<Date>, ru: bool) -> Vec<Activity> {
        let mut outcome: HashMap<&str, &Entry> = HashMap::new();
        for e in journal.iter().filter(|e| e.kind != Kind::Signed) { outcome.entry(e.id.as_str()).or_insert(e); }
        let mut signed: Vec<&Entry> = journal.iter().filter(|e| e.kind == Kind::Signed && before.is_none_or(|b| e.at < b)).collect();
        signed.sort_by_key(|e| e.at);
        let mut groups: Vec<Vec<&Entry>> = vec![];
        for e in signed {
            match groups.last_mut() {
                Some(g) if g.last().is_some_and(|l| (e.at - l.at).num_milliseconds() < 600_000) => g.push(e),
                _ => groups.push(vec![e]),
            }
        }
        groups.iter().rev().map(|group| {
            let bubbles: Vec<Bubble> = group.iter().map(|e| {
                let done = outcome.get(e.id.as_str());
                let result = done.map(|o| if o.kind == Kind::Failed { format!("failed: {}", Self::english(&o.note)) } else { Self::english(&o.note) });
                let note = Self::english(&e.note);
                bubble("signed", &capitalized(&e.chain), note.split(" · valid until").next().unwrap_or(&e.note),
                       result.filter(|r| !["", "ok", "success"].contains(&r.as_str())).into_iter().collect(),
                       if done.is_some_and(|o| o.kind == Kind::Failed) { "blocked" } else { "info" })
            }).collect();
            let mut titles: Vec<String> = vec![];
            for b in &bubbles { if !titles.contains(&b.title) { titles.push(b.title.clone()) } }
            let chains: std::collections::BTreeSet<String> = group.iter().map(|e| capitalized(&e.chain)).collect();
            let chains = chains.into_iter().collect::<Vec<_>>().join(", ");
            let n = group.len();
            Activity {
                title: Some(titles.iter().take(3).cloned().collect::<Vec<_>>().join(", ") + if titles.len() > 3 { "…" } else { "" }),
                bubbles,
                lines: vec![if ru { format!("Подписей: {n} · {chains}") } else { format!("{n} signature{} · {chains}", if n == 1 { "" } else { "s" }) }],
                ..Activity::new("execution", group[0].at)
            }
        }).collect()
    }

    /// Journal notes written before the app switched to English.
    pub fn english(note: &str) -> String {
        const WORDS: [(&str, &str); 15] = [
            ("плечо", "leverage"), ("вход", "entry"), ("стоп и тейк", "stop and take-profit"), ("отмена ордеров", "cancel orders"),
            ("закрытие", "close"), ("новый стоп", "new stop"), ("снять старый стоп", "remove old stop"), ("частичный тейк", "partial take-profit"),
            ("частичное закрытие", "partial close"), ("новый тейк", "new take-profit"), ("снять старый тейк", "remove old take-profit"),
            ("стоит в стакане", "resting on the book"), ("HLP депозит", "HLP deposit"), ("лимит со стопом и тейком", "limit with stop and take-profit"),
            ("обернуть SOL", "wrap SOL"),
        ];
        static FILLED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"исполнено\s+([\d.]+)\s+по\s+([\d.]+)").expect("a regex"));
        if let Some((_, en)) = WORDS.iter().find(|w| w.0 == note) { return (*en).into() }
        if let Ok(Some(m)) = FILLED.captures(note) { return format!("filled {} at {}", &m[1], &m[2]) }
        note.replace("закрыть пустой счёт", "close the empty account")
            .replace("закрыть счёт wSOL (SOL развёрнут)", "close the wSOL account (SOL unwrapped)")
            .replace("закрыть счёт долей", "close the share account")
            .replace("счёт долей", "share account")
            .replace("; valid until block", " · valid until block")
            .replace("; действует до блока", " · valid until block")
    }
}

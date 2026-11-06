//! The History block's moves (HistoryMoves.swift): every move the replies on every canvas say was made, and each
//! recurring buy's swap, newest first, with the request it answered; the latest day summed up; and, before anything
//! moved, the latest requests instead.

use std::collections::BTreeSet;

use chrono::{Local, TimeZone};
use ihatecrypto_core::foundation::{Date, adding, number};
use ihatecrypto_core::moved_line::{Kind, MovedLine};
use rust_decimal::Decimal;
use serde::Serialize;

use crate::engine::Engine;
use crate::history_tabs::Activity;
use crate::json;

/// One move ihatecrypto made or saw, as the History block lists it (the Mac's HistoryBoard): what it was, how much, where
/// to, when; the request it came from and the reply it was in, opened in place; the canvas it was said on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MoveRow {
    #[serde(with = "json::date")]
    pub at: Date,
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub amount: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Sent to one of this computer's own wallets: still the user's money, outside the balance.
    pub own: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    pub reply: String,
    /// The canvas it was said on; none for a recurring buy, which runs without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas: Option<String>,
    /// The request as History keeps it (the Mac's ActivityCard opened from the block): how it was understood, the cards
    /// it drew and all its lines. Filled for the block's rows only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
}

/// What the latest day did to the money: «−25 USDT to Automation · 5 sends», and the own wallets it went to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MovesDay {
    #[serde(with = "json::date")]
    pub day: Date,
    pub lines: Vec<String>,
    pub own: Vec<String>,
}

/// `MovedLine.Kind`'s raw value.
fn raw(kind: Kind) -> &'static str {
    match kind {
        Kind::Sent => "sent", Kind::Received => "received", Kind::Swapped => "swapped", Kind::Moved => "moved", Kind::Bought => "bought",
        Kind::Sold => "sold", Kind::Deposited => "deposited", Kind::Withdrew => "withdrew", Kind::Closed => "closed",
    }
}

/// `Calendar.current.startOfDay(for:)`: the day's first moment in this computer's time zone.
fn start_of_day(d: Date) -> Date {
    let midnight = d.with_timezone(&Local).date_naive().and_hms_opt(0, 0, 0).expect("midnight");
    Local.from_local_datetime(&midnight).earliest().map_or(d, |m| m.to_utc())
}

/// A move's row from its line, as said at `at` in answer to `request`.
fn row(l: &MovedLine, at: Date, own: bool, request: Option<String>, reply: &str, canvas: Option<String>) -> MoveRow {
    MoveRow {
        at, kind: raw(l.kind).into(), text: l.text.clone(), amount: l.amount, symbol: l.symbol.clone(), to: l.to.clone(), own, link: l.link.clone(),
        request, reply: reply.into(), canvas, activity: None,
    }
}

impl Engine {
    /// Every move in the replies, on every canvas, newest first, each with the request it answered.
    fn all_moves(&self) -> Vec<(MoveRow, MovedLine)> {
        let own: BTreeSet<String> = self.st().status.as_ref().and_then(|s| s.wallets.as_ref()).map(|w| w.iter().map(|w| w.label.clone()).collect()).unwrap_or_default();
        let mut out = vec![];
        for c in self.history_chats() {
            let mut request: Option<String> = None;
            for m in &c.messages {
                if m.from_user {
                    request = Some(m.text.clone());
                    continue;
                }
                for l in m.text.split('\n').filter_map(MovedLine::new) {
                    let own = l.to.as_ref().is_some_and(|to| own.contains(to));
                    out.push((row(&l, m.at, own, request.clone(), &m.text, Some(c.id.clone())), l));
                }
            }
        }
        // A recurring buy runs with no canvas and no chat line: its run's swap line is the move (the Mac records it as an
        // activity, Execution.swift:1147), so the block and the day's line count it too.
        for r in self.st().schedule_runs.iter().filter(|r| r.canvas.is_none() && r.result == "done") {
            let line = r.line.clone().unwrap_or_default();
            for l in line.split('\n').filter_map(MovedLine::new) {
                out.push((row(&l, r.at, false, Some(r.title.clone()), &line, None), l));
            }
        }
        out.sort_by_key(|m| std::cmp::Reverse(m.0.at));
        out
    }

    /// With nothing moved yet, the History block shows the latest three requests instead (the Mac's HistoryBoard):
    /// each with what was said and drawn for it.
    pub fn recent_requests(&self) -> Vec<Activity> {
        if !self.all_moves().is_empty() { return vec![] }
        self.history_activities().into_iter().rev().filter(|a| !a.lines.is_empty() || !a.bubbles.is_empty() || a.request.is_some()).take(3).collect()
    }

    /// The block's rows: the newest 14, each with the request it came from as History keeps it — the latest one asked
    /// with those words at or before the move.
    pub fn history_moves(&self) -> Vec<MoveRow> {
        let rows: Vec<MoveRow> = self.all_moves().into_iter().take(14).map(|m| m.0).collect();
        // Only the History block opens them: no reading every chat for a state push without one.
        if rows.is_empty() || !self.st().boards().iter().any(|b| b.kind == "history") { return rows }
        let asked: Vec<Activity> = self.history_activities().into_iter().filter(|a| a.kind == "request" && a.request.is_some()).collect();
        rows.into_iter().map(|mut row| {
            let until = adding(row.at, 1.0);
            // Swift's max(by:): the first of the latest.
            row.activity = asked.iter().filter(|a| a.request == row.request && a.at <= until)
                .fold(None::<&Activity>, |m, a| if m.is_none_or(|m| m.at < a.at) { Some(a) } else { m }).cloned();
            row
        }).collect()
    }

    /// The newest move's day, summed up the shared way (MovedLine.summary), which the chat reads too.
    pub fn moves_day(&self) -> Option<MovesDay> {
        let moves = self.all_moves();
        let day = start_of_day(moves.first()?.0.at);
        let that: Vec<&(MoveRow, MovedLine)> = moves.iter().filter(|m| start_of_day(m.0.at) == day).collect();
        let lines: Vec<MovedLine> = that.iter().map(|m| m.1.clone()).collect();
        let own: BTreeSet<String> = that.iter().filter(|m| m.0.own).filter_map(|m| m.0.to.clone()).collect();
        Some(MovesDay { day, lines: MovedLine::summary(&lines), own: own.into_iter().collect() })
    }
}

#[cfg(test)]
#[path = "history_moves/tests.rs"]
mod tests;

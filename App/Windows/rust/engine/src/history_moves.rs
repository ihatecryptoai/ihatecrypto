//! The History block's moves (HistoryMoves.swift).
//! Layer 2 adds here: history_moves and moves_day, read from the journal and the History's record.

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::Serialize;

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

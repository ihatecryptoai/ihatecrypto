//! Moves: their cards, Confirm and running them (Moves.swift).
//! Layer 2 adds here: show (a move's card), confirm, cancel_pending, run (each Pending), the plan's second check,
//! history(), saving addresses from the chat, «Send anyway» and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::Date;
use serde::Serialize;

use crate::json;

/// How the second check read a plan: it matches, it has a doubt (said in its words), or it couldn't run (said why).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanReview {
    Matches,
    Doubt(String),
    NotChecked(String),
}

/// A line of the history the window lists (the journal's).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryLine {
    #[serde(with = "json::date")]
    pub at: Date,
    pub chain: String,
    pub kind: String,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// The EVM networks a move's place can be.
pub const EVM_PLACES: [&str; 9] = ["base", "arbitrum", "ethereum", "polygon", "robinhood", "tempo", "hyperevm", "optimism", "bnb"];
/// The cards a stopped costly move offers: send it anyway, or leave it.
pub const SEND_ANYWAY: &str = "__send anyway";
pub const LEAVE_COSTLY: &str = "__leave it";
/// Exchanges whose logo a saved name brings, by the name's first word: their own sites, nobody else's (the Mac's
/// AddressIcons.exchanges; a first word guessed into «<word>.com» would tell a stranger's site about the save).
pub const EXCHANGES: [(&str, &str); 12] = [
    ("binance", "binance.com"), ("bybit", "bybit.com"), ("okx", "okx.com"), ("coinbase", "coinbase.com"), ("kraken", "kraken.com"),
    ("kucoin", "kucoin.com"), ("bitget", "bitget.com"), ("mexc", "mexc.com"), ("gate", "gate.io"), ("htx", "htx.com"), ("bitfinex", "bitfinex.com"),
    ("gemini", "gemini.com"),
];

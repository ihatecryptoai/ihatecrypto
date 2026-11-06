//! The balances the window shows (Portfolio.swift): a card per network, the coins on it with their dollar value, and the
//! total.
//! Layer 2 adds here: refresh (the reads side by side, read_waiters), each network's read (Read), pricing, the lending
//! reads (LendingReadings), balances.json, last_full_balance, portfolio_facts and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::Serialize;

use crate::json;

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioView {
    pub networks: Vec<Network>,
    #[serde(with = "number")]
    pub total: Decimal,
    /// When the networks were last read: every read, whole or not — what the checks and the chat go by.
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub updated_at: Option<Date>,
    /// The balance on screen is whole (the Mac's balanceReady): this wallet's Solana and Hyperliquid and its Automation
    /// wallet have answered since launch or the switch. Until then the screen shows the last full balance, dimmed.
    pub ready: bool,
    pub errors: Vec<String>,
    /// The wallet's value over the last 7 days, oldest first, the last point now: the sparkline.
    pub equity: Vec<Point>,
    /// Its change over those days; None with fewer than three points, as the Mac draws no line then.
    #[serde(rename = "change7d", skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change7d: Option<Decimal>,
}

/// `PortfolioView.Row`: a coin on a network.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub symbol: String,
    pub amount: String,
    /// The amount as a number, for the «money arrived» check.
    #[serde(with = "number")]
    pub value: Decimal,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Where its cost is kept (CostBasis): the mint on Solana, «chain:contract» elsewhere; None for dollars and perps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// «+$1.20 (+5.0%)» against what it cost, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub gain: Option<Decimal>,
    /// How far the price moved in 24 hours, in %, where its market says; None for dollars.
    #[serde(rename = "change24h", skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change24h: Option<Decimal>,
    /// A market list vouches for it (true), or nothing does (false: «unverified»); None for dollars and gas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    /// «tokens», or «defi» for money put in somewhere (Jupiter Lend, Kamino, Aave, Morpho).
    pub section: String,
    /// Under a deposit's name: «earning», «Allez USDC · 5.42%».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A token found by listing the address (Holding.id): the person can hide it or show it again; None for the
    /// network's dollars, gas and deposits, which always show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// It looks real (Holding.looksReal): verified, or a real pool and a cent's worth. Shown unless the person hid it;
    /// the others are tucked until the person shows them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real: Option<bool>,
    /// One of Robinhood Chain's stock tokens: its Buy and Sell name the stock, not the contract (the Mac's tokenPhrases).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock: Option<bool>,
    /// A deposit managed on its own site (Jupiter's other platforms): a click opens it in the browser, as the Mac's
    /// opens it beside the chat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

impl Row {
    /// `Row(symbol:amount:value:)`, in the tokens section; the rest set after.
    pub fn new(symbol: &str, amount: &str, value: Decimal) -> Self {
        Row {
            symbol: symbol.into(), amount: amount.into(), value, usd: None, icon: None, key: None, contract: None, pnl: None, gain: None, change24h: None,
            verified: None, section: "tokens".into(), note: None, id: None, real: None, stock: None, site: None,
        }
    }
}

/// `PortfolioView.Network`: one network's card.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub key: String,
    pub name: String,
    #[serde(with = "number")]
    pub usd: Decimal,
    pub rows: Vec<Row>,
    /// What its money does, as the Mac's network card says it: «idle $757.59 · earning $2.51», «$0.01 · margin».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Tokens tucked under «N hidden · show»: hidden by the person, or airdrops that look like scams. Out of `usd`.
    pub hidden: Vec<Row>,
    /// Tucked beyond the first hundred (a spammed address has hundreds): counted, not sent.
    pub hidden_more: i64,
}

/// `PortfolioView.Point`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Point {
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(with = "number")]
    pub usd: Decimal,
}

/// `Engine.Read`: one network's read — its card (None: nothing there), or why it failed (the network, the reason).
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    Success(Option<Network>),
    Failure(String, String),
}

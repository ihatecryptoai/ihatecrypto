//! What the Mac's result boards draw, as data (Blocks.swift): the window draws them as the Mac does (TokenBoard,
//! PnLBoard, YieldBubbles, MoversBoard, ApprovalsBoard, JupiterDCABoard, ReceiveAcrossBoard, ServiceAnswerBoard). A board
//! keeps its `lines` too: they're what the model reads about it, and what an older window draws.
//! Layer 2 adds here: last_request, the token card (open_token, load_token, show_token), P/L, movers, approvals, the
//! chart extras (with_chart_extras), recently closed trades, the address book's extras (address-extras.json), the block
//! catalog and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::folders::CanvasIcon;
use crate::from_core::MppAnswer;
use crate::json;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pnl: Option<PnLCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rates: Option<Vec<RateRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movers: Option<MoversCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals: Option<ApprovalsCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dca: Option<Vec<DcaRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receive: Option<ReceiveCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceCard>,
}

/// `BlockData.Fact`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub title: String,
    pub value: String,
}

/// `BlockData.Link`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// One token, as the Mac's TokenBoard: DexScreener's deepest pool and GeckoTerminal's history of it, what's held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCard {
    pub chain: String,
    pub address: String,
    pub symbol: String,
    pub name: String,
    pub full_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// «SPCX · Robinhood Chain · Uniswap · USDC pool».
    pub subtitle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_by: Option<String>,
    #[serde(default)]
    pub stock: bool,
    #[serde(default)]
    pub favorite: bool,
    /// The network's own dollar (USDC, Robinhood's USDG): its Buy sells the network's coin, there's nothing to watch.
    #[serde(default)]
    pub dollar: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub price: Option<Decimal>,
    #[serde(default = "day")]
    pub range: String,
    #[serde(default)]
    pub closes: Vec<f64>,
    /// Each close's time, seconds since 1970 (the time axis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<Vec<f64>>,
    /// The move over the range shown, or the pool's 24h when there's no history.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change: Option<Decimal>,
    /// «5m», «1h», «6h», «24h» → %.
    #[serde(default)]
    pub changes: Vec<Fact>,
    #[serde(default)]
    pub tiles: Vec<Fact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<TokenHeld>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default = "yes")]
    pub loading: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn day() -> String { "day".into() }
fn yes() -> bool { true }

impl TokenCard {
    /// The card before its market is read: loading, the day's range.
    pub fn new(chain: &str, address: &str, symbol: &str, name: &str, full_name: &str, subtitle: &str) -> Self {
        TokenCard {
            chain: chain.into(), address: address.into(), symbol: symbol.into(), name: name.into(), full_name: full_name.into(), image: None,
            subtitle: subtitle.into(), verified: None, verified_by: None, stock: false, favorite: false, dollar: false, price: None, range: day(),
            closes: vec![], times: None, change: None, changes: vec![], tiles: vec![], held: None, links: vec![], loading: true, error: None,
        }
    }
}

/// `TokenCard.Held`: what the wallet holds of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenHeld {
    pub amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
    pub how: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pnl: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub gain: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub today: Option<Decimal>,
    /// «Bought 0.3968 NEAR for $2.00 at $5.04 · … · worth $2.39 now» and «since you bought on Oct 10» (portfolio.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

/// The Mac's PnLBoard: every period read at once, so the picker switches without asking again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PnLCard {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    pub period: String,
    pub reports: Vec<PnLReport>,
    pub held: Vec<PnLHeld>,
}

/// `PnLCard.Report`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PnLReport {
    pub period: String,
    pub title: String,
    #[serde(with = "number")]
    pub trading: Decimal,
    #[serde(with = "number")]
    pub realized: Decimal,
    #[serde(with = "number")]
    pub funding: Decimal,
    #[serde(with = "number")]
    pub unrealized: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub change_since: Option<Date>,
    #[serde(with = "number")]
    pub flows: Decimal,
    pub closed: i64,
    pub wins: i64,
    pub by_asset: Vec<PnLAsset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best: Option<PnLTrade>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worst: Option<PnLTrade>,
}

/// `PnLCard.Asset`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PnLAsset {
    pub coin: String,
    pub name: String,
    #[serde(with = "number")]
    pub realized: Decimal,
    pub closed: i64,
}

/// `PnLCard.Trade`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PnLTrade {
    pub coin: String,
    pub name: String,
    pub long: bool,
    #[serde(with = "json::date")]
    pub closed: Date,
    #[serde(with = "number")]
    pub realized: Decimal,
}

/// `PnLCard.Held`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PnLHeld {
    pub symbol: String,
    pub network: String,
    pub place: String,
    pub amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub gain: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub paid: Option<Decimal>,
    /// «(0.5 of it)»: the cost counts only this much of what's held (the Mac's partly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counted: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateRow {
    pub place: String,
    pub asset: String,
    #[serde(with = "number")]
    pub apy: Decimal,
    /// The phrase a tap puts in the chat; None: nothing to do from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoversCard {
    pub up: Vec<Mover>,
    pub down: Vec<Mover>,
}

/// `MoversCard.Mover`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mover {
    pub coin: String,
    pub name: String,
    pub price: String,
    #[serde(with = "number")]
    pub change: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalsCard {
    pub list: Vec<Approval>,
    pub unread: Vec<String>,
    /// Drawn before the first read is back: «Reading every network's approvals…» (the Mac's ApprovalsBoard loading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loading: Option<bool>,
}

/// `ApprovalsCard.Approval`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub chain: String,
    pub place: String,
    pub symbol: String,
    pub spender: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spender_name: Option<String>,
    pub token: String,
    pub unlimited: bool,
    pub amount: String,
    pub permit2: bool,
    /// The revoke's phrase, put in the chat field.
    pub revoke: String,
}

/// `BlockData.DCARow`: one Jupiter DCA order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DcaRow {
    pub coin: String,
    #[serde(with = "number")]
    pub per_cycle: Decimal,
    pub every: String,
    #[serde(with = "number")]
    pub used: Decimal,
    #[serde(with = "number")]
    pub deposited: Decimal,
    pub fills: i64,
    #[serde(with = "number")]
    pub received: Decimal,
    pub cancel: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceiveCard {
    pub network: String,
    pub title: String,
    pub token: String,
    #[serde(with = "number")]
    pub amount: Decimal,
    pub address: String,
    #[serde(with = "number")]
    pub minimum: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub until: Option<Date>,
    pub arrives: String,
    /// 1Click's word for it, asked every 10 s until it's over.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceCard {
    pub service: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usdc: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    pub answer: MppAnswer,
    pub raw: String,
}

/// What a chart board shows beside its candles (the Mac's ChartBoard): read for the window on each state, never kept.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ChartExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change24h: Option<Decimal>,
    #[serde(default)]
    pub tiles: Vec<Fact>,
    #[serde(default)]
    pub levels: Vec<Level>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<TokenHeld>,
}

/// A line across the chart: the position's entry, its stop or take order, an alert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub price: f64,
    pub label: String,
    pub tint: String,
    pub dashed: bool,
}

/// A trade from Hyperliquid's fills closed in the last 7 days (the Mac's recentlyClosed, under the History block).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosedTrade {
    pub coin: String,
    pub name: String,
    pub long: bool,
    #[serde(with = "json::date")]
    pub closed: Date,
    #[serde(with = "number")]
    pub realized: Decimal,
}

/// What the Mac keeps beside a saved address that the signer's book doesn't: its picture, category and note
/// (address-extras.json, by «network:name»).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AddressExtra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<CanvasIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// «chat» when it was saved from a chat message: the row says «from chat», as the Mac's SavedAddress.source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

//! What the Mac's result boards draw, as data (Blocks.swift): the window draws them as the Mac does (TokenBoard,
//! PnLBoard, YieldBubbles, MoversBoard, ApprovalsBoard, JupiterDCABoard, ReceiveAcrossBoard, ServiceAnswerBoard). A board
//! keeps its `lines` too: they're what the model reads about it, and what an older window draws. Here too: the request
//! that drew a board, the token card (open_token, load_token), a chart opened by a click and its live parts
//! (with_chart_extras), the P/L board, approvals, Jupiter DCA, spreading and rebalancing as route steps, a receive from
//! another network followed, the trades closed this week, and the address book's own extras (address-extras.json).

use std::collections::HashMap;

use defi::rebalance;
use ihatecrypto_core::chat_language::ChatWords;
use ihatecrypto_core::charts::Range;
use ihatecrypto_core::foundation::{Date, Num, Rounding, double, number, rounded, seconds, text};
use ihatecrypto_core::intent::{Fmt, IntentParser, Side, TradeIntent};
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::pnl::{self, PnLCommand, Period};
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use ihatecrypto_core::tokens::TokenMarket;
use ihatecrypto_core::{token_research, trade_history};
use protocols::{one_click, recurring};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::SignerAddress;
use trade::approvals::TokenApproval;
use trade::stocks::{RobinhoodStocks, Token};
use trade::wallet::AllocationPreview;
use trade::{EvmNetwork, Executor};
use unicode_segmentation::UnicodeSegmentation;
use wallet::WalletError;
use wallet::address_check::{self, Network as AddressNetwork};

use crate::canvas::{Action, Board, Money, Step};
use crate::engine::{ChartLine, Engine};
use crate::folders::CanvasIcon;
use crate::from_core::MppAnswer;
use crate::portfolio::Row;
use crate::profit::EquityPoint;
use crate::{json, store};

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
    pub limits: Option<Vec<LimitRow>>,
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

/// One open limit order on 1Click (LimitOrdersBoard): its side, coin, size and price, how much filled, until when, and
/// the chat command its Cancel sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitRow {
    pub id: String,
    pub sell: bool,
    pub coin: String,
    pub coins: String,
    pub price: String,
    pub filled: String,
    /// 0…1 of the order filled.
    pub done: f64,
    pub status: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    pub cancel: String,
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

/// A route card with its words: no money line, drawn as the move's own.
fn step(label: &str, title: &str, lines: Vec<String>) -> Step {
    Step { label: label.into(), title: title.into(), lines, money: None, status: None }
}

/// `$0 > -100 ? usd - usd / (1 + $0 / 100) : nil`: what the day's move did to `usd`.
fn today(usd: Decimal, change: Decimal) -> Option<Decimal> {
    if change <= Decimal::from(-100) { return None }
    (Num::of(usd) - Num::of(usd) / (1 + Num::of(change) / 100)).0
}

impl Engine {
    /// The request that drew a board, not a «yes» or «cancel» said after (the Mac's Refresh asked «cancel» again).
    pub fn last_request(&self) -> Option<String> {
        self.st().messages().iter().rev().find(|m| m.from_user && !ChatWords::confirms(&m.text) && !ChatWords::declines(&m.text)).map(|m| m.text.clone())
    }

    // MARK: The token card

    /// A token by its contract opens its card on the canvas; the market is read after, into the same card.
    pub async fn open_token(&self, address: &str, chain: &str) -> HashMap<String, String> {
        // A dollar row has no contract in the portfolio (no P/L to keep): it's the network's own dollar.
        let address = if address.is_empty() { Self::wallet_dollar(chain).unwrap_or_default() } else { address.to_string() };
        if address.is_empty() { return HashMap::new() }
        let lower = address.to_lowercase();
        let held = self.held_row(chain, &address);
        let fav = self.st().favorites.iter().find(|f| f.chain == chain && f.address.to_lowercase() == lower).cloned();
        let symbol = held.as_ref().map(|h| h.symbol.clone()).or_else(|| fav.as_ref().map(|f| f.symbol.clone())).unwrap_or_else(|| "Token".into());
        // The same token's card again comes to the top instead of a second one.
        let old = self.st().boards().iter().find(|b| b.data.as_ref().and_then(|d| d.token.as_ref())
            .is_some_and(|t| t.address.to_lowercase() == lower && t.chain == chain)).cloned();
        if let Some(old) = old {
            self.set_boards(|boards| {
                boards.retain(|b| b.id != old.id);
                boards.insert(0, old.clone());
            });
            let (me, id) = (self.rc(), old.id.clone());
            tokio::task::spawn_local(async move { me.load_token(&id, Range::Day).await });
            return HashMap::from([("board".into(), old.id)]);
        }
        let network = self.network_title(chain);
        let name = fav.as_ref().map_or_else(|| symbol.clone(), |f| f.name.clone());
        let mut card = TokenCard::new(chain, &address, &symbol, &name, &name, &format!("{symbol} · {network}"));
        card.image = held.as_ref().and_then(|h| h.icon.clone()).or_else(|| fav.as_ref().and_then(|f| f.image.clone()));
        card.favorite = fav.is_some();
        card.dollar = Self::wallet_dollar(chain).is_some_and(|d| d.to_lowercase() == lower);
        let mut b = Board::new("token", &format!("{symbol} · {network}"));
        b.lines = vec![format!("{symbol} on {network}, contract {address}. Its market is loading.")];
        b.data = Some(BlockData { token: Some(card), ..Default::default() });
        let id = self.pin(b);
        self.changed();
        let (me, board) = (self.rc(), id.clone());
        tokio::task::spawn_local(async move { me.load_token(&board, Range::Day).await });
        HashMap::from([("board".into(), id)])
    }

    /// The card's market, history in `range`, the list that vouches for it, and what's held of it.
    pub async fn load_token(&self, id: &str, range: Range) {
        let Some(mut card) = self.st().boards().iter().find(|b| b.id == id).and_then(|b| b.data.as_ref()?.token.clone()) else { return };
        let (chain, address) = (card.chain.clone(), card.address.clone());
        card.range = serde_json::to_value(range).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "day".into());
        let market = TokenMarket::info(&address, &chain).await.ok().flatten();
        let mut points = vec![];
        if let Some(m) = &market {
            match TokenMarket::history(&chain, &m.pool, range).await {
                Ok(p) => {
                    points = p;
                    card.error = if points.len() > 1 { None } else { Some(self.say("No trades in this period.", "За этот период сделок нет.")) };
                }
                Err(e) => card.error = Some(e.0),
            }
        } else {
            let network = self.network_title(&chain);
            card.error = Some(self.say(&format!("No pool trades this token on {network}."), &format!("Ни один пул не торгует этим токеном в {network}.")));
        }
        let stock = if chain == "robinhood" { self.robinhood_stock(&address).await } else { None };
        let verified: Option<(String, bool)> = if chain == "solana" {
            token_research::solana_token(&address).await.ok().flatten().map(|t| ("Jupiter".into(), t.verified))
        } else if stock.is_some() {
            Some(("Robinhood's registry".into(), true))
        } else {
            None
        };
        let held = self.held_row(&chain, &address);
        let network = self.network_title(&chain);
        card.stock = stock.is_some();
        if let Some(m) = &market {
            card.symbol = held.as_ref().map(|h| h.symbol.clone()).or_else(|| stock.as_ref().map(|s| s.symbol.clone())).unwrap_or_else(|| m.symbol.clone());
            card.full_name = stock.as_ref().map_or_else(|| m.name.clone(), |s| s.name.clone());
            card.name = Self::short_name(&card.full_name);
            card.image = card.image.take().or_else(|| m.image_url.clone());
            card.subtitle = [card.symbol.clone(), network.clone(), format!("{} · {} pool", m.dex, m.quote)].join(" · ");
            card.price = m.price_usd;
            card.changes = [("m5", "5m"), ("h1", "1h"), ("h6", "6h"), ("h24", "24h")].into_iter()
                .filter_map(|(key, title)| m.changes.get(key).map(|c| Fact { title: title.into(), value: text(*c) })).collect();
            let tiles: [(String, Option<String>); 6] = [
                (if card.stock { self.say("Tokenized value", "Стоимость токенов") } else { self.say("Market cap", "Капитализация") },
                 m.market_cap.or(m.fdv).map(Self::compact_usd)),
                (self.say("Liquidity", "Ликвидность"), m.liquidity_usd.map(Self::compact_usd)),
                (self.say("Volume 24h", "Объём 24ч"), m.volume24h.map(Self::compact_usd)),
                (self.say("Buys 24h", "Покупок 24ч"), m.buys24h.map(|b| b.to_string())),
                (self.say("Sells 24h", "Продаж 24ч"), m.sells24h.map(|s| s.to_string())),
                (self.say("Pool since", "Пул с"), m.created.map(Self::day_month)),
            ];
            card.tiles = tiles.into_iter().map(|(title, value)| Fact { title, value: value.unwrap_or_else(|| "—".into()) }).collect();
            card.links = m.links.iter().map(|l| Link { label: l.label.clone(), url: l.url.clone() })
                .chain(m.pair_url.iter().map(|u| Link { label: "DexScreener".into(), url: u.clone() })).collect();
        }
        if chain == "robinhood" {
            card.links.push(Link { label: self.say("Explorer", "Обозреватель"), url: format!("https://robinhoodchain.blockscout.com/token/{address}") });
        }
        card.closes = points.iter().map(|p| double(p.close)).collect();
        card.times = Some(points.iter().map(|p| seconds(p.time)).collect());
        card.change = match (points.first(), points.last()) {
            (Some(first), Some(last)) if first.close > Decimal::ZERO => ((Num::of(last.close) - first.close) / first.close * 100).0,
            _ => market.as_ref().and_then(|m| m.changes.get("h24").copied()),
        };
        card.verified = verified.as_ref().map(|v| v.1).or_else(|| held.as_ref().and_then(|h| h.verified));
        card.verified_by = verified.as_ref().map(|v| v.0.clone());
        if let Some(h) = &held {
            let how = if h.usd.is_none() { self.say("no price yet", "цены пока нет") }
                else if card.dollar { self.say("not earning", "не приносит дохода") }
                else if card.stock { self.say("valued at Robinhood's bid", "по цене покупки Robinhood") }
                else { self.say("valued at the pool price", "по цене пула") };
            let mut told = TokenHeld { amount: h.amount.clone(), usd: h.usd, how, pnl: h.pnl.clone(), gain: h.gain,
                                       today: h.usd.and_then(|usd| today(usd, h.change24h?)), paid: None, since: None };
            if !card.dollar && let Some(key) = &h.key {
                let (paid, since) = Self::held_paid(&card.symbol, h.value, h.gain, self.cost(key).as_ref(), self.st().russian());
                (told.paid, told.since) = (Some(paid), since);
            }
            card.held = Some(told);
        }
        card.favorite = self.st().favorites.iter().any(|f| f.chain == chain && f.address.to_lowercase() == address.to_lowercase());
        card.loading = false;
        // What the card shows, for the model: its numbers, the chart in the range picked, what the wallet holds.
        let vouched = verified.as_ref().map_or(String::new(), |(by, ok)| if *ok { format!(", verified by {by}") } else { format!(", not verified by {by}") });
        let mut facts = vec![format!("{} ({}), contract {address} on {network}{vouched}.", card.full_name, card.symbol)];
        match &market {
            Some(m) => facts.extend(token_research::pool_facts(m, chrono::Utc::now())),
            None => facts.push(card.error.clone().unwrap_or_default()),
        }
        facts.extend(token_research::chart_facts(&points, range, "Its chart (shown now)", &chrono::Local));
        if let Some(h) = &held { facts.push(format!("The wallet holds {}{}.", h.amount, h.usd.map_or(String::new(), |u| format!(" ≈ {}", Fmt::usd(u))))) }
        let title = format!("{} · {network}", card.symbol);
        self.update_board(Some(id), |b| {
            b.title = title;
            b.lines = facts.into_iter().filter(|f| !f.is_empty()).collect();
            b.data = Some(BlockData { token: Some(card), ..Default::default() });
        });
    }

    /// A Robinhood Chain token that is one of its stock tokens (its own registry).
    pub async fn robinhood_stock(&self, address: &str) -> Option<Token> {
        if self.st().stock_registry.is_none() {
            let read = RobinhoodStocks::registry().await.ok();
            self.st_mut().stock_registry = read;
        }
        let lower = address.to_lowercase();
        self.st().stock_registry.as_ref()?.iter().find(|t| t.address.to_lowercase() == lower).cloned()
    }

    /// The wallet's row of a token on a network, shown or hidden.
    pub fn held_row(&self, chain: &str, contract: &str) -> Option<Row> {
        let st = self.st();
        let n = st.portfolio.networks.iter().find(|n| n.key == chain)?;
        let lower = contract.to_lowercase();
        let dollar = Self::wallet_dollar(chain).is_some_and(|d| d.to_lowercase() == lower);
        n.rows.iter().chain(&n.hidden).find(|r| r.contract.as_ref().is_some_and(|c| c.to_lowercase() == lower))
            .or_else(|| if dollar { n.rows.iter().find(|r| r.contract.is_none() && r.section == "tokens" && ["USDC", "USDG"].contains(&r.symbol.as_str())) } else { None })
            .cloned()
    }

    /// The network's own dollar: Circle's USDC on Solana and each EVM network, USDG on Robinhood Chain (the Mac's walletDollar).
    pub fn wallet_dollar(chain: &str) -> Option<String> {
        if chain == "solana" { Some(SpotAsset::usdc().mint) } else { EvmNetwork::named(chain).map(|n| n.usdc.to_string()) }
    }

    pub fn network_title(&self, key: &str) -> String {
        if let Some(n) = self.st().portfolio.networks.iter().find(|n| n.key == key) { return n.name.clone() }
        serde_json::from_value::<AddressNetwork>(serde_json::Value::String(key.into())).map_or_else(|_| WalletCommand::place_name(key), |n| n.title().into())
    }

    /// «Space Exploration Technologies Corp. Class A Common Stock» → «Space Exploration Technologies».
    pub fn short_name(full: &str) -> String {
        let cut = [" Corp", ", Inc", " Inc.", " Class ", " Common Stock", " Ltd"].iter().filter_map(|w| full.find(w)).min();
        cut.map_or_else(|| full.to_string(), |c| full[..c].to_string())
    }

    /// $1.2B, $34.5M, $12.3K, $950.
    pub fn compact_usd(d: Decimal) -> String {
        let v = double(d);
        for (limit, suffix) in [(1e9, "B"), (1e6, "M"), (1e3, "K")] {
            if v.abs() >= limit { return format!("${}{suffix}", format!("{:.1}", v / limit).replace(".0", "")) }
        }
        Fmt::usd(rounded(d, 0, Rounding::Plain))
    }

    /// «Oct 10»: the template «d MMM» in en_US.
    pub fn day_month(d: Date) -> String { crate::portfolio::short_day(d, false) }

    // MARK: A chart opened by a click

    /// A coin row or a mover opens its market's chart on the canvas, as the Mac's opener: no line in the chat. Swift's
    /// defaults: the day's range, no network.
    pub async fn open_chart(&self, coin: &str, range: Range, network: Option<String>) -> Result<HashMap<String, String>, WalletError> {
        let mut line = self.chart_line(coin, range).await?;
        // Opened again in another range it keeps the network it was opened from.
        line.network = network.or_else(|| self.st().boards().iter().find(|b| b.kind == "chart" && b.chart.as_ref().and_then(|c| c.coin.as_deref()) == Some(coin))
            .and_then(|b| b.chart.as_ref()?.network.clone()));
        Ok(HashMap::from([("board".into(), self.place_chart(line))]))
    }

    pub async fn chart_line(&self, coin: &str, range: Range) -> Result<ChartLine, WalletError> {
        let candles = Hyperliquid::candles(coin, range, chrono::Utc::now()).await?;
        let pretty = IntentParser::pretty(coin);
        let (Some(first), Some(last)) = (candles.first(), candles.last()) else {
            return Err(WalletError(self.say(&format!("No candles for {pretty}."), &format!("Нет свечей по {pretty}."))));
        };
        if first.close <= Decimal::ZERO { return Err(WalletError(self.say(&format!("No candles for {pretty}."), &format!("Нет свечей по {pretty}.")))) }
        let change = ((Num::of(last.close) - first.close) / first.close * 100).0.unwrap_or_default();
        let low = candles.iter().map(|c| c.low).min().unwrap_or(last.close);
        let high = candles.iter().map(|c| c.high).max().unwrap_or(last.close);
        Ok(ChartLine {
            title: format!("{pretty} · {}", range.title()), closes: candles.iter().map(|c| double(c.close)).collect(),
            times: Some(candles.iter().map(|c| seconds(c.time)).collect()), low: Fmt::price(low), high: Fmt::price(high), change: self.signed(change),
            coin: Some(coin.into()), network: None, extras: None,
        })
    }

    /// One step of a route as its own block for the chat (the Mac's RouteCards.sendToChat): its name, money and rows.
    pub fn step_board(&self, b: &Board, i: i64) -> Option<Board> {
        let s = b.steps.as_ref()?.get(usize::try_from(i).ok()?)?;
        let name = format!("Step {} · {} · {}", i + 1, s.label, s.title);
        let facts: Vec<String> = [s.title.clone()].into_iter()
            .chain(s.money.iter().map(|m| format!("{} {} → {} {}", m.place, m.was, m.after, m.symbol)))
            .chain(s.lines.iter().filter(|l| !l.is_empty()).map(|l| l.replace('\t', " "))).collect();
        let mut board = Board::new("result", &name);
        board.lines = vec![format!("About the step «{name}» of the route «{}» on the canvas ({})", b.title, facts.join(" · "))];
        Some(board)
    }

    /// With a trade plan on the canvas a price alert is for that plan: 2% its way from the price now, the plan as it
    /// stands (the Mac's BlocksPanel example; Alerts.swift ties the two).
    pub fn plan_alert_example(&self) -> Option<String> {
        let st = self.st();
        let intent = &st.intent;
        let managed = st.traded_plan.as_ref() == Some(intent) || st.positions.iter().any(|p| p.coin == intent.asset);
        if *intent == TradeIntent::default() || managed { return None }
        let mark = st.markets.get(&intent.asset)?.mark_px;
        let long = intent.side == Side::Long;
        let level = (Num::of(mark) * if long { Decimal::new(98, 2) } else { Decimal::new(102, 2) }).0?;
        let asset = intent.asset.strip_prefix("xyz:").unwrap_or(&intent.asset);
        Some(format!("when {asset} is {} {}: {}", if long { "below" } else { "above" }, Fmt::price(level), intent.command()))
    }

    // MARK: The chart's live parts

    /// The chart boards as the Mac's ChartBoard reads them on screen: the market's numbers, the position's entry, its
    /// stop and take orders and the alerts across the line, and what's held of the coin at Hyperliquid's price.
    pub fn with_chart_extras(&self, boards: Vec<Board>) -> Vec<Board> {
        boards.into_iter().map(|mut b| {
            if b.kind == "chart" && let Some(line) = &mut b.chart && let Some(coin) = line.coin.clone() {
                line.extras = Some(self.chart_extras(&coin, line.network.as_deref()));
            }
            b
        }).collect()
    }

    pub fn chart_extras(&self, coin: &str, network: Option<&str>) -> ChartExtras {
        let mut x = ChartExtras { network: network.map(str::to_string), ..Default::default() };
        let (m, row, levels) = {
            let st = self.st();
            let m = st.markets.get(coin).cloned();
            let mut levels = vec![];
            if let Some(p) = st.positions.iter().find(|p| p.coin == coin) {
                let label = if p.size > Decimal::ZERO { st.say("Long entry", "Вход в лонг") } else { st.say("Short entry", "Вход в шорт") };
                levels.push(Level { price: double(p.entry), label, tint: "entry".into(), dashed: false });
            }
            for o in st.hl_orders.iter().filter(|o| o.coin == coin && o.is_trigger) {
                let Some(px) = o.trigger_px else { continue };
                let stop = o.order_type.as_deref().unwrap_or("").contains("Stop");
                levels.push(Level { price: double(px), label: if stop { st.say("Stop", "Стоп") } else { st.say("Take", "Тейк") },
                                    tint: if stop { "loss" } else { "profit" }.into(), dashed: false });
            }
            for a in st.alerts.iter().filter(|a| a.coin == coin) {
                levels.push(Level { price: double(a.price), label: st.say("Alert", "Оповещение"), tint: "alert".into(), dashed: true });
            }
            // A coin with one home network is held there all the same when the chart came from the chat.
            let name = IntentParser::pretty(coin).to_uppercase();
            let home = network.or(match coin { "NEAR" => Some("near"), "SOL" => Some("solana"), _ => None });
            let row = home.and_then(|home| st.portfolio.networks.iter().find(|n| n.key == home))
                .and_then(|n| n.rows.iter().find(|r| r.symbol.to_uppercase() == name && r.section == "tokens")).cloned();
            (m, row, levels)
        };
        x.levels = levels;
        if let Some(m) = &m {
            let funding = m.funding.and_then(|f| (Num::of(f) * 100).0).map_or_else(|| "—".into(), |f| format!("{}%", Fmt::num(f, 4)));
            x.change24h = m.change24h();
            x.tiles = vec![
                Fact { title: self.say("Volume 24h", "Объём 24ч"), value: if m.volume24h > Decimal::ZERO { Self::compact_usd(m.volume24h) } else { "—".into() } },
                Fact { title: self.say("Funding 1h", "Фандинг 1ч"), value: funding },
                Fact { title: self.say("Max leverage", "Макс. плечо"), value: format!("{}×", m.max_leverage) },
            ];
        }
        let Some(row) = row.filter(|r| r.value > Decimal::ZERO) else { return x };
        let price = m.as_ref().map(|m| m.mark_px);
        let usd = price.and_then(|p| (Num::of(row.value) * p).0);
        let basis = row.key.as_deref().and_then(|k| self.cost(k));
        let pnl = price.and_then(|p| basis.as_ref()?.pnl(row.value, p));
        let how = if price.is_none() { self.say("no price yet", "цены пока нет") } else { self.say("valued at Hyperliquid's price", "по цене Hyperliquid") };
        let mut held = TokenHeld {
            amount: row.amount.clone(), usd, how,
            pnl: pnl.map(|(gain, pct)| format!("{} ({}{}%)", self.signed_usd(gain), if pct >= Decimal::ZERO { "+" } else { "" }, Fmt::num(pct, 1))),
            gain: pnl.map(|p| p.0), today: usd.and_then(|u| today(u, m.as_ref()?.change24h()?)), paid: None, since: None,
        };
        if row.key.is_some() {
            let (paid, since) = Self::held_paid(&IntentParser::pretty(coin), row.value, pnl.map(|p| p.0), basis.as_ref(), self.st().russian());
            (held.paid, held.since) = (Some(paid), since);
        }
        x.held = Some(held);
        x
    }

    // MARK: P/L

    /// Every period's report and what's held of the coin asked about, for the board.
    pub async fn pnl_card(&self, command: &PnLCommand) -> Option<PnLCard> {
        // Hyperliquid's fills and funding once, then each period from them (profit.rs's pnl_report asks per period).
        let (me, wallet) = {
            let st = self.st();
            (st.evm_address()?, st.solana_address()?)
        };
        let (fills, funding) = tokio::join!(trade_history::fills(&me), trade_history::funding(&me));
        let (fills, funding) = (fills.unwrap_or_default(), funding.unwrap_or_default());
        let now = chrono::Utc::now();
        let (unrealized, updated) = {
            let st = self.st();
            let mut unrealized: HashMap<String, Decimal> = HashMap::new();
            for p in &st.positions {
                let e = unrealized.entry(p.coin.clone()).or_default();
                *e = e.saturating_add(p.unrealized_pnl);
            }
            (unrealized, st.portfolio.updated_at.is_some())
        };
        let mut points: Vec<(Date, Decimal)> = store::load::<Vec<EquityPoint>>("equity.json").unwrap_or_default().into_iter()
            .filter(|p| p.wallet == wallet).map(|p| (p.at, p.usd)).collect();
        if updated { points.push((now, self.rule_equity().unwrap_or_default())) }
        let flows: Vec<pnl::Flow> = store::load::<Vec<pnl::Flow>>("flows.json").unwrap_or_default().into_iter().filter(|f| f.wallet == wallet).collect();
        let reports = Period::ALL.iter().map(|&period| {
            let r = pnl::report(period, command.coin.as_deref(), now, &fills, &funding, &unrealized, &points, &flows);
            let trade = |t: &pnl::TradeLine| PnLTrade { coin: t.coin.clone(), name: IntentParser::pretty(&t.coin), long: t.long, closed: t.closed, realized: t.realized };
            PnLReport {
                period: raw(period), title: self.period_title(period), trading: r.trading(), realized: r.realized, funding: r.funding,
                unrealized: r.unrealized, change: r.change, change_since: r.change_since, flows: r.flows, closed: r.closed_trades as i64, wins: r.wins as i64,
                by_asset: r.by_asset.iter().take(6).map(|a| PnLAsset { coin: a.coin.clone(), name: IntentParser::pretty(&a.coin), realized: a.realized,
                                                                         closed: a.closed_trades as i64 }).collect(),
                best: r.best.as_ref().map(trade), worst: r.worst.as_ref().map(trade),
            }
        }).collect();
        let mut held = vec![];
        if let Some(coin) = &command.coin {
            let symbol = coin.rsplit(':').next().unwrap_or(coin).to_uppercase();
            let networks = self.st().portfolio.networks.clone();
            for n in &networks {
                for row in n.rows.iter().filter(|r| r.symbol.to_uppercase() == symbol && r.value > Decimal::ZERO) {
                    let basis = row.key.as_deref().and_then(|k| self.cost(k));
                    let amount = basis.as_ref().map_or(Decimal::ZERO, |b| b.amount);
                    let partly = row.gain.is_some() && amount > Decimal::ZERO && (Num::of(row.value) * Decimal::new(999, 3)).0.is_some_and(|v| amount < v);
                    held.push(PnLHeld {
                        symbol: row.symbol.clone(), network: n.key.clone(), place: n.name.clone(), amount: row.amount.clone(), usd: row.usd, gain: row.gain,
                        paid: row.gain.and_then(|g| row.usd?.checked_sub(g)), counted: partly.then(|| Fmt::num(amount, 6)),
                    });
                }
            }
        }
        Some(PnLCard { coin: command.coin.clone(), period: raw(command.period), reports, held })
    }

    pub fn period_title(&self, p: Period) -> String {
        if !self.st().russian() { return p.title().into() }
        match p { Period::Day => "24 ч", Period::Week => "7 дней", Period::Month => "30 дней", Period::All => "всё время" }.into()
    }

    // MARK: Approvals, rates, Jupiter DCA

    pub fn approvals_card(&self, list: &[TokenApproval], unread: &[String]) -> ApprovalsCard {
        ApprovalsCard {
            list: list.iter().take(40).map(|a| Approval {
                chain: a.chain.clone(), place: WalletCommand::place_name(&a.chain), symbol: a.symbol.clone(), spender: a.spender.clone(),
                spender_name: a.spender_name.clone(), token: a.token.clone(), unlimited: a.unlimited(),
                amount: if a.unlimited() { self.say("unlimited", "без лимита") } else { a.shown() }, permit2: a.permit2, revoke: revoke(a).summary(),
            }).collect(),
            unread: unread.iter().map(|u| WalletCommand::place_name(u)).collect(),
            loading: None,
        }
    }

    /// The approvals board's ↻: read every network again into the same board.
    pub async fn refresh_approvals(&self, id: &str) {
        let Ok(e) = self.executor() else { return };
        let (list, unread) = e.approvals().await;
        self.fill_approvals(id, &list, &unread);
    }

    /// The approvals board at once, saying it reads every network, as the Mac's does on the first ask; `fill_approvals`
    /// puts what was read into it.
    pub fn reading_approvals(&self) -> String {
        let mut b = Board::new("result", &self.say("Approvals", "Разрешения"));
        b.data = Some(BlockData { approvals: Some(ApprovalsCard { list: vec![], unread: vec![], loading: Some(true) }), ..Default::default() });
        b.request = self.last_request();
        self.pin(b)
    }

    pub fn fill_approvals(&self, id: &str, list: &[TokenApproval], unread: &[String]) {
        let lines = if list.is_empty() { vec![self.say("No token approvals.", "Разрешений нет.")] } else {
            list.iter().take(30).map(|a| format!("{} on {} → {}{}", a.symbol, a.chain, a.spender_name.as_ref().unwrap_or(&a.spender),
                                                    if a.unlimited() { " · unlimited" } else { "" })).collect()
        };
        let actions = (!list.is_empty()).then(|| list.iter().take(12).map(|a| Action {
            title: self.say(&format!("Revoke {} · {}", a.symbol, a.chain), &format!("Отозвать {} · {}", a.symbol, a.chain)), command: revoke(a).summary(),
        }).collect());
        let card = self.approvals_card(list, unread);
        self.update_board(Some(id), |b| {
            b.lines = lines;
            b.actions = actions;
            b.data = Some(BlockData { approvals: Some(card), ..Default::default() });
            b.at = chrono::Utc::now();
        });
    }

    pub fn dca_rows(&self, orders: &[recurring::Order]) -> Vec<DcaRow> {
        orders.iter().map(|o| {
            let coin = SpotAsset::all().iter().find(|a| a.mint == o.output_mint).map_or_else(|| format!("{}…", o.output_mint.chars().take(4).collect::<String>()), |a| a.symbol.clone());
            DcaRow {
                per_cycle: o.per_cycle, every: WalletCommand::every_text(o.every), used: o.used, deposited: o.deposited, fills: o.fills as i64,
                received: o.received, cancel: format!("cancel Jupiter DCA {coin}"), coin,
            }
        }).collect()
    }

    // MARK: Spreading and rebalancing as route cards

    /// The spread as the Mac's route cards (allocationBubbles): what it takes from each network, the moves between them,
    /// then into each place with its rate; what's skipped goes under them.
    pub fn allocation_steps(&self, a: &AllocationPreview) -> (Vec<Step>, Vec<String>) {
        let amount = |d: Decimal| format!("{} {}", Fmt::num(d, if a.asset == "SOL" { 6 } else { 2 }), a.asset);
        let rate = |apy: Decimal| Fmt::num(rounded(apy, 2, Rounding::Plain), 2);
        let placed = a.slices.iter().fold(Decimal::ZERO, |s, x| s.saturating_add(x.amount));
        let drawn = if a.drawn.is_empty() { vec![("Solana".to_string(), placed)] } else { a.drawn.clone() };
        let from = drawn.iter().enumerate().map(|(k, (place, take))| Step {
            lines: if k == 0 && a.leftover > Decimal::ZERO { vec![format!("{}\t{}", self.say("stays free", "остаётся свободным"), amount(a.leftover))] } else { vec![] },
            money: a.sources.iter().find(|s| &s.0 == place).map(|(_, had)| Money {
                place: place.clone(), symbol: a.asset.clone(), was: Fmt::num(*had, 6), after: Fmt::num(had.saturating_sub(*take).max(Decimal::ZERO), 6),
            }),
            ..step(&format!("FROM · {}", place.to_uppercase()), &amount(*take), vec![])
        });
        let moves = a.moves.iter().map(|(from, to, x)| step("BRIDGE · RELAY", &amount(*x), vec![format!("{from} → {to}")]));
        let year = format!("≈ {}{}", amount(a.yearly), a.yearly_usd.map_or(String::new(), |u| format!(" (≈ {})", Fmt::usd(u))));
        let n = a.slices.len();
        let best = step("INTO · BEST RATES", &self.say(&format!("{n} place{}", if n == 1 { "" } else { "s" }), &format!("мест: {n}")),
                        vec![format!("{}\t{year}", self.say("a year", "за год")), format!("{}\t{}", self.say("max per place", "максимум на место"), Fmt::pct(a.max_share))]);
        let into = a.slices.iter().map(|s| step(&format!("INTO · {}", s.place.to_uppercase()), &amount(s.amount),
            vec![format!("{}\t{}", self.say("rate", "ставка"), self.say(&format!("{}% a year", rate(s.apy)), &format!("{}% годовых", rate(s.apy))))]));
        let notes = a.skipped.iter().map(|(place, apy, reason)| self.say(&format!("{place} · {}% — skipped: {reason}", rate(*apy)),
                                                                          &format!("{place} · {}% — пропущено: {reason}", rate(*apy)))).collect();
        (from.chain(moves).chain([best]).chain(into).collect(), notes)
    }

    /// The rebalance as the Mac's RebalanceBoard: the whole, then each move out of one place and into the next with its
    /// rates, the extra interest a year, what it costs and when it pays back; the moves not worth it go under them.
    pub fn rebalance_steps(&self, p: &rebalance::Plan, max_share: Decimal) -> (Vec<Step>, Vec<String>) {
        let rate = |apy: Decimal| Fmt::num(rounded(apy, 2, Rounding::Plain), 2);
        let (gain, cost) = (Fmt::usd(p.yearly_gain()), Fmt::usd(p.cost()));
        let whole = step("REBALANCE · USDC", &self.say(&format!("+{gain} a year"), &format!("+{gain} в год")),
                         vec![format!("{}\t{}", self.say("max per place", "максимум на место"), Fmt::pct(max_share)), format!("{}\t≈ {cost}", self.say("costs", "стоит"))]);
        let moves = p.moves.iter().flat_map(|m| {
            let usdc = format!("{} USDC", Fmt::num(m.amount, 2));
            let mut into = vec![format!("{}\t{}%", self.say("rate", "ставка"), rate(m.to_apy)), format!("{}\t+{}", self.say("a year", "за год"), Fmt::usd(m.yearly_gain())),
                                format!("{}\t≈ {}", self.say("costs", "стоит"), Fmt::usd(m.cost))];
            if let Some(days) = m.payback_days() {
                let d = Fmt::num(rounded(days, 0, Rounding::Up), 2);
                into.push(format!("{}\t{}", self.say("pays back", "окупится"), self.say(&format!("{d} days"), &format!("за {d} дн."))));
            }
            [step(&format!("FROM · {}", m.from.to_uppercase()), &usdc, vec![format!("{}\t{}%", self.say("rate", "ставка"), rate(m.from_apy))]),
             step(&format!("INTO · {}", m.to.to_uppercase()), &usdc, into)]
        });
        let mut notes = vec![];
        if !p.moves.is_empty() {
            notes.push(self.say(&format!("Together ≈ +{gain} a year for ≈ {cost}. Each move goes through Solana: out of one place, into the next, checked and simulated like the single commands."),
                                &format!("Вместе ≈ +{gain} в год за ≈ {cost}. Каждый перенос идёт через Solana: из одного места в другое, с теми же проверками и симуляцией, что и у отдельных команд.")));
        }
        notes.extend(p.skipped.iter().map(|s| self.say(&format!("{} → {} not worth it: {}.", s.step.from, s.step.to, s.reason),
                                                        &format!("{} → {} не стоит того: {}.", s.step.from, s.step.to, s.reason))));
        notes.extend(p.notes.iter().cloned());
        ([whole].into_iter().chain(moves).collect(), notes)
    }

    // MARK: Receive from another network

    /// 1Click's word for the deposit, every 10 s while the board is on a canvas, until it's done or over.
    pub async fn follow_across(&self, id: &str, address: &str) {
        while self.st().canvases.iter().any(|c| c.boards.iter().any(|b| b.id == id)) {
            if let Ok(s) = one_click::status(address).await {
                let word = if s.status == "SUCCESS" {
                    let out = s.amount_out_formatted.clone().unwrap_or_else(|| "?".into());
                    self.say(&format!("done: {out} USDC on Solana"), &format!("готово: {out} USDC на Solana"))
                } else {
                    Executor::one_click_word(&s.status).map_or_else(|| s.status.to_lowercase(), str::to_string)
                };
                self.update_board(Some(id), |b| if let Some(r) = b.data.as_mut().and_then(|d| d.receive.as_mut()) { r.state = word });
                if s.is_final() { return }
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    }

    // MARK: Closed trades

    /// Hyperliquid's trades closed in the last 7 days, read with the balances (the Mac reads them with its account).
    pub async fn read_closed(&self) {
        let Some(me) = self.st().evm_address() else { return };
        let Ok(fills) = trade_history::fills(&me).await else { return };
        // Another wallet since it was asked: its trades aren't this one's (the Mac checks the account, Workspace.swift:2148).
        if self.st().evm_address().map(|a| a.to_lowercase()) != Some(me.to_lowercase()) { return }
        let week = chrono::Utc::now() - chrono::Duration::days(7);
        let mut closed: Vec<ClosedTrade> = trade_history::stories(&fills).into_iter().filter_map(|s| {
            let at = s.closed().filter(|at| *at > week)?;
            Some(ClosedTrade { name: IntentParser::pretty(&s.coin), long: s.long, closed: at, realized: s.realized(), coin: s.coin })
        }).collect();
        closed.sort_by_key(|c| std::cmp::Reverse(c.closed));
        if closed != self.st().recently_closed {
            self.st_mut().recently_closed = closed;
            self.changed();
        }
    }

    // MARK: The address book's own extras and the form's checks

    pub fn extra_key(name: &str, network: &str) -> String { format!("{network}:{}", name.to_lowercase()) }

    /// Read once, then kept: every state carries them.
    pub fn address_extras(&self) -> HashMap<String, AddressExtra> {
        if let Some(e) = &self.st().extras_cache { return e.clone() }
        let e: HashMap<String, AddressExtra> = store::load("address-extras.json").unwrap_or_default();
        self.st_mut().extras_cache = Some(e.clone());
        e
    }

    /// `addressExtras`' setter: kept, and saved.
    pub fn set_address_extras(&self, all: HashMap<String, AddressExtra>) {
        store::save(&all, "address-extras.json");
        self.st_mut().extras_cache = Some(all);
    }

    /// A picture, a category or a note for a saved name; None fields leave what's there, "" clears it.
    pub fn set_address_extra(&self, name: &str, network: &str, icon: Option<&str>, category: Option<&str>, note: Option<&str>) -> Result<(), WalletError> {
        let (mut all, key) = (self.address_extras(), Self::extra_key(name, network));
        let mut e = all.get(&key).cloned().unwrap_or_default();
        if let Some(icon) = icon {
            if icon.is_empty() {
                e.icon = None;
            } else {
                let ok = serde_json::from_str::<CanvasIcon>(icon).ok().and_then(CanvasIcon::checked);
                e.icon = Some(ok.ok_or_else(|| WalletError(self.say("That picture can't be used.", "Эту картинку нельзя использовать.")))?);
            }
        }
        let prefix = |s: &str, n: usize| s.graphemes(true).take(n).collect::<String>();
        if let Some(c) = category { e.category = (!c.is_empty()).then(|| prefix(c, 40)) }
        if let Some(n) = note { e.note = (!n.is_empty()).then(|| prefix(n, 200)) }
        if e == AddressExtra::default() { all.remove(&key); } else { all.insert(key, e); }
        self.set_address_extras(all);
        Ok(())
    }

    /// The form's line under the address as it's typed (the Mac's AddressForm.status): what's wrong with it, or its ends to
    /// compare, whether it's saved already, whether it's this wallet's own. Reads only.
    pub fn check_address(&self, address: &str, network: &str, editing: Option<&str>) -> HashMap<String, String> {
        let a = address.trim();
        if a.is_empty() { return HashMap::new() }
        let Ok(net) = serde_json::from_value::<AddressNetwork>(serde_json::Value::String(network.into())) else {
            return HashMap::from([("problem".into(), self.say("Pick the address's network.", "Выбери сеть адреса."))]);
        };
        if let Some(mut p) = address_check::problem(a, net) {
            let evm = a.starts_with("0x");
            if net == AddressNetwork::Solana && evm {
                p += &self.say(" It looks like an EVM address — pick its network.", " Похоже на EVM-адрес — выбери его сеть.");
            } else if net != AddressNetwork::Solana && !evm && address_check::problem(a, AddressNetwork::Solana).is_none() {
                p += &self.say(" It looks like a Solana address — pick Solana.", " Похоже на адрес Solana — выбери Solana.");
            }
            return HashMap::from([("problem".into(), p)]);
        }
        let (start, end) = address_check::ends(a);
        let mut out = HashMap::from([("start".to_string(), start), ("end".to_string(), end)]);
        let st = self.st();
        let editing = editing.unwrap_or("").to_lowercase();
        if let Some(dup) = st.book.iter().find(|e| e.network == network && e.address.to_lowercase() == a.to_lowercase() && e.name.to_lowercase() != editing) {
            out.insert("duplicate".into(), dup.name.clone());
        }
        if st.solana_address().as_deref() == Some(a) || st.evm_address().is_some_and(|e| e.to_lowercase() == a.to_lowercase()) {
            out.insert("own".into(), "true".into());
        }
        out
    }

    /// A saved address changed in the form: a new name, address, network or memo goes into the signer's book first (Windows
    /// Hello, the whole address on its window), then the old entry goes; its picture and the rest follow the name.
    /// `old`: the entry's name and network as they were.
    pub async fn edit_address(&self, old: (&str, &str), entry: SignerAddress) -> Result<(), WalletError> {
        let previous = self.st().book.iter().find(|e| e.name.to_lowercase() == old.0.to_lowercase() && e.network == old.1).cloned();
        let Some(previous) = previous else { return Err(WalletError(self.say("That saved address is gone.", "Этого сохранённого адреса уже нет."))) };
        let changed = previous.name != entry.name || previous.address != entry.address || previous.network != entry.network || previous.memo != entry.memo;
        if !changed { return Ok(()) }
        self.save_address(entry.clone()).await?;
        if previous.name.to_lowercase() != entry.name.to_lowercase() || previous.network != entry.network {
            self.remove_address(&previous.name, &previous.network).await?;
            let mut all = self.address_extras();
            if let Some(e) = all.remove(&Self::extra_key(&previous.name, &previous.network)) { all.insert(Self::extra_key(&entry.name, &entry.network), e); }
            self.set_address_extras(all);
            let (was, now) = (Self::star_key(&previous), Self::star_key(&entry));
            if self.st().starred.contains(&was) {
                self.set_starred(|s| {
                    s.remove(&was);
                    s.insert(now);
                });
            }
        }
        Ok(())
    }
}

/// `WalletCommand.revokeApproval(chain, token:, spender:, permit2:)`.
fn revoke(a: &TokenApproval) -> WalletCommand {
    WalletCommand::RevokeApproval { place: a.chain.clone(), token: a.token.clone(), spender: a.spender.clone(), permit2: a.permit2 }
}

/// A period's raw value: «day», «week», «month», «all».
fn raw(p: Period) -> String { serde_json::to_value(p).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default() }

#[cfg(test)]
#[path = "blocks/tests.rs"]
mod tests;

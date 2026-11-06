//! A route step's menu and details (RouteEdit.swift, the Mac's RouteCards: «Why this way?», «Edit step…»): what of the
//! waiting move can change — its amount, and for a trade on an EVM network the way (Relay's swap, or Relay's bridge +
//! Uniswap) — and the answer from the step's own facts, which goes in the chat without the model. And a trade plan as
//! its coin's chart draws it (TradeView).
//! Layer 2 adds here: quote_ways, quote_stock, edit_step, take_way, why_step, draw_trade, note (each move's footer),
//! note_route, step_edit.

use std::sync::LazyLock;

use fancy_regex::Regex;
use ihatecrypto_core::foundation::{self, number};
use ihatecrypto_core::position_command::WalletCommand;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::canvas::Step;

/// What the window may change in the move waiting for Confirm: the amount as the command holds it and what it's in, and
/// whether the way can be picked (and which one is).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edit {
    pub board: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    pub unit: String,
    pub ways: bool,
    /// A Robinhood Chain trade: its ways are compared and the best is taken, nothing to pick (ROUTES · BEST IS TAKEN).
    pub stock: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub way: Option<String>,
    /// Both ways as quoted now, the better first; None while they're still being asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quotes: Option<Vec<Way>>,
}

/// One way of an EVM trade as quoted: «relay» or «uniswap» (or «here» on the network's own USDC), what arrives, and its
/// worth in dollars after gas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Way {
    pub way: String,
    pub gets: String,
    pub worth: String,
    /// How much less than the best this way leaves, for a Robinhood Chain trade's list; None for the best.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub less: Option<String>,
    /// The worth as a number, for the fee warning on the chat's quote line (the Mac's way.worth).
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
}

fn matches(text: &str, pattern: &'static LazyLock<Regex>) -> bool { pattern.is_match(text).unwrap_or(false) }

macro_rules! regex {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($p).expect("a regex"));
    };
}

regex!(SPACED, r"^\d{1,3}(?:\s\d{3})+(?:[.,]\d+)?$");
regex!(COMMAS, r"^\d{1,3}(?:,\d{3})+(?:\.\d+)?$");
regex!(DOT_OR_COMMAS, r"\.|,.*,");
regex!(DOTS, r"^\d{1,3}(?:\.\d{3})+(?:,\d+)?$");
regex!(COMMA_OR_DOTS, r",|\..*\.");
regex!(PLAIN, r"^(\d+)(?:([.,])(\d+))?$");

/// The field as one amount: «2», «0,5», «1 000», «1,000.50», «1.000,50». Anything else is None, never the number it
/// starts with («5abc» isn't 5), and so is «1,000» alone: a thousand, or one with three decimals. (RouteCards.amount)
pub fn amount(text: &str) -> Option<Decimal> {
    let mut s = text.trim_matches([' ', '\t']).to_string();
    // Thousands apart by spaces: «1 000», «12 500,5».
    if matches(&s, &SPACED) { s.retain(|c| !c.is_whitespace()) }
    // Thousands apart by the other mark: with a decimal mark after them, or more than one of them.
    if matches(&s, &COMMAS) && matches(&s, &DOT_OR_COMMAS) { s.retain(|c| c != ',') }
    else if matches(&s, &DOTS) && matches(&s, &COMMA_OR_DOTS) { s.retain(|c| c != '.') }
    let m = PLAIN.captures(&s).ok().flatten()?;
    let (int, mark, fraction) = (m.get(1).map_or("", |g| g.as_str()), m.get(2).map(|g| g.as_str()), m.get(3).map(|g| g.as_str()));
    if mark == Some(",") && fraction.is_some_and(|f| f.chars().count() == 3) && int.chars().any(|c| c != '0') { return None }
    foundation::decimal(&s.replace(',', "."))
}

/// The way key of a trade on an EVM network (Workspace.wayKey): «base:aero»; None for any other move.
pub fn way_key(command: &WalletCommand) -> Option<String> {
    match command {
        WalletCommand::BuyOn { place, token, .. } | WalletCommand::SellOn { place, token, .. } => Some(format!("{place}:{}", token.to_lowercase())),
        _ => None,
    }
}

/// What the amount is in: USDC for most moves, SOL to stake, the token for a deposit.
pub fn unit(command: &WalletCommand) -> String {
    match command {
        WalletCommand::Stake(_) => "SOL".into(),
        WalletCommand::LendDeposit(symbol, _) | WalletCommand::Kamino { symbol, .. } => symbol.clone(),
        _ => "USDC".into(),
    }
}

/// `String.capitalized`: a letter after anything but a letter (or an apostrophe) upper case, every other letter lower.
fn capitalized(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    for c in s.chars() {
        let starts = c.is_alphabetic() && prev.is_none_or(|p| !p.is_alphabetic() && p != '\'');
        if starts { out.extend(c.to_uppercase()) } else { out.extend(c.to_lowercase()) }
        prev = Some(c);
    }
    out
}

/// «BRIDGE · RELAY» → «Bridge · Relay» (StepDetails.pretty).
pub fn pretty(label: &str) -> String {
    capitalized(&label.to_lowercase()).replace(" Or ", " or ").replace("Usdc", "USDC").replace("Jitosol", "JitoSOL").replace("Hlp", "HLP")
        .replace("V3", "v3").replace("Layerzero", "LayerZero").replace("Cctp", "CCTP")
}

/// The step's facts as «Why this way?» quotes them: its title, the balance it changes, its lines.
pub fn facts(step: &Step) -> String {
    std::iter::once(step.title.clone())
        .chain(step.money.iter().map(|m| format!("{} {} → {} {}", m.place, m.was, m.after, m.symbol)))
        .chain(step.lines.iter().filter(|l| !l.is_empty()).map(|l| l.replace('\t', " ")))
        .collect::<Vec<_>>().join(" · ")
}

/// A Robinhood Chain trade.
pub fn is_stock_trade(command: &WalletCommand) -> bool { matches!(command, WalletCommand::BuyStock { .. } | WalletCommand::SellStock { .. }) }

/// Which waiting moves the window gets prices for: a Base or Arbitrum trade's two ways, a Robinhood Chain trade's.
pub fn quoted(command: &WalletCommand) -> bool { way_key(command).is_some() || is_stock_trade(command) }

/// A Robinhood Chain trade's way by name (Workspace.stockWayName).
pub fn stock_way_name(way: &str) -> &'static str {
    match way { "v4" => "Uniswap v4", "v3" => "Uniswap v3", _ => "Relay swap" }
}

/// A trade plan as its coin's chart draws it (the Mac's PlanChartBoard, the owner's pick C): the entry, stop and take
/// across the line with the loss and the gain between them, and the trade's numbers under it. Prices as numbers for the
/// drawing, the rest as the card says it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeView {
    pub coin: String,
    pub side: String,
    pub leverage: String,
    /// A limit order's price rather than the market's now («Limit» against «Entry ≈»).
    pub limit: bool,
    pub entry: f64,
    pub stop: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<f64>,
    pub notional: String,
    pub size: String,
    pub margin: String,
    pub liquidation: String,
    pub loss_at_stop: String,
    /// The loss at the stop as a share of the portfolio, «1.2»; None without a portfolio read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_pct: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profit_at_take: Option<String>,
    /// The coin's price alerts, drawn as dashed lines.
    pub alerts: Vec<f64>,
    /// When a card's numbers were read, seconds since 1970: «numbers refreshed N s ago» (the Mac's Execute sheet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared: Option<f64>,
}

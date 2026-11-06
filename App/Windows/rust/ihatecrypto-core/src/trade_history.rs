//! What happened on Hyperliquid, from the exchange itself (`TradeHistory.swift`): every fill, grouped into position
//! stories (open → changes → close), and the money that came in or went out.

use std::collections::BTreeMap;

use chrono::DateTime;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use wallet::{Result, WalletError};

use crate::foundation::{Date, decimal, distant_past, seconds, unix};
use crate::holdings::dicts;
use crate::intent::Fmt;
use crate::network::decode;
use crate::pnl::Funding;

const ZERO: Decimal = Decimal::ZERO;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(from = "RawFill")]
pub struct Fill {
    pub coin: String,
    pub px: Decimal,
    pub sz: Decimal,
    /// "B" buy, "A" sell.
    pub side: String,
    pub time: Date,
    /// Signed position size before this fill.
    pub start_position: Decimal,
    /// «Open Long», «Close Short», …
    pub dir: String,
    pub closed_pnl: Decimal,
    pub fee: Decimal,
    pub hash: Option<String>,
}

/// A fill as Hyperliquid writes it: its numbers as text (a text that isn't one reads as 0), the time in milliseconds.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFill {
    coin: String,
    px: String,
    sz: String,
    side: String,
    time: i64,
    start_position: String,
    dir: String,
    closed_pnl: String,
    fee: String,
    #[serde(default)]
    hash: Option<String>,
}

impl From<RawFill> for Fill {
    fn from(r: RawFill) -> Self {
        let d = |s: &str| decimal(s).unwrap_or_default();
        Fill {
            coin: r.coin, px: d(&r.px), sz: d(&r.sz), side: r.side, time: millis(r.time as f64), start_position: d(&r.start_position), dir: r.dir,
            closed_pnl: d(&r.closed_pnl), fee: d(&r.fee), hash: r.hash,
        }
    }
}

impl Fill {
    /// Position size after this fill.
    pub fn end_position(&self) -> Decimal { self.start_position.saturating_add(if self.side == "B" { self.sz } else { -self.sz }) }
}

/// Hyperliquid's milliseconds: exact when whole (Swift divides by 1000 as a Double, which can land a millisecond short).
pub(crate) fn millis(ms: f64) -> Date {
    if ms.fract() == 0.0 && ms.abs() < 9e15 { DateTime::from_timestamp_millis(ms as i64).unwrap_or_else(distant_past) } else { unix(ms / 1000.0) }
}

/// `"\(date.timeIntervalSince1970)"`: Swift's Double, «1800000000.0».
fn swift_seconds(d: Date) -> String {
    let s = seconds(d).to_string();
    if s.contains(['.', 'e', 'N', 'i']) { s } else { s + ".0" }
}

/// A flip's fill: the position crosses zero (long straight to short).
fn flips(start: Decimal, end: Decimal) -> bool { !start.is_zero() && (start > ZERO) != (end > ZERO) }

/// One position from the first fill to flat again (or still open).
#[derive(Debug, Clone, PartialEq)]
pub struct Story {
    pub coin: String,
    pub long: bool,
    pub fills: Vec<Fill>,
}

impl Story {
    pub fn id(&self) -> String { format!("{}-{}", self.coin, swift_seconds(self.opened())) }

    pub fn opened(&self) -> Date { self.fills.first().map_or_else(distant_past, |f| f.time) }

    pub fn closed(&self) -> Option<Date> { if self.is_open() { None } else { self.fills.last().map(|f| f.time) } }

    /// A flip's fill (long straight to short) closes it: what's left of that fill opens the next story.
    pub fn is_open(&self) -> bool {
        let Some(last) = self.fills.last() else { return false };
        let end = last.end_position();
        !end.is_zero() && !flips(last.start_position, end)
    }

    /// Realized result after fees.
    pub fn realized(&self) -> Decimal { self.fills.iter().fold(ZERO, |s, f| s.saturating_add(f.closed_pnl)).saturating_sub(self.fees()) }

    pub fn fees(&self) -> Decimal { self.fills.iter().fold(ZERO, |s, f| s.saturating_add(f.fee)) }

    /// The largest size the position reached.
    pub fn peak_size(&self) -> Decimal { self.fills.iter().map(|f| f.end_position().abs()).max().unwrap_or(ZERO) }
}

/// Fills in any order → stories, newest first. A story starts when a coin's position leaves zero and ends when it comes
/// back to zero; a flip (long straight to short) closes one story and opens the next.
pub fn stories(fills: &[Fill]) -> Vec<Story> {
    let mut open: BTreeMap<String, Vec<Fill>> = BTreeMap::new();
    let mut out = vec![];
    let mut sorted = fills.to_vec();
    sorted.sort_by_key(|f| f.time);
    for f in sorted {
        let mut current = open.remove(&f.coin).unwrap_or_default();
        current.push(f.clone());
        let end = f.end_position();
        if end.is_zero() || flips(f.start_position, end) {
            let long = current.first().is_some_and(|c| c.end_position() > ZERO);
            out.push(Story { coin: f.coin.clone(), long, fills: current });
            if !end.is_zero() {
                let dir = if end > ZERO { "Open Long" } else { "Open Short" };
                let rest = Fill { sz: end.abs(), start_position: ZERO, dir: dir.into(), closed_pnl: ZERO, fee: ZERO, hash: None, ..f.clone() };
                open.insert(f.coin, vec![rest]);
            }
        } else {
            open.insert(f.coin, current);
        }
    }
    for (coin, fills) in open {
        if !fills.is_empty() { out.push(Story { long: fills[0].end_position() > ZERO, coin, fills }) }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.opened()));
    out
}

/// Money in and out of the Hyperliquid account, in words.
#[derive(Debug, Clone, PartialEq)]
pub struct Transfer {
    pub time: Date,
    pub text: String,
    /// + in, − out, 0 for moves between the account's own balances.
    pub amount: Decimal,
}

impl Transfer {
    pub fn id(&self) -> String { format!("{}-{}", swift_seconds(self.time), self.text) }
}

/// A row's time: `"time"`, a number of milliseconds.
fn row_time(row: &Value) -> Option<Date> { row.get("time")?.as_f64().map(millis) }

/// `userNonFundingLedgerUpdates` in words, newest first.
pub fn parse_transfers(json: &[u8], user: &str) -> Vec<Transfer> {
    let Ok(rows) = serde_json::from_slice::<Value>(json) else { return vec![] };
    let me = user.to_lowercase();
    let dec = |v: Option<&Value>| v.and_then(Value::as_str).and_then(decimal).unwrap_or_default();
    let mut out: Vec<Transfer> = dicts(Some(&rows)).iter().filter_map(|row| {
        let delta = row.get("delta")?.as_object()?;
        let kind = delta.get("type")?.as_str()?;
        let time = row_time(row)?;
        let text_of = |k: &str| delta.get(k).and_then(Value::as_str);
        let (from, to) = (text_of("user").map(str::to_lowercase), text_of("destination").map(str::to_lowercase));
        let t = |text: String, amount: Decimal| Some(Transfer { time, text, amount });
        match kind {
            "deposit" => { let a = dec(delta.get("usdc")); t(format!("Deposit {}", Fmt::usd(a)), a) }
            "withdraw" => { let a = dec(delta.get("usdc")); t(format!("Withdrawal {}", Fmt::usd(a)), -a) }
            "send" | "spotTransfer" => {
                let a = dec(delta.get("usdcValue").or(delta.get("amount")));
                let (source, dest) = (text_of("sourceDex").unwrap_or(""), text_of("destinationDex").unwrap_or(""));
                if from.as_deref() == Some(me.as_str()) && to.as_deref() == Some(me.as_str()) {
                    let place = if dest.is_empty() { "the main balance".to_string() } else { format!("the {dest} exchange") };
                    let origin = if source.is_empty() || source == "spot" { String::new() } else { format!(" from {source}") };
                    return t(format!("Moved {} to {place}{origin}", Fmt::usd(a)), ZERO);
                }
                if to.as_deref() == Some(me.as_str()) { return t(format!("Received {} (bridge in)", Fmt::usd(a)), a) }
                let fee = dec(delta.get("fee"));
                let fee = if fee > ZERO { format!(", fee {}", Fmt::usd(fee)) } else { String::new() };
                t(format!("Sent {} out (bridge to another chain){fee}", Fmt::usd(a)), -a)
            }
            "accountClassTransfer" => {
                let a = dec(delta.get("usdc"));
                let way = if delta.get("toPerp").and_then(Value::as_bool) == Some(true) { "from spot to perps" } else { "from perps to spot" };
                t(format!("Moved {} {way}", Fmt::usd(a)), ZERO)
            }
            "vaultDeposit" => t(format!("Deposited {} into a vault", Fmt::usd(dec(delta.get("usdc")))), ZERO),
            "vaultWithdraw" => t(format!("Withdrew {} from a vault", Fmt::usd(dec(delta.get("usdc").or(delta.get("netWithdrawnUsd"))))), ZERO),
            other => t(other.into(), ZERO),
        }
    }).collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.time));
    out
}

/// `userFunding`'s rows: funding paid (−) or received (+) on positions, HIP-3 markets included.
pub fn parse_funding(json: &[u8]) -> Vec<Funding> {
    let Ok(rows) = serde_json::from_slice::<Value>(json) else { return vec![] };
    dicts(Some(&rows)).iter().filter_map(|row| {
        let delta = row.get("delta")?.as_object()?;
        let coin = delta.get("coin")?.as_str()?;
        let usdc = delta.get("usdc")?.as_str().and_then(decimal)?;
        Some(Funding { time: row_time(row)?, coin: coin.into(), usdc })
    }).collect()
}

pub async fn fills(user: &str) -> Result<Vec<Fill>> {
    decode(&info(&json!({"type": "userFills", "user": user, "aggregateByTime": true})).await?)
}

/// Every fill Hyperliquid still serves (its last 10,000), oldest first, page by page: `userFills` alone gives 2,000.
pub async fn all_fills(user: &str) -> Result<Vec<Fill>> {
    let (mut out, mut start) = (Vec::new(), 0i64);
    for _ in 0..6 {
        let page: Vec<Fill> = decode(&info(&json!({"type": "userFillsByTime", "user": user, "startTime": start, "aggregateByTime": true})).await?)?;
        let next = page.last().map(|f| f.time.timestamp_millis() + 1);
        let full = page.len() >= 2000;
        out.extend(page);
        match next { Some(n) if full => start = n, _ => break }
    }
    Ok(out)
}

/// All funding payments, 500 a page (`funding` reads the first page only).
pub async fn all_funding(user: &str) -> Result<Vec<Funding>> {
    let (mut out, mut start) = (Vec::new(), 0i64);
    for _ in 0..40 {
        let page = parse_funding(&info(&json!({"type": "userFunding", "user": user, "startTime": start})).await?);
        let next = page.last().map(|f| f.time.timestamp_millis() + 1);
        let full = page.len() >= 500;
        out.extend(page);
        match next { Some(n) if full => start = n, _ => break }
    }
    Ok(out)
}

/// Funding paid (−) or received (+) on positions, HIP-3 markets included (userFunding).
pub async fn funding(user: &str) -> Result<Vec<Funding>> {
    Ok(parse_funding(&info(&json!({"type": "userFunding", "user": user, "startTime": 0})).await?))
}

pub async fn transfers(user: &str) -> Result<Vec<Transfer>> {
    Ok(parse_transfers(&info(&json!({"type": "userNonFundingLedgerUpdates", "user": user, "startTime": 0})).await?, user))
}

/// Hyperliquid's `info`, its answer whatever the status (a refusal fails where it's read).
// ponytail: net's client waits 30 s; Swift's request 20 s.
pub(crate) async fn info(body: &Value) -> Result<Vec<u8>> {
    net::post_json("https://api.hyperliquid.xyz/info", body).await.map(|(data, _)| data).map_err(|e| WalletError(e.to_string()))
}

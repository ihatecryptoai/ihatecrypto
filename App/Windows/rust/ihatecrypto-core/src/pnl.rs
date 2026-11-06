//! Profit and loss (`PnL.swift`), from what is known for sure and what is only approximate:
//! - trading (exact, from Hyperliquid itself): realized P/L of the fills in the period after fees, funding paid or
//!   received, and the unrealized P/L of the positions open now;
//! - the portfolio's change: its value now minus its value when the period began, minus money that came in or left
//!   (top-ups, transfers out) — prices, lending yield and trading all end up in it.
//!
//! Lending yield isn't tracked on its own: it is inside the portfolio change. `PnLCommand` reads the chat's question,
//! `CostBasis` what a held token cost. Sums of outside numbers saturate instead of panicking.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::foundation::{self, Date, adding, first, group, has, iso8601_fraction, unix};
use crate::holdings::{amount, dicts, int_value, number};
use crate::intent::{Fmt, IntentParser};
use crate::stablecoins;
use crate::trade_history::{Fill, stories};

const ZERO: Decimal = Decimal::ZERO;

fn sum<'a, T: 'a>(items: impl IntoIterator<Item = &'a T>, f: impl Fn(&T) -> Decimal) -> Decimal {
    items.into_iter().fold(ZERO, |s, x| s.saturating_add(f(x)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Week,
    Month,
    All,
}

impl Period {
    pub const ALL: [Period; 4] = [Period::Day, Period::Week, Period::Month, Period::All];

    pub fn title(self) -> &'static str {
        match self { Period::Day => "24 h", Period::Week => "7 days", Period::Month => "30 days", Period::All => "all time" }
    }

    pub fn since(self, now: Date) -> Option<Date> {
        match self {
            Period::Day => Some(adding(now, -86_400.0)),
            Period::Week => Some(adding(now, -7.0 * 86_400.0)),
            Period::Month => Some(adding(now, -30.0 * 86_400.0)),
            Period::All => None,
        }
    }
}

/// Money that came into the wallet from outside (+) or left it (−), in dollars at the time. Kept per wallet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    #[serde(with = "foundation::date")]
    pub at: Date,
    #[serde(with = "foundation::number")]
    pub usd: Decimal,
    pub note: String,
    pub wallet: String,
}

/// A holding's amount and price at one moment: what the portfolio's P/L line is counted from.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    #[serde(with = "foundation::number")]
    pub amount: Decimal,
    #[serde(with = "foundation::number")]
    pub price: Decimal,
}

/// What prices did to the holdings between two moments: each amount held at the first, times how far its price moved.
/// Money that came in or left, swaps and bridges change amounts, not prices, so the line doesn't jump with them; a coin
/// missing at either moment (sold, or not read) adds nothing.
// ponytail: funding, fees, slippage and lending yield aren't price moves, so they're left out; add them from
// Hyperliquid's ledger and the vaults' rates if the line drifts from the P/L board.
pub fn price_effect(a: &HashMap<String, Mark>, b: &HashMap<String, Mark>) -> Decimal {
    a.iter().fold(ZERO, |s, (key, held)| match b.get(key) {
        Some(now) => s.saturating_add(held.amount.saturating_mul(now.price.saturating_sub(held.price))),
        None => s,
    })
}

/// A funding payment on a Hyperliquid position: − paid, + received.
#[derive(Debug, Clone, PartialEq)]
pub struct Funding {
    pub time: Date,
    pub coin: String,
    pub usdc: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetLine {
    pub coin: String,
    pub realized: Decimal,
    pub closed_trades: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeLine {
    pub coin: String,
    pub long: bool,
    pub closed: Date,
    pub realized: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub period: Period,
    pub coin: Option<String>,
    /// Closed P/L of the period's fills minus all their fees.
    pub realized: Decimal,
    pub funding: Decimal,
    /// Positions open now (the period doesn't apply).
    pub unrealized: Decimal,
    /// Portfolio value change without money moved in or out; None without history for the period (or for one asset).
    pub change: Option<Decimal>,
    /// From when the change is measured: the period's start, or the oldest point kept when history is shorter.
    pub change_since: Option<Date>,
    /// Net money moved in (+) or out (−) during the change's span.
    pub flows: Decimal,
    pub by_asset: Vec<AssetLine>,
    pub closed_trades: usize,
    pub wins: usize,
    pub best: Option<TradeLine>,
    pub worst: Option<TradeLine>,
}

impl Report {
    /// What trading made in the period: realized after fees, plus funding.
    pub fn trading(&self) -> Decimal { self.realized.saturating_add(self.funding) }
}

/// `PnL.report`: `coin` None for the whole wallet, `equity` the portfolio's value over time.
#[allow(clippy::too_many_arguments)]
pub fn report(period: Period, coin: Option<&str>, now: Date, fills: &[Fill], funding: &[Funding], unrealized: &HashMap<String, Decimal>,
              equity: &[(Date, Decimal)], flows: &[Flow]) -> Report {
    let since = period.since(now);
    let in_period = |d: Date| since.is_none_or(|s| d >= s);
    let for_coin = |c: &str| coin.is_none_or(|x| x == c);

    let period_fills: Vec<&Fill> = fills.iter().filter(|f| in_period(f.time) && for_coin(&f.coin)).collect();
    let net = |f: &&Fill| f.closed_pnl.saturating_sub(f.fee);
    let realized = sum(&period_fills, net);
    let paid = sum(funding.iter().filter(|f| in_period(f.time) && for_coin(&f.coin)), |f| f.usdc);
    let open = sum(unrealized.iter().filter(|(k, _)| for_coin(k)).map(|(_, v)| v), |v| *v);

    let closed: Vec<_> = stories(fills).into_iter().filter(|s| !s.is_open() && for_coin(&s.coin) && s.closed().is_some_and(in_period)).collect();
    let lines: Vec<TradeLine> = closed.iter().map(|s| TradeLine { coin: s.coin.clone(), long: s.long, closed: s.closed().unwrap_or(now), realized: s.realized() }).collect();
    let mut coins: Vec<&str> = vec![];
    for f in &period_fills { if !coins.contains(&f.coin.as_str()) { coins.push(&f.coin) } }
    let mut by_asset: Vec<AssetLine> = coins.into_iter().map(|c| AssetLine {
        coin: c.into(),
        realized: sum(period_fills.iter().filter(|f| f.coin == c), net),
        closed_trades: closed.iter().filter(|s| s.coin == c).count(),
    }).collect();
    by_asset.sort_by_key(|a| std::cmp::Reverse(a.realized.abs()));

    // The portfolio change needs the whole wallet: not for one asset.
    let (mut change, mut change_since, mut moved) = (None, None, ZERO);
    let mut points = equity.to_vec();
    points.sort_by_key(|p| p.0);
    if coin.is_none() && let (Some(end), Some(start)) = (points.last(), points.iter().find(|p| in_period(p.0))) && start.0 < end.0 {
        moved = sum(flows.iter().filter(|f| f.at > start.0 && f.at <= end.0), |f| f.usd);
        change = Some(end.1.saturating_sub(start.1).saturating_sub(moved));
        change_since = Some(start.0);
    }

    // Swift's max(by:) and min(by:): the first of equals.
    let best = lines.iter().fold(None::<&TradeLine>, |m, l| if m.is_none_or(|m| m.realized < l.realized) { Some(l) } else { m }).cloned();
    let worst = lines.iter().fold(None::<&TradeLine>, |m, l| if m.is_none_or(|m| l.realized < m.realized) { Some(l) } else { m }).cloned();
    Report {
        period, coin: coin.map(String::from), realized, funding: paid, unrealized: open, change, change_since, flows: moved, by_asset,
        closed_trades: lines.len(), wins: lines.iter().filter(|l| l.realized > ZERO).count(), best, worst,
    }
}

/// For the chat's answers: the facts, one line each, plainly labeled. `pretty` names an asset (Swift's default: as is).
pub fn facts(r: &Report, pretty: &dyn Fn(&str) -> String) -> Vec<String> {
    let s = |d: Decimal| if d < ZERO { format!("−${}", Fmt::num(-d, 2)) } else { format!("+${}", Fmt::num(d, 2)) };
    let on = r.coin.as_deref().map_or(String::new(), |c| format!(" on {}", pretty(c)));
    let mut out = vec![format!(
        "P/L {}{on}: trading {} (realized after fees {}, funding {}), {} closed trades, {} in profit. Open positions now {}.",
        r.period.title(), s(r.trading()), s(r.realized), s(r.funding), r.closed_trades, r.wins, s(r.unrealized)
    )];
    if let (Some(c), Some(since)) = (r.change, r.change_since) {
        out.push(format!(
            "Portfolio {}: {} since {}, not counting {} of money moved in or out (lending yield and price moves are inside this).",
            r.period.title(), s(c), Fmt::date(since), s(r.flows)
        ));
    }
    if let Some(b) = &r.best && b.realized > ZERO { out.push(format!("Best trade: {} {} {}.", side(b.long), pretty(&b.coin), s(b.realized))) }
    if let Some(w) = &r.worst && w.realized < ZERO { out.push(format!("Worst trade: {} {} {}.", side(w.long), pretty(&w.coin), s(w.realized))) }
    out
}

fn side(long: bool) -> &'static str { if long { "long" } else { "short" } }

/// «какой у меня P/L», «pnl за неделю», «сколько я заработал на нефти», «profit and loss week for oil».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnLCommand {
    pub period: Period,
    pub coin: Option<String>,
}

impl PnLCommand {
    const NOT_TICKERS: [&str; 31] = [
        "THE", "MY", "AN", "ALL", "THIS", "LAST", "PAST", "WEEK", "WEEKS", "TODAY", "DAY", "DAYS", "MONTH", "MONTHS", "YEAR", "TIME", "EVERYTHING",
        "TOTAL", "IT", "ME", "EACH", "EVERY",
        // Networks and places: «pnl on base» is the network, not a token.
        "BASE", "SOLANA", "ARBITRUM", "ETHEREUM", "POLYGON", "ROBINHOOD", "TEMPO", "KAMINO", "JUPITER",
    ];

    pub fn parse(text: &str, markets: &HashSet<String>) -> Option<PnLCommand> {
        let q = text.to_lowercase();
        if !has(&q, r"profit\s+and\s+loss|\bp\s*[/&]\s*l\b|\bpnl\b|пнл|прибыл\w*\s+и\s+убыт|сколько\s+(?:я\s+)?(?:заработал|потерял|наторговал)|how\s+much\s+(?:did\s+)?i\s+(?:make|made|earn|earned|lose|lost)") {
            return None;
        }
        let period = if has(&q, r"today|24\s*h|\bday\b|сегодня|за\s+день|сутк|суток") { Period::Day }
            else if has(&q, r"week|7\s*d\b|недел") { Period::Week }
            else if has(&q, r"month|30\s*d\b|месяц") { Period::Month }
            else { Period::All };
        // A ticker no market or holding knows («pnl по AERO» with no AERO left) is still the asset asked about: the
        // answer says there's none, not the whole portfolio's P/L.
        let named = IntentParser::named_asset(text, markets).or_else(|| {
            first(text, r"(?i)\b(?:for|on|по|на)\s+([a-z][a-z0-9]{1,9})\b").map(|c| group(&c, 1).to_uppercase())
                .filter(|t| !Self::NOT_TICKERS.contains(&t.as_str()))
        });
        Some(PnLCommand { period, coin: named })
    }
}

/// What a held token cost, for the P/L on its holding row: the average cost. A buy adds its tokens and dollars; tokens
/// that came in unpaid (a transfer, a bridge, an airdrop) add theirs at the price that day (`came`); a sale or a send
/// takes out the same share of all of it. Only tokens no price reached count for nothing in the P/L.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CostBasis {
    #[serde(with = "foundation::number")]
    pub amount: Decimal,
    #[serde(with = "foundation::number")]
    pub usd: Decimal,
    /// When the first of what's still held came in («since you bought on Oct 10»); None in records kept before it.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "foundation::optional_date")]
    pub since: Option<Date>,
    /// Of `amount` and `usd`: what came in unpaid, at the price the day it came; None when all of it was bought.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub received: Option<Decimal>,
    #[serde(rename = "receivedUSD", default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub received_usd: Option<Decimal>,
}

/// One change in what a wallet holds of a token: tokens in (bought for `paid` dollars, or unpaid: None) or out.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub at: Date,
    pub tokens: Decimal,
    pub paid: Option<Decimal>,
}

impl CostBasis {
    pub const WSOL: &str = "So11111111111111111111111111111111111111112";

    /// Swift's default `at` is now.
    pub fn bought(&mut self, tokens: Decimal, dollars: Decimal, at: Date) {
        if tokens <= ZERO || dollars <= ZERO { return }
        if self.amount.is_zero() { self.since = Some(at) }
        self.amount = self.amount.saturating_add(tokens);
        self.usd = self.usd.saturating_add(dollars);
    }

    /// Tokens that came in unpaid, counted at what they were worth that day.
    pub fn came(&mut self, tokens: Decimal, dollars: Decimal, at: Date) {
        if tokens <= ZERO || dollars <= ZERO { return }
        self.bought(tokens, dollars, at);
        self.received = Some(self.received.unwrap_or_default().saturating_add(tokens));
        self.received_usd = Some(self.received_usd.unwrap_or_default().saturating_add(dollars));
    }

    /// What one of it cost on average.
    pub fn price(&self) -> Option<Decimal> { if self.amount > ZERO { self.usd.checked_div(self.amount) } else { None } }

    pub fn sold(&mut self, tokens: Decimal) {
        if self.amount <= ZERO || tokens <= ZERO { return }
        let (left, before) = (self.amount.saturating_sub(tokens).max(ZERO), self.amount);
        let share = |x: Decimal| x.saturating_mul(left).checked_div(before).unwrap_or_default();
        self.usd = share(self.usd);
        self.received = self.received.map(share);
        self.received_usd = self.received_usd.map(share);
        self.amount = left;
        if left.is_zero() { (self.since, self.received, self.received_usd) = (None, None, None) }
    }

    /// The cost of `held` from its changes, oldest first: tokens bought, at what was paid; tokens in unpaid, at `worth`
    /// then; tokens out take their share. What was held before the oldest change (the chain gives the newest 50) counts
    /// at `worth` then too.
    pub fn fold(changes: &[Change], held: Decimal, worth: impl Fn(Date) -> Option<Decimal>) -> CostBasis {
        let mut basis = CostBasis::default();
        let before = held.saturating_sub(sum(changes, |c| c.tokens));
        if before > ZERO && let Some(first) = changes.first() && let Some(px) = worth(first.at) {
            basis.came(before, before.saturating_mul(px), first.at);
        }
        for c in changes {
            if c.tokens < ZERO {
                basis.sold(-c.tokens);
            } else if let Some(paid) = c.paid && paid > ZERO {
                basis.bought(c.tokens, paid, c.at);
            } else if let Some(px) = worth(c.at) {
                basis.came(c.tokens, c.tokens.saturating_mul(px), c.at);
            }
        }
        basis
    }

    /// The days `fold` asks a price for: the oldest change's and every unpaid one's.
    pub fn unpaid(changes: &[Change]) -> Vec<Date> {
        let days: BTreeSet<Date> = changes.first().map(|c| c.at).into_iter()
            .chain(changes.iter().filter(|c| c.tokens > ZERO && c.paid.unwrap_or_default() <= ZERO).map(|c| c.at))
            .collect();
        days.into_iter().collect()
    }

    /// The P/L of what's held now at `price`: only the tokens with a known cost (at most what was bought here), in
    /// dollars and %.
    pub fn pnl(&self, held: Decimal, price: Decimal) -> Option<(Decimal, Decimal)> {
        if self.amount <= ZERO || self.usd <= ZERO || held <= ZERO { return None }
        let counted = held.min(self.amount);
        let cost = self.usd.saturating_mul(counted).checked_div(self.amount)?;
        let usd = counted.saturating_mul(price).saturating_sub(cost);
        Some((usd, usd.checked_div(cost)?.saturating_mul(Decimal::ONE_HUNDRED)))
    }

    /// An EVM wallet's changes in a token, from Blockscout's token transfers of each transaction that moved it, oldest
    /// first: tokens in with stablecoins leaving the wallet in the same transaction were bought for them; with no dollars
    /// out (a transfer, an airdrop, a cross-chain order's fill) they came in unpaid; tokens out were sold or sent.
    pub fn replay(wallet: &str, contract: &str, transactions: &[Vec<Value>]) -> Vec<Change> {
        let token = contract.to_lowercase();
        let mut out = vec![];
        for transfers in transactions {
            let tokens = sum(moved(transfers, wallet).iter().filter(|m| m.0 == token), |m| m.2);
            let dollars = Self::dollars_out(transfers, wallet);
            let at = transfers.first().and_then(|t| t.get("timestamp")).and_then(Value::as_str).and_then(iso8601_fraction).unwrap_or_else(Utc::now);
            if !tokens.is_zero() { out.push(Change { at, tokens, paid: (tokens > ZERO && dollars > ZERO).then_some(dollars) }) }
        }
        out
    }

    /// The dollars (stablecoins) that left the wallet in one transaction's Blockscout token transfers, net of any back.
    pub fn dollars_out(transfers: &[Value], wallet: &str) -> Decimal {
        -sum(moved(transfers, wallet).iter().filter(|m| stablecoins::DOLLARS.contains(&m.1)), |m| m.2)
    }

    /// A Solana wallet's changes in `mint` from its own transactions, oldest first: tokens in against dollars (Solana's
    /// stablecoins) out in the same transaction, or else SOL out (wrapped too) at `sol_price` then, were bought; with
    /// nothing paid (a transfer, an airdrop) they came in unpaid; tokens out were sold or sent. SOL is `WSOL`: the
    /// wallet's own lamports and its wrapped SOL together.
    pub fn replay_solana(mint: &str, moves: &[SolanaMove], sol_price: impl Fn(Date) -> Option<Decimal>) -> Vec<Change> {
        let stables: HashSet<&str> = stablecoins::on("solana").unwrap_or_default().iter().map(|c| c.address).collect();
        moves.iter().filter_map(|m| {
            let tokens = m.change.get(mint).copied().unwrap_or_default().saturating_add(if mint == Self::WSOL { m.sol } else { ZERO });
            if tokens <= ZERO { return (tokens < ZERO).then_some(Change { at: m.at, tokens, paid: None }) }
            let dollars = -sum(m.change.iter().filter(|(k, v)| stables.contains(k.as_str()) && **v < ZERO).map(|(_, v)| v), |v| *v);
            let sol = if mint == Self::WSOL { ZERO } else { m.sol.saturating_add(m.change.get(Self::WSOL).copied().unwrap_or_default().min(ZERO)) };
            let paid = if dollars > ZERO { Some(dollars) } else if sol < ZERO { sol_price(m.at).map(|p| (-sol).saturating_mul(p)) } else { None };
            Some(Change { at: m.at, tokens, paid })
        }).collect()
    }
}

/// Each transfer as the wallet saw it: the token (lowercased), its symbol (uppercased), and the amount in (+) or out (−).
fn moved(transfers: &[Value], wallet: &str) -> Vec<(String, String, Decimal)> {
    let me = wallet.to_lowercase();
    transfers.iter().filter_map(|t| {
        let info = t.get("token")?.as_object()?;
        let decimals: i64 = info.get("decimals").and_then(Value::as_str).unwrap_or("").parse().ok()?;
        let moved = amount(t.get("total").and_then(|x| x.get("value")), decimals)?;
        let side = |key: &str| t.get(key).and_then(|s| s.get("hash")).and_then(Value::as_str).map(str::to_lowercase);
        let sign = if side("to").as_deref() == Some(me.as_str()) { Decimal::ONE } else if side("from").as_deref() == Some(me.as_str()) { Decimal::NEGATIVE_ONE } else { ZERO };
        let text = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Some((text("address_hash").to_lowercase(), text("symbol").to_uppercase(), sign * moved))
    }).collect()
}

/// One transaction as a Solana wallet saw it: when, what it gained or lost of each mint, and of SOL (the fee given back
/// when it paid it).
#[derive(Debug, Clone, PartialEq)]
pub struct SolanaMove {
    pub at: Date,
    pub change: HashMap<String, Decimal>,
    pub sol: Decimal,
}

impl SolanaMove {
    /// From `getTransaction` (jsonParsed); None for a failed one.
    pub fn from_tx(tx: &Value, owner: &str) -> Option<Self> {
        let meta = tx.get("meta")?.as_object()?;
        if !matches!(meta.get("err"), None | Some(Value::Null)) { return None }
        let mut change: HashMap<String, Decimal> = HashMap::new();
        for (key, sign) in [("postTokenBalances", Decimal::ONE), ("preTokenBalances", Decimal::NEGATIVE_ONE)] {
            for b in dicts(meta.get(key)) {
                if b.get("owner").and_then(Value::as_str) != Some(owner) { continue }
                let (Some(m), Some(ui)) = (b.get("mint").and_then(Value::as_str), b.get("uiTokenAmount")) else { continue };
                let Some(amount) = int_value(ui.get("decimals")).and_then(|d| amount(ui.get("amount"), d)) else { continue };
                let entry = change.entry(m.into()).or_default();
                *entry = entry.saturating_add(sign * amount);
            }
        }
        let keys = dicts(tx.get("transaction").and_then(|t| t.get("message")).and_then(|m| m.get("accountKeys")));
        let mut sol = ZERO;
        if let Some(i) = keys.iter().position(|k| k.get("pubkey").and_then(Value::as_str) == Some(owner))
            && let (Some(pre), Some(post)) = (meta.get("preBalances").and_then(Value::as_array), meta.get("postBalances").and_then(Value::as_array))
            && i < pre.len() && i < post.len()
            && let (Some(before), Some(after)) = (number(pre.get(i)), number(post.get(i)))
        {
            let fee = if i == 0 { number(meta.get("fee")).unwrap_or_default() } else { ZERO };
            sol = after.saturating_sub(before).saturating_add(fee) / Decimal::from(1_000_000_000);
        }
        let at = tx.get("blockTime").and_then(Value::as_f64).map(unix).unwrap_or_else(Utc::now);
        Some(Self { at, change, sol })
    }
}

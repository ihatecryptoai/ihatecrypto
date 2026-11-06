//! A trader's Hyperliquid record measured the way the trading study skill asks (`TradeStudy.swift`, the owner's prompt,
//! 2026-10-16): edge or luck. Closed positions only, each after fees and the funding paid or received while it was open;
//! every number here comes from the exchange's fills, the model only reads them.

use chrono::{Datelike, TimeZone};
use rust_decimal::Decimal;

use crate::foundation::{Date, Num, seconds};
use crate::intent::Fmt;
use crate::pnl::Funding;
use crate::trade_history::{Fill, Story};

const ZERO: Decimal = Decimal::ZERO;

#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    pub coin: String,
    pub long: bool,
    pub opened: Date,
    pub closed: Date,
    /// After fees and funding.
    pub net: Decimal,
    pub fees: Decimal,
    pub funding: Decimal,
    /// The largest the position got, in dollars at the average entry.
    pub peak_usd: Decimal,
    pub entry: Decimal,
    pub adds: usize,
    /// Adds made at a worse price than the average so far: adding to a loser.
    pub adds_losing: usize,
    pub liquidated: bool,
}

impl Trade {
    pub fn hours(&self) -> f64 { (seconds(self.closed) - seconds(self.opened)) / 3600.0 }
}

fn sum<T>(items: &[T], f: impl Fn(&T) -> Decimal) -> Decimal { items.iter().fold(ZERO, |s, x| s.saturating_add(f(x))) }

/// The closed stories as trades, oldest first.
pub fn trades(stories: &[Story], funding: &[Funding]) -> Vec<Trade> {
    let mut out: Vec<Trade> = stories.iter().filter_map(|s| {
        // A position already open when the history begins has no entry to measure from (`cut`).
        let closed = s.closed()?;
        let first = s.fills.first().filter(|f| f.start_position.is_zero())?;
        // A flip (long straight to short) closes this story: its fill counts as a close to zero here.
        let sizes = |f: &Fill| {
            let end = f.end_position();
            let flips = !f.start_position.is_zero() && !end.is_zero() && (f.start_position > ZERO) != (end > ZERO);
            (f.start_position.abs(), if flips { ZERO } else { end.abs() })
        };
        let (mut size, mut cost, mut adds, mut adds_losing, mut peak) = (ZERO, ZERO, 0, 0, ZERO);
        for (i, f) in s.fills.iter().enumerate() {
            let (before, after) = sizes(f);
            if after > before {
                let avg = if size > ZERO { cost.checked_div(size).unwrap_or(f.px) } else { f.px };
                if i > 0 {
                    adds += 1;
                    if if s.long { f.px < avg } else { f.px > avg } { adds_losing += 1 }
                }
                cost = cost.saturating_add((after - before).saturating_mul(f.px));
                size = size.saturating_add(after - before);
                peak = peak.max((Num::of(after) * cost / size).0.unwrap_or(ZERO));
            } else if size > ZERO {
                cost = (Num::of(cost) - Num::of(cost) / size * (before - after)).0.unwrap_or(ZERO);
                size = after;
            }
        }
        let paid = funding.iter().filter(|x| x.coin == s.coin && x.time >= first.time && x.time <= closed).fold(ZERO, |t, x| t.saturating_add(x.usdc));
        let entries: Vec<(Decimal, Decimal)> = s.fills.iter().map(|f| (sizes(f), f.px)).filter(|((b, a), _)| a > b).map(|((b, a), px)| (a - b, px)).collect();
        let bought = sum(&entries, |e| e.0);
        let entry = if bought > ZERO { sum(&entries, |e| e.0.saturating_mul(e.1)).checked_div(bought).unwrap_or(first.px) } else { first.px };
        Some(Trade {
            coin: s.coin.clone(), long: s.long, opened: first.time, closed, net: s.realized().saturating_add(paid), fees: s.fees(), funding: paid,
            peak_usd: peak, entry, adds, adds_losing, liquidated: s.fills.iter().any(|f| f.dir.to_lowercase().contains("liquidat")),
        })
    }).collect();
    out.sort_by_key(|t| t.closed);
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub wins: usize,
    pub total: Decimal,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    /// Won ÷ lost; None with no losses.
    pub profit_factor: Option<Decimal>,
    /// The deepest fall of the running result from its high, in dollars.
    pub max_drawdown: Decimal,
}

impl Stats {
    pub fn win_rate(&self) -> Decimal {
        if self.count > 0 { Decimal::from(self.wins) / Decimal::from(self.count) * Decimal::ONE_HUNDRED } else { ZERO }
    }

    pub fn expectancy(&self) -> Decimal { if self.count > 0 { self.total / Decimal::from(self.count) } else { ZERO } }
}

pub fn stats(trades: &[Trade]) -> Stats {
    let (won, lost): (Vec<&Trade>, Vec<&Trade>) = trades.iter().partition(|t| t.net > ZERO);
    let plus = won.iter().fold(ZERO, |s, t| s.saturating_add(t.net));
    let minus = lost.iter().fold(ZERO, |s, t| s.saturating_add(t.net));
    let (mut running, mut high, mut drawdown) = (ZERO, ZERO, ZERO);
    let mut by_close: Vec<&Trade> = trades.iter().collect();
    by_close.sort_by_key(|t| t.closed);
    for t in by_close {
        running = running.saturating_add(t.net);
        high = high.max(running);
        drawdown = drawdown.max(high.saturating_sub(running));
    }
    let avg = |total: Decimal, n: usize| if n == 0 { ZERO } else { total / Decimal::from(n) };
    Stats {
        count: trades.len(), wins: won.len(), total: plus.saturating_add(minus), avg_win: avg(plus, won.len()), avg_loss: avg(minus, lost.len()),
        profit_factor: if minus < ZERO { plus.checked_div(-minus) } else { None }, max_drawdown: drawdown,
    }
}

/// How few winners make most of the profit: «72% of the profit came from 4 trades» (the trades, the share in %).
pub fn concentration(trades: &[Trade]) -> Option<(usize, Decimal)> {
    let mut wins: Vec<Decimal> = trades.iter().map(|t| t.net).filter(|n| *n > ZERO).collect();
    wins.sort_by(|a, b| b.cmp(a));
    let plus = wins.iter().fold(ZERO, |s, w| s.saturating_add(*w));
    if plus <= ZERO || wins.len() < 3 { return None }
    let mut sum = ZERO;
    for (i, w) in wins.iter().enumerate() {
        sum = sum.saturating_add(*w);
        if sum >= plus / Decimal::TWO { return Some((i + 1, (Num::of(sum) / plus * 100).0?)) }
    }
    None
}

/// Positions the history begins in the middle of: Hyperliquid serves only the last 10,000 fills.
pub fn cut(stories: &[Story]) -> usize { stories.iter().filter(|s| s.fills.first().is_some_and(|f| !f.start_position.is_zero())).count() }

fn plural(n: usize, word: &str) -> String { format!("{n} {word}{}", if n == 1 { "" } else { "s" }) }

/// The record in lines for the model: the whole, without the best trades, by side, coin, holding time and weekday,
/// adding, and the biggest winners and losers. Weekdays in `zone` (Swift's default: this computer's).
pub fn report<Z: TimeZone>(trades: &[Trade], open: usize, cut: usize, zone: &Z) -> String {
    let (Some(first), Some(last)) = (trades.first(), trades.last()) else {
        return format!("No closed Hyperliquid positions in this history{}.", if open > 0 { format!(" ({open} still open)") } else { String::new() });
    };
    let line = |title: &str, t: &[Trade]| {
        let s = stats(t);
        format!(
            "{title}: {}, won {}%, net {}, avg win {}, avg loss {}{}, expectancy {} a trade",
            plural(s.count, "trade"), Fmt::num(s.win_rate(), 0), Fmt::usd(s.total), Fmt::usd(s.avg_win), Fmt::usd(s.avg_loss),
            s.profit_factor.map_or(String::new(), |p| format!(", profit factor {}", Fmt::num(p, 2))), Fmt::usd(s.expectancy())
        )
    };
    let all = stats(trades);
    let still_open = if open > 0 { format!("; {open} still open, not counted") } else { String::new() };
    let begun = if cut > 0 { format!("; {cut} begun before the history, not counted") } else { String::new() };
    let mut out = vec![
        format!("Hyperliquid record, {} – {} (closed positions after fees and funding{still_open}{begun}):", Fmt::date(first.opened), Fmt::date(last.closed)),
        line("All", trades),
        format!("Fees {}, funding {}, worst drawdown {}.", Fmt::usd(sum(trades, |t| t.fees)), Fmt::usd(sum(trades, |t| t.funding)), Fmt::usd(-all.max_drawdown)),
    ];
    if let Some((n, share)) = concentration(trades) {
        out.push(format!("{}% of the profit came from {}.", Fmt::num(share, 0), plural(n, "trade")));
    }
    let mut best = trades.to_vec();
    best.sort_by_key(|t| std::cmp::Reverse(t.net));
    for n in [1, 3] {
        if best.len() > n + 2 && best[n - 1].net > ZERO { out.push(line(&format!("Without the best {n}"), &best[n..])) }
    }
    let side = |long: bool| trades.iter().filter(|t| t.long == long).cloned().collect::<Vec<_>>();
    out.push(line("Longs", &side(true)));
    out.push(line("Shorts", &side(false)));
    let mut coins: Vec<(&str, Vec<Trade>)> = vec![];
    for t in trades {
        match coins.iter_mut().find(|(c, _)| *c == t.coin) { Some((_, v)) => v.push(t.clone()), None => coins.push((&t.coin, vec![t.clone()])) }
    }
    coins.sort_by_key(|c| std::cmp::Reverse(c.1.len()));
    for (coin, t) in coins.iter().take(8) { out.push(line(coin, t)) }
    for (title, from, to) in [("Held under 1 h", 0.0, 1.0), ("Held 1–24 h", 1.0, 24.0), ("Held 1–7 days", 24.0, 168.0), ("Held over 7 days", 168.0, f64::INFINITY)] {
        let t: Vec<Trade> = trades.iter().filter(|t| (from..to).contains(&t.hours())).cloned().collect();
        if !t.is_empty() { out.push(line(title, &t)) }
    }
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let days: Vec<String> = (1..=7).filter_map(|d| {
        let t: Vec<Trade> = trades.iter().filter(|t| t.opened.with_timezone(zone).weekday().number_from_sunday() == d).cloned().collect();
        let s = stats(&t);
        (!t.is_empty()).then(|| format!("{} {} / {}", DAYS[d as usize - 1], s.count, Fmt::usd(s.total)))
    }).collect();
    out.push(format!("By the day opened: {}.", days.join(", ")));
    let added: Vec<Trade> = trades.iter().filter(|t| t.adds > 0).cloned().collect();
    let losing: Vec<Trade> = trades.iter().filter(|t| t.adds_losing > 0).cloned().collect();
    if !added.is_empty() { out.push(line("With adds", &added)) }
    if !losing.is_empty() { out.push(line("Added to a loser", &losing)) }
    let liquidated: Vec<Trade> = trades.iter().filter(|t| t.liquidated).cloned().collect();
    if !liquidated.is_empty() {
        out.push(format!("Liquidated {}, {}.", plural(liquidated.len(), "time"), Fmt::usd(sum(&liquidated, |t| t.net))));
    }
    let story = |t: &Trade| {
        format!(
            "{} {} {} ≈ {} at {}{}, held {} → {}",
            Fmt::date(t.opened), if t.long { "long" } else { "short" }, t.coin, Fmt::usd(t.peak_usd), Fmt::price(t.entry),
            if t.adds > 0 { format!(", {}", plural(t.adds, "add")) } else { String::new() }, held(t.hours()), Fmt::usd(t.net)
        )
    };
    let winners: Vec<String> = best.iter().take(5).filter(|t| t.net > ZERO).map(story).collect();
    let losers: Vec<String> = best.iter().rev().take(5).filter(|t| t.net < ZERO).map(story).collect();
    out.push(format!("Biggest winners: {}.", winners.join("; ")));
    out.push(format!("Biggest losers: {}.", losers.join("; ")));
    out.join("\n")
}

/// «25 min», «30 h», «3 d».
pub fn held(hours: f64) -> String {
    if hours < 1.0 { format!("{} min", ((hours * 60.0) as i64).max(1)) }
    else if hours < 48.0 { format!("{} h", hours.round() as i64) }
    else { format!("{} d", (hours / 24.0).round() as i64) }
}

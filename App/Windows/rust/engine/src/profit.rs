//! The wallet's value over time and its P/L (Profit.swift): equity.json, a point per read, for the portfolio's 7-day
//! line; flows.json, the money moved in or out; and «какой у меня P/L», «pnl за неделю», «сколько я заработал на нефти»:
//! the shared PnL report — Hyperliquid's fills and funding, the open positions, and the portfolio's change without the
//! money moved in or out, as the Mac keeps them.

use std::collections::HashMap;

use ihatecrypto_core::foundation::{Date, number};
use ihatecrypto_core::intent::{Fmt, IntentParser};
use ihatecrypto_core::pnl::{self, Flow, Period, PnLCommand, Report};
use ihatecrypto_core::trade_history;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::blocks::BlockData;
use crate::canvas::Board;
use crate::engine::Engine;
use crate::portfolio::Point;
use crate::{json, store};

/// `Engine.EquityPoint`: what a wallet was worth at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquityPoint {
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(with = "number")]
    pub usd: Decimal,
    pub wallet: String,
}

fn seconds_between(later: Date, earlier: Date) -> f64 { (later - earlier).num_milliseconds() as f64 / 1000.0 }

impl Engine {
    /// A point of the portfolio's value, at most one in ten minutes, a year kept.
    pub fn record_equity(&self, usd: Decimal, wallet: &str) {
        let now = chrono::Utc::now();
        let mut points: Vec<EquityPoint> = store::load("equity.json").unwrap_or_default();
        if points.iter().rev().find(|p| p.wallet == wallet).is_some_and(|last| seconds_between(now, last.at) < 600.0) { return }
        points.push(EquityPoint { at: now, usd, wallet: wallet.into() });
        points.retain(|p| seconds_between(now, p.at) <= 365.0 * 86_400.0);
        store::save(&points, "equity.json");
    }

    /// The week the sparkline draws, for the wallet on screen: its points of the last 7 days, at most 200 (the oldest
    /// kept), and the value now when the balances are in. A watched or newly opened wallet gets its own line at once.
    pub fn load_equity(&self) {
        let (me, updated) = {
            let st = self.st();
            (st.read_wallet(), st.portfolio.updated_at)
        };
        let Some(me) = me else {
            let mut st = self.st_mut();
            st.portfolio.equity = vec![];
            st.portfolio.change7d = None;
            return;
        };
        let wallet = Self::balance_key(&me);
        let since = chrono::Utc::now() - chrono::Duration::days(7);
        let mut week: Vec<EquityPoint> = store::load::<Vec<EquityPoint>>("equity.json").unwrap_or_default().into_iter()
            .filter(|p| p.wallet == wallet && p.at >= since).collect();
        week.sort_by_key(|p| p.at);
        let mut points = Self::thinned(week.into_iter().map(|p| Point { at: p.at, usd: p.usd }).collect(), if updated.is_none() { 200 } else { 199 });
        if let Some(now) = updated { points.push(Point { at: now, usd: self.rule_equity().unwrap_or_default() }) }
        let change = if points.len() > 2 { points[points.len() - 1].usd.checked_sub(points[0].usd) } else { None };
        let mut st = self.st_mut();
        st.portfolio.equity = points;
        st.portfolio.change7d = change;
    }

    /// Every n-th point so that at most `limit` are left, the first always among them.
    pub fn thinned<T>(points: Vec<T>, limit: usize) -> Vec<T> {
        if points.len() <= limit || limit == 0 { return points }
        let step = points.len().div_ceil(limit);
        points.into_iter().step_by(step).collect()
    }

    /// Money that came in (+) or went out (−), so the portfolio's change counts only what prices and trading did.
    pub fn record_flow(&self, usd: Decimal, note: &str) {
        let Some(wallet) = self.st().solana_address() else { return };
        if usd.is_zero() { return }
        let mut flows: Vec<Flow> = store::load("flows.json").unwrap_or_default();
        flows.push(Flow { at: chrono::Utc::now(), usd, note: note.into(), wallet });
        store::save(&flows, "flows.json");
    }

    pub async fn pnl_report(&self, command: &PnLCommand) -> Option<Report> {
        loop {
            let (me, wallet) = {
                let st = self.st();
                (st.evm_address()?, st.solana_address()?)
            };
            let (fills, funding) = tokio::join!(trade_history::fills(&me), trade_history::funding(&me));
            // Another wallet since the request: its own fills, not the last one's against its positions.
            if self.st().evm_address().as_deref() != Some(me.as_str()) { continue }
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
            let flows: Vec<Flow> = store::load::<Vec<Flow>>("flows.json").unwrap_or_default().into_iter().filter(|f| f.wallet == wallet).collect();
            return Some(pnl::report(command.period, command.coin.as_deref(), now, &fills, &funding, &unrealized, &points, &flows));
        }
    }

    pub async fn show_pnl(&self, command: PnLCommand, note: Option<String>) {
        let note = note.unwrap_or_else(|| self.say("P/L · from Hyperliquid and your portfolio", "P/L · из Hyperliquid и твоего портфеля"));
        let Some(r) = self.pnl_report(&command).await else {
            self.reply(&self.say("Add a wallet first.", "Сначала добавь кошелёк."), Some(note));
            return;
        };
        // What the wallet holds of the asset asked about, on each network.
        let mut held = vec![];
        if let Some(coin) = &command.coin {
            let symbol = coin.rsplit(':').next().unwrap_or(coin).to_uppercase();
            let networks = self.st().portfolio.networks.clone();
            for n in &networks {
                for row in n.rows.iter().filter(|r| r.symbol.to_uppercase() == symbol) {
                    held.push(format!("{} {} {} {}{}", row.amount, row.symbol, self.say("on", "в"), n.name, row.usd.map_or(String::new(), |u| format!(" ({})", Fmt::usd(u)))));
                }
            }
            // In no period at all, as the Mac's (PnLBoard.traded over every period): a trade last month still counts today.
            let traded = |r: &Report| !r.realized.is_zero() || !r.funding.is_zero() || !r.unrealized.is_zero() || !r.by_asset.is_empty();
            let mut ever = traded(&r);
            if !ever && command.period != Period::All && let Some(all) = self.pnl_report(&PnLCommand { period: Period::All, coin: Some(coin.clone()) }).await {
                ever = traded(&all);
            }
            if held.is_empty() && !ever {
                let name = IntentParser::pretty(coin);
                self.reply(&self.say(&format!("No {name} in the wallet on any network, and no Hyperliquid trades in it: nothing to count."),
                                     &format!("{name} нет в кошельке ни в одной сети, и сделок по нему на Hyperliquid не было — считать нечего.")), Some(note));
                return;
            }
        }
        let russian = self.st().russian();
        let period = self.period_title(r.period);
        let mut lines = if russian {
            let mut lines = vec![format!(
                "P/L за {period}{}: торговля {} (реализовано после комиссий {}, фандинг {}), закрыто сделок {}, в плюсе {}; открытые позиции сейчас {}.",
                r.coin.as_ref().map_or(String::new(), |c| format!(" по {}", IntentParser::pretty(c))), self.signed_usd(r.trading()), self.signed_usd(r.realized),
                self.signed_usd(r.funding), r.closed_trades, r.wins, self.signed_usd(r.unrealized))];
            if let (Some(c), Some(since)) = (r.change, r.change_since) {
                lines.push(format!("Портфель: {} с {}, без учёта {} пополнений и выводов.", self.signed_usd(c), Fmt::date(since), self.signed_usd(r.flows)));
            }
            if let Some(b) = &r.best && b.realized > Decimal::ZERO {
                lines.push(format!("Лучшая сделка: {} {} {}.", if b.long { "лонг" } else { "шорт" }, IntentParser::pretty(&b.coin), self.signed_usd(b.realized)));
            }
            if let Some(w) = &r.worst && w.realized < Decimal::ZERO {
                lines.push(format!("Худшая сделка: {} {} {}.", if w.long { "лонг" } else { "шорт" }, IntentParser::pretty(&w.coin), self.signed_usd(w.realized)));
            }
            lines
        } else {
            pnl::facts(&r, &IntentParser::pretty)
        };
        if !held.is_empty() { lines.push(self.say("Held now: ", "Сейчас в кошельке: ") + &held.join(", ") + ".") }
        let title = format!("P/L · {period}{}", r.coin.as_ref().map_or(String::new(), |c| format!(" · {}", IntentParser::pretty(c))));
        let data = self.pnl_card(&command).await.map(|c| BlockData { pnl: Some(c), ..Default::default() });
        // `pin("result", title, lines:, data:)`: the request that drew it, not a «yes» or «cancel» said after.
        let mut b = Board::new("result", &title);
        (b.lines, b.data, b.request) = (lines.clone(), data, self.last_request());
        self.pin(b);
        self.reply(&lines.join(" "), Some(note));
    }

    pub fn signed_usd(&self, d: Decimal) -> String { if d < Decimal::ZERO { format!("−${}", Fmt::num(-d, 2)) } else { format!("+${}", Fmt::num(d, 2)) } }

    /// For the model's answers about money: the week and all time.
    pub async fn pnl_facts(&self) -> Vec<String> {
        let mut out = vec![];
        for period in [Period::Week, Period::All] {
            if let Some(r) = self.pnl_report(&PnLCommand { period, coin: None }).await { out.extend(pnl::facts(&r, &IntentParser::pretty)) }
        }
        out
    }
}

#[cfg(test)]
#[path = "profit/tests.rs"]
mod tests;

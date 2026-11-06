//! «chart ETH», the movers, Hyperliquid's candles and the portfolio's split (`Charts.swift`).
// ponytail: `CanvasZoom` and `ScrollTrail` are the Mac canvas's drawing (pinch, trackpad pacing); the Windows window
// draws its own, so they aren't here.

use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wallet::Result;

use crate::foundation::{self, Date, Rounding, has, number, rounded};
use crate::intent::IntentParser;
use crate::modules::ModuleNetwork;
use crate::network::{Hyperliquid, dec, decode, post};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Range {
    Day,
    Week,
    Month,
}

impl Range {
    pub const ALL: [Range; 3] = [Self::Day, Self::Week, Self::Month];

    /// Hyperliquid candle interval and how far back, ms: about 100–170 bars each.
    pub fn interval(self) -> &'static str { match self { Self::Day => "15m", Self::Week => "1h", Self::Month => "4h" } }

    pub fn span(self) -> i64 { match self { Self::Day => 86_400_000, Self::Week => 7 * 86_400_000, Self::Month => 30 * 86_400_000 } }

    pub fn title(self) -> &'static str { match self { Self::Day => "24 h", Self::Week => "7 days", Self::Month => "30 days" } }
}

/// A Hyperliquid candle (`Hyperliquid.Candle`), as `Codable` writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    #[serde(with = "foundation::date")]
    pub time: Date,
    #[serde(with = "number")]
    pub open: Decimal,
    #[serde(with = "number")]
    pub high: Decimal,
    #[serde(with = "number")]
    pub low: Decimal,
    #[serde(with = "number")]
    pub close: Decimal,
}

impl Hyperliquid {
    /// Candles for a chart, oldest first. HIP-3 markets («xyz:CL») are asked by their full name.
    pub async fn candles(coin: &str, range: Range, now: Date) -> Result<Vec<Candle>> {
        let end = (foundation::seconds(now) * 1000.0) as i64;
        let body = json!({"type": "candleSnapshot", "req": {"coin": coin, "interval": range.interval(), "startTime": end - range.span(), "endTime": end}});
        Self::parse_candles(&post(Self::INFO, body).await?)
    }

    /// Candles of any interval («1h», «4h», «1d») between two times, oldest first; Hyperliquid gives at most 5,000.
    pub async fn candles_between(coin: &str, interval: &str, from: Date, to: Date) -> Result<Vec<Candle>> {
        let ms = |d: Date| (foundation::seconds(d) * 1000.0) as i64;
        let body = json!({"type": "candleSnapshot", "req": {"coin": coin, "interval": interval, "startTime": ms(from), "endTime": ms(to)}});
        Self::parse_candles(&post(Self::INFO, body).await?)
    }

    pub fn parse_candles(data: &[u8]) -> Result<Vec<Candle>> {
        #[derive(Deserialize)]
        struct Row {
            t: i64,
            o: String,
            h: String,
            l: String,
            c: String,
        }
        let rows: Vec<Row> = decode(data)?;
        let mut out = rows.iter()
            .map(|r| Ok(Candle {
                time: foundation::unix(r.t as f64 / 1000.0),
                open: dec(Some(&r.o))?,
                high: dec(Some(&r.h))?,
                low: dec(Some(&r.l))?,
                close: dec(Some(&r.c))?,
            }))
            .collect::<Result<Vec<_>>>()?;
        out.sort_by_key(|c| c.time);
        Ok(out)
    }
}

/// «chart ETH», «график нефти за неделю», «покажи эфир»: a price chart of one market on the canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartCommand {
    Chart { coin: String, range: Range },
    /// «what's moving», «что растёт»: the biggest movers of the day.
    Movers,
}

impl ChartCommand {
    pub fn parse(text: &str, markets: &HashSet<String>) -> Option<Self> {
        let q = text.to_lowercase();
        if has(&q, r"\bmovers?\b|what'?s\s+(?:moving|pumping|dumping|hot)|top\s+(?:gainers|losers|coins)|что\s+(?:растёт|растет|падает|движется|качает)|лидеры\s+(?:роста|падения)|топ\s+(?:монет|движени|рост)") {
            return Some(Self::Movers);
        }
        if !has(&q, r"\bcharts?\b|\bgraph\b|график|свеч|candles?|^\s*(?:show|покажи)\b|how'?s\s+\S+\s+(?:doing|looking)|как\s+там\b") { return None }
        let coin = IntentParser::named_asset(text, markets)?;
        let range = if has(&q, r"month|месяц|30\s*(?:d|дн)") { Range::Month } else if has(&q, r"week|недел|7\s*(?:d|дн)") { Range::Week } else { Range::Day };
        Some(Self::Chart { coin, range })
    }
}

/// Where the money is, as shares of the whole: for the portfolio ring.
pub struct PortfolioSplit;

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub name: String,
    pub usd: Decimal,
    pub share: Decimal,
}

impl PortfolioSplit {
    /// Positive parts only, largest first; shares sum to 100 (the last one takes the rounding). Empty when the parts'
    /// sum is past rust_decimal's range.
    pub fn slices(parts: &[(&str, Decimal)]) -> Vec<Slice> {
        let mut positive: Vec<&(&str, Decimal)> = parts.iter().filter(|p| p.1 > Decimal::ZERO).collect();
        positive.sort_by_key(|b| std::cmp::Reverse(b.1));
        let Some(total) = positive.iter().try_fold(Decimal::ZERO, |a, p| a.checked_add(p.1)) else { return vec![] };
        if total <= Decimal::ZERO { return vec![] }
        let mut out = Vec::new();
        let mut used = Decimal::ZERO;
        for (i, p) in positive.iter().enumerate() {
            // A part over the total times 100 stays within range: it's at most 100.
            let share = if i == positive.len() - 1 { Decimal::ONE_HUNDRED - used } else { rounded(p.1 / total * Decimal::ONE_HUNDRED, 1, Rounding::Plain) };
            used += share;
            out.push(Slice { name: p.0.into(), usd: p.1, share });
        }
        out
    }

    /// The portfolio's network cards, in order: Solana, Hyperliquid and Robinhood Chain first (the user's priority),
    /// each when its account is there or it holds money; then the others by money, the most first. One of the others
    /// holding $100 or less is folded under «Other networks» instead of getting a card.
    pub fn network_cards(held: &HashMap<ModuleNetwork, Decimal>, present: &HashSet<ModuleNetwork>) -> (Vec<ModuleNetwork>, Vec<ModuleNetwork>) {
        let pinned = [ModuleNetwork::Solana, ModuleNetwork::Hyperliquid, ModuleNetwork::Robinhood];
        let cent = Decimal::new(1, 2);
        let mut rest: Vec<(ModuleNetwork, Decimal)> = held.iter().filter(|(k, v)| !pinned.contains(k) && **v >= cent).map(|(k, v)| (*k, *v)).collect();
        rest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.raw().cmp(b.0.raw())));
        let hundred = Decimal::ONE_HUNDRED;
        let cards = pinned.into_iter().filter(|n| present.contains(n) || held.get(n).copied().unwrap_or_default() >= cent)
            .chain(rest.iter().filter(|r| r.1 > hundred).map(|r| r.0))
            .collect();
        (cards, rest.iter().filter(|r| r.1 <= hundred).map(|r| r.0).collect())
    }
}

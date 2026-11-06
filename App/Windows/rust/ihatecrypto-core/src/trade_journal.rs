//! One journal entry (`TradeJournal.swift`, JournalCommand): the trader's words and what the market was doing as they
//! wrote them, so the trading study can set the reasoning against what happened next.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::foundation::{self, Date, Num, Rounding, Uuid, decimal, rounded, seconds};
use crate::holdings::strict_dicts;
use crate::intent::Fmt;
use crate::trade_history::info;
use crate::trade_study::held;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: Uuid,
    #[serde(with = "foundation::date")]
    pub at: Date,
    /// Hyperliquid's name for the market: «HYPE», «xyz:GOLD».
    pub asset: String,
    pub words: String,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub price: Option<Decimal>,
    /// Hourly; positive means longs pay shorts.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub funding: Option<Decimal>,
    #[serde(rename = "openInterestUSD", default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub open_interest_usd: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub volume24h: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "foundation::number::option")]
    pub change24h: Option<Decimal>,
    /// «BTC $67,120 (24h +1.2%)», «ETH …».
    pub market: Vec<String>,
    /// Their position in it then, as the portfolio says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
}

impl JournalEntry {
    pub fn new(at: Date, asset: &str, words: &str) -> Self {
        Self {
            id: Uuid::new(), at, asset: asset.into(), words: words.into(), price: None, funding: None, open_interest_usd: None, volume24h: None,
            change24h: None, market: vec![], position: None,
        }
    }

    /// What was kept with it, in a line.
    pub fn snapshot(&self) -> String {
        let parts: Vec<String> = [
            self.price.map(|p| format!("price {}", Fmt::price(p))),
            self.change24h.map(|c| format!("24h {}", change(c))),
            self.funding.and_then(|f| Some(format!("funding {}% an hour", Fmt::num(rounded(f.checked_mul(Decimal::ONE_HUNDRED)?, 4, Rounding::Plain), 4)))),
            self.open_interest_usd.map(|o| format!("open interest {}", big(o))),
            self.volume24h.map(|v| format!("24h volume {}", big(v))),
        ].into_iter().flatten().collect();
        let market = if self.market.is_empty() { String::new() } else { format!("; {}", self.market.join(", ")) };
        let position = self.position.as_ref().map_or(String::new(), |p| format!("; their position: {p}"));
        format!("{}{market}{position}", parts.join(", "))
    }

    /// The entry for the study's review: when, their words, the snapshot, and what the price did since.
    pub fn review(&self, price: Option<Decimal>, now: Date) -> String {
        let since = match (self.price, price) {
            (Some(then), Some(price)) if then > Decimal::ZERO => format!(
                " → since: {} → {} ({}) in {}",
                Fmt::price(then), Fmt::price(price), ((Num::of(price) - then) / then * 100).0.map_or(String::new(), change),
                held((seconds(now) - seconds(self.at)) / 3600.0)
            ),
            _ => String::new(),
        };
        format!("{} {}: «{}» [{}]{since}", Fmt::date(self.at), self.asset, self.words, self.snapshot())
    }

    /// Open interest in dollars now, from Hyperliquid (a HIP-3 market from its own exchange: «xyz:GOLD» → dex xyz).
    pub async fn open_interest_usd(coin: &str) -> Option<Decimal> {
        let mut body = json!({"type": "metaAndAssetCtxs"});
        if coin.contains(':') && let Some(dex) = coin.split(':').find(|s| !s.is_empty()) { body["dex"] = dex.into() }
        Self::parse_open_interest_usd(&info(&body).await.ok()?, coin)
    }

    /// Open interest in dollars from a `metaAndAssetCtxs` answer: the coins open × the mark price.
    pub fn parse_open_interest_usd(json: &[u8], coin: &str) -> Option<Decimal> {
        let pair: Vec<Value> = serde_json::from_slice::<Value>(json).ok()?.as_array()?.clone();
        if pair.len() != 2 { return None }
        let universe = strict_dicts(pair[0].get("universe"))?;
        let ctxs = strict_dicts(Some(&pair[1]))?;
        let i = universe.iter().position(|u| u.get("name").and_then(Value::as_str) == Some(coin)).filter(|i| *i < ctxs.len())?;
        let field = |k: &str| ctxs[i].get(k)?.as_str().and_then(decimal);
        field("openInterest")?.checked_mul(field("markPx")?)
    }
}

/// «$1.64B», «$585.3M».
fn big(d: Decimal) -> String {
    let billion = Decimal::from(1_000_000_000);
    if d >= billion { format!("${}B", Fmt::num(rounded(d / billion, 2, Rounding::Plain), 2)) }
    else { format!("${}M", Fmt::num(rounded(d / Decimal::from(1_000_000), 1, Rounding::Plain), 2)) }
}

fn change(d: Decimal) -> String { format!("{}{}%", if d >= Decimal::ZERO { "+" } else { "−" }, Fmt::num(rounded(d.abs(), 2, Rounding::Plain), 2)) }

//! Part of `Plan.swift`, what round 1 needed (the Plan round completes this file): a Hyperliquid market's facts and
//! the trade fees `RiskCalc` counts.

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq)]
pub struct Market {
    pub name: String,
    pub mark_px: Decimal,
    pub sz_decimals: i64,
    pub max_leverage: i64,
    /// Position in Hyperliquid's perp universe, the asset id in orders; −1 when not loaded from the exchange.
    pub index: i64,
    /// Price 24 hours ago, for the entry-point check.
    pub prev_day_px: Option<Decimal>,
    /// Hourly funding rate; positive means longs pay shorts.
    pub funding: Option<Decimal>,
    /// Average true range of 4-hour candles, % of price; loaded separately from candles.
    pub atr_pct: Option<Decimal>,
    /// The HIP-3 exchange ("xyz") for markets outside the main one; its margin is a separate balance.
    pub dex: Option<String>,
    /// Traded in the last 24 hours, USD; orders the market picker.
    pub volume24h: Decimal,
}

impl Market {
    /// The rest as Swift's init defaults them: index −1, no 24 h price, funding, ATR or HIP-3 exchange.
    pub fn new(name: &str, mark_px: Decimal, sz_decimals: i64, max_leverage: i64) -> Self {
        Self {
            name: name.into(), mark_px, sz_decimals, max_leverage, index: -1, prev_day_px: None, funding: None, atr_pct: None, dex: None,
            volume24h: Decimal::ZERO,
        }
    }
}

/// Position math. Code decides the numbers; the model never does.
pub struct RiskCalc;

impl RiskCalc {
    /// Hyperliquid tier-0 taker fee (RESEARCH.md §8).
    pub fn taker_fee() -> Decimal { Decimal::new(45, 5) }

    /// ihatecrypto's fee on each side (HLBuilder), on top of Hyperliquid's.
    pub fn builder_fee() -> Decimal { hyperliquid::builder::share() }

    /// What each side of a trade pays in all.
    pub fn trade_fee() -> Decimal { Self::taker_fee() + Self::builder_fee() }
}

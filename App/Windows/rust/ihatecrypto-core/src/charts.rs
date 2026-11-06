//! Part of `Charts.swift`, what round 1 needed: a chart's range (`ChartCommand.Range`).
// pending: ChartCommand itself, its parser and the candles.

use serde::{Deserialize, Serialize};

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

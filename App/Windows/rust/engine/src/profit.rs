//! The wallet's value over time (Profit.swift): equity.json, a point per read, for the portfolio's 7-day line.
//! Layer 2 adds here: recording a point (load_equity, keep_equity) and the line and its change.

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::json;

/// `Engine.EquityPoint`: what a wallet was worth at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EquityPoint {
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(with = "number")]
    pub usd: Decimal,
    pub wallet: String,
}

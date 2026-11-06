//! A place's details board beyond the portfolio's rows (PlaceFacts.swift, the Mac's PlaceDetailBoard): Hyperliquid's
//! account in full, and the rate each deposit earns now.
//! Layer 2 adds here: reading them with the balances.

use std::collections::HashMap;

use ihatecrypto_core::foundation::{Date, number};
use rust_decimal::Decimal;
use serde::Serialize;

use crate::json;

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PlaceFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyperliquid: Option<HlFacts>,
    /// USDC's rate a year now, in %, by place: kamino, jupiter, aave (Base), morpho (Base), hlp.
    #[serde(with = "json::numbers")]
    pub rates: HashMap<String, Decimal>,
}

/// `PlaceFacts.HL`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HlFacts {
    #[serde(with = "number")]
    pub account_value: Decimal,
    #[serde(with = "number")]
    pub withdrawable: Decimal,
    #[serde(rename = "spotUSDC", with = "number")]
    pub spot_usdc: Decimal,
    /// HIP-3 exchanges' own margin (stocks, commodities): `xyz`.
    pub dexes: Vec<DexFacts>,
    #[serde(with = "number")]
    pub hlp_equity: Decimal,
    /// HLP keeps a deposit 4 days.
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub hlp_locked_until: Option<Date>,
}

/// `PlaceFacts.Dex`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DexFacts {
    pub dex: String,
    #[serde(with = "number")]
    pub account_value: Decimal,
    #[serde(with = "number")]
    pub withdrawable: Decimal,
}

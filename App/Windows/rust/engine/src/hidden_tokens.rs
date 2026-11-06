//! Tokens the person hid and money between networks (HiddenTokens.swift).
//! Layer 2 adds here: show_token (Settings' tokenChoices), the in-transit reads (bridge orders with the balances) and
//! transit_state.

use ihatecrypto_core::foundation::Date;
use serde::Serialize;

use crate::json;

/// Money between networks, as the Mac's «In transit» card: each bridge order signed and not landed yet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransitView {
    /// The bridge's order: Relay's request, 1Click's deposit address, «domain:burn» for CCTP, a LayerZero transaction.
    pub id: String,
    pub chain: String,
    /// «Base USDC→Solana 5», without its transaction.
    pub what: String,
    pub bridge: String,
    pub logo: String,
    /// A deposit address opened for money coming in from elsewhere, not a move of the app's own.
    pub receiving: bool,
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx: Option<String>,
    pub links: Vec<TransitLink>,
}

/// `TransitView.Link`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TransitLink {
    pub title: String,
    pub url: String,
}

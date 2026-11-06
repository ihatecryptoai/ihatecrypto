//! Hyperliquid's trading key, as on the Mac (TradingKey.swift; Execution.swift enableAgent, resumeTradingKey,
//! offerTradingKey; Ledger.swift enableLedgerAgent; Settings › Wallet › Hyperliquid). The signer makes the key and signs
//! its approval with the main key (or the Ledger does); only once Hyperliquid accepts it does the signer sign orders,
//! stops and closes with it, without Windows Hello. It can't withdraw: anything that moves money out still takes the
//! main key.
//! Layer 2 adds here: offer_trading_key, turn_on_trading_key, revoke_fee, read_fee_approval, read_ledger_agent,
//! trading_key_view.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradingKeyView {
    /// The key's address once made; None before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    pub active: bool,
    /// Approved before and paused by «lock»: on again after one Windows Hello.
    pub paused: bool,
    /// The Ledger's own key (its account), approved on the device.
    pub ledger: bool,
    /// This Hyperliquid account approved ihatecrypto's fee; None until Hyperliquid answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_approved: Option<bool>,
    pub fee_rate: String,
    pub referral: String,
    /// The wallet can have one: an EVM address and a key here.
    pub possible: bool,
}

/// The chat's answers to the trading key's offer.
pub const TRADING_KEY_ON: &str = "approve the hyperliquid trading key";
pub const TRADING_KEY_LATER: &str = "leave the hyperliquid trading key off";

//! A trade plan as the Mac draws it (TradePlan.swift; Studio.swift planRoute, Canvases.swift planBlocker/planFix,
//! TradeAdvice.swift): every message about it redraws its route — FROM, BRIDGE, CHECK RULES, SECOND CHECK, OPEN,
//! PROTECT, SIMULATION, SIGN, RESULT — and the chat checks it item by item. A plan the rules stop stays on the canvas
//! with the one change that passes; one that passes becomes the card waiting for Confirm in its place. Each coin's ATR
//! and the last bridge quote are `State::atr_kept` and `quote_kept`.
//! Layer 2 adds here: plan (drawing and checking it), plan_op (planMarkets, planMarket, planTake, planCancel), the
//! position views and their ruler, take-profit ways and the rest of the file's `extension Engine`.

use std::collections::BTreeMap;

use evm::protocols::SponsoredTransfer;
use evm::protocols::batch7702::BatchCall;
use evm::protocols::relay::MulticallPermit;
use evm::protocols::{Permit3009, safe};
use execution::trade_signer::{EvmFees, Signing, TradeError, TradeSigner};
use hyperliquid::MsgPack;
use ihatecrypto_core::plan::{BridgeQuote, MarketGroup};
use serde::{Deserialize, Serialize};
use trade::EvmNetwork;
use wallet::evm::EvmSignature;

use crate::canvas::Action;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    /// The plan's market and its group, for the picker in its header.
    pub market: String,
    pub group: String,
    /// Why it can't go, as a sentence and in a few words for its status; None when it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    /// A rule stops it (red); false while it only waits for a detail such as the stop (orange).
    #[serde(default)]
    pub blocked: bool,
    /// The one change that passes every rule, the $10 minimum and the money (Plan.fix), or «By the book».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_detail: Option<String>,
    /// The per-trade risk limit is what stops it: Settings › Trading agent can raise it, at the user's own risk.
    #[serde(default)]
    pub over_limit: bool,
    /// «Take profit like the pros»: the three ways, while asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takes: Option<Vec<TakeChoice>>,
    /// What Confirm does, under the route.
    pub footer: String,
    /// Watched addresses: the simulation is shown and nothing can sign here (the Mac's Preview).
    #[serde(default)]
    pub preview: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeChoice {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub note: String,
    /// What it pays at the take: «+$2.00», or «no cap».
    pub payoff: String,
    pub recommended: bool,
}

/// The plan's dry bridge quote and the EVM network the USDC leaves from when Solana is short (None: Solana).
#[derive(Debug, Clone)]
pub struct PlanQuote {
    pub quote: BridgeQuote,
    pub from: Option<EvmNetwork>,
}

/// The position board's price ruler (the Mac's PriceRuler): take, entry, now and stop on one line, and the guardian's
/// word under it (PositionBoard). Prices as numbers for the drawing, each label as the card says it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionRuler {
    pub points: Vec<RulerPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard_line: Option<String>,
}

/// `PositionRuler.Point`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RulerPoint {
    pub kind: String,
    pub at: f64,
    pub text: String,
}

/// A plan's board in these states is the plan's own and gives way to its next version; a move that ran stays.
pub const PLAN_STATES: [&str; 3] = ["plan", "waiting", "cancelled"];

/// The picker's groups in Russian.
pub fn group_ru(g: MarketGroup) -> &'static str {
    match g { MarketGroup::Crypto => "крипта", MarketGroup::Commodities => "сырьё", MarketGroup::Stocks => "акции", MarketGroup::Indices => "индексы и валюты" }
}

/// Signs nothing: a plan for watched addresses is prepared and simulated, as the Mac's Preview, and stops there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSigner {
    pub refusal: String,
}

impl PreviewSigner {
    fn refuse<T: Send + 'static>(&self) -> Signing<'_, T> {
        let refusal = self.refusal.clone();
        Box::pin(async move { Err(TradeError::Failed(refusal)) })
    }
}

impl TradeSigner for PreviewSigner {
    fn sign_solana(&self, _: Vec<u8>, _: Vec<String>, _: Vec<String>, _: BTreeMap<String, String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_hyperliquid(&self, _: MsgPack, _: u64) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_hyperliquid_user(&self, _: MsgPack, _: Vec<String>) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_relay_mapping(&self, _: String, _: String, _: Vec<u8>, _: u64) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_evm(&self, _: EvmFees, _: String, _: Vec<u8>, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_near_intent_deposit(&self, _: u128, _: String, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { self.refuse() }
    // The rest refuse too, as their defaults do; spelled out so a preview never signs by a default changing.
    fn sign_evm_batch(&self, _: EvmFees, _: Vec<BatchCall>, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_evm_payable(&self, _: EvmFees, _: String, _: u64, _: Vec<u8>, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_evm_transfer(&self, _: EvmFees, _: String, _: u64, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_near_transfer(&self, _: u128, _: String, _: Vec<String>, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_near_delete_account(&self, _: String, _: Vec<String>, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_safe_move(&self, _: safe::Move, _: Vec<String>) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_solana_sponsored(&self, _: Vec<u8>, _: Vec<String>, _: BTreeMap<String, String>, _: String) -> Signing<'_, Vec<u8>> { self.refuse() }
    fn sign_receive_authorization(&self, _: Permit3009) -> Signing<'_, EvmSignature> { self.refuse() }
    fn sign_relay_multicall_permit(&self, _: MulticallPermit) -> Signing<'_, Vec<EvmSignature>> { self.refuse() }
    fn sign_sponsored_transfer(&self, _: SponsoredTransfer, _: Vec<String>) -> Signing<'_, Vec<u8>> { self.refuse() }
}

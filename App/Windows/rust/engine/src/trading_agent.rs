//! Settings › Trading agent, as on the Mac (TradingAgent.swift, SettingsView.agent): a style that sets every rule at
//! once, the limits a plan may not break, what «by the book» uses when no numbers are named, and Safe search. Code checks
//! every plan against these (Plan.resolve, RiskRules in the executor), as the Mac's Workspace.policy does.
//! Layer 2 adds here: trading_view, set_policy, set_contract_cap, the policy in use.

use ihatecrypto_core::foundation;
use ihatecrypto_core::risk::RiskPolicy;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::trading_key::TradingKeyView;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradingView {
    pub policy: RiskPolicy,
    pub styles: Vec<Style>,
    pub safe_search: bool,
    /// Settings › Wallet: «One transaction for approve and swap» (EIP-7702) and the cap for unknown protocols.
    pub batches: bool,
    pub contract_cap: f64,
    /// Settings › Wallet › Hyperliquid (trading_key.rs).
    pub key: TradingKeyView,
}

/// `TradingView.Style`: a ready set of the rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Style {
    pub id: String,
    pub policy: RiskPolicy,
}

/// Three ready sets of the rules (AgentStyle); Balanced is the default.
pub fn trading_styles() -> Vec<Style> {
    let d = |s: &str| foundation::decimal(s).expect("a decimal literal");
    let n = Decimal::from;
    let careful = RiskPolicy {
        max_risk_pct: n(1), default_risk_pct: d("0.5"), max_open_risk_pct: n(3), min_reward_risk: n(2), default_reward_risk: d("2.5"), max_leverage: n(2),
        atr_stop_multiple: n(2),
    };
    let bold = RiskPolicy {
        max_risk_pct: n(3), default_risk_pct: n(2), max_open_risk_pct: n(10), min_reward_risk: d("1.2"), default_reward_risk: d("1.5"), max_leverage: n(10),
        atr_stop_multiple: n(1),
    };
    vec![
        Style { id: "careful".into(), policy: careful },
        Style { id: "balanced".into(), policy: RiskPolicy::default() },
        Style { id: "bold".into(), policy: bold },
    ]
}

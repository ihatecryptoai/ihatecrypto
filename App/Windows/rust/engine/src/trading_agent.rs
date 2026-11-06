//! Settings › Trading agent, as on the Mac (TradingAgent.swift, SettingsView.agent): a style that sets every rule at
//! once, the limits a plan may not break, what «by the book» uses when no numbers are named, and Safe search. Code checks
//! every plan against these (Plan.resolve, RiskRules in the executor), as the Mac's Workspace.policy does.
//! Layer 2: trading_view, set_policy, set_contract_cap, the policy in use and the capital the rules measure.

use ihatecrypto_core::foundation::{self, Num, Rounding};
use ihatecrypto_core::pnl::CostBasis;
use ihatecrypto_core::risk::RiskPolicy;
use rust_decimal::Decimal;
use serde::Serialize;
use wallet::WalletError;

use crate::engine::Engine;
use crate::portfolio::Row;
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

impl Engine {
    /// The rules the trades are planned and checked by.
    pub fn policy(&self) -> RiskPolicy { self.st().settings.policy.clone().unwrap_or_default() }

    /// Jupiter-verified tokens only, unless turned off.
    pub fn safe_search(&self) -> bool { self.st().settings.safe_search.unwrap_or(true) }

    pub fn trading_view(&self) -> TradingView {
        let (batches, contract_cap) = {
            let st = self.st();
            (st.settings.batches == Some(true), st.settings.contract_cap.unwrap_or(50.0))
        };
        TradingView { policy: self.policy(), styles: trading_styles(), safe_search: self.safe_search(), batches, contract_cap, key: self.trading_key_view() }
    }

    /// New rules from Settings, kept within the Mac's steppers' bounds and in line with each other.
    pub fn set_policy(&self, p: Option<RiskPolicy>) -> Result<(), WalletError> {
        let Some(mut p) = p else { return Err(WalletError("No rules given.".into())) };
        let clamp = |v: Decimal, lo: Decimal, hi: Decimal| v.max(lo).min(hi);
        let n = Decimal::from;
        p.max_risk_pct = clamp(p.max_risk_pct, Decimal::new(5, 1), n(10));
        p.max_open_risk_pct = clamp(p.max_open_risk_pct.max(p.max_risk_pct), p.max_risk_pct, n(30));
        p.min_reward_risk = clamp(p.min_reward_risk, n(1), n(5));
        p.max_leverage = foundation::rounded(clamp(p.max_leverage, n(1), n(20)), 0, Rounding::Plain);
        p.default_risk_pct = clamp(p.default_risk_pct, Decimal::new(25, 2), p.max_risk_pct);
        p.default_reward_risk = clamp(p.default_reward_risk.max(p.min_reward_risk), p.min_reward_risk, n(10));
        p.atr_stop_multiple = clamp(p.atr_stop_multiple, Decimal::new(5, 1), n(5));
        self.set_settings(|s| s.policy = (p != RiskPolicy::default()).then_some(p));
        Ok(())
    }

    /// The cap for a contract outside ihatecrypto's own list, in dollars: one of the Mac's choices.
    pub fn set_contract_cap(&self, usd: Option<f64>) -> Result<(), WalletError> {
        let Some(usd) = usd.filter(|u| [10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0].contains(u)) else {
            return Err(WalletError("Pick one of the caps.".into()));
        };
        self.set_settings(|s| s.contract_cap = (usd != 50.0).then_some(usd));
        Ok(())
    }

    /// Capital the risk rules measure against, as the Mac's `equity`: everything on show but the networks' own coins (SOL,
    /// ETH and the other gas coins) — the user's money, in the balance, but no trade may spend it. NEAR counts, as on the
    /// Mac. The plan, a stop change and a Solana buy all measure against this. A sum past the arithmetic (balances from
    /// the nodes) measures nothing.
    pub fn rule_equity(&self) -> Option<Decimal> {
        let st = self.st();
        if st.portfolio.total <= Decimal::ZERO { return None }
        let gas = st.portfolio.networks.iter().filter(|n| n.key != "near").flat_map(|n| &n.rows).filter(|r| Self::is_gas_coin(r))
            .fold(Num::of(Decimal::ZERO), |sum, r| sum + r.usd.unwrap_or_default());
        Some((st.portfolio.total - gas).0?.max(Decimal::ZERO))
    }

    /// A network's own coin: an EVM network's «native» row, SOL on Solana.
    pub fn is_gas_coin(r: &Row) -> bool { r.contract.as_deref() == Some("native") || (r.symbol == "SOL" && r.contract.as_deref() == Some(CostBasis::WSOL)) }
}

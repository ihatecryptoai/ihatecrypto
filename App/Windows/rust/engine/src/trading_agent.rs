//! Settings › Trading agent, as on the Mac (TradingAgent.swift, SettingsView.agent): a style that sets every rule at
//! once, the limits a plan may not break, what «by the book» uses when no numbers are named, and Safe search. Code checks
//! every plan against these (Plan.resolve, RiskRules in the executor), as the Mac's Workspace.policy does.
//! Layer 2: trading_view, set_policy, set_contract_cap, the policy in use and the capital the rules measure.

use ihatecrypto_core::foundation::{self, Num, Rounding};
use ihatecrypto_core::pnl::CostBasis;
use ihatecrypto_core::risk::{OverLimit, RiskPolicy};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use signer_api::{SignerLimits, SignerRequest, TradingRules};
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
        atr_stop_multiple: n(2), over_limit: None,
    };
    let bold = RiskPolicy {
        max_risk_pct: n(3), default_risk_pct: n(2), max_open_risk_pct: n(10), min_reward_risk: d("1.2"), default_reward_risk: d("1.5"), max_leverage: n(10),
        atr_stop_multiple: n(1), over_limit: None,
    };
    vec![
        Style { id: "careful".into(), policy: careful },
        Style { id: "balanced".into(), policy: RiskPolicy::default() },
        Style { id: "bold".into(), policy: bold },
    ]
}

impl Engine {
    /// The rules the trades are planned and checked by: the limits as the signer keeps them (any change of them asks for
    /// Windows Hello there), the rest from Settings.
    pub fn policy(&self) -> RiskPolicy {
        let st = self.st();
        let mut p = st.settings.policy.clone().unwrap_or_default();
        match st.status.as_ref().and_then(|s| s.limits) {
            Some(SignerLimits { trading: Some(t), .. }) => Self::take_rules(&mut p, &t),
            // The rules not in the signer yet (saved before it kept them): the trading key's cap is what holds, so the
            // screen and the plans say it until the person saves theirs.
            Some(l) => p.max_leverage = p.max_leverage.min(Decimal::from(l.leverage_cap())),
            None => {}
        }
        p
    }

    /// The signer's trading rules put into a policy.
    pub fn take_rules(p: &mut RiskPolicy, t: &TradingRules) {
        let d = |v: f64, or: Decimal| Decimal::from_f64(v).map_or(or, |x| x.round_dp(2));
        p.max_risk_pct = d(t.risk_pct, p.max_risk_pct);
        p.max_open_risk_pct = d(t.open_risk_pct, p.max_open_risk_pct);
        p.min_reward_risk = d(t.min_reward_risk, p.min_reward_risk);
        p.max_leverage = Decimal::from(t.leverage);
        p.over_limit = Some(match t.over_limit { signer_api::OverLimit::Block => OverLimit::Block, signer_api::OverLimit::Ask => OverLimit::Ask,
                                                  signer_api::OverLimit::Warn => OverLimit::Warn });
        p.default_risk_pct = p.default_risk_pct.min(p.max_risk_pct);
        p.default_reward_risk = p.default_reward_risk.max(p.min_reward_risk);
    }

    /// The limits of a policy, as the signer keeps them.
    pub fn trading_rules(p: &RiskPolicy) -> TradingRules {
        let f = |v: Decimal| v.to_f64().unwrap_or(0.0);
        TradingRules { risk_pct: f(p.max_risk_pct), open_risk_pct: f(p.max_open_risk_pct), min_reward_risk: f(p.min_reward_risk),
                       leverage: p.max_leverage.to_i64().unwrap_or(5),
                       over_limit: match p.limit_mode() { OverLimit::Block => signer_api::OverLimit::Block, OverLimit::Ask => signer_api::OverLimit::Ask,
                                                          OverLimit::Warn => signer_api::OverLimit::Warn } }
    }

    /// Jupiter-verified tokens only, unless turned off.
    pub fn safe_search(&self) -> bool { self.st().settings.safe_search.unwrap_or(true) }

    pub fn trading_view(&self) -> TradingView {
        let (batches, contract_cap) = {
            let st = self.st();
            (st.settings.batches == Some(true), st.settings.contract_cap.unwrap_or(50.0))
        };
        TradingView { policy: self.policy(), styles: trading_styles(), safe_search: self.safe_search(), batches, contract_cap, key: self.trading_key_view() }
    }

    /// New rules from Settings, kept within the steps' bounds and in line with each other (`bounded`). The limits go to the
    /// signer first: a change of them asks for Windows Hello there, and nothing is kept when the person says no.
    pub async fn set_policy(&self, p: Option<RiskPolicy>) -> Result<(), WalletError> {
        let Some(p) = p else { return Err(WalletError("No rules given.".into())) };
        let p = Self::bounded(p, self.policy().over_limit);
        let rules = Self::trading_rules(&p);
        let kept = self.st().status.as_ref().and_then(|s| s.limits);
        if kept.and_then(|l| l.trading) != Some(rules) {
            let Some(limits) = kept else { return Err(WalletError("The signer isn't ready yet: the limits weren't saved.".into())) };
            self.signer_call(SignerRequest::SetLimits { limits: SignerLimits { trading: Some(rules), ..limits } }).await?;
        }
        self.set_settings(|s| s.policy = (p != RiskPolicy::default()).then_some(p));
        Ok(())
    }

    /// Rules within the steps' bounds and in line with each other. A style's rules name no «Over a limit»: `over_limit`,
    /// the person's own, stays.
    pub fn bounded(mut p: RiskPolicy, over_limit: Option<OverLimit>) -> RiskPolicy {
        let clamp = |v: Decimal, lo: Decimal, hi: Decimal| v.max(lo).min(hi);
        let n = Decimal::from;
        if p.over_limit.is_none() { p.over_limit = over_limit }
        p.max_risk_pct = clamp(p.max_risk_pct, Decimal::new(5, 1), RiskPolicy::NO_LIMIT);
        p.max_open_risk_pct = clamp(p.max_open_risk_pct.max(p.max_risk_pct), p.max_risk_pct, RiskPolicy::NO_LIMIT);
        p.min_reward_risk = clamp(p.min_reward_risk, n(1), n(5));
        p.max_leverage = foundation::rounded(clamp(p.max_leverage, n(1), n(50)), 0, Rounding::Plain);
        p.default_risk_pct = clamp(p.default_risk_pct, Decimal::new(25, 2), p.max_risk_pct.min(n(10)));
        p.default_reward_risk = clamp(p.default_reward_risk.max(p.min_reward_risk), p.min_reward_risk, n(10));
        p.atr_stop_multiple = clamp(p.atr_stop_multiple, Decimal::new(5, 1), n(5));
        // ponytail: «Block» is what nothing set means, so it is kept as None and a default policy stays unsaved.
        if p.over_limit == Some(OverLimit::Block) { p.over_limit = None }
        p
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

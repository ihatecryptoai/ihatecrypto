//! Settings › Trading agent as the Mac's (TradingAgent.swift; ModulesTests' trading rules): the rules stay within the
//! Mac's steppers and in line with each other, the contract cap is one of its choices, and a network's own coin is told
//! apart from the money a trade may use.

use std::rc::Rc;

use ihatecrypto_core::foundation;
use ihatecrypto_core::pnl::CostBasis;
use ihatecrypto_core::risk::RiskPolicy;
use rust_decimal::Decimal;

use super::{Lines, engine};
use crate::engine::Engine;
use crate::portfolio::Row;
use crate::store::{self, Settings};

async fn on_engine(test: impl AsyncFnOnce(Rc<Engine>)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async { test(engine(&Lines::default())).await }).await;
}

#[tokio::test]
async fn the_trading_rules_stay_within_the_macs_bounds() {
    on_engine(async |e| {
        let view = e.trading_view();
        assert!(view.policy.max_risk_pct == Decimal::TWO && view.policy.max_leverage == Decimal::from(5));
        assert_eq!(view.styles.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["careful", "balanced", "bold"]);
        assert!(view.safe_search && !view.batches && view.contract_cap == 50.0);

        let n = Decimal::from;
        let asked = RiskPolicy { max_risk_pct: n(50), default_risk_pct: n(40), max_leverage: n(100), min_reward_risk: n(3), default_reward_risk: n(2), ..RiskPolicy::default() };
        e.set_policy(Some(asked)).unwrap();
        let kept = e.trading_view().policy;
        assert_eq!((kept.max_risk_pct, kept.default_risk_pct, kept.max_leverage, kept.default_reward_risk, kept.max_open_risk_pct), (n(10), n(10), n(20), n(3), n(10)));
        assert_eq!(store::load::<Settings>("settings.json").unwrap().policy, Some(kept));
        // The other ends, and a leverage rounded as the stepper's.
        let low = RiskPolicy {
            max_risk_pct: Decimal::ZERO, default_risk_pct: Decimal::ZERO, min_reward_risk: Decimal::ZERO, default_reward_risk: Decimal::ZERO,
            max_leverage: foundation::decimal("2.5").unwrap(), atr_stop_multiple: n(9), max_open_risk_pct: Decimal::ZERO,
        };
        e.set_policy(Some(low)).unwrap();
        let p = e.policy();
        let (half, quarter) = (foundation::decimal("0.5").unwrap(), foundation::decimal("0.25").unwrap());
        assert_eq!((p.max_risk_pct, p.default_risk_pct, p.min_reward_risk, p.default_reward_risk, p.max_leverage, p.atr_stop_multiple, p.max_open_risk_pct),
                   (half, quarter, n(1), n(1), n(3), n(5), half));
        // The defaults again: nothing kept.
        e.set_policy(Some(RiskPolicy::default())).unwrap();
        assert_eq!(e.st().settings.policy, None);
        assert_eq!(e.set_policy(None).unwrap_err().0, "No rules given.");
    }).await;
}

#[tokio::test]
async fn the_contract_cap_is_one_of_the_macs_choices() {
    on_engine(async |e| {
        assert_eq!(e.set_contract_cap(Some(77.0)).unwrap_err().0, "Pick one of the caps.");
        assert_eq!(e.set_contract_cap(None).unwrap_err().0, "Pick one of the caps.");
        e.set_contract_cap(Some(250.0)).unwrap();
        assert_eq!(e.trading_view().contract_cap, 250.0);
        e.set_contract_cap(Some(50.0)).unwrap();
        assert!(e.st().settings.contract_cap.is_none() && e.trading_view().contract_cap == 50.0);
        e.set_settings(|s| { s.safe_search = Some(false); s.batches = Some(true) });
        let view = e.trading_view();
        assert!(!view.safe_search && view.batches && !e.safe_search());
    }).await;
}

#[test]
fn a_networks_own_coin_is_its_gas() {
    let coin = |symbol: &str, contract: Option<&str>| Row { contract: contract.map(String::from), ..Row::new(symbol, "1", Decimal::ONE) };
    assert!(Engine::is_gas_coin(&coin("ETH", Some("native"))) && Engine::is_gas_coin(&coin("SOL", Some(CostBasis::WSOL))));
    assert!(!Engine::is_gas_coin(&coin("SOL", Some("So1aNaOtherMint"))) && !Engine::is_gas_coin(&coin("USDC", None)));
}

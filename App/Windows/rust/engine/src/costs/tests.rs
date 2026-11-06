//! The cost a coin's P/L is measured from, and «seen»: no Swift test covers these, so their rules are checked here.

use std::sync::Arc;

use ihatecrypto_core::pnl::CostBasis;
use rust_decimal::Decimal;

use crate::engine::{ChainCost, Engine};
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

fn basis(amount: i64, usd: i64) -> CostBasis { CostBasis { amount: amount.into(), usd: usd.into(), ..Default::default() } }

#[tokio::test]
async fn the_cost_covering_more_counts_the_chains_first_then_seen() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("L1's tests draw no state"));
        assert!(e.cost("m").is_none());
        // Nothing else tells: from when the app first saw it, at the price then.
        e.seen("m", 4.into(), Decimal::new(25, 1));
        assert_eq!(e.cost("m"), e.st().seen_costs.get("m").cloned());
        assert_eq!(e.cost("m").map(|c| (c.amount, c.usd)), Some((4.into(), 10.into())));
        assert!(store::url("seen.json").exists());
        // Less held since: that share of the cost goes.
        e.seen("m", 2.into(), 100.into());
        assert_eq!(e.cost("m").map(|c| (c.amount, c.usd)), Some((2.into(), 5.into())));
        // The chain's or the app's, whichever covers more; equal: the chain's, as Swift's max(by:) keeps the first.
        e.st_mut().app_costs.insert("m".into(), basis(3, 30));
        assert_eq!(e.cost("m"), Some(basis(3, 30)));
        e.st_mut().chain_cost.insert("m".into(), ChainCost { held: 3.into(), cost: basis(3, 21) });
        assert_eq!(e.cost("m"), Some(basis(3, 21)));
        e.st_mut().app_costs.insert("m".into(), basis(5, 40));
        assert_eq!(e.cost("m"), Some(basis(5, 40)));
    }).await;
}

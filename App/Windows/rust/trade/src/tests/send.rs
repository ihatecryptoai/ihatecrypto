//! Send.swift's offline cases: the fee oracle's selector (EVMTests) and a send to an empty Solana account under the
//! network's rent minimum (CosignedTests).

use serde_json::Value;
use wallet::evm::keccak;

use super::d;
use super::g2_fake::{TempJournal, executor, node, result};
use crate::Executor;

/// «Send all ETH» on Base and Optimism asks the fee oracle's getL1FeeUpperBound(uint256); its selector, checked against
/// the OP-stack predeploy on Base mainnet 2026-10-26.
#[test]
fn l1_fee_upper_bound_selector() {
    assert_eq!(Executor::L1_FEE_UPPER_BOUND[..], keccak(b"getL1FeeUpperBound(uint256)")[..4]);
    assert_eq!(wallet::to_hex(&Executor::L1_FEE_UPPER_BOUND), "f1c7a58b");
}

/// A send to an empty account: refused under the network's rent minimum, with that minimum named; 0.0007 SOL, which the
/// old constant refused, goes on past it (to the blockhash this node doesn't give).
#[tokio::test]
async fn an_empty_account_opens_with_the_networks_rent_minimum() {
    let wallet = "5PkKzEWa11etTestTestTestTestTestTestTestZhb1";
    let url = node(move |method, params| match method {
        "getBalance" => result(&format!(r#"{{"context":{{"slot":130}},"value":{}}}"#, if params.first().and_then(Value::as_str) == Some(wallet) { 1_000_000_000 } else { 0 })),
        "getMinimumBalanceForRentExemption" => result(&((128 + params.first().and_then(Value::as_u64).unwrap_or(0)) * 5_080).to_string()),
        _ => result("null"),
    }).await;
    let journal = TempJournal::new("rent");
    let e = executor(wallet, Some(&url), &journal);
    let said = |sol: &'static str| { let e = e.clone(); async move { e.send_sol(Some(d(sol)), "EmptyAccount", "Savings").await.map_or_else(|e| e.to_string(), |_| "sent".into()) } };
    let short = said("0.0006").await;
    assert!(short.contains("at least 0.00065024 SOL"), "{short}");
    let enough = said("0.0007").await;
    assert!(!enough.contains("rent minimum"), "{enough}");
    assert_eq!(Executor::usdc(1_234_567), "1.234567");
    assert_eq!(Executor::usdc(0), "0");
}

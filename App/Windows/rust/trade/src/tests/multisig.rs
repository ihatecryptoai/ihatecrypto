//! SafeTests' isContract read from a node's eth_getCode (the rest of SafeTests runs on an anvil fork): a contract, no
//! code, and a wallet delegated by EIP-7702, which still signs with its own key.

use net::evm_rpc::EvmRpc;
use serde_json::json;

use super::g4_fake::serve;
use crate::{EvmNetwork, Executor};

#[tokio::test]
async fn a_delegated_wallet_is_a_persons_not_a_contract() {
    let (url, _) = serve(|_, body| {
        let code = match body["params"][0].as_str() {
            Some("0x00000000000000000000000000000000000000aa") => "0x6080604052",
            Some("0x00000000000000000000000000000000000000bb") => "0xef010063c0c19a282a1b52b07dd5a65b58948a07dae32b",
            _ => "0x",
        };
        Some(json!({"jsonrpc": "2.0", "id": 1, "result": code}).to_string())
    }).await;
    let net = EvmNetwork { rpc: EvmRpc::pinned(&url, 8453), ..EvmNetwork::base().clone() };
    let (contract, delegated, person) =
        ("0x00000000000000000000000000000000000000aa", "0x00000000000000000000000000000000000000bb", "0x00000000000000000000000000000000000000cc");
    assert!(Executor::is_contract(contract, &net).await.unwrap() && Executor::deployed(contract, &net).await.unwrap());
    assert!(!Executor::is_contract(delegated, &net).await.unwrap() && Executor::deployed(delegated, &net).await.unwrap());
    assert!(!Executor::is_contract(person, &net).await.unwrap() && !Executor::deployed(person, &net).await.unwrap());
}

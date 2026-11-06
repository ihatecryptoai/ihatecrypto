//! JSON-RPC against a local stand-in node that answers from a list: retries, the walk to the next node, the words.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::evm_rpc::{EvmRpc, ens};
use crate::solana_rpc::SolanaRpc;
use crate::*;

const QUICK: [Duration; 4] = [Duration::from_millis(1); 4];

/// A node at 127.0.0.1 that answers each request with the next (status, body), and keeps the requests' bodies.
async fn node(answers: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    tokio::spawn(async move {
        for (status, body) in answers {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).await.unwrap();
                buf.extend(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    if buf.len() >= end + 4 + length {
                        kept.lock().unwrap().push(serde_json::from_slice(&buf[end + 4..end + 4 + length]).unwrap_or(Value::Null));
                        break;
                    }
                }
                if n == 0 { break }
            }
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(reply.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        }
    });
    (url, seen)
}

fn ok(result: Value) -> (u16, String) { (200, json!({"jsonrpc": "2.0", "id": 1, "result": result}).to_string()) }
fn error(message: &str) -> (u16, String) { (200, json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32000, "message": message}}).to_string()) }

#[tokio::test]
async fn a_throttled_node_is_asked_again_and_the_answer_is_read() {
    let (url, seen) = node(vec![(429, "slow down".into()), error("header not found"), ok(json!("0x2a"))]).await;
    let rpc = EvmRpc::pinned(&url, 8453);
    assert_eq!(rpc.call_with("eth_chainId", json!([]), &QUICK).await.unwrap(), json!("0x2a"));
    assert_eq!(seen.lock().unwrap().len(), 3);
    assert_eq!(seen.lock().unwrap()[0]["method"], "eth_chainId");
}

#[tokio::test]
async fn a_revert_is_final_and_short_gas_is_said_in_words() {
    let (url, seen) = node(vec![error("execution reverted: nope"), error("insufficient funds for gas * price + value: have 1000000000000000 want 2000000000000000")]).await;
    let rpc = EvmRpc::pinned(&url, 8453);
    assert_eq!(rpc.call_with("eth_call", json!([]), &QUICK).await.unwrap_err().0, "EVM RPC eth_call: execution reverted: nope");
    assert_eq!(rpc.call_with("eth_estimateGas", json!([]), &QUICK).await.unwrap_err().0,
               "Not enough ETH for gas: the wallet has 0.001 ETH and needs 0.002 ETH with the fee. Send it a little ETH, then try again.");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_key_turned_away_walks_to_the_next_node() {
    let (first, _) = node(vec![(403, "{\"error\":\"forbidden\"}".into())]).await;
    let (second, seen) = node(vec![ok(json!("0x5"))]).await;
    let rpc = EvmRpc { url: first, chain_id: 1, fallbacks: vec![second], send_url: None, follows_settings: false };
    assert_eq!(rpc.call_with("eth_getTransactionCount", json!([]), &QUICK).await.unwrap(), json!("0x5"));
    assert_eq!(seen.lock().unwrap().len(), 1);
    // Alone, the same refusal is final: no other node to ask.
    let (only, _) = node(vec![(403, "no".into())]).await;
    assert!(EvmRpc::pinned(&only, 1).call_with("eth_chainId", json!([]), &QUICK).await.unwrap_err().0.contains("HTTP 403"));
}

#[tokio::test]
async fn reads_quantities_balances_and_a_resent_transaction() {
    let (url, _) = node(vec![ok(json!("0x1158e460913d00000")), ok(json!("0x10")), error("already known"),
                             ok(json!({"status": "0x1"})), ok(json!("0x0000000000000000000000000000000000000000000000000000000000000064"))]).await;
    let rpc = EvmRpc::pinned(&url, 8453);
    assert_eq!(rpc.balance("0x1").await.unwrap(), 20_000_000_000_000_000_000);
    assert_eq!(rpc.nonce("0x1").await.unwrap(), 16);
    assert_eq!(rpc.send(&[1, 2, 3]).await.unwrap(), format!("0x{}", wallet::to_hex(&wallet::evm::keccak(&[1, 2, 3]))));
    assert!(rpc.wait_for_receipt("0xabc", Duration::from_secs(5)).await.unwrap());
    assert_eq!(rpc.token_balance(evm::BASE_USDC, "0x0000000000000000000000000000000000000001").await.unwrap(), evm::U256::from(100));
}

#[tokio::test]
async fn solana_calls_and_a_send_that_already_landed() {
    let tx = [vec![1u8], vec![7; 64], vec![0; 10]].concat();
    let (url, seen) = node(vec![(503, "<html>down</html>".into()), ok(json!({"value": 5000})),
                                error("Transaction simulation failed: This transaction has already been processed"),
                                ok(json!({"value": {"blockhash": "abc", "lastValidBlockHeight": 9}}))]).await;
    let rpc = SolanaRpc::pinned(&url);
    assert_eq!(rpc.balance("x").await.unwrap(), 5000);
    assert_eq!(rpc.send(&tx).await.unwrap(), wallet::keys::base58::encode(&[7; 64]));
    assert_eq!(rpc.latest_blockhash().await.unwrap(), ("abc".into(), 9));
    assert_eq!(seen.lock().unwrap()[1]["params"][1]["commitment"], "finalized");
}

#[tokio::test]
async fn a_simulation_short_of_sol_says_what_to_do() {
    let payer = wallet::keys::solana_address(&[7; 32]).unwrap();
    let (_, wire) = solana::transfer(&[7; 32], &wallet::keys::base58::encode(&[9; 32]), 1, &wallet::keys::base58::encode(&[1; 32]), 1).unwrap();
    let (url, _) = node(vec![
        ok(json!({"context": {"slot": 77}, "value": {"err": {"InsufficientFundsForRent": {"account_index": 0}}, "logs": [], "accounts": [null]}})),
        ok(json!({"value": 1_500_000})),
    ]).await;
    let sim = SolanaRpc::pinned(&url).simulate(&wire, &[payer.clone()], false).await.unwrap();
    assert_eq!(sim.slot, 77);
    assert_eq!(sim.error.unwrap(), format!("Not enough SOL: the wallet {}…{} has 0.0015 SOL and can't pay Solana's fee and the rent of the account this move opens. Send it ≈ 0.01 SOL, then try again.",
                                           &payer[..4], &payer[payer.len() - 4..]));
    assert_eq!(sim.after[&payer], (0, None));
}

#[test]
fn words_for_a_refusal() {
    assert_eq!(said(b"<html><body>503 Service Temporarily Unavailable</body></html>", 503), "no answer right now (HTTP 503). Try again in a minute.");
    assert_eq!(said(b"  amount too small  ", 400), "amount too small");
    assert_eq!(said(b"", 0), "no answer right now. Try again in a minute.");
    assert!(transient("You reached the usage limit") && !transient("execution reverted: rate limit"));
}

#[test]
fn ens_names() {
    assert!(ens::is_name("vitalik.eth") && ens::is_name("a.b-c.eth"));
    assert!(!ens::is_name("Vitalik.eth") && !ens::is_name("-a.eth") && !ens::is_name(".eth") && !ens::is_name("vitalik"));
    // EIP-137's own vectors.
    assert_eq!(wallet::to_hex(&ens::namehash("eth")), "93cdeb708b7545dc668eb9280176169d1c33cfd8ed6f04690a0bcc88a93fc4ae");
    assert_eq!(wallet::to_hex(&ens::namehash("foo.eth")), "de9b09fd7c5f901e23a3f19fecc54828e9c848539801e86591bd9801b019f84f");
}

/// Read-only, against the public nodes: `cargo test -p net -- --ignored`.
#[tokio::test]
#[ignore]
async fn live_reads() {
    let base = EvmRpc::network(8453).unwrap();
    assert_eq!(base.quantity("eth_chainId", json!([])).await.unwrap(), 8453);
    let (hash, height) = SolanaRpc::new().latest_blockhash().await.unwrap();
    assert!(hash.len() > 30 && height > 0);
    assert_eq!(ens::resolve("vitalik.eth", &EvmRpc::network(1).unwrap()).await.unwrap(), "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
}

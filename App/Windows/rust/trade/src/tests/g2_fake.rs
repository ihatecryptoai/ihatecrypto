//! G2's stand-ins: a Solana node at 127.0.0.1 answering each JSON-RPC call as a function of its method and params
//! (CosignedTests' CosignedNode), a signer that signs nothing, and an executor with a journal of its own.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use execution::Journal;
use execution::trade_signer::{Signing, TradeError, TradeSigner, TradeWallet};
use hyperliquid::MsgPack;
use net::solana_rpc::SolanaRpc;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wallet::evm::EvmSignature;

use crate::Executor;
use crate::executor::Setup;

/// A node answering `answer(method, params)` with (HTTP status, body).
pub async fn node(answer: impl Fn(&str, &[Value]) -> (u16, String) + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let answer = Arc::new(answer);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let answer = answer.clone();
            tokio::spawn(async move {
                let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
                let body: Value = loop {
                    let Ok(n) = socket.read(&mut chunk).await else { return };
                    if n == 0 { return }
                    buf.extend(&chunk[..n]);
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let Some(end) = head.find("\r\n\r\n") else { continue };
                    let length = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + length { break serde_json::from_slice(&buf[end + 4..end + 4 + length]).unwrap_or(Value::Null) }
                };
                let params = body.get("params").and_then(Value::as_array).cloned().unwrap_or_default();
                let (status, reply) = answer(body.get("method").and_then(Value::as_str).unwrap_or(""), &params);
                let response = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    url
}

/// A JSON-RPC result.
pub fn result(json: &str) -> (u16, String) { (200, format!(r#"{{"jsonrpc":"2.0","id":1,"result":{json}}}"#)) }

/// Signs nothing (ZeroSigner's refusals): what the tests here run never gets that far, or must not.
pub struct NoSigner;

fn no<T: Send + 'static>() -> Signing<'static, T> { Box::pin(async { Err(TradeError::Failed("no".into())) }) }

impl TradeSigner for NoSigner {
    fn sign_solana(&self, _: Vec<u8>, _: Vec<String>, _: Vec<String>, _: BTreeMap<String, String>) -> Signing<'_, Vec<u8>> { no() }
    fn sign_hyperliquid(&self, _: MsgPack, _: u64) -> Signing<'_, EvmSignature> { no() }
    fn sign_hyperliquid_user(&self, _: MsgPack, _: Vec<String>) -> Signing<'_, EvmSignature> { no() }
    fn sign_relay_mapping(&self, _: String, _: String, _: Vec<u8>, _: u64) -> Signing<'_, EvmSignature> { no() }
    fn sign_evm(&self, _: execution::trade_signer::EvmFees, _: String, _: Vec<u8>, _: Vec<String>) -> Signing<'_, Vec<u8>> { no() }
    fn sign_near_intent_deposit(&self, _: u128, _: String, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { no() }
}

/// A journal file of its own in the temp folder, removed when dropped.
pub struct TempJournal(pub PathBuf);

impl TempJournal {
    pub fn new(name: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!("g2-{name}-{}-{}.jsonl", std::process::id(), N.fetch_add(1, Ordering::Relaxed))))
    }
    pub fn journal(&self) -> Journal { Journal::new(&self.0) }
}

impl Drop for TempJournal {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

/// An executor for `solana`, its Solana reads at `node` (None: no node at all is asked in the test).
pub fn executor(solana: &str, node: Option<&str>, journal: &TempJournal) -> Arc<Executor> {
    let rpc = node.map_or_else(|| SolanaRpc::pinned("http://127.0.0.1:9"), SolanaRpc::pinned);
    Executor::with(TradeWallet { solana: solana.into(), evm: "0x0".into() }, Arc::new(NoSigner), journal.journal(), |_| {},
                   Setup { rpc, ..Default::default() })
}

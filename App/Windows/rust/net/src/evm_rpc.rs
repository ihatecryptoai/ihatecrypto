//! Ethereum JSON-RPC for one network (`EVMRPC`, `EVMNodes`, `ENS` in Swift).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use evm::abi::{self, address_in};
use evm::simulation::Simulation;
use evm::{U256, short_of_gas, not_this_node};
use serde_json::{Value, json};
use wallet::evm::{checksum, keccak};
use wallet::{Result, WalletError, from_hex, to_hex};

use crate::{Again, DELAYS, fail, head, post_json, run, transient};

/// The person's own node per network (Settings › General › Your own nodes): asked before the built-in ones.
static OWN: LazyLock<Mutex<HashMap<u64, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn custom(chain_id: u64) -> Option<String> { OWN.lock().expect("nodes").get(&chain_id).cloned() }
pub fn set_custom(url: Option<String>, chain_id: u64) {
    let mut own = OWN.lock().expect("nodes");
    match url { Some(u) => { own.insert(chain_id, u); } None => { own.remove(&chain_id); } }
}

#[derive(Debug, Clone)]
pub struct EvmRpc {
    pub url: String,
    pub chain_id: u64,
    /// Asked in turn when `url` throttles or drops a call: each retry goes to the next node.
    pub fallbacks: Vec<String>,
    /// Where signed transactions go instead, when set: a private relay that keeps them out of the public mempool.
    pub send_url: Option<String>,
    /// The person's node for the network is asked before `url`. A node made for one URL (a fork, a check) never is.
    pub follows_settings: bool,
}

impl EvmRpc {
    pub fn pinned(url: &str, chain_id: u64) -> Self { Self { url: url.into(), chain_id, fallbacks: vec![], send_url: None, follows_settings: false } }

    /// The app's own nodes for a network it signs on (2026-11-05: each served calls, estimates with a state override,
    /// fees, nonces, receipts and eth_simulateV1; Ethereum sends through MEV Blocker).
    pub fn network(chain_id: u64) -> Option<Self> {
        let n = |url: &str, fallbacks: &[&str], send: Option<&str>| Self {
            url: url.into(), chain_id, fallbacks: fallbacks.iter().map(|s| s.to_string()).collect(), send_url: send.map(str::to_string), follows_settings: true,
        };
        Some(match chain_id {
            42161 => n("https://arb1.arbitrum.io/rpc", &["https://arbitrum.drpc.org"], None),
            4663 => n("https://rpc.mainnet.chain.robinhood.com", &["https://robinhood.drpc.org"], None),
            8453 => n("https://mainnet.base.org", &["https://base.gateway.tenderly.co", "https://base.drpc.org"], None),
            137 => n("https://polygon.drpc.org", &["https://polygon.gateway.tenderly.co"], None),
            1 => n("https://eth.drpc.org", &["https://ethereum-rpc.publicnode.com"], Some("https://rpc.mevblocker.io")),
            4217 => n("https://rpc.mainnet.tempo.xyz", &[], None),
            999 => n("https://hyperliquid.drpc.org", &["https://rpc.hyperliquid.xyz/evm", "https://rpc.hypurrscan.io"], None),
            10 => n("https://mainnet.optimism.io", &["https://optimism.drpc.org"], None),
            56 => n("https://bsc-dataseed.bnbchain.org", &["https://bsc-rpc.publicnode.com", "https://bsc.drpc.org"], None),
            _ => return None,
        })
    }

    /// Every node a call may go to, in turn: the person's own (when this follows Settings), then `url`, then the fallbacks.
    pub fn nodes(&self) -> Vec<String> {
        let own = if self.follows_settings { custom(self.chain_id) } else { None };
        own.into_iter().chain([self.url.clone()]).chain(self.fallbacks.iter().cloned()).collect()
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> { self.call_with(method, params, &DELAYS).await }

    pub async fn call_with(&self, method: &str, params: Value, delays: &[Duration]) -> Result<Value> {
        let nodes = self.nodes();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let elsewhere = nodes.len() > 1;
        let chain_id = self.chain_id;
        // Public nodes throttle bursts, drop connections and have bad seconds: wait and ask again, each time the next
        // node. What the node answered on purpose (a revert, a short balance) is final at once.
        run(delays, |attempt| post_json(&nodes[attempt % nodes.len()], &body), |data, status| {
            let Ok(Value::Object(reply)) = serde_json::from_slice::<Value>(data) else {
                return Err(Again::new(status >= 500 || status == 429 || (elsewhere && (status == 401 || status == 403)),
                                      format!("EVM RPC {method}: HTTP {status}, response isn't JSON: {}", head(data, 120))));
            };
            if let Some(error) = reply.get("error").filter(|e| e.is_object()) {
                let message = error.get("message").and_then(Value::as_str).map_or_else(|| error.to_string(), str::to_string);
                if let Some(short) = short_of_gas(&message, chain_id) { return Err(Again::fin(WalletError(short))) }
                return Err(Again::new(status == 429 || transient(&message) || (elsewhere && (status == 401 || status == 403 || not_this_node(&message))),
                                      format!("EVM RPC {method}: {message}")));
            }
            // A refusal with a JSON body of its own ({"error": "forbidden"}) is a refusal, not an empty result — Swift read
            // it as null.
            if !(200..300).contains(&status) && !reply.contains_key("result") {
                return Err(Again::new(status >= 500 || status == 429 || (elsewhere && (status == 401 || status == 403)),
                                      format!("EVM RPC {method}: HTTP {status} {}", head(data, 120))));
            }
            Ok(reply.get("result").cloned().unwrap_or(Value::Null))
        }).await
    }

    /// eth_call: the raw return value.
    pub async fn read(&self, to: &str, data: &[u8]) -> Result<Vec<u8>> {
        let answer = self.call("eth_call", json!([{"to": to, "data": format!("0x{}", to_hex(data))}, "latest"])).await?;
        Ok(answer.as_str().and_then(from_hex).unwrap_or_default())
    }

    pub async fn quantity(&self, method: &str, params: Value) -> Result<u64> {
        let answer = self.call(method, params).await?;
        answer.as_str().and_then(|h| h.strip_prefix("0x")).and_then(|h| u64::from_str_radix(h, 16).ok())
            .ok_or_else(|| WalletError(format!("EVM RPC {method}: not a number ({})", answer.to_string().chars().take(60).collect::<String>())))
    }

    /// The coin at `address`, in wei: past 64 bits is common (20 ETH).
    pub async fn balance(&self, address: &str) -> Result<u128> {
        let answer = self.call("eth_getBalance", json!([address, "latest"])).await?;
        evm::wei(&answer).ok_or_else(|| WalletError(format!("EVM RPC eth_getBalance: not a number ({})", answer.to_string().chars().take(60).collect::<String>())))
    }

    pub async fn nonce(&self, address: &str) -> Result<u64> { self.quantity("eth_getTransactionCount", json!([address, "pending"])).await }

    pub async fn token_balance(&self, token: &str, owner: &str) -> Result<U256> {
        let data = evm::erc20::balance_of(owner)?;
        let answer = self.call("eth_call", json!([{"to": token, "data": format!("0x{}", to_hex(&data))}, "latest"])).await?;
        let hex = answer.as_str().unwrap_or("0x0").trim_start_matches("0x");
        Ok(U256::from_str_radix(if hex.is_empty() { "0" } else { hex }, 16).unwrap_or(U256::ZERO))
    }

    /// Current fees with headroom: max fee = 2 × base fee + tip. (max priority, max fee)
    pub async fn fees(&self) -> Result<(u64, u64)> {
        let base = self.quantity("eth_gasPrice", json!([])).await?;
        let tip = self.quantity("eth_maxPriorityFeePerGas", json!([])).await.unwrap_or(0);
        Ok((tip, base.saturating_mul(2).saturating_add(tip)))
    }

    /// eth_estimateGas as a dry run: fails if the call would revert or the balance is short. `running_code_of`: as if
    /// `from` were already delegated to that contract (EIP-7702), by a state override of its code.
    pub async fn estimate_gas(&self, from: &str, to: &str, value: u64, data: &[u8], running_code_of: Option<&str>) -> Result<u64> {
        let tx = json!({"from": from, "to": to, "value": format!("0x{value:x}"), "data": format!("0x{}", to_hex(data))});
        match running_code_of {
            None => self.quantity("eth_estimateGas", json!([tx])).await,
            Some(d) => {
                let mut over = serde_json::Map::new();
                over.insert(from.into(), json!({"code": format!("0xef0100{}", d.trim_start_matches("0x").to_lowercase())}));
                self.quantity("eth_estimateGas", json!([tx, "latest", over])).await
            }
        }
    }

    pub async fn send(&self, raw: &[u8]) -> Result<String> {
        let node = self.send_url.as_ref().map(|u| Self::pinned(u, self.chain_id)).unwrap_or_else(|| self.clone());
        match node.call("eth_sendRawTransaction", json!([format!("0x{}", to_hex(raw))])).await {
            Ok(Value::String(hash)) => Ok(hash),
            Ok(_) => fail("No transaction hash"),
            // A retry after the first send got through: the same transaction, waiting for its block.
            Err(e) if e.0.to_lowercase().contains("already known") || e.0.to_lowercase().contains("known transaction") => Ok(format!("0x{}", to_hex(&keccak(raw)))),
            Err(e) => Err(e),
        }
    }

    /// Several calls one after another in one block (an approve, then the call that spends it): the changes of all of
    /// them together, or the first one's revert.
    pub async fn simulate(&self, from: &str, calls: &[(String, u64, Vec<u8>)]) -> Result<Simulation> {
        let txs: Vec<Value> = calls.iter().map(|(to, value, data)| json!({"from": from, "to": to, "value": format!("0x{value:x}"), "data": format!("0x{}", to_hex(data))})).collect();
        let answer = self.call("eth_simulateV1", json!([{"blockStateCalls": [{"calls": txs}], "traceTransfers": true}, "latest"])).await?;
        Simulation::parse(&answer, from)
    }

    /// Waits for a receipt; true if the transaction succeeded.
    pub async fn wait_for_receipt(&self, hash: &str, timeout: Duration) -> Result<bool> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(receipt) = self.call("eth_getTransactionReceipt", json!([hash])).await {
                if let Some(status) = receipt.get("status").and_then(Value::as_str) { return Ok(status == "0x1") }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        fail(format!("No receipt after {} s: {hash}", timeout.as_secs()))
    }

    /// Why `url` can't be the wallet's node on this network, or None: https only; that very network; fees, nonces and
    /// receipts answered; and a state override really applied to a dry run (what a 7702 batch's estimate stands on).
    pub async fn problem(url: &str, chain_id: u64) -> Option<String> {
        if !url.starts_with("https://") || url.len() <= "https://".len() { return Some("A node's address starts with https://.".into()) }
        let rpc = Self::pinned(url, chain_id);
        let me = "0x00000000000000000000000000000000000f0a11";
        let checked: Result<Option<String>> = async {
            let id = rpc.quantity("eth_chainId", json!([])).await?;
            if id != chain_id { return Ok(Some(format!("That node is on network {id}, not {} ({chain_id}).", evm::name(chain_id).map_or(chain_id.to_string(), str::to_string)))) }
            rpc.fees().await?;
            rpc.nonce(me).await?;
            let block = rpc.call("eth_getBlockByNumber", json!(["latest", false])).await?;
            if !block.is_object() { return Ok(Some("The node didn't give its latest block.".into())) }
            if let Some(tx) = block.get("transactions").and_then(|t| t.get(0)).and_then(Value::as_str) {
                if !rpc.call("eth_getTransactionReceipt", json!([tx])).await?.is_object() { return Ok(Some("The node doesn't give transaction receipts.".into())) }
            }
            let dollar = match chain_id { 8453 => Some(evm::BASE_USDC), 42161 => Some(evm::ARBITRUM_USDC), 1 => Some(evm::ETHEREUM_USDC),
                                          137 => Some(evm::POLYGON_USDC), 4663 => Some(evm::ROBINHOOD_USDG), _ => None };
            if let Some(dollar) = dollar {
                let batch = evm::protocols::batch7702::execute_batch(&[evm::protocols::batch7702::BatchCall { to: dollar.into(), data: evm::erc20::approve(me, U256::from(1))? }])?;
                let gas = rpc.estimate_gas(me, me, 0, &batch, Some(evm::protocols::batch7702::DELEGATE)).await?;
                if gas <= 40_000 { return Ok(Some(format!("The node ignores state overrides in a dry run ({gas} gas): batched moves would go out short of gas."))) }
            }
            Ok(None)
        }.await;
        match checked {
            Ok(p) => p,
            Err(e) => {
                let why = e.0;
                let l = why.to_lowercase();
                if why.contains("HTTP 401") || why.contains("HTTP 403") || ["unauthorized", "api key", "authenticat", "forbidden"].iter().any(|w| l.contains(w)) {
                    Some("The node turned the key away: copy the whole address again, key included — or this plan doesn't serve these calls.".into())
                } else {
                    Some(format!("The node didn't answer: {why}"))
                }
            }
        }
    }
}

/// ENS names (vitalik.eth) to addresses, read on Ethereum itself: the registry names the name's resolver, the resolver
/// its address. Only plain ASCII labels: a look-alike letter is a different name, so it's refused rather than normalized.
pub mod ens {
    use super::*;

    pub const REGISTRY: &str = "0x00000000000c2e074ec69a0dfb2997ba6c7d2e1e";

    /// Lowercase ASCII labels of letters, digits and hyphens, ending in .eth.
    pub fn is_name(name: &str) -> bool {
        let Some(rest) = name.strip_suffix(".eth") else { return false };
        let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        rest.split('.').all(|l| {
            let b = l.as_bytes();
            !b.is_empty() && ok(b[0]) && ok(b[b.len() - 1]) && b.iter().all(|&c| ok(c) || c == b'-')
        })
    }

    /// EIP-137's namehash: keccak256(parent's node ‖ keccak256(label)), from the right.
    pub fn namehash(name: &str) -> [u8; 32] {
        name.split('.').filter(|l| !l.is_empty()).rev().fold([0u8; 32], |node, label| keccak(&[node, keccak(label.as_bytes())].concat()))
    }

    /// The address `name` points at now, checksummed.
    pub async fn resolve(name: &str, rpc: &EvmRpc) -> Result<String> {
        if !is_name(name) { return fail(format!("{name} isn't an ENS name ihatecrypto reads: plain letters, digits and hyphens, ending in .eth.")) }
        let node = namehash(name);
        let zero = format!("0x{}", "0".repeat(40));
        let resolver = address_in(&rpc.read(REGISTRY, &[&[0x01, 0x78, 0xb8, 0xbf][..], &node].concat()).await?, 0).filter(|r| *r != zero);
        let Some(resolver) = resolver else { return fail(format!("{name} has no resolver on Ethereum: it isn't registered, or it lives elsewhere.")) };
        let Ok(answer) = rpc.read(&resolver, &[&[0x3b, 0x3b, 0x57, 0xde][..], &node].concat()).await else {
            return fail(format!("{name}'s resolver doesn't answer on Ethereum (an off-chain name?). Paste the address itself."));
        };
        match address_in(&answer, 0).filter(|a| *a != zero) {
            Some(a) => Ok(checksum(&a)),
            None => fail(format!("{name} points at no address.")),
        }
    }

    #[allow(dead_code)]
    fn _words() -> [u8; 32] { abi::uint(0) }
}

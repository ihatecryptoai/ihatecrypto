//! Solana JSON-RPC (`SolanaRPC`, `SolanaNodes` and the RPC half of `Message.swift` and `SNS.swift`): what sending,
//! confirming and checking a transaction needs.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use solana::message::{self, AccountRights, Message, State};
use solana::spl::{self, sns};
use wallet::keys::base58;
use wallet::{Result, WalletError};

use crate::{Again, DELAYS, fail, head, post_json, run, transient};

/// The person's own node (Settings › General › Solana node), asked first when set; the public one after it, then Pocket's.
static OWN: Mutex<Option<String>> = Mutex::new(None);

pub fn custom() -> Option<String> { OWN.lock().expect("nodes").clone() }
pub fn set_custom(url: Option<String>) { *OWN.lock().expect("nodes") = url }

/// A node that doesn't serve this call, so the next node should: its key refused (401, 403: expired, over its plan), or
/// the node's own limits in Pocket's words (2026-11-09): HTTP 400 «… requires a mint filter on this endpoint», HTTP 400
/// «request budget exhausted», and -32010 «Tokenkeg… excluded from account secondary indexes» from a backend without the
/// index. `body`: the HTTP body, or a JSON-RPC error's message. Any other answer is the call's own.
pub fn passes_on(status: u16, body: &[u8]) -> bool {
    let text = head(body, 400);
    status == 401 || status == 403 || ["on this endpoint", "budget exhausted", "secondary indexes"].iter().any(|w| text.contains(w))
}

#[derive(Debug, Clone)]
pub struct SolanaRpc {
    /// The nodes asked when made for some (a check, a test); None: the person's own, the public one, Pocket's.
    fixed: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome { Confirmed, Failed(String), Expired }

/// What a simulation left: its error, the watched accounts' state after, the slot it ran at, who has a say over each.
#[derive(Debug, Clone, Default)]
pub struct Simulated {
    pub error: Option<String>,
    pub after: HashMap<String, State>,
    pub slot: u64,
    pub rights: HashMap<String, AccountRights>,
}

impl Default for SolanaRpc {
    fn default() -> Self { Self::new() }
}

impl SolanaRpc {
    /// The nodes read at each call: the person's own first when set.
    pub fn new() -> Self { Self { fixed: None } }
    /// One node and no other (a check, a test).
    pub fn pinned(url: &str) -> Self { Self { fixed: Some(vec![url.into()]) } }
    #[cfg(test)]
    pub(crate) fn with_nodes(nodes: &[String]) -> Self { Self { fixed: Some(nodes.to_vec()) } }

    pub fn nodes(&self) -> Vec<String> {
        match &self.fixed {
            Some(nodes) => nodes.clone(),
            None => custom().into_iter().chain([solana::PUBLIC_NODE, solana::POCKET_NODE].map(String::from)).collect(),
        }
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> { self.call_with(method, params, &DELAYS).await }

    pub async fn call_with(&self, method: &str, params: Value, delays: &[Duration]) -> Result<Value> {
        let nodes = self.nodes();
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        // The public RPC answers 429 under bursts and drops a connection now and then: backed-off retries, each to the
        // next node. A node refusing its key or the call (`passes_on`) passes the call to the next.
        run(delays, |attempt| post_json(&nodes[attempt % nodes.len()], &body), |data, status| {
            if !(200..300).contains(&status) {
                let next = passes_on(status, data) && nodes.len() > 1;
                return Err(Again::new(status == 429 || status >= 500 || next, format!("Solana RPC {method}: HTTP {status} {}", head(data, 120))));
            }
            let Ok(Value::Object(reply)) = serde_json::from_slice::<Value>(data) else {
                return Err(Again::new(false, format!("Solana RPC {method}: response isn't JSON: {}", head(data, 120))));
            };
            if let Some(error) = reply.get("error").filter(|e| e.is_object()) {
                let message = error.get("message").and_then(Value::as_str).map_or_else(|| error.to_string(), str::to_string);
                let next = passes_on(status, message.as_bytes()) && nodes.len() > 1;
                return Err(Again::new(transient(&message) || next, format!("Solana RPC {method}: {message}")));
            }
            Ok(reply.get("result").cloned().unwrap_or(Value::Null))
        }).await
    }

    pub async fn balance(&self, address: &str) -> Result<u64> {
        Ok(self.call("getBalance", json!([address, {"commitment": "finalized"}])).await?.get("value").and_then(Value::as_u64).unwrap_or(0))
    }

    /// The lamports an account of `bytes` must hold to open, as the network says it now. Never written down: it keeps
    /// stepping down (SIMD-0437).
    pub async fn rent_minimum(&self, bytes: usize) -> Result<u64> {
        // Bounded, so no sum with it can overflow: a node's -1 or 1e30 is no rent.
        match self.call("getMinimumBalanceForRentExemption", json!([bytes])).await?.as_i64() {
            Some(l) if l > 0 && l < 1 << 40 => Ok(l as u64),
            _ => fail("Solana RPC getMinimumBalanceForRentExemption: no rent minimum in the answer"),
        }
    }

    /// What `owner`'s own token account for `mint` holds, in whole tokens; 0 without one. Two light reads, never
    /// getTokenAccountsByOwner, which the public node throttles right after a swap.
    pub async fn owned_amount(&self, owner: &str, mint: &str) -> Result<Decimal> {
        let info = self.call("getAccountInfo", json!([mint, {"encoding": "base64", "commitment": "finalized"}])).await?;
        let program = info.get("value").and_then(|v| v.get("owner")).and_then(Value::as_str)
            .filter(|p| [spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM].contains(p));
        let Some(program) = program else { return fail(format!("{mint} isn't a token mint on Solana.")) };
        let account = spl::associated_account(owner, mint, program)?;
        let Ok(answer) = self.call("getTokenAccountBalance", json!([account, {"commitment": "finalized"}])).await else { return Ok(Decimal::ZERO) };
        let value = answer.get("value");
        let units = value.and_then(|v| v.get("amount")).and_then(Value::as_str).and_then(|a| a.parse::<Decimal>().ok());
        let decimals = value.and_then(|v| v.get("decimals")).and_then(Value::as_u64);
        match (units, decimals) {
            (Some(u), Some(d)) if d <= 28 => Ok(u / Decimal::from_i128_with_scale(10i128.pow(d as u32), 0)),
            _ => Ok(Decimal::ZERO),
        }
    }

    /// Raw units of an SPL token held by `owner` across its accounts.
    pub async fn token_balance(&self, owner: &str, mint: &str) -> Result<u64> {
        let r = self.call("getTokenAccountsByOwner", json!([owner, {"mint": mint}, {"encoding": "jsonParsed", "commitment": "finalized"}])).await?;
        Ok(r.get("value").and_then(Value::as_array).map_or(0, |list| list.iter().map(|a| {
            a.pointer("/account/data/parsed/info/tokenAmount/amount").and_then(Value::as_str).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0)
        }).fold(0u64, u64::saturating_add)))
    }

    /// (blockhash, lastValidBlockHeight)
    pub async fn latest_blockhash(&self) -> Result<(String, u64)> {
        let r = self.call("getLatestBlockhash", json!([{"commitment": "finalized"}])).await?;
        let v = r.get("value");
        match (v.and_then(|v| v.get("blockhash")).and_then(Value::as_str), v.and_then(|v| v.get("lastValidBlockHeight")).and_then(Value::as_u64)) {
            (Some(h), Some(height)) => Ok((h.into(), height)),
            _ => fail("No blockhash"),
        }
    }

    /// Simulates first (preflight), then broadcasts. Returns the signature, which is the transaction id.
    pub async fn send(&self, transaction: &[u8]) -> Result<String> {
        match self.call("sendTransaction", json!([STANDARD.encode(transaction), {"encoding": "base64", "preflightCommitment": "finalized", "maxRetries": 5}])).await {
            Ok(Value::String(s)) => Ok(s),
            Ok(_) => fail("sendTransaction returned no signature"),
            // A retry after the first send got through: the same transaction, named by its first signature.
            Err(e) if e.0.contains("already been processed") && transaction.len() > 65 => Ok(base58::encode(&transaction[1..65])),
            Err(e) => Err(e),
        }
    }

    /// Runs the transaction without sending it, with signature checks on. None = it would succeed.
    pub async fn simulate_plain(&self, transaction: &[u8]) -> Result<(Option<String>, Vec<String>)> {
        let r = self.call("simulateTransaction", json!([STANDARD.encode(transaction), {"encoding": "base64", "sigVerify": true, "commitment": "finalized"}])).await?;
        let value = r.get("value");
        let err = value.and_then(|v| v.get("err")).filter(|e| !e.is_null()).map(Value::to_string);
        let logs = value.and_then(|v| v.get("logs")).and_then(Value::as_array).map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Ok((err, logs))
    }

    /// Addresses stored in an address lookup table account.
    pub async fn lookup_table(&self, address: &str) -> Result<Vec<String>> {
        let r = self.call("getAccountInfo", json!([address, {"encoding": "base64", "commitment": "finalized"}])).await?;
        let raw = r.pointer("/value/data/0").and_then(Value::as_str).and_then(|d| STANDARD.decode(d).ok()).filter(|r| r.len() >= 56);
        let Some(raw) = raw else { return fail(format!("Address lookup table not found: {address}")) };
        Ok(message::lookup_table(&raw))
    }

    /// Full account list of a message, loading its lookup tables from the chain.
    pub async fn resolve_accounts(&self, m: &Message) -> Result<Vec<String>> {
        let mut tables = HashMap::new();
        for l in &m.lookups { tables.insert(l.table.clone(), self.lookup_table(&l.table).await?); }
        m.accounts(&tables)
    }

    /// Simulates and returns the watched accounts' state afterwards. `sig_verify: false` simulates before signing.
    pub async fn simulate(&self, transaction: &[u8], watch: &[String], sig_verify: bool) -> Result<Simulated> {
        let r = self.call("simulateTransaction", json!([STANDARD.encode(transaction), {
            "encoding": "base64", "sigVerify": sig_verify, "commitment": "finalized",
            "accounts": {"encoding": "jsonParsed", "addresses": watch},
        }])).await?;
        let value = r.get("value");
        let mut out = Simulated { slot: r.pointer("/context/slot").and_then(Value::as_u64).unwrap_or(0), ..Default::default() };
        let accounts = value.and_then(|v| v.get("accounts")).and_then(Value::as_array).cloned().unwrap_or_default();
        for (address, account) in watch.iter().zip(&accounts) {
            out.after.insert(address.clone(), message::state(account).unwrap_or((0, None)));
            if let Some(r) = message::rights(account) { out.rights.insert(address.clone(), r); }
        }
        let Some(err) = value.and_then(|v| v.get("err")).filter(|e| !e.is_null()) else { return Ok(out) };
        let text = if err.is_object() || err.is_array() { err.to_string() } else { err.as_str().map_or_else(|| err.to_string(), str::to_string) };
        let logs: Vec<String> = value.and_then(|v| v.get("logs")).and_then(Value::as_array)
            .map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        // Short of SOL: said with what to do, not as Solana's code.
        if message::short_of_sol(&text, &logs) {
            if let Some(payer) = Message::parse_transaction(transaction).ok().and_then(|m| m.static_keys.first().cloned()) {
                let have = self.balance(&payer).await.map(|l| format!(" has {} SOL and", Decimal::from(l) / Decimal::from(1_000_000_000u64))).unwrap_or_default();
                out.error = Some(format!("Not enough SOL: the wallet {}…{}{have} can't pay Solana's fee and the rent of the account this move opens. \
                                          Send it ≈ 0.01 SOL, then try again.", &payer[..4], &payer[payer.len() - 4..]));
                return Ok(out);
            }
        }
        // The program's own words when it logged any, e.g. «Error: insufficient funds».
        let reason = logs.iter().find(|l| l.contains("Error")).map(|l| l.replace("Program log: ", ""));
        out.error = Some(reason.map_or(text.clone(), |r| format!("{r} {text}")));
        Ok(out)
    }

    /// The watched accounts' state now, in one read and one slot.
    pub async fn accounts(&self, addresses: &[String]) -> Result<HashMap<String, State>> {
        let r = self.call("getMultipleAccounts", json!([addresses, {"encoding": "jsonParsed", "commitment": "finalized"}])).await?;
        message::accounts(addresses, r.get("value"))
    }

    /// Waits for «confirmed». Past `last_valid_block_height` a transaction can't land any more, so «expired» is a definite
    /// «didn't land» — once the status, read again from a node that has finalized a block past that height, still finds
    /// nothing.
    pub async fn wait_for_confirmation(&self, signature: &str, last_valid_block_height: u64, timeout: Duration) -> Result<Outcome> {
        let deadline = Instant::now() + timeout;
        let mut past_slot: Option<u64> = None;
        while Instant::now() < deadline {
            let r = self.call("getSignatureStatuses", json!([[signature], {"searchTransactionHistory": true}])).await?;
            let status = r.pointer("/value/0").filter(|s| s.is_object());
            if let Some(status) = status {
                if let Some(err) = status.get("err").filter(|e| !e.is_null()) { return Ok(Outcome::Failed(err.to_string())) }
                if matches!(status.get("confirmationStatus").and_then(Value::as_str), Some("confirmed" | "finalized")) { return Ok(Outcome::Confirmed) }
            } else if let Some(past) = past_slot {
                if r.pointer("/context/slot").and_then(Value::as_u64).unwrap_or(0) >= past { return Ok(Outcome::Expired) }
            } else {
                let epoch = self.call("getEpochInfo", json!([{"commitment": "finalized"}])).await?;
                if let (Some(height), Some(slot)) = (epoch.get("blockHeight").and_then(Value::as_u64), epoch.get("absoluteSlot").and_then(Value::as_u64)) {
                    if height > last_valid_block_height {
                        past_slot = Some(slot);
                        continue; // the status once more, now: it may have landed between the two reads
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        fail(format!("Transaction status still unknown after {} s: {signature}", timeout.as_secs()))
    }

    /// A transaction we signed second (Jupiter's gasless swap, a JupiterZ fill): looked for among `signer`'s own
    /// finalized transactions after `slot`. «expired» only once its blockhash is past use and the wallet's history back
    /// to `slot` holds none with ours.
    pub async fn wait_for_cosigned(&self, signature: &str, signer: &str, blockhash: &str, slot: u64, timeout: Duration) -> Result<(Outcome, Option<String>)> {
        let deadline = Instant::now() + timeout;
        let mut others: HashSet<String> = HashSet::new();
        while Instant::now() < deadline {
            let attempt: Result<Option<(Outcome, Option<String>)>> = async {
                let valid = self.call("isBlockhashValid", json!([blockhash, {"commitment": "finalized", "minContextSlot": slot}])).await.ok();
                let dead = valid.as_ref().filter(|v| slot > 0 && v.get("value").and_then(Value::as_bool) == Some(false))
                    .and_then(|v| v.pointer("/context/slot").and_then(Value::as_u64));
                let list = self.call("getSignaturesForAddress", json!([signer, {"limit": 100, "commitment": "finalized", "minContextSlot": dead.unwrap_or(slot)}])).await?;
                let Some(list) = list.as_array() else { return fail("Solana RPC getSignaturesForAddress: no list") };
                let (mut reached, mut unread) = (list.len() < 100, false);
                for s in list {
                    let (Some(id), Some(at)) = (s.get("signature").and_then(Value::as_str), s.get("slot").and_then(Value::as_u64)) else { unread = true; continue };
                    if at <= slot { reached = true; break }
                    if others.contains(id) { continue }
                    // Read whole or not at all: a reply without its signatures or its outcome may be ours.
                    let tx = self.call("getTransaction", json!([id, {"encoding": "json", "commitment": "finalized", "maxSupportedTransactionVersion": 1}])).await?;
                    let (Some(meta), Some(signatures)) = (tx.get("meta"), tx.pointer("/transaction/signatures").and_then(Value::as_array)) else { unread = true; continue };
                    if !signatures.iter().any(|s| s.as_str() == Some(signature)) { others.insert(id.into()); continue }
                    let outcome = match meta.get("err").filter(|e| !e.is_null()) { None => Outcome::Confirmed, Some(e) => Outcome::Failed(e.to_string()) };
                    return Ok(Some((outcome, Some(id.to_string()))));
                }
                Ok((dead.is_some() && reached && !unread).then_some((Outcome::Expired, None)))
            }.await;
            // A node that fails or is behind proves nothing either way: asked again in a moment, until the deadline.
            if let Ok(Some(found)) = attempt { return Ok(found) }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        fail(format!("Transaction status still unknown after {} s: signed {signature}", timeout.as_secs()))
    }

    /// A .sol name's owner, read on Solana itself.
    pub async fn resolve_sns(&self, name: &str) -> Result<String> {
        if !sns::is_name(name) { return Err(WalletError(sns::not_a_name(name))) }
        let r = self.call("getAccountInfo", json!([sns::account(name)?, {"encoding": "base64", "commitment": "finalized"}])).await?;
        let program = r.pointer("/value/owner").and_then(Value::as_str);
        let data = r.pointer("/value/data/0").and_then(Value::as_str).and_then(|d| STANDARD.decode(d).ok());
        sns::owner(name, program, data.as_deref())
    }

    /// Why `url` can't be the wallet's Solana node, or None: https only; Solana mainnet; token accounts by owner answered.
    pub async fn problem(url: &str) -> Option<String> {
        if !url.starts_with("https://") || url.len() <= "https://".len() { return Some("A node's address starts with https://.".into()) }
        let node = Self::pinned(url);
        let checked: Result<Option<String>> = async {
            let genesis = node.call("getGenesisHash", json!([])).await?;
            let Some(genesis) = genesis.as_str() else { return Ok(Some("The node didn't answer like a Solana node.".into())) };
            if genesis != solana::MAINNET_GENESIS { return Ok(Some("That node is on another Solana network (devnet or testnet), not mainnet.".into())) }
            node.call("getTokenAccountsByOwner", json!(["11111111111111111111111111111111", {"programId": spl::TOKEN_PROGRAM}, {"encoding": "jsonParsed"}])).await?;
            node.call("getLatestBlockhash", json!([{"commitment": "finalized"}])).await?;
            Ok(None)
        }.await;
        match checked {
            Ok(p) => p,
            Err(e) if e.0.contains("HTTP 401") || e.0.contains("HTTP 403") => Some(format!(
                "The node turned the key away ({}): copy the whole address again, api-key included — or this plan doesn't serve these calls.",
                if e.0.contains("HTTP 401") { "401" } else { "403" })),
            Err(e) => Some(format!("The node didn't answer: {}", e.0)),
        }
    }
}

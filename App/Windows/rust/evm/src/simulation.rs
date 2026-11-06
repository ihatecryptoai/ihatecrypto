//! One eth_simulateV1 result, read as one address's balance changes (`EVMSimulation` in EVM.swift). With
//! traceTransfers the node reports coin moves as Transfer logs of 0xeeee…eeee, which no contract can emit. Token moves are
//! their contracts' own Transfer logs (plus WETH9's Deposit/Withdrawal) — a contract can log anything, so a token is
//! known by its address.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde_json::Value as Json;
use wallet::evm::keccak;
use wallet::{Result, from_hex, to_hex};

use crate::abi::u256_in;
use crate::fail;

pub const NATIVE: &str = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
/// EIP-7708: the chain itself logs every coin move as a Transfer from SYSTEM_ADDRESS. Read as `NATIVE`; a node that
/// sends both is counted once, by 0xeeee.
const SYSTEM: &str = "0xfffffffffffffffffffffffffffffffffffffffe";
/// WETH9 on Ethereum and the OP chains, WHYPE on HyperEVM, WBNB on BNB Chain (the same WETH9 code).
const WETH9: [&str; 4] = ["0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", "0x4200000000000000000000000000000000000006",
                          "0x5555555555555555555555555555555555555555", "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"];
const TRANSFER: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const DEPOSIT: &str = "0xe1fffcc4923d04b559f4d29a8bfc6cda04eb5b0d3c460751c2402c5c5cc9109c";
const WITHDRAWAL: &str = "0x7fcf532c15f0a6db0bd6d0e038bea71d30d808c7d98cb3bf7268a95bf5081b65";

/// A right this wallet hands out in the simulated calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// approve, nft, all, permit2, operator, authorization, credit, ownership
    pub kind: String,
    /// The contract that logged it.
    pub asset: String,
    /// Who gets the right.
    pub to: String,
    pub unlimited: bool,
}

/// (event topic, kind, topic of the owner, topic of whoever gets the right)
static GRANT_EVENTS: LazyLock<Vec<(String, &'static str, usize, usize)>> = LazyLock::new(|| {
    [("Approval(address,address,uint256)", "approve", 1, 2), ("ApprovalForAll(address,address,bool)", "all", 1, 2),
     ("Approval(address,address,address,uint160,uint48)", "permit2", 1, 3), ("OperatorSet(address,address,bool)", "operator", 1, 2),
     ("SetAuthorization(address,address,address,bool)", "authorization", 2, 3),
     ("BorrowAllowanceDelegated(address,address,address,uint256)", "credit", 1, 2), ("OwnershipTransferred(address,address)", "ownership", 1, 2)]
        .into_iter().map(|(e, k, o, t)| (format!("0x{}", to_hex(&keccak(e.as_bytes()))), k, o, t)).collect()
});

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Simulation {
    /// Why the call reverts; None when it succeeds.
    pub revert: Option<String>,
    /// Net change per asset contract (`NATIVE` for the coin), in raw units; an NFT collection counts items. No zeros.
    /// ponytail: i128, saturating — a real amount is far below 1.7e38; a hostile log's huge one stays huge, in its sign.
    pub changes: BTreeMap<String, i128>,
    /// ERC-721 collections among `changes`.
    pub nfts: BTreeSet<String>,
    /// Rights this wallet hands out, read from the events standard contracts emit: what the card's balances can't show.
    pub grants: Vec<Grant>,
}

impl Simulation {
    pub fn parse(result: &Json, owner: &str) -> Result<Self> {
        let Some(calls) = result.get(0).and_then(|r| r.get("calls")).and_then(Json::as_array).filter(|c| !c.is_empty()) else {
            return fail("eth_simulateV1: unexpected reply");
        };
        let mut sim = Simulation::default();
        if let Some(failed) = calls.iter().position(|c| c.get("status").and_then(Json::as_str) != Some("0x1")) {
            let why = calls[failed].get("error").and_then(|e| e.get("message")).and_then(Json::as_str).unwrap_or("reverted");
            sim.revert = Some(if calls.len() > 1 { format!("call {} of {}: {why}", failed + 1, calls.len()) } else { why.into() });
            return Ok(sim);
        }
        let me = owner.to_lowercase();
        let me = me.strip_prefix("0x").unwrap_or(&me).to_string();
        let logs: Vec<&Json> = calls.iter().filter_map(|c| c.get("logs").and_then(Json::as_array)).flatten().collect();
        let traced = logs.iter().any(|l| l.get("address").and_then(Json::as_str).map(str::to_lowercase).as_deref() == Some(NATIVE));
        for log in logs {
            let (Some(address), Some(topics)) = (log.get("address").and_then(Json::as_str), log.get("topics").and_then(Json::as_array)) else { continue };
            let Some(topics) = topics.iter().map(|t| t.as_str().map(str::to_lowercase)).collect::<Option<Vec<_>>>() else { continue };
            let Some(event) = topics.first().cloned() else { continue };
            let mut asset = address.to_lowercase();
            if asset == SYSTEM {
                if traced { continue }
                asset = NATIVE.into();
            }
            let data = from_hex(log.get("data").and_then(Json::as_str).unwrap_or("0x")).unwrap_or_default();
            if let Some(g) = grant(&asset, &topics, &data, &me) { sim.grants.push(g) }
            let amount = u256_in(&data, 0).map_or(0, |a| i128::try_from(a).unwrap_or(i128::MAX));
            let (from, to, n) = match (event.as_str(), topics.len()) {
                (TRANSFER, 3) => (topics[1].clone(), topics[2].clone(), amount),
                (TRANSFER, 4) => {
                    sim.nfts.insert(asset.clone());
                    (topics[1].clone(), topics[2].clone(), 1)
                }
                (DEPOSIT, 2) if WETH9.contains(&asset.as_str()) => (String::new(), topics[1].clone(), amount),
                (WITHDRAWAL, 2) if WETH9.contains(&asset.as_str()) => (topics[1].clone(), String::new(), amount),
                _ => continue,
            };
            let entry = sim.changes.entry(asset).or_insert(0);
            if from.ends_with(&me) { *entry = entry.saturating_sub(n) }
            if to.ends_with(&me) { *entry = entry.saturating_add(n) }
        }
        sim.changes.retain(|_, v| *v != 0);
        Ok(sim)
    }
}

/// A right `me` gives in this log, or None (another owner, or a revoke: zero, false).
fn grant(asset: &str, topics: &[String], data: &[u8], me: &str) -> Option<Grant> {
    let (_, kind, owner, to) = GRANT_EVENTS.iter().find(|e| Some(&e.0) == topics.first())?;
    if topics.len() <= *owner.max(to) || !topics[*owner].ends_with(me) { return None }
    let who = format!("0x{}", &topics[*to][topics[*to].len().saturating_sub(40)..]);
    // Nobody: the zero address clears an approval.
    if who == format!("0x{}", "0".repeat(40)) { return None }
    // An ERC-721 approve carries its token id as a fourth topic: one NFT, no amount in the data.
    if *kind == "approve" && topics.len() == 4 { return Some(Grant { kind: "nft".into(), asset: asset.into(), to: who, unlimited: false }) }
    if *kind == "ownership" { return Some(Grant { kind: (*kind).into(), asset: asset.into(), to: who, unlimited: true }) }
    let word: Vec<u8> = if data.len() >= 32 { data[..32].to_vec() } else { vec![0; 32] };
    if word.iter().all(|&b| b == 0) { return None }
    // A bool says only yes; an amount past 2^128 is as good as unlimited.
    let flag = ["all", "operator", "authorization"].contains(kind);
    Some(Grant { kind: (*kind).into(), asset: asset.into(), to: who, unlimited: flag || word[..16].iter().any(|&b| b != 0) })
}

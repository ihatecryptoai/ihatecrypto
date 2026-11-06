//! Uniswap v4 (V4.swift), for the stock and token buys and sales (stocks.rs) and the pair swaps: the pool without hooks
//! for a pair, its quote, and the swap — the input to Permit2 (exact), Permit2 to the Universal Router (exact, for half an
//! hour), then one V4_SWAP that pays this wallet (the signer reads it back field by field: `v4::refusal`). Also every way
//! a trade on Robinhood Chain can go, quoted for one amount.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use evm::abi::{address_in, uint_in};
use evm::erc20;
use evm::protocols::batch7702::BatchCall;
use evm::protocols::v4::{self as uniswap_v4, INITIALIZE_TOPIC, NATIVE, PERMIT2, PoolKey};
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded, text};
use ihatecrypto_core::holdings;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::tokens::LookupInfo;
use net::evm_rpc::EvmRpc;
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::aave::EvmNetwork;
use crate::executor::uint64;
use crate::stocks::Token;
use crate::swap::{dollars, fetch, millionths, scale, sleep, tokens, unix_now, web};
use crate::{Executor, Result, fail, num, token_balance, u256, word};

/// A v4 pool and the way through it, and what it gives for the amount quoted.
#[derive(Debug, Clone, PartialEq)]
pub struct V4Route {
    pub key: PoolKey,
    pub zero_for_one: bool,
    pub out: Decimal,
}

/// Every way a trade on Robinhood Chain can go, quoted for one amount: Uniswap v4's pool without hooks, v3's best pool,
/// Relay's swap (only for a token DexScreener lists: its checks need that price).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StockWays {
    pub v4: Option<V4Route>,
    /// v3's best pool: its fee and what it gives.
    pub v3: Option<(u64, Decimal)>,
    pub relay: Option<Decimal>,
}

impl StockWays {
    /// What each gives, the most first: «v4», «v3», «relay».
    pub fn ranked(&self) -> Vec<(&'static str, Decimal)> {
        let mut ranked: Vec<(&'static str, Decimal)> = [("v4", self.v4.as_ref().map(|v| v.out)), ("v3", self.v3.map(|v| v.1)), ("relay", self.relay)]
            .into_iter().filter_map(|(name, out)| out.map(|o| (name, o))).filter(|w| w.1 > Decimal::ZERO).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked
    }

    pub fn title(way: &str) -> &'static str {
        match way { "v4" => "Uniswap v4", "v3" => "Uniswap v3", _ => "Relay's swap" }
    }
}

/// Pool keys read this launch, by network and pool id: a key never changes (Swift's `V4Keys`).
static V4_KEYS: LazyLock<Mutex<HashMap<String, PoolKey>>> = LazyLock::new(Default::default);

fn known(chain_id: u64, id: &str) -> Option<PoolKey> {
    V4_KEYS.lock().unwrap_or_else(|e| e.into_inner()).get(&format!("{chain_id}:{}", id.to_lowercase())).cloned()
}

fn keep(chain_id: u64, id: &str, key: &PoolKey) {
    V4_KEYS.lock().unwrap_or_else(|e| e.into_inner()).insert(format!("{chain_id}:{}", id.to_lowercase()), key.clone());
}

/// Eight big-endian bytes as Swift's `reduce(Int64(0)) { $0 << 8 | Int64($1) }`, then `Int32(truncatingIfNeeded:)`.
fn spacing(bytes: &[u8]) -> Option<i32> { Some(i64::from_be_bytes(bytes.try_into().ok()?) as i32) }

impl Executor {
    /// All three ways for `units` of what goes in (USDG for a buy, the token for a sale), quoted at once.
    pub async fn stock_ways(&self, token: &Token, buy: bool, units: Decimal, listing: Option<&LookupInfo>) -> StockWays {
        let (usdg, me) = (evm::ROBINHOOD_USDG, self.wallet.evm.as_str());
        let (token_in, token_out) = if buy { (usdg, token.address.as_str()) } else { (token.address.as_str(), usdg) };
        let relay = async {
            listing?;
            let quote = relay::quote(&relay::Ask {
                user: me.into(), recipient: me.into(), origin_chain: relay::chain::ROBINHOOD, destination_chain: relay::chain::ROBINHOOD,
                origin_currency: token_in.into(), destination_currency: token_out.into(), units: Some(text(units)), ..Default::default()
            }).await.ok()?;
            decimal(&quote.amount_out)
        };
        let rpc = &EvmNetwork::robinhood().rpc;
        let (v4, v3, relay) = tokio::join!(self.v4_route(token_in, token_out, units, rpc), self.best_pool(token_in, token_out, units), relay);
        StockWays { v4, v3: v3.ok().flatten(), relay }
    }

    /// The ways for the card while it waits for Confirm, the best first, nothing signed: what each gives and its worth
    /// in dollars (a buy: the dollars in, scaled by what each gives against the best). (way, gets, worth)
    pub async fn stock_quotes(&self, symbol_or_contract: &str, usdc: Option<Decimal>, sell: Option<Decimal>) -> Result<Vec<(String, String, Decimal)>> {
        let rpc = &EvmNetwork::robinhood().rpc;
        let (token, listing) = crate::stocks::RobinhoodStocks::resolve(symbol_or_contract, rpc).await?;
        if let Some(usdc) = usdc {
            let mut units = millionths(usdc)?;
            let there = token_balance(rpc, evm::ROBINHOOD_USDG, &self.wallet.evm).await?;
            if there < units && there > 0 && units - there <= 1_000_000.max(units / 50) { units = there }
            let ranked = self.stock_ways(&token, true, Decimal::from(units), listing.as_ref()).await.ranked();
            let Some(top) = ranked.first().map(|w| w.1) else { return Ok(vec![]) };
            // Six places: a dollar of a $400 stock is 0.0025 of it, and two places showed «0 TSLA».
            return Ok(ranked.iter().map(|(name, out)| {
                let worth = (Num::of(dollars(units)) * *out / top).0.unwrap_or_default();
                (name.to_string(), format!("{} {}", Fmt::num(tokens(*out, token.decimals), 6), token.symbol), worth)
            }).collect());
        }
        let held = self.balance(&token.address, rpc).await?;
        let wanted = rounded(match sell { Some(s) => num(Num::of(s) * scale(token.decimals))?.min(held), None => held }, 0, Rounding::Down);
        if wanted <= Decimal::ZERO { return Ok(vec![]) }
        let million = Decimal::from(1_000_000);
        Ok(self.stock_ways(&token, false, wanted, listing.as_ref()).await.ranked().into_iter()
            .map(|(name, out)| (name.to_string(), format!("{} USDG", Fmt::num(out / million, 2)), out / million)).collect())
    }

    /// «Uniswap v4 ≈ 1,906 AI · Relay's swap ≈ 1,880 · Uniswap v3 ≈ 1,372»: what the report says before the best is taken.
    pub fn ways_line(&self, ways: &StockWays, unit: impl Fn(Decimal) -> String) -> String {
        ways.ranked().into_iter().map(|(name, out)| format!("{} ≈ {}", StockWays::title(name), unit(out))).collect::<Vec<_>>().join(" · ")
    }

    /// The v4 pool for token_in → token_out on `rpc`'s network and what it gives for `amount_in`; None when there's none
    /// without hooks, or with hooks Uniswap's registry vouches for (`v4::allows_hooks`). Every such pool of the pair is
    /// quoted for this amount and the one that gives most is taken: the deepest isn't always the best price (AI/USDG on
    /// Robinhood has five). (Swift's default `rpc` is Robinhood Chain's.)
    pub async fn v4_route(&self, token_in: &str, token_out: &str, amount_in: Decimal, rpc: &EvmRpc) -> Option<V4Route> {
        let mut best: Option<V4Route> = None;
        for key in Self::v4_pools(token_in, token_out, rpc.chain_id).await {
            let zero_for_one = token_in.to_lowercase() == key.currency0;
            if let Some(out) = self.v4_quote(&key, zero_for_one, amount_in, rpc).await {
                if out > best.as_ref().map_or(Decimal::ZERO, |b| b.out) { best = Some(V4Route { key, zero_for_one, out }) }
            }
        }
        best
    }

    /// V4Quoter on the chain itself.
    pub async fn v4_quote(&self, key: &PoolKey, zero_for_one: bool, amount: Decimal, rpc: &EvmRpc) -> Option<Decimal> {
        let quoter = uniswap_v4::on(rpc.chain_id)?.quoter;
        let call = uniswap_v4::quote_exact_input_single(key, zero_for_one, u256(amount).ok()?).ok()?;
        word(&rpc.read(quoter, &call).await.ok()?, 0)
    }

    /// The pair's deepest v4 pool whose hooks may run (v4_pools). (Swift's default chain is Robinhood Chain, 4663.)
    pub async fn v4_pool(a: &str, b: &str, chain_id: u64) -> Option<PoolKey> { Self::v4_pools(a, b, chain_id).await.into_iter().next() }

    /// The pair's v4 pools among its five deepest whose hooks may run (none, or vouched for: `v4::allows_hooks`), the
    /// deepest first. DexScreener's token-pairs lists a token's pools (a v4 pair's address is its pool id; 30 at most —
    /// tokens/v1 gave only its one biggest pair); each key comes from v4_key and must hash back to that id. One side is the
    /// network's dollar.
    pub async fn v4_pools(a: &str, b: &str, chain_id: u64) -> Vec<PoolKey> {
        let (Some(net), Some(_)) = (EvmNetwork::of(chain_id), uniswap_v4::on(chain_id)) else { return vec![] };
        let pair: HashSet<String> = [a.to_lowercase(), b.to_lowercase()].into();
        // DexScreener lists a token's pools: the side that's neither the dollar nor the network's coin (v4::NATIVE).
        let token = [a, b].into_iter().find(|t| ![net.usdc.to_lowercase(), NATIVE.to_string()].contains(&t.to_lowercase())).unwrap_or(a);
        let Some((data, _)) = fetch(&web(&format!("https://api.dexscreener.com/token-pairs/v1/{}/{token}", net.key)), &[], 15).await else { return vec![] };
        let Ok(Value::Array(pairs)) = serde_json::from_slice::<Value>(&data) else { return vec![] };
        let side = |p: &Value, k: &str| p.get(k)?.get("address")?.as_str().map(str::to_lowercase);
        let depth = |p: &Value| p.get("liquidity").and_then(|l| l.get("usd")).and_then(Value::as_f64).unwrap_or(0.0);
        let mut v4: Vec<&Value> = pairs.iter().filter(|p| {
            p.get("dexId").and_then(Value::as_str) == Some("uniswap")
                && p.get("labels").and_then(Value::as_array).is_some_and(|l| l.iter().any(|x| x.as_str() == Some("v4")))
                && [side(p, "baseToken"), side(p, "quoteToken")].into_iter().flatten().collect::<HashSet<_>>() == pair
        }).collect();
        v4.sort_by(|x, y| depth(y).total_cmp(&depth(x)));
        let ids: Vec<String> = v4.iter().filter_map(|p| p.get("pairAddress").and_then(Value::as_str).map(str::to_lowercase)).collect();
        let mut keys = vec![];
        for id in ids.iter().take(5) {
            if let Some(key) = Self::v4_key(id, chain_id).await {
                if key.id().as_deref() == Some(id.as_str()) && uniswap_v4::allows_hooks(&key, chain_id) { keys.push(key) }
            }
        }
        keys
    }

    /// A v4 pool's key: the PositionManager's poolKeys(bytes25 id) on the network's own node first (every pool that took
    /// liquidity through it is there), else the PoolManager's Initialize event on the network's Blockscout (it reads the
    /// whole history; public nodes cap a log query at a range of blocks), its 429 waited out twice, 2 s apart — Blockscout
    /// turned away every lookup for minutes after a few dozen. The caller checks the key hashes back to the id. A key
    /// never changes, so each is asked once a launch.
    pub async fn v4_key(id: &str, chain_id: u64) -> Option<PoolKey> {
        if let Some(k) = known(chain_id, id) { return Some(k) }
        let v4 = uniswap_v4::on(chain_id)?;
        let raw = wallet::from_hex(id.get(2..).unwrap_or("")).filter(|r| r.len() == 32);
        if let (Some(net), Some(raw)) = (EvmNetwork::of(chain_id), raw) {
            let call = [&[0x86, 0xb6, 0xbe, 0x7d][..], &raw[..25], &[0u8; 7]].concat();
            if let Ok(answer) = net.rpc.read(v4.position_manager, &call).await {
                let read = || -> Option<PoolKey> {
                    let (c0, c1) = (address_in(&answer, 0)?, address_in(&answer, 1)?);
                    if c1 == format!("0x{}", "0".repeat(40)) { return None }
                    let fee = uint_in(&answer, 2).filter(|f| *f < 1 << 24)?;
                    let hooks = address_in(&answer, 4)?;
                    Some(PoolKey::new(&c0, &c1, fee as u32, spacing(&answer[120..128])?, &hooks))
                };
                if let Some(key) = read().filter(|k| k.id().as_deref() == Some(id.to_lowercase().as_str())) {
                    keep(chain_id, id, &key);
                    return Some(key);
                }
            }
        }
        if v4.blockscout.is_empty() { return None }
        let url = web(&format!("https://{}/api?module=logs&action=getLogs&fromBlock=0&toBlock=latest&address={}&topic0={INITIALIZE_TOPIC}&topic1={id}\
                                &topic0_1_opr=and", v4.blockscout, v4.pool_manager));
        let mut answer = None;
        for attempt in 0..3 {
            let (data, status) = fetch(&url, &holdings::BROWSER, 20).await?;
            if status != 429 {
                answer = Some(data);
                break;
            }
            if attempt < 2 { sleep(2).await }
        }
        let j: Value = serde_json::from_slice(&answer?).ok()?;
        let log = j.get("result")?.as_array()?.first()?;
        let topics: Vec<&str> = log.get("topics")?.as_array()?.iter().map(Value::as_str).collect::<Option<_>>()?;
        if topics.len() != 4 { return None }
        let words = wallet::from_hex(log.get("data")?.as_str()?.get(2..)?)?;
        if words.len() < 96 { return None }
        let topic_address = |t: &str| wallet::from_hex(t.get(2..)?).and_then(|d| address_in(&d, 0));
        let (c0, c1) = (topic_address(topics[2])?, topic_address(topics[3])?);
        let fee = uint_in(&words, 0).filter(|f| *f < 1 << 24)?;
        let hooks = address_in(&words, 2)?;
        let key = PoolKey::new(&c0, &c1, fee as u32, spacing(&words[56..64])?, &hooks);
        keep(chain_id, id, &key);
        Some(key)
    }

    /// Permit2's own allowance for the router: (amount, until).
    async fn permit2_allowed(&self, rpc: &EvmRpc, token: &str, router: &str) -> Result<(Decimal, u64)> {
        let answer = rpc.read(PERMIT2, &uniswap_v4::permit2_allowance(&self.wallet.evm, token, router)?).await?;
        Ok((word(&answer, 0).unwrap_or_default(), uint_in(&answer, 1).unwrap_or(0)))
    }

    /// The swap itself; returns its transaction. `rpc`: the network's node (Robinhood Chain's in Swift's default), or a
    /// fork in a test.
    #[allow(clippy::too_many_arguments)]
    pub async fn v4_swap(&self, route: &V4Route, token_in: &str, units: Decimal, min_out: Decimal, what: &str, note: &str, rpc: &EvmRpc) -> Result<String> {
        let Some(router) = uniswap_v4::on(rpc.chain_id).map(|d| d.router) else {
            return fail(format!("No Uniswap v4 on network {}. Nothing was signed.", rpc.chain_id));
        };
        let me = self.wallet.evm.as_str();
        let now = unix_now();
        let swap = uniswap_v4::execute(&route.key, route.zero_for_one, u256(units)?, u256(min_out)?, now + 600)?;
        // The network's own coin goes in as the call's value: no approve, nothing for Permit2 (the signer lets the coins go
        // only into this swap, exactly its input).
        if token_in.to_lowercase() == NATIVE {
            if !(units > Decimal::ZERO && units <= Decimal::from(u64::MAX)) {
                return fail(format!("At most 18 {} at once. Nothing was signed.", evm::coin(rpc.chain_id)));
            }
            return self.send_evm(rpc, router, &swap, note, &[], None, uint64(units)).await;
        }
        let permit = || uniswap_v4::permit2_approve(token_in, router, u256(units)?, now + 1800).map_err(crate::Error::from);
        // The three as one transaction where it batches: the approve to Permit2, Permit2's to the router, the swap.
        if self.can_batch(rpc) {
            let mut calls = vec![];
            let to_permit2 = word(&rpc.read(token_in, &erc20::allowance(me, PERMIT2)?).await?, 0).unwrap_or_default();
            if to_permit2 < units { calls.push(BatchCall { to: token_in.into(), data: erc20::approve(PERMIT2, u256(units)?)? }) }
            let current = self.permit2_allowed(rpc, token_in, router).await?;
            if current.0 < units || current.1 < now + 120 { calls.push(BatchCall { to: PERMIT2.into(), data: permit()? }) }
            if calls.is_empty() { return self.send_evm(rpc, router, &swap, note, &[], None, 0).await }
            calls.push(BatchCall { to: router.into(), data: swap });
            return self.send_evm(rpc, me, &[], note, &[], Some(calls), 0).await;
        }
        self.approve(rpc, token_in, PERMIT2, u256(units)?, &format!("Approve exactly {what} to Permit2"), &[]).await?;
        let current = self.permit2_allowed(rpc, token_in, router).await?;
        if current.0 < units || current.1 < now + 120 {
            self.send_evm(rpc, PERMIT2, &permit()?, &format!("Permit2: let Uniswap's router take exactly {what}, for 30 minutes"), &[], None, 0).await?;
            let mut seen = false;
            for _ in 0..15 {
                if seen { break }
                let a = self.permit2_allowed(rpc, token_in, router).await?;
                seen = a.0 >= units && a.1 >= now + 120;
                if !seen { sleep(1).await }
            }
            if !seen { return fail("Permit2's approval went through but the network doesn't show it yet. Try again in a minute; nothing was swapped.") }
        }
        self.send_evm(rpc, router, &swap, note, &[], None, 0).await
    }
}

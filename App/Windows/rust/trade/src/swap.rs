//! Buying and selling tokens on EVM networks (Swap.swift). With the money on Solana there are two ways each time, and
//! the one that leaves more is taken: Relay swaps on the way in one step, or Relay bridges USDC and the best exchange on
//! the network swaps it — Uniswap or KyberSwap (its gas, and a gas top-up when there's none, counted against it). Also
//! one token for another on a network (the best of Uniswap v2/v3/v4, Relay's swap and KyberSwap), the price-impact stop
//! on each, and the token lists by ticker.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use chrono::{SubsecRound, Utc};
use evm::abi::{self, string, uint_in};
use evm::protocols::{kyber, uniswap, v4 as uniswap_v4};
use execution::Kind;
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded, shifted, text};
use ihatecrypto_core::holdings;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::modules::{ModuleNetwork, ModuleProtocol};
use ihatecrypto_core::network::Amount;
use ihatecrypto_core::stablecoins;
use ihatecrypto_core::tokens::TokenLookup;
use net::evm_rpc::EvmRpc;
use protocols::relay;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use tokio::task::JoinSet;

use crate::aave::EvmNetwork;
use crate::executor::{uint64, url_error};
use crate::stocks::RobinhoodStocks;
use crate::v4::V4Route;
use crate::{CostlyMove, Error, Executor, Result, fail, impact_accepted, loss_accepted, modules, num, stop, u256, word};

// MARK: Shared by the swap modules

pub(crate) fn d(s: &str) -> Decimal { s.parse().expect("a decimal literal") }
pub(crate) fn sleep(secs: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_secs(secs)) }
pub(crate) fn unix_now() -> u64 { Utc::now().timestamp().max(0) as u64 }

/// `.whole`: rounded down to a whole number.
pub(crate) fn whole(x: Decimal) -> Decimal { x.floor() }

/// `Decimal(sign: .plus, exponent: decimals, significand: 1)`; past Decimal's 28 digits it's out of range.
pub(crate) fn scale(decimals: u32) -> Num { Num((decimals <= 28).then(|| Decimal::from_i128_with_scale(10i128.pow(decimals), 0))) }

/// `raw / scale`: smallest units as whole tokens; never fails (a token past 60 places reads as 0).
pub(crate) fn tokens(raw: Decimal, decimals: u32) -> Decimal { shifted(raw, decimals as i64).unwrap_or_default() }

/// `NSDecimalNumber((x * 1_000_000).rounded(0, .down)).uint64Value`: dollars as millionths.
pub(crate) fn millionths(x: Decimal) -> Result<u64> { Ok(uint64(num((Num::of(x) * 1_000_000).rounded(0, Rounding::Down))?)) }

/// Dollars from millionths.
pub(crate) fn dollars(micro: u64) -> Decimal { Decimal::from(micro) / Decimal::from(1_000_000) }

/// Wei as a Decimal (Swift's `Decimal(UInt128)`); out of range past Decimal's 28 digits.
pub(crate) fn wei(x: u128) -> Result<Decimal> { num(Num(Decimal::try_from(x).ok())) }

/// `net.raw(micro)` as a Decimal of the dollar's own units.
pub(crate) fn raw(net: &EvmNetwork, micro: u64) -> Decimal { crate::decimal(net.raw(micro)).unwrap_or_default() }

/// `net.micro(raw)` of a Decimal amount in the dollar's own units: its millionths, rounded down.
pub(crate) fn micro(net: &EvmNetwork, raw: Decimal) -> u64 { u256(raw.max(Decimal::ZERO).floor()).map_or(0, |u| net.micro(u)) }

/// `Decimal(string:) ?? 0`.
pub(crate) fn dec(s: &str) -> Decimal { decimal(s).unwrap_or_default() }

/// A ticker as people write it: upper case, without «$».
pub(crate) fn plain(s: &str) -> String { s.to_uppercase().trim_matches('$').to_string() }

pub(crate) fn pairs(p: &[(&str, &str)]) -> HashMap<String, String> { p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }

/// How much worse `out` for `amount` is than `small_out` for `small` on the same route, 0…1.
pub(crate) fn impact(out: Decimal, amount: Decimal, small_out: Decimal, small: Decimal) -> Option<Decimal> {
    (Num::of(Decimal::ONE) - Num::of(out) / amount / (Num::of(small_out) / small)).0.map(|x| x.max(Decimal::ZERO))
}

/// An EVM step that is exactly «approve `units` of `token` to `spender`» (both lowercase).
pub(crate) fn approves(step: &relay::EvmStep, token: &str, spender: &str, units: Decimal) -> bool {
    let args = step.data.get(4..).unwrap_or(&[]);
    step.to.to_lowercase() == token && step.data.starts_with(&[0x09, 0x5e, 0xa7, 0xb3])
        && abi::address_in(args, 0).map(|a| a.to_lowercase()).as_deref() == Some(spender) && word(args, 1) == Some(units)
}

/// Relay's call carries no coins.
pub(crate) fn no_value(step: &relay::EvmStep) -> bool { step.value == "0" || step.value == "<null>" }

pub(crate) const DECIMALS: [u8; 4] = [0x31, 0x3c, 0xe5, 0x67];
pub(crate) const SYMBOL: [u8; 4] = [0x95, 0xd8, 0x9b, 0x41];
const DEPOSIT: [u8; 4] = [0xd0, 0xe3, 0x0d, 0xb0];
const WITHDRAW: [u8; 4] = [0x2e, 0x1a, 0x7d, 0x4d];
const SWAPS_ON: &str = "ihatecrypto swaps on Solana, Base, Arbitrum, Polygon, Ethereum, Optimism, BNB Chain and HyperEVM.";

static STAND_IN: RwLock<Option<String>> = RwLock::new(None);

/// A web API's address (DexScreener, Blockscout, Robinhood, Uniswap's list, KyberSwap), or the same path and query on
/// the tests' stand-in.
pub(crate) fn web(url: &str) -> String {
    match STAND_IN.read().unwrap_or_else(|e| e.into_inner()).as_deref() {
        Some(root) => format!("{root}/{}", url.splitn(4, '/').nth(3).unwrap_or("")),
        None => url.into(),
    }
}

#[cfg(test)]
pub(crate) fn set_web(root: Option<String>) { *STAND_IN.write().unwrap_or_else(|e| e.into_inner()) = root }

/// A GET with these headers, given up after `secs`; None when nothing answered (`try? await URLSession…`).
pub(crate) async fn fetch(url: &str, headers: &[(&str, &str)], secs: u64) -> Option<(Vec<u8>, u16)> {
    let mut r = net::HTTP.get(url).timeout(Duration::from_secs(secs));
    for (k, v) in headers { r = r.header(*k, *v) }
    let response = r.send().await.ok()?;
    let status = response.status().as_u16();
    Some((response.bytes().await.ok()?.to_vec(), status))
}

// MARK: Tokens by ticker

/// A token on an EVM network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub symbol: String,
    pub name: String,
    pub address: String,
    pub decimals: u32,
}

impl Token {
    pub fn new(symbol: &str, name: &str, address: &str, decimals: u32) -> Self {
        Self { symbol: symbol.into(), name: name.into(), address: address.into(), decimals }
    }
}

/// Tokens on Base and Arbitrum by ticker: Uniswap's default token list (tokens.uniswap.org), the curated list its own
/// app offers. A ticker outside it is bought by its contract.
pub struct EvmTokens;

impl EvmTokens {
    /// HyperEVM's own: Uniswap's list has nothing for chain 999. Unit's bridged coins, Kinetiq's kHYPE, USD₮0 and WHYPE,
    /// each address, symbol and decimals read on-chain 2026-10-07.
    pub const HYPEREVM: [(&str, &str, &str, u32); 6] = [
        ("UBTC", "Unit Bitcoin", "0x9fdbda0a5e284c32744d2f17ee5c74b284993463", 8), ("UETH", "Unit Ethereum", "0xbe6727b535545c67d5caa73dea54865b92cf7907", 18),
        ("USOL", "Unit Solana", "0x068f321fa8fb9f0d135f290ef6a3e2813e1c8a29", 9), ("kHYPE", "Kinetiq Staked HYPE", "0xfd739d4e423301ce9385c1fb8850539d657c296d", 18),
        ("USDT0", "USD₮0", "0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb", 6), ("WHYPE", "Wrapped HYPE", "0x5555555555555555555555555555555555555555", 18),
    ];

    /// BNB Chain's own beside Uniswap's list, which has neither CAKE nor BTCB nor Binance-Peg ETH: read on-chain 2026-10-08.
    pub const BNB: [(&str, &str, &str, u32); 3] = [
        ("CAKE", "PancakeSwap Token", "0x0e09fabb73bd3ade0a17ecc321fd13a19e81ce82", 18), ("BTCB", "Binance-Peg BTCB", "0x7130d2a12b9bcbfae4f2634d864a1ee1ce3ead9c", 18),
        ("ETH", "Binance-Peg Ethereum", "0x2170ed0880ac9a755fd29b2688956bd959f933f8", 18),
    ];

    fn listed(table: &[(&str, &str, &str, u32)], want: &str) -> Option<Token> {
        table.iter().find(|t| t.0.to_uppercase() == want).map(|t| Token::new(t.0, t.1, t.2, t.3))
    }

    pub async fn find(symbol: &str, chain_id: u64) -> Result<Option<Token>> {
        let want = plain(symbol);
        // The big five stablecoins by ihatecrypto's own table: Uniswap's list misses USDe and USDS on most networks.
        if let Some(c) = EvmNetwork::of(chain_id).and_then(|n| stablecoins::find(&want, n.key)) {
            return Ok(Some(Token::new(c.symbol, c.symbol, c.address, c.decimals)));
        }
        if chain_id == 999 { return Ok(Self::listed(&Self::HYPEREVM, &want)) }
        if chain_id == 56 {
            if let Some(own) = Self::listed(&Self::BNB, &want) { return Ok(Some(own)) }
        }
        let (data, _) = net::get(&web("https://tokens.uniswap.org"), &[]).await.map_err(url_error)?;
        Ok(Self::find_in(&data, symbol, chain_id))
    }

    /// The list's token with that ticker on the chain (Swift's `find(_ data:symbol:chainId:)`).
    pub fn find_in(data: &[u8], symbol: &str, chain_id: u64) -> Option<Token> {
        let list: Value = serde_json::from_slice(data).ok()?;
        let want = plain(symbol);
        let t = list.get("tokens")?.as_array()?.iter().find(|t| {
            t.get("chainId").and_then(Value::as_u64) == Some(chain_id) && t.get("symbol").and_then(Value::as_str).map(str::to_uppercase) == Some(want.clone())
        })?;
        let address = t.get("address")?.as_str()?;
        let decimals = u32::try_from(t.get("decimals")?.as_u64()?).ok()?;
        let field = |k: &str| t.get(k).and_then(Value::as_str).unwrap_or(symbol).to_string();
        Some(Token { symbol: field("symbol"), name: field("name"), address: address.to_lowercase(), decimals })
    }
}

// MARK: Ways and routes

/// One way to do the trade: what comes out in the target's smallest units, and what it's worth after its gas, in USD.
#[derive(Debug, Clone, PartialEq)]
pub struct SwapWay {
    /// "relay" (Relay's swap) or "bridge" (Relay's bridge and the exchange `venue` on the network).
    pub name: String,
    pub out: Decimal,
    pub worth: Decimal,
    pub line: String,
    pub venue: String,
    /// Relay's swap of a token: Relay's dollar valuation of what goes in, as quoted ("" when it gave none).
    pub in_usd: String,
}

impl SwapWay {
    pub fn new(name: &str, out: Decimal, worth: Decimal, line: &str) -> Self {
        Self { name: name.into(), out, worth, line: line.into(), venue: String::new(), in_usd: String::new() }
    }
}

/// Uniswap v3 on a network (`UniswapV3.Deployment`, evm's `uniswap::v3`): its QuoterV2, SwapRouter02 and the WETH it
/// routes through.
pub type UniswapV3 = (&'static str, &'static str, &'static str);

/// Uniswap v3's pool fee tiers, the most common first (`UniswapV3.fees`).
pub const V3_FEES: [u64; 4] = [500, 3000, 10000, 100];

/// Uniswap on an EVM network for one trade: v3's best path, v4's pool without hooks or v2's path, whichever gives most.
#[derive(Debug, Clone, PartialEq)]
pub struct UniRoute {
    /// v3's path and how many pools it crosses.
    pub v3: Option<(Vec<u8>, usize)>,
    pub v4: Option<V4Route>,
    /// v2's path: the pair's pool, or two through WETH.
    pub v2: Option<Vec<String>>,
    pub out: Decimal,
    /// How much worse the trade's price is than a twentieth of it on the same route, 0…1; None when it couldn't be read.
    pub impact: Option<Decimal>,
}

impl UniRoute {
    pub fn of_v3(path: Vec<u8>, hops: usize, out: Decimal) -> Self { Self { v3: Some((path, hops)), v4: None, v2: None, out, impact: None } }
    pub fn of_v4(route: V4Route) -> Self { Self { out: route.out, v3: None, v4: Some(route), v2: None, impact: None } }
    pub fn of_v2(path: Vec<String>, out: Decimal) -> Self { Self { v3: None, v4: None, v2: Some(path), out, impact: None } }

    pub fn title(&self) -> &'static str {
        if self.v4.is_some() { "Uniswap v4" } else if self.v2.is_some() { "Uniswap v2" } else { "Uniswap v3" }
    }

    /// How far below the quote the swap may still go through: 0.5% in a deep pool (impact under 0.5%), 1% under 2%,
    /// past that half the impact plus 1%, at most 3%; 1% when the impact is unknown. A tighter floor leaves less to take
    /// for anyone moving the price around the swap; a thin pool moves more on its own between the quote and the block.
    pub fn tolerance(&self) -> Decimal { Self::tolerance_of(self.impact) }

    pub fn tolerance_of(impact: Option<Decimal>) -> Decimal {
        let Some(impact) = impact else { return d("0.01") };
        if impact < d("0.005") { return d("0.005") }
        if impact < d("0.02") { return d("0.01") }
        (Num::of(impact) / 2 + d("0.01")).0.unwrap_or(Decimal::MAX).min(d("0.03"))
    }

    /// « · price impact 1.2%» from a tenth of a percent up, for the card and the report.
    pub fn impact_text(&self) -> String { Self::impact_text_of(self.impact) }

    pub fn impact_text_of(impact: Option<Decimal>) -> String {
        let Some(impact) = impact.filter(|i| *i >= d("0.001")) else { return String::new() };
        let percent = (Num::of(impact) * 100).rounded(if impact < d("0.1") { 2 } else { 1 }, Rounding::Plain).0;
        format!(" · price impact {}%", percent.map_or("NaN".into(), |p| Fmt::num(p, 2)))
    }

    /// Gas the swap takes with its approves: v4's goes through Permit2 (an approve to it, Permit2's own, the swap).
    pub fn gas(&self) -> u64 {
        if self.v4.is_some() { return 50_000 + 60_000 + 180_000 }
        if let Some(v2) = &self.v2 { return 50_000 + if v2.len() == 2 { 130_000 } else { 200_000 } }
        50_000 + if self.v3.as_ref().map_or(1, |v| v.1) == 1 { 160_000 } else { 240_000 }
    }
}

/// One token for another on a network, quoted: both sides (the network's own coin as its wrapped token), the amount in,
/// and the way that leaves most (best_pair) — Uniswap's best for the pair (v2, v3, v4, or a v4 pool of the coin itself),
/// Relay's swap or KyberSwap's route.
#[derive(Debug, Clone, PartialEq)]
pub struct PairSwap {
    pub token_in: Token,
    pub token_out: Token,
    /// The network's own coin (ETH; POL on Polygon), not its wrapped token.
    pub coin_in: bool,
    pub coin_out: bool,
    pub units: Decimal,
    /// None for the coin into its own wrapped token or back: a wrap or an unwrap, no swap — or for Relay's swap.
    pub route: Option<UniRoute>,
    /// What Relay's swap on the network gives, in the output's smallest units, when it leaves the most (HyperEVM's money
    /// sits on other exchanges, which Relay reaches).
    pub relay_out: Option<Decimal>,
    /// KyberSwap's route, when it leaves the most.
    pub kyber: Option<KyberRoute>,
    /// Relay's or KyberSwap's price impact by its own valuation, else Uniswap's measured one for the same pair: a token
    /// they don't price must not pass the impact stop as 0% (the 2026-10-28 review).
    pub way_impact: Option<Decimal>,
}

impl PairSwap {
    pub fn new(token_in: Token, token_out: Token, coin_in: bool, coin_out: bool, units: Decimal, route: Option<UniRoute>) -> Self {
        Self { token_in, token_out, coin_in, coin_out, units, route, relay_out: None, kyber: None, way_impact: None }
    }

    /// The route is a v4 pool of the coin itself: it goes in as the call's value or comes out as coins, unwrapped.
    pub fn direct(&self) -> bool { self.route.as_ref().and_then(|r| r.v4.as_ref()).is_some_and(|v| v.key.currency0 == uniswap_v4::NATIVE) }

    /// What it gives, in the output's smallest units (a wrap or an unwrap: the amount itself).
    pub fn out(&self) -> Decimal {
        self.route.as_ref().map(|r| r.out).or(self.kyber.as_ref().map(|k| k.out)).or(self.relay_out).unwrap_or(self.units)
    }

    pub fn title(&self) -> String {
        match (&self.route, &self.kyber) {
            (Some(r), _) => r.title().into(),
            (None, Some(_)) => "KyberSwap".into(),
            _ if self.relay_out.is_some() => "Relay's swap".into(),
            _ => if self.coin_in { "wrap" } else { "unwrap" }.into(),
        }
    }

    /// How far it moves the price: Uniswap's measured route, else the other way's (way_impact).
    pub fn impact(&self) -> Option<Decimal> { self.route.as_ref().and_then(|r| r.impact).or(self.way_impact) }

    pub fn impact_text(&self) -> String { UniRoute::impact_text_of(self.impact()) }

    /// Gas it takes with its exact approve: Uniswap's route, KyberSwap's estimate, Relay's swap ≈ 300k, a wrap ≈ 50k.
    pub fn gas(&self) -> u64 {
        self.route.as_ref().map_or_else(|| 50_000u64.saturating_add(self.kyber.as_ref().map_or(if self.relay_out.is_some() { 300_000 } else { 0 }, |k| k.gas)),
                                        UniRoute::gas)
    }
}

/// A buy's second way: the USDC that arrives over Relay's bridge, whether gas rides along, and the exchange there.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgedBuy {
    pub arrive: u64,
    pub topup: bool,
    pub pair: PairSwap,
}

/// Relay's swap on one network, checked (relay_pair_quote).
#[derive(Debug, Clone, PartialEq)]
pub struct RelayPair {
    pub quote: relay::Quote,
    pub call: relay::EvmStep,
    pub proxy: String,
    pub out: Decimal,
}

/// Which ways Settings › Modules allows on a network.
#[derive(Debug, Clone, Copy)]
struct Allowed {
    relay: bool,
    uniswap: bool,
    kyber: bool,
}

// MARK: KyberSwap's API (KyberSwap.swift's route and build)

/// A route KyberSwap's API found: what comes out, both sides and the gas in its USD, and its summary as given (the
/// build takes it back unchanged).
#[derive(Debug, Clone, PartialEq)]
pub struct KyberRoute {
    pub out: Decimal,
    pub in_usd: Decimal,
    pub out_usd: Decimal,
    pub gas_usd: Decimal,
    pub gas: u64,
    pub(crate) summary: Value,
}

impl KyberRoute {
    /// Its price impact by its own valuation, 0…1: what the trade loses against what goes in; None when it prices either
    /// side at nothing (a token it doesn't know), rather than a made-up 0% or 100%.
    pub fn impact(&self) -> Option<Decimal> {
        if self.in_usd > Decimal::ZERO && self.out_usd > Decimal::ZERO { (Num::of(Decimal::ONE) - Num::of(self.out_usd) / self.in_usd).0.map(|x| x.max(Decimal::ZERO)) } else { None }
    }
}

/// A JSON string or number as a Decimal (`KyberSwap.number`).
fn number(v: Option<&Value>) -> Option<Decimal> {
    match v? { Value::String(s) => decimal(s), Value::Number(n) => decimal(&n.to_string()), _ => None }
}

async fn kyber_post(url: &str, body: &Value) -> std::result::Result<net::Answer, reqwest::Error> {
    let r = net::HTTP.post(url).header("x-client-id", "ihatecrypto").header("Content-Type", "application/json").body(body.to_string())
        .timeout(Duration::from_secs(20)).send().await?;
    let status = r.status().as_u16();
    Ok((r.bytes().await?.to_vec(), status))
}

fn kyber_refused(data: &[u8], status: u16) -> net::Again {
    net::Again::new(status == 429 || status >= 500, format!("KyberSwap: {}", net::said(data, status)))
}

/// KyberSwap's `data` when the answer is a 200 from its own router.
fn kyber_data(data: &[u8], status: u16) -> Option<Value> {
    let j: Value = serde_json::from_slice(data).ok()?;
    let d = j.get("data").filter(|d| d.is_object())?;
    (status == 200 && d.get("routerAddress").and_then(Value::as_str).map(str::to_lowercase).as_deref() == Some(kyber::ROUTER)).then(|| d.clone())
}

/// The route for `amount` (smallest units) of `from` into `to` on the network.
pub async fn kyber_route(chain_id: u64, from: &str, to: &str, amount: Decimal) -> Result<KyberRoute> {
    let Some(chain) = kyber::chain(chain_id) else { return Err(Error::Wallet(format!("KyberSwap doesn't serve network {chain_id}."))) };
    let url = web(&format!("{}/{chain}/api/v1/routes?tokenIn={from}&tokenOut={to}&amountIn={}", kyber::API, text(amount)));
    Ok(net::run(&net::DELAYS, |_| net::get(&url, &[("x-client-id", "ihatecrypto")]), |data, status| {
        let route = kyber_data(data, status).and_then(|d| {
            let s = d.get("routeSummary").filter(|s| s.is_object())?;
            let out = number(s.get("amountOut")).filter(|o| *o > Decimal::ZERO)?;
            Some(KyberRoute {
                out, in_usd: number(s.get("amountInUsd")).unwrap_or_default(), out_usd: number(s.get("amountOutUsd")).unwrap_or_default(),
                gas_usd: number(s.get("gasUsd")).unwrap_or_default(), gas: number(s.get("gas")).map_or(0, uint64), summary: s.clone(),
            })
        });
        route.ok_or_else(|| kyber_refused(data, status))
    }).await?)
}

/// The router call for `route`, paying `sender`, at most `slippage_bps` under its quote. Read back by the caller
/// (`kyber::decode`) before anything is signed.
pub async fn kyber_build(route: &KyberRoute, chain_id: u64, sender: &str, slippage_bps: i64) -> Result<Vec<u8>> {
    let Some(chain) = kyber::chain(chain_id) else { return Err(Error::Wallet(format!("KyberSwap doesn't serve network {chain_id}."))) };
    let url = web(&format!("{}/{chain}/api/v1/route/build", kyber::API));
    let body = json!({"routeSummary": route.summary, "sender": sender, "recipient": sender, "slippageTolerance": slippage_bps, "source": "ihatecrypto"});
    // Nothing is signed yet, so this is asked again like the route.
    Ok(net::run(&net::DELAYS, |_| kyber_post(&url, &body), |data, status| {
        kyber_data(data, status).and_then(|d| d.get("data").and_then(Value::as_str).and_then(wallet::from_hex)).ok_or_else(|| kyber_refused(data, status))
    }).await?)
}

// MARK: The executor's swaps

impl Executor {
    /// A ticker from Uniswap's list, or a contract that trades on the chain with $50k+ of liquidity; decimals are read
    /// from the contract itself.
    pub async fn evm_token(&self, r: &str, net: &EvmNetwork) -> Result<Token> {
        let rpc = &net.rpc;
        if !r.starts_with("0x") {
            let want = plain(r);
            // Robinhood Chain's dollar and its Stock Tokens by Robinhood's own registry.
            if rpc.chain_id == 4663 {
                if want == "USDG" { return Ok(Token::new("USDG", "Global Dollar", evm::ROBINHOOD_USDG, 6)) }
                if let Ok(s) = RobinhoodStocks::verified(&want, rpc).await { return Ok(Token::new(&s.symbol, &s.name, &s.address.to_lowercase(), s.decimals)) }
            }
            let Some(t) = EvmTokens::find(r, rpc.chain_id).await? else {
                // Not on a list, but in the wallet there under that ticker: its contract, with the contract's own checks.
                let held = holdings::evm(net.key, &self.wallet.evm, false, &HashSet::new(), &[]).await
                    .and_then(|h| h.into_iter().find(|h| h.symbol.to_uppercase() == want && h.contract.starts_with("0x")));
                if let Some(held) = held { return Box::pin(self.evm_token(&held.contract, net)).await }
                let list = if rpc.chain_id == 999 { "ihatecrypto's HyperEVM list (UBTC, UETH, USOL, kHYPE, USDT0)" } else { "Uniswap's token list" };
                return fail(format!("{} isn't on {list} for {}. To buy it anyway, name it by its contract: “buy token 0x… on {} for 5 USDC”.",
                                    r.to_uppercase(), net.name, net.key));
            };
            if uint_in(&rpc.read(&t.address, &DECIMALS).await?, 0) != Some(t.decimals as u64) {
                return fail(format!("The {} contract on {} doesn't match Uniswap's list. Nothing was signed.", t.symbol, net.name));
            }
            return Ok(t);
        }
        let Some(info) = TokenLookup::lookup(r).await?.into_iter().find(|i| i.chain == net.key) else {
            return fail(format!("This contract doesn't trade on {}.", net.name));
        };
        if !info.liquidity_usd.is_some_and(|l| l >= Decimal::from(50_000)) {
            return fail(format!("{} has only {} of liquidity on {}: too thin to trade safely.", info.symbol, Fmt::usd(info.liquidity_usd.unwrap_or_default()), net.name));
        }
        let Some(decimals) = uint_in(&rpc.read(r, &DECIMALS).await?, 0).filter(|d| *d <= 36) else {
            return fail("The contract doesn't answer decimals(): not a token ihatecrypto can trade.");
        };
        let symbol = string(&rpc.read(r, &SYMBOL).await?).unwrap_or(info.symbol);
        Ok(Token { symbol, name: info.name, address: r.to_lowercase(), decimals: decimals as u32 })
    }

    /// The Uniswap v3 route with the most output: one pool, or two through WETH. (path, out, pools crossed)
    pub async fn best_path(&self, rpc: &EvmRpc, uni: UniswapV3, a: &str, b: &str, amount_in: Decimal) -> Option<(Vec<u8>, Decimal, usize)> {
        let amount = u256(amount_in).ok()?;
        let mut routes: Vec<(Vec<String>, Vec<u64>)> = V3_FEES.iter().map(|f| (vec![a.to_string(), b.to_string()], vec![*f])).collect();
        if a.to_lowercase() != uni.2 && b.to_lowercase() != uni.2 {
            for f1 in [500, 3000, 10000] {
                for f2 in [500, 3000, 10000] { routes.push((vec![a.into(), uni.2.into(), b.into()], vec![f1, f2])) }
            }
        }
        let mut group = JoinSet::new();
        for (tokens, fees) in routes {
            let rpc = rpc.clone();
            group.spawn(async move {
                let path = uniswap::path(&tokens.iter().map(String::as_str).collect::<Vec<_>>(), &fees).ok()?;
                let answer = rpc.read(uni.0, &uniswap::quote_exact_input(&path, amount)).await.ok()?;
                let out = word(&answer, 0).filter(|o| *o > Decimal::ZERO)?;
                Some((path, out, fees.len()))
            });
        }
        let mut best: Option<(Vec<u8>, Decimal, usize)> = None;
        while let Some(r) = group.join_next().await {
            if let Ok(Some(r)) = r {
                if r.1 > best.as_ref().map_or(Decimal::ZERO, |b| b.1) { best = Some(r) }
            }
        }
        best
    }

    /// Uniswap v2's best path: its pool of the pair, or two through WETH, quoted by the router (before any transfer tax).
    pub async fn best_v2(&self, rpc: &EvmRpc, a: &str, b: &str, amount_in: Decimal) -> Option<(Vec<String>, Decimal)> {
        let (router, weth) = uniswap::v2(rpc.chain_id)?;
        let amount = u256(amount_in).ok()?;
        let mut paths = vec![vec![a.to_string(), b.to_string()]];
        if a.to_lowercase() != weth && b.to_lowercase() != weth { paths.push(vec![a.into(), weth.into(), b.into()]) }
        let mut group = JoinSet::new();
        for path in paths {
            let rpc = rpc.clone();
            group.spawn(async move {
                let call = uniswap::get_amounts_out(amount, &path.iter().map(String::as_str).collect::<Vec<_>>()).ok()?;
                let answer = rpc.read(router, &call).await.ok()?;
                let out = uniswap::amount_out(&answer).and_then(crate::decimal).filter(|o| *o > Decimal::ZERO)?;
                Some((path, out))
            });
        }
        let mut best: Option<(Vec<String>, Decimal)> = None;
        while let Some(r) = group.join_next().await {
            if let Ok(Some(r)) = r {
                if r.1 > best.as_ref().map_or(Decimal::ZERO, |b| b.1) { best = Some(r) }
            }
        }
        best
    }

    /// The best of Uniswap's v3 path (one pool or two through WETH), its v4 pool without hooks (UniswapV4.on) and its v2
    /// path (UniswapV2.on), for `amount_in` of `a` into `b` on the network: all quoted at once, the one that gives most taken.
    pub async fn best_uniswap(&self, net: &EvmNetwork, uni: UniswapV3, a: &str, b: &str, amount_in: Decimal) -> Option<UniRoute> {
        let rpc = &net.rpc;
        let v4 = async { if uniswap_v4::on(rpc.chain_id).is_none() { None } else { self.v4_route(a, b, amount_in, rpc).await } };
        let (p3, p4, p2) = tokio::join!(self.best_path(rpc, uni, a, b, amount_in), v4, self.best_v2(rpc, a, b, amount_in));
        let top = [p3.as_ref().map(|p| p.1), p4.as_ref().map(|p| p.out), p2.as_ref().map(|p| p.1)].into_iter().flatten().fold(Decimal::ZERO, Decimal::max);
        if top <= Decimal::ZERO { return None }
        let mut route = if let Some(p4) = p4.filter(|p| p.out == top) { UniRoute::of_v4(p4) }
            else if let Some(p3) = p3.filter(|p| p.1 == top) { UniRoute::of_v3(p3.0, p3.2, p3.1) }
            else if let Some(p2) = p2 { UniRoute::of_v2(p2.0, p2.1) }
            else { return None };
        // Price impact: the trade's own price against a twentieth of it on the same route.
        let small = whole(amount_in / Decimal::from(20));
        if small > Decimal::ZERO {
            let small_out = if let Some(v4) = &route.v4 {
                self.v4_quote(&v4.key, v4.zero_for_one, small, rpc).await
            } else if let Some(v3) = &route.v3 {
                match u256(small) {
                    Ok(s) => rpc.read(uni.0, &uniswap::quote_exact_input(&v3.0, s)).await.ok().and_then(|a| word(&a, 0)),
                    Err(_) => None,
                }
            } else if let (Some(path), Some((router, _)), Ok(s)) = (&route.v2, uniswap::v2(rpc.chain_id), u256(small)) {
                match uniswap::get_amounts_out(s, &path.iter().map(String::as_str).collect::<Vec<_>>()) {
                    Ok(call) => rpc.read(router, &call).await.ok().and_then(|a| uniswap::amount_out(&a)).and_then(crate::decimal),
                    Err(_) => None,
                }
            } else { None };
            if let Some(small_out) = small_out.filter(|s| *s > Decimal::ZERO) { route.impact = impact(route.out, amount_in, small_out, small) }
        }
        Some(route)
    }

    /// The swap by `route` for `units` of `token_in`, for at least what it quoted less the route's tolerance (UniRoute:
    /// 0.5% to 3% by its price impact), paying only this wallet: v3's exactInput or v2's router with its exact approve, or
    /// v4's swap through Permit2 (v4_swap); batched where the wallet can (EIP-7702). `rpc`: the network's node, or a fork.
    #[allow(clippy::too_many_arguments)]
    pub async fn swap_uniswap(&self, route: &UniRoute, net: &EvmNetwork, uni: UniswapV3, token_in: &str, units: Decimal, what: &str, note: &str,
                              rpc: Option<&EvmRpc>) -> Result<String> {
        let rpc = rpc.unwrap_or(&net.rpc);
        let min_out = whole(num(Num::of(route.out) * (Decimal::ONE - route.tolerance()))?);
        if let Some(v4) = &route.v4 {
            return self.v4_swap(v4, token_in, units, min_out, what, &format!("{note} on Uniswap v4"), rpc).await;
        }
        let me = self.wallet.evm.as_str();
        if let Some(path) = &route.v2 {
            let Some((router, _)) = uniswap::v2(rpc.chain_id) else { return fail(format!("No Uniswap v2 on {}. Nothing was signed.", net.name)) };
            let swap = uniswap::swap_exact_tokens(u256(units)?, u256(min_out)?, &path.iter().map(String::as_str).collect::<Vec<_>>(), me, unix_now() + 600, false)?;
            return self.approve_and_send(rpc, token_in, router, u256(units)?, &format!("Approve exactly {what} to Uniswap v2"), router, &swap,
                                         &format!("{note} on Uniswap v2"), &[], 0).await;
        }
        let Some(v3) = &route.v3 else { return fail("Uniswap's route went away. Nothing was signed.") };
        let swap = uniswap::exact_input(&v3.0, me, u256(units)?, u256(min_out)?)?;
        self.approve_and_send(rpc, token_in, uni.1, u256(units)?, &format!("Approve exactly {what} to Uniswap"), uni.1, &swap,
                              &format!("{note} on Uniswap"), &[], 0).await
    }

    /// What `gas` units cost on the chain now, in USD (ETH priced through Uniswap's WETH/USDC pool).
    pub async fn gas_usd(&self, gas: u64, net: &EvmNetwork, uni: UniswapV3) -> Result<Decimal> {
        let price = net.rpc.quantity("eth_gasPrice", json!([])).await?;
        // 0.001 ETH → USDC, times a thousand: the price of one ETH.
        let milli = Decimal::from(1_000_000_000_000_000u64);
        let usdc = self.best_path(&net.rpc, uni, uni.2, net.usdc, milli).await.map_or(Decimal::ZERO, |p| p.1);
        let eth_usd = dollars(micro(net, usdc)) * Decimal::from(1000);
        num(Num::of(Decimal::from(gas)) * Decimal::from(price) / Decimal::from(1_000_000_000_000_000_000u64) * eth_usd)
    }

    /// Which ways Settings › Modules allows on the network: Relay (its swap, and its bridge to and from Solana), Uniswap,
    /// and KyberSwap where it serves. A way across from Solana through Uniswap needs both Relay and Uniswap.
    fn allowed_ways(&self, net: &EvmNetwork) -> Allowed {
        let place: Vec<ModuleNetwork> = ModuleNetwork::from_place(net.key).into_iter().collect();
        let m = modules();
        Allowed {
            relay: m.refusal(ModuleProtocol::Relay, &place, false).is_none(),
            uniswap: m.refusal(ModuleProtocol::Uniswap, &place, false).is_none(),
            kyber: kyber::chain(net.rpc.chain_id).is_some() && m.refusal(ModuleProtocol::Kyberswap, &place, false).is_none(),
        }
    }

    /// The network's own USDC as a token, for a swap into it or out of it there.
    pub fn usdc_token(net: &EvmNetwork) -> Token { Token::new("USDC", "USD Coin", &net.usdc.to_lowercase(), net.usdc_decimals) }

    /// The way a trade takes: the one that leaves more, or only the one the person picked on the route's step
    /// ("relay" = Relay's swap, "bridge" = Relay's bridge + the best exchange there). A pick only narrows the two quoted ways.
    pub fn pick(ways: &[SwapWay], prefer: Option<&str>) -> Option<SwapWay> {
        // The first of the best, as Swift's `max(by:)` keeps it.
        ways.iter().filter(|w| prefer.is_none_or(|p| w.name == p))
            .fold(None::<&SwapWay>, |best, w| match best { Some(b) if b.worth >= w.worth => Some(b), _ => Some(w) }).cloned()
    }

    /// What the refusal says when the picked way has no quote: the other one is still there to choose.
    fn unquoted(prefer: Option<&str>, what: &str) -> Option<String> {
        prefer.map(|p| format!("{} doesn't quote {what} right now. Nothing was signed; pick «The best one» on the route's step to take the other way.",
                               if p == "relay" { "Relay's swap" } else { "Relay's bridge + an exchange" }))
    }

    /// Both ways of a buy (`usdc`) or a sale (`sell`, None = all) quoted now for the route's card, the one that leaves
    /// more first; nothing is signed. `gets` is what arrives («2.41 AERO», «1.97 USDC»), `worth` its USD after gas.
    /// "here:<the way's name>" («here:KyberSwap») is the one way when the trade runs on the network's own USDC (buy_here,
    /// or a sale that keeps the USDC): the best of Uniswap, Relay's swap and KyberSwap there (best_pair). (way, gets, worth)
    pub async fn swap_quotes(&self, net: &EvmNetwork, r: &str, usdc: Option<Decimal>, sell: Option<Decimal>, from: Option<&str>, keep: bool)
        -> Result<Vec<(String, String, Decimal)>> {
        let Some(uni) = uniswap::v3(net.rpc.chain_id) else { return Ok(vec![]) };
        let token = self.evm_token(r, net).await?;
        let shown = |raw: Decimal| format!("{} {}", Fmt::num(tokens(raw, token.decimals), 6), token.symbol);
        if let Some(usdc) = usdc {
            let units = millionths(usdc)?;
            if let Some((pair, _)) = self.buy_here(net, &token, units, from, uni, true).await? {
                return Ok(vec![(format!("here:{}", pair.title()), format!("{}{}", shown(pair.out()), pair.impact_text()), usdc)]);
            }
            let mut ways = self.buy_ways(net, &token, units, uni).await?.0;
            ways.sort_by(|a, b| b.worth.cmp(&a.worth));
            return Ok(ways.iter().map(|w| (Self::way_key(w), shown(w.out), w.worth)).collect());
        }
        let held = self.balance(&token.address, &net.rpc).await?;
        let units = whole(match sell { Some(s) => num(Num::of(s) * scale(token.decimals))?.min(held), None => held });
        if units <= Decimal::ZERO { return Ok(vec![]) }
        if keep {
            let best = self.best_pair(net, uni, &token, &Self::usdc_token(net), false, false, units, self.allowed_ways(net).uniswap, None, true).await?;
            let Some(best) = best else { return Ok(vec![]) };
            let got = dollars(micro(net, best.out()));
            return Ok(vec![(format!("here:{}", best.title()), format!("{} USDC{}", Fmt::num(got, 2), best.impact_text()), got)]);
        }
        let mut ways = self.sell_ways(net, &token, units, uni).await?.0;
        ways.sort_by(|a, b| b.worth.cmp(&a.worth));
        Ok(ways.iter().map(|w| (Self::way_key(w), format!("{} USDC", Fmt::num(w.out / Decimal::from(1_000_000), 2)), w.worth)).collect())
    }

    /// A way as the card names it: "relay", or "bridge:<the exchange>" («bridge:KyberSwap»).
    pub fn way_key(w: &SwapWay) -> String { if w.venue.is_empty() { w.name.clone() } else { format!("{}:{}", w.name, w.venue) } }

    /// An exchange as the card names it: Uniswap's versions are one («Uniswap v3» → «Uniswap»).
    pub fn family(title: &str) -> &str { if title.starts_with("Uniswap") { "Uniswap" } else { title } }

    /// The exchanges after Relay's bridge: Relay's own swap there would be the other way over again.
    pub const AFTER_BRIDGE: [&str; 2] = ["Uniswap", "KyberSwap"];

    /// The card showed another way or exchange than the one quoted now (two close ones, and the price moved): nothing is
    /// signed, and a fresh quote shows the new one — the card's approve step names who may take the money
    /// (the 2026-10-28 review). `shown` is the card's way («relay», «bridge:KyberSwap», «here:Uniswap v3»); None = no card.
    pub fn holds_shown_way(taken: &str, shown: Option<&str>) -> Result<()> {
        // `split(separator: ":", maxSplits: 1)`: empty pieces left out.
        fn parts(way: &str) -> Vec<&str> { way.splitn(2, ':').filter(|s| !s.is_empty()).collect() }
        fn plain(way: &str) -> String { parts(way).into_iter().map(Executor::family).collect::<Vec<_>>().join(":") }
        fn name(way: &str) -> String {
            let p = plain(way);
            let parts = parts(&p);
            match parts.first().copied() {
                Some("relay") => "Relay's swap".into(),
                Some("bridge") => format!("Relay's bridge + {}", parts.last().unwrap_or(&"an exchange")),
                _ => parts.last().map_or_else(|| way.to_string(), |s| s.to_string()),
            }
        }
        let Some(shown) = shown else { return Ok(()) };
        if plain(shown) == plain(taken) { return Ok(()) }
        fail(format!("The card showed {}, but {} leaves more now. Nothing was signed; Refresh the card (or ask again) to see it.", name(shown), name(taken)))
    }

    /// Buys a token on an EVM network for `usdc`: with the USDC already there when it holds enough (or `from` names it),
    /// with another EVM network's USDC brought over first (`from`), else from Solana, the better of the two ways (or the
    /// one picked on the route's step). `shown`: the way the card showed (holds_shown_way).
    pub async fn buy_on(&self, net: &EvmNetwork, r: &str, usdc: Decimal, prefer: Option<&str>, from: Option<&str>, shown: Option<&str>) -> Result<String> {
        let Some(uni) = uniswap::v3(net.rpc.chain_id) else { return fail(SWAPS_ON) };
        let token = self.evm_token(r, net).await?;
        let me = self.wallet.evm.as_str();
        let units = millionths(usdc)?;
        if units < 500_000 { return fail("At least 0.5 USDC: less isn't worth the bridge.") }
        let amount = |raw: Decimal| format!("{} {}", Fmt::num(tokens(raw, token.decimals), 6), token.symbol);
        // Solana short and the network without the USDC: it comes straight from the EVM network that holds it (the card
        // shows the same, EvmNetwork::source), not from a Solana that can't pay.
        let mut from = from.map(str::to_string);
        if from.is_none() && prefer.is_none() && dollars(net.usdc_balance(me).await?) < usdc {
            let on_solana = dollars(self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?);
            let idle: HashMap<String, Decimal> = self.idle_usdc().await.into_iter().map(|(n, u)| (n.key.to_string(), u)).collect();
            from = EvmNetwork::source(usdc, on_solana, net.key, |k| idle.get(k).copied()).map(|s| s.0.key.to_string());
        }
        // A way picked on the route's step is one of Solana's: only then is the money on the network passed over.
        if prefer.is_none() || from.is_some() {
            if let Some((pair, src)) = self.buy_here(net, &token, units, from.as_deref(), uni, false).await? {
                Self::holds_shown_way(&format!("here:{}", pair.title()), shown)?;
                self.holds_pair_impact(net, &pair, None).await?;
                let mut spend = units;
                if let Some(src) = src {
                    let before = net.usdc_balance(me).await?;
                    self.report(&self.bridge(src, Some(net), false, Some(usdc)).await?);
                    // What arrived pays: Relay's fee comes out of it.
                    spend = units.min(net.usdc_balance(me).await?.saturating_sub(before));
                }
                let title = pair.title();
                return self.buy_there(net, &token, spend, uni, Some(&[Self::family(&title)]), Some(usdc)).await;
            }
        }
        let (ways, bridged) = self.buy_ways(net, &token, units, uni).await?;
        let Some(way) = Self::pick(&ways, prefer) else {
            return fail(Self::unquoted(prefer, &format!("{} on {}", token.symbol, net.name)).unwrap_or_else(|| {
                format!("Neither Relay nor an exchange on {} quotes {} for {} USDC right now. Nothing was signed.", net.name, token.symbol, Fmt::num(usdc, 2))
            }));
        };
        Self::holds_shown_way(&Self::way_key(&way), shown)?;
        let plan = if way.name == "bridge" { bridged } else { None };
        if let Some(plan) = &plan { self.holds_pair_impact(net, &plan.pair, None).await? }
        self.report(&format!("{} USDC → {} on {}. {}. Taking {}{}.", Fmt::num(usdc, 2), token.symbol, net.name,
                             ways.iter().map(|w| w.line.as_str()).collect::<Vec<_>>().join("; "),
                             plan.as_ref().map_or_else(|| "Relay's swap".to_string(), |p| format!("the bridge and {}", p.pair.title())),
                             if prefer.is_none() { "" } else { ", as picked" }));
        if let Some(plan) = plan {
            // The USDC over the bridge (gas rides along when there's none), then the same exchange there, quoted again
            // with the money in place (buy_there): the price may have moved while the bridge ran.
            let there = net.usdc_balance(me).await?;
            self.ensure_evm(net, there.saturating_add(plan.arrive), None).await?;
            let now = net.usdc_balance(me).await?;
            let title = plan.pair.title();
            return self.buy_there(net, &token, plan.arrive.min(now.saturating_sub(there)), uni, Some(&[Self::family(&title)]), Some(usdc)).await;
        }

        let before = self.balance(&token.address, &net.rpc).await?;
        let (step, changes) = self.prepare_bridge(Amount::Send(usdc), net.relay_chain, &token.address, token.decimals, None, true).await?;
        self.report(&changes.join(" "));
        self.send_bridge(&step).await?;
        let mut after = self.balance(&token.address, &net.rpc).await?;
        let started = Instant::now();
        while after <= before && started.elapsed() < Duration::from_secs(240) {
            sleep(2).await;
            after = self.balance(&token.address, &net.rpc).await?;
        }
        if after <= before { return fail(format!("{} didn't arrive on {} within 4 minutes. The order is in the journal.", token.symbol, net.name)) }
        if let Some(traded) = &self.traded { traded(&format!("{}:{}", net.key, token.address.to_lowercase()), tokens(after - before, token.decimals), usdc) }
        Ok(format!("Bought {} on {} for {} USDC via Relay's swap. You hold {} there.", amount(after - before), net.name, Fmt::num(usdc, 2), amount(after)))
    }

    /// Whether a buy is paid on the network itself, and the best way there (best_pair): `from` None = when the network
    /// holds the USDC and an exchange there quotes it (else None: Solana's ways); «solana» = never; the network's key = it
    /// must; another EVM network's key = its USDC comes over first (`src`). A named place that can't pay is refused, never
    /// swapped for another. `quote_only`: for the card.
    pub async fn buy_here(&self, net: &EvmNetwork, token: &Token, units: u64, from: Option<&str>, uni: UniswapV3, quote_only: bool)
        -> Result<Option<(PairSwap, Option<&'static EvmNetwork>)>> {
        if from == Some("solana") { return Ok(None) }
        let want = Fmt::num(dollars(units), 2);
        let src = from.and_then(EvmNetwork::named).filter(|s| s.key != net.key);
        if from.is_some() && from != Some(net.key) && src.is_none() {
            return fail(format!("ihatecrypto doesn't pay for a buy on {} from {}. Nothing was signed.", net.name, from.unwrap_or("there")));
        }
        let payer: &EvmNetwork = match src { Some(s) => s, None => net };
        let there = payer.usdc_balance(&self.wallet.evm).await?;
        let best = if there >= units {
            self.best_pair(net, uni, &Self::usdc_token(net), token, false, false, raw(net, units), self.allowed_ways(net).uniswap, None, quote_only).await?
        } else { None };
        if let Some(best) = best { return Ok(Some((best, src))) }
        if from.is_none() { return Ok(None) }
        fail(if there < units {
            format!("{} USDC on {}; the buy needs {want}. Nothing was signed.", Fmt::num(dollars(there), 2), payer.name)
        } else {
            format!("No exchange on {} quotes {} for USDC now; “buy {} on {} for {want} USDC from solana” takes Relay's swap. Nothing was signed.",
                    net.name, token.symbol, token.symbol, net.key)
        })
    }

    /// A buy with USDC already on the network: the best way there (best_pair) — Uniswap's route, Relay's swap or
    /// KyberSwap's — each with an exact approve, paying only this wallet. Quoted again now: a bridge may have brought the
    /// USDC. `only`: after a bridge, the exchange the plan took — a live run (2026-10-28) approved Relay's proxy under a
    /// card that said only KyberSwap may take the USDC. `cost`: what the buy spent where it started (the bridge's fee in
    /// it), for P/L.
    async fn buy_there(&self, net: &EvmNetwork, token: &Token, units: u64, uni: UniswapV3, only: Option<&[&str]>, cost: Option<Decimal>) -> Result<String> {
        let usdc = Fmt::num(dollars(units), 6);
        let amount = |raw: Decimal| format!("{} {}", Fmt::num(tokens(raw, token.decimals), 6), token.symbol);
        // A loss that grew past what was accepted while the bridge ran: said plainly, the USDC where it is — a «Send anyway»
        // card would run the whole buy again, bridge and all.
        let pair = if units > 0 {
            match self.best_pair(net, uni, &Self::usdc_token(net), token, false, false, raw(net, units), self.allowed_ways(net).uniswap, only, false).await {
                Err(Error::Costly(stop)) => {
                    return fail(format!("{} The USDC stays on {}; nothing was swapped.", stop.description().replace(" Nothing was signed.", ""), net.name));
                }
                other => other?,
            }
        } else { None };
        let Some(best) = pair else { return fail(format!("No exchange on {} quotes {} now; the USDC stays there. Nothing was swapped.", net.name, token.symbol)) };
        self.holds_pair_impact(net, &best, Some(&format!("The USDC stays on {}; nothing was swapped.", net.name))).await?;
        self.report(&format!("{usdc} USDC on {} → {} on {}: ≈ {}{}.", net.name, token.symbol, best.title(), amount(best.out()), best.impact_text()));
        // Gas only: the USDC is there.
        self.ensure_evm(net, units, None).await?;
        let before = self.balance(&token.address, &net.rpc).await?;
        let hash = self.run_pair(net, uni, &best, &format!("{usdc} USDC"), &format!("Buy {} for {usdc} USDC", token.symbol)).await?.0;
        let mut after = self.balance(&token.address, &net.rpc).await?;
        for _ in 0..10 {
            if after > before { break }
            sleep(1).await;
            after = self.balance(&token.address, &net.rpc).await?;
        }
        if after > before {
            if let Some(traded) = &self.traded {
                traded(&format!("{}:{}", net.key, token.address.to_lowercase()), tokens(after - before, token.decimals), cost.unwrap_or_else(|| dollars(units)));
            }
        }
        Ok(format!("Bought {} on {} for {usdc} USDC there, on {}. You hold {} there. {}/tx/{hash}", amount(if after > before { after - before } else { Decimal::ZERO }),
                   net.name, best.title(), amount(after), net.explorer))
    }

    /// Both ways to buy for `units` of USDC from Solana, quoted and valued (nothing is signed): Relay's swap, and Relay's
    /// bridge (with a gas top-up when there's no ETH) followed by the best exchange there — Uniswap's best path or
    /// KyberSwap's route (best_pair) — its gas counted against it.
    pub async fn buy_ways(&self, net: &EvmNetwork, token: &Token, units: u64, uni: UniswapV3) -> Result<(Vec<SwapWay>, Option<BridgedBuy>)> {
        let (me, sol) = (self.wallet.evm.as_str(), self.wallet.solana.as_str());
        let allowed = self.allowed_ways(net);
        if !allowed.relay { return fail(modules().refusal(ModuleProtocol::Relay, &[], false).unwrap_or_else(|| "Relay is off in Settings › Modules.".into())) }
        let amount = |raw: Decimal| format!("{} {}", Fmt::num(tokens(raw, token.decimals), 6), token.symbol);
        let ask = |currency: &str, topup: Option<Decimal>| relay::Ask {
            user: sol.into(), recipient: me.into(), origin_chain: relay::chain::SOLANA, destination_chain: net.relay_chain,
            origin_currency: relay::currency::SOLANA_USDC.into(), destination_currency: currency.into(), amount: units, topup_gas_usd: topup, ..Default::default()
        };

        // Relay: Solana USDC in, the token out on the chain.
        let relay = relay::quote(&ask(&token.address, None)).await.ok();
        let relay_out = relay.as_ref().and_then(|q| decimal(&q.amount_out)).unwrap_or_default();
        let relay_worth = relay.as_ref().and_then(|q| decimal(&q.amount_out_usd)).unwrap_or_default();
        // The token's price for comparing: Relay's own valuation, else the Uniswap route's.
        let mut price = if relay_out > Decimal::ZERO { (Num::of(relay_worth) / tokens(relay_out, token.decimals)).0.unwrap_or_default() } else { Decimal::ZERO };
        let mut ways = vec![];
        if relay_out > Decimal::ZERO {
            ways.push(SwapWay::new("relay", relay_out, relay_worth, &format!("Relay swap: {} ≈ {}", amount(relay_out), Fmt::usd(relay_worth))));
        }

        // Relay bridge + the best exchange there: the USDC that arrives (a gas top-up rides along when there's no ETH), then
        // Uniswap's or KyberSwap's way for it — never Relay's swap on the network, which the first way already is.
        let mut bridged = None;
        if allowed.uniswap || allowed.kyber {
            let topup = net.rpc.balance(me).await? < u128::from(net.gas_floor);
            let arrive = relay::quote(&ask(net.usdc, topup.then_some(net.topup_usd))).await.ok()
                .and_then(|b| decimal(&b.amount_out)).map_or(0, |out| micro(net, out));
            let best = if arrive > 0 {
                self.best_pair(net, uni, &Self::usdc_token(net), token, false, false, raw(net, arrive), allowed.uniswap, Some(&Self::AFTER_BRIDGE), false).await
                    .ok().flatten()
            } else { None };
            if let Some(best) = best {
                if price.is_zero() { price = (Num::of(dollars(arrive)) / tokens(best.out(), token.decimals)).0.unwrap_or_default() }
                let gas = self.gas_usd(best.gas(), net, uni).await?;
                let worth = num(Num::of(tokens(best.out(), token.decimals)) * price - gas + if topup { net.topup_usd } else { Decimal::ZERO })?;
                let line = format!("Relay bridge + {}: {}{} ≈ {} after {} of gas{}", best.title(), amount(best.out()), best.impact_text(), Fmt::usd(worth), Fmt::usd(gas),
                                   if topup { format!(" ({} for gas rides along)", net.gas_coin) } else { String::new() });
                ways.push(SwapWay { venue: best.title(), ..SwapWay::new("bridge", best.out(), worth, &line) });
                bridged = Some(BridgedBuy { arrive, topup, pair: best });
            }
        }
        Ok((ways, bridged))
    }

    /// Sells a token on an EVM network back to USDC on Solana; None = all of it. The better of Relay's swap-and-bridge
    /// and the best exchange on the chain (Uniswap or KyberSwap) followed by the Relay bridge. `keep`: the best way there
    /// (best_pair), and the USDC stays on the network. `shown`: the way the card showed (holds_shown_way).
    pub async fn sell_on(&self, net: &EvmNetwork, r: &str, amount: Option<Decimal>, prefer: Option<&str>, keep: bool, shown: Option<&str>) -> Result<String> {
        let Some(uni) = uniswap::v3(net.rpc.chain_id) else { return fail(SWAPS_ON) };
        let token = self.evm_token(r, net).await?;
        let held = self.balance(&token.address, &net.rpc).await?;
        let units = whole(match amount { Some(a) => num(Num::of(a) * scale(token.decimals))?.min(held), None => held });
        if units <= Decimal::ZERO { return fail(format!("No {} on {}.", token.symbol, net.name)) }
        let amount_text = |raw: Decimal| format!("{} {}", Fmt::num(tokens(raw, token.decimals), 6), token.symbol);
        let key = format!("{}:{}", net.key, token.address.to_lowercase());
        if keep {
            let best = self.best_pair(net, uni, &token, &Self::usdc_token(net), false, false, units, self.allowed_ways(net).uniswap, None, false).await?;
            let Some(best) = best else {
                return fail(format!("No exchange on {} quotes {} for USDC now; without «keep», Relay's swap sells it for USDC on Solana. Nothing was signed.",
                                    net.name, token.symbol));
            };
            Self::holds_shown_way(&format!("here:{}", best.title()), shown)?;
            self.holds_pair_impact(net, &best, None).await?;
            self.report(&format!("{} on {} → ≈ {} USDC there{}, on {}.", amount_text(units), net.name, Fmt::num(dollars(micro(net, best.out())), 2), best.impact_text(),
                                 best.title()));
            self.ensure_evm(net, 0, None).await?;
            let (got, text) = self.sell_there(net, &best, uni).await?;
            if let Some(traded) = &self.traded { traded(&key, -tokens(units, token.decimals), got) }
            return Ok(text);
        }
        let (ways, bridged) = self.sell_ways(net, &token, units, uni).await?;
        let Some(way) = Self::pick(&ways, prefer) else {
            return fail(Self::unquoted(prefer, &format!("{} on {}", amount_text(units), net.name)).unwrap_or_else(|| {
                format!("Neither Relay nor an exchange on {} quotes {} right now. Nothing was signed.", net.name, amount_text(units))
            }));
        };
        Self::holds_shown_way(&Self::way_key(&way), shown)?;
        let plan = if way.name == "bridge" { bridged } else { None };
        if let Some(plan) = &plan { self.holds_pair_impact(net, plan, None).await? }
        // Relay's swap that nobody prices stops here, before a gas top-up is signed for it; relay_sell checks its fresh quote.
        if plan.is_none() && self.relay_worth_in(net, &token, units, &way.in_usd).await <= Decimal::ZERO {
            Self::holds_relay_sale(Decimal::ZERO, way.out / Decimal::from(1_000_000), &token.symbol, net.name)?;
        }
        self.report(&format!("{} on {} → USDC on Solana. {}. Taking {}{}.", amount_text(units), net.name,
                             ways.iter().map(|w| w.line.as_str()).collect::<Vec<_>>().join("; "),
                             plan.as_ref().map_or_else(|| "Relay's swap".to_string(), |p| format!("{} and the bridge", p.title())),
                             if prefer.is_none() { "" } else { ", as picked" }));
        // Both ways sign on the chain: ETH for gas first (a top-up from Solana when there's none).
        self.ensure_evm(net, 0, None).await?;

        if let Some(plan) = plan {
            // Quoted again now on the same exchange: the gas top-up may have taken a while.
            let title = plan.title();
            let fresh = self.best_pair(net, uni, &token, &Self::usdc_token(net), false, false, units, self.allowed_ways(net).uniswap, Some(&[Self::family(&title)]),
                                       false).await?.unwrap_or(plan);
            self.holds_pair_impact(net, &fresh, Some(&format!("{} stays on {}; nothing was swapped.", token.symbol, net.name))).await?;
            let (got, text) = self.sell_there(net, &fresh, uni).await?;
            self.report(&text);
            if let Some(traded) = &self.traded { traded(&key, -tokens(units, token.decimals), got) }
            return self.evm_to_solana(net, Some(got)).await;
        }
        let text = self.relay_sell(net, &token, units).await?;
        if let Some(traded) = &self.traded { traded(&key, -tokens(units, token.decimals), Decimal::ZERO) }
        Ok(text)
    }

    /// The sale on the network, the way `p` took: an exact approve of the tokens, then the swap to USDC at this wallet.
    async fn sell_there(&self, net: &EvmNetwork, p: &PairSwap, uni: UniswapV3) -> Result<(Decimal, String)> {
        let me = self.wallet.evm.as_str();
        let amount = format!("{} {}", Fmt::num(tokens(p.units, p.token_in.decimals), 6), p.token_in.symbol);
        let before = net.usdc_balance(me).await?;
        let hash = self.run_pair(net, uni, p, &amount, &format!("Sell {amount}")).await?.0;
        let mut after = net.usdc_balance(me).await?;
        for _ in 0..10 {
            if after > before { break }
            sleep(1).await;
            after = net.usdc_balance(me).await?;
        }
        let got = dollars(after.saturating_sub(before));
        Ok((got, format!("Sold {amount} for {} USDC on {}, on {}. {}/tx/{hash}", Fmt::num(got, 2), net.name, p.title(), net.explorer)))
    }

    /// Both ways to sell `units` back to USDC on Solana, quoted and valued (nothing is signed): Relay's swap-and-bridge,
    /// and the best exchange's way to USDC on the chain (Uniswap or KyberSwap, best_pair) followed by Relay's bridge; each
    /// one's gas counted against it.
    pub async fn sell_ways(&self, net: &EvmNetwork, token: &Token, units: Decimal, uni: UniswapV3) -> Result<(Vec<SwapWay>, Option<PairSwap>)> {
        let (me, sol) = (self.wallet.evm.as_str(), self.wallet.solana.as_str());
        let allowed = self.allowed_ways(net);
        if !allowed.relay { return fail(modules().refusal(ModuleProtocol::Relay, &[], false).unwrap_or_else(|| "Relay is off in Settings › Modules.".into())) }
        let ask = |currency: &str, units: String| relay::Ask {
            user: me.into(), recipient: sol.into(), origin_chain: net.relay_chain, destination_chain: relay::chain::SOLANA, origin_currency: currency.into(),
            destination_currency: relay::currency::SOLANA_USDC.into(), units: Some(units), ..Default::default()
        };
        let mut ways = vec![];
        if let Ok(relay) = relay::quote(&ask(&token.address, text(units))).await {
            if let Some(out) = decimal(&relay.amount_out).filter(|o| *o > Decimal::ZERO) {
                let gas = self.gas_usd(50_000 + 300_000, net, uni).await?;
                let line = format!("Relay swap: {} USDC on Solana after {} of gas", Fmt::num(out / Decimal::from(1_000_000), 2), Fmt::usd(gas));
                ways.push(SwapWay { in_usd: relay.amount_in_usd.clone(), ..SwapWay::new("relay", out, num(Num::of(out) / 1_000_000 - gas)?, &line) });
            }
        }
        let mut bridged = None;
        if allowed.uniswap || allowed.kyber {
            if let Ok(Some(best)) = self.best_pair(net, uni, token, &Self::usdc_token(net), false, false, units, allowed.uniswap, Some(&Self::AFTER_BRIDGE), false).await {
                let out = relay::quote(&ask(net.usdc, net.raw(micro(net, best.out())).to_string())).await.ok().and_then(|b| decimal(&b.amount_out));
                if let Some(out) = out.filter(|o| *o > Decimal::ZERO) {
                    let gas = self.gas_usd(best.gas().saturating_add(120_000), net, uni).await?;
                    let line = format!("{} + Relay bridge: {} USDC on Solana{} after {} of gas", best.title(), Fmt::num(out / Decimal::from(1_000_000), 2),
                                       best.impact_text(), Fmt::usd(gas));
                    ways.push(SwapWay { venue: best.title(), ..SwapWay::new("bridge", out, num(Num::of(out) / 1_000_000 - gas)?, &line) });
                    bridged = Some(best);
                }
            }
        }
        Ok((ways, bridged))
    }

    /// Whether `r` names the network's own coin: ETH, or POL (MATIC) on Polygon — never its wrapped token, which is a token.
    pub fn is_coin(r: &str, net: &EvmNetwork) -> bool {
        let r = plain(r);
        r == net.gas_coin.to_uppercase() || (net.gas_coin == "POL" && r == "MATIC")
    }

    /// One side of a pair: the network's own coin as its wrapped token, or a token.
    async fn pair_side(&self, r: &str, net: &EvmNetwork, weth: &str) -> Result<(Token, bool)> {
        if Self::is_coin(r, net) { return Ok((Token::new(net.gas_coin, net.gas_coin, weth, 18), true)) }
        Ok((self.evm_token(r, net).await?, false))
    }

    /// swap_on's quote, for the card too: nothing is signed. `quote_only`: for the card, priced at the amount asked even
    /// when the wallet holds less (the run checks it).
    pub async fn pair_swap(&self, net: &EvmNetwork, a: &str, b: &str, amount: Option<Decimal>, quote_only: bool) -> Result<PairSwap> {
        let (Some(uni), Some((_, weth))) = (uniswap::v3(net.rpc.chain_id), uniswap::v2(net.rpc.chain_id)) else {
            return fail("ihatecrypto swaps one token for another on Base, Arbitrum, Ethereum, Polygon, Optimism, BNB Chain and HyperEVM.");
        };
        let rpc = &net.rpc;
        let (token_in, coin_in) = self.pair_side(a, net, weth).await?;
        let (token_out, coin_out) = self.pair_side(b, net, weth).await?;
        if token_in.address == token_out.address && coin_in == coin_out { return fail("That's the same token on both sides. Nothing to swap.") }
        let held = if coin_in { wei(rpc.balance(&self.wallet.evm).await?)? } else { self.balance(&token_in.address, rpc).await? };
        let units = if let Some(amount) = amount {
            let units = whole(num(Num::of(amount) * scale(token_in.decimals))?);
            if !(units <= held || quote_only) {
                return fail(format!("Only {} {} on {}. Nothing was signed.", Fmt::num(tokens(held, token_in.decimals), 8), token_in.symbol, net.name));
            }
            units
        } else if coin_in {
            // All of the coin but what this swap's fees may take, twice over.
            let (_, max_fee) = rpc.fees().await?;
            num(Num::of(held) - Num::of(Decimal::from(max_fee)) * 1_200_000)?.max(Decimal::ZERO)
        } else { held };
        if units <= Decimal::ZERO { return fail(format!("No {} to swap on {}.", token_in.symbol, net.name)) }
        if coin_in && units > Decimal::from(u64::MAX) { return fail(format!("At most 18 {} at once. Nothing was signed.", net.gas_coin)) }
        let p = self.best_pair(net, uni, &token_in, &token_out, coin_in, coin_out, units, self.allowed_ways(net).uniswap, None, quote_only).await?;
        let Some(p) = p else {
            return fail(format!("Neither Uniswap, Relay nor KyberSwap swaps {} for {} on {} right now. Nothing was signed.", token_in.symbol, token_out.symbol, net.name));
        };
        Ok(p)
    }

    /// The way that leaves the most for `units` of `token_in` → `token_out` on the network itself, nothing signed:
    /// Uniswap's best route (or a v4 pool of the coin itself), Relay's swap, or KyberSwap's route over every exchange
    /// there — each valued at one price for what comes out (KyberSwap's, else Relay's) less the gas it takes. 1 BTCB → USDC
    /// on BNB Chain, 2026-10-28: Uniswap $74,004, Relay $81,168, KyberSwap $81,267. None when none quotes it. `uniswap`
    /// false: Uniswap is off in Settings › Modules. `quote_only`: for the card, Relay's loss shown rather than stopped at
    /// (relay_pair_quote). `only`: the exchanges it may take, by family («Uniswap», «KyberSwap», «Relay's swap»; None =
    /// any of them).
    #[allow(clippy::too_many_arguments)]
    pub async fn best_pair(&self, net: &EvmNetwork, uni: UniswapV3, token_in: &Token, token_out: &Token, coin_in: bool, coin_out: bool, units: Decimal,
                           uniswap: bool, only: Option<&[&str]>, quote_only: bool) -> Result<Option<PairSwap>> {
        let rpc = &net.rpc;
        let allowed = self.allowed_ways(net);
        let may = |family: &str| only.is_none_or(|o| o.contains(&family));
        let (uniswap, relay_on, kyber_on) = (uniswap && may("Uniswap"), allowed.relay && may("Relay's swap"), allowed.kyber && may("KyberSwap"));
        let pair = PairSwap::new(token_in.clone(), token_out.clone(), coin_in, coin_out, units, None);
        // The coin and its wrapped token are the same money: a wrap or an unwrap. The same token on both sides is no swap
        // («buy USDC for USDC» once reached here and would have unwrapped WETH: the Codex review, 2026-10-28).
        if token_in.address == token_out.address { return Ok((coin_in != coin_out).then_some(pair)) }
        let native = uniswap_v4::NATIVE;
        let wrapped = async { if uniswap { self.best_uniswap(net, uni, &token_in.address, &token_out.address, units).await } else { None } };
        let coin_pool = async {
            if !(uniswap && (coin_in || coin_out)) { return None }
            self.v4_route(if coin_in { native } else { &token_in.address }, if coin_out { native } else { &token_out.address }, units, rpc).await
        };
        // Relay takes the coin wrapped; KyberSwap only tokens on both sides (the coin would go as the call's value, unread).
        let relay_quote = async {
            if !relay_on { return None }
            self.relay_pair_quote(net, &token_in.address, if coin_out { native } else { &token_out.address }, units, false).await.ok()
        };
        let kyber_way = async { if kyber_on && !coin_in && !coin_out { kyber_route(rpc.chain_id, &token_in.address, &token_out.address, units).await.ok() } else { None } };
        let (mut route, pool, relay, kyber) = tokio::join!(wrapped, coin_pool, relay_quote, kyber_way);
        if let Some(pool) = pool.filter(|p| p.out > route.as_ref().map_or(Decimal::ZERO, |r| r.out)) {
            let small = whole(units / Decimal::from(20));
            let small_out = if small > Decimal::ZERO { self.v4_quote(&pool.key, pool.zero_for_one, small, rpc).await } else { None };
            let (out, mut direct) = (pool.out, UniRoute::of_v4(pool));
            if let Some(small_out) = small_out.filter(|s| *s > Decimal::ZERO) { direct.impact = impact(out, units, small_out, small) }
            route = Some(direct);
        }
        let decimals = token_out.decimals;
        // One price for what comes out, and one for a unit of gas — KyberSwap's own estimates where it quoted.
        let relay_usd = relay.as_ref().map_or(Decimal::ZERO, |r| dec(&r.quote.amount_out_usd));
        let price = kyber.as_ref().and_then(|k| if k.out_usd > Decimal::ZERO { (Num::of(k.out_usd) / tokens(k.out, decimals)).0 } else { None })
            .or_else(|| relay.as_ref().and_then(|r| if relay_usd > Decimal::ZERO { (Num::of(relay_usd) / tokens(r.out, decimals)).0 } else { None }));
        let ways = [route.is_some(), relay.is_some(), kyber.is_some()].iter().filter(|w| **w).count();
        let per_gas = if let Some(k) = kyber.as_ref().filter(|k| k.gas > 0) {
            (Num::of(k.gas_usd) / Decimal::from(k.gas)).0.unwrap_or_default()
        } else if ways > 1 && price.is_some() {
            self.gas_usd(1_000_000, net, uni).await.unwrap_or_default() / Decimal::from(1_000_000)
        } else { Decimal::ZERO };
        let worth = |out: Decimal, gas: u64| match price {
            Some(p) => (Num::of(tokens(out, decimals)) * p - Num::of(Decimal::from(gas)) * per_gas).0.unwrap_or(Decimal::MIN),
            None => tokens(out, decimals),
        };
        fn offer(best: &mut Option<(Decimal, PairSwap)>, worth: Decimal, p: PairSwap) {
            if best.as_ref().is_none_or(|b| worth > b.0) { *best = Some((worth, p)) }
        }
        let mut best: Option<(Decimal, PairSwap)> = None;
        if let Some(r) = &route { offer(&mut best, worth(r.out, r.gas()), PairSwap { route: Some(r.clone()), ..pair.clone() }) }
        if let Some(relay) = &relay {
            let in_usd = dec(&relay.quote.amount_in_usd);
            let mut p = PairSwap { relay_out: Some(relay.out), ..pair.clone() };
            p.way_impact = if in_usd > Decimal::ZERO && relay_usd > Decimal::ZERO {
                (Num::of(Decimal::ONE) - Num::of(relay_usd) / in_usd).0.map(|x| x.max(Decimal::ZERO))
            } else { route.as_ref().and_then(|r| r.impact) };
            offer(&mut best, worth(relay.out, p.gas()), p);
        }
        if let Some(k) = &kyber {
            let mut p = PairSwap { kyber: Some(k.clone()), way_impact: k.impact().or(route.as_ref().and_then(|r| r.impact)), ..pair.clone() };
            // A token KyberSwap doesn't price, and no Uniswap route to measure by: its own price for a twentieth instead,
            // so the impact stop still reads it (the 2026-10-28 review).
            let small = whole(units / Decimal::from(20));
            if p.way_impact.is_none() && small > Decimal::ZERO {
                if let Ok(s) = kyber_route(rpc.chain_id, &token_in.address, &token_out.address, small).await {
                    if s.out > Decimal::ZERO { p.way_impact = impact(k.out, units, s.out, small) }
                }
            }
            offer(&mut best, worth(k.out, p.gas()), p);
        }
        // Relay's swap leaves the most though it loses past its cap, and the card showed it so: asked like any costly move
        // («Send anyway» takes it), never swapped silently for the next way (the 2026-10-28 review).
        if let (false, Some((_, b)), Some(r)) = (quote_only, &best, &relay) {
            if b.relay_out.is_some() { Self::holds_relay_loss(dec(&r.quote.amount_in_usd), dec(&r.quote.amount_out_usd))? }
        }
        Ok(best.map(|b| b.1))
    }

    /// Relay's swap losing more than 3% (or $0.15) by its own valuation: a CostlyMove to ask about first. It carries the
    /// loss as an impact, so one «Send anyway» covers this and the impact stop on the same loss (holds_pair_impact reads
    /// it as Relay's impact) — two separate answers asked in turn forever (the 2026-10-28 review).
    pub fn holds_relay_loss(worth_in: Decimal, worth_out: Decimal) -> Result<()> {
        let lost = num(Num::of(worth_in) - worth_out)?;
        let share = if worth_in > Decimal::ZERO { num(Num::of(lost) / worth_in)? } else { Decimal::ZERO };
        let cap = num(Num::of(worth_in) * d("0.03"))?.max(d("0.15"));
        if !(lost > cap && lost > loss_accepted() && share > impact_accepted()) { return Ok(()) }
        Err(CostlyMove { impact: Some(share), ..CostlyMove::new(lost, worth_in, "Relay's swap") }.into())
    }

    /// What `units` of the token going into Relay's swap are worth in dollars: Relay's own valuation, else DefiLlama's sure
    /// price — 0 when neither prices it, which holds_relay_sale stops (read as $0 before, it let any loss through).
    pub async fn relay_worth_in(&self, net: &EvmNetwork, token: &Token, units: Decimal, relay_usd: &str) -> Decimal {
        Self::relay_worth_in_by(relay_usd, tokens(units, token.decimals), || async {
            holdings::llama_prices(net.key, std::slice::from_ref(&token.address)).await.get(&token.address.to_lowercase()).copied()
        }).await
    }

    /// The same with DefiLlama's price as `llama`, asked only when Relay gave none (empty, 0 or less); only a sure one
    /// counts: a low price would hide a real loss, a high one only asks «Send anyway». (Swift's static `relayWorthIn`.)
    pub async fn relay_worth_in_by<F: Future<Output = Option<(Decimal, bool)>>>(relay_usd: &str, amount: Decimal, llama: impl FnOnce() -> F) -> Decimal {
        if let Some(usd) = decimal(relay_usd).filter(|u| *u > Decimal::ZERO) { return usd }
        match llama().await {
            Some((price, true)) => (Num::of(amount) * price).0.unwrap_or_default(),
            _ => Decimal::ZERO,
        }
    }

    /// Relay's swap-and-bridge of a token, judged by what goes in: nobody's dollar price stops it (read as $0 it let any
    /// loss through), else at most 3% (or $0.15) lost, past that a CostlyMove to ask about.
    pub fn holds_relay_sale(worth_in: Decimal, arriving: Decimal, symbol: &str, network: &str) -> Result<()> {
        if worth_in <= Decimal::ZERO {
            return fail(format!("Neither Relay nor DefiLlama prices {symbol} on {network} now, so what Relay's swap keeps can't be checked. \
                                 {symbol} stays on {network}; nothing was swapped. Pick «Relay bridge + the best exchange» on the route's step."));
        }
        Self::check_loss(num(Num::of(worth_in) - arriving)?, worth_in, num(Num::of(worth_in) * d("0.03"))?.max(d("0.15")), "Relay's swap")
    }

    /// A swap on the network checked against IMPACT_CAP before anything is signed (a bridge or gas top-up included,
    /// `signed` says where the money waits): Uniswap's measured impact, KyberSwap's own; Relay's swap has its cap in
    /// relay_pair_quote. What goes in, in USD: the USDC itself, else what the USDC out was worth before the impact, else
    /// KyberSwap's valuation, else DefiLlama's price (none: the impact alone decides).
    pub async fn holds_pair_impact(&self, net: &EvmNetwork, p: &PairSwap, signed: Option<&str>) -> Result<()> {
        let Some(impact) = p.impact().filter(|i| *i > Self::IMPACT_CAP) else { return Ok(()) };
        let amount = tokens(p.units, p.token_in.decimals);
        let usdc = net.usdc.to_lowercase();
        let mut paid = if p.token_in.address == usdc {
            dollars(micro(net, p.units))
        } else if p.token_out.address == usdc && impact < Decimal::ONE {
            (Num::of(dollars(micro(net, p.out()))) / (Num::of(Decimal::ONE) - impact)).0.unwrap_or_default()
        } else { p.kyber.as_ref().map_or(Decimal::ZERO, |k| k.in_usd) };
        if paid.is_zero() {
            let prices = holdings::llama_prices(net.key, std::slice::from_ref(&p.token_in.address)).await;
            paid = (Num::of(amount) * prices.get(&p.token_in.address.to_lowercase()).map_or(Decimal::ZERO, |x| x.0)).0.unwrap_or_default();
        }
        Self::check_impact(impact, paid, amount, if p.coin_in { net.gas_coin } else { &p.token_in.symbol }, &p.title(), signed)
    }

    /// KyberSwap's route, built for this wallet and read back before anything is signed: the router call must be its swap
    /// of exactly `units` of the token in for the token out, paying only this wallet, no fee to anyone, at most the route's
    /// tolerance under its quote (UniRoute::tolerance_of, by its impact); then an exact approve to its router, and the call.
    #[allow(clippy::too_many_arguments)]
    pub async fn kyber_swap(&self, net: &EvmNetwork, route: &KyberRoute, token_in: &Token, token_out: &Token, units: Decimal, what: &str,
                            rpc: Option<&EvmRpc>) -> Result<String> {
        let rpc = rpc.unwrap_or(&net.rpc);
        let me = self.wallet.evm.as_str();
        let tolerance = UniRoute::tolerance_of(route.impact());
        let call = kyber_build(route, net.rpc.chain_id, me, (tolerance * Decimal::from(10_000)).to_i64().unwrap_or(0)).await?;
        let floor = whole(num(Num::of(route.out) * (Decimal::ONE - tolerance))?);
        let exact = kyber::decode(&call).is_some_and(|s| {
            kyber::refusal(&s, me).is_none() && s.src_token.to_lowercase() == token_in.address.to_lowercase()
                && s.dst_token.to_lowercase() == token_out.address.to_lowercase() && u256(units).ok() == Some(s.amount)
                && crate::decimal(s.min_return).is_some_and(|m| m >= floor - Decimal::ONE)
        });
        if !exact { return fail("KyberSwap's call isn't the swap it quoted (this wallet, these tokens, this amount, its floor). Nothing was signed.") }
        self.approve_and_send(rpc, &token_in.address, kyber::ROUTER, u256(units)?, &format!("Approve exactly {what} to KyberSwap"), kyber::ROUTER, &call,
                              &format!("KyberSwap: swap {what} for {} on {}", token_out.symbol, net.name), &[], 0).await
    }

    /// Relay's swap on one network, from and to this wallet: its quote, checked — the sender and the recipient are this
    /// wallet, the steps an exact approve of what goes in to the approval proxy Relay publishes for the network and then
    /// a call to that same proxy with no coins, and at most 3% (or $0.15) lost by Relay's own valuation. `to` 0x0…0 is
    /// the network's coin. `capped` false: for comparing, where best_pair asks about the loss only if Relay's swap is the
    /// one taken.
    pub async fn relay_pair_quote(&self, net: &EvmNetwork, from: &str, to: &str, units: Decimal, capped: bool) -> Result<RelayPair> {
        let me = self.wallet.evm.to_lowercase();
        let quote = self.relay_quote(&relay::Ask {
            user: self.wallet.evm.clone(), recipient: self.wallet.evm.clone(), origin_chain: net.relay_chain, destination_chain: net.relay_chain,
            origin_currency: from.into(), destination_currency: to.into(), units: Some(text(units)), ..Default::default()
        }).await?;
        let binding = guard::binding_check(&pairs(&[("sender", &quote.sender.to_lowercase()), ("recipient", &quote.recipient.to_lowercase())]),
                                           &pairs(&[("sender", &me), ("recipient", &me)]), "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let proxy = relay::approval_proxy(net.relay_chain).await?.map(|p| p.to_lowercase());
        let Some((proxy, call)) = proxy.zip(quote.evm.last().cloned()).filter(|(proxy, call)| {
            call.to.to_lowercase() == *proxy && call.chain_id == net.relay_chain && no_value(call)
                && quote.evm[..quote.evm.len() - 1].iter().all(|s| approves(s, &from.to_lowercase(), proxy, units))
        }) else {
            return fail(format!("Relay's transactions don't go to the approval proxy it publishes for {}, for exactly this amount. Nothing was signed.", net.name));
        };
        let out = dec(&quote.amount_out);
        if out <= Decimal::ZERO { return fail(format!("Relay doesn't quote this swap on {} now. Nothing was signed.", net.name)) }
        if capped { Self::holds_relay_loss(dec(&quote.amount_in_usd), dec(&quote.amount_out_usd))? }
        Ok(RelayPair { quote, call, proxy, out })
    }

    /// What the card shows while a token-for-token swap waits: «≈ 25.31 AERO · price impact 0.4%», and the route's name.
    pub async fn pair_quote(&self, net: &EvmNetwork, a: &str, b: &str, amount: Option<Decimal>) -> Result<(String, String)> {
        if net.key == "tempo" {
            return match self.tempo_quote(a, b, amount, true).await {
                Ok(q) => Ok((format!("{} {}", Fmt::num(dollars(q.1), 6), q.3), "Tempo's exchange".into())),
                Err(error) => {
                    let Some(r) = self.tempo_relay(a, b, amount, true).await? else { return Err(error) };
                    Ok((format!("{} {}", Fmt::num(r.out / Decimal::from(1_000_000), 6), r.y.0), "Relay's swap".into()))
                }
            };
        }
        let p = self.pair_swap(net, a, b, amount, true).await?;
        Ok((format!("{} {}{}", Fmt::num(tokens(p.out(), p.token_out.decimals), 6), p.token_out.symbol, p.impact_text()), p.title()))
    }

    /// What the wallet holds of a pair's output: the coin itself, or the token.
    async fn held_out(&self, net: &EvmNetwork, p: &PairSwap) -> Result<Decimal> {
        if p.coin_out { wei(net.rpc.balance(&self.wallet.evm).await?) } else { self.balance(&p.token_out.address, &net.rpc).await }
    }

    /// «swap 0.01 ETH for AERO on Base», «swap all DEGEN for AERO on base»: one token for another on the network,
    /// straight — the best of Uniswap v2, v3 and v4 for the pair (pair_swap), no USDC in the middle, paying only this
    /// wallet. The network's own coin goes into a v4 pool of it as the call's value, or is wrapped first (its WETH, one
    /// transaction); it comes out of such a pool as coins, or is unwrapped after. None amount = all of it (of the coin,
    /// all but the fees).
    pub async fn swap_on(&self, net: &EvmNetwork, a: &str, b: &str, amount: Option<Decimal>) -> Result<String> {
        if net.key == "tempo" { return self.tempo_swap(a, b, amount).await }
        let p = self.pair_swap(net, a, b, amount, false).await?;
        let Some(uni) = uniswap::v3(net.rpc.chain_id) else { return fail(format!("No Uniswap on {}.", net.name)) };
        self.holds_pair_impact(net, &p, None).await?;
        let show = |raw: Decimal, t: &Token| format!("{} {}", Fmt::num(tokens(raw, t.decimals), 8), t.symbol);
        let what = show(p.units, &p.token_in);
        if p.units > Decimal::from(u64::MAX) && p.coin_in { return fail(format!("At most 18 {} at once. Nothing was signed.", net.gas_coin)) }
        self.ensure_evm(net, 0, None).await?;
        let before = self.held_out(net, &p).await?;
        let (last, received) = self.run_pair(net, uni, &p, &what, &format!("Swap {what} for {}", p.token_out.symbol)).await?;
        let mut after = self.held_out(net, &p).await?;
        for _ in 0..10 {
            if after > before { break }
            sleep(1).await;
            after = self.held_out(net, &p).await?;
        }
        // ponytail: a token-for-token swap has no dollar cost to record; the P/L by token counts only buys and sales for USDC.
        let got = received.unwrap_or(if after > before { after - before } else { Decimal::ZERO });
        let approx = if p.direct() && p.coin_out { "≈ " } else { "" };
        let via = if p.route.is_some() || p.kyber.is_some() || p.relay_out.is_some() { format!(", on {}", p.title()) } else { String::new() };
        Ok(format!("Swapped {what} for {approx}{} on {}{via}. You hold {} there. {}/tx/{last}", show(got, &p.token_out), net.name, show(after, &p.token_out),
                   net.explorer))
    }

    /// The coin wrapped into its WETH, then what spends it; a failure after the wrap says where the money is now. One
    /// sent with no outcome yet (a receipt that didn't come) is said as it is: the WETH may be swapped already.
    #[allow(clippy::too_many_arguments)]
    async fn wrap_then<T>(&self, net: &EvmNetwork, p: &PairSwap, weth: &str, wei: u64, what: &str, wrapped_name: &str,
                          spend: impl Future<Output = Result<T>>) -> Result<T> {
        if !p.coin_in { return spend.await }
        self.send_evm(&net.rpc, weth, &DEPOSIT, &format!("Wrap {what} into {wrapped_name}"), &[], None, wei).await?;
        let wrapped = Utc::now().trunc_subsecs(0); // the journal keeps whole seconds
        spend.await.map_err(|error| {
            let in_flight = self.journal.unresolved().ok().is_some_and(|u| u.iter().any(|e| e.chain == net.key && e.at >= wrapped));
            if in_flight { error } else { Self::left_wrapped(&error, p.units / Decimal::from(1_000_000_000_000_000_000u64), wrapped_name, net) }
        })
    }

    /// Carries out the swap `p` chose on the network, nothing checked here that the caller hasn't (holds_pair_impact):
    /// Relay's swap, asked again before the coin is wrapped and every check run on that quote; KyberSwap's route, built and
    /// read back (kyber_swap); Uniswap's route, the network's own coin into a v4 pool of it as the call's value or wrapped
    /// first (its WETH, one transaction), out of such a pool as coins or unwrapped after; or a plain wrap or unwrap.
    /// Returns the last transaction and, where the coin's balance can't say it (its fees come out of the same coin), what
    /// came out.
    pub async fn run_pair(&self, net: &EvmNetwork, uni: UniswapV3, p: &PairSwap, what: &str, note: &str) -> Result<(String, Option<Decimal>)> {
        let Some((_, weth)) = uniswap::v2(net.rpc.chain_id) else { return fail(format!("No wrapped {} known on {}.", net.gas_coin, net.name)) };
        let rpc = &net.rpc;
        let wrapped_name = format!("W{}", net.gas_coin);
        let wei = if p.coin_in { uint64(p.units) } else { 0 };
        let show = |raw: Decimal, t: &Token| format!("{} {}", Fmt::num(tokens(raw, t.decimals), 8), t.symbol);
        let as_wrapped = || if p.coin_in { format!("{} {wrapped_name}", Fmt::num(tokens(p.units, 18), 8)) } else { what.to_string() };
        if p.relay_out.is_some() {
            // Relay is asked again first, while the coin is still the coin: the price may have moved since the card, and a
            // refusal or a loss past the cap then leaves nothing wrapped. It quotes the wrapped token the wallet doesn't
            // hold yet (best_pair's quote does the same). Then the wrap, the exact approve and the call to its proxy.
            let q = self.relay_pair_quote(net, &p.token_in.address, if p.coin_out { uniswap_v4::NATIVE } else { &p.token_out.address }, p.units, true).await?;
            self.report(&format!("{what} → {} on {} through Relay's swap: ≈ {}.", p.token_out.symbol, net.name, show(q.out, &p.token_out)));
            let units = u256(p.units)?;
            let last = self.wrap_then(net, p, weth, wei, what, &wrapped_name, self.approve_and_send(
                rpc, &p.token_in.address, &q.proxy, units, &format!("Approve exactly {} to Relay", as_wrapped()), &q.call.to, &q.call.data,
                &format!("Relay: swap {what} for {} on {}", p.token_out.symbol, net.name), &[], 0)).await?;
            self.record("relay", &q.quote.request_id, Kind::Signed, &format!("{} {}→{}, tx {last}", net.name, p.token_in.symbol, p.token_out.symbol))?;
            return Ok((last, None));
        }
        if let Some(k) = &p.kyber {
            self.report(&format!("{what} → {} on {} through KyberSwap: ≈ {}{}.", p.token_out.symbol, net.name, show(k.out, &p.token_out), p.impact_text()));
            return Ok((self.kyber_swap(net, k, &p.token_in, &p.token_out, p.units, what, None).await?, None));
        }
        if let Some(route) = &p.route {
            self.report(&format!("{what} → {} on {}, on {}{}: ≈ {}{}.", p.token_out.symbol, net.name, route.title(),
                                 if p.direct() { format!(" ({} itself, no wrap)", net.gas_coin) } else { String::new() }, show(route.out, &p.token_out), route.impact_text()));
            if p.direct() {
                let last = self.swap_uniswap(route, net, uni, if p.coin_in { uniswap_v4::NATIVE } else { &p.token_in.address }, p.units, what, note, None).await?;
                return Ok((last, p.coin_out.then_some(route.out)));
            }
            let weth_before = if p.coin_out { self.balance(weth, rpc).await? } else { Decimal::ZERO };
            // The coin is its wrapped token by then: that's what the approve lets Uniswap take.
            let mut last = self.wrap_then(net, p, weth, wei, what, &wrapped_name,
                                          self.swap_uniswap(route, net, uni, &p.token_in.address, p.units, &as_wrapped(), note, None)).await?;
            if !p.coin_out { return Ok((last, None)) }
            // A node can answer from before the swap's block: it said 0, the unwrap was skipped and the WETH stayed in the
            // wallet (2026-10-22, after an EIP-7702 batch on Base). So it's asked again while nothing shows.
            let mut got = self.balance(weth, rpc).await? - weth_before;
            for _ in 0..10 {
                if got > Decimal::ZERO { break }
                sleep(1).await;
                got = self.balance(weth, rpc).await? - weth_before;
            }
            if got > Decimal::ZERO {
                last = self.send_evm(rpc, weth, &[&WITHDRAW[..], &abi::word(u256(got)?)].concat(),
                                     &format!("Unwrap {} {wrapped_name} into {}", Fmt::num(tokens(got, 18), 8), net.gas_coin), &[], None, 0).await?;
            }
            return Ok((last, Some(got)));
        }
        if p.coin_in && !p.coin_out {
            return Ok((self.send_evm(rpc, weth, &DEPOSIT, &format!("Wrap {what} into {wrapped_name}"), &[], None, wei).await?, None));
        }
        if !(p.coin_out && !p.coin_in && p.token_in.address == weth) {
            return fail(format!("No way to swap {} for {} on {}. Nothing was signed.", p.token_in.symbol, p.token_out.symbol, net.name));
        }
        let data = [&WITHDRAW[..], &abi::word(u256(p.units)?)].concat();
        Ok((self.send_evm(rpc, weth, &data, &format!("Unwrap {what} into {}", net.gas_coin), &[], None, 0).await?, Some(p.units)))
    }

    /// What a failure after the coin's wrap says: not «Nothing was signed» (the wrap was), and the way back, one unwrap of
    /// this amount — not «all», which would take WETH the wallet held before. A guard's stop stays a guard's stop: the
    /// chat keeps it apart.
    pub fn left_wrapped(error: &Error, coins: Decimal, wrapped: &str, net: &EvmNetwork) -> Error {
        let n = Fmt::num(rounded(coins, 8, Rounding::Down), 8); // down: never more than the wrap gave
        let said = error.to_string().replace(" Nothing was signed.", "");
        let text = format!("{said}{} {n} {} is {wrapped} in the wallet now: «swap {n} {wrapped} for {} on {}» turns it back.",
                           if said.ends_with('.') { "" } else { "." }, net.gas_coin, net.gas_coin, net.key);
        if matches!(error, Error::Guard(_)) { Error::Guard(text) } else { Error::Execution(text) }
    }

    /// Relay's own swap-and-bridge from a token on the chain: an exact approve to the approval proxy Relay publishes for
    /// that chain, then its call, which must go to that same proxy. The quote must be from this wallet to our Solana
    /// address.
    async fn relay_sell(&self, net: &EvmNetwork, token: &Token, units: Decimal) -> Result<String> {
        let (me, sol) = (self.wallet.evm.as_str(), self.wallet.solana.as_str());
        let quote = self.relay_quote(&relay::Ask {
            user: me.into(), recipient: sol.into(), origin_chain: net.relay_chain, destination_chain: relay::chain::SOLANA, origin_currency: token.address.clone(),
            destination_currency: relay::currency::SOLANA_USDC.into(), units: Some(text(units)), ..Default::default()
        }).await?;
        let binding = guard::binding_check(&pairs(&[("sender", &quote.sender), ("recipient", &quote.recipient)]), &pairs(&[("sender", me), ("recipient", sol)]),
                                           "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let proxy = relay::approval_proxy(net.relay_chain).await?.map(|p| p.to_lowercase());
        let Some((proxy, call)) = proxy.zip(quote.evm.last().cloned()).filter(|(proxy, call)| {
            call.to.to_lowercase() == *proxy && call.chain_id == net.relay_chain && no_value(call)
                && quote.evm[..quote.evm.len() - 1].iter().all(|s| approves(s, &token.address, proxy, units))
        }) else {
            return fail(format!("Relay's transactions don't go to the approval proxy it publishes for {}, for exactly this amount. Nothing was signed.", net.name));
        };
        let arriving = dec(&quote.amount_out) / Decimal::from(1_000_000);
        if arriving <= Decimal::ZERO { return fail(format!("Relay doesn't quote {} for USDC now. Nothing was signed.", token.symbol)) }
        let worth_in = self.relay_worth_in(net, token, units, &quote.amount_in_usd).await;
        Self::holds_relay_sale(worth_in, arriving, &token.symbol, net.name)?;
        let mint = relay::currency::SOLANA_USDC;
        let sol_before = self.rpc.token_balance(sol, mint).await?;
        let amount = format!("{} {}", Fmt::num(tokens(units, token.decimals), 6), token.symbol);
        let hash = self.approve_and_send(&net.rpc, &token.address, &proxy, u256(units)?, &format!("Approve exactly {amount} to Relay"), &call.to, &call.data,
                                         &format!("Relay: sell {amount} on {}", net.name), &[], 0).await?;
        self.record("relay", &quote.request_id, Kind::Signed, &format!("{} {}→Solana USDC, tx {hash}", net.name, token.symbol))?;
        let started = Instant::now();
        let target = sol_before.saturating_add(uint64(num(Num::of(arriving) * 990_000)?));
        while started.elapsed() < Duration::from_secs(240) {
            let status = relay::status(&quote.request_id).await?.0;
            if ["refund", "failure"].contains(&status.as_str()) {
                self.record("relay", &quote.request_id, Kind::Failed, &status)?;
                return fail(format!("Relay returned {status}. Check {} on {}. {}/tx/{hash}", token.symbol, net.name, net.explorer));
            }
            let now = self.rpc.token_balance(sol, mint).await?;
            if now >= target {
                self.record("relay", &quote.request_id, Kind::Settled, &status)?;
                return Ok(format!("Sold {amount} on {} via Relay: {} USDC arrived on Solana in {} s. {}/tx/{hash}", net.name,
                                  Fmt::num(dollars(now.saturating_sub(sol_before)), 2), started.elapsed().as_secs(), net.explorer));
            }
            sleep(2).await;
        }
        fail(format!("Relay didn't credit Solana within 4 minutes. Order {} is in the journal. {}/tx/{hash}", quote.request_id, net.explorer))
    }
}

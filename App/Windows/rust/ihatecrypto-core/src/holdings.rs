//! Tokens the wallet holds on any network, beyond the balances the app reads one by one (`Holdings.swift`): listed per
//! network (Blockscout, a node's own answers on Tempo, HyperEVM and BNB Chain, Hyperliquid's spot book, NEAR's lists),
//! priced where a market knows them (CoinGecko's rate, DexScreener's deepest pool, DefiLlama, Relay), and the history a
//! holding's cost is read from. Requests go through llm's HTTP (Swift's `HTTP`); each answer is read by its own
//! function, testable offline.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};
use tokio::task::JoinSet;

use crate::foundation::{Date, chars, count, decimal, double, iso8601_fraction, json_decimal, seconds, shifted, trim_spaces, unix, units};
use crate::pnl::{Change, CostBasis};
use crate::stablecoins;

/// A token the wallet holds: found by listing the address on each network, priced where a market knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    /// «solana», «base», «arbitrum», «ethereum», «robinhood», «hyperliquid» (spot) or «near».
    pub chain: String,
    /// The mint, the contract, the spot token's name or the NEAR contract; «native» for the network's own coin.
    pub contract: String,
    pub amount: Decimal,
    /// Made plain by `new` (`plain`): set it through `new`, not by hand.
    pub symbol: String,
    pub icon: Option<String>,
    pub price: Option<Decimal>,
    /// A market list vouches for it: Jupiter's check, a CoinGecko rate on Blockscout, Ref's price list.
    pub verified: bool,
    /// The pool behind the price (or, on Hyperliquid's spot book, the day's volume).
    pub liquidity: Option<Decimal>,
    /// The network's own coin (ETH on Arbitrum and Ethereum): gas, in the balance but not in the risk rules' capital.
    pub native: bool,
    /// How far the price moved in 24 hours, in %, where the market that priced it says.
    pub change24h: Option<Decimal>,
}

impl Holding {
    /// `Holding(chain:contract:amount:symbol:)`; the rest is set after: `Holding { price, ..Holding::new(…) }`.
    pub fn new(chain: &str, contract: &str, amount: Decimal, symbol: &str) -> Self {
        Self {
            chain: chain.into(), contract: contract.into(), amount, symbol: Self::plain(symbol, 16), icon: None, price: None,
            verified: false, liquidity: None, native: false, change24h: None,
        }
    }

    /// The key the user's show/hide choice is kept under: the mint on Solana, «chain:contract» elsewhere.
    pub fn id(&self) -> String {
        if self.chain == "solana" { self.contract.clone() } else { format!("{}:{}", self.chain, self.contract.to_lowercase()) }
    }

    /// The same token at another amount.
    pub fn at(&self, amount: Decimal) -> Holding { Holding { amount, symbol: Self::plain(&self.symbol, 16), ..self.clone() } }

    pub fn usd(&self) -> Option<Decimal> { self.price?.checked_mul(self.amount) }

    /// A token's ticker or name as anyone deploying it wrote it, made fit for the chat's model and the screen (research
    /// E-9): no line breaks or invisible characters (AddressCheck.hidden), at most `max` characters — a «symbol» can be a
    /// paragraph of instructions. Swift's default `max` is 16.
    pub fn plain(text: &str, max: usize) -> String {
        let kept: String = text.chars().filter(|&c| !wallet::address_check::hidden(c)).collect();
        let s = trim_spaces(&kept);
        if count(s) > max { format!("{}…", chars(s)[..max.saturating_sub(1)].concat()) } else { s.into() }
    }

    /// Airdropped look-alikes are unverified with no real pool behind them: they stay out of sight until the user
    /// asks. A token the user bought has a pool (or a market list's check) and shows.
    pub fn looks_real(&self) -> bool {
        self.verified || (self.liquidity.unwrap_or_default() >= Decimal::from(10_000) && self.usd().unwrap_or_default() >= Decimal::new(1, 2))
    }

    /// Marked «unverified»: no list vouches for it. Never on Hyperliquid's spot book, where a name belongs to one token,
    /// so nothing there can be a look-alike.
    pub fn unverified(&self) -> bool { !self.verified && self.chain != "hyperliquid" }
}

/// The holding a transfer names.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    One(Holding),
    None,
    Several(Vec<Holding>),
}

/// The holding a transfer names: by mint or contract (any case on EVM), else by ticker, any case. The network's own
/// coin wins its ticker («ETH»), and among tokens sharing one — an airdrop copying a real token's name — the only one
/// that looks real; otherwise `Several`, and the address decides.
pub fn pick(query: &str, held: &[Holding]) -> Pick {
    let q = trim_spaces(query).to_lowercase();
    if let Some(one) = held.iter().find(|h| h.contract.to_lowercase() == q) { return Pick::One(one.clone()) }
    let named: Vec<&Holding> = held.iter().filter(|h| h.symbol.to_lowercase() == q).collect();
    if let Some(coin) = named.iter().find(|h| h.native) { return Pick::One((*coin).clone()) }
    let real: Vec<&&Holding> = named.iter().filter(|h| h.looks_real()).collect();
    match (named.len(), real.len()) {
        (1, _) => Pick::One(named[0].clone()),
        (_, 1) => Pick::One((*real[0]).clone()),
        (0, _) => Pick::None,
        _ => Pick::Several(named.into_iter().cloned().collect()),
    }
}

/// Blockscout turns away clients whose User-Agent doesn't start like a browser's (403 on Base, Arbitrum, Polygon and
/// Robinhood Chain, 2026-10-23); a crawler's «Mozilla/5.0 (compatible; …)» passes and still says who is asking.
pub const BROWSER: [(&str, &str); 1] = [("User-Agent", "Mozilla/5.0 (compatible; ihatecrypto; +https://ihatecrypto.ai)")];

pub const BLOCKSCOUT: [(&str, &str); 6] = [
    ("base", "base.blockscout.com"), ("arbitrum", "arbitrum.blockscout.com"), ("polygon", "polygon.blockscout.com"),
    ("ethereum", "eth.blockscout.com"), ("robinhood", "robinhoodchain.blockscout.com"), ("optimism", "explorer.optimism.io"),
];

/// DefiLlama's names for the networks it prices; Robinhood Chain and Tempo it doesn't.
pub const LLAMA_CHAIN: [(&str, &str); 7] = [
    ("base", "base"), ("arbitrum", "arbitrum"), ("ethereum", "ethereum"), ("polygon", "polygon"), ("optimism", "optimism"), ("bnb", "bsc"),
    ("hyperevm", "hyperliquid"),
];

/// Bridged USDC from before Circle issued its own there: both contracts call themselves «USDC», so the portfolio names
/// them «USDC.e» (as Uniswap's list does), and offers to swap them for Circle's.
pub const BRIDGED_USDC: [(&str, &str); 2] =
    [("polygon", "0x2791bca1f2de4661ed88a30c99a7a9449aa84174"), ("arbitrum", "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8")];

/// A network's entry in one of the tables above.
pub fn lookup(table: &[(&'static str, &'static str)], chain: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == chain).map(|(_, v)| *v)
}

const ETH_ICON: &str = "https://assets.coingecko.com/coins/images/279/small/ethereum.png";

// MARK: EVM networks

/// Every ERC-20 at the address on one network, and its ETH when `native` (Base's and Robinhood's ETH is already
/// counted as gas). `skip`: contracts the app counts elsewhere (USDC, Aave's aUSDC, the Morpho vault…), lowercased.
/// None when the network couldn't be read in full: the caller keeps what it had instead of showing a shorter list.
pub async fn evm(chain: &str, address: &str, native: bool, skip: &HashSet<String>, extra: &[String]) -> Option<Vec<Holding>> {
    match chain {
        "tempo" => return tempo(address).await,
        "hyperevm" => return from_node(&HYPEREVM, &[], address).await,
        "bnb" => return from_node(&BNB, extra, address).await,
        _ => {}
    }
    let Some(host) = lookup(&BLOCKSCOUT, chain) else { return Some(vec![]) };
    let link = format!("https://{host}/api/v2/addresses/{address}");
    let account = async {
        if !native { return None }
        let link = link.as_str();
        Some(retrying(|| async move { object(get(link, &BROWSER, 15).await) }).await)
    };
    let (pages, account) = tokio::join!(token_pages(host, address), account);
    let mut out = evm_tokens(chain, &pages?, skip);
    match account {
        Some(None) => return None,
        Some(Some(a)) => out.extend(native_coin(chain, &a)),
        None => {}
    }
    // DexScreener gives every token its day's move, and the ones CoinGecko doesn't list their price and pool, so the
    // scam check has something to go on. It takes 30 contracts a call; the priced ones go first.
    // ponytail: 20 calls at most (600 tokens); past that the rest stays listed under «hidden», unpriced.
    let asked: Vec<String> = out.iter().filter(|t| t.price.is_some()).chain(out.iter().filter(|t| t.price.is_none()))
        .filter(|t| !t.native).map(|t| t.contract.clone()).collect();
    for start in (0..asked.len().min(600)).step_by(30) {
        let chunk = &asked[start..(start + 30).min(asked.len())];
        let answer = get(&format!("https://api.dexscreener.com/tokens/v1/{chain}/{}", chunk.join(",")), &[], 15).await;
        out = dex_prices(out, answer.as_deref());
    }
    Some(llama_priced(chain, out).await)
}

/// Dollar prices DefiLlama gives these contracts (lowercased keys), and whether it's sure (confidence ≥ 0.9). It prices
/// a vault's or a lending market's shares by what's under them — Fluid's fUSDC, Compound's cUSDCv3, Moonwell's mUSDC,
/// Spark's sUSDC — which no market list or pool does.
pub async fn llama_prices(chain: &str, contracts: &[String]) -> HashMap<String, (Decimal, bool)> {
    let mut out = HashMap::new();
    let Some(name) = lookup(&LLAMA_CHAIN, chain) else { return out };
    for start in (0..contracts.len().min(200)).step_by(50) {
        let chunk = &contracts[start..(start + 50).min(contracts.len())];
        let coins = chunk.iter().map(|c| format!("{name}:{c}")).collect::<Vec<_>>().join(",");
        if let Some(data) = get(&format!("https://coins.llama.fi/prices/current/{coins}"), &[], 15).await {
            out.extend(parse_llama_prices(&data));
        }
    }
    out
}

/// A `prices/current` answer: each coin's price by its contract, lowercased, when DefiLlama is at least half sure.
pub fn parse_llama_prices(data: &[u8]) -> HashMap<String, (Decimal, bool)> {
    let mut out = HashMap::new();
    let Some(coins) = json(data).as_ref().and_then(|j| dict_of_dicts(j.get("coins"))) else { return out };
    for (key, c) in coins {
        let Some(p) = number(c.get("price")) else { continue };
        let confidence = c.get("confidence").and_then(Value::as_f64).unwrap_or(0.0);
        if confidence < 0.5 { continue }
        out.insert(key.split(':').rfind(|s| !s.is_empty()).unwrap_or("").to_lowercase(), (p, confidence >= 0.9));
    }
    out
}

/// The tokens no list or pool priced, priced by DefiLlama; a sure price makes a position real (not an airdrop's
/// look-alike), so a deposit in any vault shows with its worth.
pub async fn llama_priced(chain: &str, tokens: Vec<Holding>) -> Vec<Holding> {
    let unpriced: Vec<String> = tokens.iter().filter(|t| t.price.is_none() && !t.native).map(|t| t.contract.clone()).collect();
    if unpriced.is_empty() || lookup(&LLAMA_CHAIN, chain).is_none() { return tokens }
    let prices = llama_prices(chain, &unpriced).await;
    tokens.into_iter().map(|mut t| {
        if t.price.is_none() && let Some(&(price, sure)) = prices.get(&t.contract.to_lowercase()) {
            t.price = Some(price);
            if sure { t.verified = true }
        }
        t
    }).collect()
}

/// The wallet's changes in one token on an EVM network (CostBasis::replay); None off Blockscout's networks or when it
/// didn't answer. Any token standard: Robinhood's stock tokens are ERC-8056, not ERC-20.
// ponytail: the 50 newest transfers, one transaction at a time; what was held before them counts at the price then.
pub async fn token_changes(chain: &str, address: &str, contract: &str) -> Option<Vec<Change>> {
    let host = lookup(&BLOCKSCOUT, chain)?;
    let link = format!("https://{host}/api/v2/addresses/{address}/token-transfers?token={contract}");
    let link = link.as_str();
    let list = retrying(|| async move { object(get(link, &BROWSER, 15).await) }).await?;
    let mut transactions = vec![];
    for h in transaction_hashes(&strict_dicts(list.get("items"))?) {
        transactions.push(strict_dicts(transaction_transfers(host, &h).await?.get("items"))?);
    }
    Some(CostBasis::replay(address, contract, &transactions))
}

/// The transactions behind a list of transfers, oldest first, each once.
pub fn transaction_hashes(items: &[Value]) -> Vec<String> {
    let mut hashes: Vec<String> = vec![];
    for item in items.iter().rev() {
        if let Some(h) = item.get("transaction_hash").and_then(Value::as_str) && !hashes.iter().any(|x| x == h) { hashes.push(h.into()) }
    }
    hashes
}

async fn transaction_transfers(host: &str, hash: &str) -> Option<Value> {
    let link = format!("https://{host}/api/v2/transactions/{hash}/token-transfers");
    let link = link.as_str();
    retrying(|| async move { object(get(link, &BROWSER, 15).await) }).await
}

/// The wallet's changes in its network's own coin (ETH, POL), from Blockscout's coin balance history, oldest first:
/// coin in with stablecoins leaving the wallet in the same transaction was bought for them (a swap for ETH); gas and
/// sends went out. None off Blockscout's networks or when it didn't answer.
// ponytail: the 50 newest balance changes; what was held before them counts at the price then.
pub async fn coin_changes(chain: &str, address: &str) -> Option<Vec<Change>> {
    let host = lookup(&BLOCKSCOUT, chain)?;
    let link = format!("https://{host}/api/v2/addresses/{address}/coin-balance-history");
    let link = link.as_str();
    let list = retrying(|| async move { object(get(link, &BROWSER, 15).await) }).await?;
    let mut out = vec![];
    for (at, tokens, hash) in balance_changes(&strict_dicts(list.get("items"))?) {
        let mut paid = None;
        if let Some(hash) = hash && let Some(tx) = transaction_transfers(host, &hash).await {
            let dollars = CostBasis::dollars_out(&dicts(tx.get("items")), address);
            paid = (dollars > Decimal::ZERO).then_some(dollars);
        }
        out.push(Change { at, tokens, paid });
    }
    Some(out)
}

/// A coin balance history's items, oldest first: when, the coins in (+) or out (−), and for coins in the transaction
/// to look for dollars paid in.
pub fn balance_changes(items: &[Value]) -> Vec<(Date, Decimal, Option<String>)> {
    items.iter().rev().filter_map(|item| {
        let coins = amount(item.get("delta"), 18).filter(|w| !w.is_zero())?;
        let at = item.get("block_timestamp").and_then(Value::as_str).and_then(iso8601_fraction).unwrap_or_else(Utc::now);
        let hash = item.get("transaction_hash").and_then(Value::as_str).filter(|_| coins > Decimal::ZERO).map(String::from);
        Some((at, coins, hash))
    }).collect()
}

/// The NEAR account's changes in each of its tokens, by contract, from NearBlocks' token transfers (the 50 newest),
/// oldest first: a token in with NEAR's stablecoins leaving in the same transaction was bought for them. None when it
/// didn't answer.
pub async fn near_token_changes(account: &str) -> Option<HashMap<String, Vec<Change>>> {
    let link = format!("https://api.nearblocks.io/v1/account/{account}/ft-txns?per_page=50");
    let link = link.as_str();
    let list = retrying(|| async move { object(get(link, &[], 15).await) }).await?;
    parse_near_token_changes(&list, account)
}

/// NearBlocks' `ft-txns` answer as each token's changes.
pub fn parse_near_token_changes(list: &Value, account: &str) -> Option<HashMap<String, Vec<Change>>> {
    let txns = strict_dicts(list.get("txns"))?;
    let stables: HashSet<&str> = stablecoins::on("near").unwrap_or_default().iter().map(|c| c.address).collect();
    let moved: Vec<(&str, &str, Date, Decimal)> = txns.iter().filter_map(|t| {
        if t.get("affected_account_id").and_then(Value::as_str) != Some(account) { return None }
        let ft = t.get("ft")?.as_object()?;
        let contract = ft.get("contract")?.as_str()?;
        let moved = amount(t.get("delta_amount"), int_value(ft.get("decimals"))?)?;
        let ns = number(t.get("block_timestamp"))?;
        let at = unix(double(ns.checked_div(Decimal::from(1_000_000_000))?));
        Some((t.get("transaction_hash").and_then(Value::as_str).unwrap_or(""), contract, at, moved))
    }).collect();
    let mut out: HashMap<String, Vec<Change>> = HashMap::new();
    for &(hash, contract, at, amount) in moved.iter().rev() {
        if stables.contains(contract) || amount.is_zero() { continue }
        let dollars = -moved.iter().filter(|m| m.0 == hash && stables.contains(m.1) && m.3 < Decimal::ZERO).fold(Decimal::ZERO, |s, m| s.saturating_add(m.3));
        let paid = (amount > Decimal::ZERO && dollars > Decimal::ZERO).then_some(dollars);
        out.entry(contract.into()).or_default().push(Change { at, tokens: amount, paid });
    }
    Some(out)
}

const INFO: &str = "https://api.hyperliquid.xyz/info";

/// What Hyperliquid itself says each spot token held cost (its `entryNtl`), by the token's name; None when it couldn't
/// be read.
pub async fn hyperliquid_entries(user: &str) -> Option<HashMap<String, CostBasis>> {
    let body = json!({"type": "spotClearinghouseState", "user": user});
    let body = &body;
    let state = retrying(|| async move { post(INFO, body).await }).await?;
    Some(parse_hyperliquid_entries(&state))
}

pub fn parse_hyperliquid_entries(state: &[u8]) -> HashMap<String, CostBasis> {
    let mut out = HashMap::new();
    for b in dicts(json(state).as_ref().and_then(|j| j.get("balances"))) {
        let (Some(coin), Some(total), Some(entry)) = (b.get("coin").and_then(Value::as_str), number(b.get("total")), number(b.get("entryNtl"))) else { continue };
        let mut basis = CostBasis::default();
        basis.bought(total, entry, Utc::now());
        basis.since = None; // Hyperliquid doesn't say when
        out.insert(coin.to_string(), basis);
    }
    out
}

/// DefiLlama's name for a coin, for its past prices: a network's own coin by its CoinGecko id, a token by
/// «network:address»; None where it has none (Robinhood Chain, Tempo, Hyperliquid's spot book).
pub fn llama_id(chain: &str, contract: &str) -> Option<String> {
    match (chain, contract) {
        ("solana", CostBasis::WSOL) => Some("coingecko:solana".into()),
        ("solana", _) => Some(format!("solana:{contract}")),
        ("near", "native") => Some("coingecko:near".into()),
        ("near", _) => Some(format!("near:{contract}")),
        ("polygon", "native") => Some("coingecko:polygon-ecosystem-token".into()),
        ("bnb", "native") => Some("coingecko:binancecoin".into()),
        ("hyperevm", "native") => Some("coingecko:hyperliquid".into()),
        ("tempo", _) | ("hyperliquid", _) => None,
        (_, "native") => Some("coingecko:ethereum".into()),
        _ => lookup(&LLAMA_CHAIN, chain).map(|n| format!("{n}:{}", contract.to_lowercase())),
    }
}

/// What DefiLlama says `coin` was worth on each of `days`: the nearest price within four hours; a day it has none for
/// is left out. None when it didn't answer.
pub async fn past_prices(coin: &str, days: &[Date]) -> Option<HashMap<Date, Decimal>> {
    if days.is_empty() { return Some(HashMap::new()) }
    let link = past_prices_link(coin, days);
    let link = link.as_str();
    let data = retrying(|| async move { get(link, &[], 20).await }).await?;
    parse_past_prices(&data, coin, days)
}

/// `batchHistorical`'s address: each day as its Unix second.
pub fn past_prices_link(coin: &str, days: &[Date]) -> String {
    let mut asked = Map::new();
    asked.insert(coin.into(), days.iter().map(|d| seconds(*d) as i64).collect());
    let asked = Value::Object(asked).to_string();
    llm::http::with_query("https://coins.llama.fi/batchHistorical", &[("coins", &asked), ("searchWidth", "4h")])
}

pub fn parse_past_prices(data: &[u8], coin: &str, days: &[Date]) -> Option<HashMap<Date, Decimal>> {
    let coins = dict_of_dicts(json(data)?.get("coins"))?;
    let prices: Vec<(f64, Decimal)> = dicts(coins.get(coin).and_then(|c| c.get("prices"))).iter()
        .filter_map(|p| Some((p.get("timestamp")?.as_f64()?, number(p.get("price"))?)))
        .collect();
    let mut out = HashMap::new();
    for &d in days {
        let t = seconds(d);
        let near = prices.iter().min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(&(at, px)) = near && (at - t).abs() <= 4.0 * 3600.0 { out.insert(d, px); }
    }
    Some(out)
}

// MARK: Networks read from their nodes

/// Tempo's dollars, read from its node (it has no Blockscout): USDC — what Relay brings and services charge — and
/// pathUSD and USDT0, Tempo's other stablecoins. Tempo has no coin of its own. With the icons Relay publishes for them
/// (its /chains, 2026-09-30).
const TEMPO_STABLES: [(&str, &str, &str); 3] = [
    ("0x20c000000000000000000000b9537d11c60e8b50", "USDC", "https://assets.relay.link/icons/currencies/usdc.png"),
    ("0x20c0000000000000000000000000000000000000", "pathUSD", "https://assets.relay.link/icons/currencies/pathUSD.png"),
    ("0x20c00000000000000000000014f22ca97301eb73", "USDT0", "https://coin-images.coingecko.com/coins/images/53705/large/usdt0.jpg"),
];

/// None when the node didn't answer.
async fn tempo(address: &str) -> Option<Vec<Holding>> {
    let owner = address.to_lowercase().replace("0x", "");
    if count(&owner) != 40 { return Some(vec![]) }
    let mut out = vec![];
    for (contract, symbol, icon) in TEMPO_STABLES {
        let amount = units(&hex_units(&node_answer("https://rpc.mainnet.tempo.xyz", &eth_call(contract, &balance_of(&owner))).await?), 6).unwrap_or_default();
        if amount > Decimal::ZERO {
            out.push(Holding { icon: url(icon), price: Some(Decimal::ONE), verified: true, ..Holding::new("tempo", contract, amount, symbol) });
        }
    }
    Some(out)
}

/// A dollar stablecoin read straight from its network's node (the app's fresh read before a move:
/// Workspace.freshenSource). Swift's default `symbol` is «USDC».
pub fn dollar(chain: &str, contract: &str, amount: Decimal, symbol: &str, icon: Option<String>) -> Holding {
    Holding { icon, price: Some(Decimal::ONE), verified: true, ..Holding::new(chain, contract, amount, symbol) }
}

/// A network read straight from its node: its coin, a fixed list of tokens (contract, symbol, decimals, icon), the coin
/// and its wrapped token priced by Relay, dollars at 1, the rest by DexScreener's `dex`.
struct Node {
    chain: &'static str,
    url: &'static str,
    coin: &'static str,
    icon: &'static str,
    relay_chain: u64,
    wrapped: &'static str,
    dex: &'static str,
    tokens: &'static [(&'static str, &'static str, i64, &'static str)],
}

const USDE_ICON: &str = "https://coin-images.coingecko.com/coins/images/33613/large/usde.png";

/// HyperEVM, read from its node (its explorers have no API ihatecrypto can use): HYPE itself, USDC — what Relay brings —
/// USD₮0 and WHYPE, HYPE priced by Relay, with the icons Relay publishes (its /chains, 2026-10-06). Unit's UBTC, UETH
/// and USOL and Kinetiq's kHYPE too, the coins ihatecrypto swaps there (EVMTokens.hyperevm), priced by DexScreener.
const HYPEREVM: Node = Node {
    chain: "hyperevm", url: "https://hyperliquid.drpc.org", coin: "HYPE",
    icon: "https://assets.coingecko.com/coins/images/50882/standard/hyperliquid.jpg", relay_chain: relay::chain::HYPEREVM, wrapped: "WHYPE",
    dex: "hyperevm",
    tokens: &[
        ("0xb88339cb7199b77e23db6e890353e22632ba630f", "USDC", 6, "https://coin-images.coingecko.com/coins/images/6319/large/usdc.png"),
        ("0xb8ce59fc3717ada4c02eadf9682a9e934f625ebb", "USD₮0", 6, "https://coin-images.coingecko.com/coins/images/53705/large/usdt0.jpg"),
        ("0x5d3a1ff2b6bab83b63cd9ad0787074081a52ef34", "USDe", 18, USDE_ICON),
        ("0x5555555555555555555555555555555555555555", "WHYPE", 18, "https://coin-images.coingecko.com/coins/images/54469/large/_UP3jBsi_400x400.jpg"),
        ("0x9fdbda0a5e284c32744d2f17ee5c74b284993463", "UBTC", 8, ""),
        ("0xbe6727b535545c67d5caa73dea54865b92cf7907", "UETH", 18, ""),
        ("0x068f321fa8fb9f0d135f290ef6a3e2813e1c8a29", "USOL", 9, ""),
        ("0xfd739d4e423301ce9385c1fb8850539d657c296d", "kHYPE", 18, ""),
    ],
};

/// BNB Chain, read from its node too (its explorers want an API key): BNB itself, Binance-Peg USDC and USDT (18
/// decimals), WBNB, the coins ihatecrypto names there (EVMTokens.bnb), and any token bought here (`extra`: an address
/// with no explorer would lose it otherwise). BNB priced by Relay, the rest by DexScreener (contracts read 2026-10-08).
const BNB: Node = Node {
    chain: "bnb", url: "https://bsc-rpc.publicnode.com", coin: "BNB", icon: "https://assets.relay.link/icons/currencies/bnb.png",
    relay_chain: relay::chain::BNB, wrapped: "WBNB", dex: "bsc",
    tokens: &[
        ("0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d", "USDC", 18, "https://coin-images.coingecko.com/coins/images/6319/large/usdc.png"),
        ("0x55d398326f99059ff775485246999027b3197955", "USDT", 18, "https://coin-images.coingecko.com/coins/images/39963/large/usdt.png"),
        ("0x5d3a1ff2b6bab83b63cd9ad0787074081a52ef34", "USDe", 18, USDE_ICON),
        ("0x1af3f329e8be154074d8769d1ffa4ee058b1dbc3", "DAI", 18, "https://coin-images.coingecko.com/coins/images/9956/large/Badge_Dai.png"),
        ("0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c", "WBNB", 18, "https://assets.relay.link/icons/currencies/bnb.png"),
        ("0x0e09fabb73bd3ade0a17ecc321fd13a19e81ce82", "CAKE", 18, ""),
        ("0x7130d2a12b9bcbfae4f2634d864a1ee1ce3ead9c", "BTCB", 18, ""),
        ("0x2170ed0880ac9a755fd29b2688956bd959f933f8", "ETH", 18, ""),
    ],
};

/// The coin, the fixed tokens and `extra` contracts (their symbol and decimals read too). None when the node didn't
/// answer.
async fn from_node(n: &Node, extra: &[String], address: &str) -> Option<Vec<Holding>> {
    let owner = address.to_lowercase().replace("0x", "");
    if count(&owner) != 40 { return Some(vec![]) }
    let price = relay::native_price(n.relay_chain).await.ok().flatten();
    let coins = units(&hex_units(&node_answer(n.url, &json!({"jsonrpc": "2.0", "id": 1, "method": "eth_getBalance", "params": [address, "latest"]})).await?), 18).unwrap_or_default();
    let mut out = vec![];
    if coins > Decimal::ZERO {
        out.push(Holding {
            icon: url(n.icon), price, verified: true, native: true, ..Holding::new(n.chain, "native", coins, n.coin)
        });
    }
    let mut list: Vec<(String, String, i64, String)> = n.tokens.iter().map(|t| (t.0.into(), t.1.into(), t.2, t.3.into())).collect();
    let extra: BTreeSet<String> = extra.iter().map(|c| c.to_lowercase()).filter(|c| !n.tokens.iter().any(|t| *c == t.0)).collect();
    for contract in extra {
        let Some(d) = node_answer(n.url, &eth_call(&contract, "0x313ce567")).await.map(|h| hex_units(&h)) else { continue };
        let Some(raw) = node_answer(n.url, &eth_call(&contract, "0x95d89b41")).await else { continue };
        let Some(symbol) = abi_symbol(&raw, &contract) else { continue };
        list.push((contract, symbol, d.parse().unwrap_or(i64::MAX), String::new()));
    }
    for (contract, symbol, decimals, icon) in list {
        let raw = hex_units(&node_answer(n.url, &eth_call(&contract, &balance_of(&owner))).await?);
        if let Some(amount) = units(&raw, decimals).filter(|a| *a > Decimal::ZERO) {
            let price = if symbol == n.wrapped { price } else if stablecoins::DOLLARS.contains(&symbol) { Some(Decimal::ONE) } else { None };
            out.push(Holding { icon: url(&icon), price, verified: true, ..Holding::new(n.chain, &contract, amount, &symbol) });
        }
    }
    // The swapped coins (UBTC, CAKE…) priced by their deepest pool; nothing asked when none is held.
    let unpriced: Vec<&str> = out.iter().filter(|h| h.price.is_none()).map(|h| h.contract.as_str()).collect();
    if !unpriced.is_empty() {
        let answer = get(&format!("https://api.dexscreener.com/tokens/v1/{}/{}", n.dex, unpriced.join(",")), &[], 15).await;
        out = dex_prices(out, answer.as_deref());
    }
    Some(out)
}

fn eth_call(to: &str, data: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "eth_call", "params": [{"to": to, "data": data}, "latest"]})
}

/// `balanceOf(owner)`'s calldata; `owner` without its «0x».
fn balance_of(owner: &str) -> String { format!("0x70a08231{}{owner}", "0".repeat(24)) }

/// A node's `result` text, asked up to three times; None when it never answered with a JSON object or had no result.
async fn node_answer(node: &str, call: &Value) -> Option<String> {
    retrying(|| async move { object(post(node, call).await) }).await?.get("result")?.as_str().map(String::from)
}

/// A quantity in hex as the node writes it («0x…», the first two characters dropped) as decimal digits for `units`, a
/// character that isn't a hex digit counting 0.
// ponytail: "0" past u128 (3.4·10^38 base units, Swift's goes to 10^165): no token supply runs that high.
pub fn hex_units(hex: &str) -> String {
    hex.chars().skip(2).try_fold(0u128, |v, c| v.checked_mul(16)?.checked_add(c.to_digit(16).unwrap_or(0).into())).unwrap_or(0).to_string()
}

/// `symbol()`'s answer, an ABI string (offset, length, bytes); the contract's first 8 characters when the length runs
/// past the answer, None when it isn't one.
pub fn abi_symbol(raw: &str, contract: &str) -> Option<String> {
    let bytes = wallet::from_hex(&raw.chars().skip(2).collect::<String>())?;
    if bytes.len() < 64 { return None }
    let word = &bytes[32..64];
    let length = word[..24].iter().all(|b| *b == 0).then(|| u64::from_be_bytes(word[24..].try_into().expect("8 bytes")));
    Some(match length {
        Some(l) if l <= (bytes.len() - 64) as u64 => String::from_utf8_lossy(&bytes[64..64 + l as usize]).into_owned(),
        _ => chars(contract).into_iter().take(8).collect(),
    })
}

// MARK: Blockscout's lists

/// Every page of the address's ERC-20s. Blockscout serves 50 at a time, the most valuable first, and says where the
/// next page starts; a page that fails is asked again (the API throws the odd 500). None when even the first page never
/// came; a later one that never comes ends the list there, losing only the least valuable tail.
// ponytail: 20 pages at most (1000 tokens): past that the address is a spam dump and the tail is unpriced dust.
async fn token_pages(host: &str, address: &str) -> Option<Vec<Value>> {
    let mut items = vec![];
    let mut next = Some(Map::new());
    for page in 0..20 {
        let Some(params) = next else { break };
        let link = page_link(host, address, &params);
        let link = link.as_str();
        let Some(body) = retrying(|| async move { object(get(link, &BROWSER, 15).await) }).await else {
            return if page == 0 { None } else { Some(items) };
        };
        items.extend(dicts(body.get("items")));
        next = body.get("next_page_params").and_then(Value::as_object).cloned();
    }
    Some(items)
}

/// A page's address: ERC-20s only, then the next page's parameters as Blockscout gave them, by name.
pub fn page_link(host: &str, address: &str, params: &Map<String, Value>) -> String {
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    let values: Vec<(&str, String)> = keys.into_iter().filter_map(|k| {
        let v = match &params[k] {
            Value::Null => return None,
            Value::String(s) => s.clone(),
            Value::Bool(b) => u8::from(*b).to_string(),
            v => v.to_string(),
        };
        Some((k.as_str(), v))
    }).collect();
    let mut items = vec![("type", "ERC-20")];
    items.extend(values.iter().map(|(k, v)| (*k, v.as_str())));
    llm::http::with_query(&format!("https://{host}/api/v2/addresses/{address}/tokens"), &items)
}

/// Blockscout's token items as holdings, but for the contracts in `skip`.
pub fn evm_tokens(chain: &str, items: &[Value], skip: &HashSet<String>) -> Vec<Holding> {
    items.iter().filter_map(|item| {
        let t = item.get("token")?.as_object()?;
        let contract = t.get("address_hash")?.as_str()?.to_lowercase();
        if skip.contains(&contract) { return None }
        let decimals: i64 = t.get("decimals").and_then(Value::as_str).unwrap_or("").parse().ok()?;
        let held = amount(item.get("value"), decimals).filter(|r| *r > Decimal::ZERO)?;
        let rate = number(t.get("exchange_rate"));
        let symbol = if lookup(&BRIDGED_USDC, chain) == Some(contract.as_str()) { "USDC.e" } else { t.get("symbol").and_then(Value::as_str).unwrap_or("?") };
        Some(Holding {
            icon: t.get("icon_url").and_then(Value::as_str).and_then(url), price: rate,
            verified: rate.is_some() && t.get("reputation").and_then(Value::as_str) != Some("scam"),
            ..Holding::new(chain, &contract, held, symbol)
        })
    }).collect()
}

/// The address's own coin from Blockscout's address answer: POL on Polygon, ETH everywhere else.
pub fn native_coin(chain: &str, a: &Value) -> Option<Holding> {
    let coins = amount(a.get("coin_balance"), 18).filter(|w| *w > Decimal::ZERO)?;
    let polygon = chain == "polygon";
    Some(Holding {
        icon: if polygon { None } else { url(ETH_ICON) }, price: number(a.get("exchange_rate")), verified: true, native: true,
        ..Holding::new(chain, "native", coins, if polygon { "POL" } else { "ETH" })
    })
}

/// The deepest pool per token from DexScreener's pairs: its day's move for every token, its price and pool for an
/// unpriced one (a CoinGecko rate stays).
pub fn dex_prices(tokens: Vec<Holding>, data: Option<&[u8]>) -> Vec<Holding> {
    let mut best: HashMap<String, (Decimal, Decimal, Option<Decimal>)> = HashMap::new();
    for pair in dicts(data.and_then(json).as_ref()) {
        let Some(base) = pair.get("baseToken").and_then(|b| b.get("address")).and_then(Value::as_str).map(str::to_lowercase) else { continue };
        let Some(price) = number(pair.get("priceUsd")) else { continue };
        let liquidity = number(pair.get("liquidity").and_then(|l| l.get("usd"))).unwrap_or_default();
        if liquidity >= best.get(&base).map_or(Decimal::NEGATIVE_ONE, |b| b.1) {
            best.insert(base, (price, liquidity, number(pair.get("priceChange").and_then(|c| c.get("h24")))));
        }
    }
    tokens.into_iter().map(|mut t| {
        if let Some(&(price, liquidity, change)) = best.get(&t.contract) {
            t.change24h = t.change24h.or(change);
            if t.price.is_none() {
                t.price = Some(price);
                t.liquidity = Some(liquidity);
            }
        }
        t
    }).collect()
}

// MARK: Hyperliquid spot

/// Everything on Hyperliquid's spot side, USDC included (the perp account doesn't count it), priced off its own book.
/// A unified account's USDC is the account itself (Hyperliquid.balance counts it), so it isn't listed again. None when
/// it couldn't be read.
pub async fn hyperliquid_spot(user: &str) -> Option<Vec<Holding>> {
    let (state, meta) = (json!({"type": "spotClearinghouseState", "user": user}), json!({"type": "spotMetaAndAssetCtxs"}));
    let (state, meta) = (&state, &meta);
    let (s, m, one) = tokio::join!(
        retrying(|| async move { post(INFO, state).await }),
        retrying(|| async move { post(INFO, meta).await }),
        retrying(|| async move { crate::network::Hyperliquid::unified(user).await.ok() }),
    );
    let (s, m, unified) = (s?, m?, one?);
    Some(parse_hyperliquid_spot(&s, &m).into_iter().filter(|h| !(unified && h.contract == "USDC")).collect())
}

pub fn parse_hyperliquid_spot(state: &[u8], meta: &[u8]) -> Vec<Holding> {
    let balances = dicts(json(state).as_ref().and_then(|j| j.get("balances")));
    let pair = json(meta).and_then(|j| j.as_array().cloned()).unwrap_or_default();
    let universe = dicts(pair.first().and_then(|p| p.get("universe")));
    let contexts = if pair.len() > 1 { dicts(Some(&pair[1])) } else { vec![] };
    balances.iter().filter_map(|b| {
        let coin = b.get("coin")?.as_str()?;
        let token = b.get("token")?.as_i64()?;
        let amount = number(b.get("total")).filter(|a| *a > Decimal::ZERO)?;
        if token == 0 { return Some(Holding { price: Some(Decimal::ONE), verified: true, ..Holding::new("hyperliquid", coin, amount, "USDC") }) }
        // The token's market against USDC (token 0) on the spot book.
        let market = universe.iter().find(|u| {
            u.get("tokens").and_then(Value::as_array).and_then(|t| t.iter().map(Value::as_i64).collect::<Option<Vec<_>>>()) == Some(vec![token, 0])
        });
        let context = market.and_then(|m| m.get("index")?.as_i64()).and_then(|i| usize::try_from(i).ok()).and_then(|i| contexts.get(i));
        let field = |k: &str| context.and_then(|c| number(c.get(k)));
        let price = field("midPx").or_else(|| field("markPx"));
        let before = field("prevDayPx");
        let change = price.zip(before).filter(|(_, b)| *b > Decimal::ZERO).and_then(|(p, b)| (p - b).checked_div(b)?.checked_mul(Decimal::ONE_HUNDRED));
        // HYPE is Hyperliquid's own coin, vouched for like ETH on its networks (a spot name belongs to one token).
        Some(Holding { price, verified: coin == "HYPE", liquidity: field("dayNtlVlm"), change24h: change, ..Holding::new("hyperliquid", coin, amount, coin) })
    }).collect()
}

// MARK: NEAR

/// Fungible tokens on the NEAR account, wNEAR aside (counted with NEAR). Ref's price list names and prices them; a
/// token it doesn't list is named by its own contract's metadata, all of them asked at once. None when the account's
/// list or the prices couldn't be read.
pub async fn near(account: &str) -> Option<Vec<Holding>> {
    let link = format!("https://api.fastnear.com/v1/account/{account}/ft");
    let link = link.as_str();
    let (list, prices) = tokio::join!(
        retrying(|| async move { get(link, &[], 15).await }),
        retrying(|| async { get("https://indexer.ref.finance/list-token-price", &[], 15).await }),
    );
    let (listed, priced) = (list?, prices?);
    let listed_by_ref = json(&priced).as_ref().and_then(|j| dict_of_dicts(Some(j))).unwrap_or_default();
    let mut group = JoinSet::new();
    for (contract, _) in near_balances(&listed) {
        if listed_by_ref.contains_key(&contract) { continue }
        group.spawn(async move { (near_metadata(&contract).await, contract) });
    }
    let mut metadata = HashMap::new();
    while let Some(Ok((m, contract))) = group.join_next().await {
        if let Some(m) = m { metadata.insert(contract, m); }
    }
    Some(parse_near(&listed, &priced, &metadata))
}

/// FastNEAR's `ft` list: each contract and its raw balance, wNEAR and empty ones left out.
pub fn near_balances(data: &[u8]) -> Vec<(String, String)> {
    dicts(json(data).as_ref().and_then(|j| j.get("tokens"))).iter().filter_map(|t| {
        let c = t.get("contract_id")?.as_str().filter(|c| *c != "wrap.near")?;
        let raw = match t.get("balance")? { Value::String(s) => s.clone(), n @ Value::Number(_) => n.to_string(), _ => return None };
        units(&raw, 0).filter(|r| *r > Decimal::ZERO)?;
        Some((c.to_string(), raw))
    }).collect()
}

/// The account's tokens named and priced by Ref's list (`prices`), else by their own metadata (symbol, decimals).
pub fn parse_near(list: &[u8], prices: &[u8], metadata: &HashMap<String, (String, i64)>) -> Vec<Holding> {
    let listed_by_ref = json(prices).as_ref().and_then(|j| dict_of_dicts(Some(j))).unwrap_or_default();
    near_balances(list).into_iter().filter_map(|(contract, raw)| {
        let listed = listed_by_ref.get(&contract);
        let decimals = listed.and_then(|l| l.get("decimal")?.as_i64()).or_else(|| metadata.get(&contract).map(|m| m.1))?;
        let symbol = listed.and_then(|l| l.get("symbol")?.as_str()).filter(|s| !s.is_empty()).map(String::from)
            .or_else(|| metadata.get(&contract).map(|m| m.0.clone()))
            .unwrap_or_else(|| chars(&contract).into_iter().take(8).collect());
        let amount = units(&raw, decimals).or_else(|| shifted(decimal(&raw)?, decimals))?;
        let price = listed.and_then(|l| number(l.get("price")));
        // Ref lists junk too: its price counts as a check only when the holding is worth a cent.
        let worth = price.unwrap_or_default().checked_mul(amount).unwrap_or_default();
        Some(Holding { price, verified: worth >= Decimal::new(1, 2), ..Holding::new("near", &contract, amount, &symbol) })
    }).collect()
}

async fn near_metadata(contract: &str) -> Option<(String, i64)> {
    let params = json!({"request_type": "call_function", "finality": "final", "account_id": contract, "method_name": "ft_metadata", "args_base64": "e30="});
    let data = post("https://rpc.mainnet.fastnear.com", &json!({"jsonrpc": "2.0", "id": 1, "method": "query", "params": params})).await?;
    parse_near_metadata(&data, contract)
}

/// `ft_metadata`'s answer: the result's bytes are the metadata's JSON.
pub fn parse_near_metadata(data: &[u8], contract: &str) -> Option<(String, i64)> {
    let bytes: Vec<u8> = json(data)?.get("result")?.get("result")?.as_array()?.iter()
        .map(|b| b.as_i64().and_then(|b| u8::try_from(b).ok())).collect::<Option<_>>()?;
    let m = json(&bytes)?;
    let decimals = m.get("decimals")?.as_i64()?;
    Some((m.get("symbol").and_then(Value::as_str).map_or_else(|| chars(contract).into_iter().take(8).collect(), String::from), decimals))
}

// MARK: Helpers

/// `HTTP.get`: the body of a 2xx answer.
async fn get(url: &str, headers: &[(&str, &str)], seconds: u64) -> Option<Vec<u8>> { llm::http::get(url, headers, seconds).await.ok() }

/// `HTTP.post(_:json:)`: the body of a 2xx answer.
async fn post(url: &str, body: &Value) -> Option<Vec<u8>> { llm::http::post_json(url, body, &[], 15).await.ok() }

pub(crate) fn json(data: &[u8]) -> Option<Value> { serde_json::from_slice(data).ok() }

/// `json(data) as? [String: Any]`.
fn object(data: Option<Vec<u8>>) -> Option<Value> { json(&data?).filter(Value::is_object) }

/// `as? [[String: Any]]`: an array of objects, None when it isn't one.
pub(crate) fn strict_dicts(v: Option<&Value>) -> Option<Vec<Value>> {
    let a = v?.as_array()?;
    a.iter().all(Value::is_object).then(|| a.clone())
}

/// `as? [[String: Any]] ?? []`.
pub(crate) fn dicts(v: Option<&Value>) -> Vec<Value> { strict_dicts(v).unwrap_or_default() }

/// `as? [String: [String: Any]]`.
fn dict_of_dicts(v: Option<&Value>) -> Option<Map<String, Value>> {
    let o = v?.as_object()?;
    o.values().all(Value::is_object).then(|| o.clone())
}

/// `(v as? NSNumber)?.intValue`.
pub(crate) fn int_value(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_bool().map(i64::from))
}

/// `URL(string:)` of an icon's address: none for an empty one.
fn url(s: &str) -> Option<String> { (!s.is_empty()).then(|| s.into()) }

/// Up to three tries, a second apart: public APIs drop the odd request.
async fn retrying<T, F: Future<Output = Option<T>>>(mut fetch: impl FnMut() -> F) -> Option<T> {
    for attempt in 0..3 {
        if let Some(value) = fetch().await { return Some(value) }
        if attempt < 2 { tokio::time::sleep(Duration::from_secs(1)).await }
    }
    None
}

/// A raw amount the API sent over 10^decimals: a whole number's text past Decimal's 28 digits too (`units`), anything
/// else as `number` reads it.
pub(crate) fn amount(v: Option<&Value>, decimals: i64) -> Option<Decimal> {
    match v? {
        Value::String(s) => units(s, decimals).or_else(|| shifted(decimal(s)?, decimals)),
        other => shifted(number(Some(other))?, decimals),
    }
}

/// A number the API sent, through its text: a JSON double's decimalValue turns 1.88 into 1.8799999….
pub(crate) fn number(v: Option<&Value>) -> Option<Decimal> {
    match v? {
        Value::String(s) => decimal(s),
        Value::Bool(b) => Some(Decimal::from(u8::from(*b))),
        n @ Value::Number(_) => json_decimal(n),
        _ => None,
    }
}

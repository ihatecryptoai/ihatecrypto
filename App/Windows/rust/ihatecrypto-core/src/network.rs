//! What the networks say (`Network.swift`): Hyperliquid's markets, ATR and balances, NEAR's balance, transaction
//! context and send, and the bridges' dry quotes for a plan. Each call goes through `net`; the reading of each answer
//! is its own function, testable offline.

use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wallet::keys::base58;
use wallet::{Result, WalletError};

use crate::foundation::{Num, Rounding, decimal, text};
use crate::intent::Fmt;
use crate::plan::{BridgeQuote, Market};

fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

/// `dec(_:)`: a number the API sent as text, or «Unexpected number format».
pub(crate) fn dec(s: Option<&str>) -> Result<Decimal> {
    match s.and_then(decimal) { Some(d) => Ok(d), None => fail(format!("Unexpected number format: {}", s.unwrap_or("nil"))) }
}

/// What `JSONDecoder` says of an answer that isn't the shape asked for (its `localizedDescription`).
pub(crate) fn unreadable() -> WalletError { WalletError("The data couldn’t be read because it isn’t in the correct format.".into()) }

/// `JSONDecoder().decode(T.self, from:)`.
pub(crate) fn decode<T: DeserializeOwned>(data: &[u8]) -> Result<T> { serde_json::from_slice(data).map_err(|_| unreadable()) }

/// `HTTP.post(_:json:)`: the body of a 2xx answer, else «HTTP 429: …» with the body's first 300 bytes.
// ponytail: net's client waits 30 s; Swift's HTTP.post 15 s.
pub(crate) async fn post(url: &str, body: Value) -> Result<Vec<u8>> {
    let (data, status) = net::post_json(url, &body).await.map_err(|e| WalletError(e.to_string()))?;
    if !(200..300).contains(&status) {
        return fail(format!("HTTP {status}: {}", String::from_utf8_lossy(&data[..data.len().min(300)])));
    }
    Ok(data)
}

/// `"\(x)"` of a JSON value: a string as is, anything else as JSON.
// ponytail: Swift prints a dictionary's description («["name": …]»); JSON here, the same words inside.
fn describe(v: &Value) -> String { v.as_str().map_or_else(|| v.to_string(), str::to_string) }

// MARK: - Hyperliquid (read-only `info`)

pub struct Hyperliquid;

/// Perp and spot are separate balances on HyperCore; they are shown apart, never summed.
#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    pub account_value: Decimal,
    pub withdrawable: Decimal,
    pub spot_usdc: Decimal,
    /// A unified account or portfolio margin: one USDC balance on spot carries perps, HIP-3 exchanges and spot.
    /// account_value and withdrawable are read from it, spot_usdc is 0 (the same money), nothing moves between them.
    pub unified: bool,
}

#[derive(Deserialize)]
struct Summary {
    #[serde(rename = "accountValue")]
    account_value: String,
}

#[derive(Deserialize)]
struct Perp {
    #[serde(rename = "marginSummary")]
    margin_summary: Summary,
    withdrawable: String,
}

#[derive(Deserialize)]
struct SpotRow {
    coin: String,
    total: String,
    hold: Option<String>,
}

#[derive(Deserialize)]
struct Spot {
    balances: Vec<SpotRow>,
}

impl Hyperliquid {
    pub const INFO: &str = "https://api.hyperliquid.xyz/info";

    /// HIP-3 exchanges whose markets ihatecrypto shows: xyz carries the liquid commodities and stocks (oil, gold,
    /// Tesla…); the others had no volume on 2026-09-25.
    pub const HIP3_DEXES: [&str; 1] = ["xyz"];

    /// An address Hyperliquid has never seen: its first deposit pays a one-time fee for opening the account.
    pub async fn is_new_account(address: &str) -> Result<bool> {
        Self::parse_new_account(&post(Self::INFO, json!({"type": "userRole", "user": address})).await?)
    }

    /// An answer without a role is an error: read as «not new», a send to a new account lost its 1 USDC. Any role but
    /// «missing» (user, agent, vault, subAccount, one added later) is an account Hyperliquid knows.
    pub fn parse_new_account(data: &[u8]) -> Result<bool> {
        let role = serde_json::from_slice::<Value>(data).ok().and_then(|j| j.get("role")?.as_str().map(str::to_string)).filter(|r| !r.is_empty());
        match role {
            Some(r) => Ok(r == "missing"),
            None => fail(format!("Hyperliquid's userRole answered without a role: {}", String::from_utf8_lossy(&data[..data.len().min(200)]))),
        }
    }

    /// All live perp markets keyed by name: the main exchange ("ETH") and HIP-3 ones ("xyz:CL").
    pub async fn markets() -> Result<HashMap<String, Market>> {
        let (main, order) = tokio::join!(Self::markets_on(None, 0), post(Self::INFO, json!({"type": "perpDexs"})));
        let mut out = main?;
        // Asset ids of HIP-3 markets depend on the exchange's position in perpDexs.
        let dexes = order.map(|d| Self::dex_names(&d)).unwrap_or_default();
        for dex in Self::HIP3_DEXES {
            let Some(i) = dexes.iter().position(|d| d.as_deref() == Some(dex)) else { continue };
            let Ok(extra) = Self::markets_on(Some(dex), i as i64).await else { continue };
            for (name, m) in extra { out.entry(name).or_insert(m); }
        }
        Ok(out)
    }

    /// perpDexs' names in order: nil for the main exchange (its null) and for anything that isn't one.
    pub fn dex_names(data: &[u8]) -> Vec<Option<String>> {
        let list = serde_json::from_slice::<Value>(data).ok().and_then(|j| j.as_array().cloned()).unwrap_or_default();
        list.iter().map(|d| d.get("name").and_then(Value::as_str).map(str::to_string)).collect()
    }

    pub async fn markets_on(dex: Option<&str>, dex_index: i64) -> Result<HashMap<String, Market>> {
        let mut body = json!({"type": "metaAndAssetCtxs"});
        if let Some(dex) = dex { body["dex"] = json!(dex) }
        Self::parse_markets(&post(Self::INFO, body).await?, dex, dex_index)
    }

    /// metaAndAssetCtxs' two-element array, [meta, contexts]: delisted markets and markets with no price left out.
    pub fn parse_markets(data: &[u8], dex: Option<&str>, dex_index: i64) -> Result<HashMap<String, Market>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Asset {
            name: String,
            sz_decimals: i64,
            max_leverage: i64,
            is_delisted: Option<bool>,
        }
        #[derive(Deserialize)]
        struct Meta {
            universe: Vec<Asset>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Ctx {
            mark_px: Option<String>,
            prev_day_px: Option<String>,
            funding: Option<String>,
            day_ntl_vlm: Option<String>,
        }
        let parts: Vec<Value> = decode(data)?;
        let (Some(meta), Some(ctxs)) = (parts.first(), parts.get(1)) else { return Err(unreadable()) };
        let meta: Meta = serde_json::from_value(meta.clone()).map_err(|_| unreadable())?;
        let ctxs: Vec<Ctx> = serde_json::from_value(ctxs.clone()).map_err(|_| unreadable())?;
        let mut out = HashMap::new();
        for (index, (asset, ctx)) in meta.universe.into_iter().zip(ctxs).enumerate() {
            if asset.is_delisted == Some(true) { continue }
            let Some(px) = ctx.mark_px.as_deref().and_then(decimal) else { continue };
            let index = index as i64;
            let mut m = Market::new(&asset.name, px, asset.sz_decimals, asset.max_leverage);
            m.index = if dex.is_none() { index } else { 100_000 + dex_index * 10_000 + index };
            m.prev_day_px = ctx.prev_day_px.as_deref().and_then(decimal);
            m.funding = ctx.funding.as_deref().and_then(decimal);
            m.dex = dex.map(str::to_string);
            m.volume24h = ctx.day_ntl_vlm.as_deref().and_then(decimal).unwrap_or_default();
            out.insert(asset.name, m);
        }
        Ok(out)
    }

    /// ATR(14) of 4-hour candles as % of the last close: the market's ordinary move, for stops.
    pub async fn atr_pct(coin: &str, now: crate::foundation::Date) -> Result<Decimal> {
        let end = (crate::foundation::seconds(now) * 1000.0) as i64;
        let body = json!({"type": "candleSnapshot", "req": {"coin": coin, "interval": "4h", "startTime": end - 16 * 4 * 3_600_000, "endTime": end}});
        Self::parse_atr(&post(Self::INFO, body).await?, coin)
    }

    pub fn parse_atr(data: &[u8], coin: &str) -> Result<Decimal> {
        #[derive(Deserialize)]
        struct Candle {
            h: String,
            l: String,
            c: String,
        }
        let candles: Vec<Candle> = decode(data)?;
        let bars = candles.iter().map(|b| Ok((dec(Some(&b.h))?, dec(Some(&b.l))?, dec(Some(&b.c))?))).collect::<Result<Vec<_>>>()?;
        if bars.len() < 15 { return fail(format!("Not enough {coin} candles for ATR.")) }
        let ranges: Vec<Num> = (1..bars.len())
            .map(|i| {
                let (h, l, prev) = (Num::of(bars[i].0), Num::of(bars[i].1), Num::of(bars[i - 1].2));
                [h - l, (h - prev).abs(), (l - prev).abs()].into_iter().fold(Num::of(Decimal::MIN), |a, b| match (a.0, b.0) {
                    (Some(x), Some(y)) => Num::of(x.max(y)),
                    _ => Num(None),
                })
            })
            .collect();
        let sum = ranges[ranges.len() - 14..].iter().fold(Num::of(Decimal::ZERO), |a, &b| a + b);
        // A sum past range or a last close of 0 (NaN in Swift) reads as an unreadable answer.
        (sum / 14 / bars[bars.len() - 1].2 * 100).0.ok_or_else(unreadable)
    }

    /// The account's abstraction mode (userAbstraction): «unifiedAccount» and «portfolioMargin» keep every balance and
    /// hold in the spot state. «disabled» (Manual) and «default» keep perp and spot apart; so does a reply that isn't a
    /// mode.
    pub async fn unified(user: &str) -> Result<bool> {
        Self::parse_unified(&post(Self::INFO, json!({"type": "userAbstraction", "user": user})).await?)
    }

    pub fn parse_unified(data: &[u8]) -> Result<bool> {
        let mode: Value = decode(data)?;
        Ok(matches!(mode.as_str(), Some("unifiedAccount" | "portfolioMargin")))
    }

    /// A HIP-3 exchange's own margin balance: account value and what is free. Nothing in a unified account, whose money
    /// is all on the one balance `balance` reads.
    pub async fn dex_balance(user: &str, dex: &str) -> Result<(Decimal, Decimal)> {
        let (one, perp) = tokio::join!(Self::unified(user), post(Self::INFO, json!({"type": "clearinghouseState", "user": user, "dex": dex})));
        let perp: Perp = decode(&perp?)?;
        if one? { return Ok((Decimal::ZERO, Decimal::ZERO)) }
        Ok((dec(Some(&perp.margin_summary.account_value))?, dec(Some(&perp.withdrawable))?))
    }

    /// In a unified account the spot USDC is the whole account (positions' profit and loss included) and what open
    /// positions and orders hold of it is its `hold`.
    // ponytail: portfolio margin's other collateral (HYPE, BTC) isn't counted here, and a USDC loan reads as 0; the
    // spot tokens show in the portfolio on their own. Its `hold` goes negative by what can be borrowed against them:
    // loan room, not free money, so «free» stays within the account.
    pub async fn balance(user: &str) -> Result<Balance> {
        let (perp, spot, one) = tokio::join!(
            post(Self::INFO, json!({"type": "clearinghouseState", "user": user})),
            post(Self::INFO, json!({"type": "spotClearinghouseState", "user": user})),
            Self::unified(user),
        );
        let (perp, spot) = (perp?, spot?);
        let (perp, spot): (Perp, Spot) = (decode(&perp)?, decode(&spot)?);
        Self::balance_from(perp, spot, one?)
    }

    /// `balance` from the two states' answers and the account's mode.
    pub fn parse_balance(perp: &[u8], spot: &[u8], unified: bool) -> Result<Balance> { Self::balance_from(decode(perp)?, decode(spot)?, unified) }

    fn balance_from(perp: Perp, spot: Spot, unified: bool) -> Result<Balance> {
        let usdc = spot.balances.iter().find(|r| r.coin == "USDC");
        if unified {
            let total = dec(Some(usdc.map_or("0", |u| &u.total)))?.max(Decimal::ZERO);
            let hold = dec(Some(usdc.and_then(|u| u.hold.as_deref()).unwrap_or("0")))?;
            let free = (Num::of(total) - hold).0.ok_or_else(unreadable)?.max(Decimal::ZERO);
            return Ok(Balance { account_value: total, withdrawable: total.min(free), spot_usdc: Decimal::ZERO, unified: true });
        }
        Ok(Balance {
            account_value: dec(Some(&perp.margin_summary.account_value))?,
            withdrawable: dec(Some(&perp.withdrawable))?,
            spot_usdc: dec(Some(usdc.map_or("0", |u| &u.total)))?,
            unified: false,
        })
    }
}

// MARK: - Solana (public RPC)

/// Solana over its public nodes.
// pending: `Solana.Balance`, `balance(owner:lendShares:)` and `holdings` (round 5): their tokens are Holdings.swift's
// `Holding`.
pub struct Solana;

impl Solana {
    pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
}

// MARK: - NEAR

/// The NEAR chain: native NEAR and wNEAR on an account. 1Click delivers «wrap.near», so both are counted.
pub struct Near;

/// What a new transaction from an account needs: the next nonce of its key, a recent final block hash, and the
/// account's native balance and storage (yoctoNEAR, bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct TxContext {
    pub nonce: u64,
    pub block_hash: Vec<u8>,
    pub balance: String,
    pub storage_bytes: u64,
    pub gas_price: String,
}

impl Near {
    pub const RPC: &str = "https://rpc.mainnet.fastnear.com";

    async fn rpc(method: &str, params: Value) -> Result<Vec<u8>> {
        post(Self::RPC, json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})).await
    }

    pub async fn balance(account: &str) -> Result<(Decimal, Decimal)> {
        let view = Self::parse_query(&Self::rpc("query", json!({"request_type": "view_account", "finality": "final", "account_id": account})).await?)?;
        let args = STANDARD.encode(json!({"account_id": account}).to_string());
        let call = Self::parse_query(&Self::rpc("query", json!({"request_type": "call_function", "finality": "final", "account_id": "wrap.near",
                                                                  "method_name": "ft_balance_of", "args_base64": args})).await?)?;
        Ok(Self::amounts(view.as_ref(), call.as_ref()))
    }

    /// A query's result; None for an account that doesn't exist yet (an implicit account exists only after NEAR first
    /// arrives, and before that it simply holds nothing).
    pub fn parse_query(data: &[u8]) -> Result<Option<Value>> {
        let j: Value = serde_json::from_slice(data).map_err(|_| unreadable())?;
        if let Some(e) = j.get("error").filter(|e| e.is_object()) {
            let said = e.to_string();
            if said.contains("UNKNOWN_ACCOUNT") || said.contains("does not exist") { return Ok(None) }
            return fail(format!("NEAR RPC: {}", describe(e.get("message").unwrap_or(e))));
        }
        Ok(j.get("result").filter(|r| r.is_object()).cloned())
    }

    /// NEAR and wNEAR in whole coins from view_account's result and ft_balance_of's (the JSON string «"123"» as bytes).
    pub fn amounts(view: Option<&Value>, call: Option<&Value>) -> (Decimal, Decimal) {
        let yocto = crate::foundation::pow10(24);
        let native = view.and_then(|v| v.get("amount")?.as_str()).and_then(decimal).unwrap_or_default();
        // `as? [Int]` then UInt8: anything else reads as nothing.
        let bytes: Option<Vec<u8>> = call.and_then(|c| c.get("result")?.as_array())
            .and_then(|r| r.iter().map(|b| b.as_i64().and_then(|b| u8::try_from(b).ok())).collect());
        let text = String::from_utf8_lossy(&bytes.unwrap_or_default()).into_owned();
        let wrapped = decimal(text.trim_matches('"')).unwrap_or_default();
        (native / yocto, wrapped / yocto)
    }

    /// The key is the implicit account's own.
    pub async fn tx_context(account: &str) -> Result<TxContext> {
        let raw = wallet::from_hex(account).filter(|r| r.len() == 32);
        let Some(raw) = raw else { return fail(format!("Not a NEAR implicit account: {account}.")) };
        let public_key = format!("ed25519:{}", base58::encode(&raw));
        let key = Self::result("query", &Self::rpc("query", json!({"request_type": "view_access_key", "finality": "final", "account_id": account,
                                                                     "public_key": public_key})).await?)?;
        let view = Self::result("query", &Self::rpc("query", json!({"request_type": "view_account", "finality": "final", "account_id": account})).await?)?;
        let block = Self::result("block", &Self::rpc("block", json!({"finality": "final"})).await?)?;
        Self::context(&key, &view, &block)
    }

    /// A call's result object, or what NEAR said.
    pub fn result(method: &str, data: &[u8]) -> Result<Value> {
        let j: Option<Value> = serde_json::from_slice(data).ok().filter(Value::is_object);
        if let Some(r) = j.as_ref().and_then(|j| j.get("result")).filter(|r| r.is_object()) { return Ok(r.clone()) }
        let error = j.as_ref().and_then(|j| j.get("error")).map_or_else(|| "no result".into(), describe);
        // No NEAR ever came: the account isn't there to sign from.
        if error.contains("UNKNOWN_ACCOUNT") || error.contains("does not exist while viewing") {
            return fail("This NEAR account doesn't exist yet: NEAR opens it with the first NEAR it receives. Send it ≈ 0.1 NEAR, then try again.");
        }
        fail(format!("NEAR RPC {method}: {error}"))
    }

    pub fn context(key: &Value, view: &Value, block: &Value) -> Result<TxContext> {
        let header = block.get("header");
        let (Some(nonce), Some(hash), Some(balance), Some(storage)) = (
            key.get("nonce").and_then(Value::as_u64),
            header.and_then(|h| h.get("hash")?.as_str()),
            view.get("amount").and_then(Value::as_str),
            view.get("storage_usage").and_then(Value::as_u64),
        ) else {
            return fail("NEAR RPC: no nonce, block hash or balance.");
        };
        let gas_price = header.and_then(|h| h.get("gas_price")?.as_str()).unwrap_or("100000000");
        Ok(TxContext {
            nonce: nonce.checked_add(1).ok_or_else(unreadable)?,
            block_hash: base58::decode(hash)?,
            balance: balance.into(),
            storage_bytes: storage,
            gas_price: gas_price.into(),
        })
    }

    /// NEAR's «NotEnoughBalance» (the fee) or «LackBalanceForState» (what the account must keep for its storage) in
    /// words, with what to do; None for anything else.
    pub fn short_of_near(error: &str) -> Option<String> {
        if !error.contains("NotEnoughBalance") && !error.contains("LackBalanceForState") { return None }
        Some("Not enough NEAR: the account keeps NEAR for its storage and pays the fee in NEAR. Send it ≈ 0.1 NEAR, then try again.".into())
    }

    /// Broadcasts a borsh SignedTransaction and waits until it has executed; an error with NEAR's reason on failure.
    /// Returns the transaction hash (base58).
    pub async fn send(signed: &[u8]) -> Result<String> {
        let data = Self::rpc("send_tx", json!({"signed_tx_base64": STANDARD.encode(signed), "wait_until": "EXECUTED_OPTIMISTIC"})).await?;
        Self::parse_sent(&data)
    }

    pub fn parse_sent(data: &[u8]) -> Result<String> {
        let j: Option<Value> = serde_json::from_slice(data).ok().filter(Value::is_object);
        let Some(result) = j.as_ref().and_then(|j| j.get("result")).filter(|r| r.is_object()) else {
            let error = j.as_ref().and_then(|j| j.get("error")).map_or_else(|| "no result".into(), describe);
            return fail(Self::short_of_near(&error).unwrap_or(format!("NEAR RPC send_tx: {error}")));
        };
        if let Some(failure) = result.get("status").and_then(|s| s.get("Failure")) {
            let said = describe(failure);
            return fail(Self::short_of_near(&said).unwrap_or(format!("The NEAR transaction failed: {said}")));
        }
        Ok(result.get("transaction").and_then(|t| t.get("hash")?.as_str()).unwrap_or("?").into())
    }
}

// MARK: - The bridges' dry quotes for a plan (execution lives in `protocols`)

pub struct Bridge;

/// `arrive` = exact USDC to land on HyperCore; `send` = exact USDC to spend from Solana.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Amount {
    Arrive(Decimal),
    Send(Decimal),
}

/// The EVM network the USDC leaves from instead of Solana: its chain, USDC contract and that USDC's decimals.
#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    pub chain: u64,
    pub usdc: String,
    pub decimals: u32,
}

impl Bridge {
    // Dry quotes do not depend on addresses, so the user's own addresses are not sent.
    pub const PLACEHOLDER_REFUND: &str = "So11111111111111111111111111111111111111112";
    pub const PLACEHOLDER_RECIPIENT: &str = "0x0000000000000000000000000000000000000001";

    /// Both bridges at once. The app bridges trades through Relay, so Relay's price is the plan's (the signature asks
    /// for it); 1Click is shown for comparison and stands in only when Relay doesn't answer. `new_account`: the user's
    /// Hyperliquid account doesn't exist yet, so the quote goes to a fresh random address and carries the same one-time
    /// opening fee the real deposit will. `origin`: the USDC leaves an EVM network instead — Relay only, straight to
    /// Hyperliquid. `one_click_first`: the user chose to open a new account through 1Click.
    pub async fn dry_quote(amount: Amount, new_account: bool, origin: Option<&Origin>, one_click_first: bool) -> Result<BridgeQuote> {
        let recipient = if new_account {
            let mut b = [0u8; 20];
            getrandom::fill(&mut b).expect("the system's randomness");
            format!("0x{}", wallet::to_hex(&b))
        } else {
            Self::PLACEHOLDER_RECIPIENT.into()
        };
        let (relay, one_click) = tokio::join!(async { Self::relay_quote(amount, &recipient, origin).await.ok() }, async {
            if origin.is_none() { Self::one_click_quote(amount, &recipient).await.ok() } else { None }
        });
        let options: Vec<BridgeQuote> = if one_click_first { [one_click, relay] } else { [relay, one_click] }.into_iter().flatten().collect();
        let mut options = options.into_iter();
        let Some(mut best) = options.next() else { return fail("No bridge quotes: Relay and 1Click didn't respond.") };
        if let Some(other) = options.next() {
            best.alternative = Some(format!("{}: send {} → receive {}, {}", other.provider, Fmt::num(other.amount_in, 2), Fmt::num(other.amount_out, 2),
                                            other.time.to_lowercase()));
            best.alternative_fee = (Num::of(other.amount_in) - other.reserve - other.amount_out).0;
        }
        best.opening = new_account;
        Ok(best)
    }

    /// Relay's dry quote: Hyperliquid perp USDC has 8 decimals in Relay; USDC on Solana and the EVM networks 6, BNB
    /// Chain's 18.
    pub async fn relay_quote(amount: Amount, recipient: &str, origin: Option<&Origin>) -> Result<BridgeQuote> {
        use protocols::relay::{self, chain, currency};
        let (exact_output, units) = match amount {
            Amount::Arrive(v) => (true, Num::of(v) * Decimal::from(100_000_000)),
            Amount::Send(v) => (false, (0..origin.map_or(6, |o| o.decimals)).fold(Num::of(v), |a, _| a * 10)),
        };
        let units = units.rounded(0, Rounding::Up).0.ok_or_else(unreadable)?;
        let q = relay::quote(&relay::Ask {
            user: if origin.is_none() { Self::PLACEHOLDER_REFUND } else { Self::PLACEHOLDER_RECIPIENT }.into(),
            recipient: recipient.into(),
            origin_chain: origin.map_or(chain::SOLANA, |o| o.chain),
            destination_chain: chain::HYPERLIQUID,
            origin_currency: origin.map_or(currency::SOLANA_USDC, |o| &o.usdc).into(),
            destination_currency: currency::HYPERLIQUID_USDC.into(),
            amount: 0,
            exact_output,
            units: Some(text(units)),
            ..Default::default()
        })
        .await?;
        // Measured 2026-09-25: 4 s from send to credit on Hyperliquid from Solana; from an EVM network, Relay's estimate.
        Ok(BridgeQuote::new("Relay", dec(Some(&q.amount_in_formatted))?, Decimal::ZERO, dec(Some(&q.amount_out_formatted))?,
                            &if origin.is_none() { "Seconds: 4 s to credit in live runs.".into() } else {
                                format!("Seconds: ≈ {} s by Relay's estimate.", q.seconds.max(1))
                            }, None))
    }

    /// 1Click's dry quote, Solana USDC (6 decimals) to HyperCore USDC (8).
    pub async fn one_click_quote(amount: Amount, recipient: &str) -> Result<BridgeQuote> {
        use protocols::one_click::{self, asset};
        let (exact_output, units) = match amount {
            Amount::Arrive(v) => (true, Num::of(v) * Decimal::from(100_000_000)),
            Amount::Send(v) => (false, Num::of(v) * Decimal::from(1_000_000)),
        };
        let units = units.rounded(0, Rounding::Up).0.ok_or_else(unreadable)?;
        // EXACT_OUTPUT adds the slippage tolerance to amountIn; 0.1% is enough for USDC → USDC.
        let mut ask = one_click::Ask::new(true, asset::SOLANA_USDC, asset::HYPER_CORE_USDC, &text(units), Self::PLACEHOLDER_REFUND, recipient);
        ask.exact_output = exact_output;
        ask.slippage_bps = 10;
        Self::one_click_from(&one_click::quote(&ask).await?)
    }

    /// 1Click estimates ~30 s; live runs took 114–237 s from send to credit.
    pub fn one_click_from(q: &protocols::one_click::Quote) -> Result<BridgeQuote> {
        let amount_in = dec(Some(&q.amount_in_formatted))?;
        let price_in = match &q.min_amount_in { Some(m) => dec(Some(m))? / Decimal::from(1_000_000), None => amount_in };
        let reserve = (Num::of(amount_in) - price_in).0.ok_or_else(unreadable)?.max(Decimal::ZERO);
        Ok(BridgeQuote::new("NEAR 1Click", amount_in, reserve, dec(Some(&q.amount_out_formatted))?, "Usually 2–4 min.", None))
    }
}

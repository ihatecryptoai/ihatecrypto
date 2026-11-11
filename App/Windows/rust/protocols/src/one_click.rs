//! NEAR Intents 1Click: cross-chain swaps and bridges as quote → deposit → status (`OneClick.swift`). Works without a
//! partner key, so there is no server of our own.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde_json::{Value, json};
use wallet::{Result, WalletError};

use crate::{dec, post, s};

pub const BASE: &str = "https://1click.chaindefuser.com/v0";

/// `OneClickError`: what 1Click said, in its own words.
pub fn error(body: &[u8]) -> WalletError { WalletError(format!("1Click: {}", net::said(body, 0))) }

static KEY: Mutex<Option<String>> = Mutex::new(None);

/// The person's partner key: a JWT from partners.near-intents.org, sent as X-API-Key on every call. Without one 1Click
/// adds 0.25% to every quote; with one it charges its partner fees. Set at launch and from Settings; "" is none.
pub fn key() -> Option<String> { KEY.lock().expect("key").clone() }
pub fn set_key(key: Option<String>) { *KEY.lock().expect("key") = key.filter(|k| !k.is_empty()) }

/// The headers of a request to 1Click: the partner key when there is one.
pub fn headers() -> Vec<(&'static str, String)> { key().map(|k| vec![("X-API-Key", k)]).unwrap_or_default() }

fn headers_ref<'a>(h: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> { h.iter().map(|(k, v)| (*k, v.as_str())).collect() }

async fn get(url: &str) -> std::result::Result<net::Answer, reqwest::Error> {
    let h = headers();
    net::get(url, &headers_ref(&h)).await
}

/// A token 1Click swaps: its asset id, network (its `blockchain`: «sol», «base», «near»…), symbol, decimals, contract.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub asset_id: String,
    pub blockchain: String,
    pub symbol: String,
    pub decimals: i64,
    pub contract: Option<String>,
    pub price: Option<Decimal>,
}

static CACHED: Mutex<Option<(Instant, Vec<Token>)>> = Mutex::new(None);

/// Every token 1Click swaps (/v0/tokens), read again after ten minutes.
pub async fn tokens() -> Result<Vec<Token>> {
    if let Some((at, list)) = CACHED.lock().expect("tokens").as_ref() {
        if at.elapsed() < Duration::from_secs(600) { return Ok(list.clone()) }
    }
    let (data, _) = get(&format!("{BASE}/tokens")).await.map_err(|e| WalletError(e.to_string()))?;
    let list = parse_tokens(&data);
    if list.is_empty() { return Err(error(&data)) }
    *CACHED.lock().expect("tokens") = Some((Instant::now(), list.clone()));
    Ok(list)
}

pub fn parse_tokens(data: &[u8]) -> Vec<Token> {
    let Ok(Value::Array(list)) = serde_json::from_slice::<Value>(data) else { return vec![] };
    list.iter().filter_map(|t| Some(Token {
        asset_id: s(t, "assetId")?.into(), blockchain: s(t, "blockchain")?.into(), symbol: s(t, "symbol")?.into(),
        decimals: t.get("decimals")?.as_f64()? as i64, contract: s(t, "contractAddress").map(str::to_lowercase),
        // NSNumber's doubleValue as a Decimal.
        price: t.get("price").and_then(Value::as_f64).and_then(Decimal::from_f64),
    })).collect()
}

pub mod asset {
    pub const SOLANA_SOL: &str = "nep141:sol.omft.near";
    pub const SOLANA_USDC: &str = "nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near";
    /// Lands on the HyperCore spot balance (hip1 token), not directly in perp margin.
    pub const HYPER_CORE_USDC: &str = "1cs_v1:hypercore:hip1:0x6d1e7cde53ba9467b783cb7c530ce054";
    /// NEAR on the NEAR chain (listed as wNEAR, 24 decimals).
    pub const NEAR: &str = "nep141:wrap.near";

    /// Circle's USDC on the EVM networks 1Click quotes it from, by ihatecrypto's key.
    pub fn usdc_on_evm(key: &str) -> Option<&'static str> {
        Some(match key {
            "base" => "nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near",
            "arbitrum" => "nep141:arb-0xaf88d065e77c8cc2239327c5edb3a432268e5831.omft.near",
            "ethereum" => "nep141:eth-0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.omft.near",
            _ => return None,
        })
    }

    /// What 1Click pays out on networks ihatecrypto has no wallet on, by the address book's network: (ticker, asset, decimals).
    pub fn across(network: &str) -> &'static [(&'static str, &'static str, u32)] {
        match network {
            "bitcoin" => &[("BTC", "nep141:btc.omft.near", 8)],
            "zcash" => &[("ZEC", "nep141:zec.omft.near", 8)],
            "tron" => &[("USDT", "nep141:tron-d28a265909efecdcee7c5028585214ea0b96f015.omft.near", 6), ("TRX", "nep141:tron.omft.near", 6)],
            "xrp" => &[("XRP", "nep141:xrp.omft.near", 6)],
            "dogecoin" => &[("DOGE", "nep141:doge.omft.near", 8)],
            "sui" => &[("SUI", "nep141:sui.omft.near", 9), ("USDC", "nep141:sui-c1b81ecaf27933252d31a963bc5e9458f13c18ce.omft.near", 6)],
            _ => &[],
        }
    }

    /// What a private send can pay on `network`: the coins of a network ihatecrypto has no wallet on (`across`), or USDC
    /// on Solana and on the EVM networks 1Click takes it on: (ticker, asset, decimals).
    pub fn payout(symbol: &str, network: &str) -> Option<(&'static str, &'static str, u32)> {
        let up = symbol.to_uppercase();
        if let Some(a) = across(network).iter().find(|a| a.0 == up) { return Some(*a) }
        if up != "USDC" { return None }
        let id = if network == "solana" { Some(SOLANA_USDC) } else { usdc_on_evm(network) }?;
        Some(("USDC", id, 6))
    }
}

/// ISO 8601 UTC, whole seconds: what ISO8601DateFormatter writes.
fn deadline(minutes: f64) -> String {
    (chrono::Utc::now() + chrono::Duration::milliseconds((minutes * 60_000.0) as i64)).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

async fn post_json(path: &str, body: &Value) -> Result<Vec<u8>> {
    let (data, status) = post(&format!("{BASE}{path}"), body.to_string().into_bytes(), &headers(), None).await.map_err(|e| WalletError(e.to_string()))?;
    if (200..300).contains(&status) { Ok(data) } else { Err(error(&data)) }
}

/// Why 1Click won't take `key`, or None: a dry quote (1 USDC, Solana → Base) asked with it. A key it doesn't know answers
/// 401 «Invalid token».
pub async fn problem_with_key(key: &str) -> Option<String> {
    let body = json!({
        "dry": true, "swapType": "EXACT_INPUT", "slippageTolerance": 100, "originAsset": asset::SOLANA_USDC, "depositType": "ORIGIN_CHAIN",
        "destinationAsset": asset::usdc_on_evm("base").expect("base"), "amount": "1000000", "refundTo": "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm",
        "refundType": "ORIGIN_CHAIN", "recipient": "0x00000000000000000000000000000000000f0a11", "recipientType": "DESTINATION_CHAIN",
        "deadline": deadline(30.0),
    });
    let Ok((data, code)) = post(&format!("{BASE}/quote"), body.to_string().into_bytes(), &[("X-API-Key", key.into())], None).await else {
        return Some("1Click didn't answer. Try again in a minute.".into());
    };
    if code == 401 || code == 403 { return Some(format!("1Click turned the key away ({code}): copy the whole token from partners.near-intents.org again.")) }
    if (200..300).contains(&code) { None } else { Some(error(&data).0) }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quote {
    pub deposit_address: Option<String>,
    pub amount_in: String,
    pub amount_in_formatted: String,
    /// Present for EXACT_OUTPUT: the input without the slippage reserve.
    pub min_amount_in: Option<String>,
    pub amount_out_formatted: String,
    pub min_amount_out: Option<String>,
    pub deadline: Option<String>,
    pub time_estimate: Option<i64>,
    /// What goes in and what comes out, in dollars as 1Click prices them: their gap is its fee and the network's.
    pub amount_in_usd: Option<Decimal>,
    pub amount_out_usd: Option<Decimal>,
}

/// What /quote is asked. `dry: false` issues a deposit address; `refund_to` and `recipient` must be the person's own
/// addresses then. `flexible`: a deposit someone else sends (any amount from the minimum); `refund_to_intents`: the
/// refund to a NEAR Intents account; `minutes`: how long the deadline is.
#[derive(Debug, Clone)]
pub struct Ask {
    pub dry: bool,
    pub exact_output: bool,
    pub origin: String,
    pub destination: String,
    pub amount: String,
    pub refund_to: String,
    pub recipient: String,
    pub slippage_bps: i64,
    pub flexible: bool,
    pub refund_to_intents: bool,
    pub minutes: f64,
    /// Confidential Intents: the deposit and the payout can't be matched to each other on chain. Needs the partner key.
    pub confidential: bool,
}

impl Ask {
    /// The Swift defaults: 1% slippage, an exact input, 30 minutes.
    pub fn new(dry: bool, origin: &str, destination: &str, amount: &str, refund_to: &str, recipient: &str) -> Self {
        Ask { dry, exact_output: false, origin: origin.into(), destination: destination.into(), amount: amount.into(), refund_to: refund_to.into(),
              recipient: recipient.into(), slippage_bps: 100, flexible: false, refund_to_intents: false, minutes: 30.0, confidential: false }
    }

    pub fn body(&self) -> Value {
        let mut body = json!({
            "dry": self.dry, "swapType": if self.flexible { "FLEX_INPUT" } else if self.exact_output { "EXACT_OUTPUT" } else { "EXACT_INPUT" },
            "slippageTolerance": self.slippage_bps, "originAsset": self.origin, "depositType": "ORIGIN_CHAIN", "destinationAsset": self.destination,
            "amount": self.amount, "refundTo": self.refund_to, "refundType": if self.refund_to_intents { "INTENTS" } else { "ORIGIN_CHAIN" },
            "recipient": self.recipient, "recipientType": "DESTINATION_CHAIN", "deadline": deadline(self.minutes), "quoteWaitingTimeMs": 3000,
        });
        if self.confidential { body["confidentiality"] = json!("basic") }
        body
    }
}

pub async fn quote(ask: &Ask) -> Result<Quote> {
    if ask.confidential && key().is_none() {
        return Err(WalletError("A private send needs a NEAR Intents partner key: add it in Settings › General › NEAR Intents key.".into()));
    }
    parse_quote(&post_json("/quote", &ask.body()).await?)
}

pub fn parse_quote(data: &[u8]) -> Result<Quote> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    let Some(q) = j.as_ref().and_then(|j| j.get("quote")).filter(|q| q.is_object()) else { return Err(error(data)) };
    let Some(amount_in) = s(q, "amountIn") else { return Err(error(data)) };
    let opt = |k: &str| s(q, k).map(str::to_string);
    Ok(Quote {
        deposit_address: opt("depositAddress"), amount_in: amount_in.into(),
        amount_in_formatted: opt("amountInFormatted").unwrap_or_else(|| amount_in.into()), min_amount_in: opt("minAmountIn"),
        amount_out_formatted: opt("amountOutFormatted").unwrap_or_else(|| "?".into()), min_amount_out: opt("minAmountOut"),
        deadline: opt("deadline"), time_estimate: q.get("timeEstimate").and_then(Value::as_f64).map(|t| t as i64),
        amount_in_usd: s(q, "amountInUsd").and_then(dec), amount_out_usd: s(q, "amountOutUsd").and_then(dec),
    })
}

/// What 1Click has bound to a deposit address: recipient, refundTo, originAsset, destinationAsset, amount and the address
/// itself. Read back from 1Click itself, so an address slipped in from elsewhere does not match.
pub async fn bound_request(deposit_address: &str) -> Result<HashMap<String, String>> {
    let (data, _) = get(&format!("{BASE}/status?depositAddress={deposit_address}")).await.map_err(|e| WalletError(e.to_string()))?;
    parse_bound(&data)
}

pub fn parse_bound(data: &[u8]) -> Result<HashMap<String, String>> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    let Some(quote) = j.as_ref().and_then(|j| j.get("quoteResponse")).filter(|q| q.is_object()) else { return Err(error(data)) };
    let Some(request) = quote.get("quoteRequest").filter(|r| r.is_object()) else { return Err(error(data)) };
    let mut out: HashMap<String, String> = ["recipient", "refundTo", "originAsset", "destinationAsset", "amount", "confidentiality", "swapType"].iter()
        .filter_map(|k| Some((k.to_string(), s(request, k)?.to_string()))).collect();
    let q = quote.get("quote");
    // What 1Click's own quote takes for this deposit and until when: an MPP service's challenge may not ask more
    // (DepositGuard::charge).
    for (key, field) in [("depositAddress", "depositAddress"), ("quoteAmountIn", "amountIn"), ("quoteMinAmountIn", "minAmountIn"), ("deadline", "deadline")] {
        if let Some(v) = q.and_then(|q| s(q, field)) { out.insert(key.into(), v.into()); }
    }
    Ok(out)
}

/// Optional: tells 1Click about the deposit so it does not wait to notice it.
pub async fn submit_deposit(tx_hash: &str, deposit_address: &str) -> Result<()> {
    post_json("/deposit/submit", &json!({"txHash": tx_hash, "depositAddress": deposit_address})).await.map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub status: String,
    pub amount_out_formatted: Option<String>,
    pub refunded_formatted: Option<String>,
    pub destination_txs: Vec<String>,
}

impl Status {
    pub fn is_final(&self) -> bool { ["SUCCESS", "REFUNDED", "FAILED"].contains(&self.status.as_str()) }
}

pub async fn status(deposit_address: &str) -> Result<Status> {
    let (data, _) = get(&format!("{BASE}/status?depositAddress={deposit_address}")).await.map_err(|e| WalletError(e.to_string()))?;
    parse_status(&data)
}

pub fn parse_status(data: &[u8]) -> Result<Status> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    let Some(status) = j.as_ref().and_then(|j| s(j, "status")) else { return Err(error(data)) };
    let details = j.as_ref().and_then(|j| j.get("swapDetails"));
    let opt = |k: &str| details.and_then(|d| s(d, k)).map(str::to_string);
    let txs = details.and_then(|d| d.get("destinationChainTxHashes")).and_then(Value::as_array).into_iter().flatten()
        .filter_map(|t| s(t, "hash").map(str::to_string)).collect();
    Ok(Status { status: status.into(), amount_out_formatted: opt("amountOutFormatted"), refunded_formatted: opt("refundedAmountFormatted"), destination_txs: txs })
}

/// A finished swap as 1Click keeps it: what went in and came out, and for whom. For the cost of what the wallet got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub origin: String,
    pub destination: String,
    pub recipient: String,
    pub refund_to: String,
    pub amount_in: Decimal,
    pub amount_out: Decimal,
    pub in_usd: Decimal,
}

impl Settled {
    /// From /status; None unless it succeeded.
    pub fn new(j: &Value) -> Option<Self> {
        let d = j.get("swapDetails");
        let q = j.get("quoteResponse").and_then(|r| r.get("quoteRequest"));
        let d_dec = |k: &str| d.and_then(|d| s(d, k)).and_then(dec);
        let q_str = |k: &str| q.and_then(|q| s(q, k));
        if s(j, "status") != Some("SUCCESS") { return None }
        Some(Settled {
            amount_in: d_dec("amountInFormatted")?, amount_out: d_dec("amountOutFormatted")?,
            origin: q_str("originAsset")?.into(), destination: q_str("destinationAsset")?.into(),
            recipient: q_str("recipient").unwrap_or("").into(), refund_to: q_str("refundTo").unwrap_or("").into(),
            in_usd: d_dec("amountInUsd").unwrap_or_default(),
        })
    }

    /// The NEAR it moved for `account`: + bought, with the dollars it cost (USDC at its amount, another coin at its
    /// dollar value then); − sold. None: no NEAR of this account's in it.
    pub fn near(&self, account: &str) -> Option<(Decimal, Decimal)> {
        let usdc = self.origin == asset::SOLANA_USDC || ["base", "arbitrum", "ethereum"].iter().any(|n| asset::usdc_on_evm(n) == Some(self.origin.as_str()));
        if self.destination == asset::NEAR && self.recipient == account { return Some((self.amount_out, if usdc { self.amount_in } else { self.in_usd })) }
        if self.origin == asset::NEAR && self.refund_to == account { return Some((-self.amount_in, Decimal::ZERO)) }
        None
    }
}

/// None: 1Click answered and the swap isn't a success, or it doesn't know the address (its own 404). An error when it
/// didn't answer — an error code (sent as JSON), a page, JSON without a status — so a cost isn't counted without the
/// swap; a throttle (429) or a server error is asked again first.
pub async fn settled(deposit_address: &str) -> Result<Option<Settled>> { settled_at(BASE, deposit_address, &net::DELAYS).await }

pub async fn settled_at(base: &str, deposit_address: &str, delays: &[Duration]) -> Result<Option<Settled>> {
    let url = format!("{base}/status?depositAddress={deposit_address}");
    net::run(delays, |_| get(&url), |data, code| {
        let j: Option<Value> = serde_json::from_slice(data).ok().filter(Value::is_object);
        // Its own 404, not a gateway's page.
        if code == 404 && j.as_ref().and_then(|j| j.get("statusCode")).and_then(Value::as_i64) == Some(404) { return Ok(None) }
        match j {
            Some(j) if (200..300).contains(&code) && j.get("status").is_some_and(Value::is_string) => Ok(Settled::new(&j)),
            _ => Err(net::Again::new(code == 429 || code >= 500, error(data).0)),
        }
    }).await
}

// Limit orders (/v0/orders, checked 2026-11-11)

/// ihatecrypto's fee on a limit order: 5 basis points (0.05%, as on Hyperliquid) of what goes in, paid to the owner's
/// NEAR Intents account. 1Click deducts it from the input; the guard refuses an order that carries any other fee.
pub const ORDER_FEE: (&str, i64) = ("ihatecrypto.near", 5);

/// A limit order as 1Click keeps it, flattened for the guard: `orderType`, `side`, `baseAsset`, `quoteAsset`,
/// `quantity`, `price`, `depositAddress`, `depositType`, `recipient`, `recipientType`, `refundTo`, `refundType`,
/// `confidentiality`, `fillStatus`, `payoutStatus`, `deadline`, `depositedAmount`, the swap view's `swapType` and
/// `amountIn` / `maxAmountIn` / `minAmountOut` / `amountOut`, `filledIn` / `filledOut` (the fills' sums), and `appFees`
/// as «recipient:bps» joined by «,».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: String,
    pub fields: HashMap<String, String>,
}

impl Order {
    pub fn get(&self, key: &str) -> Option<&str> { self.fields.get(key).map(String::as_str) }
    /// Funded and resting, or waiting for its deposit: not filled, canceled or expired.
    pub fn is_open(&self) -> bool {
        ["AWAITING_DEPOSIT", "OPEN", "PARTIALLY_FILLED", "PENDING_CANCEL"].contains(&self.get("fillStatus").unwrap_or(""))
    }
}

/// One order out of 1Click's JSON:API `data` object; None when it isn't one.
pub fn parse_order(data: &Value) -> Option<Order> {
    let id = s(data, "id")?;
    let a = data.get("attributes").filter(|a| a.is_object())?;
    let mut f: HashMap<String, String> = ["orderType", "side", "baseAsset", "quoteAsset", "quantity", "price", "depositAddress", "depositMemo",
        "depositType", "recipient", "recipientType", "refundTo", "refundType", "confidentiality", "fillStatus", "payoutStatus", "deadline",
        "createdAt", "depositedAmount"].iter().filter_map(|k| Some((k.to_string(), s(a, k)?.to_string()))).collect();
    if let Some(v) = a.get("swapView").filter(|v| v.is_object()) {
        for k in ["swapType", "amountIn", "maxAmountIn", "minAmountOut", "amountOut"] {
            if let Some(x) = s(v, k) { f.insert(k.into(), x.into()); }
        }
    }
    if let Some(fin) = a.get("isPayoutStatusFinal").and_then(Value::as_bool) { f.insert("isPayoutStatusFinal".into(), fin.to_string()); }
    let fees: Vec<String> = a.get("appFees").and_then(Value::as_array).into_iter().flatten().map(|fee| {
        // NSNumber's intValue: a whole number, a fraction cut; anything else -1.
        let bps = fee.get("fee").and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|x| x as i64))).unwrap_or(-1);
        format!("{}:{bps}", s(fee, "recipient").unwrap_or("?"))
    }).collect();
    f.insert("appFees".into(), fees.join(","));
    let fills = a.get("partialFills").and_then(Value::as_array).cloned().unwrap_or_default();
    let sum = |k: &str| fills.iter().filter_map(|x| s(x, k).and_then(dec)).fold(Decimal::ZERO, |a, b| a.checked_add(b).unwrap_or(a)).normalize().to_string();
    f.insert("filledIn".into(), sum("amountIn"));
    f.insert("filledOut".into(), sum("amountOut"));
    Some(Order { id: id.into(), fields: f })
}

fn order_from(data: &[u8]) -> Result<Order> {
    serde_json::from_slice::<Value>(data).ok().and_then(|j| j.get("data").and_then(parse_order)).ok_or_else(|| error(data))
}

/// A limit order 1Click rests until the price comes, its deadline, or a cancel: `quantity` of the base in its smallest
/// units, `price` in quote per one whole base. Always confidential (1Click takes no other: «Only confidential orders are
/// supported»), funded from the origin chain (`depositAddress`), paid out at `recipient`, refunded to `refund_to`, with
/// ihatecrypto's fee. Needs the partner key: the order belongs to it, and only it reads or cancels the order.
#[allow(clippy::too_many_arguments)]
pub async fn create_order(sell: bool, base: &str, quote: &str, quantity: &str, price: &str, refund_to: &str, recipient: &str,
                          deadline: chrono::DateTime<chrono::Utc>) -> Result<Order> {
    if key().is_none() {
        return Err(WalletError("1Click: A limit order needs a NEAR Intents partner key: add it in Settings › General › NEAR Intents key.".into()));
    }
    let attributes = json!({
        "orderType": "LIMIT", "baseAsset": base, "quoteAsset": quote, "quantity": quantity, "side": if sell { "SELL" } else { "BUY" }, "price": price,
        "depositType": "ORIGIN_CHAIN", "refundTo": refund_to, "refundType": "ORIGIN_CHAIN", "recipient": recipient,
        "recipientType": "DESTINATION_CHAIN", "confidentiality": "basic", "deadline": deadline.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "appFees": [{"recipient": ORDER_FEE.0, "fee": ORDER_FEE.1}],
    });
    order_from(&post_json("/orders", &json!({"data": {"type": "orders", "attributes": attributes}})).await?)
}

/// An order id is opaque: anything but letters, digits and `-_.~` is escaped, so it stays one path segment.
pub fn order_path(id: &str) -> String {
    id.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The order as 1Click holds it now: what the guard checks before the deposit, and its fills after.
pub async fn order(id: &str) -> Result<Order> {
    let (data, code) = get(&format!("{BASE}/orders/{}", order_path(id))).await.map_err(|e| WalletError(e.to_string()))?;
    if !(200..300).contains(&code) { return Err(error(&data)) }
    order_from(&data)
}

/// The partner key's orders whose payout isn't final yet, newest first (one page of 50: more open orders than that is
/// not a wallet's).
pub async fn open_orders() -> Result<Vec<Order>> {
    if key().is_none() {
        return Err(WalletError("1Click: Limit orders need a NEAR Intents partner key: add it in Settings › General › NEAR Intents key.".into()));
    }
    let (data, code) = get(&format!("{BASE}/orders?filter%5BisPayoutStatusFinal%5D=false&page%5Bsize%5D=50")).await
        .map_err(|e| WalletError(e.to_string()))?;
    parse_orders(&data, code)
}

pub fn parse_orders(data: &[u8], code: u16) -> Result<Vec<Order>> {
    let list = serde_json::from_slice::<Value>(data).ok().and_then(|j| j.get("data").and_then(Value::as_array).cloned());
    match list {
        Some(list) if (200..300).contains(&code) => Ok(list.iter().filter_map(parse_order).collect()),
        _ => Err(error(data)),
    }
}

/// Asks 1Click to cancel: matching stops, what filled is paid out, the rest comes back to the order's refund address.
/// Asynchronous: a last slice can still fill.
pub async fn cancel_order(id: &str) -> Result<()> {
    post_json(&format!("/orders/{}/cancel", order_path(id)), &json!({})).await.map(|_| ())
}

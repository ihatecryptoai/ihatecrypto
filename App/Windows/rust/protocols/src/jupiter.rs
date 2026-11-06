//! Swaps inside Solana through Jupiter's free API (`Jupiter.swift`). Jupiter builds the transaction; the wallet only
//! checks that it is the fee payer (or the second signer of a gasless swap) and signs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use wallet::{Result, WalletError};

use crate::{dec_value, post, query, s};

pub const BASE: &str = "https://api.jup.ag/swap/v1";
pub const V2: &str = "https://api.jup.ag/swap/v2";
pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// `JupiterError`: Jupiter's own words when it answers JSON («Each order valued at 30.00 USDC, minimum is 50.00 USDC»),
/// else the body as text; a gateway's page is only its status.
pub fn error(body: &[u8], status: u16) -> WalletError {
    let own = serde_json::from_slice::<Value>(body).ok().and_then(|j| s(&j, "error").map(str::to_string));
    said(&own.unwrap_or_else(|| net::said(body, status)))
}

pub fn said(message: &str) -> WalletError { WalletError(format!("Jupiter: {message}")) }

#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    /// The quote exactly as Jupiter returned it; /swap needs it back unchanged.
    pub raw: Vec<u8>,
    pub in_amount: u64,
    pub out_amount: u64,
    /// Least output after slippage: the floor the person agrees to.
    pub min_out: u64,
    pub route: String,
    /// The route's pools by Jupiter's label, e.g. ["HumidiFi", "Hadron"]: what `exclude_dexes` takes.
    pub dexes: Vec<String>,
    /// How far the swap moves the price, 0…1 (Jupiter's priceImpactPct).
    pub impact: Decimal,
    /// What comes out, in USD by Jupiter's prices (swapUsdValue); 0 when it doesn't say.
    pub usd: Decimal,
}

/// Jupiter's price impact (its sign dropped: v2 says it negative) and the swap's USD worth, from a quote or an order.
pub fn impact_and_worth(j: &Value) -> (Decimal, Decimal) {
    (dec_value(j.get("priceImpactPct")).unwrap_or_default().abs(), dec_value(j.get("swapUsdValue")).unwrap_or_default())
}

fn amount(j: &Value, key: &str) -> Option<u64> { s(j, key)?.parse().ok() }

fn dexes(j: &Value) -> Vec<String> {
    j.get("routePlan").and_then(Value::as_array).into_iter().flatten()
        .filter_map(|p| p.get("swapInfo").and_then(|i| s(i, "label")).map(str::to_string)).collect()
}

fn retry(status: u16) -> bool { status == 429 || status >= 500 }

/// /quote's answer read: amounts, the route and its pools.
pub fn parse_quote(data: &[u8], status: u16) -> std::result::Result<Quote, net::Again> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    let read = j.as_ref().filter(|_| status == 200).and_then(|j| Some((j, amount(j, "inAmount")?, amount(j, "outAmount")?, amount(j, "otherAmountThreshold")?)));
    let Some((j, in_amount, out_amount, min_out)) = read else { return Err(net::Again { again: retry(status), error: error(data, status) }) };
    let dexes = dexes(j);
    let (impact, usd) = impact_and_worth(j);
    Ok(Quote { raw: data.to_vec(), in_amount, out_amount, min_out, route: dexes.join(" → "), dexes, impact, usd })
}

/// `exclude_dexes`: labels Jupiter must route around (a route that failed before signing). A throttle or a gateway's 5xx
/// is asked again: one 503 here stopped a chain of moves at its swap.
pub async fn quote(input: &str, output: &str, amount: u64, slippage_bps: i64, exclude_dexes: &[String]) -> Result<Quote> {
    let mut url = format!("{BASE}/quote?inputMint={input}&outputMint={output}&amount={amount}&slippageBps={slippage_bps}");
    if !exclude_dexes.is_empty() { url += &format!("&excludeDexes={}", query(&exclude_dexes.join(","))) }
    net::run(&net::DELAYS, |_| net::get(&url, &[]), parse_quote).await
}

/// Unsigned versioned transaction for `quote`, paid by `user`, and the block its blockhash is good until.
pub async fn swap_transaction(quote: &Quote, user: &str) -> Result<(Vec<u8>, u64)> {
    let quote_object: Value = serde_json::from_slice(&quote.raw).map_err(|e| WalletError(e.to_string()))?;
    let body = json!({
        "quoteResponse": quote_object, "userPublicKey": user, "wrapAndUnwrapSol": true, "dynamicComputeUnitLimit": true,
        "prioritizationFeeLamports": {"priorityLevelWithMaxLamports": {"maxLamports": 200_000, "priorityLevel": "medium"}},
    });
    // Nothing is signed yet, so this is asked again like the quote.
    let url = format!("{BASE}/swap");
    net::run(&net::DELAYS, |_| net::post_json(&url, &body), parse_swap).await
}

pub fn parse_swap(data: &[u8], status: u16) -> std::result::Result<(Vec<u8>, u64), net::Again> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    let read = j.as_ref().and_then(|j| Some((STANDARD.decode(s(j, "swapTransaction")?).ok()?, j.get("lastValidBlockHeight")?.as_u64()?)));
    read.ok_or_else(|| net::Again { again: retry(status), error: error(data, status) })
}

/// Jupiter's gasless swap (Swap API v2): for a wallet with under 0.01 SOL, Jupiter's own wallet pays the fee and the rent
/// and takes them out of the output. Its transaction comes with Jupiter's wallet as the fee payer and ours second; it goes
/// back through /execute, which adds its signature and sends.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub request_id: String,
    pub wire: Vec<u8>,
    pub in_amount: u64,
    pub out_amount: u64,
    /// The least the swap gives after slippage.
    pub min_out: u64,
    /// Whoever pays the fee: Jupiter's wallet when `gasless`, else the taker.
    pub gasless: bool,
    pub fee_bps: i64,
    pub route: String,
    /// The block its blockhash is good until (0 when Jupiter doesn't say).
    pub last_valid_block_height: u64,
    pub impact: Decimal,
    pub usd: Decimal,
    /// Who fills it: «metis» (Jupiter's own routing) or «jupiterz» (a market maker, which pays the fee and signs first).
    pub router: String,
}

/// What /order gives for `amount` without a taker: its output, price impact and router — for the card, which compares
/// it with /quote's.
pub async fn order_quote(input: &str, output: &str, amount: u64) -> Result<(u64, Decimal, String)> {
    let url = format!("{V2}/order?inputMint={input}&outputMint={output}&amount={amount}");
    net::run(&net::DELAYS, |_| net::get(&url, &[]), parse_order_quote).await
}

pub fn parse_order_quote(data: &[u8], status: u16) -> std::result::Result<(u64, Decimal, String), net::Again> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    let read = j.as_ref().filter(|_| status == 200).and_then(|j| Some((amount(j, "outAmount")?, impact_and_worth(j).0, s(j, "router").unwrap_or("").to_string())));
    read.ok_or_else(|| net::Again { again: retry(status), error: error(data, status) })
}

/// The swap for `taker` as Jupiter builds it; an error with its words when it can't (too little, no route).
/// `exclude_routers`: routers Jupiter must leave out («jupiterz»: its market makers).
pub async fn order(input: &str, output: &str, amount: u64, taker: &str, exclude_routers: &[String]) -> Result<Order> {
    let skip = if exclude_routers.is_empty() { String::new() } else { format!("&excludeRouters={}", exclude_routers.join(",")) };
    let url = format!("{V2}/order?inputMint={input}&outputMint={output}&amount={amount}&taker={taker}{skip}");
    net::run(&net::DELAYS, |_| net::get(&url, &[]), parse_order).await
}

pub fn parse_order(data: &[u8], status: u16) -> std::result::Result<Order, net::Again> {
    let j: Option<Value> = serde_json::from_slice(data).ok().filter(Value::is_object);
    let Some(j) = j.filter(|_| status == 200) else { return Err(net::Again { again: retry(status), error: error(data, status) }) };
    let read = (|| Some((s(&j, "requestId")?, STANDARD.decode(s(&j, "transaction")?).ok().filter(|w| !w.is_empty())?,
                         amount(&j, "inAmount")?, amount(&j, "outAmount")?, amount(&j, "otherAmountThreshold")?)))();
    let Some((request_id, wire, in_amount, out_amount, min_out)) = read else {
        let why = s(&j, "errorMessage").or_else(|| s(&j, "error")).unwrap_or("no transaction for this swap");
        return Err(net::Again { again: false, error: said(why) });
    };
    let dexes = dexes(&j);
    let (impact, usd) = impact_and_worth(&j);
    let height = j.get("lastValidBlockHeight");
    Ok(Order {
        request_id: request_id.into(), wire, in_amount, out_amount, min_out,
        gasless: j.get("gasless").and_then(Value::as_bool).unwrap_or(false),
        fee_bps: j.get("feeBps").and_then(Value::as_f64).map_or(0, |f| f as i64),
        route: dexes.join(" → "),
        last_valid_block_height: height.and_then(Value::as_u64).or_else(|| height.and_then(Value::as_str).and_then(|h| h.parse().ok())).unwrap_or(0),
        impact, usd, router: s(&j, "router").unwrap_or("").into(),
    })
}

/// How long one /execute waits for its answer, in ms. Only the Dev copy's flag sets it, for a live run of a swap whose
/// answer doesn't come.
pub static EXECUTE_TIMEOUT_MS: AtomicU64 = AtomicU64::new(60_000);

/// Hands our signed transaction to Jupiter, which signs as the fee payer and sends it. The same transaction and id may be
/// sent again (Jupiter answers for the first one), so a dropped answer is asked again. Returns the signature.
pub async fn execute(signed: &[u8], request_id: &str) -> Result<String> { execute_at(V2, signed, request_id, &net::DELAYS).await }

pub async fn execute_at(base: &str, signed: &[u8], request_id: &str, delays: &[Duration]) -> Result<String> {
    let body = json!({"signedTransaction": STANDARD.encode(signed), "requestId": request_id}).to_string().into_bytes();
    let timeout = Duration::from_millis(EXECUTE_TIMEOUT_MS.load(Ordering::Relaxed));
    let url = format!("{base}/execute");
    net::run(delays, |_| post(&url, body.clone(), &[], Some(timeout)), parse_execute).await
}

pub fn parse_execute(data: &[u8], status: u16) -> std::result::Result<String, net::Again> {
    let j: Option<Value> = serde_json::from_slice(data).ok().filter(Value::is_object);
    if let Some(sig) = j.as_ref().filter(|j| status == 200 && s(j, "status") == Some("Success")).and_then(|j| s(j, "signature")) {
        return Ok(sig.into());
    }
    let what = j.as_ref().and_then(|j| s(j, "error")).map_or_else(|| net::said(data, status), str::to_string);
    let code = j.as_ref().and_then(|j| j.get("code")).map_or_else(|| status.to_string(), |c| crate::text(Some(c), ""));
    Err(net::Again { again: retry(status), error: said(&format!("{what} (code {code})")) })
}

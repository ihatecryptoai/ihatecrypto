//! Jupiter's on-chain DCA (Recurring v1, program DCA265…, `JupiterRecurring.swift`): the whole amount goes into an order
//! account of the person's own and Jupiter's keepers swap one part every interval; cancelling returns what wasn't used,
//! with the accounts' rent. Keyless at api.jup.ag. Minimums, checked by Jupiter: $100 in all, $50 an order, 2 orders.

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use wallet::{Result, WalletError};

use crate::jupiter::error;
use crate::{dec_value, s};

pub const BASE: &str = "https://api.jup.ag/recurring/v1";
pub const MIN_TOTAL_USD: u32 = 100;
pub const MIN_ORDER_USD: u32 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub key: String,
    pub input_mint: String,
    pub output_mint: String,
    /// In the input token: put in, spent so far, spent per swap.
    pub deposited: Decimal,
    pub used: Decimal,
    pub per_cycle: Decimal,
    /// Seconds between swaps.
    pub every: i64,
    /// In the output token, bought so far (sent to the wallet after each fill).
    pub received: Decimal,
    pub created_at: Option<DateTime<Utc>>,
    pub fills: usize,
}

/// The body of an order: `amount` raw input units split into `orders` swaps `every` seconds apart, the first right away.
pub fn create_body(user: &str, input: &str, output: &str, amount: u64, orders: i64, every: i64) -> Value {
    json!({
        "user": user, "inputMint": input, "outputMint": output,
        "params": {"time": {"inAmount": amount, "numberOfOrders": orders, "interval": every, "minPrice": null, "maxPrice": null, "startAt": null}},
    })
}

/// Unsigned order transaction: (requestId, wire).
pub async fn create_order(user: &str, input: &str, output: &str, amount: u64, orders: i64, every: i64) -> Result<(String, Vec<u8>)> {
    transaction("createOrder", &create_body(user, input, output, amount, orders, every)).await
}

/// Unsigned transaction that closes `order`: the input not yet swapped and the rent come back to `user`.
pub async fn cancel_order(user: &str, order: &str) -> Result<(String, Vec<u8>)> {
    transaction("cancelOrder", &json!({"order": order, "user": user, "recurringType": "time"})).await
}

/// The person's running orders, newest first.
pub async fn orders(user: &str) -> Result<Vec<Order>> {
    let mut all = Vec::new();
    for page in 1..=5 {
        let url = format!("{BASE}/getRecurringOrders?user={user}&orderStatus=active&recurringType=time&includeFailedTx=false&page={page}");
        let (data, status) = net::get(&url, &[]).await.map_err(|e| WalletError(e.to_string()))?;
        let (orders, pages) = parse_page(&data, status)?;
        all.extend(orders);
        if page >= pages { break }
    }
    all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(all)
}

/// One page of getRecurringOrders: its orders and how many pages there are.
pub fn parse_page(data: &[u8], status: u16) -> Result<(Vec<Order>, i64)> {
    let j: Option<Value> = serde_json::from_slice(data).ok().filter(Value::is_object);
    let Some(j) = j.filter(|_| status == 200) else { return Err(error(data, status)) };
    let orders = j.get("time").and_then(Value::as_array).into_iter().flatten().filter_map(order).collect();
    Ok((orders, j.get("totalPages").and_then(Value::as_i64).unwrap_or(0)))
}

pub fn order(o: &Value) -> Option<Order> {
    let n = |k: &str| dec_value(o.get(k)).unwrap_or_default();
    Some(Order {
        key: s(o, "orderKey")?.into(), input_mint: s(o, "inputMint")?.into(), output_mint: s(o, "outputMint")?.into(),
        deposited: n("inDeposited"), used: n("inUsed"), per_cycle: n("inAmountPerCycle"),
        every: n("cycleFrequency").trunc().to_i64().unwrap_or(0), received: n("outReceived"),
        // ISO 8601, with or without fractions of a second; one without a zone is UTC.
        created_at: s(o, "createdAt").and_then(|t| {
            DateTime::parse_from_rfc3339(t).map(|t| t.with_timezone(&Utc)).ok()
                .or_else(|| chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|n| n.and_utc()))
        }),
        fills: o.get("trades").and_then(Value::as_array).map_or(0, Vec::len),
    })
}

async fn transaction(path: &str, body: &Value) -> Result<(String, Vec<u8>)> {
    let (data, status) = net::post_json(&format!("{BASE}/{path}"), body).await.map_err(|e| WalletError(e.to_string()))?;
    parse_transaction(&data, status)
}

/// (requestId, wire); else Jupiter's own words («Each order valued at 30.00 USDC, minimum is 50.00 USDC»).
pub fn parse_transaction(data: &[u8], status: u16) -> Result<(String, Vec<u8>)> {
    let j: Option<Value> = serde_json::from_slice(data).ok();
    j.as_ref().and_then(|j| Some((s(j, "requestId")?.to_string(), STANDARD.decode(s(j, "transaction")?).ok()?))).ok_or_else(|| error(data, status))
}

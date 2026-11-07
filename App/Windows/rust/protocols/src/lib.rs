//! Relay, NEAR Intents' 1Click and Agent API, and Jupiter (`App/Sources/Protocols`): quotes asked, answers read, what they hand back
//! to sign. Every call goes through `net` (one client, the same retries); what each answer means is read here and
//! tested offline. Hyperliquid's signing is its own crate.

pub mod agent_api;
pub mod jupiter;
pub mod one_click;
pub mod recurring;
pub mod relay;

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde_json::Value;
use wallet::{Result, WalletError};

pub(crate) fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

/// A string field of a JSON object.
pub(crate) fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> { v.get(key)?.as_str() }

/// `"\(x ?? fallback)"`: a value as Swift interpolates it — a string as is, a number as written, anything else as JSON.
pub(crate) fn text(v: Option<&Value>, fallback: &str) -> String {
    match v {
        None => fallback.into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "<null>".into(),
        Some(other) => other.to_string(),
    }
}

/// A JSON number or a decimal string as u64: what Swift's `number` helpers read.
pub(crate) fn number(v: Option<&Value>) -> Option<u64> {
    match v? { Value::Number(n) => n.as_u64(), Value::String(s) => s.parse().ok(), _ => None }
}

/// `Decimal(string:)`: plain or with an exponent; None when it isn't a number.
pub fn dec(s: &str) -> Option<Decimal> {
    let s = s.trim();
    s.parse::<Decimal>().ok().or_else(|| Decimal::from_scientific(s).ok())
}

/// A JSON number or decimal string as a Decimal (NSNumber's `decimalValue`, `Decimal(string:)`).
pub(crate) fn dec_value(v: Option<&Value>) -> Option<Decimal> {
    match v? {
        Value::String(s) => dec(s),
        Value::Number(n) => dec(&n.to_string()).or_else(|| n.as_f64().and_then(Decimal::from_f64)),
        _ => None,
    }
}

/// A query value, percent-encoded but for what URLComponents leaves as is in one (letters, digits, `-._~,`).
pub(crate) fn query(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~,".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// POSTs bytes as JSON with these headers; the body and status.
pub(crate) async fn post(url: &str, body: Vec<u8>, headers: &[(&str, String)], timeout: Option<std::time::Duration>)
    -> std::result::Result<net::Answer, reqwest::Error> {
    let mut r = net::HTTP.post(url).header("Content-Type", "application/json").body(body);
    for (k, v) in headers { r = r.header(*k, v) }
    if let Some(t) = timeout { r = r.timeout(t) }
    let response = r.send().await?;
    let status = response.status().as_u16();
    Ok((response.bytes().await?.to_vec(), status))
}

#[cfg(test)]
mod tests;

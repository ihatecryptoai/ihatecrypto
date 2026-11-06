//! Jupiter Lend, Kamino and where money earns (`App/Sources/DeFi`): rates, positions, the unsigned transactions these
//! services build, and the pure rules that spread money over the best rates. Every answer is read the way Swift's
//! JSONSerialization and NSNumber read it (the helpers below), so the same answer gives the same numbers and words.

pub mod allocation;
pub mod jupiter_lend;
pub mod kamino;
pub mod rebalance;
pub mod yields;

use std::str::FromStr;
use std::time::Duration;

use rust_decimal::{Decimal, RoundingStrategy};
use serde_json::{Map, Value};
use wallet::{Result, WalletError};

pub(crate) type Object = Map<String, Value>;

/// What JSONSerialization throws for an answer that isn't JSON (a gateway's HTML page, nothing at all) or is a bare
/// value outside an array or object, in the Mac's words.
pub const NOT_JSON: &str = "The data couldn’t be read because it isn’t in the correct format.";

/// `try JSONSerialization.jsonObject(with:)`: an array or an object, else Foundation's error.
pub(crate) fn json(data: &[u8]) -> Result<Value> {
    match serde_json::from_slice::<Value>(data) {
        Ok(v @ (Value::Array(_) | Value::Object(_))) => Ok(v),
        _ => Err(WalletError(NOT_JSON.into())),
    }
}

/// `as? [[String: Any]]`: nil unless every element is an object.
pub(crate) fn objects(v: Option<&Value>) -> Option<Vec<&Object>> { v?.as_array()?.iter().map(Value::as_object).collect() }

/// `o[key] as? [String: Any]`.
pub(crate) fn object<'a>(o: Option<&'a Object>, key: &str) -> Option<&'a Object> { o?.get(key)?.as_object() }

/// `o[key] as? String`.
pub(crate) fn string<'a>(o: Option<&'a Object>, key: &str) -> Option<&'a str> { o?.get(key)?.as_str() }

/// `(v as? String).flatMap { Decimal(string: $0) }`.
pub(crate) fn str_dec(v: Option<&Value>) -> Option<Decimal> { decimal(v?.as_str()?) }

/// `(v as? NSNumber)?.decimalValue`: a fraction as the shortest decimal that reads back the same double (what
/// JSONSerialization's NSNumber gives, checked on macOS 27), true and false as 1 and 0.
pub(crate) fn ns_dec(v: Option<&Value>) -> Option<Decimal> {
    match v? {
        Value::Bool(b) => Some(Decimal::from(u8::from(*b))),
        Value::Number(n) => n.as_i64().map(Decimal::from).or_else(|| n.as_u64().map(Decimal::from)).or_else(|| decimal(&n.as_f64()?.to_string())),
        _ => None,
    }
}

/// `(v as? NSNumber)?.intValue`: a fraction cut toward zero.
pub(crate) fn ns_int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)).or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

/// `(v as? NSNumber)?.uint64Value`: −1 is 2⁶⁴ − 1, as on the Mac.
pub(crate) fn ns_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Bool(b) => Some(u64::from(*b)),
        Value::Number(n) => n.as_u64().or_else(|| n.as_i64().map(|i| i as u64)).or_else(|| n.as_f64().map(|f| f as u64)),
        _ => None,
    }
}

/// A string read as `Decimal(string:)`, else a number as NSNumber: Kamino's `dec` and every rate read that way.
pub(crate) fn dec(v: Option<&Value>) -> Option<Decimal> { str_dec(v).or_else(|| ns_dec(v)) }

/// `Decimal(string:)`: the number at the start of the text, spaces before it skipped and what follows ignored
/// («12abc» is 12); nil when there is none. Digits past rust_decimal's 28 places are rounded (Swift keeps 38).
pub(crate) fn decimal(s: &str) -> Option<Decimal> {
    let t = s.trim_start();
    let b = t.as_bytes();
    let run = |from: usize| from + b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut i = usize::from(matches!(b.first(), Some(b'-' | b'+')));
    let whole = &t[i..run(i)];
    i = run(i);
    let mut fraction = "";
    if b.get(i) == Some(&b'.') {
        fraction = &t[i + 1..run(i + 1)];
        i = run(i + 1);
    }
    if i == 0 { return None }
    let sign = if b[0] == b'-' { "-" } else { "" };
    let mut number = format!("{sign}{}.{}", if whole.is_empty() { "0" } else { whole }, if fraction.is_empty() { "0" } else { fraction });
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let from = i + 1 + usize::from(matches!(b.get(i + 1), Some(b'-' | b'+')));
        if run(from) > from { number = format!("{number}e{}", &t[i + 1..run(from)]) }
    }
    if number.contains('e') { Decimal::from_scientific(&number).ok() } else { Decimal::from_str(&number).ok() }
}

/// `"\(decimal)"`: Decimal's description, no exponent, no trailing zeros.
pub fn text(d: Decimal) -> String { if d.is_zero() { "0".into() } else { d.normalize().to_string() } }

/// `d.formatted(.number.precision(.fractionLength(0...2)).locale(Locale(identifier: "en_US")))`, with `.grouping(.never)`
/// when `grouping` is off: ICU's half to even, thousands grouped, and a minus kept on a negative that rounds to 0 («-0»).
pub(crate) fn two_places(d: Decimal, grouping: bool) -> String {
    let rounded = d.abs().round_dp_with_strategy(2, RoundingStrategy::MidpointNearestEven);
    let text = text(rounded);
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if grouping && i > 0 && (whole.len() - i) % 3 == 0 { grouped.push(',') }
        grouped.push(c);
    }
    let sign = if d.is_sign_negative() && !d.is_zero() { "-" } else { "" };
    format!("{sign}{grouped}{}{fraction}", if fraction.is_empty() { "" } else { "." })
}

/// Sends a request and gives the body and status; a request that fails on the way is its error's words.
pub(crate) async fn send(r: reqwest::RequestBuilder) -> Result<net::Answer> {
    let response = r.send().await.map_err(|e| WalletError(e.to_string()))?;
    let status = response.status().as_u16();
    Ok((response.bytes().await.map_err(|e| WalletError(e.to_string()))?.to_vec(), status))
}

/// The 20 s Kamino's and Hyperliquid's reads wait (the client's own limit is 30 s, what the posts wait).
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(20);

#[cfg(test)]
mod tests;

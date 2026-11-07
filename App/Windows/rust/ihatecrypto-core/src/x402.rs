//! `X402.swift`: x402 (github.com/coinbase/x402), version 2. An API answers 402 with a PAYMENT-REQUIRED header — base64
//! JSON whose `accepts` lists what it takes — and the request is sent again with PAYMENT-SIGNATURE: the chosen entry
//! echoed byte for byte and an EIP-3009 authorization the service's facilitator settles. ihatecrypto takes the `exact`
//! scheme in USDC on Base only.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};

use crate::foundation::whole;

pub const BASE_USDC: &str = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913";
/// The signer's cap per payment (its x402 authorization's): a dearer offer gets no card.
pub const CAP: u64 = 5_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The `accepts` entry and the `resource` exactly as they came: the payment echoes them unchanged.
    pub accepted: String,
    pub resource: Option<String>,
    /// The 402's `extensions`, echoed back as the spec asks.
    pub extensions: Option<String>,
    pub chain_id: u64,
    pub asset: String,
    pub pay_to: String,
    /// USDC units.
    pub amount: u64,
    pub max_timeout_seconds: i64,
}

impl Offer {
    pub fn usdc(&self) -> Decimal { Decimal::from_i128_with_scale(self.amount.into(), 6).normalize() }
}

#[derive(Deserialize)]
struct Required<'a> {
    #[serde(rename = "x402Version")]
    version: Option<i64>,
    #[serde(borrow)]
    accepts: Option<Vec<&'a RawValue>>,
    #[serde(borrow)]
    resource: Option<&'a RawValue>,
    #[serde(borrow)]
    extensions: Option<&'a RawValue>,
}

/// The offer in a 402's PAYMENT-REQUIRED header that ihatecrypto can pay, or None.
pub fn offer(header: Option<&str>) -> Option<Offer> {
    let json = STANDARD.decode(header?.trim()).ok()?;
    let r: Required = serde_json::from_slice(&json).ok()?;
    if r.version != Some(2) { return None }
    for raw in r.accepts? {
        let Ok(a) = serde_json::from_str::<Value>(raw.get()) else { continue };
        let s = |k: &str| a.get(k).and_then(Value::as_str);
        let asset = s("asset").map(str::to_lowercase);
        let amount = s("amount").and_then(|x| x.parse::<u64>().ok()).filter(|x| *x > 0 && *x <= CAP);
        let method = a.get("extra").and_then(|e| e.get("assetTransferMethod")).and_then(Value::as_str);
        let (Some(asset), Some(pay_to), Some(amount)) = (asset, s("payTo").filter(|p| whole(p, "0x[0-9a-fA-F]{40}").is_some()), amount) else { continue };
        if s("scheme") != Some("exact") || s("network") != Some("eip155:8453") || asset != BASE_USDC || method.is_some_and(|m| m != "eip3009") { continue }
        return Some(Offer {
            accepted: raw.get().to_string(), resource: r.resource.map(|v| v.get().to_string()), extensions: r.extensions.map(|v| v.get().to_string()),
            chain_id: 8453, asset, pay_to: pay_to.to_string(), amount, max_timeout_seconds: a.get("maxTimeoutSeconds").and_then(Value::as_i64).unwrap_or(300),
        });
    }
    None
}

/// The PAYMENT-SIGNATURE header for `offer`, signed by `from`.
pub fn payment(offer: &Offer, from: &str, valid_before: u64, nonce: &[u8], signature: &[u8]) -> (&'static str, String) {
    let hex = |d: &[u8]| format!("0x{}", d.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let authorization = json!({"from": from, "to": offer.pay_to, "value": offer.amount.to_string(), "validAfter": "0",
                               "validBefore": valid_before.to_string(), "nonce": hex(nonce)});
    let payload = json!({"signature": hex(signature), "authorization": authorization});
    let mut out = String::from(r#"{"x402Version":2,"#);
    if let Some(resource) = &offer.resource { out += &format!(r#""resource":{resource},"#) }
    out += &format!(r#""accepted":{},"payload":{payload}"#, offer.accepted);
    if let Some(extensions) = &offer.extensions { out += &format!(r#","extensions":{extensions}"#) }
    out += "}";
    ("PAYMENT-SIGNATURE", STANDARD.encode(out))
}

/// The settlement's transaction from a PAYMENT-RESPONSE header, when it says it succeeded.
pub fn settled(header: Option<&str>) -> Option<String> {
    let o: Value = serde_json::from_slice(&STANDARD.decode(header?.trim()).ok()?).ok()?;
    if o.get("success").and_then(Value::as_bool) != Some(true) { return None }
    o.get("transaction").and_then(Value::as_str).filter(|t| whole(t, "0x[0-9a-fA-F]{64}").is_some()).map(str::to_string)
}

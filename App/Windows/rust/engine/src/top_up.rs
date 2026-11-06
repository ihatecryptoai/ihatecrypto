//! Top up (TopUp.swift), as the Mac's ReceiveBoard and TopUpPage: what this wallet holds of the chosen coin on the chosen
//! network (read from the network itself, for «now → after»), whether dollars there would be stuck without the gas
//! coin, and — once a way is used — the balance watched every 4 s until money lands. «From a browser wallet» is a page
//! this PC serves to its own browser on 127.0.0.1, so the recipient comes from here and no website can swap it.
//! Layer 2 adds here: top_up_networks (TopUpOrder's networks from EvmNetwork), top_up_order, top_up, top_up_open,
//! top_up_stop, TopUpPage (what's shown and watched, the page's server on 127.0.0.1 and its HTML) and the Solana
//! transfers the page asks to sign.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::Serialize;

use crate::json;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopUpView {
    pub networks: Vec<TopUpNet>,
    pub network: String,
    pub coin: String,
    /// The coin at this wallet there, by symbol (USDC and the gas coin for «any coin»); empty until the network answers.
    #[serde(with = "json::numbers")]
    pub held: HashMap<String, Decimal>,
    /// The network's gas coin when the wallet holds less of it than a move there needs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_short: Option<String>,
    /// A way was used (a QR or an address shown, the page opened): the balance is watched.
    pub waiting: bool,
    /// «Landed on Solana: +20 USDC, 5 → 25».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrived: Option<String>,
    /// What the browser page said it sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent: Option<TopUpSent>,
}

/// `TopUpView.Sent`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopUpSent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// `TopUpView.Net`: a network the wallet receives on, for the window's chips and its pay links (TopUpOrder.payLink).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopUpNet {
    pub key: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    pub usdc: String,
    pub usdc_symbol: String,
    pub usdc_decimals: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    pub recipient: String,
    /// Dollars there move by signature, Relay paying the gas (Permit3009): the gas line says so.
    pub by_signature: bool,
}

/// Solana's system program.
pub const SYSTEM: &str = "11111111111111111111111111111111";

/// A request to the top-up page: its method, path, lowercased headers and body.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl PageRequest {
    /// One HTTP/1.1 request from what was read; None while it isn't whole yet (its head, then `Content-Length` bytes).
    pub fn parse(data: &[u8]) -> Option<PageRequest> {
        let end = data.windows(4).position(|w| w == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&data[..end]);
        let mut lines = head.split("\r\n");
        let first: Vec<&str> = lines.next().unwrap_or("").split(' ').filter(|s| !s.is_empty()).collect();
        if first.len() < 2 { return None }
        let mut headers = HashMap::new();
        for line in lines {
            let Some((name, value)) = line.split_once(':') else { continue };
            headers.insert(name.to_lowercase(), value.trim_matches([' ', '\t']).to_string());
        }
        let body = &data[end + 4..];
        let wanted = headers.get("content-length").map_or(Some(0), |l| l.parse::<usize>().ok()).unwrap_or(0);
        if body.len() < wanted { return None }
        Some(PageRequest { method: first[0].into(), path: first[1].split('?').next().unwrap_or("").into(), headers, body: body.to_vec() })
    }
}

/// The page's answer: status, type and body, with the headers that keep it from being framed or cached.
pub fn response(status: u16, kind: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        if status == 200 { "OK" } else { "Error" }, body.len());
    [head.as_bytes(), body].concat()
}

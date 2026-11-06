//! The browser sign-ins (`Core/OAuth.swift`): RFC 7636's proof key, reading our own JWTs, and the one redirect caught
//! on 127.0.0.1, so nothing outside this computer can reach it.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{Result, fail};

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new(verifier: &str) -> Self {
        Self { verifier: verifier.into(), challenge: URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) }
    }

    pub fn make() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("the system's randomness");
        Self::new(&URL_SAFE_NO_PAD.encode(bytes))
    }
}

/// Payload claims without verifying the signature: only for reading our own tokens.
pub fn claims(token: &str) -> Option<Map<String, Value>> {
    let payload = token.split('.').nth(1).filter(|_| token.split('.').count() >= 2)?;
    let mut s = payload.replace('-', "+").replace('_', "/");
    s.push_str(&"=".repeat((4 - s.len() % 4) % 4));
    match serde_json::from_slice(&STANDARD.decode(s).ok()?).ok()? { Value::Object(m) => Some(m), _ => None }
}

/// A token's «exp», Unix seconds.
pub fn expiry(token: &str) -> Option<f64> { claims(token)?.get("exp")?.as_f64() }

/// Catches one OAuth redirect on the loopback interface.
pub struct LoopbackServer {
    pub port: u16,
    listener: TcpListener,
    path: String,
}

impl LoopbackServer {
    /// `port` 0 picks a free one. `path` is the redirect path, e.g. "/callback".
    pub async fn start(port: u16, path: &str) -> Result<Self> {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => Ok(Self { port: listener.local_addr().map(|a| a.port()).unwrap_or(port), listener, path: path.into() }),
            Err(e) => fail(format!("Couldn't open port {port} for sign-in: {e}")),
        }
    }

    /// Query parameters of the redirect. Stops listening afterwards.
    pub async fn wait_for_callback(self, timeout: Duration) -> Result<HashMap<String, String>> {
        let wait = async {
            loop {
                let Ok((mut socket, _)) = self.listener.accept().await else { continue };
                let mut buf = vec![0u8; 65_536];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                // First line of the request: "GET /callback?code=…&state=… HTTP/1.1"
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let target = head.split(' ').nth(1).unwrap_or("");
                let (path, query) = target.split_once('?').unwrap_or((target, ""));
                let hit = path == self.path;
                let _ = socket.write_all(&page(hit)).await;
                let _ = socket.shutdown().await;
                if hit { return query_items(query) }
            }
        };
        match tokio::time::timeout(timeout, wait).await {
            Ok(query) => Ok(query),
            Err(_) => fail(format!("Sign-in didn't finish within {} min.", timeout.as_secs() / 60)),
        }
    }
}

/// «a=1&b=x%20y» → {a: 1, b: x y}; the first of a repeated name kept, as URLComponents' queryItems give it.
fn query_items(query: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.entry(unpercent(k)).or_insert_with(|| unpercent(v));
    }
    out
}

fn unpercent(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn page(ok: bool) -> Vec<u8> {
    let text = if ok { "Signed in. You can go back to ihatecrypto." } else { "Not found" };
    let body = format!("<!doctype html><meta charset=utf-8><title>ihatecrypto</title><p style=\"font:16px -apple-system;margin:40px\">{text}</p>");
    format!("HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            if ok { "200 OK" } else { "404 Not Found" }, body.len()).into_bytes()
}

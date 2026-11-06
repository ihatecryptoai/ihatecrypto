//! HTTP for the model services (`Core/HTTP.swift`) over the engine's one client (`net::HTTP`). A timeout here is
//! URLSession's: how long to wait for the next bytes, not for the whole answer, so a long streamed answer isn't cut.

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

fn io(e: reqwest::Error) -> Error { if e.is_timeout() { timed_out() } else { Error::Io(e.to_string()) } }

fn timed_out() -> Error { Error::Io("The request timed out.".into()) }

/// «HTTP 429: …», the body's first 300 bytes: a service's refusal (Swift's `HTTP.check`).
pub fn check(status: u16, body: &[u8]) -> Result<()> {
    if (200..300).contains(&status) { return Ok(()) }
    Err(Error::Net(format!("HTTP {status}: {}", String::from_utf8_lossy(&body[..body.len().min(300)]))))
}

async fn idle<T>(seconds: u64, f: impl Future<Output = std::result::Result<T, reqwest::Error>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(seconds), f).await.map_err(|_| timed_out())?.map_err(io)
}

/// Sends, waiting `seconds` for the answer to begin; the whole answer may take longer.
pub async fn send(request: reqwest::RequestBuilder, seconds: u64) -> Result<reqwest::Response> {
    idle(seconds, request.timeout(Duration::from_secs(3600)).send()).await
}

/// The whole body, `seconds` at most between its bytes, then checked.
async fn body(mut response: reqwest::Response, seconds: u64) -> Result<Vec<u8>> {
    let status = response.status().as_u16();
    let mut out = Vec::new();
    while let Some(chunk) = idle(seconds, response.chunk()).await? { out.extend_from_slice(&chunk) }
    check(status, &out)?;
    Ok(out)
}

fn headers(mut r: reqwest::RequestBuilder, headers: &[(&str, &str)]) -> reqwest::RequestBuilder {
    for (k, v) in headers { r = r.header(*k, *v) }
    r
}

pub async fn post_json(url: &str, json: &Value, extra: &[(&str, &str)], seconds: u64) -> Result<Vec<u8>> {
    let r = headers(net::HTTP.post(url).header("Content-Type", "application/json").body(json.to_string()), extra);
    body(send(r, seconds).await?, seconds).await
}

/// An `application/x-www-form-urlencoded` POST: each value percent-encoded but for letters, digits and «-._~».
pub async fn post_form(url: &str, form: &[(&str, &str)], seconds: u64) -> Result<Vec<u8>> {
    let encoded = form.iter().map(|(k, v)| format!("{k}={}", percent(v, b"-._~"))).collect::<Vec<_>>().join("&");
    let r = net::HTTP.post(url).header("Content-Type", "application/x-www-form-urlencoded").body(encoded);
    body(send(r, seconds).await?, seconds).await
}

pub async fn get(url: &str, extra: &[(&str, &str)], seconds: u64) -> Result<Vec<u8>> {
    body(send(headers(net::HTTP.get(url), extra), seconds).await?, seconds).await
}

/// Every byte but ASCII letters, digits and `keep` as %XX.
pub fn percent(s: &str, keep: &[u8]) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || keep.contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A URL with query items, encoded as Foundation's URLComponents writes them (space as %20, «/» and «:» kept).
pub fn with_query(base: &str, items: &[(&str, &str)]) -> String {
    let keep = b"-._~!$'()*+,;:@/?";
    format!("{base}?{}", items.iter().map(|(k, v)| format!("{}={}", percent(k, keep), percent(v, keep))).collect::<Vec<_>>().join("&"))
}

/// An answer read line by line as it arrives (server-sent events, JSON lines).
pub struct Lines {
    response: reqwest::Response,
    pending: Vec<u8>,
    seconds: u64,
    done: bool,
}

impl Lines {
    /// POSTs `json` and opens the answer; a refusal is read (600 bytes at most) and said as `check` says it.
    pub async fn post(url: &str, json: &Value, extra: &[(&str, &str)], seconds: u64) -> Result<Self> {
        let r = headers(net::HTTP.post(url).header("Content-Type", "application/json").body(json.to_string()), extra);
        let mut response = send(r, seconds).await?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let mut head = Vec::new();
            while head.len() < 600 {
                match idle(seconds, response.chunk()).await? { Some(c) => head.extend_from_slice(&c), None => break }
            }
            head.truncate(600);
            check(status, &head)?;
        }
        Ok(Self { response, pending: Vec::new(), seconds, done: false })
    }

    /// The status the answer came with.
    pub fn status(&self) -> u16 { self.response.status().as_u16() }

    /// The next line without its end («\n» or «\r\n»), None at the end of the answer.
    pub async fn next(&mut self) -> Result<Option<String>> {
        loop {
            if let Some(at) = self.pending.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.pending.drain(..=at).collect();
                line.pop();
                if line.last() == Some(&b'\r') { line.pop(); }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            if self.done {
                if self.pending.is_empty() { return Ok(None) }
                let rest = std::mem::take(&mut self.pending);
                return Ok(Some(String::from_utf8_lossy(&rest).into_owned()));
            }
            match idle(self.seconds, self.response.chunk()).await? {
                Some(chunk) => self.pending.extend_from_slice(&chunk),
                None => self.done = true,
            }
        }
    }
}

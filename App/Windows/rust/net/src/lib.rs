//! What the engine asks the networks (`RPCRetry`, `SolanaRPC`, `EVMRPC`, the node lists, ENS, SNS in Swift): one HTTP
//! client, retries for what a second ask may fix, and JSON-RPC that walks to the next node when one throttles, fails or
//! turns a key away. The signer links none of this: it has no network.

pub mod broker;
pub mod evm_rpc;
pub mod solana_rpc;

use std::sync::LazyLock;
use std::time::Duration;

use wallet::{Result, WalletError};

pub(crate) fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

/// One client for every call: its connections are kept and reused.
pub static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).user_agent("ihatecrypto").build().expect("an HTTP client")
});

/// What an answer's reader returns when it refuses: `again` = worth asking once more.
#[derive(Debug)]
pub struct Again {
    pub again: bool,
    pub error: WalletError,
}

impl Again {
    pub fn new(again: bool, error: impl Into<String>) -> Self { Self { again, error: WalletError(error.into()) } }
    pub fn fin(error: WalletError) -> Self { Self { again: false, error } }
}

/// A throttle (429), a server error, a dropped connection or a node's bad second gets four more tries, 1, 2, 4 and 8 s
/// apart; any other answer is final — a revert or a short balance says the same the second time.
pub const DELAYS: [Duration; 4] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4), Duration::from_secs(8)];

/// An HTTP answer: its body and status.
pub type Answer = (Vec<u8>, u16);

/// Asks with `fetch` (the attempt's number given, so each try may go to another node) and reads the answer with
/// `answer`, again after a delay while the reader says it's worth it or the connection dropped.
pub async fn run<T, F, Fut>(delays: &[Duration], mut fetch: F, mut answer: impl FnMut(&[u8], u16) -> std::result::Result<T, Again>) -> Result<T>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<Answer, reqwest::Error>>,
{
    let mut attempt = 0;
    loop {
        let failure = match fetch(attempt).await {
            Ok((body, status)) => match answer(&body, status) {
                Ok(v) => return Ok(v),
                Err(e) if e.again => e.error,
                Err(e) => return Err(e.error),
            },
            // A dropped or timed-out connection: asked again. Anything else about the request itself is final.
            Err(e) if e.is_timeout() || e.is_connect() || e.is_request() => WalletError(format!("The network didn't answer: {e}")),
            Err(e) => return Err(WalletError(e.to_string())),
        };
        let Some(delay) = delays.get(attempt) else { return Err(failure) };
        tokio::time::sleep(*delay).await;
        attempt += 1;
    }
}

/// POSTs JSON and gives the body and status.
pub async fn post_json(url: &str, body: &serde_json::Value) -> std::result::Result<Answer, reqwest::Error> {
    let response = HTTP.post(url).header("Content-Type", "application/json").body(body.to_string()).send().await?;
    let status = response.status().as_u16();
    Ok((response.bytes().await?.to_vec(), status))
}

/// GETs and gives the body and status.
pub async fn get(url: &str, headers: &[(&str, &str)]) -> std::result::Result<Answer, reqwest::Error> {
    let mut request = HTTP.get(url);
    for (k, v) in headers { request = request.header(*k, *v) }
    let response = request.send().await?;
    let status = response.status().as_u16();
    Ok((response.bytes().await?.to_vec(), status))
}

/// What a service answered when it refused, as a line for the chat: its own words, never a gateway's error page.
/// `status` 0: not known.
pub fn said(body: &[u8], status: u16) -> String {
    let text = String::from_utf8_lossy(&body[..body.len().min(300)]).trim().to_string();
    if !text.is_empty() && !text.starts_with('<') { return text }
    format!("no answer right now{}. Try again in a minute.", if status > 0 { format!(" (HTTP {status})") } else { String::new() })
}

/// A node's error message that a second ask may not get.
pub fn transient(message: &str) -> bool {
    let m = message.to_lowercase();
    !m.contains("revert") && ["rate limit", "too many requests", "timeout", "timed out", "temporarily", "try again", "header not found",
                              "node is behind", "service unavailable", "internal error", "overloaded", "usage limit", "quota"].iter().any(|w| m.contains(w))
}

/// The first `n` bytes of an answer as text, for an error line.
pub(crate) fn head(body: &[u8], n: usize) -> String { String::from_utf8_lossy(&body[..body.len().min(n)]).into_owned() }

#[cfg(test)]
mod tests;

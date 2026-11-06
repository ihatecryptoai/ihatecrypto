//! G4's stand-ins at 127.0.0.1: any service as a function of the request (GoPlus Security, a node), and Hyperliquid's
//! spot book (OneClickTests' FakeHL with its spotMeta) inside a FakeHL turn, since the crate's Hyperliquid address is
//! global.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::fake_hl::Fake;

/// What a stand-in was asked: the path with its query, and the JSON body (Null for none).
pub type Asked = Arc<Mutex<Vec<(String, Value)>>>;

/// One request off the socket.
async fn request(socket: &mut tokio::net::TcpStream) -> Option<(String, Value)> {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 { return None }
        buf.extend(&chunk[..n]);
        let head = String::from_utf8_lossy(&buf).to_string();
        let Some(end) = head.find("\r\n\r\n") else { continue };
        let length = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
        if buf.len() < end + 4 + length { continue }
        let path = head.lines().next()?.split(' ').nth(1)?.to_string();
        return Some((path, serde_json::from_slice(&buf[end + 4..end + 4 + length]).unwrap_or(Value::Null)));
    }
}

/// A server answering each request with `answer(path, body)`: Some(body) as HTTP 200, None as a dropped connection.
pub async fn serve(answer: impl Fn(&str, &Value) -> Option<String> + Send + Sync + 'static) -> (String, Asked) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (kept, answer) = (Asked::default(), Arc::new(answer));
    let seen = kept.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let (seen, answer) = (seen.clone(), answer.clone());
            tokio::spawn(async move {
                let Some((path, body)) = request(&mut socket).await else { return };
                seen.lock().unwrap().push((path.clone(), body.clone()));
                let Some(reply) = answer(&path, &body) else { return };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                socket.write_all(response.as_bytes()).await.ok();
                socket.shutdown().await.ok();
            });
        }
    });
    (url, kept)
}

/// The spot book: USDC (token 0), and PURR, HYPE and USDT0 each paired with it.
pub const SPOT_META: &str = concat!(
    r#"{"tokens":[{"name":"USDC","index":0,"szDecimals":8},{"name":"PURR","index":1,"szDecimals":0},{"name":"HYPE","index":150,"szDecimals":2},"#,
    r#"{"name":"USDT0","index":268,"szDecimals":2}],"universe":[{"tokens":[1,0],"index":0,"name":"PURR/USDC"},"#,
    r#"{"tokens":[150,0],"index":107,"name":"@107"},{"tokens":[268,0],"index":166,"name":"@166"}]}"#,
);

/// Hyperliquid with the spot book and nothing else («null» to every other info request), in a FakeHL turn. Keep it
/// alive for the test.
pub struct SpotHl {
    pub asked: Asked,
    _turn: Fake,
}

pub async fn spot_hl() -> SpotHl {
    let turn = Fake::start(|_| {}).await;
    let (url, asked) = serve(|path, body| {
        Some(if path == "/info" && body["type"] == "spotMeta" { SPOT_META.into() } else if path == "/exchange" { r#"{"status":"err"}"#.into() } else { "null".into() })
    }).await;
    crate::set_hl_api(Some(url));
    SpotHl { asked, _turn: turn }
}

impl SpotHl {
    /// What went to /exchange.
    pub fn sent(&self) -> usize { self.asked.lock().unwrap().iter().filter(|(p, _)| p == "/exchange").count() }
}

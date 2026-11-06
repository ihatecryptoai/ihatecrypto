//! FakeHL (ExecutorCloseTests.swift): a Hyperliquid with one market (ETH, mark 3000) at 127.0.0.1. /info answers from
//! `State`, /exchange writes down what was sent and answers the entry and the close as `State.entry` and `State.close`
//! say (None: its reply is lost, the connection dropped). One test at a time: the crate's Hyperliquid address is global.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use execution::Journal;
use execution::trade_signer::{Signing, TradeError, TradeSigner, TradeWallet};
use hyperliquid::MsgPack;
use ihatecrypto_core::foundation::text;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wallet::evm::EvmSignature;

use crate::Executor;

pub type Reply = Box<dyn FnMut(&mut State) -> Option<String> + Send>;

pub struct State {
    pub position: Decimal,
    pub orders: Vec<Value>,
    pub sent: Vec<String>,
    pub close: Reply,
    /// clearinghouseState times out.
    pub positions_down: bool,
    /// This many reads of the main exchange's open orders or positions time out first.
    pub down: u32,
    /// The entry — a market IOC, or a limit with its stop and take in one normalTpsl group — as the exchange answers it.
    pub entry: Reply,
    /// Stops and takes for a position (positionTpsl) turned down before one is taken.
    pub refusals: u32,
    /// This many stops or takes go on the book with their reply lost.
    pub lost: u32,
    /// The positionTpsl orders sent, and the executor's report lines.
    pub protections: Vec<Value>,
    pub reported: Vec<String>,
    /// twapHistory's rows; a twapOrder is answered by `entry`, a twapCancel stops the row with its id.
    pub twaps: Vec<Value>,
    /// A twapCancel is answered with an error the TWAP isn't stopped by.
    pub twap_cancel_refused: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            position: Decimal::ZERO, orders: vec![], sent: vec![], close: Box::new(|_| None), positions_down: false, down: 0, entry: Box::new(|_| None),
            refusals: 0, lost: 0, protections: vec![], reported: vec![], twaps: vec![], twap_cancel_refused: false,
        }
    }
}

pub fn stop(oid: u64, px: &str) -> Value { stop_side(oid, px, "A") }
pub fn stop_side(oid: u64, px: &str, side: &str) -> Value {
    json!({"coin": "ETH", "oid": oid, "isTrigger": true, "triggerPx": px, "orderType": "Stop Market", "reduceOnly": true, "side": side, "sz": "0.01"})
}
pub fn take(oid: u64, px: &str) -> Value {
    json!({"coin": "ETH", "oid": oid, "isTrigger": true, "triggerPx": px, "orderType": "Take Profit Market", "reduceOnly": true, "side": "A", "sz": "0.01"})
}
pub fn filled(size: &str) -> String { filled_at(size, "2999") }
pub fn filled_at(size: &str, px: &str) -> String {
    format!(r#"{{"status":"ok","response":{{"type":"order","data":{{"statuses":[{{"filled":{{"totalSz":"{size}","avgPx":"{px}","oid":9}}}}]}}}}}}"#)
}
pub const NO_MATCH: &str = r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"error":"Order could not immediately match against any resting orders. asset=0"}]}}}"#;

fn info(s: &mut State, body: &Value) -> Option<String> {
    let main = body.get("dex").is_none();
    Some(match body.get("type").and_then(Value::as_str) {
        Some("metaAndAssetCtxs") => r#"[{"universe":[{"name":"ETH","szDecimals":4,"maxLeverage":25}]},[{"markPx":"3000"}]]"#.into(),
        Some("perpDexs") => "[]".into(),
        Some("maxBuilderFee") => "0".into(),
        Some("twapHistory") => Value::Array(s.twaps.clone()).to_string(),
        Some("frontendOpenOrders") => {
            if !main { return Some("[]".into()) }
            if s.down > 0 { s.down -= 1; return None }
            Value::Array(s.orders.clone()).to_string()
        }
        Some("clearinghouseState") => {
            if s.positions_down { return None }
            if main && s.down > 0 { s.down -= 1; return None }
            if !main || s.position.is_zero() { return Some(r#"{"assetPositions":[]}"#.into()) }
            format!(r#"{{"assetPositions":[{{"position":{{"coin":"ETH","szi":"{}","entryPx":"3000","unrealizedPnl":"0.1"}}}}]}}"#, text(s.position))
        }
        _ => "null".into(),
    })
}

fn call(s: &mut State, which: fn(&mut State) -> &mut Reply) -> Option<String> {
    let mut reply = std::mem::replace(which(s), Box::new(|_| None));
    let answer = reply(s);
    *which(s) = reply;
    answer
}

fn exchange(s: &mut State, action: &Value) -> Option<String> {
    let kind = action.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "trailingStop" => {
            let pct = action.pointer("/retracement/pct").and_then(Value::as_str).unwrap_or("?");
            s.sent.push(format!("trailing {} {} {pct}", if action["isBuy"] == true { "buy" } else { "sell" }, action["sz"].as_str().unwrap_or("?")));
            return Some(r#"{"status":"ok","response":{"type":"trailingStop","data":{"oid":11}}}"#.into());
        }
        "twapOrder" => {
            s.sent.push(format!("twap {} min", action.pointer("/twap/m").map_or("?".into(), Value::to_string)));
            return call(s, |s| &mut s.entry);
        }
        "twapCancel" => {
            let id = action["t"].as_u64().unwrap_or(0);
            s.sent.push(format!("twap cancel {id}"));
            if s.twap_cancel_refused { return Some(r#"{"status":"ok","response":{"type":"twapCancel","data":{"status":{"error":"Exchange busy."}}}}"#.into()) }
            s.twaps.retain(|t| t["twapId"].as_u64() != Some(id));
            return Some(r#"{"status":"ok","response":{"type":"twapCancel","data":{"status":"success"}}}"#.into());
        }
        "updateLeverage" => {
            s.sent.push("leverage".into());
            return Some(r#"{"status":"ok","response":{"type":"default"}}"#.into());
        }
        "cancel" => {
            let oids: Vec<u64> = action["cancels"].as_array().into_iter().flatten().filter_map(|c| c["o"].as_u64()).collect();
            s.sent.push(format!("cancel {}", oids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")));
            s.orders.retain(|o| !oids.contains(&o["oid"].as_u64().unwrap_or(0)));
            return Some(r#"{"status":"ok","response":{"type":"cancel","data":{"statuses":["success"]}}}"#.into());
        }
        _ => {}
    }
    let orders = action["orders"].as_array().cloned().unwrap_or_default();
    let triggers: Vec<Value> = orders.iter().filter_map(|o| o.pointer("/t/trigger").cloned()).collect();
    let tpsl = triggers.iter().filter_map(|t| t["tpsl"].as_str()).collect::<Vec<_>>().join("+");
    if action["grouping"] == "normalTpsl" {
        s.sent.push(format!("limit {tpsl}"));
        return call(s, |s| &mut s.entry);
    }
    if triggers.is_empty() {
        let o = orders.first().cloned().unwrap_or(Value::Null);
        if o["r"] != true {
            s.sent.push("entry".into());
            return call(s, |s| &mut s.entry);
        }
        s.sent.push(if o["b"] == true { "close buy".into() } else { "close sell".into() });
        return call(s, |s| &mut s.close);
    }
    // A stop put back: listed among the open orders at once, so verify_protection finds it — unless it's refused.
    s.sent.push(tpsl);
    if action["grouping"] == "positionTpsl" { s.protections.extend(orders) }
    if s.refusals > 0 {
        s.refusals -= 1;
        return Some(r#"{"status":"ok","response":{"type":"order","data":{"statuses":[{"error":"Trigger order has invalid price."}]}}}"#.into());
    }
    for t in &triggers { s.orders.push(stop(100, t["triggerPx"].as_str().unwrap_or(""))) }
    if s.lost > 0 { s.lost -= 1; return None }
    Some(r#"{"status":"ok","response":{"type":"order","data":{"statuses":["waitingForTrigger"]}}}"#.into())
}

/// One request off the socket: its path and JSON body.
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

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub struct Fake {
    pub state: Arc<Mutex<State>>,
    _turn: tokio::sync::MutexGuard<'static, ()>,
}

impl Fake {
    /// The stand-in started, the crate pointed at it, `State` as `setup` leaves it.
    pub async fn start(setup: impl FnOnce(&mut State)) -> Fake {
        let turn = ONE_AT_A_TIME.lock().await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        crate::set_hl_api(Some(format!("http://{}", listener.local_addr().unwrap())));
        let state = Arc::new(Mutex::new(State::default()));
        setup(&mut state.lock().unwrap());
        let shared = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let state = shared.clone();
                tokio::spawn(async move {
                    let Some((path, body)) = request(&mut socket).await else { return };
                    let reply = {
                        let mut s = state.lock().unwrap();
                        if path == "/exchange" { exchange(&mut s, &body["action"]) } else { info(&mut s, &body) }
                    };
                    // No reply: the connection drops, as a request that timed out.
                    let Some(reply) = reply else { return };
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                    socket.write_all(response.as_bytes()).await.ok();
                    socket.shutdown().await.ok();
                });
            }
        });
        Fake { state, _turn: turn }
    }

    pub fn state(&self) -> MutexGuard<'_, State> { self.state.lock().unwrap() }

    /// A fresh State as `setup` leaves it.
    pub fn reset(&self, setup: impl FnOnce(&mut State)) {
        let mut s = self.state();
        *s = State::default();
        setup(&mut s);
    }

    pub fn sent(&self) -> Vec<String> { self.state().sent.clone() }

    /// An executor on wallet 0x0 whose report lines go to `State.reported`.
    pub fn executor(&self, signer: ZeroSigner) -> Arc<Executor> {
        let state = self.state.clone();
        Executor::new(TradeWallet { solana: String::new(), evm: "0x0".into() }, Arc::new(signer), journal("fake"),
                      move |line| state.lock().unwrap().reported.push(line.into()))
    }
}

/// A scratch journal of its own.
pub fn journal(name: &str) -> Journal {
    static N: AtomicU64 = AtomicU64::new(0);
    Journal::new(std::env::temp_dir().join(format!("trade-{name}-{}-{}.jsonl", std::process::id(), N.fetch_add(1, Ordering::Relaxed))))
}

/// Signs with zeros; `refuses`: the signer turns every Hyperliquid action down (Touch ID declined, the guard).
/// `refuses_orders`: only orders are turned down, with this error (leverage before them is signed).
#[derive(Default, Clone)]
pub struct ZeroSigner {
    pub refuses: bool,
    pub refuses_orders: Option<TradeError>,
    pub batches: bool,
}

fn no<T: Send + 'static>() -> Signing<'static, T> { Box::pin(async { Err(TradeError::Failed("no".into())) }) }

impl TradeSigner for ZeroSigner {
    fn sign_solana(&self, _: Vec<u8>, _: Vec<String>, _: Vec<String>, _: BTreeMap<String, String>) -> Signing<'_, Vec<u8>> { no() }
    fn sign_hyperliquid(&self, action: MsgPack, _: u64) -> Signing<'_, EvmSignature> {
        let answer = if self.refuses {
            Err(TradeError::Guard("declined".into()))
        } else if let Some(e) = self.refuses_orders.clone().filter(|_| hyperliquid::action::kind(&action) == Some("order")) {
            Err(e)
        } else {
            Ok(EvmSignature { r: vec![0; 32], s: vec![0; 32], v: 27 })
        };
        Box::pin(async move { answer })
    }
    fn sign_hyperliquid_user(&self, _: MsgPack, _: Vec<String>) -> Signing<'_, EvmSignature> { no() }
    fn sign_relay_mapping(&self, _: String, _: String, _: Vec<u8>, _: u64) -> Signing<'_, EvmSignature> { no() }
    fn sign_evm(&self, _: execution::trade_signer::EvmFees, _: String, _: Vec<u8>, _: Vec<String>) -> Signing<'_, Vec<u8>> { no() }
    fn sign_near_intent_deposit(&self, _: u128, _: String, _: u64, _: Vec<u8>) -> Signing<'_, Vec<u8>> { no() }
    fn signs_batches(&self) -> bool { self.batches }
}

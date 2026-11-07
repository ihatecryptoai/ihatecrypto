//! NEAR Intents' Agent API (docs.agentsonintents.com, `AgentAPI.swift`): a wallet for an agent, held by NEAR Intents'
//! custody under a policy its owner signs — what it may do, with which coins, how much a day, to which addresses. The
//! owner (this wallet's NEAR account) signs each change through the signer (NEP-413 / NEP-366); the agent then acts with
//! a grant token alone, and the API refuses what the policy doesn't allow. Mainnet only: the demo host moves real money
//! too.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wallet::WalletError;
use wallet::near_owner::raw_value;

/// sha256 of a policy written with sorted keys and no spaces: how the API names a policy (preview.policy_hash).
pub use wallet::agent_owner::policy_hash;

/// The hackathon's demo host (its partner keys are temporary); the production one is api.agentsonintents.com.
pub const DEMO_HOST: &str = "api.demo.agentsonintents.com";
pub const HOSTS: [&str; 2] = [DEMO_HOST, "api.agentsonintents.com"];

static KEY: Mutex<Option<String>> = Mutex::new(None);
static HOST: Mutex<Option<String>> = Mutex::new(None);

/// The partner key (naa_…), from the vault; never in a prompt or a file. "" is none.
pub fn key() -> Option<String> { KEY.lock().expect("key").clone() }
pub fn set_key(key: Option<String>) { *KEY.lock().expect("key") = key.filter(|k| !k.is_empty()) }

pub fn host() -> String { HOST.lock().expect("host").clone().unwrap_or_else(|| DEMO_HOST.into()) }
/// One of `HOSTS`; anything else is the demo host.
pub fn set_host(host: &str) { *HOST.lock().expect("host") = HOSTS.contains(&host).then(|| host.to_string()) }

fn base() -> String { format!("https://{}", host()) }

/// An owner intent waiting for a signature: `payload` is the raw JSON to sign, exactly as the API wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    pub correlation_id: String,
    pub ty: String,
    pub agent_id: Option<String>,
    pub standard: String,
    pub payload: Vec<u8>,
    pub signer_account: Option<String>,
    pub expires_at: Option<String>,
    /// What the API says the owner installs: the policy and its hash (`policy_hash`).
    pub preview_policy: Option<Vec<u8>>,
    pub preview_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub status: String,
    pub details: Option<Vec<u8>>,
}

impl Status {
    pub fn is_final(&self) -> bool { ["SUCCESS", "REFUNDED", "FAILED", "NEEDS_REVIEW"].contains(&self.status.as_str()) }
    pub fn ok(&self) -> bool { self.status == "SUCCESS" }
}

/// An error as the API gives it: `code` to branch on, the title and detail to show. `status` 0: no answer (a dropped
/// connection, a timeout — Swift's URLError), so what was asked may still run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub status: u16,
    pub code: String,
    pub text: String,
}

impl Failure {
    pub fn new(status: u16, data: &[u8]) -> Self {
        let first = serde_json::from_slice::<Value>(data).ok().and_then(|v| v.get("errors")?.as_array()?.first().cloned()).unwrap_or(Value::Null);
        let code = first["code"].as_str().map_or_else(|| format!("http_{status}"), str::to_string);
        let text = [first["title"].as_str(), first["detail"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(": ");
        let text = if text.is_empty() { String::from_utf8_lossy(&data[..data.len().min(200)]).into_owned() } else { text };
        Failure { status, code, text }
    }

    fn no_answer(e: reqwest::Error) -> Self { Failure { status: 0, code: "no_answer".into(), text: format!("The Agent API didn't answer: {e}") } }

    pub fn is_no_answer(&self) -> bool { self.status == 0 }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "Agent API: {} ({})", self.text, self.code) }
}

impl From<Failure> for WalletError {
    fn from(f: Failure) -> Self { WalletError(f.to_string()) }
}

pub type Result<T> = std::result::Result<T, Failure>;

/// One call. `grant`: the agent's grant token, for its actions; `idempotency`: required on money actions, reused on a
/// retry of the same body.
pub async fn call(method: &str, path: &str, body: Option<&[u8]>, grant: Option<&str>, idempotency: Option<&str>) -> Result<Vec<u8>> {
    call_at(&base(), method, path, body, grant, idempotency).await
}

pub async fn call_at(base: &str, method: &str, path: &str, body: Option<&[u8]>, grant: Option<&str>, idempotency: Option<&str>) -> Result<Vec<u8>> {
    let Some(key) = key() else { return Err(Failure::new(401, br#"{"errors":[{"code":"no_key","title":"No Agent API key: add it in Settings"}]}"#)) };
    let Ok(method) = reqwest::Method::from_bytes(method.as_bytes()) else { return Err(Failure::new(400, format!("Not an HTTP method: {method}").as_bytes())) };
    let mut r = net::HTTP.request(method, format!("{base}{path}")).timeout(Duration::from_secs(60)).header("X-API-Key", key);
    if let Some(grant) = grant { r = r.header("X-Grant-Token", grant) }
    if let Some(idempotency) = idempotency { r = r.header("Idempotency-Key", idempotency) }
    if let Some(body) = body { r = r.header("Content-Type", "application/json").body(body.to_vec()) }
    let response = r.send().await.map_err(Failure::no_answer)?;
    let code = response.status().as_u16();
    let data = response.bytes().await.map_err(Failure::no_answer)?.to_vec();
    if !(200..300).contains(&code) { return Err(Failure::new(code, &data)) }
    Ok(data)
}

/// Why the API won't take `key`, or None: GET /v1/whoami asked with it.
pub async fn problem_with_key(key: &str) -> Option<String> {
    let asked = net::HTTP.get(format!("{}/v1/whoami", base())).timeout(Duration::from_secs(30)).header("X-API-Key", key).send().await;
    let answer = match asked {
        Ok(response) => { let code = response.status().as_u16(); response.bytes().await.ok().map(|data| (code, data)) }
        Err(_) => None,
    };
    let Some((code, data)) = answer else { return Some("The Agent API didn't answer. Try again in a minute.".into()) };
    (!(200..300).contains(&code)).then(|| Failure::new(code, &data).to_string())
}

/// POST /v1/generate-intent: what the owner signs for `body` (a JSON object: its `type` and fields).
pub async fn generate(body: &[u8], idempotency: Option<&str>) -> Result<Generated> {
    parse_generated(&call("POST", "/v1/generate-intent", Some(body), None, idempotency).await?)
}

pub fn parse_generated(data: &[u8]) -> Result<Generated> {
    let bad = || Failure::new(200, data);
    let o: Value = serde_json::from_slice(data).map_err(|_| bad())?;
    let text = |v: &Value| v.as_str().map(str::to_string);
    let (Some(correlation_id), Some(standard)) = (text(&o["correlation_id"]), o.get("intent").filter(|i| i.is_object()).and_then(|i| text(&i["standard"]))) else {
        return Err(bad());
    };
    let payload = raw_value("intent", data).and_then(|intent| raw_value("payload", intent)).map_err(|_| bad())?.to_vec();
    let preview = &o["preview"];
    Ok(Generated {
        correlation_id, ty: text(&o["type"]).unwrap_or_default(), agent_id: text(&o["agent_id"]), standard, payload,
        signer_account: text(&o["signer"]["account_id"]), expires_at: text(&o["expires_at"]),
        preview_policy: preview.get("policy").filter(|p| p.is_object()).map(|p| p.to_string().into_bytes()), preview_hash: text(&preview["policy_hash"]),
    })
}

/// POST /v1/submit-intent with what the signer gave: sending the same signature again is safe.
pub async fn submit(g: &Generated, signed_data: &[u8]) -> Result<()> {
    call("POST", "/v1/submit-intent", Some(&submit_body(g, signed_data)), None, None).await.map(|_| ())
}

/// signed_data carries the payload byte for byte as it came, so it's spliced in, not re-encoded.
pub fn submit_body(g: &Generated, signed_data: &[u8]) -> Vec<u8> {
    let mut body = json!({"type": g.ty, "correlation_id": g.correlation_id}).to_string().into_bytes();
    body.pop();
    body.extend(br#","signed_data":"#);
    body.extend(signed_data);
    body.push(b'}');
    body
}

/// GET /v1/status, waiting up to `wait_ms` on the server for a change (Swift's default: 30000).
pub async fn status(correlation_id: &str, wait_ms: u64) -> Result<Status> { status_at(&base(), correlation_id, wait_ms).await }

async fn status_at(base: &str, correlation_id: &str, wait_ms: u64) -> Result<Status> {
    let path = format!("/v1/status?correlation_id={}&wait_ms={wait_ms}", crate::query(correlation_id));
    parse_status(&call_at(base, "GET", &path, None, None, None).await?)
}

pub fn parse_status(data: &[u8]) -> Result<Status> {
    let o: Value = serde_json::from_slice(data).map_err(|_| Failure::new(200, data))?;
    Ok(Status { status: o["status"].as_str().unwrap_or("UNKNOWN").into(), details: o.get("details").filter(|d| d.is_object()).map(|d| d.to_string().into_bytes()) })
}

/// Until it's final (Swift's `final`, 600 s by default): UNCERTAIN keeps being asked, NEEDS_REVIEW stops (never retried).
/// No answer while asking is asked again until `within` passes: the operation runs on regardless, and giving up on the
/// first one read a send that went as «failed» (2026-11-05).
pub async fn until_final(correlation_id: &str, within: Duration) -> Result<Status> {
    until_final_at(&base(), correlation_id, within, Duration::from_secs(3)).await
}

pub async fn until_final_at(base: &str, correlation_id: &str, within: Duration, pause: Duration) -> Result<Status> {
    let end = Instant::now() + within;
    loop {
        match status_at(base, correlation_id, 30_000).await {
            Ok(s) if s.is_final() || Instant::now() > end => return Ok(s),
            Ok(_) => {}
            Err(e) if e.is_no_answer() && Instant::now() < end => tokio::time::sleep(pause).await,
            Err(e) => return Err(e),
        }
    }
}

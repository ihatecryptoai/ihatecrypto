//! ChatGPT Plus/Pro through the Codex backend (`ChatGPT/ChatGPTSubscription.swift`): the sign-in OpenClaw and OpenCode
//! use, with Codex CLI's public OAuth client; the requests name ihatecrypto as their originator. Calls count against the
//! person's ChatGPT plan. OpenAI tolerates this publicly but it isn't in their terms: the key options must keep working
//! without it.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::catalog::{Info, Kind};
use crate::http::{self, Lines};
use crate::oauth::{self, LoopbackServer, Pkce};
use crate::usage::{self, Usage};
use crate::{Credential, Error, OAuthTokens, Partial, Report, Request, Result, Role, Save, fail, now};

// Codex CLI's public OAuth client and its registered redirect.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";
pub const REDIRECT: &str = "http://localhost:1455/auth/callback";
pub const BACKEND: &str = "https://chatgpt.com/backend-api/codex/responses";
pub const MODELS: &str = "https://chatgpt.com/backend-api/codex/models";

pub fn info() -> Info {
    let mut info = Info::new("chatgpt", "ChatGPT (subscription)", Kind::Subscription, "gpt-6.1-sol",
                             "Plus or Pro. Signs in through OpenAI's Codex CLI sign-in, as OpenClaw and OpenCode do. OpenAI doesn't officially allow other apps to use it — a gray area. Usage counts against the subscription limits.")
        // The live list replaces these once signed in.
        .models(&["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna"]);
    info.sign_in_label = Some("Sign in with ChatGPT".into());
    info.existing_login_label = Some("Use the Codex CLI login".into());
    info
}

/// Where Codex CLI keeps its ChatGPT login: ~/.codex/auth.json (%USERPROFILE%\.codex\auth.json on Windows).
pub fn codex_auth_file() -> PathBuf {
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    PathBuf::from(home).join(".codex").join("auth.json")
}

/// The models this subscription offers, live from the Codex backend, recommended first; the checked ones when it can't
/// be read. Tokens are never refreshed here: that happens only with a request.
pub(crate) async fn models(credential: Option<&Credential>, endpoint: &str) -> Vec<String> {
    let access = match credential {
        Some(Credential::OAuth(t)) if t.expires > now() => t.account_id.clone().map(|a| (t.access.clone(), a)),
        Some(Credential::External { path }) => read_codex(path).await.ok(),
        _ => None,
    };
    let Some((token, account)) = access else { return info().models };
    let bearer = format!("Bearer {token}");
    let Ok(data) = http::get(&format!("{endpoint}?client_version=0.200.0"), &[("Authorization", &bearer), ("chatgpt-account-id", &account),
                                                                              ("originator", "ihatecrypto")], 15).await else { return info().models };
    // The same list says each model's context window: kept for the client, which reports it with an answer.
    for (model, window) in windows(&data) { usage::tell_window(&model, window) }
    let live = listed(&data);
    if live.is_empty() { info().models } else { live }
}

fn rows(data: &[u8]) -> Vec<Value> {
    serde_json::from_slice::<Value>(data).ok().and_then(|j| j.get("models")?.as_array().cloned()).unwrap_or_default()
}

/// Each model's context window in a Codex backend reply.
pub fn windows(data: &[u8]) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    for row in rows(data) {
        let (Some(slug), Some(window)) = (row.get("slug").and_then(Value::as_str), usage::int(row.get("context_window"))) else { continue };
        if !out.iter().any(|(s, _)| s == slug) { out.push((slug.into(), window)) }
    }
    out
}

/// The models a Codex backend reply lists for people to pick («visibility: list»), by its priority.
pub fn listed(data: &[u8]) -> Vec<String> {
    let mut rows: Vec<Value> = rows(data).into_iter().filter(|r| r.get("visibility").and_then(Value::as_str) == Some("list")).collect();
    rows.sort_by_key(|r| r.get("priority").and_then(Value::as_i64).unwrap_or(i64::MAX));
    rows.iter().filter_map(|r| r.get("slug").and_then(Value::as_str).map(str::to_string)).collect()
}

pub(crate) fn existing_login() -> Option<Credential> {
    let path = codex_auth_file().to_string_lossy().into_owned();
    // In its container the engine can't open the person's folders: the window says whether the file is there, and reads
    // it only once the person has picked it (`read_codex`).
    if let Some(broker) = net::broker::get() { return broker.find("codex").then_some(Credential::External { path }) }
    read_codex_file(&path).ok().map(|_| Credential::External { path })
}

pub(crate) async fn sign_in(open: &(dyn Fn(String) + Send + Sync), issuer: &str) -> Result<Credential> {
    let pkce = Pkce::make();
    let state = Pkce::make().verifier;
    let server = LoopbackServer::start(1455, "/auth/callback").await?;
    open(http::with_query(&format!("{issuer}/oauth/authorize"), &[
        ("response_type", "code"), ("client_id", CLIENT_ID), ("redirect_uri", REDIRECT), ("scope", "openid profile email offline_access"),
        ("code_challenge", &pkce.challenge), ("code_challenge_method", "S256"), ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"), ("state", &state), ("originator", "codex_cli_rs"),
    ]));
    let query = server.wait_for_callback(Duration::from_secs(300)).await?;
    let code = match (query.get("state"), query.get("code")) {
        (Some(s), Some(code)) if *s == state => code.clone(),
        _ => return fail(query.get("error_description").cloned().unwrap_or_else(|| "ChatGPT sign-in cancelled.".into())),
    };
    let data = http::post_form(&format!("{issuer}/oauth/token"), &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", REDIRECT),
                                                                   ("client_id", CLIENT_ID), ("code_verifier", &pkce.verifier)], 15).await?;
    Ok(Credential::OAuth(tokens(&data, None)?))
}

pub fn tokens(data: &[u8], previous: Option<&OAuthTokens>) -> Result<OAuthTokens> {
    let r: Value = serde_json::from_slice(&data).map_err(|_| Error::Llm("ChatGPT's token answer couldn't be read.".into()))?;
    let Some(access) = r.get("access_token").and_then(Value::as_str) else { return fail("ChatGPT's token answer couldn't be read.") };
    let text = |k: &str| r.get(k).and_then(Value::as_str);
    let Some(refresh) = text("refresh_token").map(str::to_string).or_else(|| previous.map(|p| p.refresh.clone())) else {
        return fail("ChatGPT didn't return a refresh token.");
    };
    Ok(OAuthTokens {
        access: access.into(),
        refresh,
        expires: oauth::expiry(access).unwrap_or(now() + 3600.0),
        account_id: text("id_token").and_then(account_id).or_else(|| account_id(access)).or_else(|| previous.and_then(|p| p.account_id.clone())),
    })
}

pub fn account_id(jwt: &str) -> Option<String> {
    oauth::claims(jwt)?.get("https://api.openai.com/auth")?.get("chatgpt_account_id")?.as_str().map(str::to_string)
}

/// Codex CLI's login file: its access token and account. Codex CLI refreshes it itself. In the container the window
/// reads it — the file the person picked once in its Open dialog — and hands over its tokens without the refresh token.
pub async fn read_codex(path: &str) -> Result<(String, String)> {
    let Some(broker) = net::broker::get() else { return read_codex_file(path) };
    // The first time the window asks the person to pick it: off the engine's thread while the dialog waits.
    let answer = tokio::task::spawn_blocking(move || broker.call(json!({ "op": "codex" }), Duration::from_secs(600))).await
        .map_err(|e| Error::Io(e.to_string()))?.map_err(Error::Llm)?;
    read_codex_text(answer["text"].as_str().unwrap_or("").as_bytes())
}

/// The same read here, outside a container.
pub fn read_codex_file(path: &str) -> Result<(String, String)> {
    read_codex_text(&std::fs::read(path).map_err(|e| Error::Io(e.to_string()))?)
}

/// The access token and account in a Codex CLI login file's text.
pub fn read_codex_text(data: &[u8]) -> Result<(String, String)> {
    let file: Value = serde_json::from_slice(data).map_err(|_| Error::Llm("Codex CLI isn't signed in with ChatGPT.".into()))?;
    let t = file.get("tokens");
    let access = t.and_then(|t| t.get("access_token")).and_then(Value::as_str);
    let account = t.and_then(|t| t.get("account_id")).and_then(Value::as_str).map(str::to_string)
        .or_else(|| t.and_then(|t| t.get("id_token")).and_then(Value::as_str).and_then(account_id));
    match (access, account) {
        (Some(a), Some(acc)) => Ok((a.into(), acc)),
        _ => fail("Codex CLI isn't signed in with ChatGPT."),
    }
}

pub(crate) enum Source { Own(OAuthTokens), CodexFile(String) }

pub struct ChatGptClient {
    source: Mutex<Source>,
    save: Save,
    pub(crate) issuer: String,
    pub(crate) backend: String,
}

/// One id for this run of the app: with a cache key from the instructions, the same system prompt every turn is read
/// from the backend's prompt cache instead of anew.
static SESSION: LazyLock<String> = LazyLock::new(|| {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system's randomness");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
});

impl ChatGptClient {
    pub(crate) fn new(source: Source, save: Save) -> Self {
        Self { source: Mutex::new(source), save, issuer: ISSUER.into(), backend: BACKEND.into() }
    }

    pub(crate) async fn stream(&self, r: &Request, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        let (token, account) = self.credentials().await?;
        let bearer = format!("Bearer {token}");
        let mut lines = Lines::post(&self.backend, &Self::body(r), &[
            ("Authorization", &bearer), ("chatgpt-account-id", &account), ("OpenAI-Beta", "responses=experimental"),
            ("originator", "ihatecrypto"), ("session_id", &SESSION), ("Accept", "text/event-stream"),
        ], 120).await?;
        let mut stream = CodexStream::default();
        while let Some(line) = lines.next().await? {
            let before = stream.text.len();
            if stream.feed(&line)? { break }
            if stream.text.len() != before { partial(&stream.text) }
        }
        if stream.text.is_empty() { return fail("ChatGPT returned an empty reply.") }
        if let Some(mut u) = stream.usage {
            u.window = usage::told_window(&r.model);
            report(u);
        }
        Ok(stream.text)
    }

    /// Own tokens are refreshed here. Codex CLI's are never refreshed by us: the refresh token rotates, and refreshing
    /// it would log Codex CLI out.
    async fn credentials(&self) -> Result<(String, String)> {
        let mut source = self.source.lock().await;
        match &mut *source {
            Source::CodexFile(path) => {
                let (access, account) = read_codex(path).await?;
                if oauth::expiry(&access).is_some_and(|exp| exp < now()) {
                    return fail("The Codex CLI login expired. Run codex once or sign in via the browser.");
                }
                Ok((access, account))
            }
            Source::Own(tokens) => {
                // The lock holds one refresh at a time: the rotating refresh token is spent once.
                if tokens.expires < now() + 300.0 {
                    let data = http::post_form(&format!("{}/oauth/token", self.issuer), &[
                        ("grant_type", "refresh_token"), ("client_id", CLIENT_ID), ("refresh_token", &tokens.refresh), ("scope", "openid profile email"),
                    ], 15).await?;
                    *tokens = self::tokens(&data, Some(tokens))?;
                    (self.save)(Credential::OAuth(tokens.clone()));
                }
                let Some(account) = tokens.account_id.clone() else { return fail("No chatgpt_account_id. Sign in again.") };
                Ok((tokens.access.clone(), account))
            }
        }
    }

    pub fn body(r: &Request) -> Value {
        let input: Vec<Value> = r.dialogue().iter().map(|m| json!({
            "type": "message", "role": m.role.as_str(),
            "content": [{"type": if m.role == Role::Assistant { "output_text" } else { "input_text" }, "text": m.text}],
        })).collect();
        let system = r.system();
        let mut body = json!({"model": r.model, "instructions": system, "input": input, "store": false, "stream": true,
                              "prompt_cache_key": format!("ihatecrypto-{}", cache_key(&system))});
        // Left to the model's own default unless asked to think it through.
        if r.thinking { body["reasoning"] = json!({"effort": "high"}) }
        body
    }
}

/// FNV-1a of the instructions in base 36, the same across launches: a changed prompt gets a cache of its own.
pub fn cache_key(text: &str) -> String {
    let mut h = text.bytes().fold(14_695_981_039_346_656_037u64, |h, b| (h ^ b as u64).wrapping_mul(1_099_511_628_211));
    if h == 0 { return "0".into() }
    let mut digits = Vec::new();
    while h > 0 {
        digits.push(std::char::from_digit((h % 36) as u32, 36).expect("a digit"));
        h /= 36;
    }
    digits.iter().rev().collect()
}

/// Collects text from the Responses API event stream.
#[derive(Debug, Default)]
pub struct CodexStream {
    pub text: String,
    /// What the completed response says it read and wrote.
    pub usage: Option<Usage>,
}

impl CodexStream {
    /// True when the response is complete.
    pub fn feed(&mut self, line: &str) -> Result<bool> {
        let Some(Ok(Value::Object(event))) = line.strip_prefix("data:").map(serde_json::from_str::<Value>) else { return Ok(false) };
        let Some(kind) = event.get("type").and_then(Value::as_str) else { return Ok(false) };
        match kind {
            "response.output_text.delta" => self.text.push_str(event.get("delta").and_then(Value::as_str).unwrap_or("")),
            "response.completed" => {
                self.usage = Usage::reported(event.get("response").and_then(|r| r.get("usage")));
                return Ok(true);
            }
            "response.failed" | "response.incomplete" | "error" => {
                let said = event.get("response").and_then(|r| r.get("error")).and_then(|e| e.get("message")).and_then(Value::as_str)
                    .or_else(|| event.get("message").and_then(Value::as_str)).unwrap_or(kind);
                return fail(format!("ChatGPT: {said}"));
            }
            _ => {}
        }
        Ok(false)
    }
}

pub(crate) fn client(credential: Option<&Credential>, save: Save) -> Result<ChatGptClient> {
    match credential {
        Some(Credential::OAuth(t)) => Ok(ChatGptClient::new(Source::Own(t.clone()), save)),
        Some(Credential::External { path }) => Ok(ChatGptClient::new(Source::CodexFile(path.clone()), save)),
        _ => fail("ChatGPT: sign in via the browser or use the Codex CLI login."),
    }
}

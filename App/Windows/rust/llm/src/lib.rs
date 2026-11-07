//! Model access for the engine (`App/Sources/LLM`): the chat's request, a ready client to one model service, and every
//! way in — API keys (`api_key`, `anthropic`, `open_router`, `near_ai` with its enclave check), subscriptions (`chatgpt`, `claude_code`) and local servers —
//! listed in `catalog`. Logins aren't kept here: the caller hands a `Credential` in and is handed refreshed ones back
//! (`Save`), and keeps them itself (the engine's DPAPI file, in the Swift `Credential`'s own JSON).

pub mod anthropic;
pub mod api_key;
pub mod catalog;
pub mod chatgpt;
pub mod claude_code;
pub mod http;
pub mod near_ai;
pub mod near_attestation;
pub mod oauth;
pub mod open_router;
pub mod usage;

use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

pub use catalog::Provider;
pub use usage::Usage;

/// Why a model wasn't reached or didn't answer. `Llm` is Swift's LLMError (said as it is), `Net` its NetError — a
/// service's refusal, «HTTP 400: …», which the clients read to ask again without what was refused — and `Io` anything
/// on the way (a timeout, a dropped connection, a program that wouldn't start).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Llm(String),
    Net(String),
    Io(String),
}

impl Error {
    pub fn message(&self) -> &str {
        match self { Error::Llm(m) | Error::Net(m) | Error::Io(m) => m }
    }

    /// Swift's `"\(error)".localizedCaseInsensitiveContains(word)`.
    pub(crate) fn says(&self, word: &str) -> bool { self.message().to_lowercase().contains(&word.to_lowercase()) }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.message()) }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn fail<T>(m: impl Into<String>) -> Result<T> { Err(Error::Llm(m.into())) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role { System, User, Assistant }

impl Role {
    pub fn as_str(self) -> &'static str {
        match self { Role::System => "system", Role::User => "user", Role::Assistant => "assistant" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self { Self { role, text: text.into() } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub model: String,
    pub messages: Vec<Message>,
    /// Ask for a bare JSON object where the API supports it. Callers still parse leniently (`json_object`).
    pub wants_json: bool,
    /// Let the model reason before it answers (Settings › Models › «Think before answering»). Each service gets it its
    /// own way; one that doesn't know it answers as usual.
    pub thinking: bool,
}

impl Request {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self { model: model.into(), messages, wants_json: false, thinking: false }
    }

    pub fn system(&self) -> String {
        self.messages.iter().filter(|m| m.role == Role::System).map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n\n")
    }

    pub fn dialogue(&self) -> Vec<&Message> { self.messages.iter().filter(|m| m.role != Role::System).collect() }

    pub(crate) fn messages_json(&self) -> Value {
        Value::Array(self.messages.iter().map(|m| json!({"role": m.role.as_str(), "content": m.text})).collect())
    }
}

/// The JSON object in a model reply, whether bare, fenced in ```json or surrounded by prose.
pub fn json_object(text: &str) -> Option<&str> {
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    (start < end).then(|| &text[start..=end])
}

/// Told the answer's text so far as it arrives.
pub type Partial<'a> = &'a (dyn Fn(&str) + Send + Sync);
/// Told what an answer used, as its service counted it (Swift's `LLMUsage.report`).
pub(crate) type Report<'a> = &'a (dyn Fn(Usage) + Send + Sync);
/// Told what each answer used, with the request it answered (Swift's `MeteredClient`).
pub type Heard = Arc<dyn Fn(&Usage, &Request) + Send + Sync>;
/// Given refreshed credentials, so they survive a restart.
pub type Save = Arc<dyn Fn(Credential) + Send + Sync>;

pub(crate) enum Inner {
    OpenAi(api_key::OpenAiChatClient),
    Ollama(api_key::OllamaChatClient),
    Anthropic(anthropic::AnthropicClient),
    ChatGpt(chatgpt::ChatGptClient),
    ClaudeCode(claude_code::ClaudeCodeClient),
    NearAi(near_ai::NearAiClient),
}

/// A ready connection to one model service. The engine talks only to this.
pub struct Client {
    pub(crate) inner: Inner,
    thinking: bool,
    heard: Option<Heard>,
}

impl Client {
    pub(crate) fn new(inner: Inner) -> Self { Self { inner, thinking: false, heard: None } }

    /// Every request thinks first, whatever its caller asked (Swift's `ThinkingClient`).
    pub fn thinking(mut self) -> Self {
        self.thinking = true;
        self
    }

    /// Each answer's usage told to `heard` with its request — counted even when the answer then fails (a refusal is
    /// billed for what was read).
    pub fn metered(mut self, heard: Heard) -> Self {
        self.heard = Some(heard);
        self
    }

    pub async fn complete(&self, request: &Request) -> Result<String> { self.ask(request, None).await }

    /// The same answer, with `partial` told the text so far as it arrives, so a reader can start before it ends.
    pub async fn stream(&self, request: &Request, partial: Partial<'_>) -> Result<String> { self.ask(request, Some(partial)).await }

    async fn ask(&self, request: &Request, partial: Option<Partial<'_>>) -> Result<String> {
        let mut r = request.clone();
        if self.thinking { r.thinking = true }
        let report = |u: Usage| { if let Some(h) = &self.heard { h(&u, request) } };
        let none = |_: &str| {};
        match (&self.inner, partial) {
            (Inner::OpenAi(c), None) => c.complete(&r, &report).await,
            (Inner::OpenAi(c), Some(p)) => c.stream(&r, p, &report).await,
            (Inner::Ollama(c), p) => c.stream(&r, p.unwrap_or(&none), &report).await,
            (Inner::Anthropic(c), p) => c.stream(&r, p.unwrap_or(&none), &report).await,
            (Inner::ChatGpt(c), p) => c.stream(&r, p.unwrap_or(&none), &report).await,
            (Inner::ClaudeCode(c), p) => c.stream(&r, p.unwrap_or(&none), &report).await,
            // Not streamed (a streamed answer proves less): told once, at the end.
            (Inner::NearAi(c), p) => {
                let text = c.complete(&r, &report).await?;
                if let Some(p) = p { p(&text) }
                Ok(text)
            }
        }
    }
}

/// Unix seconds now.
pub(crate) fn now() -> f64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()) }

/// Seconds between 1970 and 2001: Swift's `JSONEncoder` writes a `Date` as seconds since 2001.
const REFERENCE_DATE: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq)]
pub struct OAuthTokens {
    pub access: String,
    pub refresh: String,
    /// Unix seconds.
    pub expires: f64,
    pub account_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct TokensWire {
    access: String,
    refresh: String,
    #[serde(serialize_with = "swift_double")]
    expires: f64,
    #[serde(rename = "accountID", default, skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}

/// A whole Double as Swift's JSONEncoder writes it: «800000000», not «800000000.0».
fn swift_double<S: Serializer>(v: &f64, s: S) -> std::result::Result<S::Ok, S::Error> {
    if v.fract() == 0.0 && v.abs() < 9e15 { s.serialize_i64(*v as i64) } else { s.serialize_f64(*v) }
}

impl Serialize for OAuthTokens {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        TokensWire { access: self.access.clone(), refresh: self.refresh.clone(), expires: self.expires - REFERENCE_DATE,
                     account_id: self.account_id.clone() }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for OAuthTokens {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let w = TokensWire::deserialize(d)?;
        Ok(Self { access: w.access, refresh: w.refresh, expires: w.expires + REFERENCE_DATE, account_id: w.account_id })
    }
}

/// What the engine keeps for a provider, in Swift's synthesized `Codable` shape ({"apiKey": {"_0": "…"}}), so the
/// credentials the Swift engine saved still open.
#[derive(Debug, Clone, PartialEq)]
pub enum Credential {
    ApiKey(String),
    OAuth(OAuthTokens),
    /// A login owned by another tool (Codex CLI's auth file, Claude Code's binary). Only the path is kept.
    External { path: String },
}

impl Serialize for Credential {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Credential::ApiKey(k) => json!({"apiKey": {"_0": k}}),
            Credential::OAuth(t) => json!({"oauth": {"_0": t}}),
            Credential::External { path } => json!({"external": {"path": path}}),
        }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Credential {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let Some((case, body)) = v.as_object().filter(|o| o.len() == 1).and_then(|o| o.iter().next()) else {
            return Err(D::Error::custom("a credential is one case"));
        };
        let field = |name: &str| body.get(name).cloned().ok_or_else(|| D::Error::missing_field(if name == "_0" { "_0" } else { "path" }));
        Ok(match case.as_str() {
            "apiKey" => Credential::ApiKey(serde_json::from_value(field("_0")?).map_err(D::Error::custom)?),
            "oauth" => Credential::OAuth(serde_json::from_value(field("_0")?).map_err(D::Error::custom)?),
            "external" => Credential::External { path: serde_json::from_value(field("path")?).map_err(D::Error::custom)? },
            other => return Err(D::Error::unknown_variant(other, &["apiKey", "oauth", "external"])),
        })
    }
}

#[cfg(test)]
mod tests;

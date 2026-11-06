//! An Anthropic API key on Anthropic's own Messages API (`Anthropic/AnthropicAPI.swift`): the chat's instructions
//! cached, the effort following «Think before answering», the cache's tokens counted, and a request the model's safety
//! check declines answered by the fallback model Anthropic recommends for that kind of refusal.

use serde_json::{Map, Value, json};

use crate::catalog::{Info, Kind};
use crate::http::{self, Lines};
use crate::usage::Usage;
use crate::{Error, Partial, Report, Request, Result, fail};

pub const BASE: &str = "https://api.anthropic.com/v1";

pub fn info() -> Info {
    Info::new("anthropic", "Anthropic API", Kind::ApiKey, "claude-opus-5-5",
              "Key from platform.claude.com/settings/keys › Create key, scoped to a workspace: Anthropic refuses a key without one. Billed separately from a Claude subscription.")
        .key().models(&["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-haiku-5-5"])
}

/// Anthropic's 400 for a key that isn't scoped to a workspace, said as what to do.
pub const NO_WORKSPACE: &str = "This Anthropic key isn't scoped to a workspace, so Anthropic refuses it. Make one that is: platform.claude.com/settings/keys › Create key › scope it to a workspace, then paste it in Settings › AI model.";

fn explained(e: Error) -> Error {
    match e { Error::Net(m) if m.contains("anthropic-workspace-id") => Error::Llm(NO_WORKSPACE.into()), other => other }
}

#[derive(Debug, Clone)]
pub struct AnthropicClient {
    pub key: String,
    pub base: String,
}

pub const VERSION: &str = "2026-10-21";
/// The beta that takes `"fallbacks": "default"`.
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

impl AnthropicClient {
    pub fn new(key: &str) -> Self { Self { key: key.into(), base: BASE.into() } }

    pub fn headers(&self) -> Vec<(&'static str, String)> { vec![("x-api-key", self.key.clone()), ("anthropic-version", VERSION.into())] }

    /// The instructions go as one block marked for the cache (the same in every chat request), a JSON answer is asked
    /// for in the instructions, and «Think before answering» is adaptive thinking at high effort.
    pub fn body(r: &Request, think: bool, cap: i64, fallback: bool) -> Value {
        let messages: Vec<Value> = r.dialogue().iter().map(|m| json!({"role": m.role.as_str(), "content": m.text})).collect();
        // A cap, not a target: thinking counts toward it, and a JSON answer is a few hundred tokens.
        let mut body = json!({"model": r.model, "max_tokens": cap, "stream": true, "messages": messages});
        let system = r.system();
        if !system.is_empty() { body["system"] = json!([{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}]) }
        if think {
            body["thinking"] = json!({"type": "adaptive"});
            body["output_config"] = json!({"effort": "high"});
        }
        // ponytail: a fallback's answer is priced at the asked model's rates, and a declined partial (usage.iterations)
        // isn't counted; read iterations when refusals become common.
        if fallback && falls_back(&r.model) { body["fallbacks"] = json!("default") }
        body
    }

    /// A refused fallback field (its beta renamed or ended) is dropped and the request asked again as it was.
    pub(crate) async fn stream(&self, r: &Request, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        match self.stream_plainer(r, true, partial, report).await {
            Err(Error::Net(m)) if falls_back(&r.model) && m.starts_with("HTTP 400") && m.contains("fallback") => {
                self.stream_plainer(r, false, partial, report).await
            }
            other => other,
        }
    }

    /// An older model is asked once more as plainly as it gets: Haiku 4.5 and before take neither adaptive thinking
    /// nor an effort, and some write 4,096 tokens at most.
    async fn stream_plainer(&self, r: &Request, fallback: bool, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        match self.stream_once(r, r.thinking, 32_000, fallback, partial, report).await {
            Err(Error::Net(m)) if m.starts_with("HTTP 400") && !m.contains("fallback")
                && ["thinking", "effort", "max_tokens"].iter().any(|w| m.contains(w)) => {
                self.stream_once(r, false, 4_096, fallback, partial, report).await
            }
            other => other,
        }
    }

    async fn stream_once(&self, r: &Request, think: bool, cap: i64, fallback: bool, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        let body = Self::body(r, think, cap, fallback);
        let headers = self.headers();
        let mut sent: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        sent.push(("Accept", "text/event-stream"));
        if body.get("fallbacks").is_some() { sent.push(("anthropic-beta", FALLBACK_BETA)) }
        let mut lines = Lines::post(&format!("{}/messages", self.base), &body, &sent, if think { 180 } else { 90 }).await.map_err(explained)?;
        let (mut stream, mut ended) = (AnthropicStream::default(), false);
        while let Some(line) = lines.next().await? {
            let before = stream.text.len();
            if stream.feed(&line)? {
                ended = true;
                break;
            }
            if stream.text.len() != before { partial(&stream.text) }
        }
        // A refusal is billed for what was read: counted before it's said.
        if let Some(u) = stream.usage() { report(u) }
        if stream.stop.as_deref() == Some("refusal") {
            return fail("Claude declined this request (Anthropic's safety check). Put it in other words, or pick another model.");
        }
        // Half an answer is never taken for a whole one.
        if !ended { return fail("Anthropic's answer was cut off on the way. Send it again.") }
        if stream.stop.as_deref() == Some("max_tokens") { return fail("The model's answer ran past its length limit. Send it again, or pick another model.") }
        if stream.text.is_empty() { return fail("The model returned an empty reply.") }
        Ok(stream.text)
    }

    pub async fn models(&self) -> Result<Vec<String>> {
        let headers = self.headers();
        let extra: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let data = http::get(&format!("{}/models?limit=1000", self.base), &extra, 15).await.map_err(explained)?;
        let list: Value = serde_json::from_slice(&data).map_err(|_| Error::Llm("The model list couldn't be read.".into()))?;
        list.get("data").and_then(Value::as_array)
            .and_then(|rows| rows.iter().map(|m| m.get("id").and_then(Value::as_str).map(str::to_string)).collect())
            .ok_or_else(|| Error::Llm("The model list couldn't be read.".into()))
    }
}

/// Opus 5 and 5.5, Sonnet 5.5, Fable 5 and 5.1: the models whose declined request Anthropic answers with a fallback
/// model. Haiku 5.5 has none, nor have Sonnet 5 and the 4.x models.
pub fn falls_back(model: &str) -> bool { ["claude-opus-5", "claude-sonnet-5-5", "claude-fable-5"].iter().any(|p| model.starts_with(p)) }

/// The Messages API's server-sent events, read into the answer's text, why it stopped and what it used.
#[derive(Debug, Default)]
pub struct AnthropicStream {
    pub text: String,
    pub stop: Option<String>,
    /// message_start's usage, updated by each message_delta's (its counts are the totals so far).
    counts: Map<String, Value>,
}

impl AnthropicStream {
    pub fn usage(&self) -> Option<Usage> { Usage::reported(Some(&Value::Object(self.counts.clone()))) }

    /// True when the message is complete.
    pub fn feed(&mut self, line: &str) -> Result<bool> {
        let Some(Ok(Value::Object(event))) = line.strip_prefix("data:").map(serde_json::from_str::<Value>) else { return Ok(false) };
        let Some(kind) = event.get("type").and_then(Value::as_str) else { return Ok(false) };
        match kind {
            "message_start" => {
                self.counts = event.get("message").and_then(|m| m.get("usage")).and_then(Value::as_object).cloned().unwrap_or_default();
            }
            // Thinking comes in blocks of its own: only the answer's text is kept. A fallback block has no deltas, and
            // the fallback model goes on from the text so far.
            "content_block_delta" => {
                if let Some(delta) = event.get("delta").filter(|d| d.get("type").and_then(Value::as_str) == Some("text_delta")) {
                    self.text.push_str(delta.get("text").and_then(Value::as_str).unwrap_or(""));
                }
            }
            "message_delta" => {
                for (k, v) in event.get("usage").and_then(Value::as_object).into_iter().flatten() {
                    if !v.is_null() { self.counts.insert(k.clone(), v.clone()); }
                }
                if let Some(stop) = event.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str) { self.stop = Some(stop.into()) }
            }
            "message_stop" => return Ok(true),
            "error" => {
                let error = event.get("error");
                let said = error.and_then(|e| e.get("message")).and_then(Value::as_str)
                    .or_else(|| error.and_then(|e| e.get("type")).and_then(Value::as_str)).unwrap_or("error");
                return fail(format!("Anthropic: {said}"));
            }
            _ => {}
        }
        Ok(false)
    }
}

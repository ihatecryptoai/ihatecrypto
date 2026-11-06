//! Any service with the OpenAI chat-completions format: DeepSeek, OpenAI, xAI, Gemini, Ollama, LM Studio
//! (`APIKey/OpenAICompatible.swift`). A new one is a new preset, not new code.

use serde_json::{Value, json};

use crate::catalog::{Info, Kind};
use crate::http::{self, Lines};
use crate::usage::Usage;
use crate::{Error, Partial, Report, Request, Result, fail};

/// One OpenAI-compatible service: its listing and where it answers.
#[derive(Debug, Clone)]
pub struct Compatible {
    pub info: Info,
    pub base_url: &'static str,
    pub json_mode: bool,
}

pub fn presets() -> Vec<Compatible> {
    let preset = |info, base_url| Compatible { info, base_url, json_mode: true };
    vec![
        preset(Info::new("deepseek", "DeepSeek API", Kind::ApiKey, "deepseek-flash", "Key from platform.deepseek.com.")
                   .key().models(&["deepseek-flash", "deepseek-v4-pro"]), "https://api.deepseek.com"),
        preset(Info::new("openai", "OpenAI API", Kind::ApiKey, "gpt-6.1-sol", "Key from platform.openai.com, billed separately from a ChatGPT subscription.")
                   .key().models(&["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna"]), "https://api.openai.com/v1"),
        preset(Info::new("xai", "xAI Grok API", Kind::ApiKey, "grok-4.7", "Key from console.x.ai, paid with credits.")
                   .key().models(&["grok-4.7", "grok-4.6"]), "https://api.x.ai/v1"),
        preset(Info::new("gemini", "Google Gemini API", Kind::ApiKey, "gemini-3.8-flash", "Key from Google AI Studio.")
                   .key().models(&["gemini-3.8-flash", "gemini-4-argon"]), "https://generativelanguage.googleapis.com/v1beta/openai"),
        // The biggest first: the engine takes the first that fits this PC's memory. On Windows the words say «this PC»,
        // as the Swift engine rewrote the Mac's.
        preset(Info::new("ollama", "Ollama on this PC", Kind::Local, "gemma4:26b",
                         "No network or key. The app downloads the biggest model that fits this PC's memory.")
                   .models(&["nemotron-3-super:120b", "gemma4:31b", "gemma4:26b", "gemma4:12b"]), "http://localhost:11434/v1"),
        Compatible { info: Info::new("lmstudio", "LM Studio on this PC", Kind::Local, "",
                                     "No network or key. Start the server in LM Studio and pick a model from the list."),
                     base_url: "http://localhost:1234/v1", json_mode: false },
    ]
}

impl Compatible {
    pub(crate) fn chat_client(&self, key: Option<&str>) -> Result<OpenAiChatClient> {
        if self.info.takes_key && key.is_none() { return fail(format!("{}: add a key in Settings.", self.info.name)) }
        Ok(OpenAiChatClient::new(self.base_url, key, self.json_mode))
    }
}

/// Ollama's /api/chat with a context window that holds the instructions, the brief and a long message: `num_ctx` per
/// request, streamed as one JSON object a line. Its OpenAI-style endpoint can't widen its 4096-token window.
#[derive(Debug, Clone)]
pub struct OllamaChatClient {
    pub base: String,
}

impl OllamaChatClient {
    // ponytail: one size for every model; a PC short of memory for 16k on a big model would want it from /api/show.
    pub const CONTEXT: i64 = 16_384;

    /// `think: false`: a thinking model otherwise reasons to itself before its JSON (544 tokens and 18.5 s instead of 19
    /// and 5.3 s on Gemma 4 12B). A model that can't stop thinking is asked again without it.
    pub fn body(&self, r: &Request, stream: bool, think: bool) -> Value {
        let mut body = json!({"model": r.model, "stream": stream, "options": {"num_ctx": Self::CONTEXT}, "messages": r.messages_json()});
        if !think { body["think"] = json!(false) }
        if r.wants_json { body["format"] = json!("json") }
        body
    }

    pub(crate) async fn stream(&self, r: &Request, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        match self.stream_once(r, r.thinking, partial, report).await {
            Err(e) if e.says("think") => self.stream_once(r, !r.thinking, partial, report).await,
            other => other,
        }
    }

    async fn stream_once(&self, r: &Request, think: bool, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        let mut lines = Lines::post(&format!("{}/api/chat", self.base), &self.body(r, true, think), &[], 300).await?;
        let mut text = String::new();
        while let Some(line) = lines.next().await? {
            let Ok(Value::Object(event)) = serde_json::from_str::<Value>(&line) else { continue };
            if let Some(error) = event.get("error").and_then(Value::as_str) { return fail(format!("Ollama: {error}")) }
            if let Some(words) = event.get("message").and_then(|m| m.get("content")).and_then(Value::as_str).filter(|w| !w.is_empty()) {
                text.push_str(words);
                partial(&text);
            }
            if event.get("done") == Some(&json!(true)) {
                // The last line counts what it read and wrote; the window is the one asked for above.
                if let Some(read) = event.get("prompt_eval_count").and_then(Value::as_i64) {
                    report(Usage { window: Some(Self::CONTEXT), ..Usage::new(read, event.get("eval_count").and_then(Value::as_i64).unwrap_or(0)) });
                }
                break;
            }
        }
        if text.is_empty() { return fail("The model returned an empty reply.") }
        Ok(text)
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiChatClient {
    pub base_url: String,
    key: Option<String>,
    json_mode: bool,
}

fn content(v: Option<&Value>) -> Option<String> {
    v?.get("choices")?.get(0)?.get("message")?.get("content")?.as_str().map(str::to_string)
}

impl OpenAiChatClient {
    pub fn new(base_url: &str, key: Option<&str>, json_mode: bool) -> Self {
        Self { base_url: base_url.trim_matches('/').into(), key: key.map(str::to_string), json_mode }
    }

    fn headers(&self) -> Vec<(&str, String)> { self.key.iter().map(|k| ("Authorization", format!("Bearer {k}"))).collect() }

    pub fn body(&self, r: &Request) -> Value {
        let mut body = json!({"model": r.model, "messages": r.messages_json()});
        if r.wants_json && self.json_mode { body["response_format"] = json!({"type": "json_object"}) }
        // OpenAI's, Gemini's and OpenRouter's reasoning models take it; a model that doesn't is asked again without it.
        if r.thinking { body["reasoning_effort"] = json!("medium") }
        body
    }

    /// A model that refuses `reasoning_effort` answers without it.
    fn plain(r: &Request, e: &Error) -> Option<Request> {
        (r.thinking && e.says("reasoning")).then(|| Request { thinking: false, ..r.clone() })
    }

    pub(crate) async fn complete(&self, r: &Request, report: Report<'_>) -> Result<String> {
        match self.complete_once(r, report).await {
            Err(e) => match Self::plain(r, &e) { Some(p) => self.complete_once(&p, report).await, None => Err(e) },
            ok => ok,
        }
    }

    async fn complete_once(&self, r: &Request, report: Report<'_>) -> Result<String> {
        let headers = self.headers();
        let extra: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let data = http::post_json(&format!("{}/chat/completions", self.base_url), &self.body(r), &extra, if r.thinking { 180 } else { 90 }).await?;
        let answer: Value = serde_json::from_slice(&data).map_err(|_| Error::Llm("The model's answer couldn't be read.".into()))?;
        if answer.get("choices").and_then(Value::as_array).is_none() { return fail("The model's answer couldn't be read.") }
        let Some(text) = content(Some(&answer)).filter(|t| !t.is_empty()) else { return fail("The model returned an empty reply.") };
        if let Some(u) = Usage::reported(answer.get("usage")) { report(u) }
        Ok(text)
    }

    /// The same answer as server-sent events, `partial` told the text so far. A server that ignores «stream» and
    /// answers with one body still works.
    pub(crate) async fn stream(&self, r: &Request, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        match self.stream_counted(r, partial, report).await {
            Err(e) => match Self::plain(r, &e) { Some(p) => self.stream_counted(&p, partial, report).await, None => Err(e) },
            ok => ok,
        }
    }

    /// The stream's last event is asked to count the tokens. A server that refuses a request it doesn't know every field
    /// of (400 or 422) is asked again without the option: the count is never worth the answer.
    async fn stream_counted(&self, r: &Request, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        match self.stream_once(r, true, partial, report).await {
            Err(Error::Net(m)) if (m.starts_with("HTTP 400") || m.starts_with("HTTP 422")) && !m.to_lowercase().contains("reasoning") => {
                self.stream_once(r, false, partial, report).await
            }
            other => other,
        }
    }

    async fn stream_once(&self, r: &Request, counted: bool, partial: Partial<'_>, report: Report<'_>) -> Result<String> {
        let mut body = self.body(r);
        body["stream"] = json!(true);
        if counted { body["stream_options"] = json!({"include_usage": true}) }
        let headers = self.headers();
        let mut extra: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        extra.push(("Accept", "text/event-stream"));
        let mut lines = Lines::post(&format!("{}/chat/completions", self.base_url), &body, &extra, if r.thinking { 180 } else { 90 }).await?;
        let (mut text, mut plain, mut usage) = (String::new(), String::new(), None);
        while let Some(line) = lines.next().await? {
            // «: OPENROUTER PROCESSING» keep-alives are comments; a line that isn't an event is a plain answer's.
            let Some(payload) = line.strip_prefix("data:") else {
                if !line.starts_with(':') { plain.push_str(&line); plain.push('\n') }
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" { break }
            let Ok(Value::Object(event)) = serde_json::from_str::<Value>(payload) else { continue };
            if let Some(error) = event.get("error") { return fail(format!("The model service: {error}")) }
            // The count comes in an event of its own after the words (OpenRouter's with the price it charged).
            usage = Usage::reported(event.get("usage")).or(usage);
            let words = event.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")).and_then(|d| d.get("content"))
                .and_then(Value::as_str).unwrap_or("");
            if !words.is_empty() {
                text.push_str(words);
                partial(&text);
            }
        }
        if text.is_empty() {
            if let Ok(whole @ Value::Object(_)) = serde_json::from_str::<Value>(&plain) {
                text = content(Some(&whole)).unwrap_or_default();
                usage = Usage::reported(whole.get("usage"));
                if !text.is_empty() { partial(&text) }
            }
        }
        if text.is_empty() { return fail("The model returned an empty reply.") }
        if let Some(u) = usage { report(u) }
        Ok(text)
    }

    pub async fn models(&self) -> Result<Vec<String>> {
        let headers = self.headers();
        let extra: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let data = http::get(&format!("{}/models", self.base_url), &extra, 15).await?;
        let list: Value = serde_json::from_slice(&data).map_err(|_| Error::Llm("The model list couldn't be read.".into()))?;
        let Some(rows) = list.get("data").and_then(Value::as_array) else { return fail("The model list couldn't be read.") };
        let mut ids = rows.iter().map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
            .collect::<Option<Vec<_>>>().ok_or_else(|| Error::Llm("The model list couldn't be read.".into()))?;
        ids.sort();
        Ok(ids)
    }
}

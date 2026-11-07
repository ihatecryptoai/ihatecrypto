//! Every way the engine can reach a model (`Catalog/LLMCatalog.swift`, with `Core/LLM.swift`'s `ProviderInfo` and
//! `LLMProvider`). Apple Intelligence isn't here: Windows has no Apple model.

use crate::anthropic::{self, AnthropicClient};
use crate::api_key::{self, Compatible, OllamaChatClient};
use crate::{Client, Credential, Inner, Result, Save, chatgpt, claude_code, fail, near_ai, open_router};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Subscription, ApiKey, Local }

impl Kind {
    /// Swift's raw value: «subscription», «apiKey», «local».
    pub fn as_str(self) -> &'static str { match self { Kind::Subscription => "subscription", Kind::ApiKey => "apiKey", Kind::Local => "local" } }
}

/// A way in as Settings lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub default_model: String,
    pub note: String,
    pub takes_key: bool,
    /// Button title for an interactive sign-in, None when there is none.
    pub sign_in_label: Option<String>,
    /// Button title for reusing a login another tool already has, None when there is none.
    pub existing_login_label: Option<String>,
    /// Why this option can't be used right now; None when it can.
    pub unavailable: Option<String>,
    /// The current models this way in offers, checked, the recommended one first.
    pub models: Vec<String>,
}

impl Info {
    pub fn new(id: &str, name: &str, kind: Kind, default_model: &str, note: &str) -> Self {
        Self { id: id.into(), name: name.into(), kind, default_model: default_model.into(), note: note.into(), takes_key: false,
               sign_in_label: None, existing_login_label: None, unavailable: None, models: vec![] }
    }
    pub(crate) fn key(mut self) -> Self { self.takes_key = true; self }
    pub(crate) fn models(mut self, models: &[&str]) -> Self { self.models = models.iter().map(|m| m.to_string()).collect(); self }
}

/// One way to reach models: an API key, a subscription login, or a local server.
#[derive(Debug, Clone)]
pub enum Provider {
    ChatGpt,
    ClaudeCode,
    /// Listed so the person sees why an option is missing.
    Unavailable(Info),
    OpenRouter,
    Compatible(Compatible),
    Anthropic,
    /// NEAR AI Cloud's private models, each chat after its enclaves check out.
    NearAi,
}

impl Provider {
    pub fn info(&self) -> Info {
        match self {
            Provider::ChatGpt => chatgpt::info(),
            Provider::ClaudeCode => claude_code::info(),
            Provider::Unavailable(info) => info.clone(),
            Provider::OpenRouter => open_router::info(),
            Provider::Compatible(c) => c.info.clone(),
            Provider::Anthropic => anthropic::info(),
            Provider::NearAi => near_ai::info(),
        }
    }

    /// Interactive sign-in in the browser: `open` is handed the page to open.
    pub async fn sign_in(&self, open: &(dyn Fn(String) + Send + Sync)) -> Result<Credential> {
        match self {
            Provider::ChatGpt => chatgpt::sign_in(open, chatgpt::ISSUER).await,
            Provider::OpenRouter => open_router::sign_in(open, open_router::BASE, "https://openrouter.ai/auth").await,
            other => fail(format!("{}: sign-in isn't supported.", other.info().name)),
        }
    }

    /// A login that already exists on this computer and can be reused, if any.
    pub fn existing_login(&self) -> Option<Credential> {
        match self {
            Provider::ChatGpt => chatgpt::existing_login(),
            Provider::ClaudeCode => claude_code::existing_login(),
            _ => None,
        }
    }

    /// `save` receives refreshed credentials so they survive a restart.
    pub fn client(&self, credential: Option<&Credential>, save: Save) -> Result<Client> {
        let key = match credential { Some(Credential::ApiKey(k)) => Some(k.as_str()), _ => None };
        Ok(Client::new(match self {
            Provider::ChatGpt => Inner::ChatGpt(chatgpt::client(credential, save)?),
            Provider::ClaudeCode => Inner::ClaudeCode(claude_code::client(credential)?),
            Provider::Unavailable(info) => return fail(info.unavailable.clone().unwrap_or_else(|| format!("{} is unavailable.", info.name))),
            Provider::OpenRouter => Inner::OpenAi(open_router::chat_client(credential)?),
            // Ollama through its own API: the OpenAI-style one can't widen its 4096-token window.
            Provider::Compatible(c) if c.info.id == "ollama" => Inner::Ollama(OllamaChatClient { base: c.base_url.trim_end_matches("/v1").into() }),
            Provider::Compatible(c) => Inner::OpenAi(c.chat_client(key)?),
            Provider::Anthropic => match key {
                Some(k) => Inner::Anthropic(AnthropicClient::new(k)),
                None => return fail("Anthropic API: add a key in Settings."),
            },
            Provider::NearAi => Inner::NearAi(near_ai::client(credential)?),
        }))
    }

    /// Model ids the service offers, when it can list them; the checked list otherwise.
    pub async fn models(&self, credential: Option<&Credential>) -> Result<Vec<String>> {
        let key = match credential { Some(Credential::ApiKey(k)) => Some(k.as_str()), _ => None };
        match self {
            Provider::ChatGpt => Ok(chatgpt::models(credential, chatgpt::MODELS).await),
            Provider::OpenRouter => open_router::chat_client(credential)?.models().await,
            Provider::Compatible(c) => c.chat_client(key)?.models().await,
            Provider::Anthropic => match key {
                Some(k) => AnthropicClient::new(k).models().await,
                None => fail("Anthropic API: add a key in Settings."),
            },
            other => Ok(other.info().models),
        }
    }
}

/// Every way in, in Settings' order: an Anthropic key keeps its place after OpenAI's, NEAR AI comes last.
pub fn all() -> Vec<Provider> {
    let unavailable = |id: &str, name: &str, why: &str| {
        let mut info = Info::new(id, name, Kind::Subscription, "", "");
        info.unavailable = Some(why.into());
        Provider::Unavailable(info)
    };
    let mut list = vec![
        Provider::ChatGpt,
        Provider::ClaudeCode,
        unavailable("grok-subscription", "Grok (SuperGrok / X Premium+)",
                    "xAI opens subscription sign-in only to partners (Hermes, OpenClaw). For now, connect Grok with an xAI API key."),
        unavailable("gemini-subscription", "Gemini (Google AI Pro / Ultra)",
                    "A Google AI subscription doesn't cover the API. Connect Gemini with a key from Google AI Studio."),
        Provider::OpenRouter,
    ];
    for p in api_key::presets() {
        let openai = p.info.id == "openai";
        list.push(Provider::Compatible(p));
        if openai { list.push(Provider::Anthropic) }
    }
    list.push(Provider::NearAi);
    list
}

pub fn provider(id: &str) -> Option<Provider> { all().into_iter().find(|p| p.info().id == id) }

/// A context window known before a service's first answer: Ollama's is the one its client asks for.
pub fn window(provider: &str) -> Option<i64> { (provider == "ollama").then_some(OllamaChatClient::CONTEXT) }

/// Why a model can't run the chat (too small for its brief); None when it can. Only Apple's model on a Mac is, and
/// Windows has none.
pub fn chat_refusal(_model: &str) -> Option<String> { None }

/// A model id as people write it: «gpt-6-sol» → «GPT 6 SOL», «claude-haiku-4-5» → «Claude Haiku 4.5»,
/// «deepseek/deepseek-v4.1-flash» → «DeepSeek V4.1 Flash», «qwen3.5:9b» → «Qwen3.5 9B».
pub fn display_name(model: &str) -> String {
    let id = model.split('/').next_back().unwrap_or(model);
    let mut words: Vec<String> = Vec::new();
    for part in id.split(|c| "-_: ".contains(c)).filter(|p| !p.is_empty()) {
        let numeric = |s: &str| s.chars().all(|c| c.is_numeric() || c == '.');
        if part.chars().all(char::is_numeric) && words.last().is_some_and(|w| numeric(w)) {
            let last = words.last_mut().expect("a word");
            last.push('.');
            last.push_str(part);
        } else if part == "deepseek" {
            words.push("DeepSeek".into());
        } else if part.chars().count() <= 3 && part.chars().all(char::is_alphabetic) && part != "pro" && part != "max" {
            words.push(part.to_uppercase());
        } else if part.ends_with('b') && numeric(&part[..part.len() - 1]) {
            words.push(part.to_uppercase());
        } else {
            let mut c = part.chars();
            words.push(c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default());
        }
    }
    words.join(" ")
}

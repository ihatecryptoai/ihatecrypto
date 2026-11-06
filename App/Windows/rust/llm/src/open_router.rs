//! One sign-in for OpenAI, Anthropic, Google, xAI and DeepSeek models, paid with the person's OpenRouter credits
//! (`OpenRouter/OpenRouter.swift`). The browser login (OAuth PKCE) returns a regular key, so a pasted key works the same.

use std::time::Duration;

use serde_json::{Value, json};

use crate::api_key::OpenAiChatClient;
use crate::catalog::{Info, Kind};
use crate::oauth::{LoopbackServer, Pkce};
use crate::{Credential, Error, Result, fail, http};

pub const BASE: &str = "https://openrouter.ai/api/v1";

pub fn info() -> Info {
    let mut info = Info::new("openrouter", "OpenRouter", Kind::ApiKey, "deepseek/deepseek-v4.1-flash",
                             "One sign-in for OpenAI, Anthropic, Google, xAI and DeepSeek models. Paid with OpenRouter credits.")
        .key()
        // A few current ones from its list (checked 2026-11-05); Settings loads all of them.
        .models(&["deepseek/deepseek-v4.1-flash", "openai/gpt-6.1-sol", "openai/gpt-6.1-terra", "anthropic/claude-sonnet-5.5",
                  "anthropic/claude-opus-5.5", "anthropic/claude-haiku-5.5", "google/gemini-3.8-flash", "google/gemini-4-argon",
                  "x-ai/grok-4.7", "qwen/qwen3.8-flash"]);
    info.sign_in_label = Some("Sign in with OpenRouter".into());
    info
}

pub(crate) async fn sign_in(open: &(dyn Fn(String) + Send + Sync), base: &str, auth: &str) -> Result<Credential> {
    let pkce = Pkce::make();
    let server = LoopbackServer::start(0, "/callback").await?;
    open(http::with_query(auth, &[("callback_url", &format!("http://localhost:{}/callback", server.port)),
                                  ("code_challenge", &pkce.challenge), ("code_challenge_method", "S256")]));
    let Some(code) = server.wait_for_callback(Duration::from_secs(300)).await?.remove("code") else { return fail("OpenRouter sign-in cancelled.") };
    let data = http::post_json(&format!("{base}/auth/keys"), &json!({"code": code, "code_verifier": pkce.verifier, "code_challenge_method": "S256"}), &[], 15).await?;
    serde_json::from_slice::<Value>(&data).ok().and_then(|k| k.get("key")?.as_str().map(|k| Credential::ApiKey(k.into())))
        .ok_or_else(|| Error::Llm("OpenRouter's answer had no key.".into()))
}

pub(crate) fn chat_client(credential: Option<&Credential>) -> Result<OpenAiChatClient> {
    let Some(Credential::ApiKey(key)) = credential else { return fail("OpenRouter: sign in or add a key in Settings.") };
    Ok(OpenAiChatClient::new(BASE, Some(key), true))
}

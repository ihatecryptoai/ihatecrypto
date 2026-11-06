//! The chat's model and the accounts it can go through (Models.swift).
//! Layer 2 adds here: model_view, provider_views, connect, sign_in, use_login, disconnect, choose_model, set_model,
//! use_account, model(of:), sign_in_jev and the rest of the file's `extension Engine`.

use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub connected: bool,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub takes_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_login: Option<String>,
    /// «Sign in with ChatGPT»: a browser sign-in, caught on 127.0.0.1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<String>,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    pub connected: bool,
    /// Its models for the menu (models(of:)) and the one it starts with.
    pub models: Vec<String>,
    pub default_model: String,
    /// The model chosen for this account, kept per account (the model menu's «<model> · connected»).
    pub chosen: String,
    /// Each model's name as the Mac's LLMCatalog.displayName writes it («GPT 5.4», «Qwen3 8B»), the chosen one too.
    pub names: HashMap<String, String>,
}

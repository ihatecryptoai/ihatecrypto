//! The chat's turn (ChatTurns.swift): the cards a reply offered, the blocks a message went with, a priced swap's bar,
//! the model menu's facts.
//! Layer 2 adds here: pin_options and the `options` setter it backs (Swift's didSet), stop_model, retry_move, requote,
//! boards_shown, chips_position, the model menu (refresh_models, download_model, Ollama: OllamaPull), quote_view,
//! models_view and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::Date;
use serde::{Deserialize, Serialize};

use crate::json;

/// One block a message went with, as its chip over the message says it (the Mac's SentContextChip): its title, what the
/// model was told, and whether only its name went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentContext {
    pub title: String,
    pub about: String,
    pub bare: bool,
}

/// A card a reply offered, kept with that reply (the Mac's Message.options): live while it's the last reply's and under
/// 30 minutes old; an older set folds, its pick keeping the check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferView {
    pub title: String,
    pub comment: String,
    pub recommended: bool,
    /// What the card runs, shown on hover as on the Mac («Answer with this» when it's a reply); None in older saved chats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// A priced swap's card, for the field's waiting bar (the Mac's quoteState): quoting, fresh until 15 s after its quote,
/// then stale — Refresh asks again before Confirm comes back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuoteView {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub at: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

/// The model menu and the connect bar's facts beyond the catalog: the lists a subscription or a model on this PC gives
/// now, whether Ollama answers and is installed, what each model takes, downloads under way, this PC's memory.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsView {
    pub live: std::collections::HashMap<String, Vec<String>>,
    pub servers: std::collections::HashMap<String, bool>,
    pub installed: bool,
    pub sizes: std::collections::HashMap<String, f64>,
    pub downloads: std::collections::HashMap<String, f64>,
    #[serde(rename = "memoryGB")]
    pub memory_gb: i64,
    pub recommended: std::collections::HashMap<String, String>,
    pub refusals: std::collections::HashMap<String, String>,
}

/// What Stop says: nothing ran (English, Russian).
pub const STOPPED_LINE: (&str, &str) = ("Stopped — nothing ran. Write what you'd like instead.", "Остановил — ничего не запускал. Напиши, что сделать вместо этого.");
/// Seconds a swap's quote stays fresh.
pub const QUOTE_LIFE: f64 = 15.0;
/// Ollama on this PC.
pub const OLLAMA: &str = "http://localhost:11434";

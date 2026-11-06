//! IhatecryptoCore types the engine keeps or draws that ihatecrypto-core hasn't ported yet: Normalize.swift's
//! `ChatRouter.Option` (round 4), MPP.swift's `Answer` and `Challenge`, ModelExam.swift's `Result` — their data and JSON
//! shapes only. When core ports their files, the engine takes core's types and these go.

use std::collections::HashMap;
use std::sync::LazyLock;

use ihatecrypto_core::foundation::Date;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// `ChatRouter.Option`: a way to go on, offered as a card — what it is, the model's comment on it, and the English form
/// it runs. An answer to the model's question («clarify») may have no command: picked, its title goes back to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterOption {
    pub title: String,
    pub comment: String,
    pub command: String,
    #[serde(default)]
    pub recommended: bool,
}

impl RouterOption {
    pub fn new(title: &str, comment: &str, command: &str, recommended: bool) -> Self {
        Self { title: title.into(), comment: comment.into(), command: command.into(), recommended }
    }
}

/// `MPP.Answer`: what a paid service answered, for the canvas — its list of results when it has one, else its plain
/// values, else its text. (Reading one from the service's bytes is MPP.swift's, ported with it.)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MppAnswer {
    #[serde(default)]
    pub items: Vec<MppItem>,
    #[serde(default)]
    pub facts: Vec<MppFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What an AI service wrote (Grok, Perplexity: the answer itself), and the links it cites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writing: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MppItem {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl MppItem {
    pub fn is_image(&self) -> bool {
        static IMAGE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"\.(?:png|jpe?g|webp|gif)(?:\?|$)").expect("a regex"));
        self.url.as_ref().is_some_and(|u| IMAGE.is_match(&u.to_lowercase()).unwrap_or(false))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MppFact {
    pub key: String,
    pub value: String,
}

/// `MPP.Challenge`: a service's price for a request — this much USDC on Tempo to this payee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MppChallenge {
    /// The auth-params as the service sent them: the credential echoes them unchanged.
    pub params: HashMap<String, String>,
    /// USDC units.
    pub amount: u64,
    pub payee: String,
    pub memo: Option<Vec<u8>>,
    pub expires: Option<Date>,
}

impl MppChallenge {
    pub fn realm(&self) -> &str { self.params.get("realm").map_or("", String::as_str) }
    pub fn purpose(&self) -> Option<&str> { self.params.get("description").map(String::as_str) }
    /// «Payment-Authorization» when the service keeps Authorization for its own login.
    pub fn field(&self) -> &str { self.params.get("header").map_or("Authorization", String::as_str) }
    pub fn usdc(&self) -> Decimal { Decimal::from(self.amount) / Decimal::from(1_000_000) }
}

/// `ModelExam.Result`: a model's Check, kept in the settings by «provider/model».
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExamResult {
    pub passed: i64,
    pub total: i64,
    /// Seconds a reply took on average.
    pub seconds: f64,
    /// The messages it misread or didn't answer.
    pub misses: Vec<String>,
}

impl ExamResult {
    /// Good enough for the chat: 8 of 10 and more.
    pub fn good(&self) -> bool { self.passed * 10 >= self.total * 8 }
    pub fn short(&self) -> String { format!("{}/{} · {:.1} s", self.passed, self.total, self.seconds) }
}

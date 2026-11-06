//! What an answer used (`Core/Usage.swift`): the counts in each service's shape, the context windows services tell,
//! and a model's window and prices from OpenRouter's public list.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde_json::{Map, Value};

/// What one answer used, as its service counted it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    /// Everything the model read for the answer, the parts its service kept in a cache included.
    pub input: i64,
    /// The part of `input` read from the service's cache, billed lower.
    pub cached: i64,
    /// The part of `input` written to the service's cache (Anthropic bills it at 1.25× the input price).
    pub written: i64,
    pub output: i64,
    /// The model's context window, when the service says it (Claude Code, the Codex backend, Ollama).
    pub window: Option<i64>,
    /// The service's own price for the answer in dollars, when it sends one (OpenRouter, Claude Code).
    pub cost: Option<f64>,
    /// Claude Code said the answer went past the plan's limits and usage credits paid for it; None when it didn't say.
    pub overage: Option<bool>,
}

/// A JSON number (or a bool, as Foundation's NSNumber reads it) as a whole number.
pub(crate) fn int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

pub(crate) fn double(v: Option<&Value>) -> Option<f64> {
    match v? { Value::Number(n) => n.as_f64(), Value::Bool(b) => Some(*b as u8 as f64), _ => None }
}

impl Usage {
    pub fn new(input: i64, output: i64) -> Self { Self { input, output, ..Default::default() } }

    /// A service's «usage» object in any of its three shapes: chat completions (prompt_tokens, the cached part counted
    /// inside them), the Responses API (input_tokens, the same) and Anthropic's (input_tokens with the cached parts beside
    /// them). None when it names no prompt size.
    pub fn reported(usage: Option<&Value>) -> Option<Self> {
        let u = usage?.as_object()?;
        let read = int(u.get("prompt_tokens").or(u.get("input_tokens")))?;
        let details = u.get("prompt_tokens_details").or(u.get("input_tokens_details")).and_then(Value::as_object);
        let written = int(u.get("cache_creation_input_tokens")).unwrap_or(0);
        let hit = int(u.get("cache_read_input_tokens"));
        Some(Self {
            input: read + written + hit.unwrap_or(0),
            cached: hit.or_else(|| int(details.and_then(|d| d.get("cached_tokens")))).unwrap_or(0),
            written,
            output: int(u.get("completion_tokens").or(u.get("output_tokens"))).unwrap_or(0),
            window: None,
            cost: double(u.get("cost")),
            overage: None,
        })
    }
}

/// Context windows a service told for its own models (the Codex backend lists them with a subscription's models), kept
/// for its client to report with an answer.
static WINDOWS: LazyLock<Mutex<HashMap<String, i64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn told_window(model: &str) -> Option<i64> { WINDOWS.lock().expect("windows").get(model).copied() }

pub fn tell_window(model: &str, window: i64) { WINDOWS.lock().expect("windows").insert(model.into(), window); }

/// A model's context window and list prices, from OpenRouter's public list.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelFacts {
    pub window: i64,
    /// Dollars a token: read, read from the cache, written as the answer.
    pub input: f64,
    pub cached: f64,
    pub output: f64,
    /// Dollars a token written to the cache: the read price where the list names none.
    pub written: f64,
    /// A prompt of more tokens than this is billed at these prices, every token of it: 0 for the base prices.
    pub over: i64,
    /// The model's dearer prices for long prompts, by size (`pricing.overrides`).
    pub longer: Vec<ModelFacts>,
}

pub const SOURCE: &str = "https://openrouter.ai/api/v1/models";

impl ModelFacts {
    pub fn new(window: i64, input: f64, cached: f64, output: f64, written: Option<f64>) -> Self {
        Self { window, input, cached, output, written: written.unwrap_or(input), over: 0, longer: vec![] }
    }

    // ponytail: DeepSeek's cheaper hours of the day aren't applied (OpenRouter's list doesn't carry them).
    pub fn cost(&self, u: &Usage) -> f64 {
        let p = self.longer.iter().rev().find(|t| u.input > t.over).unwrap_or(self);
        (u.input - u.cached - u.written) as f64 * p.input + u.cached as f64 * p.cached + u.written as f64 * p.written + u.output as f64 * p.output
    }

    /// One name for a model however a service spells it: «anthropic/claude-opus-5.5» and «claude-opus-5-5» are both
    /// «claudeopus55».
    pub fn key(model: &str) -> String {
        model.split('/').next_back().unwrap_or("").to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
    }

    /// OpenRouter's /models reply as a table by `key`. A variant («…:free») has a key of its own.
    pub fn table(data: &[u8]) -> HashMap<String, ModelFacts> {
        let mut table = HashMap::new();
        let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(data) else { return table };
        let Some(rows) = j.get("data").and_then(Value::as_array) else { return table };
        for row in rows {
            let (Some(id), Some(window)) = (row.get("id").and_then(Value::as_str), int(row.get("context_length"))) else { continue };
            if window <= 0 || table.contains_key(&Self::key(id)) { continue }
            let empty = Map::new();
            let base = row.get("pricing").and_then(Value::as_object).unwrap_or(&empty);
            let facts = |prices: &Map<String, Value>| {
                let price = |name: &str| match prices.get(name) {
                    Some(Value::String(s)) => s.parse::<f64>().ok(),
                    other => double(other),
                };
                let input = price("prompt").unwrap_or(0.0);
                ModelFacts::new(window, input, price("input_cache_read").unwrap_or(input), price("completion").unwrap_or(0.0), price("input_cache_write"))
            };
            let mut model = facts(base);
            // A tier names only what changes: the rest is the base price.
            model.longer = base.get("overrides").and_then(Value::as_array).into_iter().flatten().filter_map(|tier| {
                let tier = tier.as_object()?;
                let over = int(tier.get("min_prompt_tokens")).or_else(|| tier.get("min_prompt_tokens")?.as_str()?.parse().ok())?;
                let mut merged = base.clone();
                for (k, v) in tier { merged.insert(k.clone(), v.clone()); }
                let mut long = facts(&merged);
                long.over = over;
                Some(long)
            }).collect();
            model.longer.sort_by_key(|t| t.over);
            table.insert(Self::key(id), model);
        }
        table
    }
}

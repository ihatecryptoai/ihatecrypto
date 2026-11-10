//! Why a model didn't answer, read from what its service said (`LLMFailure.swift`), so the chat can say it in the
//! person's words instead of the service's JSON («NEAR AI: {"error":{"message":"No spending limit configured…» — the
//! owner, 2026-11-10: say what happened: the limit ran out, the balance needs topping up).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// No money on the account, or no spending limit set for the key.
    Credits,
    /// A subscription's own limit is used up until it resets.
    PlanLimit,
    /// Too many requests just now.
    RateLimit,
    /// The key is wrong, revoked or expired.
    Key,
    /// A sign-in that ran out.
    SignIn,
    /// The key or the account may not use this model.
    Access,
    /// The service has no such model.
    NoModel,
    /// The conversation is longer than the model reads.
    TooLong,
    /// The service is overloaded or down.
    Busy,
    /// No connection from this PC.
    Offline,
    /// No answer in the time given.
    Timeout,
    /// Claude Code is being reinstalled: every run fails for a minute or two.
    Updating,
}

/// The kind of `failure` (a provider's error text, http.rs's «HTTP 402: …» included), or None when it's none of these.
/// Money before pace: OpenAI's «insufficient_quota» comes with a 429. Pace before money: Gemini's free tier says
/// «quota … billing» and links its rate limits.
pub fn read(failure: &str) -> Option<Failure> {
    let t = failure.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    let code = t.find("http ").and_then(|i| t.get(i + 5..i + 8)).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    use Failure::*;
    // Only what a reinstall says while it runs: a flag the old program doesn't know.
    if has(&["claude"]) && has(&["unknown option"]) { return Some(Updating) }
    if has(&["rate limit", "rate_limit", "ratelimit", "rate-limit", "too many requests", "resource_exhausted"]) {
        // A day's allowance (OpenRouter's «free-models-per-day») doesn't come back in a minute.
        return Some(if has(&["per-day", "per day", "daily"]) { PlanLimit } else { RateLimit });
    }
    if has(&["usage limit", "hit your limit", "limit reached", "limit will reset", "out of extra usage"]) { return Some(PlanLimit) }
    if code == 402 || has(&["insufficient_quota", "insufficient balance", "insufficient credit", "insufficient fund", "no_limit_configured",
                            "spending limit", "credit", "billing", "payment required", "exceeded your current quota", "balance", "top up",
                            "top-up", "out of funds", "prepaid"]) { return Some(Credits) }
    if code == 429 || has(&["quota"]) { return Some(RateLimit) }
    if has(&["not logged in", "login expired", "sign in again", "signing in again", "/login", "token expired", "token has expired",
             "token is expired", "token_expired", "refresh token", "oauth"]) { return Some(SignIn) }
    if code == 401 || has(&["api key", "api_key", "x-api-key", "unauthorized", "authentication", "invalid key", "invalid token"]) { return Some(Key) }
    if has(&["context length", "context_length", "context window", "maximum context", "too long", "too many tokens", "token limit"]) { return Some(TooLong) }
    if code == 403 || has(&["permission", "forbidden", "not allowed", "access to", "not available in your", "unsupported_country",
                            "data policy"]) { return Some(Access) }
    if code == 404 || has(&["model_not_found", "not found", "does not exist", "no such model", "unknown model", "invalid model"]) { return Some(NoModel) }
    if has(&["timed out", "timeout", "didn't answer in", "didn’t answer in"]) { return Some(Timeout) }
    if has(&["offline", "internet connection", "network connection", "hostname could not be found", "could not connect", "cannot connect",
             "connection refused", "not connected", "error sending request", "dns error"]) { return Some(Offline) }
    if (500..=599).contains(&code) || has(&["overloaded", "unavailable", "internal server error", "bad gateway", "server error", "try again later",
                                             "capacity"]) { return Some(Busy) }
    None
}

/// The service's own sentence out of its error text, for a failure `read` doesn't know: the «message», «detail» or
/// «error» string of its JSON when there is one (cut off or not: a body is kept to its first 300 bytes), else the whole
/// text without the JSON's punctuation — a sentence, never braces and quotes, and never only «HTTP 400».
pub fn words(failure: &str) -> String {
    let cut = |s: &str| s.chars().take(140).collect::<String>().trim().to_string();
    for key in ["message", "detail", "error"] {
        if let Some(text) = string_after(failure, key).filter(|t| !t.is_empty()) { return cut(&text.replace('\n', " ")) }
    }
    cut(&failure.chars().filter(|c| !"{}[]\"".contains(*c)).collect::<String>())
}

/// The string after «"key":» in a JSON text, its escapes read; cut off counts (no closing quote).
fn string_after(failure: &str, key: &str) -> Option<String> {
    let name = format!("\"{key}\"");
    let at = failure.find(&name)?;
    let rest = failure[at + name.len()..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push(' '),
                Some('u') => {
                    let code: String = chars.by_ref().take(4).collect();
                    out.push(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32).unwrap_or('?'));
                }
                Some(other) => out.push(other),
                None => break,
            },
            other => out.push(other),
        }
    }
    Some(out)
}

/// The service's own page for keys and credits, by provider id: where the fix is, when it is there (the Mac's
/// ProviderInfo.keyURL).
pub fn site(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "openrouter" => "https://openrouter.ai/settings/keys",
        "deepseek" => "https://platform.deepseek.com/api_keys",
        "openai" => "https://platform.openai.com/api-keys",
        "xai" => "https://console.x.ai/team/default/api-keys",
        "gemini" => "https://aistudio.google.com/apikey",
        "anthropic" => "https://platform.claude.com/settings/keys",
        "nearai" => "https://cloud.near.ai",
        _ => return None,
    })
}

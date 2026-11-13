import Foundation

/// Why a model didn't answer, read from what its service said, so the chat can say it in the person's words instead of
/// the service's JSON («NEAR AI: {"error":{"message":"No spending limit configured…","type":"no_limit_configured"…» —
/// the owner, 2026-11-10: say what happened: the limit ran out, the balance needs topping up).
public enum LLMFailure: String, Sendable, CaseIterable {
    /// No money on the account, or no spending limit set for the key.
    case credits
    /// A subscription's own limit is used up until it resets.
    case planLimit
    /// Too many requests just now.
    case rateLimit
    /// The key is wrong, revoked or expired.
    case key
    /// A sign-in that ran out.
    case signIn
    /// The key or the account may not use this model.
    case access
    /// The service has no such model.
    case noModel
    /// The conversation is longer than the model reads.
    case tooLong
    /// The service is overloaded or down.
    case busy
    /// No connection from this Mac.
    case offline
    /// No answer in the time given.
    case timeout
    /// Claude Code is being reinstalled: every run fails for a minute or two.
    case updating

    /// The kind of `failure` (a provider's error text, HTTP.check's «HTTP 402: …» included), or nil when it's none of these.
    /// Money before pace: OpenAI's «insufficient_quota» comes with a 429. Pace before money: Gemini's free tier says
    /// «quota … billing» and links its rate limits.
    public static func read(_ failure: String) -> LLMFailure? {
        let t = failure.lowercased()
        func has(_ words: String...) -> Bool { words.contains { t.contains($0) } }
        let code = t.firstMatch(of: /http (\d{3})/).flatMap { Int($0.1) }
        // Only what a reinstall says while it runs: a flag the old program doesn't know, the program gone for a moment.
        if has("claude"), has("unknown option", "“claude” doesn", "\"claude\" doesn") { return .updating }
        if has("rate limit", "rate_limit", "ratelimit", "rate-limit", "too many requests", "resource_exhausted") {
            // A day's allowance (OpenRouter's «free-models-per-day») doesn't come back in a minute.
            return has("per-day", "per day", "daily") ? .planLimit : .rateLimit
        }
        if has("usage limit", "hit your limit", "limit reached", "limit will reset", "out of extra usage") { return .planLimit }
        if code == 402 || has("insufficient_quota", "insufficient balance", "insufficient credit", "insufficient fund", "no_limit_configured",
                              "spending limit", "credit", "billing", "payment required", "exceeded your current quota", "balance", "top up",
                              "top-up", "out of funds", "prepaid") { return .credits }
        if code == 429 || has("quota") { return .rateLimit }
        if has("not logged in", "login expired", "sign in again", "signing in again", "/login", "token expired", "token has expired",
               "token is expired", "token_expired", "refresh token", "oauth") { return .signIn }
        if code == 401 || has("api key", "api_key", "x-api-key", "unauthorized", "authentication", "invalid key", "invalid token") { return .key }
        if has("context length", "context_length", "context window", "maximum context", "too long", "too many tokens", "token limit") { return .tooLong }
        if code == 403 || has("permission", "forbidden", "not allowed", "access to", "not available in your", "unsupported_country",
                              "data policy") { return .access }
        if code == 404 || has("model_not_found", "not found", "does not exist", "no such model", "unknown model", "invalid model") { return .noModel }
        if has("timed out", "timeout", "didn't answer in", "didn’t answer in") { return .timeout }
        if has("offline", "internet connection", "network connection", "hostname could not be found", "could not connect", "cannot connect",
               "connection refused", "not connected") { return .offline }
        if (500...599).contains(code ?? 0) || has("overloaded", "unavailable", "internal server error", "bad gateway", "server error",
                                                   "try again later", "capacity") { return .busy }
        return nil
    }

    /// The service's own sentence out of its error text, for a failure `read` doesn't know: the «message», «detail» or
    /// «error» string of its JSON when there is one (cut off or not: a body is kept to its first 300 bytes), else the
    /// whole text without the JSON's punctuation — a sentence, never braces and quotes, and never only «HTTP 400».
    public static func words(_ failure: String) -> String {
        let cut = { (s: Substring) in String(s.prefix(140)).trimmingCharacters(in: .whitespacesAndNewlines) }
        for key in ["message", "detail", "error"] {
            guard let m = try? Regex("\"\(key)\"\\s*:\\s*\"((?:[^\"\\\\]|\\\\.)*)(\"?)").firstMatch(in: failure),
                  let raw = m.output[1].substring, !raw.isEmpty else { continue }
            // JSON's own reading of the string (’, \/, \"), when it is whole.
            let whole = (try? JSONDecoder().decode(String.self, from: Data("\"\(raw)\"".utf8)))
                ?? String(raw).replacing("\\\"", with: "\"").replacing("\\n", with: " ").replacing("\\/", with: "/")
            return cut(whole.replacing("\n", with: " ")[...])
        }
        return cut(failure.filter { !"{}[]\"".contains($0) }[...])
    }
}

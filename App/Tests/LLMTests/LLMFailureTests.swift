import Foundation
import LLMCore
import Testing

/// What a model's service said, read as what happened (the owner, 2026-11-10: the chat showed NEAR AI's JSON).
@Test func aServicesErrorIsReadAsWhatHappened() {
    let cases: [(String, LLMFailure?)] = [
        // No money, or no spending limit: the owner's own case, then the other services' words for it.
        (#"NEAR AI: {"error":{"message":"No spending limit configured. Please contact support to set up credits.","type":"no_limit_configured","param":null}}"#, .credits),
        (#"HTTP 429: {"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota"}}"#, .credits),
        ("Anthropic: Your credit balance is too low to access the Anthropic API. Please go to Plans & Billing to upgrade or purchase credits.", .credits),
        (#"HTTP 402: {"error":{"message":"Insufficient credits. Add more using https://openrouter.ai/settings/credits","code":402}}"#, .credits),
        (#"HTTP 402: {"error":{"message":"Insufficient Balance","type":"unknown_error"}}"#, .credits),
        // Pace.
        (#"HTTP 429: {"error":{"message":"Rate limit reached for gpt-5 in organization org-x on requests per min (RPM)","type":"requests"}}"#, .rateLimit),
        ("Anthropic: This request would exceed your organization's rate limit of 50 requests per minute", .rateLimit),
        (#"HTTP 429: {"error":{"code":429,"message":"Quota exceeded for metric: generate_content_free_tier_requests","status":"RESOURCE_EXHAUSTED"}}"#, .rateLimit),
        ("HTTP 429: Too Many Requests", .rateLimit),
        // A subscription's limit.
        ("Claude Code: You've hit your limit · resets 3pm (Europe/Kyiv)", .planLimit),
        ("ChatGPT: You've reached your usage limit. Upgrade to Pro or try again in 2 hours.", .planLimit),
        // Keys and sign-ins.
        (#"HTTP 401: {"error":{"message":"Incorrect API key provided: sk-proj-…","type":"invalid_request_error","code":"invalid_api_key"}}"#, .key),
        ("Anthropic: invalid x-api-key", .key),
        (#"HTTP 400: {"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#, .key),
        ("The Codex CLI login expired. Run codex once or sign in via the browser.", .signIn),
        ("Claude Code: Not logged in · Please run /login", .signIn),
        // The model itself.
        (#"HTTP 404: {"error":{"message":"The model `gpt-9` does not exist or you do not have access to it.","code":"model_not_found"}}"#, .access),
        (#"HTTP 404: {"error":{"message":"No endpoints found for x/y.","code":404}}"#, .noModel),
        (#"HTTP 400: {"error":{"message":"This model's maximum context length is 128000 tokens.","code":"context_length_exceeded"}}"#, .tooLong),
        (#"HTTP 403: {"error":{"message":"Country, region, or territory not supported","code":"unsupported_country_region_territory"}}"#, .access),
        // The service, the network, the time.
        ("Anthropic: Overloaded", .busy),
        ("HTTP 503: Service Unavailable", .busy),
        ("The Internet connection appears to be offline.", .offline),
        ("Could not connect to the server.", .offline),
        ("The request timed out.", .timeout),
        ("Claude Code didn't answer in 120 s, so it was stopped. Send it again, or pick another model.", .timeout),
        // Claude Code being reinstalled.
        ("Claude Code stopped with code 1: error: unknown option '--restricted'", .updating),
        ("The file “claude” doesn’t exist.", .updating),
        // Gemini's free tier names billing and links its rate limits: pace, not money. A day's allowance isn't a minute's.
        (#"HTTP 429: {"error":{"code":429,"message":"You exceeded your current quota, please check your plan and billing details. For more information on this error, head to: https://ai.google.dev/gemini-api/docs/rate-limits."#, .rateLimit),
        (#"HTTP 429: {"error":{"message":"Rate limit exceeded: free-models-per-day. Add 10 credits to unlock 1000 free model requests per day","code":429}}"#, .planLimit),
        // A subscription's token, a data policy, a program that isn't there for good.
        (#"HTTP 401: {"error":{"message":"Provided authentication token is expired. Please try signing in again.","code":"token_expired"}}"#, .signIn),
        (#"HTTP 404: {"error":{"message":"No endpoints found matching your data policy (Free model publication)","code":404}}"#, .access),
        ("Claude Code stopped with code 127: env: node: No such file or directory", nil),
        // Nothing it knows: the chat shows the service's own sentence.
        ("NEAR AI gave no enclave evidence for x, so nothing was sent. Pick another of its private models.", nil),
        ("The model returned an empty reply.", nil),
    ]
    for (text, kind) in cases { #expect(LLMFailure.read(text) == kind, "\(text)") }
}

/// A failure nobody knows still reads as a sentence: the service's «message», «detail» or «error» string, cut off or
/// not, never its JSON and never only «HTTP 400».
@Test func anUnknownErrorShowsTheServicesOwnSentence() {
    #expect(LLMFailure.words(#"NEAR AI: {"error":{"message":"Model is warming up, \"retry\" soon.","type":"warmup","param":null}}"#) == #"Model is warming up, "retry" soon."#)
    #expect(LLMFailure.words(#"HTTP 400: {"error":{"message":"It’s https:\/\/x.y down"}}"#) == "It’s https://x.y down")
    #expect(LLMFailure.words(#"HTTP 400: {"error":"registry.ollama.ai/library/x does not support tools"}"#) == "registry.ollama.ai/library/x does not support tools")
    #expect(LLMFailure.words(#"HTTP 400: {"detail":"Unsupported value"}"#) == "Unsupported value")
    // A body cut at its first 300 bytes: the sentence as far as it goes.
    #expect(LLMFailure.words(#"NEAR AI: {"error":{"message":"The enclave is restar"#) == "The enclave is restar")
    #expect(LLMFailure.words("The model returned an empty reply.") == "The model returned an empty reply.")
    #expect(LLMFailure.words(#"The model service: {"code":"weird"}"#) == "The model service: code:weird")
    #expect(LLMFailure.words(String(repeating: "a", count: 400)).count == 140)
}

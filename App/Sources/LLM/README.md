# LLM: model access

Each way of connecting is a separate SwiftPM module in its own folder. The trading plan (`IhatecryptoCore`) knows only the `LLMClient` protocol from `Core` and does not depend on the providers.

```
Core/        LLMClient, LLMProvider, Credential, HTTP, PKCE, LoopbackServer
APIKey/      OpenAI-compatible APIs: DeepSeek, OpenAI, Anthropic, xAI, Gemini, Ollama, LM Studio
OpenRouter/  browser sign-in (OAuth PKCE) or a key; every model through one account
ChatGPT/     Plus/Pro subscription through the Codex backend; sign in via the browser or from ~/.codex/auth.json
ClaudeCode/  Claude subscription through the installed Claude Code (`claude -p` with no tools)
Apple/       Apple Intelligence (Foundation Models): on this Mac, or Private Cloud Compute with Apple's entitlement
Catalog/     every provider, including unavailable ones with the reason (Grok, Gemini by subscription)
Check/       `swift run LLMCheck <id> "text"` — checks one provider from the terminal
```

## Adding a provider

1. A new OpenAI-compatible API: one entry in `OpenAICompatible.presets`, no new module.
2. Another protocol or sign-in: a folder `Sources/LLM/<Name>/` with a type implementing `LLMProvider`:
   `info`, `client(_:save:)`, and if needed `signIn(open:)`, `existingLogin()`, `models(_:)`.
3. A target in `Package.swift` (depends only on `LLMCore`) and one line in `LLMCatalog.all`.
4. A test in `Tests/LLMTests` and a check with `swift run LLMCheck <id>`.

## Rules

- Keys and tokens live only in the app's Keychain; for other apps' logins (Codex CLI, Claude Code), only the path.
- The app does not refresh Codex CLI tokens: the refresh token is single-use, and Codex CLI would be logged out afterwards.
- The model gets the message text, the plan's parameters and the balances it needs to answer. The user's own addresses reach it only as `me`; the seed phrase and signing never do.

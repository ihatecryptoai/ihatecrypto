import Foundation
import LLMAPIKey
import LLMCore

/// One sign-in for OpenAI, Anthropic, Google, xAI and DeepSeek models, paid with the user's OpenRouter credits.
/// The browser login (OAuth PKCE) returns a regular key, so a pasted key works the same way.
public struct OpenRouter: LLMProvider {
    static let base = "https://openrouter.ai/api/v1"

    public init() {}

    public var info: ProviderInfo {
        ProviderInfo(
            id: "openrouter", name: "OpenRouter", kind: .apiKey, defaultModel: "deepseek/deepseek-v4.1-flash",
            note: "One sign-in for OpenAI, Anthropic, Google, xAI and DeepSeek models. Paid with OpenRouter credits.",
            takesKey: true, signInLabel: "Sign in with OpenRouter",
            // A few current ones from its list (checked 2026-11-05); Settings loads all of them.
            models: ["deepseek/deepseek-v4.1-flash", "openai/gpt-6.1-sol", "openai/gpt-6.1-terra", "anthropic/claude-sonnet-5.5",
                     "anthropic/claude-opus-5.5", "anthropic/claude-haiku-5.5", "google/gemini-3.8-flash", "google/gemini-4-argon",
                     "x-ai/grok-4.7", "qwen/qwen3.8-flash"],
            keyURL: "https://openrouter.ai/settings/keys", keySteps: "Create key, add credits in Credits."
        )
    }

    public func signIn(open: OpenURL) async throws -> Credential {
        let pkce = PKCE.make()
        let server = try await LoopbackServer.start(path: "/callback")
        var url = URLComponents(string: "https://openrouter.ai/auth")!
        url.queryItems = [
            URLQueryItem(name: "callback_url", value: "http://localhost:\(server.port)/callback"),
            URLQueryItem(name: "code_challenge", value: pkce.challenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
        ]
        open(url.url!)
        guard let code = try await server.waitForCallback()["code"] else { throw LLMError("OpenRouter sign-in cancelled.") }
        let data = try await HTTP.post(Self.base + "/auth/keys", json: [
            "code": code, "code_verifier": pkce.verifier, "code_challenge_method": "S256",
        ])
        struct Key: Decodable { let key: String }
        return .apiKey(try JSONDecoder().decode(Key.self, from: data).key)
    }

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        try chatClient(credential)
    }

    public func models(_ credential: Credential?) async throws -> [String] {
        try await chatClient(credential).models()
    }

    func chatClient(_ credential: Credential?) throws -> OpenAIChatClient {
        guard case .apiKey(let key) = credential else { throw LLMError("OpenRouter: sign in or add a key in Settings.") }
        return OpenAIChatClient(baseURL: Self.base, key: key, jsonMode: true)
    }
}

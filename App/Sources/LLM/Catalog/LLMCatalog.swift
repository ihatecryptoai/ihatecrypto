import LLMAPIKey
import LLMApple
import LLMChatGPT
import LLMClaudeCode
import LLMCore
import LLMOpenRouter

/// Every way the app can reach a model. Adding a provider: a module under Sources/LLM plus one line here.
public enum LLMCatalog {
    public static let all: [any LLMProvider] = [
        ChatGPTSubscription(),
        ClaudeCodeSubscription(),
        Unavailable(info: ProviderInfo(
            id: "grok-subscription", name: "Grok (SuperGrok / X Premium+)", kind: .subscription, defaultModel: "",
            note: "", unavailable: "xAI opens subscription sign-in only to partners (Hermes, OpenClaw). For now, connect Grok with an xAI API key."
        )),
        Unavailable(info: ProviderInfo(
            id: "gemini-subscription", name: "Gemini (Google AI Pro / Ultra)", kind: .subscription, defaultModel: "",
            note: "", unavailable: "A Google AI subscription doesn't cover the API. Connect Gemini with a key from Google AI Studio."
        )),
        OpenRouter(),
    ] + OpenAICompatible.presets + [AppleIntelligence()]

    public static func provider(id: String) -> (any LLMProvider)? {
        all.first { $0.info.id == id }
    }

    /// A context window known before a service's first answer: Ollama's is the one its client asks for.
    public static func window(provider id: String) -> Int? { id == "ollama" ? OllamaChatClient.context : nil }

    /// A model id as people write it: «gpt-6-sol» → «GPT 6 SOL», «claude-haiku-4-5» → «Claude Haiku 4.5»,
    /// «deepseek/deepseek-v4.1-flash» → «DeepSeek V4.1 Flash», «qwen3.5:9b» → «Qwen3.5 9B».
    /// Why a model can't run the chat (too small for its brief); nil when it can.
    public static func chatRefusal(model: String) -> String? { AppleIntelligence.chatRefusal(model: model) }

    public static func displayName(model: String) -> String {
        let id = model.split(separator: "/").last.map(String.init) ?? model
        var words: [String] = []
        for part in id.split(whereSeparator: { "-_: ".contains($0) }).map(String.init) {
            // Version parts: «4-5» reads 4.5.
            if part.allSatisfy(\.isNumber), let last = words.last, last.allSatisfy({ $0.isNumber || $0 == "." }) {
                words[words.count - 1] += "." + part
            } else if part == "deepseek" {
                words.append("DeepSeek")
            } else if part.count <= 3, part.allSatisfy(\.isLetter), !["pro", "max"].contains(part) {
                words.append(part.uppercased())
            } else if part.last == "b", part.dropLast().allSatisfy({ $0.isNumber || $0 == "." }) {
                words.append(part.uppercased())
            } else {
                words.append(part.prefix(1).uppercased() + part.dropFirst())
            }
        }
        return words.joined(separator: " ")
    }
}

/// Listed so the user sees why an option is missing.
struct Unavailable: LLMProvider {
    let info: ProviderInfo

    func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        throw LLMError(info.unavailable ?? "\(info.name) is unavailable.")
    }
}

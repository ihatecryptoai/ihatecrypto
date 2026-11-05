import Foundation
import LLMCore

/// Any service with the OpenAI chat-completions format: DeepSeek, OpenAI, xAI, Gemini, Ollama, LM Studio. A new one is a
/// new preset, not new code. An Anthropic key has a module of its own (LLM/Anthropic): Anthropic's own API caches.
public struct OpenAICompatible: LLMProvider {
    public let info: ProviderInfo
    public let baseURL: String
    let jsonMode: Bool

    public init(info: ProviderInfo, baseURL: String, jsonMode: Bool = true) {
        self.info = info
        self.baseURL = baseURL
        self.jsonMode = jsonMode
    }

    public static let presets: [OpenAICompatible] = [
        .init(info: ProviderInfo(id: "deepseek", name: "DeepSeek API", kind: .apiKey, defaultModel: "deepseek-flash",
                                 note: "Key from platform.deepseek.com.", takesKey: true, models: ["deepseek-flash", "deepseek-v4-pro"]),
              baseURL: "https://api.deepseek.com"),
        .init(info: ProviderInfo(id: "openai", name: "OpenAI API", kind: .apiKey, defaultModel: "gpt-6.1-sol",
                                 note: "Key from platform.openai.com, billed separately from a ChatGPT subscription.", takesKey: true,
                                 models: ["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna"]),
              baseURL: "https://api.openai.com/v1"),
        .init(info: ProviderInfo(id: "xai", name: "xAI Grok API", kind: .apiKey, defaultModel: "grok-4.7",
                                 note: "Key from console.x.ai, paid with credits.", takesKey: true, models: ["grok-4.7", "grok-4.6"]),
              baseURL: "https://api.x.ai/v1"),
        .init(info: ProviderInfo(id: "gemini", name: "Google Gemini API", kind: .apiKey, defaultModel: "gemini-3.8-flash",
                                 note: "Key from Google AI Studio.", takesKey: true, models: ["gemini-3.8-flash", "gemini-4-argon"]),
              baseURL: "https://generativelanguage.googleapis.com/v1beta/openai"),
        // The biggest first: the app takes the first that fits this Mac's memory (OllamaModels.fits). The chat's exam
        // here (2026-10-06, 32 GB Mac): gemma4:26b 9/10 at 5.4 s, gemma4:12b 8/10 at 8.4 s (nimble:9b 8/10, gpt-oss:20b
        // 6/10 at 16.5 s — it can't stop thinking). gemma4:31b (≥ 36 GB) and nemotron-3-super:120b (≥ 192 GB, strong in
        // other languages) don't fit this one and aren't tested.
        .init(info: ProviderInfo(id: "ollama", name: "Ollama on this Mac", kind: .local, defaultModel: "gemma4:26b",
                                 note: "No network or key. The app downloads the biggest model that fits this Mac's memory.",
                                 models: ["nemotron-3-super:120b", "gemma4:31b", "gemma4:26b", "gemma4:12b"]),
              baseURL: "http://localhost:11434/v1"),
        .init(info: ProviderInfo(id: "lmstudio", name: "LM Studio on this Mac", kind: .local, defaultModel: "",
                                 note: "No network or key. Start the server in LM Studio and pick a model from the list."),
              baseURL: "http://localhost:1234/v1", jsonMode: false),
    ]

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        // Ollama through its own API: the OpenAI-style one can't widen its 4096-token window, which cut the start of
        // ihatecrypto's instructions (≈ 5k tokens) and any long message (docs: faq, openai-compatibility; 2026-10-06).
        info.id == "ollama" ? OllamaChatClient(base: String(baseURL.dropLast(3))) : try chatClient(credential)
    }

    public func models(_ credential: Credential?) async throws -> [String] {
        try await chatClient(credential).models()
    }

    func chatClient(_ credential: Credential?) throws -> OpenAIChatClient {
        var key: String?
        if case .apiKey(let k) = credential { key = k }
        if info.takesKey, key == nil { throw LLMError("\(info.name): add a key in Settings.") }
        return OpenAIChatClient(baseURL: baseURL, key: key, jsonMode: jsonMode)
    }
}

/// Ollama's /api/chat with a context window that holds the instructions, the brief and a long message: `num_ctx` per
/// request, streamed as one JSON object a line.
public struct OllamaChatClient: LLMClient {
    let base: String
    // ponytail: one size for every model; a Mac short of memory for 16k on a big model would want it from /api/show.
    public static let context = 16_384

    /// `think: false`: a thinking model (Gemma 4, Qwen 3.x) otherwise reasons to itself before its JSON — 544 tokens and
    /// 18.5 s instead of 19 and 5.3 s for «купи SOL на 5 долларов» on Gemma 4 12B, and ≈ 2 minutes an answer with the
    /// chat's whole brief. A model that can't stop thinking is asked again without it.
    func body(_ r: LLMRequest, stream: Bool, think: Bool = false) -> [String: Any] {
        var body: [String: Any] = ["model": r.model, "stream": stream, "options": ["num_ctx": Self.context],
                                   "messages": r.messages.map { ["role": $0.role.rawValue, "content": $0.text] }]
        if !think { body["think"] = false }
        if r.wantsJSON { body["format"] = "json" }
        return body
    }

    public func complete(_ r: LLMRequest) async throws -> String { try await stream(r) { _ in } }

    public func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        do { return try await stream(r, think: r.thinking, partial: partial) } catch {
            guard "\(error)".localizedCaseInsensitiveContains("think") else { throw error }
            return try await stream(r, think: !r.thinking, partial: partial)
        }
    }

    private func stream(_ r: LLMRequest, think: Bool, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let request = try HTTP.request(base + "/api/chat", method: "POST", body: try JSONSerialization.data(withJSONObject: body(r, stream: true, think: think)),
                                       contentType: "application/json", headers: [:], timeout: 300)
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if !(200..<300).contains((response as? HTTPURLResponse)?.statusCode ?? 0) {
            var body = Data()
            for try await byte in bytes.prefix(600) { body.append(byte) }
            try HTTP.check(response, body: body)
        }
        var text = ""
        for try await line in bytes.lines {
            guard let event = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any] else { continue }
            if let error = event["error"] as? String { throw LLMError("Ollama: \(error)") }
            if let words = (event["message"] as? [String: Any])?["content"] as? String, !words.isEmpty {
                text += words
                partial(text)
            }
            if event["done"] as? Bool == true {
                // The last line counts what it read and wrote; the window is the one asked for above.
                if let read = event["prompt_eval_count"] as? Int {
                    LLMUsage.report(LLMUsage(input: read, output: event["eval_count"] as? Int ?? 0, window: Self.context))
                }
                break
            }
        }
        guard !text.isEmpty else { throw LLMError("The model returned an empty reply.") }
        return text
    }
}

public struct OpenAIChatClient: LLMClient {
    public let baseURL: String
    let key: String?
    let jsonMode: Bool

    public init(baseURL: String, key: String?, jsonMode: Bool) {
        self.baseURL = baseURL.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        self.key = key
        self.jsonMode = jsonMode
    }

    var headers: [String: String] { key.map { ["Authorization": "Bearer \($0)"] } ?? [:] }

    func body(_ r: LLMRequest) -> [String: Any] {
        var body: [String: Any] = [
            "model": r.model,
            "messages": r.messages.map { ["role": $0.role.rawValue, "content": $0.text] },
        ]
        if r.wantsJSON, jsonMode { body["response_format"] = ["type": "json_object"] }
        // OpenAI's, Gemini's and OpenRouter's reasoning models take it; a model that doesn't is asked again without it.
        if r.thinking { body["reasoning_effort"] = "medium" }
        return body
    }

    /// A model that refuses `reasoning_effort` (one that doesn't reason, or reasons by its name) answers without it.
    private func withoutThinking<T>(_ r: LLMRequest, _ ask: (LLMRequest) async throws -> T) async throws -> T {
        do { return try await ask(r) } catch where r.thinking && "\(error)".localizedCaseInsensitiveContains("reasoning") {
            var plain = r
            plain.thinking = false
            return try await ask(plain)
        }
    }

    public func complete(_ r: LLMRequest) async throws -> String {
        try await withoutThinking(r) { try await completeOnce($0) }
    }

    private func completeOnce(_ r: LLMRequest) async throws -> String {
        let data = try await HTTP.post(baseURL + "/chat/completions", json: body(r), headers: headers, timeout: r.thinking ? 180 : 90)
        struct Message: Decodable { let content: String? }
        struct Choice: Decodable { let message: Message }
        struct Completion: Decodable { let choices: [Choice] }
        guard let text = try JSONDecoder().decode(Completion.self, from: data).choices.first?.message.content, !text.isEmpty else {
            throw LLMError("The model returned an empty reply.")
        }
        LLMUsage.report(LLMUsage(reported: (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["usage"] as? [String: Any]))
        return text
    }

    /// The same answer as server-sent events, `partial` told the text so far: OpenAI's, OpenRouter's, Ollama's and LM
    /// Studio's /chat/completions all speak them. A server that ignores «stream» and answers with one body still works.
    public func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        try await withoutThinking(r) { r in
            // The stream's last event is asked to count the tokens. A server that refuses a request it doesn't know every
            // field of (400 or 422, whatever it names) is asked again without the option: the count is never worth the answer.
            do { return try await streamOnce(r, counted: true, partial: partial) } catch let refused as NetError
                where ["HTTP 400", "HTTP 422"].contains(where: refused.message.hasPrefix) && !"\(refused)".localizedCaseInsensitiveContains("reasoning") {
                return try await streamOnce(r, counted: false, partial: partial)
            }
        }
    }

    private func streamOnce(_ r: LLMRequest, counted: Bool, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        var b = body(r)
        b["stream"] = true
        if counted { b["stream_options"] = ["include_usage": true] }
        let request = try HTTP.request(baseURL + "/chat/completions", method: "POST", body: try JSONSerialization.data(withJSONObject: b),
                                       contentType: "application/json", headers: headers.merging(["Accept": "text/event-stream"]) { mine, _ in mine },
                                       timeout: r.thinking ? 180 : 90)
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if !(200..<300).contains((response as? HTTPURLResponse)?.statusCode ?? 0) {
            var body = Data()
            for try await byte in bytes.prefix(600) { body.append(byte) }
            try HTTP.check(response, body: body)
        }
        var text = "", plain = "", usage: LLMUsage?
        for try await line in bytes.lines {
            // «: OPENROUTER PROCESSING» keep-alives are comments; a line that isn't an event is a plain answer's.
            guard line.hasPrefix("data:") else { if !line.hasPrefix(":") { plain += line + "\n" }; continue }
            let payload = line.dropFirst(5).trimmingCharacters(in: .whitespaces)
            if payload == "[DONE]" { break }
            guard let event = (try? JSONSerialization.jsonObject(with: Data(payload.utf8))) as? [String: Any] else { continue }
            if let error = event["error"] { throw LLMError("The model service: \(error)") }
            // The count comes in an event of its own after the words (OpenRouter's with the price it charged).
            usage = LLMUsage(reported: event["usage"] as? [String: Any]) ?? usage
            let words = ((event["choices"] as? [[String: Any]])?.first?["delta"] as? [String: Any])?["content"] as? String ?? ""
            if !words.isEmpty {
                text += words
                partial(text)
            }
        }
        if text.isEmpty, let whole = try? JSONSerialization.jsonObject(with: Data(plain.utf8)) as? [String: Any] {
            text = ((whole["choices"] as? [[String: Any]])?.first?["message"] as? [String: Any])?["content"] as? String ?? ""
            usage = LLMUsage(reported: whole["usage"] as? [String: Any])
            if !text.isEmpty { partial(text) }
        }
        guard !text.isEmpty else { throw LLMError("The model returned an empty reply.") }
        LLMUsage.report(usage)
        return text
    }

    public func models() async throws -> [String] {
        struct Model: Decodable { let id: String }
        struct List: Decodable { let data: [Model] }
        let data = try await HTTP.get(baseURL + "/models", headers: headers)
        return try JSONDecoder().decode(List.self, from: data).data.map(\.id).sorted()
    }
}

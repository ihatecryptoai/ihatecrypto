import Foundation
import LLMCore

/// An Anthropic API key, on Anthropic's own Messages API. Its OpenAI-compatible endpoint, used before, is «not long-term
/// or production-ready» by Anthropic's word: it ignored «Think before answering» and cached nothing. Here the chat's
/// instructions are cached (a repeat within 5 minutes reads them at a tenth of the price or less), the effort follows
/// «Think before answering», and the cache's tokens reach the context chip.
public struct AnthropicAPI: LLMProvider {
    static let base = "https://api.anthropic.com/v1"

    public init() {}

    public var info: ProviderInfo {
        ProviderInfo(id: "anthropic", name: "Anthropic API", kind: .apiKey, defaultModel: "claude-opus-5-5",
                     note: "Key from platform.claude.com/settings/keys › Create key, scoped to a workspace: Anthropic refuses a key without one. Billed separately from a Claude subscription.",
                     takesKey: true, models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-haiku-5-5"])
    }

    /// Anthropic's 400 for a key that isn't scoped to a workspace (it would need the anthropic-workspace-id header,
    /// docs: manage-claude/authentication), said as what to do.
    static let noWorkspace = "This Anthropic key isn't scoped to a workspace, so Anthropic refuses it. Make one that is: platform.claude.com/settings/keys › Create key › scope it to a workspace, then paste it in Settings › AI model."

    static func explained(_ error: any Error) -> any Error {
        (error as? NetError)?.message.contains("anthropic-workspace-id") == true ? LLMError(noWorkspace) : error
    }

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        try messagesClient(credential)
    }

    public func models(_ credential: Credential?) async throws -> [String] {
        try await messagesClient(credential).models()
    }

    func messagesClient(_ credential: Credential?) throws -> AnthropicClient {
        guard case .apiKey(let key) = credential else { throw LLMError("Anthropic API: add a key in Settings.") }
        return AnthropicClient(key: key)
    }
}

struct AnthropicClient: LLMClient {
    let key: String
    static let version = "2026-10-21"

    var headers: [String: String] { ["x-api-key": key, "anthropic-version": Self.version] }

    /// The instructions go as one block marked for the cache: they are the same in every chat request (≈ 18k tokens),
    /// so only the first in 5 minutes pays for them in full. A JSON answer is asked for in the instructions: the API's own
    /// JSON mode needs a schema. Thinking: Claude 5 models think as they see fit without being asked, at their own
    /// effort, and «Think before answering» raises it to high, as the Claude Code and ChatGPT clients do.
    static func body(_ r: LLMRequest, think: Bool) -> [String: Any] {
        var body: [String: Any] = [
            "model": r.model,
            // A cap, not a target: thinking counts toward it, and a JSON answer is a few hundred tokens.
            "max_tokens": 32_000,
            "stream": true,
            "messages": r.dialogue.map { ["role": $0.role.rawValue, "content": $0.text] },
        ]
        if !r.system.isEmpty { body["system"] = [["type": "text", "text": r.system, "cache_control": ["type": "ephemeral"]]] }
        if think {
            body["thinking"] = ["type": "adaptive"]
            body["output_config"] = ["effort": "high"]
        }
        return body
    }

    func complete(_ r: LLMRequest) async throws -> String { try await stream(r) { _ in } }

    /// A model that doesn't take adaptive thinking or an effort (Haiku 4.5 and older) answers without them.
    func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        do { return try await streamOnce(r, think: r.thinking, partial: partial) } catch let refused as NetError
            where r.thinking && refused.message.hasPrefix("HTTP 400") && ["thinking", "effort"].contains(where: { refused.message.contains($0) }) {
            return try await streamOnce(r, think: false, partial: partial)
        }
    }

    private func streamOnce(_ r: LLMRequest, think: Bool, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let request = try HTTP.request(AnthropicAPI.base + "/messages", method: "POST",
                                       body: try JSONSerialization.data(withJSONObject: Self.body(r, think: think)),
                                       contentType: "application/json", headers: headers.merging(["Accept": "text/event-stream"]) { mine, _ in mine },
                                       timeout: think ? 180 : 90)
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if !(200..<300).contains((response as? HTTPURLResponse)?.statusCode ?? 0) {
            var body = Data()
            for try await byte in bytes.prefix(600) { body.append(byte) }
            do { try HTTP.check(response, body: body) } catch { throw AnthropicAPI.explained(error) }
        }
        var stream = AnthropicStream()
        for try await line in bytes.lines {
            let before = stream.text.count
            if try stream.feed(line) { break }
            if stream.text.count != before { partial(stream.text) }
        }
        // A refusal is billed for what was read: counted before it's said.
        LLMUsage.report(stream.usage)
        if stream.stop == "refusal" {
            throw LLMError("Claude declined this request (Anthropic's safety check). Put it in other words, or pick another model.")
        }
        guard !stream.text.isEmpty else { throw LLMError("The model returned an empty reply.") }
        return stream.text
    }

    func models() async throws -> [String] {
        struct Model: Decodable { let id: String }
        struct List: Decodable { let data: [Model] }
        let data: Data
        do { data = try await HTTP.get(AnthropicAPI.base + "/models?limit=1000", headers: headers) } catch { throw AnthropicAPI.explained(error) }
        return try JSONDecoder().decode(List.self, from: data).data.map(\.id)
    }
}

/// The Messages API's server-sent events, read into the answer's text, why it stopped and what it used.
struct AnthropicStream {
    private(set) var text = ""
    private(set) var stop: String?
    /// message_start's usage, updated by each message_delta's (its counts are the totals so far).
    private var counts: [String: Any] = [:]

    var usage: LLMUsage? { LLMUsage(reported: counts) }

    /// Returns true when the message is complete.
    mutating func feed(_ line: String) throws -> Bool {
        guard line.hasPrefix("data:"),
              let event = try? JSONSerialization.jsonObject(with: Data(line.dropFirst(5).utf8)) as? [String: Any],
              let type = event["type"] as? String else { return false }
        switch type {
        case "message_start":
            counts = (event["message"] as? [String: Any])?["usage"] as? [String: Any] ?? [:]
        case "content_block_delta":
            // Thinking comes in blocks of its own (empty unless asked to show): only the answer's text is kept.
            guard let delta = event["delta"] as? [String: Any], delta["type"] as? String == "text_delta" else { break }
            text += delta["text"] as? String ?? ""
        case "message_delta":
            counts.merge((event["usage"] as? [String: Any] ?? [:]).filter { !($0.value is NSNull) }) { _, new in new }
            stop = (event["delta"] as? [String: Any])?["stop_reason"] as? String ?? stop
        case "message_stop":
            return true
        case "error":
            let error = event["error"] as? [String: Any]
            throw LLMError("Anthropic: \(error?["message"] as? String ?? error?["type"] as? String ?? "error")")
        default:
            break
        }
        return false
    }
}

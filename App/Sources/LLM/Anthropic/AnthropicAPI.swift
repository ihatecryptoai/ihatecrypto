import Foundation
import LLMCore

/// An Anthropic API key, on Anthropic's own Messages API. Its OpenAI-compatible endpoint, used before, is «not long-term
/// or production-ready» by Anthropic's word: it ignored «Think before answering» and cached nothing. Here the chat's
/// instructions are cached (a repeat within 5 minutes reads them at a tenth of the price or less), the effort follows
/// «Think before answering», and the cache's tokens reach the context chip. A request the model's safety check declines
/// is answered by the fallback model Anthropic recommends for that kind of refusal.
public struct AnthropicAPI: LLMProvider {
    static let base = "https://api.anthropic.com/v1"

    public init() {}

    public var info: ProviderInfo {
        ProviderInfo(id: "anthropic", name: "Anthropic API", kind: .apiKey, defaultModel: "claude-opus-5-5",
                     note: "Billed separately from a Claude subscription. Anthropic refuses a key that isn't scoped to a workspace.",
                     takesKey: true, models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-haiku-5-5"],
                     keyURL: "https://platform.claude.com/settings/keys", keySteps: "Create key, pick a workspace for it, add credits in Billing.")
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
    /// The beta that takes `"fallbacks": "default"` (the array form takes server-side-fallback-2026-06-01 instead).
    static let fallbackBeta = "server-side-fallback-2026-07-01"

    var headers: [String: String] { ["x-api-key": key, "anthropic-version": Self.version] }

    /// The instructions go as one block marked for the cache: they are the same in every chat request (≈ 18k tokens),
    /// so only the first in 5 minutes pays for them in full. A JSON answer is asked for in the instructions: the API's own
    /// JSON mode needs a schema. Thinking: Claude 5 models think as they see fit without being asked, at their own
    /// effort, and «Think before answering» raises it to high, as the Claude Code and ChatGPT clients do.
    static func body(_ r: LLMRequest, think: Bool, cap: Int = 32_000, fallback: Bool = true) -> [String: Any] {
        var body: [String: Any] = [
            "model": r.model,
            // A cap, not a target: thinking counts toward it, and a JSON answer is a few hundred tokens.
            "max_tokens": cap,
            "stream": true,
            "messages": r.dialogue.map { ["role": $0.role.rawValue, "content": $0.text] },
        ]
        if !r.system.isEmpty { body["system"] = [["type": "text", "text": r.system, "cache_control": ["type": "ephemeral"]]] }
        if think {
            body["thinking"] = ["type": "adaptive"]
            body["output_config"] = ["effort": "high"]
        }
        // ponytail: the chip prices a fallback's answer at the asked model's rates, and a declined partial
        // (usage.iterations) isn't counted; read iterations when refusals become common.
        if fallback && fallsBack(r.model) { body["fallbacks"] = "default" }
        return body
    }

    /// Opus 5 and 5.5, Sonnet 5.5, Fable 5 and 5.1: the models whose declined request Anthropic answers with a fallback
    /// model. Haiku 5.5 has none («a declined request stays declined»), nor have Sonnet 5 and the 4.x models.
    static func fallsBack(_ model: String) -> Bool {
        ["claude-opus-5", "claude-sonnet-5-5", "claude-fable-5"].contains { model.hasPrefix($0) }
    }

    func complete(_ r: LLMRequest) async throws -> String { try await stream(r) { _ in } }

    /// A refused fallback field (its beta renamed or ended) is dropped and the request asked again as it was.
    // ponytail: refused again on every request after a beta ends (one 400 more each); remember it if that happens.
    func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        do { return try await streamPlainer(r, fallback: true, partial: partial) } catch let refused as NetError
            where Self.fallsBack(r.model) && refused.message.hasPrefix("HTTP 400") && refused.message.contains("fallback") {
            return try await streamPlainer(r, fallback: false, partial: partial)
        }
    }

    /// An older model picked from the live list is asked once more as plainly as it gets: Haiku 4.5 and before take
    /// neither adaptive thinking nor an effort, and some write 4,096 tokens at most.
    private func streamPlainer(_ r: LLMRequest, fallback: Bool, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        do { return try await streamOnce(r, think: r.thinking, fallback: fallback, partial: partial) } catch let refused as NetError
            where refused.message.hasPrefix("HTTP 400") && !refused.message.contains("fallback")
                && ["thinking", "effort", "max_tokens"].contains(where: { refused.message.contains($0) }) {
            return try await streamOnce(r, think: false, cap: 4_096, fallback: fallback, partial: partial)
        }
    }

    private func streamOnce(_ r: LLMRequest, think: Bool, cap: Int = 32_000, fallback: Bool, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let body = Self.body(r, think: think, cap: cap, fallback: fallback)
        var sent = headers.merging(["Accept": "text/event-stream"]) { mine, _ in mine }
        if body["fallbacks"] != nil { sent["anthropic-beta"] = Self.fallbackBeta }
        let request = try HTTP.request(AnthropicAPI.base + "/messages", method: "POST",
                                       body: try JSONSerialization.data(withJSONObject: body),
                                       contentType: "application/json", headers: sent,
                                       timeout: think ? 180 : 90)
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if !(200..<300).contains((response as? HTTPURLResponse)?.statusCode ?? 0) {
            var body = Data()
            for try await byte in bytes.prefix(600) { body.append(byte) }
            do { try HTTP.check(response, body: body) } catch { throw AnthropicAPI.explained(error) }
        }
        var stream = AnthropicStream(), ended = false
        for try await line in bytes.lines {
            let before = stream.text.count
            if try stream.feed(line) { ended = true; break }
            if stream.text.count != before { partial(stream.text) }
        }
        // A refusal is billed for what was read: counted before it's said. Said only when the fallback model declined
        // too, or there was none.
        LLMUsage.report(stream.usage)
        if stream.stop == "refusal" {
            throw LLMError("Claude declined this request (Anthropic's safety check). Put it in other words, or pick another model.")
        }
        // Half an answer is never taken for a whole one.
        guard ended else { throw LLMError("Anthropic's answer was cut off on the way. Send it again.") }
        if stream.stop == "max_tokens" { throw LLMError("The model's answer ran past its length limit. Send it again, or pick another model.") }
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
            // Thinking comes in blocks of its own (empty unless asked to show): only the answer's text is kept. A fallback
            // block (a declined model's switch) has no deltas, and the fallback model goes on from the text so far.
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

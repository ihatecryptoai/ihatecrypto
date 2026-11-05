import Foundation

/// What one answer used, as its service counted it.
public struct LLMUsage: Sendable, Equatable {
    /// Everything the model read for the answer, the parts its service kept in a cache included.
    public var input: Int
    /// The part of `input` read from the service's cache, billed lower.
    public var cached: Int
    /// The part of `input` written to the service's cache (Anthropic bills it at 1.25× the input price).
    public var written: Int
    public var output: Int
    /// The model's context window, when the service says it (Claude Code, the Codex backend, Ollama).
    public var window: Int?
    /// The service's own price for the answer in dollars, when it sends one (OpenRouter, Claude Code).
    public var cost: Double?

    public init(input: Int, cached: Int = 0, written: Int = 0, output: Int, window: Int? = nil, cost: Double? = nil) {
        self.input = input
        self.cached = cached
        self.written = written
        self.output = output
        self.window = window
        self.cost = cost
    }

    /// A service's «usage» object in any of its three shapes: chat completions (prompt_tokens, the cached part counted
    /// inside them), the Responses API (input_tokens, the same) and Anthropic's (input_tokens with the cached parts
    /// beside them). nil when it names no prompt size.
    public init?(reported usage: [String: Any]?) {
        guard let usage, let read = Self.int(usage["prompt_tokens"] ?? usage["input_tokens"]) else { return nil }
        let details = (usage["prompt_tokens_details"] ?? usage["input_tokens_details"]) as? [String: Any]
        let written = Self.int(usage["cache_creation_input_tokens"]) ?? 0, hit = Self.int(usage["cache_read_input_tokens"])
        self.init(input: read + written + (hit ?? 0), cached: hit ?? Self.int(details?["cached_tokens"]) ?? 0, written: written,
                  output: Self.int(usage["completion_tokens"] ?? usage["output_tokens"]) ?? 0,
                  cost: (usage["cost"] as? NSNumber)?.doubleValue)
    }

    static func int(_ value: Any?) -> Int? { (value as? NSNumber)?.intValue }

    /// Whoever counts the answers of the task making a request (`MeteredClient`); a client tells it through `report`.
    @TaskLocal public static var listener: (@Sendable (LLMUsage) -> Void)?

    public static func report(_ usage: LLMUsage?) {
        if let usage { listener?(usage) }
    }
}

/// A client that tells `heard` what each of its answers used, with the request it answered.
public struct MeteredClient: LLMClient {
    let base: any LLMClient
    let heard: @Sendable (LLMUsage, LLMRequest) -> Void

    public init(_ base: any LLMClient, heard: @escaping @Sendable (LLMUsage, LLMRequest) -> Void) {
        self.base = base
        self.heard = heard
    }

    public func complete(_ request: LLMRequest) async throws -> String {
        try await LLMUsage.$listener.withValue({ [heard] in heard($0, request) }) { try await base.complete(request) }
    }

    public func stream(_ request: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        try await LLMUsage.$listener.withValue({ [heard] in heard($0, request) }) { try await base.stream(request, partial: partial) }
    }
}

/// Context windows a service told for its own models (the Codex backend lists them with a subscription's models),
/// kept for its client to report with an answer.
public final class ModelWindows: @unchecked Sendable {
    public static let told = ModelWindows()
    private let lock = NSLock()
    private var table: [String: Int] = [:]

    public subscript(model: String) -> Int? {
        get { lock.withLock { table[model] } }
        set { lock.withLock { table[model] = newValue } }
    }
}

/// A model's context window and list prices, from OpenRouter's public list: it carries every service's models, and
/// the services themselves publish prices only as web pages.
public struct ModelFacts: Sendable, Equatable {
    public var window: Int
    /// Dollars a token: read, read from the cache, written as the answer.
    public var input: Double, cached: Double, output: Double
    /// Dollars a token written to the cache: the read price where the list names none.
    public var written: Double
    /// A prompt of more tokens than this is billed at these prices, every token of it: 0 for the base prices.
    public var over = 0
    /// The model's dearer prices for long prompts, by size (`pricing.overrides`): Haiku 5.5 costs 5× past 100k tokens,
    /// GPT-6.1 Sol and Terra 2× past 272k.
    public var longer: [ModelFacts] = []

    public init(window: Int, input: Double, cached: Double, output: Double, written: Double? = nil) {
        self.window = window
        self.input = input
        self.cached = cached
        self.output = output
        self.written = written ?? input
    }

    // ponytail: DeepSeek's cheaper hours of the day aren't applied (OpenRouter's list doesn't carry them).
    public func cost(_ usage: LLMUsage) -> Double {
        let p = longer.last { usage.input > $0.over } ?? self
        return Double(usage.input - usage.cached - usage.written) * p.input + Double(usage.cached) * p.cached + Double(usage.written) * p.written
            + Double(usage.output) * p.output
    }

    public static let source = "https://openrouter.ai/api/v1/models"

    /// One name for a model however a service spells it: «anthropic/claude-opus-5.5» and «claude-opus-5-5» are both
    /// «claudeopus55».
    public static func key(_ model: String) -> String {
        String((model.split(separator: "/").last ?? "").lowercased().filter { $0.isLetter || $0.isNumber })
    }

    /// OpenRouter's /models reply as a table by `key`. A variant («…:free») has a key of its own.
    /// DeepSeek's own «deepseek-flash» has no row (the list's «~deepseek/deepseek-flash-latest» is a routing alias with
    /// prices of its own, 2026-10-13): unpriced, its chat shows tokens.
    public static func table(_ data: Data) -> [String: ModelFacts] {
        guard let rows = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["data"] as? [[String: Any]] else { return [:] }
        var table: [String: ModelFacts] = [:]
        for row in rows {
            guard let id = row["id"] as? String, let window = LLMUsage.int(row["context_length"]), window > 0,
                  table[key(id)] == nil else { continue }
            let base = row["pricing"] as? [String: Any] ?? [:]
            func facts(_ prices: [String: Any]) -> ModelFacts {
                func price(_ name: String) -> Double? { (prices[name] as? String).flatMap(Double.init) ?? (prices[name] as? NSNumber)?.doubleValue }
                let input = price("prompt") ?? 0
                return ModelFacts(window: window, input: input, cached: price("input_cache_read") ?? input, output: price("completion") ?? 0,
                                  written: price("input_cache_write"))
            }
            var model = facts(base)
            // A tier names only what changes: the rest is the base price.
            model.longer = (base["overrides"] as? [[String: Any]] ?? []).compactMap { tier in
                // A number in the list (2026-11-05), read as a string too, as its prices are.
                guard let over = LLMUsage.int(tier["min_prompt_tokens"]) ?? (tier["min_prompt_tokens"] as? String).flatMap(Int.init) else { return nil }
                var long = facts(base.merging(tier) { _, new in new })
                long.over = over
                return long
            }.sorted { $0.over < $1.over }
            table[key(id)] = model
        }
        return table
    }
}

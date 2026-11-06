import Foundation
@testable import LLMAPIKey
@testable import LLMApple
@testable import LLMCatalog
@testable import LLMChatGPT
@testable import LLMClaudeCode
import LLMCore
import Testing

// MARK: Core

@Test func pkceMatchesRFC7636Vector() {
    let pkce = PKCE(verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
    #expect(pkce.challenge == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
    #expect(PKCE.make().verifier.count == 43)
}

@Test func jsonTextFindsObjectInFencesAndProse() throws {
    for reply in ["{\"a\":1}", "```json\n{\"a\": 1}\n```", "Вот ответ: {\"a\": 1}. Готово"] {
        let data = try #require(JSONText.object(in: reply))
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Int])
        #expect(object["a"] == 1)
    }
    #expect(JSONText.object(in: "нет json") == nil)
}

@Test func loopbackServerReturnsCallbackQuery() async throws {
    let server = try await LoopbackServer.start(path: "/callback")
    #expect(server.port > 0)
    async let query = server.waitForCallback(timeout: 10)
    let (data, response) = try await URLSession.shared.data(from: URL(string: "http://127.0.0.1:\(server.port)/callback?code=abc&state=xyz")!)
    #expect((response as? HTTPURLResponse)?.statusCode == 200)
    #expect(String(decoding: data, as: UTF8.self).contains("Signed in"))
    #expect(try await query == ["code": "abc", "state": "xyz"])
}

// MARK: ChatGPT subscription

private func jwt(_ claims: [String: Any]) -> String {
    let payload = PKCE.base64url(try! JSONSerialization.data(withJSONObject: claims))
    return "eyJhbGciOiJub25lIn0.\(payload).sig"
}

@Test func chatGPTTokensReadAccountAndExpiry() throws {
    let id = jwt(["https://api.openai.com/auth": ["chatgpt_account_id": "acc-1"]])
    let access = jwt(["exp": 2_000_000_000])
    let data = try JSONSerialization.data(withJSONObject: ["access_token": access, "refresh_token": "r1", "id_token": id])
    let tokens = try ChatGPTSubscription.tokens(from: data, previous: nil)
    #expect(tokens.accountID == "acc-1")
    #expect(tokens.refresh == "r1")
    #expect(tokens.expires == Date(timeIntervalSince1970: 2_000_000_000))

    // A refresh response without a new refresh token or id_token keeps the old ones.
    let refreshed = try ChatGPTSubscription.tokens(
        from: try JSONSerialization.data(withJSONObject: ["access_token": access]), previous: tokens)
    #expect(refreshed.refresh == "r1")
    #expect(refreshed.accountID == "acc-1")
}

@Test func codexBodyPutsSystemIntoInstructions() throws {
    let body = ChatGPTClient.body(LLMRequest(model: "gpt-6-sol", messages: [
        LLMMessage(.system, "Отвечай JSON."), LLMMessage(.user, "привет"), LLMMessage(.assistant, "{}"),
    ]))
    #expect(body["instructions"] as? String == "Отвечай JSON.")
    #expect(body["stream"] as? Bool == true)
    #expect(body["store"] as? Bool == false)
    let input = try #require(body["input"] as? [[String: Any]])
    #expect(input.count == 2)
    let assistantPart = try #require((input[1]["content"] as? [[String: String]])?.first)
    #expect(assistantPart["type"] == "output_text")
}

@Test func codexStreamCollectsDeltasAndStopsOnCompleted() throws {
    var stream = CodexStream()
    #expect(try !stream.feed("event: response.output_text.delta"))
    #expect(try !stream.feed(#"data: {"type":"response.output_text.delta","delta":"{\"ok\""}"#))
    #expect(try !stream.feed(#"data: {"type":"response.output_text.delta","delta":":true}"}"#))
    #expect(try stream.feed(#"data: {"type":"response.completed","response":{}}"#))
    #expect(stream.text == #"{"ok":true}"#)

    var failing = CodexStream()
    #expect(throws: LLMError.self) {
        try failing.feed(#"data: {"type":"response.failed","response":{"error":{"message":"usage limit"}}}"#)
    }
}

// MARK: Claude Code, API keys, catalog

@Test func claudeCodeRunsWithoutToolsAndKeepsPromptOffTheCommandLine() {
    let args = ClaudeCodeClient.arguments(LLMRequest(model: "claude-haiku-4-5", messages: [
        LLMMessage(.system, "Отвечай JSON."), LLMMessage(.user, "секретный текст"),
    ]))
    #expect(args.contains("--restricted"))
    #expect(args[args.firstIndex(of: "--tools")! + 1] == "")
    // `--tools` takes several values; the next argument must be a flag so nothing is swallowed.
    #expect(args[args.firstIndex(of: "--tools")! + 2].hasPrefix("--"))
    #expect(!args.contains("секретный текст"))
    #expect(args[args.firstIndex(of: "--system-prompt")! + 1] == "Отвечай JSON.")
    #expect(ClaudeCodeClient.arguments(LLMRequest(model: "m", messages: [LLMMessage(.user, "x")])).contains("--system-prompt"))
}

/// A hung `claude -p` is stopped after its timeout, or when the chat's task is cancelled; it never keeps the chat busy.
@Test func claudeCodeRunStopsAHungProcess() async throws {
    let start = ContinuousClock.now
    await #expect(throws: LLMError.self) { _ = try await ClaudeCodeClient.run("/bin/sleep", ["30"], input: "x", timeout: .milliseconds(300)) }
    let run = Task { try await ClaudeCodeClient.run("/bin/sleep", ["30"], input: "x") }
    try await Task.sleep(for: .milliseconds(200))
    run.cancel()
    await #expect(throws: (any Error).self) { _ = try await run.value }
    #expect(ContinuousClock.now - start < .seconds(10))
    // A normal run still returns its output; a failed one says what it printed.
    #expect(try await ClaudeCodeClient.run("/bin/cat", [], input: "hello") == Data("hello".utf8))
    await #expect(throws: LLMError.self) { _ = try await ClaudeCodeClient.run("/bin/ls", ["/nonexistent-path"], input: "") }
}

/// `claude` closes its output a moment before it exits, many runs at once: each wait for the exit still ends (they hung in
/// `waitUntilExit` with the process gone, past the watchdog).
@Test(.timeLimit(.minutes(1))) func claudeCodeRunReturnsWhenTheProcessExitsAfterItsOutput() async throws {
    try await withThrowingTaskGroup(of: Data.self) { group in
        for i in 0..<40 { group.addTask { try await ClaudeCodeClient.run("/bin/sh", ["-c", "cat; exec 1>&-; sleep 0.3"], input: "\(i)") } }
        var outputs = 0
        for try await out in group where !out.isEmpty { outputs += 1 }
        #expect(outputs == 40)
    }
}

@Test func jsonModeOnlyWhereTheAPISupportsIt() {
    let request = LLMRequest(model: "m", messages: [LLMMessage(.user, "x")], wantsJSON: true)
    #expect(OpenAIChatClient(baseURL: "https://api.deepseek.com/", key: "k", jsonMode: true).body(request)["response_format"] != nil)
    #expect(OpenAIChatClient(baseURL: "https://api.anthropic.com/v1", key: "k", jsonMode: false).body(request)["response_format"] == nil)
    #expect(OpenAIChatClient(baseURL: "https://api.deepseek.com/", key: nil, jsonMode: true).baseURL == "https://api.deepseek.com")
}

@Test func apiKeyProviderRefusesToRunWithoutKey() {
    let deepseek = OpenAICompatible.presets.first { $0.info.id == "deepseek" }!
    #expect(throws: LLMError.self) { try deepseek.client(nil, save: { _ in }) }
    let ollama = OpenAICompatible.presets.first { $0.info.id == "ollama" }!
    #expect((try? ollama.client(nil, save: { _ in })) != nil)
}

@Test func catalogIDsAreUniqueAndUsableEntriesHaveAModelOrAreLocal() {
    let ids = LLMCatalog.all.map(\.info.id)
    #expect(Set(ids).count == ids.count)
    for p in LLMCatalog.all where p.info.unavailable == nil && p.info.kind != .local {
        #expect(!p.info.defaultModel.isEmpty, "\(p.info.id)")
    }
    #expect(LLMCatalog.provider(id: "grok-subscription")?.info.unavailable != nil)
    #expect(LLMCatalog.all.filter { $0.info.kind == .subscription }.count == 4)
}

@Test func modelIdsReadAsNames() {
    let names = [
        "gpt-6-sol": "GPT 6 SOL", "gpt-6.1-sol": "GPT 6.1 SOL", "gpt-6-astra": "GPT 6 Astra", "claude-opus-5-5": "Claude Opus 5.5", "claude-fable-5-1": "Claude Fable 5.1", "grok-4.7": "Grok 4.7", "claude-haiku-4-5": "Claude Haiku 4.5", "deepseek-flash": "DeepSeek Flash",
        "deepseek/deepseek-v4.1-flash": "DeepSeek V4.1 Flash", "gemini-3.8-flash": "Gemini 3.8 Flash",
        "grok-4.3": "Grok 4.3", "qwen3.5:9b": "Qwen3.5 9B", "gpt-6-pro": "GPT 6 Pro",
    ]
    for (id, name) in names { #expect(LLMCatalog.displayName(model: id) == name) }
}

@Test func theCodexBackendsListReadsItsPickableModelsInOrder() {
    let json = #"{"models":[{"slug":"gpt-6-sol","visibility":"list","priority":3},{"slug":"gpt-reserve","visibility":"hide","priority":4},{"slug":"gpt-6.1-sol","visibility":"list","priority":1},{"slug":"gpt-6-astra","visibility":"list","priority":2}]}"#
    #expect(ChatGPTSubscription.listed(Data(json.utf8)) == ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol"])
}

@Test func appleOffersPrivateCloudOnlyWithItsEntitlementAndSaysWhenABriefIsTooLong() async throws {
    let info = AppleIntelligence().info
    // The test runner has no com.apple.developer.private-cloud-compute.
    #expect(!info.models.contains(AppleIntelligence.privateCloud))
    guard info.models.contains(AppleIntelligence.onDevice) else { return }  // Apple Intelligence is off on this Mac.
    // The chat's router brief is ~5,000 tokens; the model on this Mac holds 4,096.
    let brief = String(repeating: "Answer as the wallet's agent. ", count: 1000)
    let error = await #expect(throws: LLMError.self) {
        try await AppleIntelligenceClient().complete(LLMRequest(model: info.defaultModel, messages: [LLMMessage(.system, brief), LLMMessage(.user, "hi")]))
    }
    // Said as what it is: too small for the chat, not «send it again».
    #expect(error?.message.contains("4,096 tokens") == true)
    #expect(AppleIntelligence.chatRefusal(model: AppleIntelligence.onDevice) != nil && AppleIntelligence.chatRefusal(model: "gpt-6.1-sol") == nil)
}

/// Claude Code's stream-json, from a stand-in that writes it the way `claude -p` does (2026-10-06): the words reach
/// `partial` as they come, and the answer is the result event's.
@Test(.timeLimit(.minutes(1))) func claudeCodeStreamTellsTheWordsAsTheyCome() async throws {
    let script = FileManager.default.temporaryDirectory.appendingPathComponent("fake-claude-\(UUID().uuidString).sh")
    try """
    #!/bin/sh
    cat > /dev/null
    echo '{"type":"system","subtype":"init"}'
    echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hm"}}}'
    echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"one "}}}'
    sleep 0.2
    echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"two"}}}'
    echo '{"type":"result","is_error":false,"result":"one two"}'
    """.write(to: script, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
    final class Seen: @unchecked Sendable { let lock = NSLock(); var texts: [String] = [] }
    let seen = Seen()
    let answer = try await ClaudeCodeClient(path: script.path).stream(LLMRequest(model: "m", messages: [LLMMessage(.user, "x")])) { text in
        seen.lock.withLock { seen.texts.append(text) }
    }
    #expect(answer == "one two")
    #expect(seen.texts == ["one ", "one two"])
    #expect(ClaudeCodeClient.arguments(LLMRequest(model: "m", messages: []), streaming: true).contains("--include-partial-messages"))
}

/// Ollama answers through its own API, with a window that holds ihatecrypto's instructions: its OpenAI-style endpoint
/// stays at 4096 tokens and cut them.
@Test func ollamaGetsAWideContextWindow() throws {
    let ollama = try #require(OpenAICompatible.presets.first { $0.info.id == "ollama" })
    let client = try #require(try ollama.client(nil, save: { _ in }) as? OllamaChatClient)
    #expect(client.base == "http://localhost:11434")
    let body = client.body(LLMRequest(model: "gemma4:26b", messages: [LLMMessage(.system, "s"), LLMMessage(.user, "u")], wantsJSON: true), stream: true)
    #expect((body["options"] as? [String: Int])?["num_ctx"] == 16_384)
    #expect(body["format"] as? String == "json" && body["stream"] as? Bool == true)
    // No thinking before the JSON: Gemma 4 12B took 117 s an answer with it, 8.4 s without.
    #expect(body["think"] as? Bool == false)
    #expect(client.body(LLMRequest(model: "gpt-oss:20b", messages: [], wantsJSON: true), stream: true, think: true)["think"] == nil)
}

// MARK: What an answer used (the chat's context chip)

@Test func usageReadsEachServicesShape() throws {
    // Chat completions: the cached part is counted inside prompt_tokens; OpenRouter adds the price it charged.
    let openRouter = ["prompt_tokens": 11_000, "completion_tokens": 40, "prompt_tokens_details": ["cached_tokens": 10_240], "cost": 0.0123] as [String: Any]
    #expect(LLMUsage(reported: openRouter) == LLMUsage(input: 11_000, cached: 10_240, output: 40, cost: 0.0123))
    // The Responses API (the Codex backend).
    let responses = ["input_tokens": 9_000, "input_tokens_details": ["cached_tokens": 8_000], "output_tokens": 25] as [String: Any]
    #expect(LLMUsage(reported: responses) == LLMUsage(input: 9_000, cached: 8_000, output: 25))
    // Anthropic: the cached parts are beside input_tokens, not in them.
    let anthropic = ["input_tokens": 521, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 10_000, "output_tokens": 36] as [String: Any]
    #expect(LLMUsage(reported: anthropic) == LLMUsage(input: 10_621, cached: 10_000, output: 36))
    #expect(LLMUsage(reported: ["total_tokens": 5]) == nil && LLMUsage(reported: nil) == nil)
}

/// The result object of a real `claude -p` run (2026-10-13), cut to what is read.
@Test func claudeCodeResultTellsWhatTheRunUsed() {
    let result = #"{"type":"result","is_error":false,"result":"works","total_cost_usd":0.000701,"usage":{"input_tokens":521,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":36},"modelUsage":{"claude-haiku-4-5":{"inputTokens":521,"outputTokens":36,"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"costUSD":0.000701,"contextWindow":200000}}}"#
    #expect(ClaudeCodeClient.usage(Data(result.utf8)) == LLMUsage(input: 521, output: 36, window: 200_000, cost: 0.000701))
    #expect(ClaudeCodeClient.usage(Data("not json".utf8)) == nil)
}

@Test func codexStreamKeepsWhatTheResponseUsedAndTheBackendsListSaysEachWindow() throws {
    var stream = CodexStream()
    _ = try stream.feed(#"data: {"type":"response.output_text.delta","delta":"ok"}"#)
    #expect(try stream.feed(#"data: {"type":"response.completed","response":{"usage":{"input_tokens":9000,"input_tokens_details":{"cached_tokens":8000},"output_tokens":25,"total_tokens":9025}}}"#))
    #expect(stream.usage == LLMUsage(input: 9_000, cached: 8_000, output: 25))
    // ~/.codex/models_cache.json holds the same reply: 272,000 for a subscription's models.
    let list = #"{"models":[{"slug":"gpt-6-luna","visibility":"list","priority":4,"context_window":272000,"max_context_window":872000},{"slug":"old"}]}"#
    #expect(ChatGPTSubscription.windows(Data(list.utf8)) == ["gpt-6-luna": 272_000])
}

@Test func modelFactsReadOpenRoutersListAndPriceAnAnswer() throws {
    let list = #"{"data":[{"id":"anthropic/claude-opus-5.5","context_length":1000000,"pricing":{"prompt":"0.000004","completion":"0.00002","input_cache_read":"0.0000002"}},{"id":"deepseek/deepseek-v4.1-flash:free","context_length":1048576,"pricing":{"prompt":"0","completion":"0"}},{"id":"~deepseek/deepseek-flash-latest","context_length":1048576,"pricing":{"prompt":"0.00000015","completion":"0.0000006"}},{"id":"no-window"}]}"#
    let table = ModelFacts.table(Data(list.utf8))
    // The Anthropic API's own spelling of the model finds OpenRouter's row; a «:free» variant is a row of its own.
    let opus = try #require(table[ModelFacts.key("claude-opus-5-5")])
    #expect(opus.window == 1_000_000 && table.count == 3 && table[ModelFacts.key("deepseek-v4.1-flash")] == nil)
    // DeepSeek's own «deepseek-flash» isn't the list's «…-latest» alias, whose prices are its own: left unpriced.
    #expect(table[ModelFacts.key("deepseek-flash")] == nil)
    // A row with no cache price reads from the cache at the full one.
    #expect(table[ModelFacts.key("deepseek-flash-latest")] == ModelFacts(window: 1_048_576, input: 0.00000015, cached: 0.00000015, output: 0.0000006))
    // 1,000 tokens read anew, 10,000 from the cache, 100 written.
    #expect(abs(opus.cost(LLMUsage(input: 11_000, cached: 10_000, output: 100)) - 0.008) < 1e-12)
    // Ollama's window is the one its client asks for: known before its first answer.
    #expect(LLMCatalog.window(provider: "ollama") == 16_384 && LLMCatalog.window(provider: "openai") == nil)
}

@Test func aMeteredClientHearsWhatItsAnswersUsed() async throws {
    struct Counting: LLMClient {
        func complete(_ request: LLMRequest) async throws -> String {
            LLMUsage.report(LLMUsage(input: 7, output: 1))
            return "ok"
        }
    }
    final class Heard: @unchecked Sendable { let lock = NSLock(); var all: [(LLMUsage, String)] = [] }
    let heard = Heard()
    let client = MeteredClient(Counting()) { usage, request in heard.lock.withLock { heard.all.append((usage, request.model)) } }
    #expect(try await client.complete(LLMRequest(model: "m", messages: [])) == "ok")
    _ = try await client.stream(LLMRequest(model: "n", messages: [])) { _ in }
    #expect(heard.all.map(\.1) == ["m", "n"] && heard.all.allSatisfy { $0.0.input == 7 })
    // Nobody listening: a client's report goes nowhere.
    _ = try await Counting().complete(LLMRequest(model: "m", messages: []))
    #expect(heard.all.count == 2)
}

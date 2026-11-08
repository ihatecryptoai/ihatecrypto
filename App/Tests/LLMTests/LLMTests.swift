import Foundation
@testable import LLMAnthropic
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
    // What the sandboxed app sends the helper can only be a model's name and the prompt, never a flag.
    #expect(ClaudeCodeClient.isModelName("claude-opus-5-5") && ClaudeCodeClient.isModelName("claude-sonnet-4-5[1m]"))
    #expect(!ClaudeCodeClient.isModelName("--dangerously-skip-permissions") && !ClaudeCodeClient.isModelName("x --tools Bash") && !ClaudeCodeClient.isModelName(""))
    let flagged = ClaudeCodeClient.arguments(model: "m", system: "--tools Bash", thinking: false)
    #expect(flagged[flagged.firstIndex(of: "--system-prompt")! + 1] == " --tools Bash")
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
    #expect(OpenAIChatClient(baseURL: "http://localhost:1234/v1", key: nil, jsonMode: false).body(request)["response_format"] == nil)
    #expect(OpenAIChatClient(baseURL: "https://api.deepseek.com/", key: nil, jsonMode: true).baseURL == "https://api.deepseek.com")
}

@Test func apiKeyProviderRefusesToRunWithoutKey() {
    let deepseek = OpenAICompatible.presets.first { $0.info.id == "deepseek" }!
    #expect(throws: LLMError.self) { try deepseek.client(nil, save: { _ in }) }
    let ollama = OpenAICompatible.presets.first { $0.info.id == "ollama" }!
    #expect((try? ollama.client(nil, save: { _ in })) != nil)
}

// MARK: Anthropic's Messages API

/// An Anthropic key goes to Anthropic's own API, in its old place in the list, and the same Keychain entry («anthropic»).
@Test func anAnthropicKeyUsesTheMessagesAPI() throws {
    let ids = LLMCatalog.all.map(\.info.id)
    #expect(ids.firstIndex(of: "anthropic") == ids.firstIndex(of: "openai").map { $0 + 1 })
    let anthropic = try #require(LLMCatalog.provider(id: "anthropic"))
    #expect(anthropic is AnthropicAPI && anthropic.info.kind == .apiKey && anthropic.info.takesKey)
    #expect(throws: LLMError.self) { try anthropic.client(nil, save: { _ in }) }
    #expect(try anthropic.client(.apiKey("k"), save: { _ in }) is AnthropicClient)
    #expect(!OpenAICompatible.presets.contains { $0.info.id == "anthropic" })
    #expect(AnthropicClient(key: "k").headers == ["x-api-key": "k", "anthropic-version": "2026-10-21"])
}

/// The instructions go as one cached block, the dialogue as it is; «Think before answering» is adaptive thinking at high
/// effort, and without it the model keeps its own.
@Test func anthropicRequestCachesTheInstructionsAndThinksWhenAsked() throws {
    let r = LLMRequest(model: "claude-opus-5-5", messages: [
        LLMMessage(.system, "Answer as JSON."), LLMMessage(.user, "купи SOL на $5"), LLMMessage(.assistant, "{}"), LLMMessage(.user, "ещё раз"),
    ], wantsJSON: true)
    let body = AnthropicClient.body(r, think: false)
    #expect(body["model"] as? String == "claude-opus-5-5" && body["stream"] as? Bool == true && body["max_tokens"] as? Int == 32_000)
    let system = try #require(body["system"] as? [[String: Any]])
    #expect(system.count == 1 && system[0]["text"] as? String == "Answer as JSON.")
    #expect((system[0]["cache_control"] as? [String: String]) == ["type": "ephemeral"])
    let messages = try #require(body["messages"] as? [[String: String]])
    #expect(messages.map { $0["role"]! } == ["user", "assistant", "user"] && messages[0]["content"] == "купи SOL на $5")
    // No OpenAI-style fields, and no thinking settings unless asked.
    #expect(body["response_format"] == nil && body["thinking"] == nil && body["output_config"] == nil)
    let thinking = AnthropicClient.body(r, think: true)
    #expect((thinking["thinking"] as? [String: String]) == ["type": "adaptive"])
    #expect((thinking["output_config"] as? [String: String]) == ["effort": "high"])
    // No instructions, no system field (an empty block is refused).
    #expect(AnthropicClient.body(LLMRequest(model: "m", messages: [LLMMessage(.user, "x")]), think: false)["system"] == nil)
}

/// The event stream as the Messages API sends it (the docs' shape, with a thinking block before the text and a cache hit):
/// the text, the stop reason and the counts, cache parts included.
@Test func anthropicStreamReadsTheTextAndWhatTheAnswerUsed() throws {
    let events = #"""
    event: message_start
    data: {"type":"message_start","message":{"id":"msg_01","type":"message","role":"assistant","model":"claude-opus-5-5","content":[],"stop_reason":null,"usage":{"input_tokens":412,"cache_creation_input_tokens":0,"cache_read_input_tokens":18210,"output_tokens":1}}}

    event: content_block_start
    data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}

    event: content_block_delta
    data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EqQB"}}

    event: ping
    data: {"type": "ping"}

    event: content_block_start
    data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}

    event: content_block_delta
    data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"{\"reply\":"}}

    event: content_block_delta
    data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"\"ok\"}"}}

    event: content_block_stop
    data: {"type":"content_block_stop","index":1}

    event: message_delta
    data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":57,"cache_creation_input_tokens":null}}

    event: message_stop
    data: {"type":"message_stop"}
    """#
    var stream = AnthropicStream(), texts: [String] = [], ended = false
    for line in events.split(separator: "\n", omittingEmptySubsequences: false).map(String.init) where !ended {
        let before = stream.text
        ended = try stream.feed(line)
        if stream.text != before { texts.append(stream.text) }
    }
    #expect(ended && stream.text == #"{"reply":"ok"}"# && texts == [#"{"reply":"#, #"{"reply":"ok"}"#])
    #expect(stream.stop == "end_turn")
    #expect(stream.usage == LLMUsage(input: 18_622, cached: 18_210, output: 57))

    // A refusal is told by its stop reason, an overloaded API by an error event.
    var refused = AnthropicStream()
    _ = try refused.feed(#"data: {"type":"message_delta","delta":{"stop_reason":"refusal"},"usage":{"output_tokens":0}}"#)
    #expect(refused.stop == "refusal" && refused.text.isEmpty)
    var failing = AnthropicStream()
    let error = #expect(throws: LLMError.self) {
        try failing.feed(#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)
    }
    #expect(error?.message == "Anthropic: Overloaded")
}

/// A model whose safety check can decline a request asks Anthropic for its recommended fallback model then. Haiku 5.5
/// has none («a declined request stays declined»), nor have Sonnet 5 and the 4.x models.
@Test func anthropicAsksForAFallbackOnTheModelsThatHaveOne() {
    func asks(_ model: String) -> Bool {
        AnthropicClient.body(LLMRequest(model: model, messages: [LLMMessage(.user, "x")]), think: false)["fallbacks"] as? String == "default"
    }
    for model in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-opus-5", "claude-fable-5"] { #expect(asks(model), "\(model)") }
    for model in ["claude-haiku-5-5", "claude-opus-4-8", "claude-haiku-4-5", "claude-sonnet-5"] { #expect(!asks(model), "\(model)") }
    #expect(AnthropicClient.body(LLMRequest(model: "claude-opus-5-5", messages: [LLMMessage(.user, "x")]), think: false, fallback: false)["fallbacks"] == nil)
}

/// A refusal midway: the declined model's words stay, a fallback block marks the switch, and the fallback model goes on
/// from them (docs: refusals-and-fallback). Read as one answer.
@Test func anthropicStreamReadsAFallbackAsOneAnswer() throws {
    let events = [
        #"data: {"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":40,"output_tokens":1}}}"#,
        #"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        #"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"route\":\"chat\",\"re"}}"#,
        #"data: {"type":"content_block_stop","index":0}"#,
        #"data: {"type":"content_block_start","index":1,"content_block":{"type":"fallback","from":{"model":"claude-opus-5-5"},"to":{"model":"claude-opus-4-8"}}}"#,
        #"data: {"type":"content_block_stop","index":1}"#,
        #"data: {"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}"#,
        #"data: {"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"ply\":\"ok\"}"}}"#,
        #"data: {"type":"content_block_stop","index":2}"#,
        #"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":12}}"#,
        #"data: {"type":"message_stop"}"#,
    ]
    var stream = AnthropicStream(), ended = false
    for line in events where !ended { ended = try stream.feed(line) }
    #expect(ended && stream.text == #"{"route":"chat","reply":"ok"}"# && stream.stop == "end_turn")
}

/// api.anthropic.com as the test answers it: each request kept with its body, the replies given in turn.
nonisolated final class AnthropicStub: URLProtocol {
    nonisolated(unsafe) static var replies: [(status: Int, body: String)] = []
    nonisolated(unsafe) static var asked: [(headers: [String: String], body: [String: Any])] = []

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host == "api.anthropic.com" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var body = Data()
        if let stream = request.httpBodyStream {   // URLProtocol gets the body as a stream, not httpBody
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 { body.append(buffer, count: n) }
            stream.close()
        }
        Self.asked.append((request.allHTTPHeaderFields ?? [:], (try? JSONSerialization.jsonObject(with: body)) as? [String: Any] ?? [:]))
        let reply = Self.replies.isEmpty ? (status: 500, body: "no reply left in the test") : Self.replies.removeFirst()
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: reply.status, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(reply.body.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// The whole way through the client: the key and version sent, the words told as they come, what the answer used told to
/// the meter. A model that refuses adaptive thinking (Haiku 4.5) is asked again without it, and a refused fallback
/// field is dropped.
@Test func anthropicClientStreamsTheAnswerAndCountsIt() async throws {
    URLProtocol.registerClass(AnthropicStub.self)
    defer { URLProtocol.unregisterClass(AnthropicStub.self) }
    AnthropicStub.asked = []
    AnthropicStub.replies = [
        (400, #"{"type":"error","error":{"type":"invalid_request_error","message":"adaptive thinking is not supported on this model"}}"#),
        (200, [#"data: {"type":"message_start","message":{"usage":{"input_tokens":30,"cache_creation_input_tokens":5000,"cache_read_input_tokens":0,"output_tokens":1}}}"#,
               #"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"wor"}}"#,
               #"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"king"}}"#,
               #"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}"#,
               #"data: {"type":"message_stop"}"#].joined(separator: "\n\n") + "\n\n"),
    ]
    final class Heard: @unchecked Sendable { let lock = NSLock(); var usage: [LLMUsage] = []; var partial: [String] = [] }
    let heard = Heard()
    let client = MeteredClient(AnthropicClient(key: "sk-ant-test")) { usage, _ in heard.lock.withLock { heard.usage.append(usage) } }
    let answer = try await client.stream(LLMRequest(model: "claude-haiku-4-5", messages: [LLMMessage(.system, "s"), LLMMessage(.user, "x")], thinking: true)) { text in
        heard.lock.withLock { heard.partial.append(text) }
    }
    #expect(answer == "working" && heard.partial == ["wor", "working"])
    #expect(heard.usage == [LLMUsage(input: 5_030, written: 5_000, output: 3)])
    #expect(AnthropicStub.asked.count == 2 && AnthropicStub.asked.allSatisfy { $0.headers["x-api-key"] == "sk-ant-test" && $0.headers["anthropic-version"] == "2026-10-21" })
    #expect(AnthropicStub.asked[0].body["thinking"] != nil && AnthropicStub.asked[1].body["thinking"] == nil && AnthropicStub.asked[1].body["output_config"] == nil)
    #expect(AnthropicStub.asked[0].body["max_tokens"] as? Int == 32_000 && AnthropicStub.asked[1].body["max_tokens"] as? Int == 4_096)

    // A stream that ends before message_stop is half an answer: said, not taken for the whole.
    AnthropicStub.replies = [(200, #"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"rep"}}"# + "\n\n")]
    let cut = await #expect(throws: LLMError.self) {
        try await AnthropicClient(key: "k").complete(LLMRequest(model: "claude-opus-5-5", messages: [LLMMessage(.user, "x")]))
    }
    #expect(cut?.message.contains("cut off") == true)

    // A key that isn't scoped to a workspace: Anthropic's 400 (docs: manage-claude/authentication) is said as what to do,
    // in the chat and in Settings' model list alike, and not asked again without thinking.
    let noWorkspace = #"{"type":"error","error":{"type":"invalid_request_error","message":"This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header with the ID of the workspace to use. Add the header, or use an API key that is scoped to a workspace."},"request_id":"req_011CSHoEeqs5C35K2UUqR7Fy"}"#
    AnthropicStub.asked = []
    AnthropicStub.replies = [(400, noWorkspace), (400, noWorkspace)]
    let chat = await #expect(throws: LLMError.self) {
        try await AnthropicClient(key: "k").complete(LLMRequest(model: "claude-opus-5-5", messages: [LLMMessage(.user, "x")], thinking: true))
    }
    #expect(chat?.message.contains("platform.claude.com/settings/keys › Create key") == true && AnthropicStub.asked.count == 1)
    let list = await #expect(throws: LLMError.self) { try await AnthropicClient(key: "k").models() }
    #expect(list?.message == AnthropicAPI.noWorkspace)
    #expect(AnthropicAPI().info.note.contains("platform.claude.com/settings/keys"))

    // The fallback field goes with its beta header. If Anthropic refuses them (the beta renamed or ended), the request
    // is asked again without them, thinking and length as before, even when the refusal names max_tokens: the
    // old-model retry would cut both.
    // (Here, not in a test of its own: tests run in parallel and the stub is one for all.)
    AnthropicStub.asked = []
    AnthropicStub.replies = [
        (400, #"{"type":"error","error":{"type":"invalid_request_error","message":"fallbacks: max_tokens 32000 is not valid for claude-opus-4-8"}}"#),
        (200, [#"data: {"type":"message_start","message":{"usage":{"input_tokens":30,"output_tokens":1}}}"#,
               #"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#,
               #"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}"#,
               #"data: {"type":"message_stop"}"#].joined(separator: "\n\n") + "\n\n"),
    ]
    let retried = try await AnthropicClient(key: "k").complete(LLMRequest(model: "claude-opus-5-5", messages: [LLMMessage(.user, "x")], thinking: true))
    #expect(retried == "ok" && AnthropicStub.asked.count == 2)
    let (first, second) = (AnthropicStub.asked[0], AnthropicStub.asked[1])
    #expect(first.body["fallbacks"] as? String == "default" && first.headers["anthropic-beta"] == "server-side-fallback-2026-07-01")
    #expect(second.body["fallbacks"] == nil && second.headers["anthropic-beta"] == nil)
    #expect((second.body["thinking"] as? [String: String]) == ["type": "adaptive"] && second.body["max_tokens"] as? Int == 32_000)

    // A model without a fallback sends neither.
    AnthropicStub.asked = []
    AnthropicStub.replies = [(500, "overloaded")]
    _ = try? await AnthropicClient(key: "k").complete(LLMRequest(model: "claude-haiku-5-5", messages: [LLMMessage(.user, "x")]))
    #expect(AnthropicStub.asked.count == 1 && AnthropicStub.asked[0].headers["anthropic-beta"] == nil && AnthropicStub.asked[0].body["fallbacks"] == nil)
}

/// The starter lists before a sign-in carry the newest models, by the ids their services use (OpenRouter's list and the
/// providers' docs, 2026-11-05).
@Test func starterListsCarryTheNewestModels() throws {
    func models(_ id: String) -> [String] { LLMCatalog.provider(id: id)?.info.models ?? [] }
    #expect(models("anthropic").contains("claude-haiku-5-5") && models("claude-code").contains("claude-haiku-5-5"))
    #expect(!(models("anthropic") + models("claude-code")).contains("claude-haiku-4-5"))
    #expect(models("openai").contains("gpt-6.1-terra") && models("chatgpt").contains("gpt-6.1-terra"))
    #expect(models("gemini").contains("gemini-4-argon"))
    #expect(Set(["openai/gpt-6.1-terra", "google/gemini-4-argon", "anthropic/claude-haiku-5.5"]).isSubset(of: models("openrouter")))
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
    // Past the window, like the chat's router brief: the model on this Mac holds what the framework says (4,096 on 27.0).
    let brief = String(repeating: "Answer as the wallet's agent. ", count: 1000)
    let error = await #expect(throws: LLMError.self) {
        try await AppleIntelligenceClient().complete(LLMRequest(model: info.defaultModel, messages: [LLMMessage(.system, brief), LLMMessage(.user, "hi")]))
    }
    // Said as what it is: too small for the chat, not «send it again».
    #expect(error?.message.contains("\(AppleIntelligence.window) tokens") == true)
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
    #expect(ClaudeCodeClient.arguments(LLMRequest(model: "m", messages: [])).contains("--include-partial-messages"))
}

/// Claude Code says before its result whether the answer went past the plan's limits onto usage credits: the
/// rate_limit_event of a real Fable 5.1 run on a Max plan (2026-11-05), which spends the plan's limit, not credits.
/// A short request (`complete`) says it too; a run without the event (an older Claude Code) says nothing.
@Test(.timeLimit(.minutes(1))) func claudeCodeSaysWhenAnAnswerIsPaidFromCredits() async throws {
    let max = #"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","rateLimitType":"seven_day","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"seven_day":{"utilization":0.15},"seven_day_overage_included":{"utilization":0.03}}}}"#
    final class Heard: @unchecked Sendable { let lock = NSLock(); var usage: LLMUsage? }
    func usage(_ event: String?, streaming: Bool = true) async throws -> LLMUsage? {
        let script = FileManager.default.temporaryDirectory.appendingPathComponent("fake-claude-\(UUID().uuidString).sh")
        defer { try? FileManager.default.removeItem(at: script) }
        try """
        #!/bin/sh
        cat > /dev/null
        echo '{"type":"system","subtype":"init"}'
        \(event.map { "echo '\($0)'" } ?? "")
        echo '{"type":"result","is_error":false,"result":"ok","total_cost_usd":0.013,"usage":{"input_tokens":5,"output_tokens":4}}'
        """.write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
        let heard = Heard()
        let client = MeteredClient(ClaudeCodeClient(path: script.path)) { usage, _ in heard.lock.withLock { heard.usage = usage } }
        let r = LLMRequest(model: "claude-fable-5-1", messages: [LLMMessage(.user, "x")])
        #expect(try await (streaming ? client.stream(r) { _ in } : client.complete(r)) == "ok")
        return heard.lock.withLock { heard.usage }
    }
    #expect(try await usage(max)?.overage == false)
    let credits = max.replacingOccurrences(of: #""isUsingOverage":false"#, with: #""isUsingOverage":true"#)
    #expect(try await usage(credits)?.overage == true)
    // Said twice in one run (a helper model's request): credits if either answer took them.
    #expect(try await usage(credits + "'\necho '" + max)?.overage == true)
    let older = try #require(try await usage(nil))
    #expect(older.overage == nil && older.cost == 0.013)
    #expect(try await usage(max, streaming: false)?.overage == false)
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
    #expect(LLMUsage(reported: anthropic) == LLMUsage(input: 10_621, cached: 10_000, written: 100, output: 36))
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
    let list = #"{"data":[{"id":"anthropic/claude-opus-5.5","context_length":1000000,"pricing":{"prompt":"0.000004","completion":"0.00002","input_cache_read":"0.0000002","input_cache_write":"0.000005"}},{"id":"deepseek/deepseek-v4.1-flash:free","context_length":1048576,"pricing":{"prompt":"0","completion":"0"}},{"id":"~deepseek/deepseek-flash-latest","context_length":1048576,"pricing":{"prompt":"0.00000015","completion":"0.0000006"}},{"id":"no-window"}]}"#
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
    // The first request writes the instructions to the cache at 1.25× (18,000 tokens), the next reads them at a twentieth.
    #expect(abs(opus.cost(LLMUsage(input: 18_500, written: 18_000, output: 100)) - 0.094) < 1e-12)
    // Haiku 5.5 as OpenRouter lists it (2026-11-05): 5× every token once the prompt passes 100k.
    let haikuRow = #"{"data":[{"id":"anthropic/claude-haiku-5.5","context_length":1000000,"pricing":{"prompt":"0.0000001","completion":"0.0000005","input_cache_read":"0.00000001","input_cache_write":"0.000000125","overrides":[{"min_prompt_tokens":100000,"prompt":"0.0000005","completion":"0.0000025","input_cache_read":"0.00000005","input_cache_write":"0.000000625"}]}}]}"#
    let haiku = try #require(ModelFacts.table(Data(haikuRow.utf8))[ModelFacts.key("claude-haiku-5-5")])
    #expect(haiku.longer.map(\.over) == [100_000])
    #expect(abs(haiku.cost(LLMUsage(input: 100_000, output: 1_000)) - 0.0105) < 1e-12)
    #expect(abs(haiku.cost(LLMUsage(input: 100_001, output: 1_000)) - 0.0525005) < 1e-12)
    // A tier that names only some prices keeps the base for the rest.
    let partial = #"{"data":[{"id":"x/m","context_length":10,"pricing":{"prompt":"1","completion":"2","overrides":[{"min_prompt_tokens":5,"prompt":"3"}]}}]}"#
    #expect(ModelFacts.table(Data(partial.utf8))[ModelFacts.key("m")]?.cost(LLMUsage(input: 6, output: 1)) == 20)
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

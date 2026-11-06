import Foundation
import LLMCore

/// Claude Pro/Max through the Claude Code the user already installed and signed in to, the way OpenClaw does it.
/// Anthropic's published terms do not allow third-party Claude.ai login, so this stays an opt-in gray-zone option.
public struct ClaudeCodeSubscription: LLMProvider {
    static let candidates = ["~/.local/bin/claude", "/opt/homebrew/bin/claude", "/usr/local/bin/claude", "~/.claude/local/claude"]

    public init() {}

    public var info: ProviderInfo {
        ProviderInfo(
            id: "claude-code", name: "Claude (subscription via Claude Code)", kind: .subscription, defaultModel: "claude-opus-5-5",
            note: "Runs the installed Claude Code with no tools. Anthropic doesn't officially allow third-party sign-in — a gray area.",
            existingLoginLabel: "Use the installed Claude Code",
            // All answer through `claude -p` (checked 2026-09-30).
            models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-haiku-4-5"]
        )
    }

    public func existingLogin() -> Credential? {
        let fromPATH = (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map { "\($0)/claude" }
        let found = (Self.candidates.map { NSString(string: $0).expandingTildeInPath } + fromPATH)
            .first { FileManager.default.isExecutableFile(atPath: $0) }
        return found.map { .external(path: $0) }
    }

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        guard case .external(let path) = credential else { throw LLMError("Claude Code isn't connected.") }
        return ClaudeCodeClient(path: path)
    }
}

struct ClaudeCodeClient: LLMClient {
    let path: String

    /// Headless, no tools, no MCP, no user settings or hooks, nothing saved to session history.
    /// Our system prompt always replaces Claude Code's own, which would answer as a coding agent.
    /// The prompt goes through stdin so it is not visible in the process list.
    /// `streaming`: one JSON event a line, the answer's words as they come (stream-json with partial messages), the result last.
    static func arguments(_ r: LLMRequest, streaming: Bool = false) -> [String] {
        ["-p", "--restricted", "--strict-mcp-config", "--tools", "", "--no-session-persistence"]
        + (streaming ? ["--output-format", "stream-json", "--verbose", "--include-partial-messages"] : ["--output-format", "json"])
        + ["--model", r.model,
         "--system-prompt", r.system.isEmpty ? "You are an assistant. Answer briefly and to the point." : r.system]
        + (r.thinking ? ["--effort", "high"] : [])
    }

    func complete(_ r: LLMRequest) async throws -> String {
        let prompt = r.dialogue.map(\.text).joined(separator: "\n\n")
        let output = try await Self.run(path, Self.arguments(r), input: prompt)
        LLMUsage.report(Self.usage(output))
        return try Self.result(output)
    }

    /// The same answer, its words told to `partial` as Claude Code writes them (text_delta events).
    func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let prompt = r.dialogue.map(\.text).joined(separator: "\n\n"), text = Accumulator()
        let output = try await Self.run(path, Self.arguments(r, streaming: true), input: prompt) { line in
            guard let j = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any], j["type"] as? String == "stream_event",
                  let event = j["event"] as? [String: Any], event["type"] as? String == "content_block_delta",
                  let delta = event["delta"] as? [String: Any], delta["type"] as? String == "text_delta", let words = delta["text"] as? String else { return }
            partial(text.add(words))
        }
        // The result is the last event; a run that wrote none (an old Claude Code) is read as one JSON answer.
        let last = output.split(separator: 0x0a).last { (try? JSONSerialization.jsonObject(with: Data($0)) as? [String: Any])?["type"] as? String == "result" }
        LLMUsage.report(Self.usage(last.map { Data($0) } ?? output))
        return try Self.result(last.map { Data($0) } ?? output)
    }

    /// What the run used, from its result object: the tokens of its one answer, the price Claude Code puts on them (a
    /// subscription isn't billed it) and the model's context window.
    static func usage(_ output: Data) -> LLMUsage? {
        guard let result = try? JSONSerialization.jsonObject(with: output) as? [String: Any],
              var usage = LLMUsage(reported: result["usage"] as? [String: Any]) else { return nil }
        usage.cost = (result["total_cost_usd"] as? NSNumber)?.doubleValue
        // A run may count a small helper model beside the one asked: the window is the one that read the most.
        let models = (result["modelUsage"] as? [String: Any])?.values.compactMap { $0 as? [String: Any] } ?? []
        func read(_ m: [String: Any]) -> Int {
            ["inputTokens", "cacheReadInputTokens", "cacheCreationInputTokens"].reduce(0) { $0 + ((m[$1] as? NSNumber)?.intValue ?? 0) }
        }
        usage.window = (models.max { read($0) < read($1) }?["contextWindow"] as? NSNumber)?.intValue
        return usage
    }

    /// The answer in Claude Code's result object, or why there's none.
    static func result(_ output: Data) throws -> String {
        struct Result: Decodable { let is_error: Bool?; let result: String? }
        guard let result = try? JSONDecoder().decode(Result.self, from: output) else {
            throw LLMError("Claude Code replied unexpectedly: \(String(decoding: output.prefix(200), as: UTF8.self))")
        }
        if result.is_error == true { throw LLMError("Claude Code: \(result.result ?? "error")") }
        return result.result ?? ""
    }

    /// A `claude -p` that hangs (a login prompt, a stalled network) would keep the chat busy for good: it is stopped after
    /// two minutes, the ChatGPT client's limit, or as soon as the task waiting for it is cancelled.
    /// `lines`: each line of the output as it comes, besides the whole at the end.
    static func run(_ path: String, _ arguments: [String], input: String, timeout: Duration = .seconds(120),
                    lines: (@Sendable (String) -> Void)? = nil) async throws -> Data {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = arguments
        process.currentDirectoryURL = FileManager.default.temporaryDirectory
        var env = ProcessInfo.processInfo.environment
        env["CLAUDECODE"] = nil
        env["CLAUDE_CODE_ENTRYPOINT"] = nil
        // A chat message never reinstalls Claude Code: while it reinstalls, every run fails («The file “claude” doesn't
        // exist», «unknown option '--restricted'») for a minute or two.
        env["DISABLE_AUTOUPDATER"] = "1"
        process.environment = env
        let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = stderr
        // The exit is signalled by the handler, set before the run: `waitUntilExit()` on a Dispatch thread sometimes never
        // woke after `claude` had exited (eight checks at once all hung on it), and the watchdog had nothing left to stop.
        let exited = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in exited.signal() }
        try process.run()
        let pid = process.processIdentifier
        // The waits and the watchdog run on Dispatch, never on Swift's few cooperative threads: several runs at once
        // (the tests) had their blocking reads hold every one of those threads, and a watchdog Task never got one to stop
        // the process.
        let timedOut = Flag()
        let watchdog = DispatchWorkItem { timedOut.set(); kill(pid, SIGTERM) }
        let (seconds, attoseconds) = timeout.components
        DispatchQueue.global().asyncAfter(deadline: .now() + .milliseconds(Int(seconds * 1000 + attoseconds / 1_000_000_000_000_000)), execute: watchdog)
        defer { watchdog.cancel() }
        // The prompt and stderr on their own, so a run that stops reading or fills stderr can't block the wait; a stopped
        // run gives the write EPIPE, not the app a SIGPIPE.
        let writer = stdin.fileHandleForWriting
        _ = fcntl(writer.fileDescriptor, F_SETNOSIGPIPE, 1)
        DispatchQueue.global().async { try? writer.write(contentsOf: Data(input.utf8)); try? writer.close() }
        async let errors = Self.readAll(stderr.fileHandleForReading)
        let data = await withTaskCancellationHandler {
            await Self.readAll(stdout.fileHandleForReading, lines: lines)
        } onCancel: { kill(pid, SIGTERM) }
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            DispatchQueue.global().async { exited.wait(); done.resume() }
        }
        watchdog.cancel()
        if timedOut.value {
            throw LLMError("Claude Code didn't answer in \(timeout.components.seconds) s, so it was stopped. Send it again, or pick another model.")
        }
        try Task.checkCancellation()
        if data.isEmpty, process.terminationStatus != 0 {
            let said = String(decoding: (await errors).suffix(300), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            throw LLMError("Claude Code stopped with code \(process.terminationStatus)" + (said.isEmpty ? "." : ": \(said)"))
        }
        return data
    }

    /// A pipe read to its end on a Dispatch thread; with `lines`, each line is told as it arrives.
    static func readAll(_ handle: FileHandle, lines: (@Sendable (String) -> Void)? = nil) async -> Data {
        await withCheckedContinuation { (done: CheckedContinuation<Data, Never>) in
            DispatchQueue.global().async {
                guard let lines else { done.resume(returning: (try? handle.readToEnd()) ?? Data()); return }
                var all = Data(), pending = Data()
                while case let chunk = handle.availableData, !chunk.isEmpty {
                    all.append(chunk)
                    pending.append(chunk)
                    while let end = pending.firstIndex(of: 0x0a) {
                        lines(String(decoding: pending[pending.startIndex..<end], as: UTF8.self))
                        pending.removeSubrange(pending.startIndex...end)
                    }
                }
                done.resume(returning: all)
            }
        }
    }
}

/// The answer's words so far, added to from the reading thread.
private final class Accumulator: @unchecked Sendable {
    private let lock = NSLock()
    private var text = ""
    func add(_ words: String) -> String { lock.withLock { text += words; return text } }
}

/// Whether the watchdog stopped the run, set on a Dispatch thread and read after the wait.
private final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var on = false
    func set() { lock.withLock { on = true } }
    var value: Bool { lock.withLock { on } }
}

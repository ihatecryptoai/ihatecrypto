import Foundation
import HelperAPI
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
            // All answer through `claude -p` (checked 2026-09-30). Haiku 5.5 needs Claude Code 2.1.293 or later. Fable isn't in a
            // Pro plan's limits, Claude bills it as usage credits; a Max plan spends up to half its weekly limit on it first.
            // Claude Code says which with each answer (LLMUsage.overage), and the context chip counts credits by it.
            models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-fable-5-1", "claude-haiku-5-5"]
        )
    }

    /// In the App Sandbox the app can't see `claude` or its login: the helper finds it, and runs it (`Helper`).
    public func existingLogin() -> Credential? {
        (Helper.sandboxed ? Helper.locateClaude() : Self.locate()).map { .external(path: $0) }
    }

    /// The installed `claude`: the usual places, then PATH.
    public static func locate() -> String? {
        let fromPATH = (ProcessInfo.processInfo.environment["PATH"] ?? "").split(separator: ":").map { "\($0)/claude" }
        return (candidates.map { NSString(string: $0).expandingTildeInPath } + fromPATH).first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        guard case .external(let path) = credential else { throw LLMError("Claude Code isn't connected.") }
        return ClaudeCodeClient(path: path)
    }
}

public struct ClaudeCodeClient: LLMClient {
    let path: String

    /// Headless, no tools, no MCP, no user settings or hooks, nothing saved to session history.
    /// Our system prompt always replaces Claude Code's own, which would answer as a coding agent.
    /// The prompt goes through stdin so it is not visible in the process list.
    /// One JSON event a line, the answer's words as they come (stream-json with partial messages), the result last: only
    /// these events say whether usage credits paid for the answer, so a short request is asked the same way.
    /// The helper builds the same from the values the sandboxed app sends (`Helper.askClaude`).
    static func arguments(_ r: LLMRequest) -> [String] { arguments(model: r.model, system: r.system, thinking: r.thinking) }

    /// A model's name and nothing else: the helper refuses any other value, which read as a flag
    /// (`--dangerously-skip-permissions`) could turn tools on.
    public static func isModelName(_ model: String) -> Bool { model.wholeMatch(of: /[A-Za-z0-9][A-Za-z0-9._\[\]-]{0,99}/) != nil }

    /// A system prompt that starts with «-» gets a space first, so it can only be read as the prompt.
    public static func arguments(model: String, system: String, thinking: Bool) -> [String] {
        ["-p", "--restricted", "--strict-mcp-config", "--tools", "", "--no-session-persistence",
         "--output-format", "stream-json", "--verbose", "--include-partial-messages"]
        + ["--model", model,
         "--system-prompt", system.isEmpty ? "You are an assistant. Answer briefly and to the point." : system.hasPrefix("-") ? " " + system : system]
        + (thinking ? ["--effort", "high"] : [])
    }

    public func complete(_ r: LLMRequest) async throws -> String {
        try await stream(r) { _ in }
    }

    /// The same answer, its words told to `partial` as Claude Code writes them (text_delta events). A sandboxed app has
    /// the helper run it.
    public func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let prompt = r.dialogue.map(\.text).joined(separator: "\n\n"), text = Accumulator()
        let line: @Sendable (String) -> Void = { line in
            guard let j = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any], j["type"] as? String == "stream_event",
                  let event = j["event"] as? [String: Any], event["type"] as? String == "content_block_delta",
                  let delta = event["delta"] as? [String: Any], delta["type"] as? String == "text_delta", let words = delta["text"] as? String else { return }
            partial(text.add(words))
        }
        let output = try await Helper.sandboxed
            ? Helper.askClaude(model: r.model, system: r.system, thinking: r.thinking, prompt: prompt, lines: line)
            : Self.run(path, Self.arguments(r), input: prompt, lines: line)
        // The result is the last event; a run that wrote none (an old Claude Code) is read as one JSON answer.
        let lines = output.split(separator: 0x0a)
        func last(_ type: String) -> Data? {
            lines.last { (try? JSONSerialization.jsonObject(with: Data($0)) as? [String: Any])?["type"] as? String == type }.map { Data($0) }
        }
        let result = last("result") ?? output
        var usage = Self.usage(result)
        // Whether usage credits paid for the answer, said just before the result. Not in Anthropic's docs: read from
        // Claude Code 2.1.318's output (2026-11-05). A run that says it more than once (a helper model's request too)
        // counts as credits if any one did; a run that doesn't say leaves it nil.
        let said = lines.compactMap { line -> Bool? in
            guard let event = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any], event["type"] as? String == "rate_limit_event" else { return nil }
            return (event["rate_limit_info"] as? [String: Any])?["isUsingOverage"] as? Bool
        }
        usage?.overage = said.isEmpty ? nil : said.contains(true)
        LLMUsage.report(usage)
        return try Self.result(result)
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
    public static func run(_ path: String, _ arguments: [String], input: String, timeout: Duration = .seconds(120),
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
        // Claude Code finds its login in the Keychain by USER («Not logged in» without it) and its settings under HOME:
        // set from the account if a launcher left them out.
        if let me = getpwuid(getuid())?.pointee {
            env["USER"] = env["USER"] ?? String(cString: me.pw_name)
            env["HOME"] = env["HOME"] ?? String(cString: me.pw_dir)
        }
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

/// Claude Code itself, for the Claude-subscription option: Anthropic's own installer (claude.ai/install.sh, served from
/// downloads.claude.ai; into ~/.local/bin, no admin rights), then its sign-in, which runs in a terminal. Checked 2026-10-06.
/// A sandboxed app has the helper do both (`Helper.installClaude`, `Helper.signInClaude`).
public enum ClaudeCodeInstaller {
    public static func install(step: @escaping @Sendable (String) async -> Void = { _ in }) async throws {
        await step("Downloading Claude Code's installer…")
        let (script, response) = try await URLSession.shared.download(from: URL(string: "https://claude.ai/install.sh")!)
        defer { try? FileManager.default.removeItem(at: script) }
        // Only Anthropic's: the redirect must stay on claude.ai's hosts.
        guard let host = response.url?.host, host == "claude.ai" || host.hasSuffix(".claude.ai") else {
            throw LLMError("The installer didn't come from claude.ai — nothing was installed.")
        }
        await step("Installing Claude Code…")
        try await withCheckedThrowingContinuation { (done: CheckedContinuation<Void, Error>) in
            let p = Process(), out = Pipe()
            p.executableURL = URL(fileURLWithPath: "/bin/bash")
            p.arguments = [script.path, "stable"]
            p.standardOutput = out
            p.standardError = out
            p.terminationHandler = { p in
                let said = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                if p.terminationStatus == 0 { done.resume() } else { done.resume(throwing: LLMError(said.split(separator: "\n").last.map(String.init) ?? "The installer failed.")) }
            }
            do { try p.run() } catch { done.resume(throwing: error) }
        }
    }

    /// Opens Terminal on `claude auth login`: the browser sign-in Anthropic's CLI runs itself.
    public static func signIn(_ path: String) throws {
        let command = FileManager.default.temporaryDirectory.appending(path: "claude-sign-in.command")
        try "#!/bin/zsh\n'\(path)' auth login\necho\necho 'Done — go back to ihatecrypto.'\n".write(to: command, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: command.path)
        try Process.run(URL(fileURLWithPath: "/usr/bin/open"), arguments: [command.path])
    }
}

import Foundation
import os

/// The helper: an XPC service inside ihatecrypto.app (Helper.xpc) that runs outside the App Sandbox, for the few things
/// the sandboxed app can't do itself. Each one is fixed in the service: the app passes values (a model name, the prompt),
/// never a program, a path or arguments, and the service takes only this copy of the app as a caller (its code
/// signature: bundle id and team, as the signer checks it).
/// - Claude Code: find the installed `claude`, ask it with no tools (`ClaudeCodeClient.arguments`), install it with
///   Anthropic's installer, open its sign-in in Terminal.
/// - Keep running: remove the launch agent a copy from before the sandbox put into ~/Library/LaunchAgents.
@objc public protocol HelperXPC {
    func locateClaude(reply: @escaping @Sendable (String?) -> Void)
    /// Claude Code's output, a JSON event a line; each line also goes to the caller's `HelperOutput` as it comes.
    func askClaude(model: String, system: String, thinking: Bool, prompt: String, reply: @escaping @Sendable (Data?, String?) -> Void)
    func installClaude(reply: @escaping @Sendable (String?) -> Void)
    func signInClaude(reply: @escaping @Sendable (String?) -> Void)
    /// True when there was one to remove.
    func removeOldKeepRunning(reply: @escaping @Sendable (Bool) -> Void)
}

/// The app's side of an answer: each line of Claude Code's output as it comes.
@objc public protocol HelperOutput {
    func line(_ text: String)
}

/// App side. Only a sandboxed copy uses the helper: `swift run`, the tests and the terminal tools do these things
/// themselves.
public enum Helper {
    /// This process runs in the App Sandbox (ihatecrypto.app and the copies build-app.sh makes).
    public static let sandboxed = ProcessInfo.processInfo.environment["APP_SANDBOX_CONTAINER_ID"] != nil
    /// The user's home folder: in the sandbox `NSHomeDirectory()` and `~` are the app's container.
    public static let home = URL(fileURLWithPath: getpwuid(getuid()).map { String(cString: $0.pointee.pw_dir) } ?? NSHomeDirectory())
    static let serviceName = (Bundle.main.bundleIdentifier ?? "app.ihatecrypto.mac") + ".helper"

    public struct Failure: LocalizedError {
        public let errorDescription: String?
        init(_ text: String) { errorDescription = text }
    }

    /// Where Claude Code is, asked again at most every 10 s: Settings asks on every redraw.
    public static func locateClaude() -> String? {
        if let hit = located.withLock({ $0.at.timeIntervalSinceNow > -10 ? $0 : nil }) { return hit.path }
        let c = Connection(nil)
        defer { c.connection.invalidate() }
        let found = OSAllocatedUnfairLock<String?>(initialState: nil)
        (c.connection.synchronousRemoteObjectProxyWithErrorHandler { _ in } as? HelperXPC)?.locateClaude { path in found.withLock { $0 = path } }
        let path = found.withLock { $0 }
        located.withLock { $0 = (Date(), path) }
        return path
    }
    private static let located = OSAllocatedUnfairLock<(at: Date, path: String?)>(initialState: (.distantPast, nil))

    public static func askClaude(model: String, system: String, thinking: Bool, prompt: String,
                                 lines: @escaping @Sendable (String) -> Void) async throws -> Data {
        try await call(Lines(lines)) { helper, done in
            helper.askClaude(model: model, system: system, thinking: thinking, prompt: prompt) { data, error in
                done(data.map { .success($0) } ?? .failure(Failure(error ?? "Claude Code didn't answer.")))
            }
        }
    }

    public static func installClaude() async throws {
        defer { located.withLock { $0 = (.distantPast, nil) } }
        try await call(nil) { helper, done in helper.installClaude { error in done(error.map { .failure(Failure($0)) } ?? .success(())) } }
    }

    public static func signInClaude() async throws {
        try await call(nil) { helper, done in helper.signInClaude { error in done(error.map { .failure(Failure($0)) } ?? .success(())) } }
    }

    public static func removeOldKeepRunning() async throws -> Bool {
        try await call(nil) { helper, done in helper.removeOldKeepRunning { done(.success($0)) } }
    }

    /// One connection a call: its one reply, or the helper's failure, ends it. Cancelling the task closes the connection,
    /// and the helper stops what it runs for it.
    private static func call<T: Sendable>(_ output: HelperOutput?,
                                          _ body: @escaping (HelperXPC, @escaping @Sendable (Result<T, Error>) -> Void) -> Void) async throws -> T {
        let c = Connection(output)
        defer { c.connection.invalidate() }
        do {
            return try await answer(c, body)
        } catch {
            // A cancelled call fails as cancelled, not as a stopped helper.
            try Task.checkCancellation()
            throw error
        }
    }

    private static func answer<T: Sendable>(_ c: Connection,
                                            _ body: @escaping (HelperXPC, @escaping @Sendable (Result<T, Error>) -> Void) -> Void) async throws -> T {
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<T, Error>) in
                let resumed = OSAllocatedUnfairLock(initialState: false)
                let done: @Sendable (Result<T, Error>) -> Void = { result in
                    if resumed.withLock({ was in defer { was = true }; return !was }) { continuation.resume(with: result) }
                }
                let proxy = c.connection.remoteObjectProxyWithErrorHandler { error in
                    done(.failure(Failure("ihatecrypto's helper stopped (\(error.localizedDescription)).")))
                }
                guard let helper = proxy as? HelperXPC else { done(.failure(Failure("ihatecrypto's helper isn't in this copy."))); return }
                body(helper, done)
            }
        } onCancel: { c.connection.invalidate() }
    }

    /// The connection, handed to the cancel handler.
    private final class Connection: @unchecked Sendable {
        let connection = NSXPCConnection(serviceName: Helper.serviceName)
        init(_ output: HelperOutput?) {
            connection.remoteObjectInterface = NSXPCInterface(with: HelperXPC.self)
            if let output {
                connection.exportedInterface = NSXPCInterface(with: HelperOutput.self)
                connection.exportedObject = output
            }
            connection.resume()
        }
    }

    private final class Lines: NSObject, HelperOutput, @unchecked Sendable {
        let told: @Sendable (String) -> Void
        init(_ told: @escaping @Sendable (String) -> Void) { self.told = told }
        func line(_ text: String) { told(text) }
    }
}

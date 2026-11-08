import Foundation
import HelperAPI
import LLMClaudeCode

/// Helper: XPC service inside ihatecrypto.app, outside the App Sandbox (HelperAPI.swift says what it does and why).
/// Nothing it runs comes from the app: the program is the `claude` it finds itself, with arguments built here, or a
/// fixed launchctl call and file of this copy's own.
final class ServiceDelegate: NSObject, NSXPCListenerDelegate {
    static let app = Bundle.main.bundleIdentifier.flatMap { $0.hasSuffix(".helper") ? String($0.dropLast(7)) : nil } ?? "app.ihatecrypto.mac"

    /// The Apple team that signed this helper, read from its own signature; nil when it's unsigned or signed ad hoc.
    static let team: String? = {
        var me: SecCode?, onDisk: SecStaticCode?, info: CFDictionary?
        guard SecCodeCopySelf([], &me) == errSecSuccess, let me, SecCodeCopyStaticCode(me, [], &onDisk) == errSecSuccess, let onDisk,
              SecCodeCopySigningInformation(onDisk, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess
        else { return nil }
        return (info as? [String: Any])?[kSecCodeInfoTeamIdentifier as String] as? String
    }()

    func listener(_ listener: NSXPCListener, shouldAcceptNewConnection connection: NSXPCConnection) -> Bool {
        // Only the app this helper is built into, signed by the same team (app.ihatecrypto.mac for ….mac.helper, the
        // development copy for ….dev.helper); a helper without a team takes nobody.
        guard let team = Self.team else { return false }
        connection.setCodeSigningRequirement("identifier \"\(Self.app)\" and anchor apple generic and certificate leaf[subject.OU] = \"\(team)\"")
        connection.remoteObjectInterface = NSXPCInterface(with: HelperOutput.self)
        let service = HelperService(output: connection.remoteObjectProxy as? HelperOutput)
        connection.exportedInterface = NSXPCInterface(with: HelperXPC.self)
        connection.exportedObject = service
        // The app closed it (an answer cancelled, the app quit): what runs for it stops.
        connection.invalidationHandler = { service.stop() }
        connection.resume()
        return true
    }
}

final class HelperService: NSObject, HelperXPC, @unchecked Sendable {
    private let output: HelperOutput?
    private let lock = NSLock()
    private var running: Task<Void, Never>?

    init(output: HelperOutput?) { self.output = output }

    func stop() { lock.withLock { running?.cancel() } }

    func locateClaude(reply: @escaping @Sendable (String?) -> Void) { reply(ClaudeCodeSubscription.locate()) }

    func askClaude(model: String, system: String, thinking: Bool, prompt: String, reply: @escaping @Sendable (Data?, String?) -> Void) {
        guard ClaudeCodeClient.isModelName(model) else { reply(nil, "Not a model name: \(model.prefix(40))"); return }
        guard let path = ClaudeCodeSubscription.locate() else { reply(nil, "Claude Code isn't installed on this Mac."); return }
        let arguments = ClaudeCodeClient.arguments(model: model, system: system, thinking: thinking), output = Output(output)
        let task = Task {
            do { reply(try await ClaudeCodeClient.run(path, arguments, input: prompt, lines: { output.proxy?.line($0) }), nil) }
            catch { reply(nil, error.localizedDescription) }
        }
        lock.withLock { running = task }
    }

    func installClaude(reply: @escaping @Sendable (String?) -> Void) {
        Task {
            do { try await ClaudeCodeInstaller.install(); reply(nil) } catch { reply(error.localizedDescription) }
        }
    }

    func signInClaude(reply: @escaping @Sendable (String?) -> Void) {
        guard let path = ClaudeCodeSubscription.locate() else { reply("Claude Code isn't installed on this Mac."); return }
        do { try ClaudeCodeInstaller.signIn(path); reply(nil) } catch { reply(error.localizedDescription) }
    }

    /// Keep running before the sandbox: ~/Library/LaunchAgents/<this copy's id>.keeprunning.plist, loaded with launchctl.
    /// The sandboxed app registers its own agent (SMAppService) in its place.
    func removeOldKeepRunning(reply: @escaping @Sendable (Bool) -> Void) {
        let label = ServiceDelegate.app + ".keeprunning"
        let plist = FileManager.default.homeDirectoryForCurrentUser.appending(path: "Library/LaunchAgents/\(label).plist")
        // Unloaded only once its file is gone: a plist that can't be removed keeps working as it did.
        guard (try? FileManager.default.removeItem(at: plist)) != nil else { reply(false); return }
        if let unload = try? Process.run(URL(fileURLWithPath: "/bin/launchctl"), arguments: ["bootout", "gui/\(getuid())/\(label)"]) {
            unload.waitUntilExit()
        }
        reply(true)
    }

    /// The app's end of the connection, for the reading thread.
    private final class Output: @unchecked Sendable {
        let proxy: HelperOutput?
        init(_ proxy: HelperOutput?) { self.proxy = proxy }
    }
}

let delegate = ServiceDelegate()
let listener = NSXPCListener.service()
listener.delegate = delegate
listener.resume()

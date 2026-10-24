import Foundation
import IhatecryptoCore
import LLMCatalog
import LLMCore
import WalletCore

/// Settings › AI model, as the Mac's ModelSection and LocalModels.swift: «Think before answering», a model's Check (the
/// chat's exam) and its score, Claude Code installed and signed in from here, Ollama installed or updated from here and
/// a model set up in one go; and Settings › Background › Keep always running.
nonisolated struct LocalSetup: Equatable {
    /// The Check while it runs: how many of the exam's requests are done, for «provider/model».
    var examProgress: Int?
    var examFor: String?
    /// The last Check's or install's line, shown under the account.
    var status: String?
    var ollamaVersion: String?
    var ollamaLatest: String?
    /// What the Ollama or Claude Code install is doing; nil when none runs.
    var ollamaInstall: String?
    var claudeInstall: String?
    /// Ollama was just updated here: its newest is in, whatever GitHub said a moment ago.
    var ollamaUpdated = false
}

nonisolated struct LocalSetupView: Encodable {
    struct Score: Encodable { var passed: Int; var total: Int; var seconds: Double; var good: Bool; var short: String; var misses: [String] }
    var thinking: Bool
    var examProgress: Int?
    var examTotal: Int
    var examFor: String?
    var status: String?
    var scores: [String: Score]
    var ollamaVersion: String?
    var ollamaLatest: String?
    /// The Mac's localState: noOllama, notRunning, outdated, missing, ready.
    var ollamaState: String
    var ollamaInstall: String?
    var claudeInstall: String?
    /// Claude Code found on this PC: «Sign in in a terminal» runs its own sign-in.
    var claudeFound: Bool
    /// As Windows has it; nil until the app has said.
    var keepRunning: Bool?
}

extension Engine {
    var localSetupView: LocalSetupView {
        let scores = (settings.modelScores ?? [:]).mapValues { r in
            LocalSetupView.Score(passed: r.passed, total: r.total, seconds: r.seconds, good: r.good, short: r.short, misses: r.misses)
        }
        return LocalSetupView(thinking: settings.thinking == true, examProgress: localSetup.examProgress, examTotal: ModelExam.questions.count,
                              examFor: localSetup.examFor, status: localSetup.status, scores: scores, ollamaVersion: localSetup.ollamaVersion,
                              ollamaLatest: localSetup.ollamaLatest, ollamaState: ollamaState, ollamaInstall: localSetup.ollamaInstall,
                              claudeInstall: localSetup.claudeInstall, claudeFound: provider("claude-code")?.existingLogin() != nil,
                              keepRunning: settings.keepRunning)
    }

    func localOp(_ r: Request) async throws -> [String: String] {
        switch r.op {
        case "thinking": settings.thinking = r.test == true
        case "checkModel": Task { await checkModel(r.provider ?? providerID, model: r.model) }
        case "installClaudeCode": Task { await installClaudeCode() }
        case "claudeSignIn":
            guard case .external(let path)? = provider("claude-code")?.existingLogin() else { throw WalletError(say("Install Claude Code first.", "Сначала установи Claude Code.")) }
            try ClaudeCodeSetup.signIn(path)
        case "installOllama": Task { await installOllama() }
        case "ollamaStatus": await readOllamaVersions()
        case "setUpLocal": Task { await setUpLocal() }
        // The wish goes to the app, which keeps Windows' scheduled task and says back how Windows has it.
        case "keepRunning": out.event("keepRunning", ["on": r.test == true ? "1" : "0"])
        case "keepRunningState": settings.keepRunning = r.test == true
        default: break
        }
        return [:]
    }

    // MARK: Check

    /// The chat's exam: the requests people send, read by this account's model; 8 of 10 or more is good for the chat
    /// (the Mac's checkModel). It spends a few of the account's requests, and only when Check is pressed.
    func checkModel(_ id: String, model asked: String?) async {
        guard localSetup.examProgress == nil else { return }
        let name = provider(id)?.info.name ?? id
        guard let client = llmClient(id) else {
            localSetup.status = say("Connect \(name) first.", "Сначала подключи \(name).")
            changed()
            return
        }
        let model = asked.flatMap { $0.isEmpty ? nil : $0 } ?? (id == providerID ? self.model : provider(id)?.info.defaultModel ?? "")
        localSetup.examProgress = 0
        localSetup.examFor = "\(id)/\(model)"
        localSetup.status = nil
        changed()
        let result = await ModelExam.run(client: client, model: model) { n in await MainActor.run { self.localSetup.examProgress = n; self.changed() } }
        var scores = settings.modelScores ?? [:]
        scores["\(id)/\(model)"] = result
        settings.modelScores = scores
        localSetup.examProgress = nil
        localSetup.examFor = nil
        localSetup.status = Self.examLine(result, model: LLMCatalog.displayName(model: model), say: { say($0, $1) })
        changed()
    }

    nonisolated static func examLine(_ r: ModelExam.Result, model: String, say: (String, String) -> String) -> String {
        let seconds = String(format: "%.1f", r.seconds)
        return r.good
            ? say("\(model) understood \(r.passed) of \(r.total) requests, \(seconds) s a reply — good for the chat.",
                  "\(model) понял \(r.passed) из \(r.total) запросов, \(seconds) с на ответ — для чата подходит.")
            : say("\(model) understood only \(r.passed) of \(r.total) — it will misread requests. A bigger model or a subscription reads better. Missed: ",
                  "\(model) понял только \(r.passed) из \(r.total) — он будет путать запросы. Модель побольше или подписка читают лучше. Не понял: ")
              + r.misses.prefix(3).joined(separator: "; ")
    }

    // MARK: Claude Code

    /// Anthropic's own Windows installer, then the account connects to it; its sign-in runs in a terminal.
    func installClaudeCode() async {
        guard localSetup.claudeInstall == nil else { return }
        localSetup.claudeInstall = "…"
        changed()
        defer { localSetup.claudeInstall = nil; changed() }
        do {
            try await ClaudeCodeSetup.install { [weak self] text in self?.localSetup.claudeInstall = text; self?.changed() }
            reply(say("Claude Code is installed. Sign in to your Claude account with «Sign in in a terminal» in Settings › AI model.",
                      "Claude Code установлен. Войди в аккаунт Claude кнопкой «Войти в терминале» в Настройках › Модель ИИ."))
        } catch {
            reply(say("Claude Code wasn't installed: ", "Claude Code не установился: ") + error.localizedDescription)
        }
    }

    // MARK: Ollama

    /// Its version here (when it runs) and its newest release, for «Update» and the outdated state.
    func readOllamaVersions() async {
        async let latest = OllamaSetup.latest()
        localSetup.ollamaVersion = await OllamaSetup.version()
        localSetup.ollamaLatest = await latest
    }

    /// Older than its newest release (and not just updated), or a model said it's too old.
    var needsNewOllama: Bool {
        guard !localSetup.ollamaUpdated, let version = localSetup.ollamaVersion, let latest = localSetup.ollamaLatest else {
            return localServers["ollama"] == false && !Self.ollamaInstalled
        }
        return OllamaSetup.older(version, than: latest)
    }

    var ollamaState: String {
        if localServers["ollama"] == false { return Self.ollamaInstalled ? "notRunning" : "noOllama" }
        let missing = liveModels["ollama"].map { $0.contains(model) } == false
        if missing && needsNewOllama { return "outdated" }
        return missing || providerID != "ollama" ? "missing" : "ready"
    }

    /// Ollama's newest release, checked by its signature before it runs, installed for this user; the chat says how it went.
    func installOllama() async {
        guard localSetup.ollamaInstall == nil else { return }
        localSetup.ollamaInstall = "…"
        changed()
        defer { localSetup.ollamaInstall = nil; changed() }
        do {
            try await OllamaSetup.install { [weak self] text in self?.localSetup.ollamaInstall = text; self?.changed() }
            localSetup.ollamaUpdated = true
            // The installer starts its app; the server takes a moment to answer after that.
            for _ in 0..<10 where localServers["ollama"] != true {
                try? await Task.sleep(for: .seconds(2))
                await refreshModels()
            }
            await readOllamaVersions()
            reply(say("Ollama \(localSetup.ollamaVersion ?? "") is ready. Download a model in Settings › AI model, or pick one in the model menu.",
                      "Ollama \(localSetup.ollamaVersion ?? "") готова. Скачай модель в Настройках › Модель ИИ или выбери в меню моделей."))
        } catch {
            reply(say("Ollama wasn't installed: ", "Ollama не установилась: ") + error.localizedDescription)
        }
    }

    /// «Set up on this PC» in one go: Ollama installed or updated and started, then the chosen model — or one this PC has,
    /// else the biggest suggested one that fits its memory — downloaded and picked (the Mac's setUpLocal).
    func setUpLocal() async {
        await refreshModels()
        await readOllamaVersions()
        if localServers["ollama"] != true || needsNewOllama { await installOllama() }
        guard localServers["ollama"] == true else { return }
        let have = (liveModels["ollama"] ?? []).filter { LLMCatalog.chatRefusal(model: $0) == nil }
        let current = providerID == "ollama" ? model : nil
        if let pick = current.flatMap({ have.contains($0) ? $0 : nil }) ?? have.first {
            try? await connect(provider: "ollama", key: nil, model: pick)
            return
        }
        guard let pick = localPick ?? provider("ollama")?.info.models.last else { return }
        await downloadModel(pick)
        if liveModels["ollama"]?.contains(pick) == true { try? await connect(provider: "ollama", key: nil, model: pick) }
    }
}

// MARK: The installers (Windows; elsewhere they say so)

nonisolated enum OllamaSetup {
    static func older(_ a: String, than b: String) -> Bool { a.compare(b, options: .numeric) == .orderedAscending }

    /// The newest release («0.35.1»), from GitHub.
    static func latest() async -> String? {
        guard let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: URL(string: "https://api.github.com/repos/ollama/ollama/releases/latest")!, timeoutInterval: 10)),
              let tag = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["tag_name"] as? String else { return nil }
        return tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
    }

    /// The running server's version; nil when it doesn't answer (nothing is started to ask).
    static func version() async -> String? {
        guard let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: URL(string: "\(Engine.ollama)/api/version")!, timeoutInterval: 3)) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["version"] as? String
    }

    /// Get-AuthenticodeSignature's «Status|Subject»: a valid signature whose signer is Ollama's organization. Anything else
    /// is refused before the installer runs, as the Mac checks its Developer ID team.
    static func signedByOllama(_ line: String) -> Bool {
        let parts = line.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: "|", maxSplits: 1).map(String.init)
        guard parts.count == 2, parts[0] == "Valid" else { return false }
        let fields = parts[1].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }
        return fields.contains { f in
            guard f.hasPrefix("O=") else { return false }
            let org = f.dropFirst(2).trimmingCharacters(in: CharacterSet(charactersIn: "\" "))
            return org == "Ollama" || org == "Ollama Inc" || org == "Ollama Inc."
        }
    }

    static func install(step: @escaping @MainActor (String) -> Void) async throws {
        #if os(Windows)
        await step("Downloading Ollama…")
        let (download, _) = try await URLSession.shared.download(from: URL(string: "https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe")!)
        let setup = FileManager.default.temporaryDirectory.appendingPathComponent("OllamaSetup-\(UUID().uuidString).exe")
        try FileManager.default.moveItem(at: download, to: setup)
        defer { try? FileManager.default.removeItem(at: setup) }
        await step("Checking Ollama's signature…")
        let signature = try await Shell.powershell("$s = Get-AuthenticodeSignature -LiteralPath '\(setup.path.replacingOccurrences(of: "'", with: "''"))'; $s.Status.ToString() + '|' + $s.SignerCertificate.Subject")
        guard signedByOllama(signature) else { throw LLMError("The downloaded Ollama isn't signed by Ollama — nothing was installed.") }
        await step("Installing Ollama…")
        // Its own Inno Setup, for this user (no administrator), with no window of its own.
        try await Shell.run(setup.path, ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        #else
        throw LLMError("Ollama installs from the Windows app.")
        #endif
    }
}

/// Claude Code for the Claude-subscription option: Anthropic's own installer (claude.ai/install.ps1, into
/// %USERPROFILE%\.local\bin, no administrator), then its sign-in, which runs in a terminal (the Mac's ClaudeCodeInstaller).
nonisolated enum ClaudeCodeSetup {
    /// Only Anthropic's: the download must stay on claude.ai's hosts after its redirects.
    static func fromClaude(_ host: String?) -> Bool { host == "claude.ai" || host?.hasSuffix(".claude.ai") == true }

    static func install(step: @escaping @MainActor (String) -> Void) async throws {
        #if os(Windows)
        await step("Downloading Claude Code's installer…")
        let (download, response) = try await URLSession.shared.download(from: URL(string: "https://claude.ai/install.ps1")!)
        guard fromClaude(response.url?.host) else {
            try? FileManager.default.removeItem(at: download)
            throw LLMError("The installer didn't come from claude.ai — nothing was installed.")
        }
        // PowerShell runs a file only with its extension.
        let script = FileManager.default.temporaryDirectory.appendingPathComponent("claude-install-\(UUID().uuidString).ps1")
        try FileManager.default.moveItem(at: download, to: script)
        defer { try? FileManager.default.removeItem(at: script) }
        await step("Installing Claude Code…")
        try await Shell.run(Shell.powershellPath, ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script.path])
        #else
        throw LLMError("Claude Code installs from the Windows app here.")
        #endif
    }

    /// A terminal on `claude auth login`: the browser sign-in Anthropic's CLI runs itself.
    static func signIn(_ path: String) throws {
        #if os(Windows)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: (ProcessInfo.processInfo.environment["SystemRoot"] ?? "C:\\Windows") + "\\System32\\cmd.exe")
        p.arguments = ["/c", "start", "Claude Code", "cmd", "/k", path, "auth", "login"]
        try p.run()
        #else
        throw LLMError("Claude Code signs in from the Windows app here.")
        #endif
    }
}

nonisolated enum Shell {
    static var powershellPath: String { (ProcessInfo.processInfo.environment["SystemRoot"] ?? "C:\\Windows") + "\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" }

    static func powershell(_ command: String) async throws -> String {
        try await run(powershellPath, ["-NoProfile", "-NonInteractive", "-Command", command])
    }

    /// A tool to its end: what it printed, or its last line as the error.
    @discardableResult
    static func run(_ tool: String, _ args: [String]) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            let p = Process(), out = Pipe()
            p.executableURL = URL(fileURLWithPath: tool)
            p.arguments = args
            p.standardOutput = out
            p.standardError = out
            p.terminationHandler = { p in
                let text = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                if p.terminationStatus == 0 { continuation.resume(returning: text) }
                else { continuation.resume(throwing: LLMError(text.split(separator: "\n").last.map(String.init) ?? "\(tool) failed")) }
            }
            do { try p.run() } catch { continuation.resume(throwing: error) }
        }
    }
}

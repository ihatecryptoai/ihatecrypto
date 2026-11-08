import AppKit
import Foundation
import HelperAPI
import IhatecryptoCore
import LLMClaudeCode
import Security
import SwiftUI
import LLMCatalog
import LLMCore

/// Models that run on this Mac through Ollama: whether Ollama is there, how big a model is (its registry manifest),
/// whether it fits this Mac's memory, and downloading one with Ollama's own /api/pull, with its progress.
enum OllamaModels {
    nonisolated static let base = "http://localhost:11434"

    /// Ollama's app, or its command-line server (Homebrew).
    static var installed: Bool {
        ["/Applications/Ollama.app", Helper.home.path + "/Applications/Ollama.app", "/opt/homebrew/bin/ollama", "/usr/local/bin/ollama"].contains { FileManager.default.fileExists(atPath: $0) }
    }

    static var memoryGB: Double { Double(ProcessInfo.processInfo.physicalMemory) / 1_073_741_824 }

    /// A model fits when it and its working memory take at most ~70% of the Mac's: the rest is macOS and the app.
    /// Sizes are decimal GB (the registry's bytes / 10⁹) and memory is GiB: a 32 GB Mac has 34.4 of them, which
    /// gemma4:26b (18.7 GB) fits — it missed by 0.08 when the two were compared as one unit.
    static func fits(sizeGB: Double) -> Bool { sizeGB * 1.2 <= memoryGB * 1.073_741_824 * 0.7 }

    /// The download size, GB, from the registry's manifest («gemma4:12b» → 8.0); nil when the registry doesn't know it.
    static func sizeGB(_ model: String) async -> Double? {
        let parts = model.split(separator: ":", maxSplits: 1).map(String.init)
        let name = parts[0].contains("/") ? parts[0] : "library/\(parts[0])"
        guard let url = URL(string: "https://registry.ollama.ai/v2/\(name)/manifests/\(parts.count > 1 ? parts[1] : "latest")") else { return nil }
        var request = URLRequest(url: url, timeoutInterval: 10)
        request.setValue("application/vnd.docker.distribution.manifest.v2+json", forHTTPHeaderField: "Accept")
        guard let (data, response) = try? await URLSession.shared.data(for: request), (response as? HTTPURLResponse)?.statusCode == 200,
              let layers = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["layers"] as? [[String: Any]] else { return nil }
        return layers.reduce(0) { $0 + ((($1["size"] as? NSNumber)?.doubleValue) ?? 0) } / 1_000_000_000
    }

    static func version() async -> String? {
        guard let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: URL(string: "\(base)/api/version")!, timeoutInterval: 3)) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["version"] as? String
    }

    /// Starts an Ollama that's installed but not running: Homebrew's service, or the app.
    static func start() {
        if OllamaInstaller.viaBrew, let brew = OllamaInstaller.brew {
            Task { try? await OllamaInstaller.run(brew, ["services", "start", "ollama"]) }
        } else {
            let app = ["/Applications/Ollama.app", Helper.home.path + "/Applications/Ollama.app"].first { FileManager.default.fileExists(atPath: $0) }
            if let app { NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: app), configuration: .init()) }
        }
    }

    /// The downloaded models and their sizes, GB, from Ollama's /api/tags; nil when Ollama doesn't answer.
    static func downloaded() async -> [String: Double]? {
        guard let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: URL(string: "\(base)/api/tags")!, timeoutInterval: 3)),
              let models = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["models"] as? [[String: Any]] else { return nil }
        return Dictionary(models.compactMap { m in (m["name"] as? String).map { ($0, ((m["size"] as? NSNumber)?.doubleValue ?? 0) / 1_000_000_000) } },
                          uniquingKeysWith: { a, _ in a })
    }

    /// Loads `model` into memory now, so the first answer doesn't wait for it; kept 30 minutes after the last use.
    static func preload(_ model: String) {
        var request = URLRequest(url: URL(string: "\(base)/api/generate")!, timeoutInterval: 300)
        request.httpMethod = "POST"
        request.httpBody = try? JSONSerialization.data(withJSONObject: ["model": model, "keep_alive": "30m"])
        Task.detached { _ = try? await URLSession.shared.data(for: request) }
    }

    /// Pulls `model`, reporting 0…1 over all its layers as Ollama streams them; throws Ollama's own error.
    /// Off the main thread: a download is thousands of JSON lines over minutes, and the bar moves only by whole percents.
    nonisolated static func pull(_ model: String, progress: @escaping @Sendable @MainActor (Double) -> Void) async throws {
        var request = URLRequest(url: URL(string: "\(base)/api/pull")!, timeoutInterval: 3600)
        request.httpMethod = "POST"
        request.httpBody = try JSONSerialization.data(withJSONObject: ["model": model, "stream": true])
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw LLMError("Ollama didn't start the download (HTTP \((response as? HTTPURLResponse)?.statusCode ?? 0)).") }
        var layers: [String: (done: Double, total: Double)] = [:]
        var shown = -1.0
        for try await line in bytes.lines {
            guard let j = try? JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any] else { continue }
            if let error = j["error"] as? String { throw LLMError(error) }
            if let digest = j["digest"] as? String, let total = (j["total"] as? NSNumber)?.doubleValue, total > 0 {
                layers[digest] = ((j["completed"] as? NSNumber)?.doubleValue ?? 0, total)
                let all = layers.values.reduce((0.0, 0.0)) { ($0.0 + $1.done, $0.1 + $1.total) }
                let p = min(all.0 / all.1, 1)
                if abs(p - shown) >= 0.01 { shown = p; await progress(p) }
            }
            if j["status"] as? String == "success" { return }
        }
        throw LLMError("Ollama stopped before the download finished.")
    }
}

/// Ollama itself, always to its newest release: Homebrew's copy through brew, otherwise the app from Ollama's GitHub
/// release, checked to be signed by Ollama's developer ID and notarized by Apple before it replaces anything. A sandboxed
/// copy can neither run brew nor write to Applications: it puts the checked app in Downloads and shows it in Finder, for
/// the person to drag into Applications and open.
enum OllamaInstaller {
    /// Ollama Inc.'s Developer ID team, read off the release's signature.
    static let teamID = "3MU9H2V9Y9"
    static let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].first { FileManager.default.isExecutableFile(atPath: $0) }
    static var viaBrew: Bool { ["/opt/homebrew/Cellar/ollama", "/usr/local/Cellar/ollama"].contains { FileManager.default.fileExists(atPath: $0) } }

    /// The newest release («0.35.1»), from GitHub.
    static func latest() async -> String? {
        guard let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: URL(string: "https://api.github.com/repos/ollama/ollama/releases/latest")!, timeoutInterval: 10)),
              let tag = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["tag_name"] as? String else { return nil }
        return tag.hasPrefix("v") ? String(tag.dropFirst()) : tag
    }

    static func older(_ a: String, than b: String) -> Bool { a.compare(b, options: .numeric) == .orderedAscending }

    /// Installs or updates Ollama and starts it; `step` says what it's doing. False: it's in Downloads, shown in Finder,
    /// waiting to be dragged into Applications (the sandbox).
    static func install(step: @escaping @MainActor (String) -> Void) async throws -> Bool {
        if viaBrew, let brew, !Helper.sandboxed {
            await step("Updating Ollama with Homebrew…")
            try await run(brew, ["upgrade", "ollama"])
            await step("Starting Ollama…")
            try await run(brew, ["services", "restart", "ollama"])
            return true
        }
        await step("Downloading Ollama (≈ 200 MB)…")
        let (zip, _) = try await URLSession.shared.download(from: URL(string: "https://github.com/ollama/ollama/releases/latest/download/Ollama-darwin.zip")!)
        let folder = FileManager.default.temporaryDirectory.appending(path: "ollama-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder); try? FileManager.default.removeItem(at: zip) }
        try await run("/usr/bin/ditto", ["-x", "-k", zip.path, folder.path])
        let app = folder.appending(path: "Ollama.app")
        await step("Checking Ollama's signature…")
        try verify(app)
        if Helper.sandboxed {
            let target = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0].resolvingSymlinksInPath().appending(path: "Ollama.app")
            if FileManager.default.fileExists(atPath: target.path) { _ = try FileManager.default.replaceItemAt(target, withItemAt: app) }
            else { try FileManager.default.moveItem(at: app, to: target) }
            NSWorkspace.shared.activateFileViewerSelecting([target])
            return false
        }
        // The running copy quits first: it can't be replaced while it runs.
        for running in NSRunningApplication.runningApplications(withBundleIdentifier: bundleID(app) ?? "") { running.terminate() }
        try? await Task.sleep(for: .seconds(2))
        let system = URL(fileURLWithPath: "/Applications")
        let target = (FileManager.default.isWritableFile(atPath: system.path) ? system
                      : FileManager.default.homeDirectoryForCurrentUser.appending(path: "Applications")).appending(path: "Ollama.app")
        try? FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        if FileManager.default.fileExists(atPath: target.path) { _ = try FileManager.default.replaceItemAt(target, withItemAt: app) }
        else { try FileManager.default.moveItem(at: app, to: target) }
        await step("Starting Ollama…")
        _ = try await NSWorkspace.shared.openApplication(at: target, configuration: .init())
        return true
    }

    static func bundleID(_ app: URL) -> String? { Bundle(url: app)?.bundleIdentifier }

    /// Signed by Ollama's team with a Developer ID and notarized: anything else is refused before it's installed.
    static func verify(_ app: URL) throws {
        var code: SecStaticCode?, requirement: SecRequirement?
        guard SecStaticCodeCreateWithPath(app as CFURL, [], &code) == errSecSuccess, let code,
              SecRequirementCreateWithString("anchor apple generic and certificate leaf[subject.OU] = \"\(teamID)\" and notarized" as CFString, [], &requirement) == errSecSuccess,
              let requirement,
              SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSCheckAllArchitectures | kSecCSStrictValidate), requirement) == errSecSuccess else {
            throw LLMError("The downloaded Ollama isn't signed by Ollama — nothing was installed.")
        }
    }

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

extension Workspace {
    /// Installs Claude Code for the Claude-subscription option, then connects to it; the chat says how it went.
    func installClaudeCode() async {
        guard claudeCodeInstall == nil else { return }
        claudeCodeInstall = "…"
        defer { claudeCodeInstall = nil }
        do {
            if Helper.sandboxed {
                claudeCodeInstall = "Installing Claude Code…"
                try await Helper.installClaude()
            } else {
                try await ClaudeCodeInstaller.install { [weak self] text in await MainActor.run { self?.claudeCodeInstall = text } }
            }
            if provider("claude-code").existingLogin() != nil { useExistingLogin("claude-code") }
            reply(say("Claude Code is installed. Sign in to your Claude account with «Sign in in Terminal» in Settings › Models.",
                      "Claude Code установлен. Войди в аккаунт Claude кнопкой «Sign in in Terminal» в Settings › Models."))
        } catch {
            reply(say("Claude Code wasn't installed: ", "Claude Code не установился: ") + error.localizedDescription)
        }
    }

    /// Claude Code's sign-in in Terminal.
    func signInClaudeCode(_ path: String) async {
        do {
            if Helper.sandboxed { try await Helper.signInClaude() } else { try ClaudeCodeInstaller.signIn(path) }
        } catch {
            llmStatus = error.localizedDescription
        }
    }

    /// Installs or updates Ollama to its newest release from the bar, the menu or the onboarding; the chat says how it went.
    func installOllama() async {
        guard ollamaInstall == nil else { return }
        ollamaInstall = "…"
        ollamaUpdated = true
        defer { ollamaInstall = nil }
        do {
            guard try await OllamaInstaller.install(step: { [weak self] text in self?.ollamaInstall = text }) else {
                ollamaUpdated = false
                reply(say("Ollama is in Downloads, shown in Finder: drag it into Applications, open it, then press Check again.",
                          "Ollama в Загрузках, открыта в Finder: перетащи её в Программы, открой и нажми Check again."))
                return
            }
            // The server takes a moment to answer after it starts.
            for _ in 0..<10 where localServers["ollama"] != true || ollamaOutdated {
                try? await Task.sleep(for: .seconds(2))
                ollamaOutdated = false
                await refreshLocalModels()
            }
            reply(say("Ollama \(ollamaVersion ?? "") is ready. Download a model in the bar below, or pick one in the model menu.",
                      "Ollama \(ollamaVersion ?? "") готова. Скачай модель в панели снизу или выбери в меню моделей."))
        } catch {
            reply(say("Ollama wasn't updated: ", "Ollama не обновилась: ") + error.localizedDescription)
        }
    }

    /// Downloads a model into Ollama from the model menu; the chat says when it's ready, or why not.
    func downloadModel(_ model: String) async {
        guard modelDownloads[model] == nil else { return }
        // New models need a new Ollama: the newest goes in first, so the download doesn't stop at «requires a newer version».
        if needsNewOllama { await installOllama() }
        modelDownloads[model] = 0
        defer { modelDownloads[model] = nil }
        do {
            try await OllamaModels.pull(model) { [weak self] p in self?.modelDownloads[model] = p }
            await refreshLocalModels()
            if providerID == "ollama", self.model == model { OllamaModels.preload(model) }
            reply(say("\(LLMCatalog.displayName(model: model)) is downloaded to this Mac — pick it in the model menu.",
                      "\(LLMCatalog.displayName(model: model)) скачана на этот Mac — выбери её в меню моделей."))
        } catch {
            // «pull model manifest: 412: The model you are attempting to pull requires a newer version of Ollama.»
            if error.localizedDescription.contains("newer version of Ollama") {
                ollamaOutdated = true
                reply(say("Ollama \(ollamaVersion ?? "") is too old for \(LLMCatalog.displayName(model: model)). Update it (free, ollama.com/download), then press Download again.",
                          "Ollama \(ollamaVersion ?? "") слишком старая для \(LLMCatalog.displayName(model: model)). Обнови её (бесплатно, ollama.com/download) и нажми Download ещё раз."))
            } else {
                reply(say("Couldn't download \(model): ", "Не удалось скачать \(model): ") + error.localizedDescription)
            }
        }
    }

    /// Where a model on this Mac stands, for the chat's bar and the onboarding: what it still needs before it can chat.
    enum LocalState: Equatable {
        case noOllama, notRunning, outdated, missing, ready
    }

    /// Ollama is older than its newest release (and wasn't just updated), or a model said it's too old.
    var needsNewOllama: Bool {
        if ollamaOutdated { return true }
        guard !ollamaUpdated, let version = ollamaVersion, let latest = ollamaLatest else { return localServers["ollama"] == false && !OllamaModels.installed }
        return OllamaInstaller.older(version, than: latest)
    }

    /// «On this Mac» in one go: Ollama installed or updated to its newest release and started, then the chosen model —
    /// or, when it isn't there, one this Mac already has, else the biggest suggested one that fits its memory —
    /// downloaded, picked and loaded.
    func setUpLocal() async {
        providerID = "ollama"
        await refreshLocalModels()
        if localServers["ollama"] != true || needsNewOllama { await installOllama() }
        guard localServers["ollama"] == true else { return }
        let have = (liveModels["ollama"] ?? []).filter { LLMCatalog.chatRefusal(model: $0) == nil }
        if have.contains(model) { useLocal(model); return }
        if let first = have.first { useLocal(first); return }
        guard let pick = localPick ?? LLMCatalog.provider(id: "ollama")?.info.models.last else { return }
        model = pick
        await downloadModel(pick)
        if liveModels["ollama"]?.contains(pick) == true { useLocal(pick) }
    }

    /// The biggest suggested model that fits this Mac's memory (the list goes biggest first); nil until the sizes are read.
    var localPick: String? {
        (LLMCatalog.provider(id: "ollama")?.info.models ?? []).first { m in modelSizes[m].map(OllamaModels.fits(sizeGB:)) ?? false }
    }

    var localState: LocalState {
        if localServers["ollama"] == false { return OllamaModels.installed ? .notRunning : .noOllama }
        // Too old for a model it tried, or older than the newest release while the model still has to be downloaded.
        let missing = liveModels["ollama"].map { $0.contains(model) } == false
        if ollamaOutdated || (missing && needsNewOllama) { return .outdated }
        return liveModels["ollama"].map { $0.contains(model) } == false || providerID != "ollama" ? .missing : .ready
    }

    /// The chat models Ollama has (embedding ones left out), then the suggested ones it hasn't: for the lists that offer
    /// Use or Download.
    var ollamaChoices: [String] {
        let have = (liveModels["ollama"] ?? []).filter { LLMCatalog.chatRefusal(model: $0) == nil }
        let rest = (LLMCatalog.provider(id: "ollama")?.info.models ?? []).filter { !have.contains($0) }
        // The ones this Mac can run before those too big for it: the list goes biggest first.
        let fits = { (m: String) in self.modelSizes[m].map(OllamaModels.fits(sizeGB:)) ?? true }
        return have + rest.filter(fits) + rest.filter { !fits($0) }
    }

    func useLocal(_ model: String) {
        providerID = "ollama"
        self.model = model
        OllamaModels.preload(model)
    }
}

/// Free, on this Mac through Ollama: what it still needs (installed, running, new enough), this Mac's memory, and the
/// models — Use one it has, or Download one, with its size and the memory it runs in. In the onboarding and Settings.
struct LocalModelsPanel: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        let memory = Int(OllamaModels.memoryGB.rounded())
        VStack(alignment: .leading, spacing: 8) {
            switch ws.localState {
            case .noOllama:
                line("One click: the newest Ollama (free, runs models here with no internet), then the best model for this Mac's \(memory) GB.")
                HStack { setUp; recheck }
            case .notRunning:
                line("Ollama is installed but not running.")
                HStack { Button("Start Ollama") { OllamaModels.start() }; setUp; recheck }
            case .outdated:
                line("Ollama \(ws.ollamaVersion ?? "") is too old for these models. One click updates it to the newest\(ws.ollamaLatest.map { ", \($0)" } ?? "") and downloads the model.")
                HStack { setUp; recheck }
            case .missing, .ready:
                line("This Mac has \(memory) GB of memory. A model runs in about 1.2× its size.")
                ForEach(ws.ollamaChoices.prefix(4), id: \.self) { model($0) }
            }
        }
        .task { await ws.refreshLocalModels() }
    }

    private func line(_ text: String) -> some View {
        Text(text).font(.ui(12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
    }

    /// Ollama to its newest release, then a model that fits, picked and loaded.
    private var setUp: some View {
        Button(ws.ollamaInstall ?? ws.modelDownloads.first.map { "Downloading · \(Int($0.value * 100))%" } ?? "Set up on this Mac") {
            Task { await ws.setUpLocal() }
        }
        .disabled(ws.ollamaInstall != nil || !ws.modelDownloads.isEmpty)
    }

    private var recheck: some View { Button("Check again") { Task { await ws.refreshLocalModels() } } }

    private func model(_ m: String) -> some View {
        let have = ws.liveModels["ollama"]?.contains(m) == true, size = ws.modelSizes[m]
        let fits = size.map { OllamaModels.fits(sizeGB: $0) } ?? true
        return HStack(spacing: 8) {
            Text(LLMCatalog.displayName(model: m)).font(.ui(13, .semibold))
            Text(size.map { "≈ \(Fmt.num(Decimal($0), maxFraction: 1)) GB · runs in ≈ \(Fmt.num(Decimal($0 * 1.2), maxFraction: 0)) GB" + (fits ? "" : " · too big") } ?? "")
                .font(.ui(11)).foregroundStyle(fits ? Color.secondary : Brand.loss)
            if let s = ws.score("ollama", m) { SettingsPill(text: s.short, good: s.good) }
            Spacer(minLength: 6)
            if ws.providerID == "ollama", ws.model == m, have {
                Label("Chat model", systemImage: "checkmark.circle.fill").font(.ui(12, .semibold)).foregroundStyle(Brand.profit)
            } else if have {
                Button("Switch") { ws.useLocal(m) }
            } else if let p = ws.modelDownloads[m] {
                Text("Downloading · \(Int(p * 100))%").font(.ui(11.5)).monospacedDigit()
            } else if fits {
                Button("Download") { Task { await ws.downloadModel(m) } }
            }
        }
    }
}

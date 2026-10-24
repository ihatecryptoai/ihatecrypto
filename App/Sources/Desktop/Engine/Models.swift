import Foundation
import IhatecryptoCore
import LLMCatalog
import LLMCore
import WalletCore

nonisolated struct ModelView: Encodable {
    var provider: String?
    var name: String?
    var model: String?
    var connected: Bool
    var models: [String]
}

nonisolated struct ProviderView: Encodable {
    var id: String
    var name: String
    var kind: String
    var takesKey: Bool
    var existingLogin: String?
    /// «Sign in with ChatGPT»: a browser sign-in, caught on 127.0.0.1.
    var signIn: String?
    var note: String
    var unavailable: String?
    var connected: Bool
    /// Its models for the menu (Engine.models(of:)) and the one it starts with.
    var models: [String] = []
    var defaultModel: String = ""
    /// The model chosen for this account, kept per account (the model menu's «<model> · connected»).
    var chosen: String = ""
    /// Each model's name as the Mac's LLMCatalog.displayName writes it («GPT 5.4», «Qwen3 8B»), the chosen one too.
    var names: [String: String] = [:]
}

/// The chat's model: the same providers as the Mac (LLMCatalog), keys kept with DPAPI. A browser sign-in (ChatGPT,
/// OpenRouter's OAuth) needs the Mac's loopback server, so on Windows those connect with a key or an existing login.
extension Engine {
    var providerID: String { settings.provider ?? "anthropic" }

    func provider(_ id: String) -> (any LLMProvider)? { LLMCatalog.provider(id: id) }

    var model: String { model(of: providerID) }

    /// The model chosen for an account, kept per account as on the Mac (`modelChoices`): switching away and back brings it
    /// back. A subscription that no longer offers it (a lapsed plan answers 400 for it) has its first listed model stand in,
    /// and the choice comes back once the list has it again.
    /// ponytail: the live list is what the provider listed last; a choice it dropped from a long list is stood in for too.
    func model(of id: String) -> String {
        guard let info = provider(id)?.info else { return "" }
        let chosen = settings.models?[id] ?? (id == settings.provider ? settings.model : nil) ?? info.defaultModel
        if info.kind == .subscription, let live = liveModels[id], let first = live.first, !live.contains(chosen) { return first }
        return chosen
    }

    /// The model for an account; nil goes back to its default.
    func setModel(_ model: String?, for id: String) {
        var all = settings.models ?? [:]
        all[id] = model.flatMap { $0.isEmpty ? nil : $0 }
        settings.models = all.isEmpty ? nil : all
        if id == settings.provider { settings.model = nil }
    }

    /// The model menu's or Settings' «Use»: the chat's account goes on with this model, on this canvas too.
    func chooseModel(_ model: String?) {
        setModel(model, for: providerID)
        canvasTalked(provider: providerID)
    }

    /// The chat goes on with this account, as the profile menu's and Settings' «Switch» do (the Mac's useAccount): its
    /// model stays as chosen for it, and on this PC the first downloaded chat model stands in for one that isn't there.
    func useAccount(_ id: String, model: String? = nil) {
        guard let p = provider(id), p.info.unavailable == nil, Store.credentials()[id] != nil || p.info.kind == .local else { return }
        settings.provider = id
        if let model { setModel(model, for: id) }
        if provider(id)?.info.kind == .local, let live = liveModels[id], !live.contains(self.model),
           let first = live.first(where: { LLMCatalog.chatRefusal(model: $0) == nil }) { setModel(first, for: id) }
        canvasTalked(provider: id)
    }

    /// The model client (the chat's account, or `id`'s for Settings' Check), or nil when it isn't connected: then the chat
    /// reads plain commands only.
    func llmClient(_ asked: String? = nil) -> (any LLMClient)? {
        guard let p = provider(asked ?? providerID), p.info.unavailable == nil else { return nil }
        let id = p.info.id, credential = Store.credentials()[id]
        guard credential != nil || p.info.kind == .local, let made = try? p.client(credential, save: { Store.setCredential($0, for: id) }) else { return nil }
        // Settings › AI model › «Think before answering»: every request reasons first (the Mac's ThinkingClient).
        let client: any LLMClient = settings.thinking == true ? ThinkingClient(made) : made
        // Every answer says what it read: the chip over the chat's field, as the Mac's ContextMeter.
        // ponytail: counted on the canvas open when the answer ends; a scheduled check answering in the background counts there too.
        return MeteredClient(client) { [weak self] usage, request in
            Task { @MainActor in await self?.counted(usage, request: request, provider: id) }
        }
    }

    /// A model that reads the chat: connected, big enough for its brief, and, on this PC, its server answering with the
    /// model downloaded (the Mac's hasModel).
    var hasModel: Bool {
        guard let p = provider(providerID), llmClient() != nil, LLMCatalog.chatRefusal(model: model) == nil else { return false }
        guard p.info.kind == .local else { return true }
        return localServers[p.info.id] != false && liveModels[p.info.id].map { $0.contains(model) } != false
    }

    var modelView: ModelView {
        let p = provider(providerID)
        return ModelView(provider: p?.info.id, name: p?.info.name, model: model.isEmpty ? nil : model, connected: hasModel,
                         models: models(of: p?.info.id ?? ""))
    }

    /// A way in's models for the menu: the live list where there is one (a subscription now, what a server here has, the
    /// suggested ones it hasn't after it), else the checked list; the model in use last when neither lists it.
    func models(of id: String) -> [String] {
        guard let info = provider(id)?.info else { return [] }
        var listed = liveModels[id] ?? info.models
        if info.kind == .local, liveModels[id] != nil { listed += info.models.filter { !listed.contains($0) } }
        return id == providerID && !model.isEmpty && !listed.contains(model) ? listed + [model] : listed
    }

    var providerViews: [ProviderView] {
        let saved = Store.credentials()
        return LLMCatalog.all.filter { $0.info.id != "apple" }.map { p in
            // The catalog is the Mac's: «Ollama on this Mac» runs on this PC here.
            ProviderView(id: p.info.id, name: p.info.name.replacingOccurrences(of: "this Mac", with: "this PC"), kind: "\(p.info.kind)", takesKey: p.info.takesKey,
                         existingLogin: p.existingLogin() != nil ? p.info.existingLoginLabel : nil, signIn: p.info.signInLabel,
                         note: p.info.note.replacingOccurrences(of: "this Mac", with: "this PC"),
                         unavailable: p.info.unavailable, connected: saved[p.info.id] != nil || p.info.kind == .local,
                         models: models(of: p.info.id), defaultModel: p.info.defaultModel, chosen: model(of: p.info.id),
                         names: Dictionary(models(of: p.info.id).map { ($0, LLMCatalog.displayName(model: $0)) } + [(model(of: p.info.id), LLMCatalog.displayName(model: model(of: p.info.id)))],
                                           uniquingKeysWith: { a, _ in a }))
        }
    }

    /// A key typed in the window, or a login another tool already has (Claude Code, Codex) when no key is given.
    /// Without a key, an account connected before is switched to as it is: «Switch to OpenRouter» sends the provider only.
    /// A new login doesn't switch the chat to it (the owner, 2026-10-20: «я просто хотел подключить другой аккаунт»),
    /// unless the chat has no model to go on with; a model on this PC picked is a switch.
    func connect(provider id: String, key: String?, model: String?) async throws {
        guard let p = provider(id) else { throw WalletError("No model provider «\(id)».") }
        if let why = p.info.unavailable { throw WalletError(why) }
        let typed = key?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        // Checked with its one short answer when it was kept: switching back asks the model nothing.
        if typed.isEmpty, Store.credentials()[id] != nil {
            useAccount(id, model: model)
            return
        }
        let hadModel = hasModel
        let credential: Credential?
        if !typed.isEmpty {
            credential = .apiKey(typed)
        } else if let existing = p.existingLogin() {
            credential = existing
        } else if p.info.kind == .local {
            credential = nil
        } else if !p.info.takesKey {
            // A subscription that signs in through its own tool (Claude Code, Codex) with no sign-in on this PC yet: as the
            // Mac's useExistingLogin says it.
            throw WalletError(say("No sign-in found on this PC.", "Вход на этом ПК не найден."))
        } else {
            throw WalletError(say("Paste the key for \(p.info.name).", "Вставь ключ для \(p.info.name)."))
        }
        // Checked with one short answer before it's kept: a wrong key fails here, not on the first money message.
        let client = try p.client(credential, save: { Store.setCredential($0, for: id) })
        let chosen = model ?? p.info.defaultModel
        _ = try await client.complete(LLMRequest(model: chosen, messages: [LLMMessage(.user, "Reply with OK.")]))
        if let credential { Store.setCredential(credential, for: id) }
        if let model { setModel(model, for: id) }
        if !hadModel || (p.info.kind == .local && typed.isEmpty) { useAccount(id) }
    }

    /// Settings' «Use <login>»: the login another tool has (Claude Code, Codex) taken again for this account, as the Mac's
    /// useExistingLogin — kept, and the chat stays where it is unless it has no model to go on with («Switch» switches).
    func useLogin(provider id: String) throws {
        guard let p = provider(id), let login = p.existingLogin() else {
            throw WalletError(say("No sign-in found on this PC.", "На этом компьютере входа не нашлось."))
        }
        let hadModel = hasModel
        Store.setCredential(login, for: id)
        if !hadModel { useAccount(id) } else { changed() }
    }

    /// The provider's browser sign-in: the window opens the page (the «openURL» event), the redirect comes back to
    /// 127.0.0.1, and the tokens are kept like a key.
    func signIn(provider id: String) async throws {
        guard let p = provider(id), p.info.signInLabel != nil else { throw WalletError("«\(id)» has no browser sign-in: paste its key.") }
        let out = out
        let hadModel = hasModel
        let credential = try await p.signIn(open: { url in out.event("openURL", ["url": url.absoluteString]) })
        Store.setCredential(credential, for: id)
        if !hadModel { useAccount(id) }
    }

    /// Settings › Second check › Connect: OpenRouter signed in for the second check alone (the Mac's signInJev) — the
    /// chat stays on its own account.
    func signInJev() async throws {
        guard let p = provider("openrouter") else { return }
        let out = out
        let credential = try await p.signIn(open: { url in out.event("openURL", ["url": url.absoluteString]) })
        Store.setCredential(credential, for: "openrouter")
        settings.jevOpenRouter = true
    }

    var secondCheck: Bool { settings.secondCheck ?? true }

    /// The OpenRouter key Jev may use: when the chat is on OpenRouter, or the user turned Jev on for it.
    var jevKey: String? {
        guard secondCheck, providerID == "openrouter" || settings.jevOpenRouter == true else { return nil }
        switch Store.credentials()["openrouter"] {
        case .apiKey(let key)?: return key
        case .oauth(let tokens)?: return tokens.access
        default: return nil
        }
    }

    func disconnect(provider id: String) {
        Store.setCredential(nil, for: id)
        changed()
    }
}

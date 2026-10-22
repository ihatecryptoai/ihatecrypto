import Foundation
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
}

/// The chat's model: the same providers as the Mac (LLMCatalog), keys kept with DPAPI. A browser sign-in (ChatGPT,
/// OpenRouter's OAuth) needs the Mac's loopback server, so on Windows those connect with a key or an existing login.
extension Engine {
    var providerID: String { settings.provider ?? "anthropic" }

    func provider(_ id: String) -> (any LLMProvider)? { LLMCatalog.provider(id: id) }

    var model: String { settings.model ?? provider(providerID)?.info.defaultModel ?? "" }

    /// The model client, or nil when none is connected: then the chat reads plain commands only.
    func llmClient() -> (any LLMClient)? {
        guard let p = provider(providerID), p.info.unavailable == nil else { return nil }
        let id = p.info.id, credential = Store.credentials()[id]
        guard credential != nil || p.info.kind == .local else { return nil }
        return try? p.client(credential, save: { Store.setCredential($0, for: id) })
    }

    var modelView: ModelView {
        let p = provider(providerID)
        return ModelView(provider: p?.info.id, name: p?.info.name, model: model.isEmpty ? nil : model, connected: llmClient() != nil,
                         models: p?.info.models ?? [])
    }

    var providerViews: [ProviderView] {
        let saved = Store.credentials()
        return LLMCatalog.all.filter { $0.info.id != "apple" }.map { p in
            ProviderView(id: p.info.id, name: p.info.name, kind: "\(p.info.kind)", takesKey: p.info.takesKey,
                         existingLogin: p.existingLogin() != nil ? p.info.existingLoginLabel : nil, signIn: p.info.signInLabel, note: p.info.note,
                         unavailable: p.info.unavailable, connected: saved[p.info.id] != nil || p.info.kind == .local)
        }
    }

    /// A key typed in the window, or a login another tool already has (Claude Code, Codex) when no key is given.
    func connect(provider id: String, key: String?, model: String?) async throws {
        guard let p = provider(id) else { throw WalletError("No model provider «\(id)».") }
        if let why = p.info.unavailable { throw WalletError(why) }
        let credential: Credential?
        if let key = key?.trimmingCharacters(in: .whitespacesAndNewlines), !key.isEmpty {
            credential = .apiKey(key)
        } else if let existing = p.existingLogin() {
            credential = existing
        } else if p.info.kind == .local {
            credential = nil
        } else {
            throw WalletError(say("Paste the key for \(p.info.name).", "Вставь ключ для \(p.info.name)."))
        }
        // Checked with one short answer before it's kept: a wrong key fails here, not on the first money message.
        let client = try p.client(credential, save: { Store.setCredential($0, for: id) })
        let chosen = model ?? p.info.defaultModel
        _ = try await client.complete(LLMRequest(model: chosen, messages: [LLMMessage(.user, "Reply with OK.")]))
        if let credential { Store.setCredential(credential, for: id) }
        settings.provider = id
        settings.model = chosen
    }

    /// The provider's browser sign-in: the window opens the page (the «openURL» event), the redirect comes back to
    /// 127.0.0.1, and the tokens are kept like a key.
    func signIn(provider id: String) async throws {
        guard let p = provider(id), p.info.signInLabel != nil else { throw WalletError("«\(id)» has no browser sign-in: paste its key.") }
        let out = out
        let credential = try await p.signIn(open: { url in out.event("openURL", ["url": url.absoluteString]) })
        Store.setCredential(credential, for: id)
        settings.provider = id
        settings.model = p.info.defaultModel
    }

    func disconnect(provider id: String) {
        Store.setCredential(nil, for: id)
        changed()
    }
}

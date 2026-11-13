import Foundation

public struct LLMMessage: Sendable, Equatable {
    public enum Role: String, Sendable { case system, user, assistant }

    public let role: Role
    public let text: String

    public init(_ role: Role, _ text: String) {
        self.role = role
        self.text = text
    }
}

public struct LLMRequest: Sendable {
    public var model: String
    public var messages: [LLMMessage]
    /// Ask for a bare JSON object where the API supports it. Callers still parse leniently (`JSONText`).
    public var wantsJSON: Bool
    /// Let the model reason before it answers (Settings › Models › «Think before answering»): slower, steadier on a muddled
    /// or tricky request. Each service gets it its own way; one that doesn't know it answers as usual.
    public var thinking: Bool

    public init(model: String, messages: [LLMMessage], wantsJSON: Bool = false, thinking: Bool = false) {
        self.model = model
        self.messages = messages
        self.wantsJSON = wantsJSON
        self.thinking = thinking
    }

    public var system: String { messages.filter { $0.role == .system }.map(\.text).joined(separator: "\n\n") }
    public var dialogue: [LLMMessage] { messages.filter { $0.role != .system } }
}

/// A client whose every request thinks first (`LLMRequest.thinking`), whatever its caller asked.
public struct ThinkingClient: LLMClient {
    let base: any LLMClient
    public init(_ base: any LLMClient) { self.base = base }

    public func complete(_ request: LLMRequest) async throws -> String {
        var r = request
        r.thinking = true
        return try await base.complete(r)
    }

    public func stream(_ request: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        var r = request
        r.thinking = true
        return try await base.stream(r, partial: partial)
    }
}

/// A ready connection to one model service. The app talks only to this.
public protocol LLMClient: Sendable {
    func complete(_ request: LLMRequest) async throws -> String
    /// The same answer, with `partial` told the text so far as it arrives, so a reader can start before it ends.
    func stream(_ request: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String
}

extension LLMClient {
    /// A service that can't stream answers once, at the end.
    public func stream(_ request: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let text = try await complete(request)
        partial(text)
        return text
    }
}

public enum ProviderKind: String, Sendable, CaseIterable {
    case subscription, apiKey, local
}

public struct ProviderInfo: Sendable, Identifiable {
    public let id: String
    public let name: String
    public let kind: ProviderKind
    public let defaultModel: String
    public let note: String
    public var takesKey: Bool
    /// Button title for an interactive sign-in, nil when there is none.
    public var signInLabel: String?
    /// Button title for reusing a login another tool already has, nil when there is none.
    public var existingLoginLabel: String?
    /// Why this option cannot be used right now; nil when it can.
    public var unavailable: String?
    /// The current models this way in offers, checked, the recommended one first: the chat's menu and Settings pick
    /// from them. Any other id can still be typed in Settings.
    public var models: [String]
    /// Where a key is made, and what to press there: Settings shows them as the way to connect.
    public var keyURL: URL?
    public var keySteps: String?

    public init(
        id: String, name: String, kind: ProviderKind, defaultModel: String, note: String,
        takesKey: Bool = false, signInLabel: String? = nil, existingLoginLabel: String? = nil, unavailable: String? = nil,
        models: [String] = [], keyURL: String? = nil, keySteps: String? = nil
    ) {
        self.models = models
        self.keyURL = keyURL.flatMap(URL.init(string:))
        self.keySteps = keySteps
        self.id = id
        self.name = name
        self.kind = kind
        self.defaultModel = defaultModel
        self.note = note
        self.takesKey = takesKey
        self.signInLabel = signInLabel
        self.existingLoginLabel = existingLoginLabel
        self.unavailable = unavailable
    }
}

/// What the app stores in Keychain for a provider.
public enum Credential: Codable, Sendable, Equatable {
    case apiKey(String)
    case oauth(OAuthTokens)
    /// A login owned by another tool (Codex CLI auth file, Claude Code binary). Only the path is stored.
    case external(path: String)
}

public struct OAuthTokens: Codable, Sendable, Equatable {
    public var access: String
    public var refresh: String
    public var expires: Date
    public var accountID: String?

    public init(access: String, refresh: String, expires: Date, accountID: String?) {
        self.access = access
        self.refresh = refresh
        self.expires = expires
        self.accountID = accountID
    }
}

public typealias OpenURL = @Sendable (URL) -> Void
public typealias SaveCredential = @Sendable (Credential) -> Void

/// One way to reach models: an API key, a subscription login, or a local server.
/// A provider lives in its own module under Sources/LLM and is listed in `LLMCatalog`.
public protocol LLMProvider: Sendable {
    var info: ProviderInfo { get }
    /// Interactive sign-in (browser OAuth or finding an installed tool).
    func signIn(open: OpenURL) async throws -> Credential
    /// A login that already exists on this Mac and can be reused, if any.
    func existingLogin() -> Credential?
    /// `save` receives refreshed credentials so they survive a restart.
    func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient
    /// Model IDs the service offers, when it can list them.
    func models(_ credential: Credential?) async throws -> [String]
}

extension LLMProvider {
    public func signIn(open: OpenURL) async throws -> Credential {
        throw LLMError("\(info.name): sign-in isn't supported.")
    }
    public func existingLogin() -> Credential? { nil }
    public func models(_ credential: Credential?) async throws -> [String] { info.models }
}

public struct LLMError: LocalizedError {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

public enum JSONText {
    /// The JSON object in a model reply, whether bare, fenced in ```json or surrounded by prose.
    public static func object(in text: String) -> Data? {
        guard let start = text.firstIndex(of: "{"), let end = text.lastIndex(of: "}"), start < end else { return nil }
        return Data(text[start...end].utf8)
    }
}

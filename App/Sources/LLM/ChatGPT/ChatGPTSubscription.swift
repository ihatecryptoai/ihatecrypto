import Foundation
import LLMCore

/// ChatGPT Plus/Pro through the Codex backend: the sign-in OpenClaw and OpenCode use. Its OAuth client is Codex CLI's (no
/// other is offered), so the sign-in says so; the requests themselves name ihatecrypto as their originator.
/// Calls count against the user's ChatGPT plan, not API credits. OpenAI tolerates this publicly,
/// but it is not in their terms, so the OpenRouter or API-key options must keep working without it.
public struct ChatGPTSubscription: LLMProvider {
    // Codex CLI's public OAuth client and its registered redirect.
    static let clientID = "app_EMoamEEZ73f0CkXaXp7hrann"
    static let issuer = "https://auth.openai.com"
    static let redirect = "http://localhost:1455/auth/callback"
    static let backend = "https://chatgpt.com/backend-api/codex/responses"
    static let codexAuthFile = "~/.codex/auth.json"

    public init() {}

    public var info: ProviderInfo {
        ProviderInfo(
            id: "chatgpt", name: "ChatGPT (subscription)", kind: .subscription, defaultModel: "gpt-6.1-sol",
            note: "Plus or Pro. Signs in through OpenAI's Codex CLI sign-in, as OpenClaw and OpenCode do; OpenAI doesn't officially allow other apps to use it — a gray area. Usage counts against the subscription limits.",
            signInLabel: "Sign in with ChatGPT", existingLoginLabel: "Use the Codex CLI login",
            // All answer through this sign-in (checked 2026-09-30). 6.1 Sol read 1,000 test messages as well as 6 Sol, as fast.
            // Without a paid plan Codex lists only gpt-6-luna and 5.6 (2026-10-08): Workspace.model stands in with those.
            models: ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna"]
        )
    }

    /// The models this subscription offers, live from the Codex backend (the list Codex CLI shows), recommended first;
    /// the checked ones when it can't be read. Tokens are never refreshed here: that happens only with a request.
    public func models(_ credential: Credential?) async throws -> [String] {
        let access: (token: String, account: String)?
        switch credential {
        case .oauth(let t) where t.expires > Date(): access = t.accountID.map { (t.access, $0) }
        case .external(let path): access = (try? CodexAuthFile.read(path)).map { ($0.access, $0.accountID) }
        default: access = nil
        }
        guard let access, let data = try? await HTTP.get(Self.models + "?client_version=0.200.0", headers: [
            "Authorization": "Bearer \(access.token)", "chatgpt-account-id": access.account, "originator": "ihatecrypto",
        ]) else { return info.models }
        // The same list says each model's context window: kept for the client, which reports it with an answer.
        for (model, window) in Self.windows(data) { ModelWindows.told[model] = window }
        let live = Self.listed(data)
        return live.isEmpty ? info.models : live
    }

    /// Each model's context window in a Codex backend reply (272,000 for a subscription's, 2026-10-13; the API's own
    /// window for the same model is wider).
    static func windows(_ data: Data) -> [String: Int] {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rows = json["models"] as? [[String: Any]] else { return [:] }
        return Dictionary(rows.compactMap { row in
            guard let slug = row["slug"] as? String, let window = (row["context_window"] as? NSNumber)?.intValue else { return nil }
            return (slug, window)
        }, uniquingKeysWith: { first, _ in first })
    }

    static let models = "https://chatgpt.com/backend-api/codex/models"

    /// The models a Codex backend reply lists for people to pick («visibility: list»), by its priority.
    static func listed(_ data: Data) -> [String] {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rows = json["models"] as? [[String: Any]] else { return [] }
        return rows.filter { $0["visibility"] as? String == "list" }
            .sorted { ($0["priority"] as? Int ?? .max) < ($1["priority"] as? Int ?? .max) }
            .compactMap { $0["slug"] as? String }
    }

    public func existingLogin() -> Credential? {
        let path = NSString(string: Self.codexAuthFile).expandingTildeInPath
        guard (try? CodexAuthFile.read(path)) != nil else { return nil }
        return .external(path: path)
    }

    public func signIn(open: OpenURL) async throws -> Credential {
        let pkce = PKCE.make()
        let state = PKCE.make().verifier
        let server = try await LoopbackServer.start(port: 1455, path: "/auth/callback")
        var url = URLComponents(string: Self.issuer + "/oauth/authorize")!
        url.queryItems = [
            ("response_type", "code"), ("client_id", Self.clientID), ("redirect_uri", Self.redirect),
            ("scope", "openid profile email offline_access"),
            ("code_challenge", pkce.challenge), ("code_challenge_method", "S256"),
            ("id_token_add_organizations", "true"), ("codex_cli_simplified_flow", "true"),
            ("state", state), ("originator", "codex_cli_rs"),
        ].map { URLQueryItem(name: $0.0, value: $0.1) }
        open(url.url!)
        let query = try await server.waitForCallback()
        guard query["state"] == state, let code = query["code"] else {
            throw LLMError(query["error_description"] ?? "ChatGPT sign-in cancelled.")
        }
        let data = try await HTTP.post(Self.issuer + "/oauth/token", form: [
            "grant_type": "authorization_code", "code": code, "redirect_uri": Self.redirect,
            "client_id": Self.clientID, "code_verifier": pkce.verifier,
        ])
        return .oauth(try Self.tokens(from: data, previous: nil))
    }

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        switch credential {
        case .oauth(let tokens): ChatGPTClient(source: .own(tokens), save: save)
        case .external(let path): ChatGPTClient(source: .codexFile(path), save: save)
        default: throw LLMError("ChatGPT: sign in via the browser or use the Codex CLI login.")
        }
    }

    static func tokens(from data: Data, previous: OAuthTokens?) throws -> OAuthTokens {
        struct Response: Decodable { let access_token: String; let refresh_token: String?; let id_token: String? }
        let r = try JSONDecoder().decode(Response.self, from: data)
        guard let refresh = r.refresh_token ?? previous?.refresh else { throw LLMError("ChatGPT didn't return a refresh token.") }
        return OAuthTokens(
            access: r.access_token, refresh: refresh,
            expires: JWT.expiry(r.access_token) ?? Date().addingTimeInterval(3600),
            accountID: r.id_token.flatMap(accountID) ?? accountID(r.access_token) ?? previous?.accountID
        )
    }

    static func accountID(_ jwt: String) -> String? {
        (JWT.claims(jwt)?["https://api.openai.com/auth"] as? [String: Any])?["chatgpt_account_id"] as? String
    }
}

/// Codex CLI keeps its ChatGPT login in ~/.codex/auth.json and refreshes it itself.
enum CodexAuthFile {
    static func read(_ path: String) throws -> (access: String, accountID: String) {
        struct Tokens: Decodable { let access_token: String; let account_id: String?; let id_token: String? }
        struct File: Decodable { let tokens: Tokens? }
        let file = try JSONDecoder().decode(File.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
        guard let t = file.tokens, let account = t.account_id ?? t.id_token.flatMap(ChatGPTSubscription.accountID) else {
            throw LLMError("Codex CLI isn't signed in with ChatGPT.")
        }
        return (t.access_token, account)
    }
}

actor ChatGPTClient: LLMClient {
    enum Source { case own(OAuthTokens), codexFile(String) }

    private var source: Source
    private let save: SaveCredential

    init(source: Source, save: @escaping SaveCredential) {
        self.source = source
        self.save = save
    }

    func complete(_ r: LLMRequest) async throws -> String { try await stream(r) { _ in } }

    /// One id for this run of the app, and a cache key from the instructions: the same 20K-character system prompt every
    /// turn is then read from the backend's prompt cache instead of anew (a new session id per request defeated it).
    static let session = UUID().uuidString

    func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
        let (token, account) = try await credentials()
        let request = try HTTP.request(
            ChatGPTSubscription.backend, method: "POST",
            body: try JSONSerialization.data(withJSONObject: Self.body(r)), contentType: "application/json",
            headers: [
                "Authorization": "Bearer \(token)", "chatgpt-account-id": account,
                "OpenAI-Beta": "responses=experimental", "originator": "ihatecrypto",
                "session_id": Self.session, "Accept": "text/event-stream",
            ],
            timeout: 120
        )
        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        if (response as? HTTPURLResponse)?.statusCode != 200 {
            var body = Data()
            for try await byte in bytes.prefix(600) { body.append(byte) }
            try HTTP.check(response, body: body)
        }
        var stream = CodexStream()
        for try await line in bytes.lines {
            let before = stream.text.count
            if try stream.feed(line) { break }
            if stream.text.count != before { partial(stream.text) }
        }
        guard !stream.text.isEmpty else { throw LLMError("ChatGPT returned an empty reply.") }
        if var usage = stream.usage {
            usage.window = ModelWindows.told[r.model]
            LLMUsage.report(usage)
        }
        return stream.text
    }

    /// Own tokens are refreshed here. Codex CLI tokens are never refreshed by us:
    /// the refresh token rotates, and refreshing it would log Codex CLI out.
    private func credentials() async throws -> (token: String, account: String) {
        switch source {
        case .codexFile(let path):
            let file = try CodexAuthFile.read(path)
            if let exp = JWT.expiry(file.access), exp < Date() {
                throw LLMError("The Codex CLI login expired. Run codex once or sign in via the browser.")
            }
            return (file.access, file.accountID)
        case .own(var tokens):
            // ponytail: two parallel refreshes would race on the rotating refresh token; the app sends one request at a time.
            if tokens.expires < Date().addingTimeInterval(300) {
                let data = try await HTTP.post(ChatGPTSubscription.issuer + "/oauth/token", form: [
                    "grant_type": "refresh_token", "client_id": ChatGPTSubscription.clientID,
                    "refresh_token": tokens.refresh, "scope": "openid profile email",
                ])
                tokens = try ChatGPTSubscription.tokens(from: data, previous: tokens)
                source = .own(tokens)
                save(.oauth(tokens))
            }
            guard let account = tokens.accountID else { throw LLMError("No chatgpt_account_id; sign in again.") }
            return (tokens.access, account)
        }
    }

    static func body(_ r: LLMRequest) -> [String: Any] {
        var body: [String: Any] = [
            "model": r.model,
            "instructions": r.system,
            "input": r.dialogue.map { m in
                ["type": "message", "role": m.role.rawValue,
                 "content": [["type": m.role == .assistant ? "output_text" : "input_text", "text": m.text]]]
            },
            "store": false,
            "stream": true,
            "prompt_cache_key": "ihatecrypto-" + cacheKey(r.system),
        ]
        // Left to the model's own default unless asked to think it through.
        if r.thinking { body["reasoning"] = ["effort": "high"] }
        return body
    }

    /// FNV-1a of the instructions, the same across launches: a changed prompt gets a cache of its own.
    static func cacheKey(_ text: String) -> String {
        String(text.utf8.reduce(UInt64(14_695_981_039_346_656_037)) { ($0 ^ UInt64($1)) &* 1_099_511_628_211 }, radix: 36)
    }
}

/// Collects text from the Responses API event stream.
struct CodexStream {
    private(set) var text = ""
    /// What the completed response says it read and wrote.
    private(set) var usage: LLMUsage?

    /// Returns true when the response is complete.
    mutating func feed(_ line: String) throws -> Bool {
        guard line.hasPrefix("data:"),
              let event = try? JSONSerialization.jsonObject(with: Data(line.dropFirst(5).utf8)) as? [String: Any],
              let type = event["type"] as? String else { return false }
        switch type {
        case "response.output_text.delta":
            text += event["delta"] as? String ?? ""
        case "response.completed":
            usage = LLMUsage(reported: (event["response"] as? [String: Any])?["usage"] as? [String: Any])
            return true
        case "response.failed", "response.incomplete", "error":
            let error = ((event["response"] as? [String: Any])?["error"] as? [String: Any])?["message"] as? String
            throw LLMError("ChatGPT: \(error ?? event["message"] as? String ?? type)")
        default:
            break
        }
        return false
    }
}

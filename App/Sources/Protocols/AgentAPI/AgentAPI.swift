import CryptoKit
import Foundation
import WalletCore

/// NEAR Intents' Agent API (docs.agentsonintents.com): a wallet for an agent, held by NEAR Intents' custody under a
/// policy its owner signs — what it may do, with which coins, how much a day, to which addresses. The owner (this
/// wallet's NEAR account) signs each change through the signer (NEP-413 / NEP-366); the agent then acts with a grant
/// token alone, and the API refuses what the policy doesn't allow. Mainnet only: the demo host moves real money too.
public enum AgentAPI {
    /// The hackathon's demo host (its partner keys are temporary); the production one is api.agentsonintents.com.
    public static let demoHost = "api.demo.agentsonintents.com"
    public static let hosts: Set<String> = [demoHost, "api.agentsonintents.com"]

    /// The partner key (naa_…), from the Keychain; never in a prompt or a file.
    public static var key: String? {
        get { lock.withLock { own } }
        set { lock.withLock { own = newValue.flatMap { $0.isEmpty ? nil : $0 } } }
    }
    public static var host: String {
        get { lock.withLock { ownHost } }
        set { lock.withLock { ownHost = hosts.contains(newValue) ? newValue : demoHost } }
    }
    private static let lock = NSLock()
    nonisolated(unsafe) private static var own: String?
    nonisolated(unsafe) private static var ownHost = demoHost

    /// An owner intent waiting for a signature: `payload` is the raw JSON to sign, exactly as the API wrote it.
    public struct Generated: Sendable {
        public let correlationID: String
        public let type: String
        public let agentID: String?
        public let standard: String
        public let payload: Data
        public let signerAccount: String?
        public let expiresAt: String?
        /// What the API says the owner installs: the policy and its hash (sha256 of its JSON with sorted keys, compact).
        public let previewPolicy: Data?
        public let previewHash: String?
    }

    /// sha256 of a policy written with sorted keys and no spaces: how the API names a policy (preview.policy_hash).
    public static func policyHash(_ policy: Data) -> String? {
        guard let o = try? JSONSerialization.jsonObject(with: policy),
              let canonical = try? JSONSerialization.data(withJSONObject: o, options: [.sortedKeys, .withoutEscapingSlashes]) else { return nil }
        return SHA256.hash(data: canonical).map { String(format: "%02x", $0) }.joined()
    }

    public struct Status: Sendable {
        public let status: String
        public let details: Data?
        public var final: Bool { ["SUCCESS", "REFUNDED", "FAILED", "NEEDS_REVIEW"].contains(status) }
        public var ok: Bool { status == "SUCCESS" }
    }

    /// An error as the API gives it: `code` to branch on, the title and detail to show.
    public struct Failure: LocalizedError, Sendable {
        public let status: Int
        public let code: String
        public let text: String
        public var errorDescription: String? { "Agent API: \(text) (\(code))" }

        init(status: Int, data: Data) {
            let first = ((try? JSONSerialization.jsonObject(with: data)) as? [String: Any])?["errors"] as? [[String: Any]]
            let e = first?.first ?? [:]
            self.status = status
            code = e["code"] as? String ?? "http_\(status)"
            text = [e["title"] as? String, e["detail"] as? String].compactMap { $0 }.joined(separator: ": ")
                .ifEmpty(String(decoding: data.prefix(200), as: UTF8.self))
        }
    }

    /// One call. `grant`: the agent's grant token, for its actions; `idempotency`: required on money actions, reused
    /// on a retry of the same body.
    public static func call(_ method: String, _ path: String, body: Data? = nil, grant: String? = nil, idempotency: String? = nil) async throws -> Data {
        guard let key else { throw Failure(status: 401, data: Data(#"{"errors":[{"code":"no_key","title":"No Agent API key: add it in Settings"}]}"#.utf8)) }
        var r = URLRequest(url: URL(string: "https://\(host)\(path)")!, timeoutInterval: 60)
        r.httpMethod = method
        r.setValue(key, forHTTPHeaderField: "X-API-Key")
        if let grant { r.setValue(grant, forHTTPHeaderField: "X-Grant-Token") }
        if let idempotency { r.setValue(idempotency, forHTTPHeaderField: "Idempotency-Key") }
        if let body { r.httpBody = body; r.setValue("application/json", forHTTPHeaderField: "Content-Type") }
        let (data, response) = try await URLSession.shared.data(for: r)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else { throw Failure(status: code, data: data) }
        return data
    }

    /// Why the API won't take `key`, or nil: GET /v1/whoami asked with it.
    public static func problem(withKey key: String) async -> String? {
        var r = URLRequest(url: URL(string: "https://\(host)/v1/whoami")!, timeoutInterval: 30)
        r.setValue(key, forHTTPHeaderField: "X-API-Key")
        guard let (data, response) = try? await URLSession.shared.data(for: r) else { return "The Agent API didn't answer. Try again in a minute." }
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        return (200..<300).contains(code) ? nil : Failure(status: code, data: data).errorDescription
    }

    public static func json(_ object: [String: Any]) -> Data { (try? JSONSerialization.data(withJSONObject: object)) ?? Data() }

    /// POST /v1/generate-intent: what the owner signs for `body` (a JSON object: its `type` and fields).
    public static func generate(_ body: Data, idempotency: String? = nil) async throws -> Generated {
        let data = try await call("POST", "/v1/generate-intent", body: body, idempotency: idempotency)
        guard let o = try JSONSerialization.jsonObject(with: data) as? [String: Any], let id = o["correlation_id"] as? String,
              let intent = o["intent"] as? [String: Any], let standard = intent["standard"] as? String else {
            throw Failure(status: 200, data: data)
        }
        let signer = o["signer"] as? [String: Any], preview = o["preview"] as? [String: Any]
        return Generated(correlationID: id, type: o["type"] as? String ?? "", agentID: o["agent_id"] as? String, standard: standard,
                         payload: try NearOwnerSigning.rawValue("payload", in: try NearOwnerSigning.rawValue("intent", in: data)),
                         signerAccount: signer?["account_id"] as? String, expiresAt: o["expires_at"] as? String,
                         previewPolicy: (preview?["policy"] as? [String: Any]).map(json), previewHash: preview?["policy_hash"] as? String)
    }

    /// POST /v1/submit-intent with what the signer gave; sending the same signature again is safe.
    public static func submit(_ g: Generated, signedData: Data) async throws {
        // signed_data carries the payload byte for byte as it came, so it's spliced in, not re-encoded.
        let body = json(["type": g.type, "correlation_id": g.correlationID]).dropLast() + Data(#","signed_data":"#.utf8) + signedData + Data("}".utf8)
        _ = try await call("POST", "/v1/submit-intent", body: body)
    }

    /// GET /v1/status, waiting up to `waitMs` on the server for a change.
    public static func status(_ correlationID: String, waitMs: Int = 30000) async throws -> Status {
        let data = try await call("GET", "/v1/status?correlation_id=\(correlationID)&wait_ms=\(waitMs)")
        let o = try JSONSerialization.jsonObject(with: data) as? [String: Any] ?? [:]
        return Status(status: o["status"] as? String ?? "UNKNOWN", details: (o["details"] as? [String: Any]).map(json))
    }

    /// Until it's final: UNCERTAIN keeps being asked, NEEDS_REVIEW stops (never retried).
    /// A dropped connection or a timeout while asking is asked again until `seconds` pass: the operation runs on
    /// regardless, and giving up on the first one read a send that went as «failed» (2026-11-05).
    public static func final(_ correlationID: String, within seconds: Double = 600) async throws -> Status {
        let end = Date().addingTimeInterval(seconds)
        while true {
            do {
                let s = try await status(correlationID)
                if s.final || Date() > end { return s }
            } catch is URLError where Date() < end {
                try await Task.sleep(for: .seconds(3))
            }
        }
    }
}

private extension String {
    func ifEmpty(_ other: String) -> String { isEmpty ? other : self }
}

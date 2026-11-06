import CryptoKit
import Foundation
import Network
import os

/// RFC 7636 proof key for the browser sign-in flows.
public struct PKCE: Sendable {
    public let verifier: String
    public let challenge: String

    public init(verifier: String) {
        self.verifier = verifier
        challenge = Self.base64url(Data(SHA256.hash(data: Data(verifier.utf8))))
    }

    public static func make() -> PKCE {
        var generator = SystemRandomNumberGenerator()
        return PKCE(verifier: base64url(Data((0..<32).map { _ in UInt8.random(in: .min ... .max, using: &generator) })))
    }

    public static func base64url(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

public enum JWT {
    /// Payload claims without verifying the signature: only for reading our own tokens.
    public static func claims(_ token: String) -> [String: Any]? {
        let parts = token.split(separator: ".")
        guard parts.count >= 2 else { return nil }
        var s = parts[1].replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        s += String(repeating: "=", count: (4 - s.count % 4) % 4)
        guard let data = Data(base64Encoded: s) else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }

    public static func expiry(_ token: String) -> Date? {
        (claims(token)?["exp"] as? Double).map(Date.init(timeIntervalSince1970:))
    }
}

/// Catches one OAuth redirect on the loopback interface, so nothing outside this Mac can reach it.
public final class LoopbackServer: @unchecked Sendable {
    public let port: UInt16
    private let listener: NWListener
    private let queries: AsyncStream<[String: String]>

    private init(port: UInt16, listener: NWListener, queries: AsyncStream<[String: String]>) {
        self.port = port
        self.listener = listener
        self.queries = queries
    }

    /// `port` 0 picks a free one. `path` is the redirect path, e.g. "/callback".
    public static func start(port: UInt16 = 0, path: String) async throws -> LoopbackServer {
        // Bound to 127.0.0.1 itself, not just filtered by interface: nothing on the network can connect.
        let params = NWParameters.tcp
        params.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: port == 0 ? .any : NWEndpoint.Port(rawValue: port)!)
        params.allowLocalEndpointReuse = true
        let listener = try NWListener(using: params)
        let queue = DispatchQueue(label: "ihatecrypto.oauth.loopback")
        let (queries, sink) = AsyncStream.makeStream(of: [String: String].self)

        listener.newConnectionHandler = { connection in
            connection.start(queue: queue)
            connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { data, _, _, _ in
                // First line of the request: "GET /callback?code=…&state=… HTTP/1.1"
                let target = String(decoding: data ?? Data(), as: UTF8.self).split(separator: " ").dropFirst().first.map(String.init) ?? ""
                let url = URLComponents(string: "http://localhost" + target)
                let hit = url?.path == path
                connection.send(content: page(hit), completion: .contentProcessed { _ in connection.cancel() })
                if hit {
                    sink.yield(Dictionary((url?.queryItems ?? []).map { ($0.name, $0.value ?? "") }, uniquingKeysWith: { a, _ in a }))
                    sink.finish()
                }
            }
        }

        let bound: UInt16 = try await withCheckedThrowingContinuation { continuation in
            let resumed = OSAllocatedUnfairLock(initialState: false)
            listener.stateUpdateHandler = { state in
                let result: Result<UInt16, Error>? = switch state {
                case .ready: .success(listener.port?.rawValue ?? 0)
                case .failed(let error), .waiting(let error): .failure(LLMError("Couldn't open port \(port) for sign-in: \(error.localizedDescription)"))
                default: nil
                }
                guard let result, resumed.withLock({ done in defer { done = true }; return !done }) else { return }
                continuation.resume(with: result)
            }
            listener.start(queue: queue)
        }
        return LoopbackServer(port: bound, listener: listener, queries: queries)
    }

    /// Query parameters of the redirect. Stops listening afterwards.
    public func waitForCallback(timeout: TimeInterval = 300) async throws -> [String: String] {
        defer { listener.cancel() }
        let queries = queries
        return try await withThrowingTaskGroup(of: [String: String].self) { group in
            group.addTask {
                for await query in queries { return query }
                throw LLMError("Sign-in interrupted.")
            }
            group.addTask {
                try await Task.sleep(for: .seconds(timeout))
                throw LLMError("Sign-in didn't finish within \(Int(timeout / 60)) min.")
            }
            defer { group.cancelAll() }
            return try await group.next()!
        }
    }

    private static func page(_ ok: Bool) -> Data {
        let text = ok ? "Signed in. You can go back to ihatecrypto." : "Not found"
        let body = "<!doctype html><meta charset=utf-8><title>ihatecrypto</title><p style=\"font:16px -apple-system;margin:40px\">\(text)</p>"
        let head = "HTTP/1.1 \(ok ? "200 OK" : "404 Not Found")\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n"
        return Data((head + body).utf8)
    }
}

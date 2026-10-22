#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif
import Foundation
#if canImport(Network)
import Network
import os
#endif

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

#if canImport(Network)
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
#else
#if os(Windows)
import WinSDK
#elseif canImport(Glibc)
import Glibc
#endif

/// Off Apple: the same catch of one OAuth redirect, on a plain socket bound to 127.0.0.1 (WinSock on Windows), so the
/// browser sign-ins (ChatGPT, OpenRouter) work in the desktop app too.
public final class LoopbackServer: @unchecked Sendable {
    #if os(Windows)
    typealias Handle = SOCKET
    static let invalid = INVALID_SOCKET
    static func close(_ s: Handle) { closesocket(s) }
    #else
    typealias Handle = Int32
    static let invalid: Int32 = -1
    static func close(_ s: Handle) { _ = Glibc.close(s) }
    #endif

    public let port: UInt16
    private let listening: Handle
    private let queries: AsyncStream<[String: String]>

    private init(port: UInt16, listening: Handle, queries: AsyncStream<[String: String]>) {
        self.port = port
        self.listening = listening
        self.queries = queries
    }

    /// `port` 0 picks a free one. `path` is the redirect path, e.g. "/callback".
    public static func start(port: UInt16 = 0, path: String) async throws -> LoopbackServer {
        #if os(Windows)
        var wsa = WSADATA()
        _ = WSAStartup(0x0202, &wsa)
        let s = socket(AF_INET, SOCK_STREAM, Int32(IPPROTO_TCP.rawValue))
        #else
        let s = socket(AF_INET, Int32(SOCK_STREAM.rawValue), 0)
        #endif
        guard s != invalid else { throw LLMError("Couldn't open a socket for sign-in.") }
        var address = sockaddr_in()
        address.sin_family = numericCast(AF_INET)
        address.sin_port = port.bigEndian
        // Bound to 127.0.0.1 itself: nothing on the network can connect.
        #if os(Windows)
        address.sin_addr.S_un.S_addr = UInt32(0x7F00_0001).bigEndian
        #else
        address.sin_addr.s_addr = UInt32(0x7F00_0001).bigEndian
        #endif
        let size = MemoryLayout<sockaddr_in>.size
        let bound = withUnsafePointer(to: &address) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(s, $0, numericCast(size)) } }
        guard bound == 0, listen(s, 4) == 0 else {
            close(s)
            throw LLMError("Couldn't open port \(port) for sign-in: another program holds it.")
        }
        var actual = sockaddr_in()
        #if os(Windows)
        var length = Int32(size)
        #else
        var length = socklen_t(size)
        #endif
        _ = withUnsafeMutablePointer(to: &actual) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(s, $0, &length) } }
        let (queries, sink) = AsyncStream.makeStream(of: [String: String].self)
        Thread.detachNewThread {
            while true {
                let c = accept(s, nil, nil)
                guard c != invalid else { sink.finish(); return }
                var buffer = [UInt8](repeating: 0, count: 65_536)
                #if os(Windows)
                let got = buffer.withUnsafeMutableBytes { recv(c, $0.baseAddress?.assumingMemoryBound(to: CChar.self), Int32($0.count), 0) }
                #else
                let got = buffer.withUnsafeMutableBytes { recv(c, $0.baseAddress, $0.count, 0) }
                #endif
                // First line of the request: "GET /callback?code=…&state=… HTTP/1.1"
                let request = got > 0 ? String(decoding: buffer[0..<Int(got)], as: UTF8.self) : ""
                let target = request.split(separator: " ").dropFirst().first.map(String.init) ?? ""
                let url = URLComponents(string: "http://localhost" + target)
                let hit = url?.path == path
                let reply = [UInt8](page(hit))
                #if os(Windows)
                _ = reply.withUnsafeBytes { send(c, $0.baseAddress?.assumingMemoryBound(to: CChar.self), Int32($0.count), 0) }
                #else
                _ = reply.withUnsafeBytes { send(c, $0.baseAddress, $0.count, 0) }
                #endif
                close(c)
                if hit {
                    sink.yield(Dictionary((url?.queryItems ?? []).map { ($0.name, $0.value ?? "") }, uniquingKeysWith: { a, _ in a }))
                    sink.finish()
                    return
                }
            }
        }
        return LoopbackServer(port: UInt16(bigEndian: actual.sin_port), listening: s, queries: queries)
    }

    /// Query parameters of the redirect. Stops listening afterwards.
    public func waitForCallback(timeout: TimeInterval = 300) async throws -> [String: String] {
        defer { Self.close(listening) }
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
        let body = "<!doctype html><meta charset=utf-8><title>ihatecrypto</title><p style=\"font:16px system-ui;margin:40px\">\(text)</p>"
        let head = "HTTP/1.1 \(ok ? "200 OK" : "404 Not Found")\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n"
        return Data((head + body).utf8)
    }
}
#endif

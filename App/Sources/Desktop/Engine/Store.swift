import CWinSec
import Foundation
import LLMCore

/// The engine's own files, beside the signer's: %LOCALAPPDATA%\ihatecrypto\engine on Windows. Settings and the chat as
/// plain JSON (as the Mac keeps them in Application Support); model keys and sign-ins encrypted to the Windows user
/// (DPAPI), as the Mac keeps them in the Keychain.
nonisolated enum Store {
    static let folder: URL = {
        let env = ProcessInfo.processInfo.environment
        #if DEBUG
        if let dir = env["IHATECRYPTO_ENGINE_DIR"] { return made(URL(fileURLWithPath: dir, isDirectory: true)) }
        let product = "ihatecrypto-dev"
        #else
        let product = "ihatecrypto"
        #endif
        #if os(Windows)
        let base = env["LOCALAPPDATA"].map { URL(fileURLWithPath: $0, isDirectory: true) } ?? FileManager.default.temporaryDirectory
        #else
        // Off Windows only the development build runs (the window on a Mac, with the stand-in signer).
        let base = FileManager.default.temporaryDirectory
        #endif
        return made(base.appendingPathComponent(product, isDirectory: true).appendingPathComponent("engine", isDirectory: true))
    }()

    private static func made(_ url: URL) -> URL {
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    static func url(_ name: String) -> URL { folder.appendingPathComponent(name) }

    static func load<T: Decodable>(_ type: T.Type, _ name: String) -> T? {
        (try? Data(contentsOf: url(name))).flatMap { try? JSONDecoder().decode(T.self, from: $0) }
    }

    static func save<T: Encodable>(_ value: T, _ name: String) {
        guard let data = try? JSONEncoder().encode(value) else { return }
        try? data.write(to: url(name), options: .atomic)
    }

    // MARK: Model sign-ins

    /// Every provider's credential, one DPAPI blob.
    static func credentials() -> [String: Credential] {
        guard let blob = try? Data(contentsOf: url("credentials.bin")), let plain = unprotect(blob) else { return [:] }
        return (try? JSONDecoder().decode([String: Credential].self, from: plain)) ?? [:]
    }

    static func setCredential(_ credential: Credential?, for provider: String) {
        var all = credentials()
        all[provider] = credential
        guard let plain = try? JSONEncoder().encode(all), let blob = protect(plain) else { return }
        try? blob.write(to: url("credentials.bin"), options: .atomic)
    }

    private static func protect(_ data: Data) -> Data? { transform(data, winsec_protect) }
    private static func unprotect(_ data: Data) -> Data? { transform(data, winsec_unprotect) }

    private static func transform(_ data: Data, _ call: (UnsafePointer<UInt8>?, Int, UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?, UnsafeMutablePointer<Int>?) -> Int32) -> Data? {
        var out: UnsafeMutablePointer<UInt8>?
        var count = 0
        let status = data.withUnsafeBytes { call($0.bindMemory(to: UInt8.self).baseAddress, data.count, &out, &count) }
        guard status == WINSEC_OK, let out else { return nil }
        defer { winsec_free(out, count) }
        return Data(bytes: out, count: count)
    }
}

/// What the person chose, kept between launches.
nonisolated struct Settings: Codable, Equatable {
    /// «en», «ru» or «es»; nil: the system's.
    var language: String?
    var provider: String?
    var model: String?
    /// The second check (on unless turned off, as on the Mac): the model reads every command again, and a trade plan
    /// against the user's words, before a card is drawn.
    var secondCheck: Bool?
    /// The second check may also ask TypeSafe Jev through OpenRouter while the chat runs elsewhere: only when turned on.
    var jevOpenRouter: Bool?
}

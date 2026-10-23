import CryptoKit
import Foundation
import IhatecryptoCore
import Security

/// Scheduled tasks run by themselves from the automation wallet, and they live in the app's preferences, which any program
/// running as the user can rewrite (`defaults write`) — the 2026-10-24 review. Each save is tagged with an HMAC whose key
/// sits in this app's Keychain item (another program reading it gets a macOS prompt, not the key). A list whose tag doesn't
/// match is loaded paused. The key is made the first time; the tasks there are then are taken as they are, once.
nonisolated enum TaskSeal {
    static let service = Bundle.main.bundleIdentifier.map { $0 + ".task-seal" } ?? "app.ihatecrypto.task-seal"

    /// The key, and whether it was just made (nothing could be checked before it existed).
    static func key() -> (key: SymmetricKey, made: Bool)? {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "hmac"]
        var read = query
        read[kSecReturnData as String] = true
        var out: CFTypeRef?
        if SecItemCopyMatching(read as CFDictionary, &out) == errSecSuccess, let data = out as? Data, data.count == 32 {
            return (SymmetricKey(data: data), false)
        }
        let fresh = SymmetricKey(size: .bits256)
        var add = query
        add[kSecValueData as String] = fresh.withUnsafeBytes { Data($0) }
        guard SecItemAdd(add as CFDictionary, nil) == errSecSuccess else { return nil }
        return (fresh, true)
    }

    static func tag(_ data: Data, _ key: SymmetricKey) -> Data { Data(HMAC<SHA256>.authenticationCode(for: data, using: key)) }

    /// Saves the tasks' JSON with its tag.
    static func save(_ data: Data?, forKey k: String) {
        UserDefaults.standard.set(data, forKey: k)
        guard let data, let (key, _) = key() else { UserDefaults.standard.removeObject(forKey: k + ".seal"); return }
        UserDefaults.standard.set(tag(data, key), forKey: k + ".seal")
    }

    /// The tasks saved under `k`; `intact` false when their tag doesn't match (or is gone) after the key existed.
    static func load(forKey k: String) -> (data: Data?, intact: Bool) {
        let data = UserDefaults.standard.data(forKey: k)
        guard let data else { return (nil, true) }
        guard let (key, made) = key() else { return (data, false) }
        if made { UserDefaults.standard.set(tag(data, key), forKey: k + ".seal"); return (data, true) }
        guard let saved = UserDefaults.standard.data(forKey: k + ".seal") else { return (data, false) }
        return (data, tag(data, key) == saved)
    }

    /// The tasks under `k`, every one paused when their list was changed outside ihatecrypto.
    static func tasks(forKey k: String) -> (tasks: [ScheduledTask], tampered: Bool) {
        let (data, intact) = load(forKey: k)
        var tasks = data.flatMap { try? JSONDecoder().decode([ScheduledTask].self, from: $0) } ?? []
        if !intact { for i in tasks.indices { tasks[i].paused = true } }
        return (tasks, !intact && !tasks.isEmpty)
    }
}

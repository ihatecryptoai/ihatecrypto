import CryptoKit
import Foundation
import IhatecryptoCore
import Security

/// Scheduled tasks run by themselves from the automation wallet, and they live in the app's preferences, which any program
/// running as the user can rewrite (`defaults write`) — the 2026-10-24 review. Each save is tagged with an HMAC whose key
/// sits in this app's Keychain item (another program reading it gets a macOS prompt, not the key). A list whose tag doesn't
/// match is loaded paused. So is a list found when the key had to be made anew: a missing key used to bless whatever was
/// there, so deleting it let a rewritten list through (the 2026-11-13 review). The first save makes the key.
/// The exit rules of open positions (a trailing stop's distance and best price, which the guardian acts on with the
/// trading key) are sealed the same way; ones changed outside ihatecrypto aren't carried out.
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
        // A key made just now vouches for nothing already on disk.
        if made { return (data, false) }
        guard let saved = UserDefaults.standard.data(forKey: k + ".seal") else { return (data, false) }
        return (data, tag(data, key) == saved)
    }

    /// Exit rules saved before they were sealed are taken as they are once, on the first launch that seals them, for every
    /// wallet at once; a Keychain item says it was done. After that, rules with no seal were changed outside ihatecrypto.
    /// Only when the Keychain says plainly that the item isn't there, and only once it is written: a Keychain that doesn't
    /// answer, or doesn't take the item, adopts nothing (the rules then load as changed, and are set again).
    /// ponytail: a program that can delete this app's Keychain items can ask for that once again; the Keychain is the
    /// only place here another program can't simply write. Drop the adoption a few releases after the one that first seals them.
    static func adoptExitRules() {
        let done: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "exit-rules-sealed"]
        guard SecItemCopyMatching(done as CFDictionary, nil) == errSecItemNotFound, let (key, _) = key() else { return }
        var add = done
        add[kSecValueData as String] = Data([1])
        guard SecItemAdd(add as CFDictionary, nil) == errSecSuccess else { return }
        for (k, value) in UserDefaults.standard.dictionaryRepresentation() where k.hasPrefix("exitRules") && !k.hasSuffix(".seal") {
            if let data = value as? Data { UserDefaults.standard.set(tag(data, key), forKey: k + ".seal") }
        }
    }

    /// The exit rules under `k`: none when they were changed outside ihatecrypto (`tampered`), so the guardian moves no
    /// stop by them; the stops already on the exchange stay where they are.
    static func exitRules(forKey k: String) -> (rules: [String: ExitRules], tampered: Bool) {
        adoptExitRules()
        let (data, intact) = load(forKey: k)
        let rules = data.flatMap { try? JSONDecoder().decode([String: ExitRules].self, from: $0) } ?? [:]
        return intact ? (rules, false) : ([:], !rules.isEmpty)
    }

    /// The tasks under `k`, every one paused when their list was changed outside ihatecrypto.
    static func tasks(forKey k: String) -> (tasks: [ScheduledTask], tampered: Bool) {
        let (data, intact) = load(forKey: k)
        var tasks = data.flatMap { try? JSONDecoder().decode([ScheduledTask].self, from: $0) } ?? []
        // The prices Confirmed in them too: the Schedule doesn't show them, so one written in could pass unseen.
        if !intact { for i in tasks.indices { tasks[i].paused = true; tasks[i].prices = nil } }
        return (tasks, !intact && !tasks.isEmpty)
    }
}

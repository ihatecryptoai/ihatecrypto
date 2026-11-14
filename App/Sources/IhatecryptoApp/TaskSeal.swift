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
/// trading key) are sealed the same way; ones changed outside ihatecrypto aren't carried out. So are the recurring buys,
/// which ask to sign on the wallet that's on when they fall due: a list changed outside ihatecrypto loads paused.
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

    /// What a list's tag is made over. Exit rules and recurring buys: the name they are saved under and their bytes, so
    /// one wallet's sealed list copied under another wallet's name doesn't hold there. The tasks' tags were in use
    /// before that and cover the bytes alone: tying them to the name would pause every task once.
    /// ponytail: an older list put back with its own tag still holds (a counter in the Keychain would stop that).
    static func sealed(_ data: Data, _ k: String) -> Data { k.hasPrefix("tasks") ? data : Data(k.utf8) + data }

    /// Saves the tasks' JSON with its tag.
    static func save(_ data: Data?, forKey k: String) {
        UserDefaults.standard.set(data, forKey: k)
        guard let data, let (key, _) = key() else { UserDefaults.standard.removeObject(forKey: k + ".seal"); return }
        UserDefaults.standard.set(tag(sealed(data, k), key), forKey: k + ".seal")
    }

    /// The tasks saved under `k`; `intact` false when their tag doesn't match (or is gone) after the key existed.
    static func load(forKey k: String) -> (data: Data?, intact: Bool) {
        let data = UserDefaults.standard.data(forKey: k)
        guard let data else { return (nil, true) }
        guard let (key, made) = key() else { return (data, false) }
        // A key made just now vouches for nothing already on disk.
        if made { return (data, false) }
        guard let saved = UserDefaults.standard.data(forKey: k + ".seal") else { return (data, false) }
        return (data, tag(sealed(data, k), key) == saved)
    }

    /// Exit rules and recurring buys saved before they were sealed (every wallet's key starts with `prefix`) are taken as
    /// they are once, on the first launch that seals them, for every wallet at once; a Keychain item (`marker`, one for
    /// each of the two) says it was done. After that, ones with no seal were changed outside ihatecrypto.
    /// Only when the Keychain says plainly that the item isn't there, and only once it is written: a Keychain that doesn't
    /// answer, or doesn't take the item, adopts nothing (they then load as changed, and are set again).
    /// ponytail: a program that can delete this app's Keychain items can ask for that once again; the Keychain is the
    /// only place here another program can't simply write. Drop the adoption a few releases after the one that first seals them.
    static func adopt(_ prefix: String, marker: String) {
        let done: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: marker]
        guard SecItemCopyMatching(done as CFDictionary, nil) == errSecItemNotFound, let (key, _) = key() else { return }
        var add = done
        add[kSecValueData as String] = Data([1])
        guard SecItemAdd(add as CFDictionary, nil) == errSecSuccess else { return }
        for (k, value) in UserDefaults.standard.dictionaryRepresentation() where k.hasPrefix(prefix) && !k.hasSuffix(".seal") {
            if let data = value as? Data { UserDefaults.standard.set(tag(sealed(data, k), key), forKey: k + ".seal") }
        }
    }

    /// The exit rules under `k`: none when they were changed outside ihatecrypto (`tampered`), so the guardian moves no
    /// stop by them; the stops already on the exchange stay where they are.
    static func exitRules(forKey k: String) -> (rules: [String: ExitRules], tampered: Bool) {
        adopt("exitRules", marker: "exit-rules-bound")
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

    /// The recurring buys under `k`. They loaded from the preferences as they were, so a buy written in there asked to
    /// sign as the user's own when it fell due (the 2026-11-13 review).
    static func schedules(forKey k: String) -> (schedules: [Schedule], tampered: Bool) {
        adopt("schedules", marker: "schedules-bound")
        let (data, intact) = load(forKey: k)
        return schedules(data, intact: intact)
    }

    /// The saved buys, every one paused when their seal didn't hold. Paused, not dropped: the Schedule shows all there is
    /// to a buy (its coin, amount and period), so the user can tell their own and switch those on.
    static func schedules(_ data: Data?, intact: Bool) -> (schedules: [Schedule], tampered: Bool) {
        var buys = data.flatMap { try? JSONDecoder().decode([Schedule].self, from: $0) } ?? []
        if !intact { for i in buys.indices { buys[i].paused = true } }
        return (buys, !intact && !buys.isEmpty)
    }
}

#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif
import Foundation
import IhatecryptoCore

/// Scheduled tasks run by themselves from the automation wallet, and tasks.json is a file any program running as the user
/// can rewrite — the Mac's TaskSeal (the 2026-10-24 review). Each save is tagged with an HMAC; a list whose tag doesn't
/// match is loaded paused and said once. The key is made the first time; the tasks there then are taken as they are, once.
/// ponytail: the key is kept under DPAPI, which any program of the same Windows user can open (the Mac's Keychain asks
/// such a program first); the signer still sends the automation wallet's money only to the user's own wallets and
/// confirmed addresses. A key the signer holds, tagging the list itself, would close it.
nonisolated enum TaskSeal {
    static func key() -> (key: SymmetricKey, made: Bool)? {
        if let sealed = try? Data(contentsOf: Store.url("task-seal.key")), let raw = Store.unprotect(sealed), raw.count == 32 {
            return (SymmetricKey(data: raw), false)
        }
        let fresh = SymmetricKey(size: .bits256)
        guard let sealed = Store.protect(fresh.withUnsafeBytes { Data($0) }), (try? sealed.write(to: Store.url("task-seal.key"), options: .atomic)) != nil else { return nil }
        return (fresh, true)
    }

    static func tag(_ data: Data, _ key: SymmetricKey) -> Data { Data(HMAC<SHA256>.authenticationCode(for: data, using: key)) }

    static func save(_ tasks: [ScheduledTask]) {
        guard let data = try? JSONEncoder().encode(tasks) else { return }
        try? data.write(to: Store.url("tasks.json"), options: .atomic)
        guard let (key, _) = key() else { try? FileManager.default.removeItem(at: Store.url("tasks.seal")); return }
        try? tag(data, key).write(to: Store.url("tasks.seal"), options: .atomic)
    }

    /// The saved tasks, every one paused when their list was changed outside ihatecrypto.
    static func load() -> (tasks: [ScheduledTask], tampered: Bool) {
        guard let data = try? Data(contentsOf: Store.url("tasks.json")) else { return ([], false) }
        var tasks = (try? JSONDecoder().decode([ScheduledTask].self, from: data)) ?? []
        let intact: Bool
        if let (key, made) = key() {
            if made { try? tag(data, key).write(to: Store.url("tasks.seal"), options: .atomic); intact = true }
            else { intact = (try? Data(contentsOf: Store.url("tasks.seal"))) == tag(data, key) }
        } else { intact = false }
        if !intact { for i in tasks.indices { tasks[i].paused = true } }
        return (tasks, !intact && !tasks.isEmpty)
    }
}

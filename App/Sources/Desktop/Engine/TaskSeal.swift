#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif
import Foundation
import IhatecryptoCore

/// Scheduled tasks run by themselves from the automation wallet, and tasks.json is a file any program running as the user
/// can rewrite — the Mac's TaskSeal (the 2026-10-24 review). Each save is tagged with an HMAC; a list whose tag doesn't
/// match is loaded paused and said once. The key is made with the first list saved; a list found with no key beside it
/// had its key taken away, and loads paused too.
/// ponytail: the key is kept under DPAPI, which any program of the same Windows user can open and tag a list with (the
/// Mac's Keychain asks such a program first); such a program can read the automation wallet's seed as well (docs/windows.md
/// § Keys), and the signer still sends that wallet's money only to the user's own wallets and confirmed addresses. A key
/// the signer holds, tagging the list itself, would close it.
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

    /// «tasks.json» → «tasks.seal», a wallet's «tasks-3.json» → «tasks-3.seal»: each list has its own tag, one key for all.
    static func sealName(_ file: String) -> String { (file as NSString).deletingPathExtension + ".seal" }

    static func save(_ tasks: [ScheduledTask], _ file: String = "tasks.json") {
        guard let data = try? JSONEncoder().encode(tasks) else { return }
        try? data.write(to: Store.url(file), options: .atomic)
        guard let (key, _) = key() else { try? FileManager.default.removeItem(at: Store.url(sealName(file))); return }
        try? tag(data, key).write(to: Store.url(sealName(file)), options: .atomic)
    }

    /// The saved tasks, every one paused when their list was changed outside ihatecrypto.
    static func load(_ file: String = "tasks.json") -> (tasks: [ScheduledTask], tampered: Bool) {
        guard let data = try? Data(contentsOf: Store.url(file)) else { return ([], false) }
        var tasks = (try? JSONDecoder().decode([ScheduledTask].self, from: data)) ?? []
        let intact: Bool
        // A list with no key beside it: the key was deleted, so any list would pass with the next one made.
        if let (key, made) = key(), !made { intact = (try? Data(contentsOf: Store.url(sealName(file)))) == tag(data, key) }
        else { intact = tasks.isEmpty }
        if !intact { for i in tasks.indices { tasks[i].paused = true } }
        return (tasks, !intact && !tasks.isEmpty)
    }
}

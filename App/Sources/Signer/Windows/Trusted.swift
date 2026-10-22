import Foundation
#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif
import SignerAPI
import WalletCore

/// What the signer trusts that isn't a secret — the wallets and their addresses, which one signs, the address book,
/// the approved and own addresses, the session length, the trading keys' switches — in `state.json`, sealed with an
/// HMAC under the Windows Hello key. On a Mac the Keychain keeps them from other programs; on Windows any program of
/// the user can write the file, so the signer checks the seal every time Windows Hello opens a key (`verify`), and a
/// change made outside it stops every signature.
///
/// Changes that only take trust away — a wallet renamed or removed, an address removed, a trading key paused, a
/// shorter session — may be made with no key in hand: they go to `pending.json`, which is read only as far as it
/// takes trust away from the sealed state, and folded into the seal at the next prompt. Anything else asks Windows
/// Hello first.
enum Trusted {
    struct State: Codable, Equatable {
        var prefs: [String: Pref] = [:]
        var recipients: [String: [String]] = [:]
        var own: [String] = []
        var book: [SignerAddress] = []
    }

    enum Pref: Codable, Equatable {
        case string(String), bool(Bool), int(Int), data(Data)

        init?(_ value: Any) {
            switch value {
            case let v as String: self = .string(v)
            case let v as Bool: self = .bool(v)
            case let v as Int: self = .int(v)
            case let v as Data: self = .data(v)
            default: return nil
            }
        }

        var value: Any {
            switch self {
            case .string(let v): v
            case .bool(let v): v
            case .int(let v): v
            case .data(let v): v
            }
        }
    }

    struct File: Codable {
        var body: Data
        var mac: Data?
    }

    private static let lock = NSRecursiveLock()
    nonisolated(unsafe) private static var loaded: (sealed: State, effective: State)?

    static var state: State { lock.withLock { load().effective } }

    static func seal(_ body: Data, _ key: SymmetricKey) -> Data {
        Data(HMAC<SHA256>.authenticationCode(for: body, using: Vault.subkey(key, "state")))
    }

    private static func decode(_ name: String) -> (File, State)? {
        guard let data = Vault.read(name), let file = try? JSONDecoder().decode(File.self, from: data),
              let state = try? JSONDecoder().decode(State.self, from: file.body) else { return nil }
        return (file, state)
    }

    private static func load() -> (sealed: State, effective: State) {
        if let loaded { return loaded }
        let sealed = decode("state.json")?.1 ?? State()
        var effective = sealed
        if let (_, pending) = decode("pending.json"), takesTrustAway(from: sealed, to: pending) { effective = pending }
        loaded = (sealed, effective)
        return (sealed, effective)
    }

    private static func write(_ state: State, key: SymmetricKey?) throws {
        let body = try JSONEncoder().encode(state)
        try Vault.write("state.json", JSONEncoder().encode(File(body: body, mac: key.map { seal(body, $0) })))
        Vault.remove("pending.json")
        loaded = (state, state)
    }

    /// Checks the sealed state with the key Windows Hello just gave. `fresh`: that key was made just now, so nothing
    /// was sealed before: what the file holds then was written while no wallet needed Windows Hello, and only what such
    /// a time can hold is kept — the wallets whose keys are here; no approved, own or saved addresses (each of those
    /// needs Windows Hello, which would have made the key).
    static func verify(with key: SymmetricKey, fresh: Bool) throws {
        try lock.withLock {
            loaded = nil
            if fresh {
                var state = decode("state.json")?.1 ?? State()
                if case .data(let list)? = state.prefs["wallets"], let wallets = try? JSONDecoder().decode([SignerWallet].self, from: list) {
                    let kept = wallets.filter { Vault.exists(SeedStore.file($0.id)) }
                    state.prefs["wallets"] = (try? JSONEncoder().encode(kept)).map(Pref.data)
                    if case .string(let active)? = state.prefs["activeWallet"], !kept.contains(where: { $0.id == active }) {
                        for k in ["activeWallet", "solana", "evm", "near"] { state.prefs[k] = nil }
                    }
                }
                (state.recipients, state.own, state.book) = ([:], [], [])
                try write(state, key: key)
                return
            }
            guard let (file, sealed) = decode("state.json") else {
                if Vault.exists("state.json") { throw tampered }
                // Never written since the key was made: only a crash right after making it leaves this.
                try write(State(), key: key)
                return
            }
            guard let mac = file.mac, seal(file.body, key) == mac else { throw tampered }
            // What was changed with no key in hand is sealed now, as far as it only takes trust away.
            if let (_, pending) = decode("pending.json"), pending != sealed {
                try write(takesTrustAway(from: sealed, to: pending) ? pending : sealed, key: key)
            } else {
                loaded = (sealed, sealed)
            }
        }
    }

    static let tampered = WalletError(
        "The signer's protected settings — the wallets, the address book, the approved addresses — were changed outside it, by a program on this computer or a damaged file. Nothing was signed. Check this computer for malware; the recovery phrases restore your wallets on a clean one.")

    /// Changes the trusted state. With a key in hand (this request's, or an open session's) or before any wallet needs
    /// Windows Hello, it's sealed at once; a change that only takes trust away waits in `pending.json`; anything else
    /// asks Windows Hello first (`reason`; nil: it isn't made).
    static func mutate(_ reason: String?, _ change: (inout State) -> Void) throws {
        let (before, current, next): (State, State, State) = lock.withLock {
            var next = load().effective
            change(&next)
            return (load().sealed, load().effective, next)
        }
        if next == current { return }
        if let key = Vault.currentKey { return try lock.withLock { try write(next, key: key) } }
        if Vault.master() == nil { return try lock.withLock { try write(next, key: nil) } }
        if takesTrustAway(from: before, to: next) {
            return try lock.withLock {
                let body = try JSONEncoder().encode(next)
                try Vault.write("pending.json", JSONEncoder().encode(File(body: body, mac: nil)))
                loaded = (before, next)
            }
        }
        guard let reason else { return }
        try Vault.confirm(reason)
        guard let key = Vault.currentKey else { throw WalletError("Not confirmed: nothing was changed.") }
        try lock.withLock { try write(next, key: key) }
    }

    /// `new` gives nothing `old` didn't: wallets only renamed or gone, addresses only gone, trading keys only paused,
    /// a session no longer — and every other setting the same.
    static func takesTrustAway(from old: State, to new: State) -> Bool {
        for (id, list) in new.recipients where !Set(list).isSubset(of: Set(old.recipients[id] ?? [])) { return false }
        guard Set(new.own).isSubset(of: Set(old.own)), Set(new.book).isSubset(of: Set(old.book)) else { return false }
        for key in Set(old.prefs.keys).union(new.prefs.keys) {
            let (was, now) = (old.prefs[key], new.prefs[key])
            if was == now { continue }
            switch key {
            case "wallets":
                func list(_ p: Pref?) -> [SignerWallet] {
                    if case .data(let d)? = p { return (try? JSONDecoder().decode([SignerWallet].self, from: d)) ?? [] }
                    return []
                }
                let before = list(was)
                for w in list(now) {
                    guard var match = before.first(where: { $0.id == w.id }) else { return false }
                    match.label = w.label
                    guard match == w else { return false }
                }
            case "sessionLength":
                func seconds(_ p: Pref?) -> Int { if case .int(let v)? = p { v } else { SignerSession.standard } }
                guard seconds(now) <= seconds(was) else { return false }
            case let k where k.hasPrefix("agentActive"):
                guard now == nil || now == .bool(false) || now == .string("paused") else { return false }
            default:
                return false
            }
        }
        return true
    }

    /// For tests: forget what was read, as a new signer process would.
    static func reload() { lock.withLock { loaded = nil } }
}

/// The Mac's SignerPrefs, on Windows: the same calls over the trusted state.
final class SignerPrefs: @unchecked Sendable {
    static let shared = SignerPrefs()

    func object(forKey key: String) -> Any? { Trusted.state.prefs[key]?.value }
    func string(forKey key: String) -> String? { object(forKey: key) as? String }
    func data(forKey key: String) -> Data? { object(forKey: key) as? Data }
    func bool(forKey key: String) -> Bool { object(forKey: key) as? Bool ?? false }
    func integer(forKey key: String) -> Int { object(forKey: key) as? Int ?? 0 }
    func dictionaryRepresentation() -> [String: Any] { Trusted.state.prefs.mapValues(\.value) }

    /// nil removes the key. A change that gives trust (a new wallet, another one signing, a trading key on) with no key
    /// in hand asks Windows Hello first; turned down, the change isn't made.
    func set(_ value: Any?, forKey key: String) {
        do { try Trusted.mutate("change what the signer trusts: \(key)") { $0.prefs[key] = value.flatMap(Trusted.Pref.init) } }
        catch { FileHandle.standardError.write(Data("ihatecrypto signer: \(key) not changed: \(error)\n".utf8)) }
    }

    func removeObject(forKey key: String) { set(nil, forKey: key) }
}

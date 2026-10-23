import CWinSec
import Foundation
#if canImport(CryptoKit)
import CryptoKit
#else
import Crypto
#endif
import SignerAPI
import WalletCore

/// The signer's files on Windows, in %LOCALAPPDATA%\ihatecrypto\signer, and the Windows Hello key that guards them.
///
/// A wallet's seed is sealed with AES-GCM under a key derived from a Windows Hello signature: Windows Hello signs a
/// random challenge with an RSA key in the TPM after the face, the fingerprint or the PIN, and the signature is the same
/// for the same challenge, so it can be derived again only with the person (KeePassXC's way). On top, the file is
/// encrypted to the Windows user (DPAPI). Every key that opens a seed comes from one signature (`master`), so one
/// Windows Hello prompt covers one move, as Touch ID does on a Mac. A test or automation wallet, made to sign with
/// no one there, is under DPAPI only — as on a Mac it is sealed without Touch ID.
enum Vault {
    static let folder: URL = {
        let env = ProcessInfo.processInfo.environment
        #if DEBUG
        if let dir = env["IHATECRYPTO_SIGNER_DIR"] { return made(URL(fileURLWithPath: dir, isDirectory: true)) }
        #endif
        #if os(Windows)
        let base = env["LOCALAPPDATA"].map { URL(fileURLWithPath: $0, isDirectory: true) } ?? FileManager.default.temporaryDirectory
        #else
        // Off Windows only the stand-in for Windows Hello runs (tests, the UI on a Mac): never beside a real wallet.
        let base = FileManager.default.temporaryDirectory
        #endif
        return made(base.appendingPathComponent(product, isDirectory: true).appendingPathComponent("signer", isDirectory: true))
    }()

    private static func made(_ url: URL) -> URL {
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    #if DEBUG
    static let product = "ihatecrypto-dev"
    #else
    static let product = "ihatecrypto"
    #endif
    /// The Windows Hello key: one per Windows user and copy of the app.
    static let helloKey = product + "-signer"

    static func url(_ name: String) -> URL { folder.appendingPathComponent(name) }
    static func read(_ name: String) -> Data? { try? Data(contentsOf: url(name)) }
    static func exists(_ name: String) -> Bool { FileManager.default.fileExists(atPath: url(name).path) }
    /// Written whole or not at all: a temporary file renamed over the old one.
    static func write(_ name: String, _ data: Data) throws { try data.write(to: url(name), options: .atomic) }
    static func remove(_ name: String) { try? FileManager.default.removeItem(at: url(name)) }

    // MARK: DPAPI

    static func protect(_ data: Data) throws -> Data {
        try transform(data, winsec_protect, "Windows didn't encrypt the signer's file")
    }

    static func unprotect(_ data: Data) throws -> Data {
        try transform(data, winsec_unprotect, "Windows didn't open the signer's file: it belongs to another Windows user or was damaged")
    }

    private static func transform(_ data: Data, _ call: (UnsafePointer<UInt8>?, Int, UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?, UnsafeMutablePointer<Int>?) -> Int32,
                                  _ failure: String) throws -> Data {
        var out: UnsafeMutablePointer<UInt8>?
        var count = 0
        let status = data.withUnsafeBytes { call($0.bindMemory(to: UInt8.self).baseAddress, data.count, &out, &count) }
        guard status == WINSEC_OK, let out else { throw WalletError(failure + ".") }
        defer { winsec_free(out, count) }
        return Data(bytes: out, count: count)
    }

    // MARK: Windows Hello

    /// The person's Windows Hello signature of `challenge`, with `reason` in the signer's window meanwhile.
    static func helloSign(_ challenge: Data, reason: String, create: Bool) throws -> Data {
        var out: UnsafeMutablePointer<UInt8>?
        var count = 0
        let name = Array(helloKey.utf8), text = Array(reason.utf8)
        let status = challenge.withUnsafeBytes { c in
            winsec_hello_sign(name, name.count, c.bindMemory(to: UInt8.self).baseAddress, challenge.count, text, text.count, create ? 1 : 0, &out, &count)
        }
        switch status {
        case WINSEC_OK:
            guard let out else { throw WalletError("Windows Hello gave no signature: nothing was signed.") }
            defer { winsec_free(out, count) }
            return Data(bytes: out, count: count)
        case WINSEC_CANCELLED: throw WalletError("Cancelled, or Windows Hello didn't match: nothing was signed.")
        case WINSEC_NOT_FOUND:
            throw WalletError("The Windows Hello key that seals your wallets is gone (Windows was reset, or its PIN removed): add the wallets again from their recovery phrases.")
        case WINSEC_UNAVAILABLE:
            throw WalletError("Set up Windows Hello first (Settings › Accounts › Sign-in options: a PIN at least): it guards every wallet that holds money.")
        case WINSEC_LOCKED: throw WalletError("Windows Hello is locked after too many tries: unlock Windows with your PIN, then try again.")
        default: throw WalletError("Windows Hello didn't answer: nothing was signed.")
        }
    }

    /// The challenge Windows Hello signs and a check of the key it gives, kept with every sealed seed too, so a lost
    /// `master.json` costs nothing.
    struct Master: Codable, Equatable {
        var challenge: Data
        var check: Data
    }

    static func master() -> Master? {
        if let data = read("master.json"), let m = try? JSONDecoder().decode(Master.self, from: data) { return m }
        for file in (try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? [] where file.hasSuffix(".seed") {
            if let sealed = try? SeedStore.sealed(file), let challenge = sealed.challenge, let check = sealed.check {
                return Master(challenge: challenge, check: check)
            }
        }
        return nil
    }

    static func masterKey(_ signature: Data, challenge: Data) -> SymmetricKey {
        HKDF<SHA256>.deriveKey(inputKeyMaterial: SymmetricKey(data: signature), salt: challenge, info: Data("ihatecrypto master".utf8), outputByteCount: 32)
    }

    static func checkValue(_ key: SymmetricKey) -> Data {
        Data(HMAC<SHA256>.authenticationCode(for: Data("ihatecrypto check".utf8), using: key))
    }

    /// A key for one purpose («seed 1», «state»), from the master key.
    static func subkey(_ master: SymmetricKey, _ label: String) -> SymmetricKey {
        HKDF<SHA256>.deriveKey(inputKeyMaterial: master, info: Data(label.utf8), outputByteCount: 32)
    }

    /// Windows Hello now, whatever session is open: the master key it gives. The first time (`fresh`), it makes the
    /// key in Windows Hello (one prompt) and signs (another).
    static func unlock(reason: String) throws -> (key: SymmetricKey, fresh: Bool) {
        if let m = master() {
            var signature = try helloSign(m.challenge, reason: reason, create: false)
            defer { SeedStore.wipe(&signature) }
            let key = masterKey(signature, challenge: m.challenge)
            guard checkValue(key) == m.check else {
                throw WalletError("Windows Hello answered with another key than the one that sealed your wallets: nothing was opened.")
            }
            return (key, false)
        }
        var challenge = Data(count: 32)
        challenge.withUnsafeMutableBytes { b in
            var rng = SystemRandomNumberGenerator()
            for i in b.indices { b[i] = rng.next() }
        }
        var signature = try helloSign(challenge, reason: reason, create: true)
        defer { SeedStore.wipe(&signature) }
        let key = masterKey(signature, challenge: challenge)
        try write("master.json", JSONEncoder().encode(Master(challenge: challenge, check: checkValue(key))))
        return (key, true)
    }

    // MARK: Sessions

    /// What one request has opened: the trusted settings it changes are sealed with it even after `endSession`, which
    /// only stops the next request from reusing it.
    final class Scope: @unchecked Sendable { var key: SymmetricKey? }
    @TaskLocal static var scope: Scope?

    nonisolated(unsafe) private static var session: (key: SymmetricKey, since: Date)?
    private static let lock = NSLock()

    /// The key of this request or of a session still open; nil when it would take a prompt.
    static var currentKey: SymmetricKey? {
        if let key = scope?.key { return key }
        return lock.withLock {
            guard let s = session, Date().timeIntervalSince(s.since) < TimeInterval(SeedStore.sessionLength) else { return nil }
            return s.key
        }
    }

    /// The master key for a move: the open session's, or Windows Hello with `reason`. The trusted settings are checked
    /// against it before it is handed out: a key never serves settings changed outside the signer.
    static func key(reason: String, session useSession: Bool = true) throws -> SymmetricKey {
        if useSession, let key = currentKey {
            // The file may have changed since the session opened: checked again before the key serves.
            try Trusted.verify(with: key, fresh: false)
            scope?.key = key
            return key
        }
        let (key, fresh) = try unlock(reason: reason)
        try Trusted.verify(with: key, fresh: fresh)
        if useSession { lock.withLock { session = (key, Date()) } }
        scope?.key = key
        return key
    }

    /// Windows Hello again, always: a fresh confirmation, whose key then serves the signature that follows.
    static func confirm(_ reason: String) throws {
        let (key, fresh) = try unlock(reason: reason)
        try Trusted.verify(with: key, fresh: fresh)
        lock.withLock { session = (key, Date()) }
        scope?.key = key
    }

    static func endSession() { lock.withLock { session = nil } }
}

/// The Mac's SeedStore, on Windows: the same calls Signer makes, over Vault's files.
enum SeedStore {
    static func wipe(_ data: inout Data) {
        let count = data.count
        data.withUnsafeMutableBytes { b in if let p = b.baseAddress { winsec_wipe(p.assumingMemoryBound(to: UInt8.self), count) } }
    }

    static func file(_ id: String) -> String { "wallet-\(id).seed" }

    /// A seed as it is kept: inside DPAPI, `box` is AES-GCM under the Windows Hello key (`presence`), or the seed
    /// itself for a wallet that signs with no prompt.
    struct Sealed: Codable {
        var version = 1
        var presence: Bool
        var challenge: Data?
        var check: Data?
        var box: Data
    }

    static func sealed(_ name: String) throws -> Sealed? {
        guard let file = Vault.read(name) else { return nil }
        var json = try Vault.unprotect(file)
        defer { wipe(&json) }
        return try JSONDecoder().decode(Sealed.self, from: json)
    }

    static var firstPresent: Bool { Vault.exists(file(SignerWallet.firstID)) }

    static var sessionLength: Int {
        let chosen = Signer.defaults.integer(forKey: "sessionLength")
        return SignerSession.choices.contains(chosen) ? chosen : SignerSession.standard
    }

    /// Seals the seed. Release builds ask Windows Hello for it except for a test wallet, as a Mac asks Touch ID.
    static func save(_ seed: Data, requirePresence: Bool, id: String, test: Bool = false) throws {
        #if DEBUG
        let presence = requirePresence
        #else
        let presence = requirePresence || !test
        #endif
        var sealed: Sealed
        if presence {
            let key = try Vault.key(reason: "keep this wallet's key under Windows Hello: it opens only with you")
            guard let m = Vault.master(), let box = try AES.GCM.seal(seed, using: Vault.subkey(key, "seed \(id)")).combined else {
                throw WalletError("Couldn't seal the key.")
            }
            sealed = Sealed(presence: true, challenge: m.challenge, check: m.check, box: box)
        } else {
            sealed = Sealed(presence: false, box: seed)
        }
        var json = try JSONEncoder().encode(sealed)
        defer { wipe(&json); wipe(&sealed.box) }
        try Vault.write(file(id), Vault.protect(json))
        endSession()
    }

    /// The seed, after Windows Hello or within its session. `session: false` (the automation wallet's) never reuses
    /// a session nor leaves one; that wallet opens with no prompt anyway.
    static func load(reason: String, id: String, session useSession: Bool = true) throws -> Data {
        guard let sealed = try sealed(file(id)) else { throw WalletError("No wallet imported.") }
        var box = sealed.box
        guard sealed.presence else { return box }
        defer { wipe(&box) }
        let key = try Vault.key(reason: reason, session: useSession)
        do { return try AES.GCM.open(AES.GCM.SealedBox(combined: box), using: Vault.subkey(key, "seed \(id)")) }
        catch { throw WalletError("This wallet's sealed key didn't open with Windows Hello's: nothing was signed.") }
    }

    /// A wallet's seed with no prompt, or nil: only a test or the automation wallet opens so.
    static func withoutPrompt(id: String) -> Data? {
        guard let sealed = try? sealed(file(id)), !sealed.presence else { return nil }
        return sealed.box
    }

    /// Windows Hello with `reason`, always a fresh prompt.
    /// ponytail: `within` (an automation wallet's prompt closing by itself) isn't kept on Windows: the prompt stays
    /// until someone answers it; cancel the Windows Hello request from the Rust side if a task must not wait.
    static func confirm(_ reason: String, within: TimeInterval? = nil) throws { try Vault.confirm(reason) }

    /// A confirmation in person, for the passcode (`CodeStore.set`), which Windows doesn't keep.
    struct Confirmation {}
    static var confirmed: Confirmation? { nil }
    static func startCodeSession(_ confirmation: Confirmation) {}

    static func delete(id: String) {
        endSession()
        Vault.remove(file(id))
    }

    static func endSession() { Vault.endSession() }
}

/// The app's 6-digit passcode, as the Mac's: it opens the lock screen. Windows Hello signs every move here, so the code
/// never does. Kept in code.json under DPAPI: a salt, the code stretched with it (PBKDF2-HMAC-SHA256) and the wrong tries
/// in a row. A try is counted before it is checked, so tries side by side or a signer stopped mid-try get no extra
/// guess; the fifth wrong one in a row deletes it all, and then only Windows Hello opens the app.
/// ponytail: a program of this Windows user can open code.json (DPAPI is the user's) and guess the 10⁶ codes offline —
/// the code hides the window from a person at the PC, as on a Mac; a TPM key with a PIN (NCrypt's Platform Crypto
/// Provider, the TPM's own lockout) if it must stand against such a program.
enum CodeStore {
    static let tries = 5
    static let rounds = 20_000
    /// Windows Hello always asks before a signature: a code set here only opens the app.
    static var canSign: Bool { false }
    static var signs: Bool { false }
    /// What opens the app without the code.
    static let fallback = "Windows Hello"
    enum Entered { case right(SeedStore.Confirmation), wrong(left: Int), off, uncounted }

    struct Record: Codable {
        var salt: Data
        var hash: Data
        var failures: Int
    }

    /// One code operation at a time — a try, a new code, turning it off — so none sees another half done.
    private static let entering = NSRecursiveLock()

    private static func record() -> Record? {
        guard let sealed = Vault.read("code.json"), let data = try? Vault.unprotect(sealed) else { return nil }
        return try? JSONDecoder().decode(Record.self, from: data)
    }

    private static func save(_ record: Record) -> Bool {
        (try? Vault.write("code.json", Vault.protect(JSONEncoder().encode(record)))) != nil
    }

    /// A damaged or unreadable file counts as no code: the lock then opens only with Windows Hello, never without it.
    static var isSet: Bool { record() != nil }

    static func stretch(_ code: String, _ salt: Data) -> Data {
        let key = SymmetricKey(data: Data(code.utf8))
        var u = Data(HMAC<SHA256>.authenticationCode(for: salt + Data([0, 0, 0, 1]), using: key))
        var t = [UInt8](u)
        for _ in 1..<rounds {
            u = Data(HMAC<SHA256>.authenticationCode(for: u, using: key))
            for (i, b) in u.enumerated() { t[i] ^= b }
        }
        return Data(t)
    }

    /// A new code in place of any earlier one (the signer asked Windows Hello first).
    static func set(_ code: String, presence: SeedStore.Confirmation?) throws {
        try entering.withLock {
            var rng = SystemRandomNumberGenerator()
            let salt = Data((0..<16).map { _ in UInt8.random(in: 0...255, using: &rng) })
            guard save(Record(salt: salt, hash: stretch(code, salt), failures: 0)) else { throw WalletError("Windows didn't save the passcode.") }
        }
    }

    static func enter(_ code: String) -> Entered {
        entering.withLock {
            guard var r = record(), r.failures < tries else { clear(); return .off }
            r.failures += 1
            guard save(r) else { return .uncounted }
            let typed = Passcode.problem(code) == nil ? stretch(code, r.salt) : Data()
            if typed.count == r.hash.count, zip(typed, r.hash).reduce(0, { $0 | ($1.0 ^ $1.1) }) == 0 {
                r.failures = 0
                _ = save(r)
                return .right(SeedStore.Confirmation())
            }
            guard r.failures < tries else { clear(); return .off }
            return .wrong(left: tries - r.failures)
        }
    }

    static func clear() { entering.withLock { Vault.remove("code.json") } }
}

/// Addresses outside the wallet the user allowed money to go to, per wallet, each confirmed in person once.
enum RecipientStore {
    /// The two lists, as on a Mac: money recipients, and contracts whose calls the signer can't read, allowed as calls
    /// only — a call allowed once doesn't make the contract a place money or a spending right may go.
    static let money = "approved", calls = "calls"

    static func all(_ list: String = money) -> Set<String> {
        let id = Wallets.activeID
        return Set((list == calls ? Trusted.state.calls?[id] : Trusted.state.recipients[id]) ?? [])
    }

    static func add(_ addresses: [String], to list: String = money) throws {
        let id = Wallets.activeID, new = addresses.map(key)
        try Trusted.mutate(list == calls ? "allow a contract call ihatecrypto can't read" : "allow money to go to a new address") { s in
            if list == calls {
                var all = s.calls ?? [:]
                all[id] = Array(Set(all[id] ?? []).union(new)).sorted()
                s.calls = all
            } else {
                s.recipients[id] = Array(Set(s.recipients[id] ?? []).union(new)).sorted()
            }
        }
    }

    /// EVM addresses compare without case; Solana's base58 is case-sensitive.
    static func key(_ address: String) -> String { address.hasPrefix("0x") ? address.lowercased() : address }

    static func delete(id: String) {
        try? Trusted.mutate("forget a wallet's approved addresses") { s in
            s.recipients[id] = nil
            s.calls?[id] = nil
        }
    }
}

/// The addresses of the wallets whose keys this signer holds, written when each key was derived here.
enum OwnStore {
    static func all() -> Set<String> { Set(Trusted.state.own) }

    /// Best effort, as on a Mac: without a key in hand (a test wallet signing) it's left for the next time.
    static func add(_ addresses: [String]) {
        let new = Set(addresses.map(RecipientStore.key)).subtracting(all())
        guard !new.isEmpty, Vault.currentKey != nil || Vault.master() == nil else { return }
        try? Trusted.mutate(nil) { $0.own = Array(Set($0.own).union(new)).sorted() }
    }

    static func remove(_ addresses: [String]) {
        let gone = Set(addresses.map(RecipientStore.key))
        try? Trusted.mutate(nil) { $0.own.removeAll { gone.contains($0) } }
    }
}

/// The address book's names and addresses: changed only after Windows Hello with every address shown.
enum AddressBookStore {
    static func all() -> [SignerAddress] { Trusted.state.book }
    static func save(_ book: [SignerAddress]) throws { try Trusted.mutate("save to your address book") { $0.book = book } }
}

/// The Hyperliquid trading key: DPAPI only, no prompt — it trades but can't withdraw.
enum AgentStore {
    static func file(_ id: String) -> String { "agent-\(Wallets.scoped("hyperliquid", id)).key" }

    static func load(id: String = Wallets.activeID) -> Data? { Vault.read(file(id)).flatMap { try? Vault.unprotect($0) } }

    /// 32 random bytes that make a valid secp256k1 key; retried in the vanishing case they don't.
    static func make() throws -> Data {
        var rng = SystemRandomNumberGenerator()
        for _ in 0..<4 {
            let bytes = Data((0..<32).map { _ in UInt8.random(in: 0...255, using: &rng) })
            if (try? EVMAccount.address(privateKey: bytes)) != nil { return bytes }
        }
        throw WalletError("Couldn't create the agent key.")
    }

    static func save(_ key: Data, id: String = Wallets.activeID) throws { try Vault.write(file(id), Vault.protect(key)) }
    static func delete(id: String) { Vault.remove(file(id)) }
}

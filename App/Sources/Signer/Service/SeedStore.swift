import Foundation
import LocalAuthentication
import Security
import SignerAPI
import WalletCore

/// The seed in the data-protection Keychain: this device only, never synced, readable only by the signer's own
/// access group. Since 2026-09-28 it is sealed to a Secure Enclave key: the Keychain holds only ciphertext, and
/// opening it takes the Enclave, which asks for Touch ID or the login password. Code that manages to read the
/// Keychain item — a re-signed copy of the signer — gets bytes it cannot open without the user.
/// A wallet saved before then sits unsealed under «default» until it is replaced.
/// After each signature the opened seed and the key derived from it are overwritten (`wipe`, see `Signer.withKey`).
/// ponytail: copies made inside the derivation and the secp256k1 library aren't reachable from here, so the guard
/// against reading them is still that no debugger can attach to the signer, Release or Debug (no get-task-allow).
enum SeedStore {
    /// Overwrites key bytes in place before they're released; memset_s is never optimized away. Call it on the last
    /// reference: a Data shared with another copy is duplicated first, and only the duplicate would be cleared.
    static func wipe(_ data: inout Data) {
        data.withUnsafeMutableBytes { b in if let p = b.baseAddress { _ = memset_s(p, b.count, 0, b.count) } }
    }

    static let service = "app.ihatecrypto.seed"

    static func item(_ account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    static let sealedAccount = "sealed", legacyAccount = "default"
    /// Wallet «1» keeps the item it had before there were several wallets; the others get their own.
    static func account(_ id: String) -> String { id == SignerWallet.firstID ? sealedAccount : "wallet-\(id)" }
    static let algorithm = SecKeyAlgorithm.eciesEncryptionCofactorVariableIVX963SHA256AESGCM

    /// Each sealing makes its own Enclave key; the sealed item names it (kSecAttrGeneric), so a new wallet is
    /// written before the old one and its key are removed — a failure halfway never leaves no wallet at all.
    static func keyQuery(_ tag: Data) -> [String: Any] {
        [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    /// The sealed seed and the tag of the key that opens it.
    private static func sealed(_ account: String) -> (data: Data, tag: Data)? {
        var q = item(account)
        q[kSecReturnData as String] = true
        q[kSecReturnAttributes as String] = true
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let found = out as? [String: Any],
              let data = found[kSecValueData as String] as? Data, let tag = found[kSecAttrGeneric as String] as? Data else { return nil }
        return (data, tag)
    }

    /// The one seed ihatecrypto held before several wallets, sealed or from before sealing: what wallet «1» migrates from.
    static var firstPresent: Bool { present(sealedAccount) || present(legacyAccount) }

    private static func present(_ account: String) -> Bool {
        var q = item(account)
        q[kSecReturnAttributes as String] = true
        let context = LAContext()
        context.interactionNotAllowed = true
        q[kSecUseAuthenticationContext as String] = context
        let status = SecItemCopyMatching(q as CFDictionary, nil)
        return status == errSecSuccess || status == errSecInteractionNotAllowed
    }

    /// Seals the seed to a new Enclave key. Release builds ask for Touch ID or the password to open it, except for a
    /// test wallet (made fresh in the app, for small amounts); Debug builds may skip it for the automated self-test
    /// with the public test phrase.
    static func save(_ seed: Data, requirePresence: Bool, id: String, test: Bool = false) throws {
        let account = account(id)
        #if DEBUG
        let presence = requirePresence
        #else
        let presence = requirePresence || !test
        #endif
        var error: Unmanaged<CFError>?
        func fail(_ what: String) -> WalletError { WalletError("\(what): \(error.map { "\($0.takeRetainedValue())" } ?? "?")") }
        guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                                                           presence ? [.privateKeyUsage, .userPresence] : [.privateKeyUsage], &error) else {
            throw fail("Couldn't set access to the key")
        }
        let tag = Data("app.ihatecrypto.seed.key.\(UUID().uuidString)".utf8)
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecUseDataProtectionKeychain as String: true,
            kSecPrivateKeyAttrs as String: [
                kSecAttrIsPermanent as String: true,
                kSecAttrApplicationTag as String: tag,
                kSecAttrAccessControl as String: access,
            ] as [String: Any],
        ]
        guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &error) else { throw fail("The Secure Enclave didn't make a key") }
        guard let publicKey = SecKeyCopyPublicKey(key),
              let ciphertext = SecKeyCreateEncryptedData(publicKey, algorithm, seed as CFData, &error) as Data? else {
            SecItemDelete(keyQuery(tag) as CFDictionary)
            throw fail("Couldn't seal the key")
        }
        // Written over the same wallet's old seal in one step; only then is the old key removed.
        let old = sealed(account)?.tag
        let values: [String: Any] = [kSecValueData as String: ciphertext, kSecAttrGeneric as String: tag]
        var status = SecItemUpdate(item(account) as CFDictionary, values as CFDictionary)
        if status == errSecItemNotFound {
            var q = item(account).merging(values) { $1 }
            q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            status = SecItemAdd(q as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            SecItemDelete(keyQuery(tag) as CFDictionary)
            throw WalletError("Keychain didn't save the key: \(status) \(SecCopyErrorMessageString(status, nil) as String? ?? "")")
        }
        endSession()
        if let old { SecItemDelete(keyQuery(old) as CFDictionary) }
        if id == SignerWallet.firstID { SecItemDelete(item(legacyAccount) as CFDictionary) }
        // A wallet that asks in person is sealed under the passcode too, when there is one and it signs here (no Touch
        // ID): the code was set in person.
        if presence, !Presence.touchID { CodeStore.seal(seed, id: id) }
    }

    /// One Touch ID per move, not per signature: a context that already authenticated opens the key again
    /// without a prompt, for at most `sessionLength` (1 minute unless the user picked 5, 15 or 30). With the standard
    /// minute the app ends it as soon as the move is done (`endSession`). Every signature still passes the guard;
    /// a locked screen or sleep ends the session.
    /// `byCode`: the passcode opened it (`startCodeSession`), so signatures open the seeds' passcode seals.
    nonisolated(unsafe) private static var session: (context: LAContext, since: Date, byCode: Bool)?

    static var sessionLength: Int {
        let chosen = Signer.defaults.integer(forKey: "sessionLength")
        return SignerSession.choices.contains(chosen) ? chosen : SignerSession.standard
    }
    private static let lock = NSLock()

    /// `session: false` — a key sealed without Touch ID (the automation wallet's) opens on a context of its own: it
    /// neither reuses the Touch ID session nor leaves its reason on it for the next prompt.
    /// On a Mac without Touch ID that has a passcode, a wallet the code seals throws `CodeNeeded` instead of asking macOS:
    /// the app asks for the code, and `.enterCode` opens the session this then uses.
    static func load(reason: String, id: String, session useSession: Bool = true) throws -> Data {
        let (opened, byCode): (LAContext, Bool) = try lock.withLock {
            let fresh = LAContext()
            fresh.localizedReason = reason
            guard useSession else { return (fresh, false) }
            // A code session counts only where the code signs: with Touch ID the sensor signs, whatever was typed.
            if let s = session, Date().timeIntervalSince(s.since) < TimeInterval(sessionLength), !s.byCode || CodeStore.signs { return (s.context, s.byCode) }
            if CodeStore.signs, CodeStore.sealed(id) != nil { throw CodeNeeded(reason: reason) }
            session = (fresh, Date(), false)
            return (fresh, false)
        }
        var context = opened
        do {
            if byCode {
                if let sealed = CodeStore.sealed(id) { return try open(sealed, context: context) }
                // A wallet the code doesn't seal (a test wallet, or one added before the code) asks as it always did.
                context = LAContext()
                context.localizedReason = reason
            }
            if let sealed = sealed(account(id)) { return try open(sealed, context: context) }
            guard id == SignerWallet.firstID, let seed = read(legacyAccount, context: context) else { throw WalletError("No wallet imported.") }
            sealLegacy(seed, id: id)
            return seed
        } catch {
            // A cancelled or failed prompt must not be reused; a code session stays when only another wallet's prompt failed.
            if useSession, context === opened { lock.withLock { session = nil } }
            throw error
        }
    }

    /// A wallet's seed opened with a context that is already confirmed in person (`.setCode` seals it again).
    static func openSeed(id: String, context: LAContext) throws -> Data {
        if let sealed = sealed(account(id)) { return try open(sealed, context: context) }
        guard id == SignerWallet.firstID, let seed = read(legacyAccount, context: context) else { throw WalletError("No wallet imported.") }
        sealLegacy(seed, id: id)
        return seed
    }

    /// A seed kept from before sealing (until 2026-09-28) is sealed to the Secure Enclave the first time it opens, as every
    /// wallet is; save writes the seal first and only then removes the unsealed item, so a failure leaves the old one.
    private static func sealLegacy(_ seed: Data, id: String) {
        try? save(seed, requirePresence: true, id: id)
    }

    /// A wallet's seed opened with no prompt, or nil: only one sealed without Touch ID (a test or the automation wallet)
    /// opens so. What the settings say about a wallet is held to this wherever it would skip a prompt.
    static func withoutPrompt(id: String) -> Data? {
        let context = LAContext()
        context.interactionNotAllowed = true
        return try? openSeed(id: id, context: context)
    }

    /// A sealed secret opened with its Enclave key, wherever that key's group is.
    private static func open(_ sealed: (data: Data, tag: Data), context: LAContext) throws -> Data {
        var q = keyQuery(sealed.tag)
        q[kSecReturnRef as String] = true
        q[kSecUseAuthenticationContext as String] = context
        var ref: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &ref) == errSecSuccess, let ref else { throw WalletError("The Secure Enclave key is missing.") }
        var error: Unmanaged<CFError>?
        // The Enclave asks for Touch ID or the password here, unless this context already did.
        guard let opened = SecKeyCreateDecryptedData(ref as! SecKey, algorithm, sealed.data as CFData, &error) else {
            throw WalletError("Cancelled, or \(Presence.short) didn't match: nothing was signed.")
        }
        // Our own copy, which the caller wipes; the Security framework's buffer is cleared before it's released.
        let bytes = CFDataGetBytePtr(opened), count = CFDataGetLength(opened)
        let seed = Data(bytes: bytes!, count: count)
        _ = memset_s(UnsafeMutableRawPointer(mutating: bytes!), count, 0, count)
        return seed
    }

    private static func read(_ account: String, context: LAContext?) -> Data? {
        var q = item(account)
        q[kSecReturnData as String] = true
        if let context { q[kSecUseAuthenticationContext as String] = context }
        var out: CFTypeRef?
        return SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess ? out as? Data : nil
    }

    /// The user in person — Touch ID or the login password — with `reason` on the system prompt, always a fresh
    /// prompt. The authenticated context then serves the signature that follows, so there is one prompt, not two.
    /// `within`: seconds before the prompt closes by itself — for the automation wallet, which signs with no one at
    /// the Mac: a prompt left up would hold its task, and the app's watch loop with it, until someone came back.
    static func confirm(_ reason: String, within: TimeInterval? = nil) throws {
        let context = LAContext()
        context.localizedReason = reason
        let done = DispatchSemaphore(value: 0)
        nonisolated(unsafe) var outcome: Error?
        context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { ok, error in
            if !ok { outcome = error ?? WalletError("Not confirmed.") }
            done.signal()
        }
        if let within, done.wait(timeout: .now() + within) == .timedOut {
            context.invalidate()
            _ = done.wait(timeout: .now() + 5)
            throw WalletError("No one confirmed it within \(Int(within)) seconds: nothing was signed.")
        }
        if within == nil { done.wait() }
        if let outcome { throw WalletError("Not confirmed (\(outcome.localizedDescription)): nothing was changed.") }
        lock.withLock { session = (context, Date(), false) }
    }

    /// The context the last `confirm` opened, while its session lasts.
    static var confirmed: LAContext? { lock.withLock { session.flatMap { $0.byCode ? nil : $0.context } } }

    /// The right passcode opened `context`: signatures go on with it as after Touch ID, for the same session length.
    static func startCodeSession(_ context: LAContext) {
        lock.withLock {
            session?.context.invalidate()
            session = (context, Date(), true)
        }
    }

    static func delete(id: String) {
        endSession()
        CodeStore.removeSeal(id)
        if let tag = sealed(account(id))?.tag { SecItemDelete(keyQuery(tag) as CFDictionary) }
        SecItemDelete(item(account(id)) as CFDictionary)
        if id == SignerWallet.firstID { SecItemDelete(item(legacyAccount) as CFDictionary) }
    }

    /// The next load asks for Touch ID again.
    static func endSession() {
        lock.withLock {
            session?.context.invalidate()
            session = nil
        }
    }
}

/// A signature that waits for the passcode instead of a macOS prompt: the reply says `needsCode`, with `reason`.
struct CodeNeeded: Error { let reason: String }

/// The 6-digit passcode. A Secure Enclave key that opens only with the code (`.applicationPassword`: the Enclave mixes it
/// into the key, nothing here keeps it), made after the person confirmed in person. Sealed to it: a check (is the code
/// right?) and, for each wallet that asks in person, its seed once more — what a Mac without Touch ID signs with instead
/// of the login password. Adding an address, or a wallet change, still asks macOS: a code seen by something on the Mac
/// can't send money anywhere new. Five wrong codes in a row delete it all.
enum CodeStore {
    static let service = "app.ihatecrypto.code"
    static let checkAccount = "check"
    static let tries = 5
    static let failuresKey = "failures"
    static func sealAccount(_ id: String) -> String { "seal-" + SeedStore.account(id) }

    static func item(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: account,
         kSecUseDataProtectionKeychain as String: true]
    }

    private static func stored(_ account: String) -> (data: Data, tag: Data)? {
        var q = item(account)
        q[kSecReturnData as String] = true
        q[kSecReturnAttributes as String] = true
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let found = out as? [String: Any],
              let data = found[kSecValueData as String] as? Data, let tag = found[kSecAttrGeneric as String] as? Data else { return nil }
        return (data, tag)
    }

    static var isSet: Bool { stored(checkAccount) != nil }
    /// With Touch ID the sensor stays the way to sign; the code signs only on a Mac without it.
    static var signs: Bool { !Presence.touchID && isSet }
    static func sealed(_ id: String) -> (data: Data, tag: Data)? { stored(sealAccount(id)) }

    /// A context that carries the code and never shows a prompt of its own: a wrong code fails, it doesn't ask.
    static func context(_ code: String) -> LAContext {
        let context = LAContext()
        context.setCredential(Data(code.utf8), type: .applicationPassword)
        context.interactionNotAllowed = true
        return context
    }

    private static func publicKey(_ tag: Data) -> SecKey? {
        var q = SeedStore.keyQuery(tag)
        q[kSecReturnRef as String] = true
        var ref: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &ref) == errSecSuccess, let ref else { return nil }
        return SecKeyCopyPublicKey(ref as! SecKey)
    }

    private static func write(_ account: String, _ ciphertext: Data, tag: Data) -> Bool {
        SecItemDelete(item(account) as CFDictionary)
        var q = item(account)
        q[kSecValueData as String] = ciphertext
        q[kSecAttrGeneric as String] = tag
        q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        return SecItemAdd(q as CFDictionary, nil) == errSecSuccess
    }

    /// Whether `context` (carrying a code) opens the check: the stored one, or one about to be.
    private static func opens(_ context: LAContext, _ check: (data: Data, tag: Data)? = nil) -> Bool {
        guard let (data, tag) = check ?? stored(checkAccount) else { return false }
        var q = SeedStore.keyQuery(tag)
        q[kSecReturnRef as String] = true
        q[kSecUseAuthenticationContext as String] = context
        var ref: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &ref) == errSecSuccess, let ref,
              let opened = SecKeyCreateDecryptedData(ref as! SecKey, SeedStore.algorithm, data as CFData, nil) else { return false }
        _ = memset_s(UnsafeMutableRawPointer(mutating: CFDataGetBytePtr(opened)!), CFDataGetLength(opened), 0, CFDataGetLength(opened))
        return true
    }

    /// A new code in place of any earlier one. `presence`: a context confirmed in person, which opens each wallet's seed
    /// for its second seal. Everything is made and checked before the earlier code goes; only a Keychain failure while
    /// writing leaves no code at all.
    static func set(_ code: String, presence: LAContext?) throws {
        try entering.withLock { try make(code, presence: presence) }
    }

    private static func make(_ code: String, presence: LAContext?) throws {
        var error: Unmanaged<CFError>?
        func fail(_ what: String) -> WalletError { WalletError("\(what): \(error.map { "\($0.takeRetainedValue())" } ?? "?")") }
        guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage, .applicationPassword], &error) else {
            throw fail("Couldn't set access to the passcode key")
        }
        let tag = Data("app.ihatecrypto.code.key.\(UUID().uuidString)".utf8)
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecUseDataProtectionKeychain as String: true,
            kSecUseAuthenticationContext as String: context(code),
            kSecPrivateKeyAttrs as String: [
                kSecAttrIsPermanent as String: true,
                kSecAttrApplicationTag as String: tag,
                kSecAttrAccessControl as String: access,
            ] as [String: Any],
        ]
        guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &error), let pub = SecKeyCopyPublicKey(key) else {
            throw fail("The Secure Enclave didn't make the passcode key")
        }
        func drop() { SecItemDelete(SeedStore.keyQuery(tag) as CFDictionary) }
        var check = Data(count: 32)
        _ = check.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 32, $0.baseAddress!) }
        guard let checkSealed = SecKeyCreateEncryptedData(pub, SeedStore.algorithm, check as CFData, &error) as Data? else { drop(); throw fail("Couldn't seal the check") }
        // The Enclave must open what it sealed with this code, and not with another, or the code would lock the user out
        // or guard nothing. (Checked on the new key: no try is counted.)
        let other = String(code.dropLast()) + String(((code.last?.wholeNumberValue ?? 0) + 1) % 10)
        guard opens(context(code), (checkSealed, tag)), !opens(context(other), (checkSealed, tag)) else {
            drop(); throw WalletError("This Mac's Secure Enclave didn't take the passcode.")
        }
        // Each wallet that asks in person, opened with the person's confirmation and sealed to the new key.
        var seals: [(String, Data)] = []
        // Only where the code signs (no Touch ID): with Touch ID the code just opens the app, and holds no seed.
        for wallet in Wallets.all where !wallet.isTest && !wallet.isAutomation && !Presence.touchID {
            var seed: Data
            do { seed = try SeedStore.openSeed(id: wallet.id, context: presence ?? LAContext()) } catch { drop(); throw error }
            defer { SeedStore.wipe(&seed) }
            guard let sealed = SecKeyCreateEncryptedData(pub, SeedStore.algorithm, seed as CFData, &error) as Data? else { drop(); throw fail("Couldn't seal «\(wallet.label)»") }
            seals.append((sealAccount(wallet.id), sealed))
        }
        let old = stored(checkAccount)?.tag
        clearItems()
        guard write(checkAccount, checkSealed, tag: tag), seals.allSatisfy({ write($0.0, $0.1, tag: tag) }) else {
            clearItems(); drop()
            if let old { SecItemDelete(SeedStore.keyQuery(old) as CFDictionary) }
            throw WalletError("Keychain didn't save the passcode: it is off now.")
        }
        if let old { SecItemDelete(SeedStore.keyQuery(old) as CFDictionary) }
    }

    /// A wallet saved while a code is set gets its passcode seal right away (only the code's public key is needed).
    /// Best effort: without it, that wallet asks macOS as before.
    static func seal(_ seed: Data, id: String) {
        entering.lock()
        defer { entering.unlock() }
        guard let tag = stored(checkAccount)?.tag, let pub = publicKey(tag),
              let sealed = SecKeyCreateEncryptedData(pub, SeedStore.algorithm, seed as CFData, nil) as Data? else { return }
        _ = write(sealAccount(id), sealed, tag: tag)
    }

    static func removeSeal(_ id: String) { SecItemDelete(item(sealAccount(id)) as CFDictionary) }

    enum Entered { case right(LAContext), wrong(left: Int), off, uncounted }

    /// One code operation at a time — a try, a new code, a seal, turning it off — so none sees another half done.
    private static let entering = NSRecursiveLock()

    /// Wrong codes in a row, in the signer's own Keychain group: not the app's to read or reset. nil: unreadable, which
    /// counts as off — never as no tries.
    private static func failures() -> Int? {
        var q = item(failuresKey)
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        if status == errSecItemNotFound { return 0 }
        guard status == errSecSuccess, let count = (out as? Data)?.first else { return nil }
        return Int(count)
    }

    /// Written in place (update, or add the first time): there is never a moment with no count to kill the signer in.
    private static func setFailures(_ count: Int) -> Bool {
        let data = Data([UInt8(min(count, 255))])
        var status = SecItemUpdate(item(failuresKey) as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var q = item(failuresKey)
            q[kSecValueData as String] = data
            q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            status = SecItemAdd(q as CFDictionary, nil)
        }
        return status == errSecSuccess
    }

    /// The code typed in the app. Right: the context that opens the seals. Wrong: tries left; the fifth wrong one in a
    /// row turns the code off. One at a time, and a try is tested only once it is counted: requests side by side, or a
    /// signer stopped mid-try, get no extra guesses.
    static func enter(_ code: String) -> Entered {
        entering.withLock {
            guard isSet, let before = failures(), before < tries else { clear(); return .off }
            guard setFailures(before + 1) else { return .uncounted }
            let context = context(code)
            if Passcode.problem(code) == nil, opens(context) {
                _ = setFailures(0)
                return .right(context)
            }
            guard before + 1 < tries else { clear(); return .off }
            return .wrong(left: tries - before - 1)
        }
    }

    /// The key, the check and every seal gone; signatures ask macOS again.
    static func clear() {
        entering.withLock {
            let tag = stored(checkAccount)?.tag
            clearItems()
            if let tag { SecItemDelete(SeedStore.keyQuery(tag) as CFDictionary) }
        }
    }

    private static func clearItems() {
        SecItemDelete([kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service,
                       kSecUseDataProtectionKeychain as String: true] as CFDictionary)
    }
}

/// Addresses outside the wallet that the user allowed money to go to, each confirmed in person once. Kept in the
/// signer's own Keychain group, so the app — or anything posing as it — can't add one silently.
enum RecipientStore {
    static let service = "app.ihatecrypto.recipients"
    /// The two lists: money recipients, and contracts whose calls the signer can't read, allowed as calls only — a call
    /// allowed once doesn't make the contract a place money or a spending right may go (the 2026-10-22 review).
    static let money = "approved", calls = "calls"
    /// Per wallet: an address approved for one wallet isn't approved for another.
    static func query(_ id: String, _ list: String = money) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: Wallets.scoped(list, id),
            kSecUseDataProtectionKeychain as String: true,
        ]
    }
    static func base(_ list: String) -> [String: Any] { query(Wallets.activeID, list) }

    static func all(_ list: String = money) -> Set<String> {
        var q = base(list)
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data,
              let list = try? JSONDecoder().decode([String].self, from: data) else { return [] }
        return Set(list)
    }

    static func add(_ addresses: [String], to list: String = money) throws {
        let entries = Array(all(list).union(addresses.map(key))).sorted()
        SecItemDelete(base(list) as CFDictionary)
        var q = base(list)
        q[kSecValueData as String] = try JSONEncoder().encode(entries)
        q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(q as CFDictionary, nil)
        guard status == errSecSuccess else { throw WalletError("Keychain didn't save the approved address: \(status)") }
    }

    /// EVM addresses compare without case; Solana's base58 is case-sensitive.
    static func key(_ address: String) -> String { address.hasPrefix("0x") ? address.lowercased() : address }

    static func delete(id: String) { for list in [money, calls] { SecItemDelete(query(id, list) as CFDictionary) } }
}

/// The addresses of the wallets whose keys this signer holds, each written when its key was derived here: money between
/// them stays the user's, so none is a new address for another (what a task bought goes from Automation to Main by
/// itself). Kept apart from the wallet list (SignerPrefs) so a wallet is «ours» only once its key was derived here.
enum OwnStore {
    static let service = "app.ihatecrypto.own"
    static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "own",
         kSecUseDataProtectionKeychain as String: true]
    }

    static func all() -> Set<String> {
        var q = query
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data,
              let list = try? JSONDecoder().decode([String].self, from: data) else { return [] }
        return Set(list)
    }

    /// Best effort: an address left out is asked about in person, like any other.
    static func add(_ addresses: [String]) {
        let have = all(), new = Set(addresses.map(RecipientStore.key)).subtracting(have)
        if !new.isEmpty { save(have.union(new)) }
    }

    /// A removed wallet is no longer ours: money to it asks again.
    static func remove(_ addresses: [String]) {
        let have = all(), gone = Set(addresses.map(RecipientStore.key)).intersection(have)
        if !gone.isEmpty { save(have.subtracting(gone)) }
    }

    private static func save(_ list: Set<String>) {
        guard let data = try? JSONEncoder().encode(list.sorted()) else { return }
        SecItemDelete(query as CFDictionary)
        var q = query
        q[kSecValueData as String] = data
        q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        SecItemAdd(q as CFDictionary, nil)
    }
}

/// The address book's names and addresses (SignerAddress), in the signer's own Keychain group: the app's file keeps
/// only how entries look. It changes only by `.saveAddresses`, after Touch ID with every address on the prompt, so
/// a changed file, a chat message or a program posing as the app can't put a scam address under a saved name.
enum AddressBookStore {
    static let service = "app.ihatecrypto.addressbook"
    static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "book",
         kSecUseDataProtectionKeychain as String: true]
    }

    static func all() -> [SignerAddress] {
        var q = query
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return [] }
        return (try? JSONDecoder().decode([SignerAddress].self, from: data)) ?? []
    }

    static func save(_ book: [SignerAddress]) throws {
        SecItemDelete(query as CFDictionary)
        var q = query
        q[kSecValueData as String] = try JSONEncoder().encode(book)
        q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(q as CFDictionary, nil)
        guard status == errSecSuccess else { throw WalletError("Keychain didn't save the address book: \(status)") }
    }
}

/// The Hyperliquid agent key: data-protection Keychain, this device, no Touch ID — it cannot withdraw.
enum AgentStore {
    static let service = "app.ihatecrypto.agent"
    /// Per wallet: each wallet's Hyperliquid account approves its own trading key.
    static func query(_ id: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: Wallets.scoped("hyperliquid", id),
            kSecUseDataProtectionKeychain as String: true,
        ]
    }
    /// `id`: a wallet's, or «ledger-0x…» for the key a Ledger approved for its own account.
    static func load(id: String = Wallets.activeID) -> Data? {
        var q = query(id)
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        return SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess ? out as? Data : nil
    }

    /// 32 random bytes that make a valid secp256k1 key; retried in the vanishing case they don't.
    static func make() throws -> Data {
        for _ in 0..<4 {
            var bytes = Data(count: 32)
            guard bytes.withUnsafeMutableBytes({ SecRandomCopyBytes(kSecRandomDefault, 32, $0.baseAddress!) }) == errSecSuccess else { continue }
            if (try? EVMAccount.address(privateKey: bytes)) != nil { return bytes }
        }
        throw WalletError("Couldn't create the agent key.")
    }

    static func save(_ key: Data, id: String = Wallets.activeID) throws {
        SecItemDelete(query(id) as CFDictionary)
        var q = query(id)
        q[kSecValueData as String] = key
        q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(q as CFDictionary, nil)
        guard status == errSecSuccess else { throw WalletError("Keychain didn't save the agent key: \(status)") }
    }

    static func delete(id: String) { SecItemDelete(query(id) as CFDictionary) }
}

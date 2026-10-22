import Foundation
import Security

/// The signer's settings — the wallets' list (each one's addresses, test or not), which one signs and its addresses,
/// the session length, the trading keys' switches — in the data-protection Keychain, not in the sandbox's preferences
/// file: anything on the Mac that can write that file could name its own address «ours», mark the main wallet a test
/// one (no new-address prompt) or stretch the session past what Touch ID allowed. Read once into memory: there is one
/// signer process. The first run of this version takes what the file held.
final class SignerPrefs: @unchecked Sendable {
    static let shared = SignerPrefs()

    private static var query: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "app.ihatecrypto.prefs",
            kSecAttrAccount as String: "prefs",
            kSecUseDataProtectionKeychain as String: true,
        ]
    }

    /// The keys the preferences file held before; nothing else of it is read.
    static func carried(_ key: String) -> Bool {
        ["wallets", "activeWallet", "solana", "evm", "near", "sessionLength"].contains(key) || key.hasPrefix("agentActive")
    }

    private let lock = NSLock()
    private var cache: [String: Any]?

    /// nil: the Keychain didn't answer (it does after the first unlock); nothing is trusted or written, the next call
    /// reads again.
    private func load() -> [String: Any]? {
        if let cache { return cache }
        var q = Self.query
        q[kSecReturnData as String] = true
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        if status == errSecSuccess, let data = out as? Data,
           let values = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any] {
            cache = values
            return values
        }
        guard status == errSecItemNotFound else { return nil }
        // Taken once as it is. What a change made to it earlier could skip is checked where it's used: «our address»
        // against the key at every signature (Signer.vouch), a test wallet by its seed opening without a prompt.
        let old = UserDefaults.standard.dictionaryRepresentation().filter { Self.carried($0.key) }
        save(old)
        return old
    }

    private func save(_ values: [String: Any]) {
        cache = values
        guard let data = try? PropertyListSerialization.data(fromPropertyList: values, format: .binary, options: 0) else { return }
        if SecItemUpdate(Self.query as CFDictionary, [kSecValueData as String: data] as CFDictionary) == errSecItemNotFound {
            var q = Self.query
            q[kSecValueData as String] = data
            // Readable while the screen is locked, after the first unlock: a task's status read then still finds the wallets.
            q[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            SecItemAdd(q as CFDictionary, nil)
        }
    }

    func object(forKey key: String) -> Any? { lock.withLock { load()?[key] } }
    func string(forKey key: String) -> String? { object(forKey: key) as? String }
    func data(forKey key: String) -> Data? { object(forKey: key) as? Data }
    func bool(forKey key: String) -> Bool { object(forKey: key) as? Bool ?? false }
    func integer(forKey key: String) -> Int { object(forKey: key) as? Int ?? 0 }
    func dictionaryRepresentation() -> [String: Any] { lock.withLock { load() ?? [:] } }

    /// nil removes the key, as UserDefaults does.
    func set(_ value: Any?, forKey key: String) {
        lock.withLock {
            guard var values = load() else { return }
            values[key] = value
            save(values)
        }
    }

    func removeObject(forKey key: String) { set(nil, forKey: key) }
}

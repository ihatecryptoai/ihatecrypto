import Foundation
import SignerAPI

/// The wallets the signer holds, public parts only, in SignerPrefs; the secrets are SeedStore's, one sealed
/// item each. Which wallet signs is the signer's own state: the app can ask to switch, and that takes Touch ID.
/// The active wallet's addresses are mirrored under the keys a single wallet used («solana», «evm», «near»), so every
/// check that reads «our address» reads the active wallet's.
enum Wallets {
    static var defaults: SignerPrefs { Signer.defaults }

    /// The registry, migrated from the single seed on first read (wallet «1», «Main», active).
    static var all: [SignerWallet] {
        let stored = defaults.data(forKey: "wallets").flatMap { try? JSONDecoder().decode([SignerWallet].self, from: $0) }
        let list = SignerWallet.migrated(stored, seedPresent: SeedStore.firstPresent, solana: defaults.string(forKey: "solana"),
                                         evm: defaults.string(forKey: "evm"), near: defaults.string(forKey: "near"))
        if stored == nil, !list.isEmpty {
            save(list)
            defaults.set(SignerWallet.firstID, forKey: "activeWallet")
        }
        return list
    }

    static func save(_ list: [SignerWallet]) { defaults.set(try? JSONEncoder().encode(list), forKey: "wallets") }

    /// The automation wallet for one request (Signer.handle's `.asWallet`): while it's set, the key, «our addresses»,
    /// the approved addresses and every per-wallet name follow it instead of the active wallet.
    @TaskLocal static var signing: SignerWallet?

    static var active: SignerWallet? {
        if let signing { return signing }
        let list = all
        return list.first { $0.id == defaults.string(forKey: "activeWallet") } ?? list.first
    }

    static var activeID: String { active?.id ?? SignerWallet.firstID }

    /// A per-wallet name: wallet «1» keeps the one it had before there were several.
    static func scoped(_ base: String, _ id: String = activeID) -> String { id == SignerWallet.firstID ? base : "\(base)-\(id)" }

    /// Signing moves to `wallet` (nil: none left): its addresses become «ours», and the next signature asks for Touch ID.
    static func activate(_ wallet: SignerWallet?) {
        defaults.set(wallet?.id, forKey: "activeWallet")
        for (key, value) in [("solana", wallet?.solana), ("evm", wallet?.evm), ("near", wallet?.near)] {
            if let value { defaults.set(value, forKey: key) } else { defaults.removeObject(forKey: key) }
        }
        SeedStore.endSession()
    }
}

import CWinSec
@testable import DesktopEngine
import Foundation
import Testing

/// Bringing a wallet, as the Mac's onboarding and ImportWalletView do it: a phrase with its BIP-39 passphrase opens the
/// wallet it came from, and the window gets the first addresses it makes, to compare with MetaMask's and Phantom's.
@Suite(.serialized) struct OnboardingTests {
    static let words = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"

    @Test func theFirstAddressesAreMetaMasksAndPhantoms() throws {
        let plain = try Engine.firstAccounts(Self.words, passphrase: "")
        #expect(plain.count == 3)
        // The public test phrase's own: MetaMask's first account, Phantom's first.
        #expect(plain[0]["evm"]?.lowercased() == "0x9858effd232b4033e47d90003d41ec34ecaeda94")
        #expect(plain[0]["solana"] == "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk")
        #expect(Set(plain.map { $0["evm"] ?? "" }).count == 3 && Set(plain.map { $0["solana"] ?? "" }).count == 3)
        // Another passphrase is another wallet.
        let other = try Engine.firstAccounts(Self.words, passphrase: "TREZOR")
        #expect(other[0]["evm"] != plain[0]["evm"] && other[0]["solana"] != plain[0]["solana"])
    }

    @Test(.enabled(if: winsec_is_stub() == 1)) func aPhraseWithItsPassphraseOpensThatWallet() throws {
        let engine = try EngineTests.Engine()
        try engine.call("hello")
        let r = try engine.call("importPhrase", ["phrase": Self.words, "passphrase": "TREZOR"])
        #expect(r.answer["ok"] as? Bool == true, "\(r.answer)")
        let text = (r.answer["result"] as? [String: Any])?["accounts"] as? String ?? ""
        let accounts = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [[String: String]] ?? []
        let expected = try Engine.firstAccounts(Self.words, passphrase: "TREZOR")
        #expect(accounts == expected)
        // The wallet the signer made is that one, not the phrase's plain one.
        let addresses = r.state?["addresses"] as? [String: String] ?? [:]
        #expect(addresses["evm"]?.lowercased() == expected[0]["evm"]?.lowercased())
        #expect(addresses["solana"] == expected[0]["solana"])
        // The facts the onboarding's Hyperliquid steps wait for are asked, not guessed: unread, it says nothing.
        let funded = try engine.call("hlFunded")
        #expect(funded.answer["ok"] as? Bool == true)
    }
}

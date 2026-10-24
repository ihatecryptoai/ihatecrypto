import Foundation
@testable import SignerCore
import SignerAPI
import Testing
import WalletCore

/// S10 and R9-1 (audit9): with only a Ledger, before anything asked Windows Hello, a Ledger's trading key is still
/// sealed — its Windows Hello key made then (one prompt) — so a file another program put there is never the key the
/// Ledger approves, and the signer's own key survives the Windows Hello prompts that come after. In VaultTests' suite:
/// they share the signer's folder, one at a time.
extension VaultTests {
    @Test(.enabled(if: fakeHello)) func aLedgersTradingKeyIsSealedEvenBeforeAnyWindowsHello() throws {
        // Hardhat's public account #1 as the Ledger's account; nothing else here: no wallet, no Windows Hello key.
        let owner = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
        let id = "ledger-" + owner.lowercased()
        #expect(Vault.master() == nil)
        let planted = try AgentStore.make()
        let plantedAddress = try EVMAccount.address(privateKey: planted)
        try Vault.write(AgentStore.plain(id), Vault.protect(planted))
        restart()

        // The planted file isn't a key the Ledger has: asking for one makes the signer's own, sealed, and the file goes.
        let made = try #require(try send(.ledgerAgent(owner: owner, create: true)).agentAddress)
        #expect(made.lowercased() != plantedAddress.lowercased())
        #expect(Vault.master() != nil)
        #expect(Vault.exists(AgentStore.file(id)) && !Vault.exists(AgentStore.plain(id)))

        // A Windows Hello prompt later (here: a new signer process opening it) keeps the same key: nothing to approve again.
        restart()
        #expect(AgentStore.has(id: id) && AgentStore.address(id: id) == made)
        #expect(try send(.ledgerAgent(owner: owner, create: true)).agentAddress == made)
        AgentStore.askAgain()
        let key = try #require(Vault.$scope.withValue(Vault.Scope()) { AgentStore.load(id: id) })
        #expect(try EVMAccount.address(privateKey: key) == made)
    }

    /// T14: the Mac's signer keeps its own words after five wrong codes. Signer/Core is compiled into the Mac's
    /// IhatecryptoSigner too, where `canImport(CWinSec)` is false: its line there is the Mac's, byte for byte.
    @Test func theMacsLineAfterFiveWrongCodesIsTheMacs() throws {
        let core = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("../../Sources/Signer/Core/Signer.swift")
        let source = try String(contentsOf: core, encoding: .utf8)
        let mac = #"let off = "\(CodeStore.tries) wrong codes: the code is off. Only your Mac password opens ihatecrypto and signs now.""#
        let macBranch = try #require(source.range(of: "#else\n                " + mac + "\n                #endif"))
        #expect(!macBranch.isEmpty)
    }
}

import Foundation
@testable import SignerCore
import SignerAPI
import Testing
import WalletCore

/// The Hyperliquid trading key on Windows (AgentStore): sealed under Windows Hello as a seed is, opened once per signer
/// run, its address readable without opening it; a key kept under DPAPI only is sealed at its first use, and one put
/// there after a key was sealed is never taken. In VaultTests' suite: they share the signer's folder, one at a time.
extension VaultTests {
    /// A new signer process: nothing opened, no session, the files read again.
    func restart() {
        AgentStore.forget()
        Vault.endSession()
        Trusted.reload()
    }

    @Test(.enabled(if: fakeHello)) func theTradingKeyIsSealedAndOpensOncePerRun() throws {
        _ = try importWallet()
        let id = SignerWallet.firstID
        let enabled = try send(.enableAgent)
        let agent = try #require(enabled.agentAddress)
        #expect(Vault.exists(AgentStore.file(id)) && !Vault.exists(AgentStore.plain(id)))

        restart()
        // With no one at Windows Hello, the status still shows the key — and nothing opens it.
        setEnv("WINSEC_FAKE_CANCEL", "1")
        #expect(try send(.status).agentAddress == agent)
        #expect(AgentStore.load(id: id) == nil)
        // One prompt opens it; after that it's in memory, with no one needed.
        setEnv("WINSEC_FAKE_CANCEL", "0")
        let key = try #require(AgentStore.load(id: id))
        #expect(try EVMAccount.address(privateKey: key) == agent)
        setEnv("WINSEC_FAKE_CANCEL", "1")
        #expect(AgentStore.load(id: id) == key)

        // Not the key in the clear, even under the stand-in's «DPAPI».
        let file = try #require(Vault.read(AgentStore.file(id)))
        #expect(try Vault.unprotect(file).range(of: key) == nil)

        // Any Windows Hello the signer asks for opens it too: the first order after a move asks nothing more.
        setEnv("WINSEC_FAKE_CANCEL", "0")
        restart()
        try send(.signPersonal(message: Data("hello".utf8)))
        setEnv("WINSEC_FAKE_CANCEL", "1")
        #expect(AgentStore.load(id: id) == key)
    }

    @Test(.enabled(if: fakeHello)) func anOldTradingKeyIsSealedAtItsFirstUseAndAPlantedOneIsNeverTaken() throws {
        _ = try importWallet()
        let id = SignerWallet.firstID
        // Kept as before 2026-10-24: DPAPI only.
        let old = try AgentStore.make()
        try Vault.write(AgentStore.plain(id), Vault.protect(old))
        restart()
        #expect(Vault.$scope.withValue(Vault.Scope()) { AgentStore.load(id: id) } == old)
        #expect(Vault.exists(AgentStore.file(id)) && !Vault.exists(AgentStore.plain(id)))
        #expect(AgentStore.address(id: id) == (try EVMAccount.address(privateKey: old)))

        // Another program deletes the sealed key and puts its own beside the wallet, the old way: never taken.
        let planted = try AgentStore.make()
        Vault.remove(AgentStore.file(id))
        try Vault.write(AgentStore.plain(id), Vault.protect(planted))
        restart()
        #expect(AgentStore.address(id: id) == nil)
        #expect(Vault.$scope.withValue(Vault.Scope()) { AgentStore.load(id: id) } == nil)
    }

    @Test(.enabled(if: fakeHello)) func aSealedKeysAddressIsTakenOnlyAsTheTrustedStateRecordedIt() throws {
        _ = try importWallet()
        let id = SignerWallet.firstID
        let agent = try #require(try send(.enableAgent).agentAddress)
        // Another program rewrites the address beside the sealed key: Settings, and an approval, would take its address.
        var json = try Vault.unprotect(try #require(Vault.read(AgentStore.file(id))))
        var sealed = try JSONDecoder().decode(AgentStore.Sealed.self, from: json)
        sealed.address = "0x000000000000000000000000000000000000dEaD"
        json = try JSONEncoder().encode(sealed)
        try Vault.write(AgentStore.file(id), Vault.protect(json))
        restart()
        #expect(AgentStore.address(id: id) == nil, "not \(agent), nor the planted one")
    }

    @Test func aWalletThatSignsWithNoOneThereKeepsItsTradingKeyUnderDPAPI() throws {
        setEnv("WINSEC_FAKE_HELLO", "0")
        let made = try send(.createWallet(requirePresence: false))
        let id = try #require(made.activeWallet)
        #expect(!AgentStore.sealedFor(id))
        let key = try AgentStore.make()
        try AgentStore.save(key, id: id)
        #expect(Vault.exists(AgentStore.plain(id)) && !Vault.exists(AgentStore.file(id)))
        restart()
        #expect(AgentStore.load(id: id) == key)
    }
}

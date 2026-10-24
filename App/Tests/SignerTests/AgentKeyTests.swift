import Foundation
@testable import SignerCore
import SignerAPI
import Testing
import WalletCore

/// The Hyperliquid trading key on Windows (AgentStore): sealed under Windows Hello as a seed is, opened once per signer
/// run, its address readable without opening it; a key kept under DPAPI only is taken only by a wallet that signs with
/// no one there (test, automation), never one put beside a wallet whose keys are sealed. In VaultTests' suite: they share the signer's folder, one at a time.
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
        // One prompt opens it; after that it's in memory, with no one needed. (After the unanswered one it waits ten
        // minutes unless an order that opens asks: AgentFallbackTests.)
        setEnv("WINSEC_FAKE_CANCEL", "0")
        AgentStore.askAgain()
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

    /// S9: a key file another program of the user puts beside a wallet whose keys are sealed — with no key there yet, or
    /// after the sealed one — is never taken: not shown, not used for an order, and turning the key on approves a new one.
    @Test(.enabled(if: fakeHello)) func aPlantedTradingKeyIsNeverTakenNorApproved() throws {
        _ = try importWallet()
        let id = SignerWallet.firstID
        let planted = try AgentStore.make()
        let plantedAddress = try EVMAccount.address(privateKey: planted)
        try Vault.write(AgentStore.plain(id), Vault.protect(planted))
        restart()
        #expect(!AgentStore.has(id: id) && AgentStore.address(id: id) == nil)
        #expect(Vault.$scope.withValue(Vault.Scope()) { AgentStore.load(id: id) } == nil)
        // Turning the key on makes one inside the signer; the planted file goes.
        let agent = try #require(try send(.enableAgent).agentAddress)
        #expect(agent.lowercased() != plantedAddress.lowercased())
        #expect(Vault.exists(AgentStore.file(id)) && !Vault.exists(AgentStore.plain(id)))

        // Put back after the sealed key is deleted: still never taken.
        Vault.remove(AgentStore.file(id))
        try Vault.write(AgentStore.plain(id), Vault.protect(planted))
        restart()
        #expect(AgentStore.address(id: id) == nil)
        #expect(Vault.$scope.withValue(Vault.Scope()) { AgentStore.load(id: id) } == nil)
        // A Ledger's trading key is sealed too: a file planted for it isn't one.
        let ledger = "ledger-0x70997970c51812dc3a010c7d01b50e0d17dc79c8"
        try Vault.write(AgentStore.plain(ledger), Vault.protect(planted))
        #expect(!AgentStore.has(id: ledger) && AgentStore.address(id: ledger) == nil)
        Vault.remove(AgentStore.plain(ledger))
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

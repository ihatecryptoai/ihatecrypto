import CWinSec
import Foundation
@testable import SignerCore
import SignerAPI
import Testing
import WalletCore
#if os(Windows)
import WinSDK
#endif

/// The tests that need a Windows Hello answer run with the stand-in only (a Mac, Linux): a Windows runner has the real
/// winsec.dll and no Windows Hello set up.
let fakeHello = winsec_is_stub() == 1

func setEnv(_ name: String, _ value: String) {
    #if os(Windows)
    _ = name.withCString(encodedAs: UTF16.self) { n in value.withCString(encodedAs: UTF16.self) { SetEnvironmentVariableW(n, $0) } }
    #else
    setenv(name, value, 1)
    #endif
}

/// The Windows signer's storage, with the stand-in for Windows Hello (WINSEC_FAKE_HELLO): a wallet sealed under it,
/// one prompt per move, and settings changed outside the signer stopping every signature.
@Suite(.serialized) struct VaultTests {
    /// Hardhat's public test phrase: not the «abandon … about» one, which the signer lets through without prompts.
    static let phrase = "test test test test test test test test test test test junk"
    static let dir: String = {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ihatecrypto-signer-tests-\(ProcessInfo.processInfo.processIdentifier)").path
        setEnv("IHATECRYPTO_SIGNER_DIR", dir)
        return dir
    }()

    init() {
        _ = Self.dir
        setEnv("WINSEC_FAKE_HELLO", "1")
        setEnv("WINSEC_FAKE_CANCEL", "0")
        for file in (try? FileManager.default.contentsOfDirectory(atPath: Vault.folder.path)) ?? [] {
            try? FileManager.default.removeItem(at: Vault.folder.appendingPathComponent(file))
        }
        Vault.endSession()
        Trusted.reload()
    }

    @discardableResult
    func send(_ request: SignerRequest) throws -> SignerReply {
        try Vault.$scope.withValue(Vault.Scope()) { try Signer.handle(request) }
    }

    func importWallet() throws -> SignerReply {
        try send(.importMnemonic(phrase: Self.phrase, passphrase: "", requirePresence: true))
    }

    @Test(.enabled(if: fakeHello)) func aWalletIsSealedUnderWindowsHelloAndSigns() throws {
        let imported = try importWallet()
        #expect(imported.ok)
        #expect(imported.evmAddress?.lowercased() == "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")
        #expect(Vault.exists("master.json"))
        let sealed = try #require(try SeedStore.sealed(SeedStore.file(SignerWallet.firstID)))
        #expect(sealed.presence && sealed.challenge != nil)
        // Not the seed in the clear, even under the stand-in's «DPAPI».
        let seed = try Mnemonic.seed(Self.phrase, passphrase: "")
        #expect(sealed.box.range(of: seed) == nil)

        SeedStore.endSession()
        let signed = try send(.signPersonal(message: Data("hello".utf8)))
        #expect(signed.ok && signed.signature != nil)
        let status = try send(.status)
        #expect(status.wallets?.count == 1 && status.evmAddress == imported.evmAddress)
    }

    @Test(.enabled(if: fakeHello)) func aCancelledPromptSignsNothing() throws {
        _ = try importWallet()
        SeedStore.endSession()
        setEnv("WINSEC_FAKE_CANCEL", "1")
        #expect(throws: WalletError.self) { try send(.signPersonal(message: Data("hello".utf8))) }
    }

    @Test(.enabled(if: fakeHello)) func settingsChangedOutsideTheSignerStopEverySignature() throws {
        _ = try importWallet()
        SeedStore.endSession()
        // A program of the same user adds its address to the approved ones, without the seal.
        var file = try JSONDecoder().decode(Trusted.File.self, from: try #require(Vault.read("state.json")))
        var state = try JSONDecoder().decode(Trusted.State.self, from: file.body)
        state.recipients[SignerWallet.firstID] = ["0x000000000000000000000000000000000000dead"]
        file.body = try JSONEncoder().encode(state)
        try Vault.write("state.json", JSONEncoder().encode(file))
        Trusted.reload()
        #expect(throws: Trusted.tampered) { try send(.signPersonal(message: Data("hello".utf8))) }
    }

    @Test(.enabled(if: fakeHello)) func aRenameWaitsForTheNextPromptAndOnlyTakesTrustAway() throws {
        let imported = try importWallet()
        SeedStore.endSession()
        let id = try #require(imported.activeWallet)
        try send(.renameWallet(id: id, label: "Savings"))
        #expect(Vault.exists("pending.json"))
        #expect(try send(.status).wallets?.first?.label == "Savings")
        // A pending file that adds an address is read as nothing.
        var file = try JSONDecoder().decode(Trusted.File.self, from: try #require(Vault.read("pending.json")))
        var bad = try JSONDecoder().decode(Trusted.State.self, from: file.body)
        bad.recipients[id] = ["0x000000000000000000000000000000000000dead"]
        file.body = try JSONEncoder().encode(bad)
        try Vault.write("pending.json", JSONEncoder().encode(file))
        Trusted.reload()
        #expect(RecipientStore.all().isEmpty)
        #expect(try send(.status).wallets?.first?.label == "Main")
    }

    @Test(.enabled(if: fakeHello)) func theRenameIsSealedAtTheNextPrompt() throws {
        let imported = try importWallet()
        SeedStore.endSession()
        try send(.renameWallet(id: try #require(imported.activeWallet), label: "Savings"))
        _ = try send(.signPersonal(message: Data("hello".utf8)))
        #expect(!Vault.exists("pending.json"))
        Trusted.reload()
        #expect(try send(.status).wallets?.first?.label == "Savings")
    }

    @Test(.enabled(if: fakeHello)) func whatWasPlantedBeforeTheFirstWalletIsDropped() throws {
        // Before any wallet needs Windows Hello nothing is sealed: a program could plant a saved address then.
        var state = Trusted.State()
        state.book = [SignerAddress(name: "mom", network: "base", address: "0x000000000000000000000000000000000000dEaD")]
        state.recipients["1"] = ["0x000000000000000000000000000000000000dead"]
        try Vault.write("state.json", JSONEncoder().encode(Trusted.File(body: JSONEncoder().encode(state), mac: nil)))
        Trusted.reload()
        _ = try importWallet()
        #expect(AddressBookStore.all().isEmpty && RecipientStore.all().isEmpty)
    }

    @Test(.enabled(if: fakeHello)) func aNewAddressAsksWindowsHelloAndIsKept() throws {
        _ = try importWallet()
        SeedStore.endSession()
        try Vault.$scope.withValue(Vault.Scope()) { try Signer.approve(["0x000000000000000000000000000000000000beef"]) }
        Trusted.reload()
        #expect(RecipientStore.all() == ["0x000000000000000000000000000000000000beef"])
    }

    @Test func aTestWalletSignsWithNoPrompt() throws {
        setEnv("WINSEC_FAKE_HELLO", "0")
        let made = try send(.createWallet(requirePresence: false))
        #expect(made.ok && made.phrase != nil)
        #expect(!Vault.exists("master.json"))
        #expect(try send(.signPersonal(message: Data("hello".utf8))).ok)
    }
}

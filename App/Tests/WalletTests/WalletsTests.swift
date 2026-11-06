import CryptoKit
import Foundation
import SignerAPI
import Testing
@testable import WalletCore

/// RFC 8032, test 1: an ed25519 secret and its public key; a Solana address is base58 of that public key.
private let rfcSecret = Data(hex: "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")!
private let rfcPublic = Data(hex: "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")!

@Test func phantomBase58KeypairAndKeyFileGiveTheSameSolanaKey() throws {
    let keypair = rfcSecret + rfcPublic
    #expect(try ImportedKey.parse(Base58.encode(keypair)) == .solana(rfcSecret))
    #expect(try ImportedKey.parse("  " + Base58.encode(keypair) + "\n") == .solana(rfcSecret))
    let file = "[" + keypair.map { String($0) }.joined(separator: ",") + "]"
    #expect(try ImportedKey.parse(file) == .solana(rfcSecret))
    // Its address is base58 of RFC 8032's public key.
    #expect(try Base58.decode(try ImportedKey.solana(rfcSecret).address()) == rfcPublic)
    // The secret alone (32 bytes of base58) is accepted too.
    #expect(try ImportedKey.parse(Base58.encode(rfcSecret)) == .solana(rfcSecret))
}

@Test func aMangledSolanaKeyIsRefused() {
    var keypair = rfcSecret + rfcPublic
    keypair[63] ^= 1
    #expect(throws: WalletError.self) { try ImportedKey.parse(Base58.encode(keypair)) }
    #expect(throws: WalletError.self) { try ImportedKey.parse("[1,2,3]") }
    #expect(throws: WalletError.self) { try ImportedKey.parse("[" + Array(repeating: "300", count: 64).joined(separator: ",") + "]") }
    #expect(throws: WalletError.self) { try ImportedKey.parse(Base58.encode(Data(repeating: 7, count: 40))) }
}

/// web3.js accounts docs: this private key is 0x2c7536E3605D9C16a7a3D7b1898e529396a65c23.
@Test func metamaskHexKeyWithOrWithout0xIsAnEVMKey() throws {
    let hex = "4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
    let key = Data(hex: hex)!
    #expect(try ImportedKey.parse(hex) == .evm(key))
    #expect(try ImportedKey.parse("0x" + hex.uppercased()) == .evm(key))
    #expect(try ImportedKey.evm(key).address() == "0x2c7536E3605D9C16a7a3D7b1898e529396a65c23")
    #expect(throws: WalletError.self) { try ImportedKey.parse(String(hex.dropLast(2))) }        // 62 digits
    #expect(throws: WalletError.self) { try ImportedKey.parse(String(repeating: "0", count: 64)) } // not a secp256k1 key
}

@Test func theSingleSeedBecomesWalletOneMainAndARegistryStaysAsItIs() {
    let first = SignerWallet.migrated(nil, seedPresent: true, solana: "Sol", evm: "0xE", near: "n")
    #expect(first == [SignerWallet(id: SignerWallet.firstID, label: "Main", kind: "phrase", solana: "Sol", evm: "0xE", near: "n")])
    #expect(SignerWallet.migrated(nil, seedPresent: false, solana: "Sol", evm: "0xE", near: nil).isEmpty)
    #expect(SignerWallet.migrated(nil, seedPresent: true, solana: nil, evm: nil, near: nil).isEmpty)
    let two = first + [SignerWallet(id: "ab12cd34", label: "Trading", kind: "evm", solana: nil, evm: "0xF", near: nil)]
    #expect(SignerWallet.migrated(two, seedPresent: true, solana: "Sol", evm: "0xE", near: "n") == two)
    #expect(SignerWallet.migrated([], seedPresent: true, solana: "Sol", evm: "0xE", near: "n").isEmpty) // removed stays removed
}

@Test func aKeyWalletSaysWhichKeyItLacks() throws {
    let evmOnly = SignerWallet(id: "x", label: "Trading", kind: "evm", solana: nil, evm: "0xF", near: nil)
    #expect(evmOnly.missing("evm") == nil)
    let message = try #require(evmOnly.missing("solana"))
    #expect(message.contains("«Trading» has no Solana key") && message.contains("an EVM key"))
    #expect(evmOnly.missing("near") != nil)
    let phrase = SignerWallet(id: "1", label: "Main", kind: "phrase", solana: "S", evm: "E", near: "N")
    #expect(["solana", "evm", "near"].allSatisfy { phrase.missing($0) == nil })
}

@Test func testWalletsAreMarkedAndOldRegistriesStayTouchID() throws {
    // A registry written before test wallets has no «test»: every wallet in it keeps asking for Touch ID.
    let old = #"[{"id":"1","label":"Main","kind":"phrase","solana":"S","evm":"E","near":"N"}]"#
    let read = try JSONDecoder().decode([SignerWallet].self, from: Data(old.utf8))
    #expect(read.count == 1 && !read[0].isTest)
    let test = SignerWallet(id: "t1", label: "Test", kind: "phrase", solana: "S2", evm: "E2", near: nil, test: true)
    let round = try JSONDecoder().decode(SignerWallet.self, from: JSONEncoder().encode(test))
    #expect(round.isTest && round == test)
}

@Test func theAutomationWalletIsNeverReachedFromTheDebugAPI() async throws {
    // Old registries and test wallets aren't automation wallets; the flag survives the registry's JSON.
    let auto = SignerWallet(id: "a1", label: "Automation", kind: "phrase", solana: "S3", evm: "E3", near: nil, automation: true)
    let round = try JSONDecoder().decode(SignerWallet.self, from: JSONEncoder().encode(auto))
    #expect(round.isAutomation && !round.isTest && round == auto)
    // A task's signature travels wrapped with the wallet's id, whole.
    let wrapped = SignerRequest.asWallet(id: "a1", request: .signEVMTransaction(chainId: 8453, nonce: 1, maxPriorityFee: 1, maxFee: 2, gasLimit: 21_000,
                                                                                  to: "0x0", value: 0, data: Data(), recipients: []))
    guard case .asWallet("a1", .signEVMTransaction(8453, 1, _, _, _, _, _, _, _)) = try JSONDecoder().decode(SignerRequest.self, from: JSONEncoder().encode(wrapped)) else {
        Issue.record("asWallet didn't round-trip"); return
    }
    // What came through the debug API can neither make the automation wallet nor sign with it: refused before the signer is asked.
    for request in [SignerRequest.createAutomationWallet, wrapped] {
        do {
            _ = try await SignerClient.$untrusted.withValue(true) { try await SignerClient().send(request) }
            Issue.record("the debug API reached the signer")
        } catch let e as WalletError {
            #expect(e.message.contains("debug API"), "refused by the gate, not by a missing signer: \(e.message)")
        }
    }
}

/// A saved name sends only to the address confirmed for it: the name in any case, a 0x address in any case, a
/// Solana one exactly — a look-alike or a swapped address is another entry.
@Test func addressBookEntryMatchesOnlyItsConfirmedAddress() {
    let evm = SignerAddress(name: "Mom", network: "base", address: "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18")
    #expect(evm.same(name: "mom", network: "base", address: "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18"))
    #expect(!evm.same(name: "Mom", network: "base", address: "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e19"))
    #expect(!evm.same(name: "Dad", network: "base", address: evm.address))
    let sol = SignerAddress(name: "Bybit", network: "solana", address: "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw")
    #expect(sol.same(name: "BYBIT", network: "solana", address: sol.address))
    #expect(!sol.same(name: "Bybit", network: "solana", address: sol.address.lowercased()))
    #expect(!sol.same(name: "Bybit", network: "near", address: sol.address))
}

/// The passcode: six ASCII digits, and not one of the codes a thief tries first in its five tries.
@Test func passcodeIsSixDigitsAndNotTheFirstGuesses() {
    for good in ["480516", "190208", "000001", "135790"] { #expect(Passcode.problem(good) == nil, "\(good)") }
    for short in ["", "12345", "1234567", "12a456", "１２３４５６", "٤٨٠٥١٦"] { #expect(Passcode.problem(short) != nil, "\(short)") }
    for guessed in ["000000", "777777", "123456", "345678", "890123", "654321", "210987"] { #expect(Passcode.problem(guessed) != nil, "\(guessed)") }
}

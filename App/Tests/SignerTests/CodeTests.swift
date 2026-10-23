import Foundation
@testable import SignerCore
import SignerAPI
import Testing

/// The passcode on Windows (CodeStore in Vault.swift): kept by the signer as a stretched hash under DPAPI, never the code;
/// a try counted before it's checked, a right code starting the count again, the fifth wrong one in a row turning it off,
/// and a damaged file opening nothing without Windows Hello. In VaultTests' suite: they share the signer's folder.
extension VaultTests {
    @Test(.enabled(if: fakeHello)) func thePasscodeIsCheckedAndFiveWrongOnesTurnItOff() throws {
        _ = try importWallet()
        #expect(throws: (any Error).self) { try send(.setCode(code: "123456")) }
        #expect(try send(.setCode(code: "481902")).codeSet == true)
        #expect(try send(.status).codeSet == true)
        let file = try #require(Vault.read("code.json"))
        #expect(!String(decoding: try Vault.unprotect(file), as: UTF8.self).contains("481902"), "the code is kept as it was typed")

        let wrong = try send(.enterCode(code: "481903", sign: false))
        #expect(!wrong.ok && wrong.error == "Wrong code · 4 tries left" && wrong.codeSet == true)
        #expect(try send(.enterCode(code: "481902", sign: false)).ok)
        // A right code starts the count again: four wrong ones after it still leave the code on.
        for _ in 0..<4 { _ = try send(.enterCode(code: "000001", sign: false)) }
        #expect(try send(.status).codeSet == true)
        let fifth = try send(.enterCode(code: "000001", sign: false))
        #expect(!fifth.ok && fifth.codeOff == true && fifth.error?.contains("Windows Hello") == true, "\(fifth.error ?? "")")
        #expect(try send(.status).codeSet == false && !Vault.exists("code.json"))
        // Even the right code opens nothing now.
        #expect(try send(.enterCode(code: "481902", sign: false)).codeOff == true)
    }

    @Test(.enabled(if: fakeHello)) func aDamagedPasscodeFileOpensNothingWithoutWindowsHello() throws {
        _ = try importWallet()
        try send(.setCode(code: "481902"))
        try Vault.write("code.json", Data("{}".utf8))
        let tried = try send(.enterCode(code: "481902", sign: false))
        #expect(!tried.ok && tried.codeOff == true)
    }
}

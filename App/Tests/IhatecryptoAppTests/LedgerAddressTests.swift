@testable import IhatecryptoApp
import Testing

/// What «Show on Ledger» decides with (LedgerConnect in Ledger.swift): is the shown address the saved one, and where not.
struct LedgerAddressTests {
    // The «abandon … about» phrase's EVM and Solana addresses, and stand-ins that share their first and last groups.
    let evm = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94"
    let poisoned = "0x9858" + String(repeating: "0", count: 32) + "da94"
    let sol = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"

    @Test func evmIgnoresLetterCaseSolanaDoesNot() {
        #expect(LedgerConnect.same(evm, evm.lowercased(), solana: false))
        #expect(!LedgerConnect.same(evm, poisoned, solana: false))
        #expect(LedgerConnect.same(sol, sol, solana: true))
        #expect(!LedgerConnect.same(sol, "hAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", solana: true))
        #expect(!LedgerConnect.same(nil, evm, solana: false))
    }

    @Test func otherIsTheShownAddressOnlyWhenItDiffers() {
        var c = LedgerConnect(evm: evm, solana: sol)
        #expect(c.other(solana: false) == nil)
        c.shown[false] = evm.lowercased()
        #expect(c.other(solana: false) == nil)
        c.shown[false] = poisoned
        #expect(c.other(solana: false) == poisoned)
        c.shown[true] = sol
        #expect(c.other(solana: true) == nil)
    }

    @Test func groupsOfFourMarkOnlyWhatDiffers() {
        let plain = LedgerConnect.groups(evm, solana: false)
        #expect(plain.map(\.text) == ["0x", "9858", "EfFD", "232B", "4033", "E47d", "9000", "3D41", "EC34", "EcaE", "da94"])
        #expect(plain.allSatisfy { !$0.differs })
        // Address poisoning: the start and the end match, every group between differs.
        let marked = LedgerConnect.groups(evm, against: poisoned, solana: false)
        #expect(marked.map(\.differs) == [false, false] + Array(repeating: true, count: 8) + [false])
        #expect(LedgerConnect.groups(evm, against: evm.lowercased(), solana: false).allSatisfy { !$0.differs })
        // Solana: no «0x», and a letter's case is a difference.
        let s = LedgerConnect.groups(sol, against: "hAgk" + sol.dropFirst(4), solana: true)
        #expect(s.first?.text == "HAgk" && s.first?.differs == true && s.dropFirst().allSatisfy { !$0.differs })
        #expect(s.count == 11)
    }
}

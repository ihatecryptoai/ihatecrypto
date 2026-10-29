import EVMChain
import Foundation
import Testing
@testable import TradeExecution
import WalletCore

private let me = "0x1111111111111111111111111111111111111111", vault = "0x2222222222222222222222222222222222222222"
private let evil = "0x3333333333333333333333333333333333333333", usdc = "0xaf88d065e77c8cc2239327c5edb3a432268e5831"
private func topic(_ a: String) -> String { "0x" + String(repeating: "0", count: 24) + a.dropFirst(2) }
private func event(_ s: String) -> String { "0x" + Keccak.hash256(Data(s.utf8)).hex }
private func word(_ n: UInt64) -> String { "0x" + String(repeating: "0", count: 48) + String(format: "%016llx", n) }
private func simulated(_ logs: [[String: Any]]) throws -> EVMSimulation {
    try EVMSimulation([["calls": [["status": "0x1", "logs": logs]]]], owner: me)
}

@Test func simulationsShowTheRightsAWalletHandsOut() throws {
    let approval = event("Approval(address,address,uint256)"), all = event("ApprovalForAll(address,address,bool)")
    let exact = try simulated([["address": usdc, "topics": [approval, topic(me), topic(vault)], "data": word(10_000_000)]])
    #expect(exact.grants == [.init(kind: "approve", asset: usdc, to: vault, unlimited: false)])
    #expect(ContractCallGuard.grantProblem(exact.grants, contract: vault) == nil)
    // To someone else, unlimited, over every NFT: refused. A revoke (zero) and someone else's approval give nothing.
    let other = try simulated([["address": usdc, "topics": [approval, topic(me), topic(evil)], "data": word(1)]])
    #expect(ContractCallGuard.grantProblem(other.grants, contract: vault) != nil)
    let unlimited = try simulated([["address": usdc, "topics": [approval, topic(me), topic(vault)], "data": "0x" + String(repeating: "f", count: 64)]])
    #expect(unlimited.grants.first?.unlimited == true && ContractCallGuard.grantProblem(unlimited.grants, contract: vault) != nil)
    let nfts = try simulated([["address": evil, "topics": [all, topic(me), topic(vault)], "data": word(1)]])
    #expect(ContractCallGuard.grantProblem(nfts.grants, contract: vault) != nil)
    let none = try simulated([["address": usdc, "topics": [approval, topic(me), topic(evil)], "data": word(0)],
                              ["address": usdc, "topics": [approval, topic(evil), topic(me)], "data": word(5)]])
    #expect(none.grants.isEmpty)
}

@Test func whatIsSignedMustStillDoWhatTheCardShowed() throws {
    var card = try simulated([]), now = try simulated([])
    card.changes = [usdc: -10_000_000, vault: 8_826_380]
    now.changes = [usdc: -10_000_000, vault: 8_820_000]
    #expect(ContractCallGuard.drift(card: card, now: now) == nil)
    now.changes[usdc] = -11_000_000
    #expect(ContractCallGuard.drift(card: card, now: now) != nil)
    now.changes = [usdc: -10_000_000, vault: 8_826_380, evil: -1]
    #expect(ContractCallGuard.drift(card: card, now: now) != nil)
    now.changes = card.changes
    now.grants = [.init(kind: "approve", asset: usdc, to: evil, unlimited: false)]
    #expect(ContractCallGuard.drift(card: card, now: now) != nil)
}

/// tokenMeta's decimals: a token that doesn't answer gets none, never 18 — an approve counted in 18 decimals for a
/// 6-decimal token is a trillion times what the user asked, and the cap would see a trillionth of what leaves.
@Test func decimalsAreReadNeverGuessed() {
    #expect(ABI.decimals(Data(hex: word(6))) == 6)                   // USDC's answer on Base
    #expect(ABI.decimals(Data(hex: word(18))) == 18)
    #expect(ABI.decimals(nil) == nil)                                // a revert (Permit2 on Base) or the node down
    #expect(ABI.decimals(Data()) == nil)                             // an EOA answers «0x»
    #expect(ABI.decimals(Data(hex: word(37))) == nil)                // past 36: refused, as Send and Swap do
    #expect(ABI.decimals(Data(hex: word(1 << 63))) == nil)           // passes ABI.uint, traps Int(…)
    #expect(ABI.decimals(Data(hex: "0x" + String(repeating: "f", count: 64))) == nil)   // past 64 bits
}

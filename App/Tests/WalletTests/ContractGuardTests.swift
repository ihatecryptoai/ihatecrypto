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

/// The cap for a protocol outside the list: an approve it can't price is refused like an outflow, never counted as $0 —
/// the contract may take the part of it this call leaves unspent later.
@Test func anApproveItCantPriceIsRefusedLikeAnOutflow() {
    let priced: [String: Decimal] = [usdc: 1, vault: 1]
    #expect(ContractCallGuard.atRisk(leaving: [], approve: (evil, "EVIL", 1_000_000), priced: priced) == .unpriced("EVIL"))
    #expect(ContractCallGuard.atRisk(leaving: [], approve: (evil, "EVIL", 0), priced: priced) == .usd(0))         // a revoke
    #expect(ContractCallGuard.atRisk(leaving: [(usdc, "USDC", 10)], approve: (vault, "VLT", 40), priced: priced) == .usd(40))
    #expect(ContractCallGuard.atRisk(leaving: [(usdc, "USDC", 100)], approve: (vault, "VLT", 1), priced: priced) == .usd(100))
    #expect(ContractCallGuard.atRisk(leaving: [(usdc, "USDC", 10), (vault, "VLT", 5)], approve: nil, priced: priced) == .usd(15))
    #expect(ContractCallGuard.atRisk(leaving: [], approve: (evil, "usdc", 30), priced: priced) == .usd(30))       // the dollar list
    #expect(ContractCallGuard.atRisk(leaving: [(evil, "EVIL", 1)], approve: nil, priced: priced) == .unpriced("EVIL"))
    // An outflow without decimals: priced, it can't be counted; unpriced (an NFT), the price is asked first.
    #expect(ContractCallGuard.atRisk(leaving: [(usdc, "USDC", nil)], approve: nil, priced: priced) == .uncounted("USDC"))
    #expect(ContractCallGuard.atRisk(leaving: [(evil, "EVIL", nil)], approve: nil, priced: priced) == .unpriced("EVIL"))
}

/// GoPlus Security's address_security answer, read as ScamList's verdict. Its rate limit comes as HTTP 200 with code
/// 4029: never read as clean. A clean contract shows only contract_address=1.
@Test func goPlusAnswersReadAsAVerdict() throws {
    func answer(_ fields: [String: String], code: Int = 1) -> Data {
        let all = ["cybercrime", "money_laundering", "number_of_malicious_contracts_created", "phishing_activities", "contract_address",
                   "blacklist_doubt", "stealing_attack", "sanctioned", "mixer", "honeypot_related_address", "data_source"]
        let result = Dictionary(uniqueKeysWithValues: all.map { ($0, $0 == "data_source" ? "" : "0") }).merging(fields) { $1 }
        return try! JSONSerialization.data(withJSONObject: ["code": code, "message": "ok", "result": result])
    }
    guard case .reported(let theft) = ScamList.verdict(answer(["stealing_attack": "1", "data_source": "SlowMist"])) else { Issue.record("theft"); return }
    #expect(theft.en.contains("theft") && theft.en.contains("SlowMist"), "\(theft.en)")
    #expect(theft.ru.contains("кража") && theft.ru.contains("SlowMist"), "\(theft.ru)")
    #expect(!theft.en.contains(";") && !theft.ru.contains(";"))
    guard case .reported(let both) = ScamList.verdict(answer(["phishing_activities": "1", "blacklist_doubt": "1", "data_source": "SlowMist,ScamSniffer"]))
    else { Issue.record("phishing"); return }
    #expect(both.en.contains("phishing") && both.en.contains("ScamSniffer") && both.ru.contains("фишинг"), "\(both)")
    #expect(ScamList.verdict(answer(["contract_address": "1"])) == .clean)
    #expect(ScamList.verdict(answer([:])) == .clean)
    guard case .reported = ScamList.verdict(answer(["number_of_malicious_contracts_created": "2"])) else { Issue.record("creator"); return }
    #expect(ScamList.verdict(Data(#"{"code":4029,"message":"too many requests"}"#.utf8)) == .unchecked)
    #expect(ScamList.verdict(answer(["stealing_attack": "1"], code: 4029)) == .unchecked)
    #expect(ScamList.verdict(Data(#"{"code":1,"message":"ok","result":{}}"#.utf8)) == .unchecked)
    #expect(ScamList.verdict(Data("<html>bad gateway</html>".utf8)) == .unchecked)
    // GoPlus's networks are asked as they are; one it doesn't serve (HyperEVM, Tempo) answers zeros even for a drainer,
    // so Ethereum's labels are asked: they're per address.
    #expect(ScamList.chain(8453) == 8453 && ScamList.chain(42161) == 42161 && ScamList.chain(56) == 56)
    #expect(ScamList.chain(999) == 1 && ScamList.chain(4217) == 1)
}

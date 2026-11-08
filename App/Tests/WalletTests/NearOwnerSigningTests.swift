import CryptoKit
import Foundation
import Testing
@testable import WalletCore
import EVMChain

@Test func stringifyKeepsKeyOrderAndWritesLikeJavaScript() throws {
    let raw = Data(#"{ "b": 1, "a": [ true, null, "x/y" ], "c": { "z": "é\n\u0001" } }"#.utf8)
    #expect(String(decoding: try NearOwnerSigning.stringify(raw), as: UTF8.self) == #"{"b":1,"a":[true,null,"x/y"],"c":{"z":"é\n\u0001"}}"#)
    #expect(String(decoding: try NearOwnerSigning.stringify(Data(#""a\/b""#.utf8)), as: UTF8.self) == #""a/b""#)
    #expect(throws: NearOwnerSigning.Problem.self) { try NearOwnerSigning.stringify(Data(#"{"a":1} x"#.utf8)) }
}

@Test func rawValueIsTheFieldAsItCame() throws {
    let object = Data(#"{"receiverId":"x.near","actions":[{"type":"FunctionCall","params":{"methodName":"m","args":{"z":1,"a":2}}}]}"#.utf8)
    #expect(String(decoding: try NearOwnerSigning.rawValue("actions", in: object), as: UTF8.self)
            == #"[{"type":"FunctionCall","params":{"methodName":"m","args":{"z":1,"a":2}}}]"#)
    #expect(throws: NearOwnerSigning.Problem.self) { try NearOwnerSigning.rawValue("missing", in: object) }
}

@Test func delegateActionIsNep366Borsh() {
    let key = Data(repeating: 7, count: 32)
    let action = NearOwnerSigning.delegateAction(sender: "a.near", receiver: "b.near", actions: [.functionCall(method: "m", args: Data("{}".utf8), gas: 5, deposit: 1)],
                                                 nonce: 9, maxBlockHeight: 100, publicKey: key)
    var expected = Data([6, 0, 0, 0]) + Data("a.near".utf8) + Data([6, 0, 0, 0]) + Data("b.near".utf8)
    expected += Data([1, 0, 0, 0, 2, 1, 0, 0, 0]) + Data("m".utf8) + Data([2, 0, 0, 0]) + Data("{}".utf8)
    expected += Data([5, 0, 0, 0, 0, 0, 0, 0]) + Data([1] + [UInt8](repeating: 0, count: 15))
    expected += Data([9, 0, 0, 0, 0, 0, 0, 0]) + Data([100, 0, 0, 0, 0, 0, 0, 0]) + Data([0]) + key
    #expect(action == expected)
}

@Test func signaturesVerifyUnderTheOwnersKey() throws {
    let key = Curve25519.Signing.PrivateKey()
    let nonce = Data(repeating: 3, count: 32)
    let sig = try NearOwnerSigning.nep413(message: #"{"type":"grant_issue"}"#, nonce: nonce, recipient: "api.example", privateKey: key.rawRepresentation)
    #expect(key.publicKey.isValidSignature(sig, for: NearOwnerSigning.nep413Hash(message: #"{"type":"grant_issue"}"#, nonce: nonce, recipient: "api.example")))
    #expect(throws: NearOwnerSigning.Problem.self) { try NearOwnerSigning.nep413(message: "m", nonce: Data(count: 31), recipient: "r", privateKey: key.rawRepresentation) }

    let signed = try NearOwnerSigning.signDelegate(receiver: "b.near", actions: [.functionCall(method: "m", args: Data("{}".utf8), gas: 5, deposit: 0)],
                                                   nonce: 1, maxBlockHeight: 2, privateKey: key.rawRepresentation)
    let body = signed.dropLast(65), sig2 = signed.suffix(64)
    #expect(signed[signed.count - 65] == 0)
    #expect(key.publicKey.isValidSignature(sig2, for: Data(SHA256.hash(data: Data([0x6E, 0x01, 0x00, 0x40]) + body))))
}

@Test func agentOwnerSignsOnlyWhatItCanSay() throws {
    let nonce = Data(repeating: 1, count: 32).base64EncodedString()
    func payload(_ message: String, _ recipient: String = "api.demo.agentsonintents.com") -> Data {
        try! JSONSerialization.data(withJSONObject: ["message": message, "recipient": recipient, "nonce": nonce])
    }
    let soon = ISO8601DateFormatter().string(from: Date().addingTimeInterval(20 * 86400))
    let grant = try AgentOwner.message(payload(#"{"type":"grant_issue","agent_id":"abcdef0123","expires_at":"\#(soon)"}"#))
    #expect(grant.doing.contains("agent wallet abcdef01") && grant.doing.contains(String(soon.prefix(10))))
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.message(payload(#"{"type":"grant_issue"}"#, "evil.example")) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.message(payload(#"{"type":"approval_vote"}"#)) }

    let create = Data(#"{"receiverId":"outlayer.near","actions":[{"type":"FunctionCall","params":{"methodName":"create_wallet","args":{"policy":{"budget":{"daily_usd":"20"},"destinations":{"mode":"only","list":[{"a":1}]}},"z":"x/y"},"gas":"100000000000000","deposit":"0"}}]}"#.utf8)
    // Nothing on the custody contract is signed until its method is pinned from a live run.
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(create) }
    let d = try AgentOwner.delegate(create, allowed: ["create_wallet"])
    #expect(d.doing == "create wallet, at most $20 a day, only to 1 address you chose")
    guard case .functionCall(_, let args, _, _) = d.actions[0] else { Issue.record("not a call"); return }
    #expect(String(decoding: args, as: UTF8.self) == #"{"policy":{"budget":{"daily_usd":"20"},"destinations":{"mode":"only","list":[{"a":1}]}},"z":"x/y"}"#)
    let elsewhere = Data(String(decoding: create, as: UTF8.self).replacingOccurrences(of: "outlayer.near", with: "evil.near").utf8)
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(elsewhere, allowed: ["create_wallet"]) }
    let paying = Data(String(decoding: create, as: UTF8.self).replacingOccurrences(of: #""deposit":"0""#, with: #""deposit":"1000""#).utf8)
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(paying, allowed: ["create_wallet"]) }
    // A key written twice reads one way here and another way on the server: refused.
    let twice = Data(String(decoding: create, as: UTF8.self).replacingOccurrences(of: #""mode":"only""#, with: #""mode":"only","mode":"any""#).utf8)
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(twice, allowed: ["create_wallet"]) }
    // Rules are stored only beside the rules the signer checked, under the custody's own hash.
    let store = NearTransaction.Action.functionCall(method: "store_wallet_policy", args: Data(#"{"agent_policy_hash":"2021a689"}"#.utf8), gas: 1, deposit: 0)
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.rulesShown([store], policy: nil) }
    try AgentOwner.rulesShown([store], policy: Data(#"{"frozen":true}"#.utf8))
    try AgentOwner.rulesShown([.functionCall(method: "freeze_wallet", args: Data("{}".utf8), gas: 1, deposit: 0)], policy: nil)
    // A grant for longer than a month isn't signed.
    let far = ISO8601DateFormatter().string(from: Date().addingTimeInterval(60 * 86400))
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.message(payload(#"{"type":"grant_issue","agent_id":"a","expires_at":"\#(far)"}"#)) }
}

@Test func x402AuthorizationOnlyForSmallPaymentsFromThisWallet() {
    let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", service = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8", usdc = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"
    let now: UInt64 = 1_800_000_000
    func a(_ to: String = service, value: UInt64 = 1000, before: UInt64 = 1_800_000_300, from: String = me, token: String = usdc) -> X402Authorization {
        X402Authorization(chainId: 8453, token: token, from: from, to: to, value: value, validBefore: before, nonce: Data(repeating: 1, count: 32))
    }
    #expect(a().refusal(owner: me, now: now) == nil)
    #expect(a(value: 5_000_001).refusal(owner: me, now: now) != nil)
    #expect(a(before: now + 7200).refusal(owner: me, now: now) != nil)
    #expect(a(from: service).refusal(owner: me, now: now) != nil)
    #expect(a(me).refusal(owner: me, now: now) != nil)
    #expect(a(token: "0x0000000000000000000000000000000000000001").refusal(owner: me, now: now) != nil)
}

@Test func anAgentPolicyPaysOnlyAddressesTheSignerKnows() throws {
    let owner = String(repeating: "ab", count: 32), mine = "So11111111111111111111111111111111111111112", evm = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
    func policy(_ list: String, mode: String = "only", extra: String = "") -> Data {
        Data(#"{"frozen":false,"actions":["withdraw"],"destinations":{"mode":"\#(mode)","list":[\#(list)]},"budget":{"daily_usd":"20"}\#(extra)}"#.utf8)
    }
    let ok = #"{"action":"transfer","address":"\#(owner)"},{"action":"withdraw","chain":"sol","address":"\#(mine)"},{"action":"withdraw","chain":"base","address":"\#(evm.lowercased())"}"#
    #expect(try AgentOwner.policy(policy(ok), owner: owner, own: [mine], known: [evm]) == "at most $20 a day, only to 3 addresses of yours")
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.policy(policy(ok, mode: "any"), owner: owner, own: [mine], known: [evm]) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.policy(policy(ok), owner: owner, own: [mine], known: []) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.policy(policy(ok, extra: #","native":{"near":{"transfer":true}}"#), owner: owner, own: [mine], known: [evm]) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.policy(policy(#"{"action":"transfer","address":"evil.near"}"#), owner: owner, own: [], known: []) }
}

/// agent_create as the Agent API asked it live (2026-11-04): store_wallet_policy with 0.01882 NEAR for storage is
/// signed and said; more NEAR, or NEAR on another method, is refused.
@Test func agentCreateStoresItsPolicyForASmallStorageDeposit() throws {
    func call(_ method: String, _ deposit: String) -> Data {
        Data(#"{"actions":[{"type":"FunctionCall","params":{"gas":"100000000000000","args":{"wallet_pubkey":"ed25519:00","encrypted_data":"AQ==","wallet_signature":"00","agent_policy_hash":"00","storage_refund_to":"relayer.near"},"deposit":"\#(deposit)","methodName":"\#(method)"}}],"receiverId":"outlayer.near"}"#.utf8)
    }
    let d = try AgentOwner.delegate(call("store_wallet_policy", "18820000000000000000000"))
    #expect(d.doing.contains("store wallet policy"))
    #expect(d.doing.contains("0.01882 NEAR"))
    #expect(d.doing.contains("not to you"))
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(call("store_wallet_policy", "100000000000000000000000")) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(call("store_wallet_policy_v2", "0")) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(call("withdraw", "18820000000000000000000"), allowed: ["withdraw"]) }
}

/// A grant as the Agent API wrote it live (2026-11-04): no «type», a domain; signed for at most 31 days, to the host
/// the payload is addressed to.
@Test func agentGrantIsReadByItsDomain() throws {
    func grant(days: Double, inner: String = "api.demo.agentsonintents.com") -> Data {
        let ms = Int((Date().timeIntervalSince1970 + days * 86400) * 1000)
        let message = #"{"agent_id":"ab12cd34ef","credential":"00","domain":"near-intents-agent-api.agent-grant.v6","expires_at_ms":\#(ms),"label":"test","network":"mainnet","owner":{"account_id":"owner.near","type":"near"},"recipient":"\#(inner)"}"#
        let nonce = Data(repeating: 7, count: 32).base64EncodedString()
        return try! JSONSerialization.data(withJSONObject: ["message": message, "nonce": nonce, "recipient": "api.demo.agentsonintents.com"])
    }
    let m = try AgentOwner.message(grant(days: 30))
    #expect(m.doing.hasPrefix("let this Mac run agent wallet ab12cd34"))
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.message(grant(days: 60)) }
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.message(grant(days: 30, inner: "evil.example")) }
}

/// agent_freeze as the API asked it live (2026-11-05): the rules stored again, then freeze_wallet, no NEAR attached.
@Test func agentFreezeIsSignedAndSaidButCostsNoNEAR() throws {
    func freeze(_ deposit: String) -> Data {
        Data(#"{"actions":[{"type":"FunctionCall","params":{"gas":"50000000000000","args":{"wallet_pubkey":"ed25519:00","encrypted_data":"AQ==","wallet_signature":"00","agent_policy_hash":"00"},"deposit":"0","methodName":"store_wallet_policy"}},{"type":"FunctionCall","params":{"gas":"50000000000000","args":{"wallet_pubkey":"ed25519:00"},"deposit":"\#(deposit)","methodName":"freeze_wallet"}}],"receiverId":"outlayer.near"}"#.utf8)
    }
    let d = try AgentOwner.delegate(freeze("0"))
    #expect(d.actions.count == 2 && d.doing.contains("freeze the agent wallet") && !d.doing.contains(";"))
    #expect(throws: NearOwnerSigning.Problem.self) { try AgentOwner.delegate(freeze("1000000000000000000000000")) }
    // agent_unfreeze as the API asked it live (2026-11-08): the rules stored unfrozen, then unfreeze_wallet.
    let unfreeze = Data(String(decoding: freeze("0"), as: UTF8.self).replacingOccurrences(of: "\"freeze_wallet\"", with: "\"unfreeze_wallet\"").utf8)
    let u = try AgentOwner.delegate(unfreeze)
    #expect(u.actions.count == 2 && u.doing.contains("unfreeze the agent wallet") && !u.doing.contains(";"))
}

import CryptoKit
import EVMChain
import Foundation
@testable import RelayBridge
@testable import SolanaChain
import Testing
import TxGuard
import WalletCore

// MARK: - Relay's payment by signature (EIP-3009)

/// Hardhat's first public test key and its address.
private let hardhatKey = Data(hex: "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")!
private let hardhat = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
private let relayProxy = "0xccc88a9d1b4ed6b0eaba998850414b24f1c315be"
private let nonce = Data(hex: "1a66a6de1f9f27675cfd809e56c7d573a25922c450f6b56ced9013f360512be7")!

private func permit(chain: UInt64 = 8453, token: String = EVMRPC.baseUSDC, from: String = hardhat, to: String = relayProxy,
                    validBefore: UInt64 = 1_792_706_124) -> Permit3009 {
    Permit3009(chainId: chain, token: token, from: from, to: to, value: 3_000_000, validBefore: validBefore, nonce: nonce)
}

@Test func dollarDomainsMatchTheContracts() throws {
    // DOMAIN_SEPARATOR() read from each contract on 2026-10-23.
    let onChain: [(UInt64, String, String)] = [
        (8453, EVMRPC.baseUSDC, "02fa7265e7c5d81118673727957699e4d68f74cd74b7db77da710fe8a2c7834f"),
        (42161, EVMRPC.arbitrumUSDC, "08d11903f8419e68b1b8721bcbe2e9fc68569122a77ef18c216f10b3b5112c78"),
        (10, EVMRPC.optimismUSDC, "26d9c34bb1a1c312f69c53b2d93b8be20faafba63af2438c6811713c9b1f933f"),
        (137, EVMRPC.polygonUSDC, "caa2ce1a5703ccbe253a34eb3166df60a705c561b44b192061e28f2a985be2ca"),
        (1, EVMRPC.ethereumUSDC, "06c37168a7db5138defc7866392bb87a741f9b3d104deb5094588ce041cae335"),
        (4663, EVMRPC.robinhoodUSDG, "7a3d7400b27830f4f91c2c16a082486d67c1befecaec2f53b33f1f35d5b62036"),
    ]
    for (chain, token, separator) in onChain {
        #expect(try permit(chain: chain, token: token).typed().parts().domain.hex == separator, "\(chain)")
    }
    #expect(Permit3009.tokens.count == onChain.count)
    #expect(!Permit3009.supports(chainId: 56, token: EVMRPC.bnbUSDC) && !Permit3009.supports(chainId: 999, token: EVMRPC.hyperevmUSDC))
}

@Test func paymentBySignatureMatchesFoundry() throws {
    // `cast wallet sign --data` of the same ReceiveWithAuthorization with the same key.
    let signature = try EVMSigner.sign(digest: try permit().typed().parts().digest, privateKey: hardhatKey)
    #expect(signature.r.hex + signature.s.hex == "6e99da39fc03c5e7a931609dedf1f20b1e89827f270ca103cfff9aa1e2934dab2dabf94c0a0783ff6ecaabb2b3eff07dba71e6e4e56d71fad2c477014842e0eb")
    #expect(signature.v == 28)
}

@Test func paymentBySignatureGoesOnlyToRelayForAnHour() {
    let now: UInt64 = 1_792_705_800
    #expect(permit().refusal(owner: hardhat.lowercased(), now: now) == nil)
    #expect(permit(to: "0x5567954cd026cebfe486062cb75848e0f1c12e58").refusal(owner: hardhat, now: now) == nil)
    #expect(permit(to: "0x000000000000000000000000000000000000dead").refusal(owner: hardhat, now: now) != nil)
    #expect(permit(from: "0x9858EfFD232B4033E47d90003D41EC34EcaEda94").refusal(owner: hardhat, now: now) != nil)
    #expect(permit(chain: 56, token: EVMRPC.bnbUSDC).refusal(owner: hardhat, now: now) != nil)
    #expect(permit(token: EVMRPC.arbitrumUSDC).refusal(owner: hardhat, now: now) != nil)  // Arbitrum's USDC named on Base
    #expect(permit(validBefore: now + 3601).refusal(owner: hardhat, now: now) != nil)
    #expect(permit(validBefore: now).refusal(owner: hardhat, now: now) != nil)
}

/// Relay's authorize1 step for 3 USDC from Base, as /quote/v2 gave it with usePermit (2026-10-23, the public test address).
private func relayStep(items: Int = 1) -> [[String: Any]] {
    let item: [String: Any] = ["status": "incomplete", "data": [
        "sign": ["signatureKind": "eip712", "primaryType": "ReceiveWithAuthorization",
                 "domain": ["name": "USD Coin", "version": "2", "chainId": 8453, "verifyingContract": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"],
                 "value": ["from": "0x9858EfFD232B4033E47d90003D41EC34EcaEda94", "to": relayProxy, "value": "3000000", "validAfter": 0,
                           "validBefore": 1_792_706_124, "nonce": "0x" + nonce.hex]],
        "post": ["endpoint": "/execute/permits", "method": "POST",
                 "body": ["kind": "eip3009", "requestId": "0x1792705524576d6b75c5353aa74b090990ce6143da4bf85d9c2661c7481d4793", "api": "swap"]],
    ]]
    return [["id": "authorize1", "kind": "signature", "items": Array(repeating: item, count: items)]]
}

@Test func relaysPermitStepIsReadAndAnythingMoreIsNot() throws {
    let p = try #require(Relay.permit(in: relayStep()))
    #expect(p.chainId == 8453 && p.token == EVMRPC.baseUSDC && p.to == relayProxy && p.value == "3000000")
    #expect(p.validAfter == 0 && p.validBefore == 1_792_706_124 && p.nonce == nonce && p.endpoint == "/execute/permits")
    #expect(p.domainName == "USD Coin" && p.domainVersion == "2")
    #expect((try JSONSerialization.jsonObject(with: p.body) as? [String: Any])?["kind"] as? String == "eip3009")
    // Two items (v3.2's MulticallAuthorization first), or an approval step before it: not a payment by signature alone.
    #expect(Relay.permit(in: relayStep(items: 2)) == nil)
    #expect(Relay.permit(in: [["id": "approve", "kind": "transaction", "items": [[String: Any]]()]] + relayStep()) == nil)
}

// MARK: - Relay's v3.2 proxy: MulticallAuthorization + EIP-3009

private let relayRouter32 = "0x0729e2a4716d9f7dd17858d5d504426d228697f7"
private let relayRelayer = "0xf70da97812cb96acdf810712aa562db8dfa3dbef"

/// Relay's deposit to another network as its quotes build it: cleanupErc20sViaCall([token], [depository], [depositErc20(
/// depositor, token, id)], [0]) on the router.
private func depositCall(depositor: String = hardhat, token: String = EVMRPC.baseUSDC, to: String = RelayMulticallPermit.depository,
                         amount: UInt64 = 0, selector: String = "5a1ee3ac") throws -> Data {
    let id = Data(hex: "db8cadf99c6d377809bd725c1a3ac01543447e320535f0d773fa8408e063a00a")!
    let deposit = Data(hex: selector)! + (try ABI.address(depositor)) + (try ABI.address(token)) + id
    var args = ABI.uint(128) + ABI.uint(192) + ABI.uint(256) + ABI.uint(480)
    args += ABI.uint(1) + (try ABI.address(token)) + ABI.uint(1) + (try ABI.address(to))
    args += ABI.uint(1) + ABI.uint(32) + ABI.uint(UInt64(deposit.count)) + deposit + Data(count: (32 - deposit.count % 32) % 32)
    args += ABI.uint(1) + ABI.uint(amount)
    return Data(hex: "73b7bb2f")! + args
}

private func multicall(calls: [RelayMulticallPermit.Call]? = nil, from: String = hardhat) throws -> RelayMulticallPermit {
    RelayMulticallPermit(chainId: 8453, token: EVMRPC.baseUSDC, from: from, value: 1_000_000, validBefore: 1_792_716_428,
                         relayer: relayRelayer, refundTo: relayRelayer, nftRecipient: relayRelayer,
                         metadata: Data(hex: "9a7b1e444964cb59dd53c1e4674c9b30248af8fb8a6462445df6f0484517297100")!,
                         calls: try calls ?? [.init(target: relayRouter32, allowFailure: false, value: 0, callData: depositCall())])
}

@Test func relaysV32SignaturesPassItsDeployedProxy() throws {
    // eth_call of permit3009TransferAndMulticall on Base's v3.2 proxy (2026-10-23) with these two signatures, from Relay's
    // relayer: the proxy took both and the router deposited the dollar (Hardhat's address given no code and 1 USDC for the
    // call); with the depositor changed it reverted InvalidMulticallSignature.
    let m = try multicall()
    #expect(try m.digest().hex == "8b3dd635048db8d0d23d504ee3dc793c97f2d616837e7839e83a26c4343dc3c3")
    let first = try EVMSigner.sign(digest: try m.digest(), privateKey: hardhatKey)
    #expect(first.r.hex + first.s.hex + String(format: "%02x", first.v) == "d840fdeea2a59e34993fe910060d9c58b9ced6f5d752d3321eb538be0d27c1343b567d328ec87edc05f654cb32efb36e12e872ba00d9df270cc5930e7687fd861b")
    let second = try EVMSigner.sign(digest: try m.permit().typed().parts().digest, privateKey: hardhatKey)
    #expect(second.r.hex == "594f03a82e680819b75c4598edf59a8a1d17f5683e7e71ef99a9063e7adf132a" && second.v == 28)
    #expect(try m.permit().to == RelayMulticallPermit.proxy && m.permit().nonce == m.digest())
}

@Test func relaysV32CallsMustBeOneDepositForThisWallet() throws {
    let now: UInt64 = 1_792_716_000
    #expect(try multicall().refusal(owner: hardhat, now: now) == nil)
    let stranger = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94"
    let bad: [[RelayMulticallPermit.Call]] = [
        [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall(depositor: stranger))],   // someone else's deposit
        [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall(to: stranger))],          // not the depository
        [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall(token: EVMRPC.arbitrumUSDC))],
        [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall(amount: 5))],             // a part, not this dollar
        [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall(selector: "a9059cbb"))],  // a transfer
        [.init(target: stranger, allowFailure: false, value: 0, callData: try depositCall())],                          // not Relay's router
        [.init(target: relayRouter32, allowFailure: true, value: 0, callData: try depositCall())],
        [.init(target: relayRouter32, allowFailure: false, value: 1, callData: try depositCall())],
        [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall()),
         .init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall())],
    ]
    for calls in bad { #expect(try multicall(calls: calls).refusal(owner: hardhat, now: now) != nil) }
    #expect(try multicall(calls: [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall(amount: 1_000_000))])
        .refusal(owner: hardhat, now: now) == nil)
    #expect(try multicall(from: stranger).refusal(owner: hardhat, now: now) != nil)
    let elsewhere = RelayMulticallPermit(chainId: 8453, token: EVMRPC.baseUSDC, from: hardhat, value: 1_000_000, validBefore: 1_792_716_428,
                                         relayer: relayRelayer, refundTo: stranger, nftRecipient: relayRelayer, metadata: Data(),
                                         calls: [.init(target: relayRouter32, allowFailure: false, value: 0, callData: try depositCall())])
    #expect(elsewhere.refusal(owner: hardhat, now: now)?.contains("leftovers") == true)
}

@Test func relaysV32StepIsReadInOrder() throws {
    let m = try multicall()
    let call: [String: Any] = ["target": relayRouter32, "allowFailure": false, "value": "0", "callData": "0x" + (try depositCall()).hex]
    let first: [String: Any] = ["status": "incomplete", "data": [
        "sign": ["signatureKind": "eip712", "primaryType": "MulticallAuthorization",
                 "domain": ["name": "RelayApprovalProxy", "version": "3.1", "chainId": 8453, "verifyingContract": RelayMulticallPermit.proxy],
                 "value": ["from": hardhat, "relayer": relayRelayer, "msgValue": "0", "refundTo": relayRelayer, "nftRecipient": relayRelayer,
                           "metadata": "0x" + m.metadata.hex, "fundingIndex": 0, "calls": [call],
                           "funding": [["token": EVMRPC.baseUSDC, "from": hardhat, "value": "1000000", "validAfter": 0, "validBefore": 1_792_716_428]]]],
        "post": ["endpoint": "/execute/permits", "method": "POST", "body": ["kind": "multicall-authorization", "requestId": "0x01", "api": "swap"]],
    ]]
    var second = relayStep()[0]["items"] as! [[String: Any]]
    var data = second[0]["data"] as! [String: Any], sign = data["sign"] as! [String: Any], value = sign["value"] as! [String: Any]
    (value["to"], value["value"], value["nonce"]) = (RelayMulticallPermit.proxy, "1000000", "0x" + (try m.digest()).hex)
    sign["value"] = value; data["sign"] = sign; second[0]["data"] = data
    let step = try #require(Relay.multicall(in: [["id": "authorize1", "kind": "signature", "items": [first, second[0]]]]))
    #expect(step.domainName == "RelayApprovalProxy" && step.domainVersion == "3.1" && step.fundingIndex == 0 && step.calls.count == 1)
    #expect(step.permit.nonce == (try m.digest()) && step.permit.to == RelayMulticallPermit.proxy)
    #expect((try JSONSerialization.jsonObject(with: step.body) as? [String: Any])?["kind"] as? String == "multicall-authorization")
    // The EIP-3009 item alone is the v3 form, not this one; and the v3 parser won't take two items.
    #expect(Relay.multicall(in: relayStep()) == nil && Relay.permit(in: [["id": "authorize1", "kind": "signature", "items": [first, second[0]]]]) == nil)
}

// MARK: - Jupiter paying a Solana fee

private let ownerKey = try! Curve25519.Signing.PrivateKey(rawRepresentation: Data(repeating: 7, count: 32))
private let sponsorKey = try! Curve25519.Signing.PrivateKey(rawRepresentation: Data(repeating: 9, count: 32))
private let owner = Base58.encode(ownerKey.publicKey.rawRepresentation)
private let sponsor = Base58.encode(sponsorKey.publicKey.rawRepresentation)
private let stranger = "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw"
private let blockhash = "7WktogJEd2wQ9eH2oWusmcoFTgeYi6rS632UviTBJ2jm"

/// A legacy message from readable parts: `signers` keys first (the sponsor, then us), the rest read-only, then System.
private func sponsored(signers: [String], others: [String] = [], _ transfers: [(from: Int, to: Int)], lamports: UInt64 = 1000) throws -> (wire: Data, message: SolanaMessage) {
    let keys = signers + others + [SolanaGuard.system]
    var m = Data([UInt8(signers.count), 0, 1]) + SolanaTransaction.compact(keys.count)
    for key in keys { m += try Base58.decode(key) }
    m += try Base58.decode(blockhash) + SolanaTransaction.compact(transfers.count)
    for t in transfers {
        m += Data([UInt8(keys.count - 1)]) + SolanaTransaction.compact(2) + Data([UInt8(t.from), UInt8(t.to)])
        m += SolanaTransaction.compact(12) + Data([2, 0, 0, 0]) + withUnsafeBytes(of: lamports.littleEndian) { Data($0) }
    }
    return (SolanaTransaction.compact(signers.count) + Data(count: 64 * signers.count) + m, try SolanaMessage.parse(message: m))
}

@Test func guardLetsJupiterPayButChecksWhatWeSign() throws {
    // The sponsor pays a stranger from its own SOL; we send to ourselves.
    let (_, m) = try sponsored(signers: [sponsor, owner], others: [stranger], [(0, 2), (1, 1)])
    let paid = SolanaPolicy(owner: owner, sponsor: sponsor)
    #expect(m.requiredSignatures == 2)
    #expect(SolanaGuard.check(m, accounts: m.staticKeys, policy: paid, tokenOwners: [:]).ok)
    // Without the sponsor named, two signers and someone else paying are refused as before.
    #expect(!SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: owner), tokenOwners: [:]).ok)
    // Another sponsor than the one named, or our SOL to a stranger, stops it.
    #expect(!SolanaGuard.check(m, accounts: m.staticKeys, policy: SolanaPolicy(owner: owner, sponsor: stranger), tokenOwners: [:]).ok)
    let (_, ours) = try sponsored(signers: [sponsor, owner], others: [stranger], [(1, 2)])
    #expect(SolanaGuard.check(ours, accounts: ours.staticKeys, policy: paid, tokenOwners: [:]).problems.contains { $0.contains(stranger) })
    // Back to the sponsor (the rent it fronted, as Jupiter's transaction ends): allowed up to the cap, in sum.
    let (_, back) = try sponsored(signers: [sponsor, owner], [(1, 0)])
    #expect(SolanaGuard.check(back, accounts: back.staticKeys, policy: paid, tokenOwners: [:]).ok)
    #expect(!SolanaGuard.check(back, accounts: back.staticKeys, policy: SolanaPolicy(owner: owner), tokenOwners: [:]).ok)
    let (_, many) = try sponsored(signers: [sponsor, owner], Array(repeating: (1, 0), count: 6))
    #expect(SolanaGuard.check(many, accounts: many.staticKeys, policy: paid, tokenOwners: [:]).ok)  // 6 × 1000 lamports
    let (_, rent) = try sponsored(signers: [sponsor, owner], [(1, 0)], lamports: 1_488_440)  // what Jupiter's took back, 2026-10-23
    #expect(SolanaGuard.check(rent, accounts: rent.staticKeys, policy: paid, tokenOwners: [:]).ok)
    let (_, over) = try sponsored(signers: [sponsor, owner], [(1, 0), (1, 0)], lamports: 2_600_000)
    #expect(SolanaGuard.check(over, accounts: over.staticKeys, policy: paid, tokenOwners: [:]).problems.contains { $0.contains("more than the rent") })
    // Our wallet first, or a third signer, isn't allowed either.
    let (_, swapped) = try sponsored(signers: [owner, sponsor], [(0, 0)])
    #expect(!SolanaGuard.check(swapped, accounts: swapped.staticKeys, policy: paid, tokenOwners: [:]).ok)
    let (_, three) = try sponsored(signers: [sponsor, owner, stranger], [(1, 1)])
    #expect(!SolanaGuard.check(three, accounts: three.staticKeys, policy: paid, tokenOwners: [:]).ok)
}

@Test func ourSignatureGoesInTheSecondSlot() throws {
    let (wire, _) = try sponsored(signers: [sponsor, owner], [(1, 1)])
    let signed = try SolanaTransaction.sign(wire, privateKey: ownerKey.rawRepresentation, sponsor: sponsorKey.publicKey.rawRepresentation).wire
    #expect(signed.prefix(65) == wire.prefix(65) && signed.dropFirst(129) == wire.dropFirst(129))
    #expect(ownerKey.publicKey.isValidSignature(signed.subdata(in: 65..<129), for: Data(wire.dropFirst(129))))
    // Without the sponsor, or with another one, nothing is signed.
    #expect(throws: (any Error).self) { try SolanaTransaction.sign(wire, privateKey: ownerKey.rawRepresentation) }
    #expect(throws: (any Error).self) {
        try SolanaTransaction.sign(wire, privateKey: ownerKey.rawRepresentation, sponsor: try Base58.decode(stranger))
    }
    // A wire with one signature slot for a two-signer message is refused.
    #expect(throws: (any Error).self) {
        try SolanaTransaction.sign(SolanaTransaction.compact(1) + wire.dropFirst(65), privateKey: ownerKey.rawRepresentation,
                                   sponsor: sponsorKey.publicKey.rawRepresentation)
    }
}

// MARK: - BNB Chain paying the gas (MegaFuel)

@Test func sponsoredTransferMatchesFoundry() throws {
    // `cast mktx --legacy --gas-price 0 --gas-limit 60000 --nonce 5 --chain 56` of the same transfer with Hardhat's key.
    let t = SponsoredTransfer(chainId: 56, nonce: 5, gasLimit: 60_000, token: EVMRPC.bnbUSDC, to: "0xd2eb0c130cd38ee0da3132d33145a10875879acb",
                              amount: "3000000000000000000")
    #expect(t.refusal == nil)
    #expect(try t.signed(privateKey: hardhatKey).raw.hex == "f8a5058082ea60948ac76a51cc950d9822d68b83fe1ad97b32cd580d80b844a9059cbb000000000000000000000000d2eb0c130cd38ee0da3132d33145a10875879acb00000000000000000000000000000000000000000000000029a2241af62c00008193a0545a46241bc7e15daf771c2757bcbc18b7df4087132a189326ab878cc697ef5ea019d448fa2fe2c096adb2bbca17ff91fbef98d80f84ad79ffefc850d8eac6aa56")
}

@Test func sponsoredTransferIsOnlyAKnownDollar() {
    func t(chain: UInt64 = 56, token: String = EVMRPC.bnbUSDC, to: String = "0xd2eb0c130cd38ee0da3132d33145a10875879acb", amount: String = "1",
           gas: UInt64 = 80_000) -> SponsoredTransfer {
        SponsoredTransfer(chainId: chain, nonce: 0, gasLimit: gas, token: token, to: to, amount: amount)
    }
    #expect(t().refusal == nil)
    #expect(t(token: "0x55d398326f99059ff775485246999027b3197955").refusal != nil)   // not a dollar on the list
    #expect(t(chain: 1).refusal != nil)
    #expect(t(amount: "0").refusal != nil && t(amount: "1.5").refusal != nil)
    #expect(t(to: "0x0000000000000000000000000000000000000000").refusal != nil && t(to: "nobody").refusal != nil)
    #expect(t(gas: 200_000).refusal != nil)
}

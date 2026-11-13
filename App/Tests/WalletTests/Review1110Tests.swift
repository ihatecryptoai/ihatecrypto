import EVMChain
import Foundation
import HyperliquidSigning
import Testing
import WalletCore

/// The 2026-11-10 review. anvil's first well-known address (a public test vector).
private let owner = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
private let key = Data(hex: "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")!

/// An address without its «0x» reads as the same 20 bytes, while every check that compares it as text misses it: the
/// wallet's own address called with executeBatch passed `selfCall`. Nothing that is signed takes one.
@Test func anAddressWithoutItsPrefixIsNotSigned() throws {
    let bare = String(owner.dropFirst(2)), batch = Data([0x34, 0xfc, 0xd5, 0xbe])
    #expect(Data(evmAddress: owner)?.count == 20 && Data(hex: bare)?.count == 20)
    for odd in [bare, "0X" + bare, owner + "00", String(owner.dropLast(2)), " " + owner, ""] { #expect(Data(evmAddress: odd) == nil) }
    // A sign where a digit goes: UInt8(_:radix:) read «+e» as 0x0e, so 42 characters with the prefix still spelled
    // Relay's depository in a text no check knew. No hex reader takes one now.
    let signed = "0x4cd0+e387622c35bddb9b4c962c136462338bc31"
    #expect(signed.count == 42 && Data(evmAddress: signed) == nil && Data(hex: signed) == nil)
    for odd in ["+e", "-0", "0x+f", "0g", "é0"] { #expect(Data(hex: odd) == nil) }
    #expect(Data(hex: "0x0aF9") == Data([0x0a, 0xf9]) && Data(hex: "0AF9") == Data([0x0a, 0xf9]) && Data(hex: "") == Data())
    // What let it through: the text comparison says «not this wallet».
    #expect(Batch7702.selfCall(to: bare, data: batch, owner: owner) == nil)
    // A transaction to it has no bytes to sign, and neither has a batch's call, an ABI word or typed data.
    let tx = EVMTransaction(chainId: 8453, nonce: 0, maxPriorityFeePerGas: 1, maxFeePerGas: 1, gasLimit: 100_000, to: bare, value: 0, data: batch)
    #expect(throws: (any Error).self) { try tx.signed(privateKey: key) }
    #expect(throws: (any Error).self) { try tx.unsignedPayload() }
    #expect(Batch7702.problems([.init(to: bare, data: batch)], owner: owner, chainId: 8453).contains { $0.hasPrefix("Not an address") })
    #expect(throws: (any Error).self) { try Batch7702.executeBatch([.init(to: bare, data: batch)]) }
    #expect(throws: (any Error).self) { try ABI.address(bare) }
    #expect(throws: (any Error).self) {
        try EIP712.digest(domain: [.init("verifyingContract", "address", .address(bare))], type: "T", message: [.init("a", "address", .address(owner))])
    }
    // With the prefix, in any case, it signs as before.
    #expect(throws: Never.self) { try EVMTransaction(chainId: 8453, nonce: 0, maxPriorityFeePerGas: 1, maxFeePerGas: 1, gasLimit: 21_000,
                                                     to: owner.lowercased(), value: 1, data: Data()).signed(privateKey: key) }
}

/// A price of 1e100 times a size of 1e100 is past what Decimal holds: NaN, which is under every limit and counts as
/// nothing. One such order in a batch took the real one beside it past the trading key's limits.
@Test func anOrderPastDecimalTakesThePerson() throws {
    func order(_ p: String, _ s: String) -> MsgPack {
        .map([("a", .uint(0)), ("b", .bool(true)), ("p", .string(p)), ("s", .string(s)), ("r", .bool(false)),
              ("t", .map([("limit", .map([("tif", .string("Ioc"))]))]))])
    }
    func action(_ orders: [MsgPack]) -> MsgPack { .map([("type", .string("order")), ("orders", .array(orders)), ("grouping", .string("na"))]) }
    #expect(HLAction.opening(action([order("100", "1.5")])) == .dollars(150))
    for junk in [order("1e100", "1e100"), order("NaN", "1"), order("1", "NaN")] {
        for batch in [[junk], [order("100000", "50"), junk], [junk, order("100000", "50")]] {
            guard case .person = HLAction.opening(action(batch)) else { Issue.record("an order the signer can't count went without the person"); continue }
        }
    }
}

/// setApprovalForAll(operator, 2): a contract that doesn't hold the flag to 0 or 1 (ABI coder v1, Solady) reads it as
/// yes, while the card read «last byte isn't 1» as a revoke. Any non-zero word grants, and is refused.
@Test func anyNonZeroApprovalForAllIsAGrant() throws {
    let thief = "0x1111111111111111111111111111111111111111"
    func call(_ flag: Data) throws -> EVMCall { EVMCall.decode(Data(hex: "a22cb465")! + (try ABI.address(thief)) + flag) }
    #expect(try call(ABI.uint(0)) == .setApprovalForAll(operator: thief, approved: false))
    #expect(try call(ABI.uint(0)).refusal(sender: owner) == nil)
    for flag in [ABI.uint(1), ABI.uint(2), ABI.uint(256), Data([0x80]) + Data(count: 31), Data(repeating: 0xff, count: 32)] {
        #expect(try call(flag) == .setApprovalForAll(operator: thief, approved: true))
        #expect(try call(flag).refusal(sender: owner) != nil)
    }
}

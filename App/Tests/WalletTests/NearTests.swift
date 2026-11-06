import CryptoKit
import Foundation
import Testing
@testable import WalletCore

/// near-api-js «serialize transfer tx»: test.near → whatever.near, 1 yoctoNEAR, nonce 1 (the deposit is a 16-byte u128).
@Test func nearTransferSerializesLikeNearApiJs() throws {
    let tx = NearTransaction.serialize(signer: "test.near", publicKey: try Base58.decode("Anu7LYDfpLtkP7E16LT9imXF694BdQaa9ufVkQiwTQxC"), nonce: 1,
                                       receiver: "whatever.near", blockHash: try Base58.decode("244ZQ9cgj3CQ6bWBdytfrJMuMQ1jdXLFGnr4HhvtCTnM"),
                                       actions: [.transfer(deposit: 1)])
    #expect(tx.hex == "09000000746573742e6e65617200917b3d268d4b58f7fec1b150bd68d69be3ee5d4cc39855e341538465bb77860d01000000000000000d00000077686174657665722e6e6561720fa473fd26901df296be6adc4cc4df34d040efa2435224b6986910e630c2fef6010000000301000000000000000000000000000000")
}

@Test func nearIntentDepositIsOneWrapTransactionThatVerifies() throws {
    let key = Curve25519.Signing.PrivateKey(), account = key.publicKey.rawRepresentation.hex
    let deposit = String(repeating: "ab", count: 32), amount: UInt128 = 100_000_000_000_000_000_000_000   // 0.1 NEAR
    let actions = NearTransaction.intentDeposit(account: account, depositAddress: deposit, amount: amount)
    #expect(actions.count == 4)
    guard case .functionCall("near_deposit", _, _, amount) = actions[2],
          case .functionCall("ft_transfer", let args, _, 1) = actions[3] else { Issue.record("wrong actions"); return }
    #expect(String(decoding: args, as: UTF8.self) == #"{"amount":"100000000000000000000000","receiver_id":"\#(deposit)"}"#)
    let signed = try NearTransaction.sign(receiver: NearTransaction.wrapContract, nonce: 7, blockHash: Data(repeating: 1, count: 32),
                                          actions: actions, privateKey: key.rawRepresentation)
    let body = signed.signed.dropLast(65)
    #expect(Data(SHA256.hash(data: body)) == signed.hash)
    #expect(signed.signed.dropLast(64).last == 0)   // ed25519 signature tag
    #expect(key.publicKey.isValidSignature(signed.signed.suffix(64), for: signed.hash))
    #expect(body.prefix(68) == NearTransaction.serialize(signer: account, publicKey: key.publicKey.rawRepresentation, nonce: 7, receiver: "wrap.near",
                                                        blockHash: Data(repeating: 1, count: 32), actions: []).prefix(68))
    #expect(NearTransaction.near(amount) == "0.1")
    #expect(NearTransaction.near(758_442_130_571_663_900_000_000) == "0.758442")
}

@Test func nearDeleteAccountIsActionSevenWithTheBeneficiary() {
    let beneficiary = String(repeating: "a", count: 64)
    let tx = NearTransaction.serialize(signer: "me.near", publicKey: Data(count: 32), nonce: 1, receiver: "me.near",
                                       blockHash: Data(count: 32), actions: [.deleteAccount(beneficiary: beneficiary)])
    // … actions count (u32 = 1), then variant 7 and the borsh string: u32 length, then the bytes.
    let tail = Data([1, 0, 0, 0, 7, 64, 0, 0, 0]) + Data(beneficiary.utf8)
    #expect(tx.suffix(tail.count) == tail)
}

@Test func nearTransferToANamedAccountCarriesItsName() {
    let tx = NearTransaction.serialize(signer: "me.near", publicKey: Data(count: 32), nonce: 7, receiver: "bybit.near",
                                       blockHash: Data(count: 32), actions: [.transfer(deposit: 1)])
    // receiver as a borsh string, then after the block hash: one action, variant 3, the u128 deposit.
    #expect(tx.range(of: Data([10, 0, 0, 0]) + Data("bybit.near".utf8)) != nil)
    #expect(tx.suffix(21) == Data([1, 0, 0, 0, 3, 1] + [UInt8](repeating: 0, count: 15)))
}

@Test func nearAddressesAreCheckedLikeTheOthers() {
    #expect(AddressCheck.problem(String(repeating: "ab", count: 32), network: .near) == nil)
    #expect(AddressCheck.problem("bybit.near", network: .near) == nil)
    #expect(AddressCheck.problem("deposit.binance.near", network: .near) == nil)
    #expect(AddressCheck.problem("Bybit.near", network: .near) != nil)          // NEAR names are lower case
    #expect(AddressCheck.problem("0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", network: .near) != nil)
    #expect(AddressCheck.problem(String(repeating: "ab", count: 31), network: .near) != nil)
}

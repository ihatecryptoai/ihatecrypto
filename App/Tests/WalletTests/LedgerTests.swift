import EVMChain
import Foundation
import HyperliquidSigning
import Ledger
import Testing
import WalletCore

@Test func hidPacketsCarryTheLengthAndReassemble() throws {
    let apdu = Data((0..<200).map { UInt8($0 & 0xff) })
    let packets = LedgerFraming.packets(apdu)
    // 2 length bytes + 200 = 202 bytes in 59-byte slices.
    #expect(packets.count == 4)
    #expect(packets.allSatisfy { $0.count == 64 })
    #expect([UInt8](packets[0].prefix(7)) == [0x01, 0x01, 0x05, 0x00, 0x00, 0x00, 200])
    #expect([UInt8](packets[3].prefix(5)) == [0x01, 0x01, 0x05, 0x00, 0x03])
    // The device answers in the same framing; the reply is complete only with its last packet.
    #expect(try LedgerFraming.reply(Array(packets.prefix(3))) == nil)
    #expect(try LedgerFraming.reply(packets) == apdu)
    #expect(throws: LedgerError.self) { try LedgerFraming.reply([packets[1]]) }
}

@Test func blePacketsFitTheLinkAndReassemble() throws {
    let apdu = Data((0..<200).map { UInt8($0 & 0xff) })
    // MTU 20: 5 header bytes and 15 of data first, then 3 and 17 each.
    let packets = LedgerFraming.packets(apdu, size: 20, hid: false)
    #expect([UInt8](packets[0].prefix(5)) == [0x05, 0x00, 0x00, 0x00, 200])
    #expect([UInt8](packets[1].prefix(3)) == [0x05, 0x00, 0x01])
    #expect(packets.allSatisfy { $0.count <= 20 })
    #expect(packets.count == 1 + Int(((200 - 15) + 16) / 17))
    #expect(try LedgerFraming.reply(Array(packets.dropLast()), hid: false) == nil)
    #expect(try LedgerFraming.reply(packets, hid: false) == apdu)
    // The same APDU over USB is framed differently: HID packets are not BLE packets.
    #expect(throws: LedgerError.self) { try LedgerFraming.reply(LedgerFraming.packets(apdu), hid: false) }
}

@Test func typedDataPartsGiveTheSameDigest() throws {
    let key = Data(hex: "0123456789012345678901234567890123456789012345678901234567890123")!
    let action = HLAction.cancel([(asset: 0, oid: 1)])
    let parts = try HLSigner.l1Parts(action, nonce: 1_700_000_000_000, mainnet: true)
    let signature = try HLSigner.signL1(action, privateKey: key, nonce: 1_700_000_000_000, mainnet: true)
    // What a Ledger signs from the two hashes recovers to the same key the app signs with.
    #expect(try EVMSigner.recover(digest: parts.digest, signature: signature) == (try EVMAccount.address(privateKey: key)))
}

@Test func aSignatureFromElsewhereMakesTheSameTransaction() throws {
    let key = Data(hex: "0123456789012345678901234567890123456789012345678901234567890123")!
    let tx = EVMTransaction(chainId: 8453, nonce: 3, maxPriorityFeePerGas: 1_000_000, maxFeePerGas: 20_000_000, gasLimit: 60_000,
                            to: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", value: 0, data: try ERC20.approve(spender: "0x0000000000000000000000000000000000000001", amount: 5))
    let signature = try EVMSigner.sign(digest: try tx.signingHash(), privateKey: key)
    #expect(try tx.signed(with: signature).raw == (try tx.signed(privateKey: key).raw))
    #expect(try EVMSigner.recover(digest: try tx.signingHash(), signature: signature) == (try EVMAccount.address(privateKey: key)))
    // The other parity recovers someone else: the Ledger signer tries both and keeps the one that is ours.
    let flipped = EVMSignature(r: signature.r, s: signature.s, v: signature.v == 27 ? 28 : 27)
    #expect(try EVMSigner.recover(digest: try tx.signingHash(), signature: flipped) != (try EVMAccount.address(privateKey: key)))
}

@Test func aSignatureThatRecoversNoKeyIsRefusedNotACrash() throws {
    let digest = Data(repeating: 0x11, count: 32)
    let n = Data(hex: "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")!
    let gx = Data(hex: "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")!
    let one = Data(repeating: 0, count: 31) + [1]
    let bad: [(r: Data, s: Data)] = [
        (Data(repeating: 0, count: 32), one),          // r = 0
        (one, n),                                        // s = n
        (one, Data(repeating: 0xff, count: 32)),        // s > n
        (Data(repeating: 0, count: 31) + [5], one),     // r = 5: no point on the curve has that x
        (gx, digest),                                    // R = G and s = e: the key comes out at infinity
    ]
    for (r, s) in bad {
        #expect(throws: WalletError("The signature didn't verify.")) { try EVMSigner.recover(digest: digest, signature: EVMSignature(r: r, s: s, v: 27)) }
    }
}

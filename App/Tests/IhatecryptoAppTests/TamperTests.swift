@testable import IhatecryptoApp
import Foundation
import SignerAPI
import SolanaChain
import Testing
import TxGuard
import WalletCore

/// The attack drill changes what it says it changes: a token transfer's payee becomes one the guard refuses, its amount
/// doubles to the same payee, and an EVM transfer's payee or amount is rewritten in the calldata.
@Test func tamperRewritesASolanaTransfer() throws {
    let me = "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk", them = Base58.encode(Data(repeating: 7, count: 32))
    let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
    let wire = SolanaTransaction.unsigned(try SPL.transferMessage(payer: me, to: them, mint: usdc, amount: 500_000, decimals: 6, blockhash: me))
    let m = try SolanaMessage.parse(transaction: wire)
    let owners = [try SPL.associatedAccount(owner: me, mint: usdc): me, try SPL.associatedAccount(owner: them, mint: usdc): them]
    let policy = SolanaPolicy(owner: me, recipients: [them])
    #expect(SolanaGuard.check(m, accounts: m.staticKeys, policy: policy, tokenOwners: owners).ok)

    let (payeeWire, payeeAccounts, _) = try #require(Tamper.solana(wire, m.staticKeys, .payee))
    let payee = try SolanaMessage.parse(transaction: payeeWire)
    #expect(payee.staticKeys == payeeAccounts && payee.staticKeys != m.staticKeys)
    #expect(!SolanaGuard.check(payee, accounts: payeeAccounts, policy: policy, tokenOwners: owners).ok)

    let (amountWire, _, did) = try #require(Tamper.solana(wire, m.staticKeys, .amount))
    #expect(did == "amount 500000 → 1000000 units")
    #expect(try SolanaMessage.parse(transaction: amountWire).staticKeys == m.staticKeys)
}

@Test func tamperRewritesAnEVMTransfer() throws {
    let to = "0x" + String(repeating: "ab", count: 20)
    let data = Data(hex: "a9059cbb" + String(repeating: "0", count: 24) + String(repeating: "cd", count: 20) + String(repeating: "0", count: 62) + "64")!
    let (_, _, payee, _) = try #require(Tamper.evm(to: to, value: 0, data: data, .payee))
    #expect(payee.prefix(16) == data.prefix(16) && payee[16..<36] != data[16..<36] && payee[36...] == data[36...])
    let (_, _, amount, _) = try #require(Tamper.evm(to: to, value: 0, data: data, .amount))
    #expect(amount.prefix(36) == data.prefix(36) && amount.last == 0xc8)
    #expect(try #require(Tamper.evm(to: to, value: 5, data: Data(), .amount)).1 == 10)
}

/// A changed request the signer signs is dropped by its own flag, even when the drill was reset while it waited; a
/// Relay order the signer only holds goes on, and a signature that isn't a Relay deposit is never judged for it.
@Test func tamperNeverPassesASignedChange() throws {
    Tamper.arm(nil)
    #expect(throws: (any Error).self) { try Tamper.judged(SignerReply(ok: true), request: .status, changed: true) }
    #expect(try Tamper.judged(SignerReply(ok: true), request: .relayOrder(order: Data()), changed: true).ok)
    #expect(try Tamper.judged(SignerReply(ok: true), request: .status, changed: false).ok)
}

@testable import IhatecryptoApp
import Foundation
import IhatecryptoCore
import SignerAPI
import Testing

/// A key wallet is asked for the key its command signs with: a send on its recipient's network, a contract call on EVM.
@Test func aKeyWalletIsAskedForTheKeyItsSendSignsWith() throws {
    let evm = SignerWallet(id: "e", label: "Trading", kind: "evm", solana: nil, evm: "0xF", near: nil)
    let sol = SignerWallet(id: "s", label: "Phantom", kind: "solana", solana: "Sol", evm: nil, near: nil)
    let phrase = SignerWallet(id: "1", label: "Main", kind: "phrase", solana: "Sol", evm: "0xE", near: "n")
    let noSolana = "has no Solana key", noEVM = "has no EVM (Hyperliquid, Base…) key"
    func missing(_ text: String, _ wallet: SignerWallet, savedOn: String? = nil) throws -> String? {
        Workspace.missingKey(try #require(WalletCommand.parse(text), "\(text)"), wallet: wallet, savedOn: savedOn)
    }
    let call = "call arbitrum 0x1A996cb54bb95462040408C06122D45D6Cdb6096 deposit(uint256,address) 10e6 me"
    let all = ["send 5 USDC on base to Mom", "send 0.001 ETH on base to Mom", "send all RAY on solana to Bybit", "send 5 USDC to Mom",
               "отправь 5 usdc на Маму в сети полигон", "переведи 0,5 TSLA в робинхуде на Маму", "отправь все RAY на солане на New wallet", call]

    // The network the message names.
    #expect(try missing("send 5 USDC on base to Mom", evm) == nil)
    #expect(try missing("send 5 USDC on base to Mom", sol)?.contains(noEVM) == true)
    #expect(try missing("send 0.001 ETH on base to Mom", evm) == nil)
    #expect(try missing("send all RAY on solana to Bybit", evm)?.contains(noSolana) == true)
    #expect(try missing("отправь 5 usdc на Маму в сети полигон", evm) == nil)
    #expect(try missing("переведи 0,5 TSLA в робинхуде на Маму", evm) == nil)
    #expect(try missing("отправь все RAY на солане на New wallet", evm)?.contains(noSolana) == true)
    // None named: where the entry is saved; no entry at all — the send itself says there's no such name.
    #expect(try missing("send 5 USDC to Mom", evm, savedOn: "arbitrum") == nil)
    #expect(try missing("send 5 USDC to Mom", evm, savedOn: "hyperliquid") == nil)
    #expect(try missing("send 5 USDC to Mom", evm, savedOn: "solana")?.contains(noSolana) == true)
    #expect(try missing("send 5 USDC to Nobody", evm) == nil)
    // NEAR Intents' networks are paid from Solana's USDC, as before.
    #expect(try missing("send 0.001 BTC to Bybit", evm, savedOn: "bitcoin")?.contains(noSolana) == true)
    // A network named that the entry can't take: such an entry still goes over NEAR Intents (acrossRecipient comes first).
    #expect(try missing("send 0.001 BTC on base to Bybit", sol, savedOn: "bitcoin") == nil)
    #expect(try missing("send 0.001 BTC on base to Bybit", evm, savedOn: "bitcoin")?.contains(noSolana) == true)
    // A contract call is on an EVM network.
    #expect(try missing(call, evm) == nil)
    #expect(try missing(call, sol)?.contains(noEVM) == true)
    // A phrase wallet has every key.
    for text in all { #expect(try missing(text, phrase, savedOn: "solana") == nil) }
}

import Foundation
@testable import IhatecryptoCore
import Testing

@Test func privateSendReadsInBothLanguages() {
    #expect(WalletCommand.parse("send 20 USDC privately to Mom") == .sendPrivately(name: "Mom", token: "USDC", amount: 20, network: nil))
    #expect(WalletCommand.parse("send 5 USDC privately on base to Mom") == .sendPrivately(name: "Mom", token: "USDC", amount: 5, network: "base"))
    #expect(WalletCommand.parse("send 0.001 BTC privately to Binance") == .sendPrivately(name: "Binance", token: "BTC", amount: Decimal(string: "0.001")!, network: nil))
    #expect(WalletCommand.parse("send 20 USDC to Mom privately") == .sendPrivately(name: "Mom", token: "USDC", amount: 20, network: nil))
    #expect(WalletCommand.parse("переведи приватно 20 USDC на Маму") == .sendPrivately(name: "Маму", token: "USDC", amount: 20, network: nil))
    #expect(WalletCommand.parse("отправь анонимно 0,001 BTC на Binance") == .sendPrivately(name: "Binance", token: "BTC", amount: Decimal(string: "0.001")!, network: nil))
    #expect(WalletCommand.parse("отправь 30 USDT в троне конфиденциально на Bybit") == .sendPrivately(name: "Bybit", token: "USDT", amount: 30, network: "tron"))
}

@Test func privateSendWritesBackToItself() {
    for c: WalletCommand in [.sendPrivately(name: "Mom", token: "USDC", amount: 20, network: "base"),
                             .sendPrivately(name: "Binance", token: "BTC", amount: Decimal(string: "0.0005")!, network: nil)] {
        #expect(WalletCommand.parse(c.summary) == c)
    }
}

@Test func aNameWithAPrivacyStemStaysAName() {
    // «Приватный кошелёк», «Тайный» are names, not requests: the send stays the public one the user wrote.
    #expect(WalletCommand.parse("send 5 USDC to Приватный кошелёк") == .sendTo(name: "Приватный кошелёк", amount: 5, network: nil))
    #expect(WalletCommand.parse("переведи 5 USDC на Тайный") == .sendTo(name: "Тайный", amount: 5, network: nil))
    #expect(WalletCommand.asksPrivacy("скинь втихую 10 баксов маме"))
    #expect(!WalletCommand.asksPrivacy("send 5 USDC to Приватный кошелёк"))
}

@Test func aPrivateRequestNeverReadsAsAPublicSend() {
    // No amount, or no send at all: nothing, so the chat asks — never the same move in the open.
    #expect(WalletCommand.parse("send all USDC privately to Mom") == nil)
    #expect(WalletCommand.parse("переведи всё приватно на Маму") == nil)
    #expect(WalletCommand.parse("buy SOL privately for 5 USDC") == nil)
    #expect(WalletCommand.parse("send 200 USDC from the multisig to Alex privately") == nil)
}

/// Asked for privacy, a public send can't hide in a chain, a schedule or an alert: their steps are read later without
/// the user's words.
@Test func aPublicSendInAChainIsFoundWhenPrivacyWasAsked() {
    #expect(WalletCommand.hasPublicSend("send 20 USDC to Mom then buy SOL for 5 USDC"))
    #expect(WalletCommand.hasPublicSend("every friday: send 20 USDC to Mom"))
    #expect(WalletCommand.hasPublicSend("when SOL is above 300: send 20 USDC to Mom"))
    #expect(!WalletCommand.hasPublicSend("send 20 USDC privately to Mom then buy SOL for 5 USDC"))
    #expect(!WalletCommand.hasPublicSend("send 20 USDC to Mom"))   // alone: made private where it's run
    #expect(!WalletCommand.hasPublicSend("buy SOL for 5 USDC then deposit 5 USDC to kamino"))
}

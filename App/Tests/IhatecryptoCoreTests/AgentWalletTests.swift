import Foundation
@testable import IhatecryptoCore
import Testing

@Test func agentWalletCommandsReadInBothLanguages() {
    #expect(WalletCommand.parse("create an agent wallet with $20 a day") == .agentCreate(dailyUSD: 20))
    #expect(WalletCommand.parse("создай кошелёк агента с лимитом 20$ в день") == .agentCreate(dailyUSD: 20))
    #expect(WalletCommand.parse("fund the agent wallet with 5 USDC") == .agentFund(usdc: 5))
    #expect(WalletCommand.parse("пополни кошелёк агента на 5 USDC") == .agentFund(usdc: 5))
    #expect(WalletCommand.parse("send 1 USDC from the agent wallet to Automation") == .agentSend(usdc: 1, name: "Automation"))
    #expect(WalletCommand.parse("переведи 1,5 USDC с кошелька агента на Маму") == .agentSend(usdc: Decimal(string: "1.5")!, name: "Маму"))
    #expect(WalletCommand.parse("freeze the agent wallet") == .agentFreeze(true))
    #expect(WalletCommand.parse("разморозь кошелёк агента") == .agentFreeze(false))
}

@Test func agentWalletCommandsWriteBackToThemselves() {
    for c: WalletCommand in [.agentCreate(dailyUSD: 20), .agentFund(usdc: Decimal(string: "2.5")!), .agentSend(usdc: 1, name: "Mom"), .agentFreeze(true), .agentFreeze(false)] {
        #expect(WalletCommand.parse(c.summary) == c)
    }
}

@Test func agentWalletNeedsItsNumbers() {
    // No daily cap, no amount: nothing, so the chat asks.
    #expect(WalletCommand.parse("create an agent wallet") == nil)
    #expect(WalletCommand.parse("fund the agent wallet") == nil)
}

/// A payee's name is a name: «Binance deposit» isn't a deposit into the agent wallet, «Frozen Yogurt» isn't a freeze,
/// and money taken out of the agent wallet is never read as money put in.
@Test func aSendOutOfTheAgentWalletIsNeverAnotherCommand() {
    #expect(WalletCommand.parse("send 1 USDC from the agent wallet to Binance deposit") == .agentSend(usdc: 1, name: "Binance deposit"))
    #expect(WalletCommand.parse("send 1 USDC from the agent wallet to Frozen Yogurt") == .agentSend(usdc: 1, name: "Frozen Yogurt"))
    #expect(WalletCommand.parse("положи 1 USDC с кошелька агента маме") == nil)
    #expect(WalletCommand.parse("fund the agent wallet with 5 USDC") == .agentFund(usdc: 5))
    #expect(WalletCommand.parse("пополни кошелёк агента на 5 USDC") == .agentFund(usdc: 5))
}

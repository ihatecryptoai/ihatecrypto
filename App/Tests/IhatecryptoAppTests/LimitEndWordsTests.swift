import Foundation
import Testing
@testable import TradeExecution
@testable import IhatecryptoApp

// The fill said as what came and its price with the fees, beside the order's own price, in English and Russian.
@Test func limitEndWordsSayWhatCame() {
    let o = Executor.LimitOrderView(id: "o1", sell: false, coin: "SOL", coins: Decimal(string: "0.00833811")!, price: Decimal(string: "83.7")!,
                                    filled: 0, status: "OPEN", until: nil)
    let filled = Workspace.limitEndWords(o, Executor.LimitOrderEnd(status: "FILLED", spent: Decimal(string: "0.6979")!, got: Decimal(string: "0.008249201")!,
                                                                 price: Decimal(string: "84.6023")!))
    #expect(filled.en == "Limit order filled: bought ≈ 0.008249 SOL for 0.6979 USDC, $84.60 a SOL with 1Click's fees and the network's (the order's price $83.70). The SOL comes to your own address.")
    #expect(filled.ru.hasPrefix("Лимитка исполнилась: куплено ≈ 0.008249 SOL за 0.6979 USDC, $84.60 за SOL"))
    let expired = Workspace.limitEndWords(o, Executor.LimitOrderEnd(status: "EXPIRED", spent: 0, got: 0, price: nil))
    #expect(expired.en == "Limit order expired: buy 0.00833811 SOL at $83.70. What didn't fill comes back to the address it came from.")
    #expect(!filled.en.contains(";") && !filled.ru.contains("…"))
}

// «мои лимитки» names each order, not only how many (the design review: «Открытых лимитных ордеров: 1.» said nothing).
@Test func limitListWordsNameEachOrder() {
    let sell = Executor.LimitOrderView(id: "o1", sell: true, coin: "SOL", coins: Decimal(string: "0.012")!, price: Decimal(string: "84.3")!,
                                       filled: 0, status: "OPEN", until: nil)
    #expect(Workspace.limitListWords([sell])?.ru == "Один открытый лимитный ордер: продажа 0.012 SOL по $84.30. Он на холсте.")
    #expect(Workspace.limitListWords([sell, sell])?.en == "2 open limit orders: sell 0.012 SOL at $84.30 · sell 0.012 SOL at $84.30. They're on the canvas.")
    #expect(Workspace.limitListWords([]) == nil)
}

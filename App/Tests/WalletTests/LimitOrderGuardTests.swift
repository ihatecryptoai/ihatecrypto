import Foundation
import OneClickKit
import Testing
import TxGuard

// The test wallet's addresses from the public phrase «abandon … about» would do as well: any two strings stand for «ours».
private let me = "2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj"
private let deposit = "5P2eQoLncuFMjAmNNF4PspnAXYNaDSE2t1gb5os76Svw"
private let sol = "nep141:sol.omft.near"
private let usdc = "nep141:sol-5ce3bf3a31af18be40ba30f721101b4341690186.omft.near"
private let now = ISO8601DateFormatter().date(from: "2026-11-11T12:00:00Z")!
private let ends = now.addingTimeInterval(7 * 86_400)

/// 1Click's answer for a sale of 0.1 SOL at 200 USDC, as GET /v0/orders/{id} gives it (the shape of 2026-11-11).
private func answer(_ change: (inout [String: Any]) -> Void = { _ in }) -> [String: Any] {
    var a: [String: Any] = [
        "orderType": "LIMIT", "depositAddress": deposit, "depositMode": "SIMPLE", "depositType": "ORIGIN_CHAIN", "fillStatus": "AWAITING_DEPOSIT",
        "payoutStatus": "NOT_STARTED", "isPayoutStatusFinal": false, "depositedAmount": "0", "baseAsset": sol, "quoteAsset": usdc,
        "quantity": "100000000", "side": "SELL", "price": "200", "recipient": me, "recipientType": "DESTINATION_CHAIN", "refundTo": me,
        "refundType": "ORIGIN_CHAIN", "confidentiality": "basic", "timeInForce": "GTC", "deadline": "2026-11-18T12:00:00.000Z",
        "createdAt": "2026-11-11T12:00:00.000Z", "appFees": [["recipient": "ihatecrypto.near", "fee": 5]],
        "swapView": ["swapType": "EXACT_INPUT", "originAsset": sol, "destinationAsset": usdc, "amountIn": "100000000", "minAmountOut": "20000000"],
        "partialFills": [] as [Any],
    ]
    change(&a)
    return a
}

private func order(_ change: (inout [String: Any]) -> Void = { _ in }) -> OneClick.Order {
    OneClick.parseOrder(["type": "orders", "id": "ord_1", "attributes": answer(change)])!
}

private func check(_ o: OneClick.Order, deposit sent: String = "100000000", most: Decimal = 100_000_000, sell: Bool = true) -> Verdict {
    OrderGuard.check(bound: o.fields, sell: sell, base: sol, quote: usdc, quantity: "100000000", price: 200, depositAddress: deposit,
                     recipient: me, refundTo: me, deposit: sent, most: most, fee: OneClick.orderFee, deadline: ends, now: now)
}

@Test func anOrderHeldAsAskedPasses() {
    let v = check(order())
    #expect(v.ok, "\(v.problems)")
    // 1Click lists its own 20 basis points beside ours.
    let both = check(order { $0["appFees"] = [["recipient": "ihatecrypto.near", "fee": 5], ["recipient": "oneclick-fees.near", "fee": 20]] })
    #expect(both.ok, "\(both.problems)")
}

@Test func theGuardRefusesAnOrderNotAsAsked() {
    // Someone else's payout, someone else's refund, a public order, a fee that isn't ours, a higher one, a memo.
    #expect(!check(order { $0["recipient"] = "ScamXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1" }).ok)
    #expect(!check(order { $0["refundTo"] = "ScamXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX1" }).ok)
    #expect(!check(order { $0["confidentiality"] = "public" }).ok)
    #expect(!check(order { $0["appFees"] = [["recipient": "someone.near", "fee": 25]] }).ok)
    #expect(!check(order { $0["appFees"] = [["recipient": "ihatecrypto.near", "fee": 5], ["recipient": "a.near", "fee": 15], ["recipient": "b.near", "fee": 15]] }).ok)
    #expect(!check(order { $0["appFees"] = [["recipient": "ihatecrypto.near", "fee": 50]] }).ok)
    #expect(!check(order { $0["depositMemo"] = "123" }).ok)
    // The wrong side, price, pair or deposit address.
    #expect(!check(order { $0["side"] = "BUY" }).ok)
    #expect(!check(order { $0["price"] = "150" }).ok)
    #expect(!check(order { $0["baseAsset"] = "nep141:wrap.near" }).ok)
    #expect(!check(order { $0["depositAddress"] = "Other1111111111111111111111111111111111111" }).ok)
    // Resting far longer than asked, or about to run out.
    #expect(!check(order { $0["deadline"] = "2027-01-01T00:00:00.000Z" }).ok)
    #expect(!check(order { $0["deadline"] = "2026-11-11T12:01:00.000Z" }).ok)
    // A deposit other than what the swap view takes, or more than approved.
    #expect(!check(order(), deposit: "100000001").ok)
    #expect(!check(order(), most: 99_999_999).ok)
}

@Test func aBuyDepositsItsSwapViewsMaximumWithinWhatWasAsked() {
    // A buy of 0.1 SOL at 200 for at most 20 USDC: the swap view's maxAmountIn is what goes in.
    let buy = order {
        $0["side"] = "BUY"
        $0["swapView"] = ["swapType": "EXACT_OUTPUT", "originAsset": usdc, "destinationAsset": sol, "maxAmountIn": "19950000", "amountOut": "100000000"]
    }
    #expect(check(buy, deposit: "19950000", most: 20_000_000, sell: false).ok)
    #expect(!check(buy, deposit: "19950000", most: 19_000_000, sell: false).ok)
    #expect(!check(buy, deposit: "20000000", most: 20_000_000, sell: false).ok)
}

@Test func aPriceThroughTheMarketFillsAtOnce() {
    #expect(OrderGuard.fillsAtOnce(sell: true, price: 140, market: 142))
    #expect(!OrderGuard.fillsAtOnce(sell: true, price: 200, market: 142))
    #expect(OrderGuard.fillsAtOnce(sell: false, price: 150, market: 142))
    #expect(!OrderGuard.fillsAtOnce(sell: false, price: 120, market: 142))
}

@Test func anOrdersFillsAddUp() {
    let o = order {
        $0["fillStatus"] = "PARTIALLY_FILLED"
        $0["partialFills"] = [["id": "f1", "amountIn": "30000000", "amountOut": "6000000", "createdAt": "", "updatedAt": ""],
                              ["id": "f2", "amountIn": "20000000", "amountOut": "4000000", "createdAt": "", "updatedAt": ""]]
    }
    #expect(o["filledIn"] == "50000000")
    #expect(o["filledOut"] == "10000000")
    #expect(o.isOpen)
    #expect(!order { $0["fillStatus"] = "FILLED" }.isOpen)
}

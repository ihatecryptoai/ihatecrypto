import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import TxGuard
import WalletCore

/// Limit orders through NEAR Intents' 1Click (/v0/orders): a swap that waits for its price. Always private — 1Click takes
/// only confidential orders — funded by one deposit from this wallet, paid out to its own address, refunded to its own,
/// with ihatecrypto's 0.05% (OneClick.orderFee). Once funded it fills, in parts too, with no signature: so before the
/// deposit the order is read back from 1Click and checked whole (OrderGuard), and a price already through the market is
/// refused as the swap it would be.
extension Executor {
    /// A limit order worked out: both sides as 1Click lists them, its quantity of the coin, the deposit it takes, and the
    /// market now (dollars for one coin, from 1Click's own quote; nil when it gives none).
    public struct LimitPlan: Sendable {
        let coin: IntentSide
        let usdc: IntentSide
        /// The coin in its smallest units: a sale's coins, a buy's target.
        let quantity: String
        /// What the wallet sends: a sale's coins, a buy's USDC (at most what was asked).
        let deposit: Decimal
        public let market: Decimal?
        /// The coins a buy gets or a sale gives, whole.
        public let coins: Decimal
        /// The USDC a sale brings or a buy spends at the limit price, before fees.
        public let dollars: Decimal
    }

    /// The headroom a buy keeps under the USDC asked: 1Click's swap view rounds up and takes its fees from the input.
    public static let limitBuyHeadroom = Decimal(string: "0.997")!

    func limitPlan(sell: Bool, coin: String, on: String, price: Decimal, amount: Decimal, usdcOn: String, near: String?) async throws -> LimitPlan {
        guard price > 0, amount > 0 else { throw ExecutionError("A limit order needs a price and an amount. Nothing was signed.") }
        async let c = intentSide(coin, on: on)
        async let u = intentSide("USDC", on: usdcOn)
        let (base, quote) = try await (c, u)
        // From the NEAR account ihatecrypto sends NEAR itself: said before 1Click makes an order nobody could fund.
        if (sell ? on : usdcOn) == "near", !(sell && base.symbol == "NEAR") {
            throw ExecutionError("From NEAR, ihatecrypto sends NEAR itself: \(sell ? base.symbol : "USDC") there can't fund an order. Nothing was signed.")
        }
        guard base.asset != quote.asset else { throw ExecutionError("That's USDC for USDC: nothing to wait for.") }
        let unit = pow(Decimal(10), base.decimals)
        let quantity: Decimal, deposit: Decimal
        if sell {
            quantity = (amount * unit).whole
            deposit = quantity / unit
        } else {
            quantity = (amount * Self.limitBuyHeadroom / price * unit).whole
            deposit = amount
        }
        guard quantity > 0 else { throw ExecutionError("\(Fmt.num(amount)) is less than one smallest unit of \(base.symbol). Nothing was signed.") }
        let coins = quantity / unit
        // The market as 1Click quotes this very size now: what the coins sell for, or what the USDC buys.
        let probe = sell ? "\(quantity)" : "\((deposit * pow(Decimal(10), quote.decimals)).whole)"
        let q = try? await OneClick.quote(dry: true, origin: sell ? base.asset : quote.asset, destination: sell ? quote.asset : base.asset, amount: probe,
                                         refundTo: try intentAddress(sell ? on : usdcOn, near: near ?? String(repeating: "0", count: 64)),
                                         recipient: try intentAddress(sell ? usdcOn : on, near: near ?? String(repeating: "0", count: 64)))
        let market: Decimal? = q.flatMap { q in
            guard let out = Decimal(string: q.amountOutFormatted), out > 0 else { return nil }
            return sell ? out / coins : deposit / out
        }
        return LimitPlan(coin: base, usdc: quote, quantity: "\(quantity)", deposit: deposit, market: market, coins: coins, dollars: coins * price)
    }

    /// For the card: the market now, and the order in a line.
    public func limitOrderText(sell: Bool, coin: String, on: String, price: Decimal, amount: Decimal, usdcOn: String, near: String?) async throws -> LimitPlan {
        try await limitPlan(sell: sell, coin: coin, on: on, price: price, amount: amount, usdcOn: usdcOn, near: near)
    }

    /// Places the order: the market checked again, the order made, read back and guarded, the deposit sent, and 1Click
    /// followed until it holds the money. It then rests up to `days` days (nil = 7).
    public func limitOrder(sell: Bool, coin: String, on: String, price: Decimal, amount: Decimal, usdcOn: String, days: Int?, near: String?) async throws -> String {
        let plan = try await limitPlan(sell: sell, coin: coin, on: on, price: price, amount: amount, usdcOn: usdcOn, near: near)
        let name = plan.coin.symbol
        guard let market = plan.market else {
            throw ExecutionError("1Click doesn't price \(name) for USDC now, so an order on it may never fill. Nothing was signed.")
        }
        if OrderGuard.fillsAtOnce(sell: sell, price: price, market: market) {
            throw GuardStop("\(name) is ≈ \(Fmt.usd(market)) now: \(sell ? "a sale at" : "a buy at") \(Fmt.usd(price)) would fill at once, as a swap at the market price. Say it as a swap, or pick a price \(sell ? "above" : "below") the market. Nothing was signed.")
        }
        let origin = sell ? on : usdcOn, destination = sell ? usdcOn : on
        let refund = try intentAddress(origin, near: near), recipient = try intentAddress(destination, near: near)
        let ends = Date().addingTimeInterval(TimeInterval((days ?? 7) * 86_400))
        let order = try await OneClick.createOrder(sell: sell, base: plan.coin.asset, quote: plan.usdc.asset, quantity: plan.quantity,
                                                   price: NSDecimalNumber(decimal: price).stringValue, refundTo: refund, recipient: recipient, deadline: ends)
        guard let address = order["depositAddress"], !address.isEmpty else { throw ExecutionError("1Click gave the order no deposit address. Nothing was signed.") }
        do {
            return try await fund(order, plan: plan, sell: sell, coin: coin, price: price, market: market, address: address, origin: origin,
                                  destination: destination, recipient: recipient, refund: refund, ends: ends, near: near)
        } catch {
            // Nothing sent to it: the order goes, so it neither shows as open nor stays behind a Try again.
            let funded = (try? journal.entries())?.contains { $0.chain == "1click-order" && $0.id == order.id } ?? false
            if !funded { try? await OneClick.cancelOrder(order.id) }
            throw error
        }
    }

    /// The order read back, guarded and funded, then followed until it rests. Nothing after the deposit throws.
    private func fund(_ order: OneClick.Order, plan: LimitPlan, sell: Bool, coin: String, price: Decimal, market: Decimal, address: String,
                      origin: String, destination: String, recipient: String, refund: String, ends: Date, near: String?) async throws -> String {
        let name = plan.coin.symbol
        // Read back from 1Click by its id: the answer that made it is not what's checked.
        let bound = try await OneClick.order(order.id)
        let paid = sell ? plan.coin : plan.usdc
        let unit = pow(Decimal(10), paid.decimals)
        let deposit = bound[sell ? "amountIn" : "maxAmountIn"] ?? ""
        let verdict = OrderGuard.check(bound: bound.fields, sell: sell, base: plan.coin.asset, quote: plan.usdc.asset, quantity: plan.quantity, price: price,
                                       depositAddress: address, recipient: recipient, refundTo: refund, deposit: deposit,
                                       most: (plan.deposit * unit).whole, fee: OneClick.orderFee, deadline: ends)
        guard verdict.ok else {
            throw GuardStop("The guard stopped the order's deposit: " + verdict.problems.joined(separator: " ") + " Nothing was sent.")
        }
        let sends = (Decimal(string: deposit) ?? 0) / unit
        let what = sell ? "sell \(Fmt.num(plan.coins, maxFraction: 8)) \(name) at \(Fmt.usd(price)) or more"
            : "buy \(Fmt.num(plan.coins, maxFraction: 8)) \(name) at \(Fmt.usd(price)) or less"
        report("Limit order (private): \(what) · \(name) ≈ \(Fmt.usd(market)) now · until \(Fmt.date(ends)).")
        try await intentDeposit(sell ? coin : "USDC", symbol: paid.symbol, amount: sends, units: deposit, from: origin, to: address, near: near,
                                journal: ("1click-order", order.id, "limit \(sell ? "sell" : "buy") \(Fmt.num(plan.coins, maxFraction: 8)) \(name) at \(price) · \(Fmt.num(sends, maxFraction: 8)) \(paid.symbol) from \(origin)"))
        // Until 1Click sees the deposit: then the order rests.
        var status = "AWAITING_DEPOSIT"
        let started = Date()
        // Nothing after the deposit may throw: it went, and a second try would fund a second order.
        while Date().timeIntervalSince(started) < 180, status == "AWAITING_DEPOSIT", !Task.isCancelled {
            try? await Task.sleep(for: .seconds(5))
            if let o = try? await OneClick.order(order.id), let s = o["fillStatus"] {
                if s != status { report("1Click · \(Self.orderWords[s] ?? s.lowercased()) · \(Int(Date().timeIntervalSince(started))) s") }
                status = s
            }
        }
        if status != "AWAITING_DEPOSIT" { try? journal.append(.init(chain: "1click-order", id: order.id, kind: .confirmed, note: status)) }
        let place = WalletCommand.placeName(destination)
        let rests = "It rests with 1Click until \(Fmt.date(ends)) and fills, in parts too, when the market gets there: the \(sell ? "USDC" : name) comes to your own address on \(place) with no signature then. «show limit orders» shows it, «cancel limit orders» brings back what hasn't filled."
        switch status {
        case "FILLED", "PARTIALLY_FILLED": return "Limit order placed and already \(status == "FILLED" ? "filled" : "partly filled"): \(what). \(rests)"
        case "AWAITING_DEPOSIT": return "Limit order made and its deposit sent (\(Fmt.num(sends, maxFraction: 8)) \(paid.symbol)): \(what). 1Click hasn't seen the deposit yet: it starts resting once it does. \(rests)"
        default: return "Limit order placed, privately: \(what). \(rests)"
        }
    }

    /// The guard's own test, for the card: a price already through the market fills at once.
    public static func limitFillsAtOnce(sell: Bool, price: Decimal, market: Decimal) -> Bool { OrderGuard.fillsAtOnce(sell: sell, price: price, market: market) }

    /// 1Click's order states, in words.
    static let orderWords = ["AWAITING_DEPOSIT": "waiting for the deposit", "OPEN": "resting", "PARTIALLY_FILLED": "partly filled", "FILLED": "filled",
                             "PENDING_CANCEL": "canceling", "CANCELED": "canceled", "EXPIRED": "expired"]

    /// An open order in words for the board: the coin, side, its size and price, how much filled, until when.
    public struct LimitOrderView: Sendable, Equatable {
        public let id: String
        public let sell: Bool
        public let coin: String
        public let coins: Decimal
        public let price: Decimal
        public let filled: Decimal
        public let status: String
        public let until: Date?
        public var state: String { Executor.orderWords[status] ?? status.lowercased() }
    }

    /// What the journal learns about an order from 1Click now: confirmed once 1Click saw its deposit (it may see it after
    /// the move's three minutes), settled once its payout is final (filled, canceled or expired, the money out). nil:
    /// nothing new for an entry last written as `kind`.
    static func limitJournalStep(after kind: Journal.Entry.Kind, fillStatus: String, payoutFinal: Bool) -> Journal.Entry.Kind? {
        if payoutFinal { return .settled }
        return kind == .signed && fillStatus != "AWAITING_DEPOSIT" ? .confirmed : nil
    }

    /// Brings the journal's orders up to date with 1Click, the newest 20 not yet settled: read with the board, so a
    /// deposit seen late or a cancel's refund is written down when the orders are next read.
    public func syncLimitJournal() async {
        guard let all = try? journal.entries() else { return }
        var latest: [String: Journal.Entry] = [:], order: [String] = []
        for e in all where e.chain == "1click-order" {
            if latest[e.id] == nil { order.append(e.id) }
            latest[e.id] = e
        }
        for id in order.reversed().filter({ latest[$0]?.kind != .settled }).prefix(20) {
            guard let last = latest[id], let o = try? await OneClick.order(id), let fill = o["fillStatus"],
                  let step = Self.limitJournalStep(after: last.kind, fillStatus: fill, payoutFinal: o["isPayoutStatusFinal"] == "true") else { continue }
            try? journal.append(.init(chain: "1click-order", id: id, kind: step, note: step == .settled ? "\(fill) · payout \(o["payoutStatus"] ?? "?")" : fill))
        }
    }

    /// This wallet's open orders: those of the partner key that pay out to or refund to one of its own addresses.
    public func limitOrders(near: String?, sync: Bool = true) async throws -> [(order: OneClick.Order, view: LimitOrderView)] {
        if sync { await syncLimitJournal() }
        let own = Set([wallet.solana, wallet.evm, near ?? ""].filter { !$0.isEmpty }.map { $0.lowercased() })
        let tokens = (try? await OneClick.tokens()) ?? []
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return try await OneClick.openOrders().filter { o in
            o.isOpen && (own.contains(o["recipient"]?.lowercased() ?? "") || own.contains(o["refundTo"]?.lowercased() ?? ""))
        }.map { o in
            let t = tokens.first { $0.assetId == o["baseAsset"] }
            let unit = pow(Decimal(10), t?.decimals ?? 0)
            let sell = o["side"] == "SELL"
            // A sale's fills spend the coin (their input), a buy's bring it (their output).
            let filled = (Decimal(string: o[sell ? "filledIn" : "filledOut"] ?? "") ?? 0) / unit
            let until = o["deadline"].flatMap { f.date(from: $0) ?? ISO8601DateFormatter().date(from: $0) }
            let coin = t.map { $0.symbol == "wNEAR" ? "NEAR" : $0.symbol } ?? (o["baseAsset"] ?? "?")
            return (o, LimitOrderView(id: o.id, sell: sell, coin: coin, coins: (Decimal(string: o["quantity"] ?? "") ?? 0) / unit,
                                      price: Decimal(string: o["price"] ?? "") ?? 0, filled: filled, status: o["fillStatus"] ?? "", until: until))
        }
    }

    /// One order's 1Click status now (FILLED, EXPIRED, CANCELED…), nil when 1Click doesn't answer.
    public func limitOrderStatus(_ id: String) async -> String? { (try? await OneClick.order(id))?["fillStatus"] }

    /// Cancels this wallet's open orders, those on `coin` only when it's named. What hasn't filled comes back to the
    /// address it came from; a last slice can still fill.
    public func cancelLimitOrders(_ coin: String?, near: String?) async throws -> String {
        let id = coin.flatMap { $0.hasPrefix("id:") ? String($0.dropFirst(3)) : nil }
        let open = try await limitOrders(near: near).filter { o in
            o.view.status != "PENDING_CANCEL" && (id.map { o.view.id == $0 } ?? (coin == nil || o.view.coin.uppercased() == coin?.uppercased()))
        }
        guard !open.isEmpty else { return id != nil ? "That limit order isn't open any more." : coin.map { "No open limit order on \($0)." } ?? "No open limit orders." }
        var done: [String] = [], failed: [String] = []
        for o in open {
            let v = o.view, line = "\(v.sell ? "sell" : "buy") \(Fmt.num(v.coins, maxFraction: 8)) \(v.coin) at \(Fmt.usd(v.price))"
            do {
                try await OneClick.cancelOrder(v.id)
                done.append(line)
            } catch {
                failed.append("\(line): \(error.localizedDescription)")
            }
        }
        let n = done.count
        var out = n == 0 ? "" : "Asked 1Click to cancel \(n == 1 ? "the order" : "\(n) orders"): \(done.joined(separator: ", ")). What hasn't filled comes back to the address it came from in a few minutes. A last slice can still fill."
        if !failed.isEmpty { out += (out.isEmpty ? "" : " ") + "Not canceled: \(failed.joined(separator: ". "))." }
        if n == 0 { throw ExecutionError(out) }
        return out
    }
}

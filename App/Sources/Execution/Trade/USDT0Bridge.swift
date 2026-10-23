import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import RelayBridge
import WalletCore

/// A bridge that can make a move, as its card lists it: what it lands, what it costs and how long it takes («≈ 99.97
/// USDC · $0.03 · ≈ 4 s»), and what lands in dollars with every fee off.
public typealias BridgeWay = (way: String, gets: String, worth: Decimal)

/// USDT between networks over Tether's own bridge, USDT0 on LayerZero (EVMChain.USDT0), beside Relay's swap: one call on
/// the source — after an exact approve on Ethereum, where the adapter locks the USDT — and the same amount is minted on
/// the other side at this wallet; LayerZero's message fee rides with the call in the source's coin. Free but slow (20–35
/// minutes from Ethereum, Arbitrum and Optimism, half a day from HyperEVM), so it goes only when asked for, with the wait
/// and Relay's price beside it on the card.
extension Executor {
    /// What the bridge takes now for `units` (millionths) of USDT: what lands, LayerZero's fee in the source's coin — wei
    /// to send, a twentieth over the quote so a rise before the block still delivers (the rest comes back) — and that fee
    /// in dollars. nil off its networks, outside its limits, or when a node doesn't answer.
    public static func usdt0Quote(from: EVMNetwork, to: EVMNetwork, units: UInt64, recipient: String) async -> (arrives: UInt64, fee: UInt64, feeUSD: Decimal)? {
        guard let a = USDT0.on(from.rpc.chainId), let b = USDT0.on(to.rpc.chainId), a.eid != b.eid, units > 0,
              let call = try? USDT0.quoteOFT(to: b.eid, recipient: recipient, units: units), let limits = try? await from.rpc.read(a.oft, call),
              let least = ABI.decimal(in: limits, word: 0), let most = ABI.decimal(in: limits, word: 1), least <= Decimal(units), Decimal(units) <= most,
              let arrives = ABI.uint(in: limits, word: 4),
              let ask = try? USDT0.quoteSend(to: b.eid, recipient: recipient, units: units, min: arrives), let fees = try? await from.rpc.read(a.oft, ask),
              let fee = ABI.uint(in: fees, word: 0), let price = try? await Relay.nativePrice(chainId: from.rpc.chainId) else { return nil }
        return (arrives, fee + fee / 20, Decimal(fee) / 1e18 * price)
    }

    /// LayerZero's word on a message by the transaction that sent it — "INFLIGHT", "CONFIRMING", "DELIVERED", "FAILED" —
    /// and the transaction that delivered it. nil when its API doesn't answer or hasn't seen the transaction yet.
    public static func layerZeroStatus(tx: String) async -> (status: String, delivered: String?)? {
        guard let url = URL(string: "https://scan.layerzero-api.com/v1/messages/tx/\(tx)"), let (data, _) = try? await URLSession.shared.data(from: url),
              let message = (((try? JSONSerialization.jsonObject(with: data)) as? [String: Any])?["data"] as? [[String: Any]])?.first,
              let status = (message["status"] as? [String: Any])?["name"] as? String else { return nil }
        return (status, ((message["destination"] as? [String: Any])?["tx"] as? [String: Any])?["txHash"] as? String)
    }

    /// USDT held on a USDT0 network, the amount a move takes of it (all of it when none is said), in millionths.
    private func usdt0Amount(_ a: USDT0.Deployment, on net: EVMNetwork, _ asked: Decimal?) async throws -> (units: UInt64, held: UInt64) {
        let held = try await net.rpc.tokenBalance(a.token, owner: wallet.evm)
        return (asked.map { NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).uint64Value } ?? held, held)
    }

    /// The bridges that can move this USDT now, for the card — Relay's swap of the same token and Tether's USDT0 — each
    /// with what it lands, what it costs and how long it takes; the one the move goes by (`layerZero`) first.
    public func usdtWays(from: EVMNetwork, to: EVMNetwork, usdt: Decimal?, layerZero: Bool) async throws -> [BridgeWay] {
        guard let a = USDT0.on(from.rpc.chainId), let b = USDT0.on(to.rpc.chainId) else { return [] }
        let units = try await usdt0Amount(a, on: from, usdt).units, sent = Decimal(units) / 1_000_000
        guard units > 0 else { return [] }
        async let own = Self.usdt0Quote(from: from, to: to, units: units, recipient: wallet.evm)
        async let relay = try? await Relay.quote(user: wallet.evm, recipient: wallet.evm, originChain: from.relayChain, destinationChain: to.relayChain,
                                           originCurrency: a.token, destinationCurrency: b.token, amount: units)
        var ways: [BridgeWay] = []
        if let q = await own {
            let lands = Decimal(q.arrives) / 1_000_000
            ways.append(("layerzero", "\(Fmt.num(lands, maxFraction: 6)) USDT · \(Self.cost(sent - lands + q.feeUSD)) · \(USDT0.wait(a.minutes))", lands - q.feeUSD))
        }
        if let r = await relay, let out = Decimal(string: r.amountOutFormatted) {
            ways.append(("relay", "≈ \(Fmt.num(out, maxFraction: 4)) USDT · \(Self.cost(sent - out)) · ≈ \(max(r.seconds, 1)) s", out))
        }
        return layerZero ? ways : ways.reversed()
    }

    /// A bridge's cost for the card: under a cent it's said as it is («$0.0007»), not as «$0.00».
    static func cost(_ usd: Decimal) -> String {
        let usd = max(usd, 0)
        return usd > 0 && usd < Decimal(string: "0.01")! ? "$" + Fmt.num(usd, maxFraction: 4) : Fmt.usd(usd)
    }

    /// «move 50 USDT from arbitrum to optimism»: Relay's swap of the same USDT — or the USDT0 bridge, when asked by name.
    public func usdtMove(from: EVMNetwork, to: EVMNetwork, usdt: Decimal?, layerZero: Bool) async throws -> String {
        if layerZero { return try await usdt0Bridge(from: from, to: to, usdt: usdt) }
        guard USDT0.on(from.rpc.chainId) != nil, USDT0.on(to.rpc.chainId) != nil else {
            throw ExecutionError("ihatecrypto moves USDT like this between Ethereum, Arbitrum, Optimism, Polygon and HyperEVM. Nothing was signed.")
        }
        // By its ticker, which Stablecoins knows as the real one: by its contract the token lookup weighed USD₮0's own
        // pools on Arbitrum ($44k) and called it too thin for Relay (2026-10-13). On Optimism «USDT» is the old bridged one.
        func ticker(_ net: EVMNetwork) -> String { net.key == "optimism" ? "USDT0" : "USDT" }
        return try await crossSwap(ticker(from), on: from.key, for: ticker(to), on: to.key, amount: usdt)
    }

    /// «move 50 USDT from arbitrum to optimism via LayerZero»: the USDT0 bridge, whatever Relay would land.
    public func usdt0Bridge(from: EVMNetwork, to: EVMNetwork, usdt: Decimal?) async throws -> String {
        guard let a = USDT0.on(from.rpc.chainId), let b = USDT0.on(to.rpc.chainId), a.eid != b.eid else {
            throw ExecutionError("Tether's USDT0 bridge moves USDT between Ethereum, Arbitrum, Optimism, Polygon and HyperEVM. Nothing was signed.")
        }
        let me = wallet.evm, rpc = from.rpc
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }
        let (units, held) = try await usdt0Amount(a, on: from, usdt)
        guard units > 0, units <= held else { throw ExecutionError("\(show(held)) USDT on \(from.name). Nothing was signed.") }
        guard let q = await Self.usdt0Quote(from: from, to: to, units: units, recipient: me) else {
            throw ExecutionError("The USDT0 bridge didn't quote \(show(units)) USDT from \(from.name) to \(to.name) now. Nothing was signed.")
        }
        // Tether takes nothing today: a fee it turns on is said, and one past half a percent stops the move.
        guard q.arrives <= units, units - q.arrives <= units / 200 else {
            throw ExecutionError("The USDT0 bridge would land \(show(q.arrives)) of \(show(units)) USDT. Nothing was signed.")
        }
        report("USDT0 on LayerZero: \(show(units)) USDT from \(from.name) → \(show(q.arrives)) on \(to.name), its message fee \(Self.cost(q.feeUSD)) in \(from.gasCoin); "
               + "it lands in \(USDT0.wait(a.minutes)).")
        // The fee is coins sent with the call, which a send never tops up by itself: gas comes over first when it's short.
        if try await rpc.balance(me) < q.fee + from.gasFloor {
            report("Not enough \(from.gasCoin) on \(from.name) for LayerZero's fee and the gas: bringing some first.")
            _ = try await topUpGas(from, usd: min(max(from.topupUSD, q.feeUSD * 3, Decimal(string: "0.1")!), 20))
        }
        if a.approves {
            // Ethereum's USDT changes an allowance only from zero.
            func allowance() async throws -> Decimal { ABI.decimal(in: try await rpc.read(a.token, try ERC20.allowance(owner: me, spender: a.oft)), word: 0) ?? 0 }
            let left = try await allowance()
            if left > 0, left < Decimal(units) {
                _ = try await sendEVM(rpc, to: a.token, data: try ERC20.approve(spender: a.oft, amount: 0), note: "Clear the USDT0 bridge's old allowance")
                var waited = 0
                while try await allowance() > 0, waited < 15 {
                    try await Task.sleep(for: .seconds(1))
                    waited += 1
                }
            }
            try await approve(rpc, token: a.token, spender: a.oft, amount: units, note: "Approve exactly \(show(units)) USDT to the USDT0 bridge")
        }
        let before = try await to.rpc.tokenBalance(b.token, owner: me)
        let hash = try await sendEVM(rpc, to: a.oft, data: try USDT0.send(to: b.eid, recipient: me, units: units, min: q.arrives, fee: q.fee, refund: me),
                                     note: "USDT0: \(show(units)) USDT from \(from.name) to \(to.name)", value: q.fee)
        try journal.append(.init(chain: "layerzero", id: hash, kind: .signed, note: "\(from.name) USDT→\(to.name) \(show(units)) (USDT0), tx \(hash)"))
        // Minutes away (from Polygon) it's waited for; a longer way stays in transit on the portfolio until it lands.
        let started = Date()
        while a.minutes <= 3, Date().timeIntervalSince(started) < 300 {
            if try await to.rpc.tokenBalance(b.token, owner: me) > before {
                try journal.append(.init(chain: "layerzero", id: hash, kind: .settled, note: "landed"))
                return "Moved \(show(units)) USDT from \(from.name) to \(to.name) over USDT0 in \(Int(Date().timeIntervalSince(started))) s: \(show(q.arrives)) USDT arrived."
            }
            try await Task.sleep(for: .seconds(5))
        }
        return "Sent \(show(units)) USDT from \(from.name) to \(to.name) over USDT0: \(show(q.arrives)) USDT lands there in \(USDT0.wait(a.minutes)), "
            + "in transit on the portfolio until then. https://layerzeroscan.com/tx/\(hash)"
    }

    /// The bridges that can move this USDC between two EVM networks now, for the card, the one `bridge` would take first:
    /// Relay, unless Circle's CCTP or NEAR Intents' 1Click lands more by the margin worth their wait (oneClickEdge) — the
    /// same rule, asked again before signing. A destination short of gas has Relay only: it brings the gas along.
    public func bridgeWays(from net: EVMNetwork, to dest: EVMNetwork, usdc requested: Decimal?) async throws -> [BridgeWay] {
        let me = wallet.evm, have = try await net.usdcBalance(me), spendable = have > net.feeReserve ? have - net.feeReserve : 0
        let units = requested.map { min(NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).uint64Value, spendable) } ?? spendable
        guard units >= 500_000 else { return [] }
        let sent = Decimal(units) / 1_000_000, stable = dest.key == "robinhood" ? "USDG" : "USDC"
        var topup: Decimal?
        if dest.feeToken == nil, try await dest.rpc.balance(me) < dest.gasFloor { topup = dest.topupUSD }
        let quote = try await relayQuote(user: me, recipient: me, originChain: net.relayChain, destinationChain: dest.relayChain, originCurrency: net.usdc,
                                          destinationCurrency: dest.usdc, amount: units, topupGasUSD: topup, units: "\(net.raw(units))")
        let relayOut = Decimal(string: quote.amountOutFormatted) ?? 0
        let relay: BridgeWay = ("relay", "≈ \(Fmt.num(relayOut, maxFraction: 4)) \(stable) · \(Self.cost(sent - relayOut - (topup ?? 0))) · ≈ \(max(quote.seconds, 1)) s"
                                + (topup == nil ? "" : ", gas comes along"), relayOut + (topup ?? 0))
        guard topup == nil else { return [relay] }
        async let circle = cctpQuote(from: net, to: dest, units: units)
        async let intents = oneClickQuote(from: net.key, to: dest.key, units: units)
        var others: [BridgeWay] = []
        if let c = await circle {
            others.append(("cctp", "≈ \(Fmt.num(c.arrives, maxFraction: 4)) USDC · \(Self.cost(sent - c.arrives)) · ≈ 1 min", c.arrives))
        }
        if let q = await intents, let out = Decimal(string: q.amountOutFormatted) {
            others.append(("1click", "≈ \(Fmt.num(out, maxFraction: 4)) USDC · \(Self.cost(sent - out)) · \(q.timeEstimate.map { "≈ \($0) s" } ?? "minutes")", out))
        }
        // `bridge` asks CCTP first, then 1Click: the first that beats Relay by the margin goes.
        let taken = others.first { $0.worth - relayOut >= Self.oneClickEdge(sent) }
        return (taken.map { [$0] } ?? []) + [relay] + others.filter { $0.way != taken?.way }
    }
}

import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import SignerAPI
import WalletCore

/// USDC between EVM networks through Circle's CCTP v2 with its Forwarding Service, beside Relay and 1Click: one
/// signature on the source (an exact approve to the TokenMessenger and the burn, one 7702 transaction where it batches),
/// then Circle attests and mints to this wallet on the destination itself — no gas needed there, no address to allow.
/// Its fee is the fast transfer's basis points plus the forward's gas; measured 2026-10-06 it lands less than Relay from
/// Base at $20, $1000 and $10000 (1.3 bp against Relay's ≈ 1), so the bridge takes it only when it lands more.
extension Executor {
    /// What CCTP charges now for `units` of USDC between two networks — the cap the burn carries (`maxFee`, a fifth over
    /// the quoted fees, so a rise between quote and block still forwards) — and what lands. nil off CCTP's networks or
    /// when Circle doesn't answer.
    public func cctpQuote(from: EVMNetwork, to: EVMNetwork, units: UInt64) async -> (maxFee: UInt64, arrives: Decimal)? {
        guard let a = CCTP.domain(from.rpc.chainId), let b = CCTP.domain(to.rpc.chainId), a != b,
              let url = URL(string: "\(CCTP.iris)/v2/burn/USDC/fees/\(a)/\(b)?forward=true"),
              let (data, _) = try? await URLSession.shared.data(from: url),
              let tiers = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]],
              let tier = tiers.first(where: { ($0["finalityThreshold"] as? NSNumber)?.uint32Value == CCTP.fast }),
              let bps = (tier["minimumFee"] as? NSNumber)?.decimalValue,
              let forward = ((tier["forwardFee"] as? [String: Any])?["med"] as? NSNumber)?.decimalValue else { return nil }
        let fees = Decimal(units) * bps / 10_000 + forward
        let cap = NSDecimalNumber(decimal: (fees * Decimal(string: "1.2")!).rounded(0, .up)).uint64Value
        guard cap < units else { return nil }
        return (cap, (Decimal(units) - fees.rounded(0, .up)) / 1_000_000)
    }

    /// Circle's word on a burn: attested, and the mint it broadcast (its forward) — nil while there's none, or when the
    /// API doesn't answer.
    public static func cctpForward(source: UInt32, burn: String) async -> String? {
        guard let url = URL(string: "\(CCTP.iris)/v2/messages/\(source)?transactionHash=\(burn)"),
              let (data, _) = try? await URLSession.shared.data(from: url),
              let message = ((try? JSONSerialization.jsonObject(with: data)) as? [String: Any]).flatMap({ ($0["messages"] as? [[String: Any]])?.first }) else { return nil }
        return message["forwardTxHash"] as? String
    }

    /// The move: the burn on `from`, Circle's mint on `to` followed in its API, then the USDC on the balance there.
    /// Returns what arrived.
    public func cctpMove(from: EVMNetwork, to: EVMNetwork, units: UInt64, maxFee: UInt64) async throws -> Decimal {
        guard let source = CCTP.domain(from.rpc.chainId), let destination = CCTP.domain(to.rpc.chainId) else {
            throw ExecutionError("Circle's CCTP moves USDC between Ethereum, Arbitrum, Base and Polygon.")
        }
        let label = "\(Fmt.num(Decimal(units) / 1_000_000)) USDC"
        try await ensureEVM(from, usdc: units)
        let before = try await to.usdcBalance(wallet.evm)
        let burn = try await approveAndSend(from.rpc, token: from.usdc, spender: CCTP.tokenMessenger, units: Decimal(units),
                                            approveNote: "Approve exactly \(label) to Circle's CCTP", to: CCTP.tokenMessenger,
                                            data: try CCTP.burn(units: units, to: destination, recipient: wallet.evm, usdc: from.usdc, maxFee: maxFee),
                                            note: "CCTP: burn \(label) on \(from.name), minted on \(to.name)",
                                            cap: SignerCap(asset: from.usdc.lowercased(), units: units, payee: wallet.evm, card: "\(label) to \(to.name)"), capCall: true)
        try journal.append(.init(chain: "cctp", id: "\(source):\(burn)", kind: .signed, note: "\(from.name) USDC→\(to.name) \(Fmt.num(Decimal(units) / 1_000_000)) (CCTP), tx \(burn)"))
        let started = Date()
        var mint: String?
        while mint == nil, Date().timeIntervalSince(started) < 900 {
            mint = await Self.cctpForward(source: source, burn: burn)
            if mint == nil { try await Task.sleep(for: .seconds(3)) }
        }
        guard let mint else {
            throw ExecutionError("Circle hasn't minted on \(to.name) within 15 minutes; the burn is \(from.explorer)/tx/\(burn), in transit on the portfolio.")
        }
        report("CCTP · Circle minted on \(to.name) in \(Int(Date().timeIntervalSince(started))) s. \(to.explorer)/tx/\(mint)")
        var after = try await to.usdcBalance(wallet.evm)
        for _ in 0..<30 where after <= before {
            try await Task.sleep(for: .seconds(2))
            after = try await to.usdcBalance(wallet.evm)
        }
        try journal.append(.init(chain: "cctp", id: "\(source):\(burn)", kind: .settled, note: mint))
        return Decimal(after > before ? after - before : 0) / 1_000_000
    }

    /// «move 500 USDC from base to arbitrum via CCTP»: CCTP whatever Relay would land.
    public func cctpBridge(from: EVMNetwork, to: EVMNetwork, usdc: Decimal?) async throws -> String {
        let held = try await from.usdcBalance(wallet.evm)
        let units = usdc.map { NSDecimalNumber(decimal: ($0 * 1_000_000).rounded(0, .down)).uint64Value } ?? held
        guard units > 0, units <= held else { throw ExecutionError("\(Fmt.num(Decimal(held) / 1_000_000)) USDC on \(from.name). Nothing was signed.") }
        guard let q = await cctpQuote(from: from, to: to, units: units) else { throw ExecutionError("Circle didn't quote CCTP from \(from.name) to \(to.name) now. Nothing was signed.") }
        report("Circle's CCTP: \(Fmt.num(Decimal(units) / 1_000_000)) USDC from \(from.name) → ≈ \(Fmt.num(q.arrives, maxFraction: 4)) on \(to.name), minted by Circle.")
        let started = Date()
        let got = try await cctpMove(from: from, to: to, units: units, maxFee: q.maxFee)
        return "Moved \(Fmt.num(Decimal(units) / 1_000_000)) USDC from \(from.name) to \(to.name) through Circle's CCTP in \(Int(Date().timeIntervalSince(started))) s: \(Fmt.num(got)) USDC arrived."
    }
}

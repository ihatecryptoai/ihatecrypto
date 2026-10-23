import Foundation
import IhatecryptoCore
import TradeExecution
import TxGuard

/// What a place's details board shows beyond the portfolio's rows (the Mac's PlaceDetailBoard): Hyperliquid's account
/// in full, and the rate each deposit earns now.
nonisolated struct PlaceFacts: Encodable, Equatable {
    struct Dex: Encodable, Equatable { var dex: String; var accountValue: Decimal; var withdrawable: Decimal }
    struct HL: Encodable, Equatable {
        var accountValue: Decimal
        var withdrawable: Decimal
        var spotUSDC: Decimal
        /// HIP-3 exchanges' own margin (stocks, commodities): `xyz`.
        var dexes: [Dex]
        var hlpEquity: Decimal
        /// HLP keeps a deposit 4 days.
        var hlpLockedUntil: Date?
    }
    var hyperliquid: HL?
    /// USDC's rate a year now, in %, by place: kamino, jupiter, aave (Base), morpho (Base), hlp.
    var rates: [String: Decimal] = [:]
}

extension Engine {
    /// The account, its HIP-3 exchanges and its HLP share, read together; the network's card counts all of it
    /// (the Mac's hlTotal), margin in «Perps» and HLP as a deposit.
    nonisolated static func hyperliquid(_ user: String) async -> (Read, PlaceFacts.HL?) {
        async let spotRead = Holdings.hyperliquidSpot(user: user)
        async let vaultRead = try? HyperliquidAccount.vaultEquity(user: user, vault: HLGuard.hlpVault)
        async let dexRead = { () async -> [PlaceFacts.Dex] in
            var out: [PlaceFacts.Dex] = []
            for dex in Hyperliquid.hip3Dexes {
                if let d = try? await Hyperliquid.dexBalance(user: user, dex: dex) { out.append(.init(dex: dex, accountValue: d.accountValue, withdrawable: d.withdrawable)) }
            }
            return out
        }()
        do {
            let b = try await Hyperliquid.balance(user: user)
            let dexes = await dexRead, vault = await vaultRead
            let margin = b.accountValue + dexes.reduce(0) { $0 + $1.accountValue }
            var rows: [PortfolioView.Row] = []
            if margin > 0 { rows.append(row("Perps", margin, usd: margin)) }
            if b.spotUSDC > 0 { rows.append(row("USDC", b.spotUSDC, usd: b.spotUSDC)) }
            rows += ((await spotRead) ?? []).filter { $0.symbol != "USDC" }.map(row)
            if let v = vault, v.equity > 0 {
                rows.append(deposit("HLP", usd: v.equity, note: v.lockedUntil.map { $0 > Date() ? "locked until \(Fmt.date($0))" : "earning" } ?? "earning"))
            }
            let facts = PlaceFacts.HL(accountValue: b.accountValue, withdrawable: b.withdrawable, spotUSDC: b.spotUSDC, dexes: dexes,
                                      hlpEquity: vault?.equity ?? 0, hlpLockedUntil: vault?.lockedUntil.flatMap { $0 > Date() ? $0 : nil })
            return (.success(.init(key: "hyperliquid", name: "Hyperliquid", usd: 0, rows: rows)), facts)
        } catch {
            return (.failure("hyperliquid", "Hyperliquid: \(error.localizedDescription)"), nil)
        }
    }

    /// The deposits' rates, read at most every 10 minutes (each is several protocols' APIs).
    func refreshRates() async {
        if let at = ratesAt, Date().timeIntervalSince(at) < 600 { return }
        ratesAt = Date()
        let options = await loadYields("USDC").filter { $0.asset == "USDC" }
        func rate(_ match: (String) -> Bool) -> Decimal? { options.first { match($0.place) }?.apy.rounded(2, .plain) }
        var rates: [String: Decimal] = [:]
        rates["kamino"] = rate { $0.contains("Kamino") }
        rates["jupiter"] = rate { $0.contains("Jupiter") }
        rates["aave"] = rate { $0 == "Aave · Base" }
        rates["morpho"] = rate { $0 == "Morpho · Base" }
        rates["hlp"] = rate { $0.hasPrefix("HLP") }
        placeFacts.rates = rates
    }
}

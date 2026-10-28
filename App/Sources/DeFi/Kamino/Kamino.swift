import Foundation

/// Kamino Lend: rates, deposit, withdraw, borrow and repay transactions built by Kamino's own API, positions. The main
/// market lends; the Backpack market takes Backpack's tokenized stocks as collateral and lends USDC against them.
/// The first deposit on a market also creates the user's Kamino accounts there: rent in SOL, shown by simulation.
public enum Kamino {
    public static let mainMarket = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF"
    /// Kamino's Backpack market (opened 2026-10-28, risk set by Allez Labs): its own pool, apart from the main market.
    public static let backpackMarket = "4iRHKGsTq3e4uut6e4PfyV9AEbNuXaMu9JaSP378p9qy"
    /// The stocks it takes as collateral, by ticker: Backpack Securities' Token-2022 tokens, each backed one to one by a
    /// US-listed share, tradable on Solana (Orca's pools, through Jupiter) by any wallet. Kamino's reserve stats on
    /// 2026-10-28: SPCX and SNDK borrow up to 30% of their value and liquidate at 50%, MU and SKHY 40% and 60%.
    public static let backpackStocks = ["SPCX": "SpaceX", "MU": "Micron", "SKHY": "SK Hynix", "SNDK": "SanDisk"]
    /// The market a token goes into: Backpack's stocks the Backpack market, everything else the main one.
    public static func market(for symbol: String) -> String { backpackStocks[symbol.uppercased()] == nil ? mainMarket : backpackMarket }
    public static let program = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD"
    static let base = "https://api.kamino.finance"

    public struct Reserve: Sendable {
        public let address: String
        public let symbol: String
        public let mint: String
        /// Supply APY, % a year.
        public let supplyAPY: Decimal
        public let tvlUSD: Decimal
        /// The token's dollar price: the reserve's supply in dollars over its supply in tokens (0 when empty).
        public var priceUSD: Decimal = 0
        /// What a loan of the token costs, % a year.
        public var borrowAPY: Decimal = 0
    }

    /// One reserve per token: the main market has several for USDC, the one holding the money is the real one.
    public static func reserves(market: String = mainMarket) async throws -> [Reserve] {
        let data = try await get("/kamino-market/\(market)/reserves/metrics")
        guard let rows = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { throw KaminoError(data) }
        func dec(_ v: Any?) -> Decimal { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber)?.decimalValue ?? 0 }
        var best: [String: Reserve] = [:]
        for r in rows {
            guard let address = r["reserve"] as? String, let symbol = r["liquidityToken"] as? String, let mint = r["liquidityTokenMint"] as? String else { continue }
            let supplied = dec(r["totalSupply"])
            let reserve = Reserve(address: address, symbol: symbol, mint: mint, supplyAPY: dec(r["supplyApy"]) * 100, tvlUSD: dec(r["totalSupplyUsd"]),
                                  priceUSD: supplied > 0 ? dec(r["totalSupplyUsd"]) / supplied : 0, borrowAPY: dec(r["borrowApy"]) * 100)
            if reserve.tvlUSD > best[mint]?.tvlUSD ?? -1 { best[mint] = reserve }
        }
        return best.values.sorted { $0.tvlUSD > $1.tvlUSD }
    }

    /// Staking yield of liquid-staking tokens by mint, % a year (Kamino tracks them for its collateral).
    public static func stakingYields() async throws -> [String: Decimal] {
        let data = try await get("/staking-yields")
        guard let rows = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { throw KaminoError(data) }
        var out: [String: Decimal] = [:]
        for r in rows {
            guard let mint = r["tokenMint"] as? String else { continue }
            let apy = (r["apy"] as? String).flatMap { Decimal(string: $0) } ?? (r["apy"] as? NSNumber)?.decimalValue ?? 0
            if apy > 0 { out[mint] = apy * 100 }
        }
        return out
    }

    /// Unsigned transaction depositing `amount` (in tokens, e.g. 1.5 USDC) into `reserve` for `wallet`; on the Backpack
    /// market it is collateral to borrow against.
    public static func depositTransaction(wallet: String, reserve: String, amount: Decimal, market: String = mainMarket) async throws -> Data {
        try await transaction("/ktx/klend/deposit", wallet: wallet, reserve: reserve, amount: amount, market: market)
    }

    public static func withdrawTransaction(wallet: String, reserve: String, amount: Decimal, market: String = mainMarket) async throws -> Data {
        try await transaction("/ktx/klend/withdraw", wallet: wallet, reserve: reserve, amount: amount, market: market)
    }

    /// Unsigned transaction borrowing `amount` of `reserve`'s token to `wallet` against its collateral on `market`, and
    /// paying it back. Kamino builds them only once the wallet's position exists (a first deposit makes it).
    public static func borrowTransaction(wallet: String, reserve: String, amount: Decimal, market: String) async throws -> Data {
        try await transaction("/ktx/klend/borrow", wallet: wallet, reserve: reserve, amount: amount, market: market)
    }

    public static func repayTransaction(wallet: String, reserve: String, amount: Decimal, market: String) async throws -> Data {
        try await transaction("/ktx/klend/repay", wallet: wallet, reserve: reserve, amount: amount, market: market)
    }

    /// A reserve's risk numbers, 0…1: how much of its value may be borrowed against it, where liquidation starts, and the
    /// factor a debt in it counts with.
    public struct Risk: Sendable, Equatable {
        public let maxLTV: Decimal
        public let liquidationLTV: Decimal
        public let borrowFactor: Decimal
    }

    public static func risk(reserve: String) async throws -> Risk {
        let data = try await get("/reserves/\(reserve)/stats")
        guard let j = try JSONSerialization.jsonObject(with: data) as? [String: Any], j["liqLTV"] != nil else { throw KaminoError(data) }
        return Risk(maxLTV: dec(j["maxLTV"]), liquidationLTV: dec(j["liqLTV"]), borrowFactor: max(dec(j["borrowFactor"]), 1))
    }

    /// The wallet's position on a market as Kamino refreshes it: what it put up, what it owes, and the dollar limits —
    /// what it may borrow in all, and the debt (borrow-factor adjusted) at which liquidation starts.
    public struct Loan: Sendable, Equatable {
        public struct Collateral: Sendable, Equatable {
            public let reserve: String
            public let symbol: String
            public let mint: String
            /// In the token, at Kamino's price (0 when it gave none).
            public let tokens: Decimal
            public let valueUSD: Decimal
        }
        public struct Debt: Sendable, Equatable {
            public let reserve: String
            public let symbol: String
            /// In the token, interest to the last refresh included.
            public let tokens: Decimal
        }
        public let collateral: [Collateral]
        public let debts: [Debt]
        public let depositUSD: Decimal
        public let debtUSD: Decimal
        public let borrowLimitUSD: Decimal
        public let liquidationLimitUSD: Decimal
        public let adjustedDebtUSD: Decimal
        /// Liquidation at 1; nil without debt.
        public var health: Decimal? { adjustedDebtUSD > 0 ? liquidationLimitUSD / adjustedDebtUSD : nil }

        public init(collateral: [Collateral], debts: [Debt], depositUSD: Decimal, debtUSD: Decimal, borrowLimitUSD: Decimal,
                    liquidationLimitUSD: Decimal, adjustedDebtUSD: Decimal) {
            self.collateral = collateral; self.debts = debts; self.depositUSD = depositUSD; self.debtUSD = debtUSD
            self.borrowLimitUSD = borrowLimitUSD; self.liquidationLimitUSD = liquidationLimitUSD; self.adjustedDebtUSD = adjustedDebtUSD
        }
    }

    /// The wallet's plain position on `market` (Kamino's «vanilla» obligation, the one its transactions use); nil before
    /// the first deposit there.
    public static func loan(wallet: String, market: String) async throws -> Loan? {
        let data = try await fresh("/kamino-market/\(market)/users/\(wallet)/obligations")
        guard let rows = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { throw KaminoError(data) }
        guard let o = rows.first(where: { ($0["obligationTag"] as? NSNumber)?.intValue == 0 }),
              let state = o["state"] as? [String: Any], let stats = o["refreshedStats"] as? [String: Any] else { return nil }
        let reserves = Dictionary(try await Self.reserves(market: market).map { ($0.address, $0) }, uniquingKeysWith: { a, _ in a })
        let none = "11111111111111111111111111111111"
        let deposits = (state["deposits"] as? [[String: Any]] ?? []).filter { ($0["depositReserve"] as? String).map { $0 != none } ?? false }
        // Each deposit's stored value is as fresh as the last refresh; the refreshed total, split in those proportions.
        let stored = deposits.map { (dec($0["marketValueSf"]) / pow(Decimal(2), 60)) }
        let storedSum = stored.reduce(0, +), total = dec(stats["userTotalDeposit"])
        let collateral = deposits.enumerated().map { i, d in
            let r = reserves[d["depositReserve"] as? String ?? ""]
            let value = storedSum > 0 ? total * stored[i] / storedSum : total / Decimal(deposits.count)
            // Backpack's stocks are never lent out (Kamino lets nobody borrow them), so a share of one stays one token
            // (6 decimals); anything else is its dollars at the reserve's price.
            let stock = r.map { backpackStocks[$0.symbol] != nil } ?? false
            return Loan.Collateral(reserve: d["depositReserve"] as? String ?? "", symbol: r?.symbol ?? "?", mint: r?.mint ?? "",
                                   tokens: stock ? dec(d["depositedAmount"]) / 1_000_000 : (r?.priceUSD ?? 0) > 0 ? value / r!.priceUSD : 0, valueUSD: value)
        }
        let debts = (state["borrows"] as? [[String: Any]] ?? []).compactMap { b -> Loan.Debt? in
            guard let reserve = b["borrowReserve"] as? String, reserve != none else { return nil }
            let r = reserves[reserve]
            // The borrowed amount in the token's smallest units, fixed point 2⁶⁰.
            // ponytail: 6 decimals but SOL's 9 — USDC is what ihatecrypto borrows; read the mint for any other debt.
            let decimals = r?.symbol == "SOL" ? 9 : 6
            return Loan.Debt(reserve: reserve, symbol: r?.symbol ?? "?", tokens: dec(b["borrowedAmountSf"]) / pow(Decimal(2), 60) / pow(Decimal(10), decimals))
        }
        return Loan(collateral: collateral, debts: debts, depositUSD: total, debtUSD: dec(stats["userTotalBorrow"]),
                    borrowLimitUSD: dec(stats["borrowLimit"]), liquidationLimitUSD: dec(stats["borrowLiquidationLimit"]),
                    adjustedDebtUSD: dec(stats["userTotalBorrowBorrowFactorAdjusted"]))
    }

    /// Unsigned transaction redeeming `shares` of a Kamino Vault for its token; shares staked in the vault's farm are
    /// unstaked in the same transaction (checked by simulation: 1 share of «Allez USDC» → 1.0665 USDC).
    public static func vaultWithdrawTransaction(wallet: String, vault: String, shares: Decimal) async throws -> Data {
        try await post("/ktx/kvault/withdraw", ["wallet": wallet, "kvault": vault, "amount": "\(shares)"])
    }

    public struct Position: Sendable {
        public let reserve: String
        public let symbol: String
        /// Kamino's collateral shares (raw), not the underlying tokens.
        public let amount: Decimal
        public let valueUSD: Decimal
        /// The reserve's supply % a year, where the market lists it.
        public var apy: Decimal? = nil
    }

    public static func positions(wallet: String) async throws -> [Position] {
        let data = try await fresh("/kamino-market/\(mainMarket)/users/\(wallet)/obligations")
        guard let obligations = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { throw KaminoError(data) }
        let names = Dictionary((try? await reserves())?.map { ($0.address, $0) } ?? [], uniquingKeysWith: { a, _ in a })
        var out: [Position] = []
        for o in obligations {
            let deposits = (((o["state"] as? [String: Any])?["deposits"] as? [[String: Any]]) ?? [])
                .filter { ($0["depositReserve"] as? String).map { $0 != "11111111111111111111111111111111" } ?? false }
            // The stored per-deposit value (fixed point, 2⁶⁰) is only as fresh as the obligation's last refresh — 0 right
            // after a first deposit. The API's refreshed total is current, so it is split in the stored proportions.
            let stored = deposits.map { d in (d["marketValueSf"] as? String).flatMap { Decimal(string: $0) }.map { $0 / pow(Decimal(2), 60) } ?? 0 }
            let storedSum = stored.reduce(0, +)
            let total = ((o["refreshedStats"] as? [String: Any])?["userTotalDeposit"] as? String).flatMap { Decimal(string: $0) } ?? storedSum
            for (i, d) in deposits.enumerated() {
                let reserve = d["depositReserve"] as? String ?? ""
                // ponytail: all stored values 0 with several deposits splits evenly; exact would read each reserve's exchange rate.
                let value = storedSum > 0 ? total * stored[i] / storedSum : total / Decimal(deposits.count)
                let shares = (d["depositedAmount"] as? String).flatMap { Decimal(string: $0) } ?? 0
                out.append(Position(reserve: reserve, symbol: names[reserve]?.symbol ?? "?", amount: shares, valueUSD: value, apy: names[reserve]?.supplyAPY))
            }
        }
        return out
    }

    /// A deposit in a Kamino Vault (Earn): the vault's shares, held or staked in its farm, as the vault's token and dollars.
    public struct VaultPosition: Sendable, Equatable {
        public let vault: String
        /// The vault's own name, «Allez USDC».
        public let name: String
        public let symbol: String
        public let mint: String
        /// The vault's shares, held and staked: what a withdrawal asks for.
        public let shares: Decimal
        public let tokens: Decimal
        public let valueUSD: Decimal
        /// % a year.
        public let apy: Decimal
    }

    /// The wallet's Kamino Vault deposits. A deposit made on kamino.com lands here, not in the Lend market's obligations
    /// that `positions` reads (5 USDC into «Allez USDC» showed nowhere).
    public static func vaultPositions(wallet: String) async throws -> [VaultPosition] {
        let data = try await get("/kvaults/users/\(wallet)/positions")
        guard let rows = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { throw KaminoError(data) }
        let held = rows.compactMap { r -> (vault: String, shares: Decimal)? in
            guard let vault = r["vaultAddress"] as? String, dec(r["totalShares"]) > 0 else { return nil }
            return (vault, dec(r["totalShares"]))
        }
        guard !held.isEmpty else { return [] }
        let vaults = try await VaultNames.shared.all()
        return try await withThrowingTaskGroup(of: VaultPosition.self) { group in
            for h in held {
                group.addTask {
                    let m = try JSONSerialization.jsonObject(with: try await get("/kvaults/\(h.vault)/metrics")) as? [String: Any] ?? [:]
                    let tokens = h.shares * dec(m["tokensPerShare"]), v = vaults[h.vault]
                    return VaultPosition(vault: h.vault, name: v?.name ?? "Kamino Vault", symbol: v.map { symbols[$0.mint] ?? "\($0.mint.prefix(4))…" } ?? "?",
                                         mint: v?.mint ?? "", shares: h.shares, tokens: tokens, valueUSD: tokens * dec(m["tokenPrice"]), apy: dec(m["apy"]) * 100)
                }
            }
            var out: [VaultPosition] = []
            for try await p in group { out.append(p) }
            return out.sorted { $0.valueUSD > $1.valueUSD }
        }
    }

    static let symbols = ["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": "USDC", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB": "USDT",
                          "So11111111111111111111111111111111111111112": "SOL"]

    static func dec(_ v: Any?) -> Decimal { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber)?.decimalValue ?? 0 }

    /// Every vault's name and token, read once per launch (≈ 400 KB) and only when the wallet has a vault deposit.
    actor VaultNames {
        static let shared = VaultNames()
        private var names: [String: (name: String, mint: String)]?
        func all() async throws -> [String: (name: String, mint: String)] {
            if let names { return names }
            let rows = try JSONSerialization.jsonObject(with: try await Kamino.get("/kvaults/vaults")) as? [[String: Any]] ?? []
            var out: [String: (name: String, mint: String)] = [:]
            for r in rows {
                guard let address = r["address"] as? String, let state = r["state"] as? [String: Any] else { continue }
                out[address] = ((state["name"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "Kamino Vault", state["tokenMint"] as? String ?? "")
            }
            names = out
            return out
        }
    }

    static func transaction(_ path: String, wallet: String, reserve: String, amount: Decimal, market: String) async throws -> Data {
        try await post(path, ["wallet": wallet, "market": market, "reserve": reserve, "amount": "\(amount)"])
    }

    static func post(_ path: String, _ body: [String: String]) async throws -> Data {
        var request = URLRequest(url: URL(string: base + path)!, timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, _) = try await URLSession.shared.data(for: request)
        guard let j = try JSONSerialization.jsonObject(with: data) as? [String: Any], let b64 = j["transaction"] as? String,
              let wire = Data(base64Encoded: b64) else { throw KaminoError(data) }
        return wire
    }

    /// A wallet's own state, past the caches: Kamino's API sends it with max-age=60, and Cloudflare served the position
    /// as it was before a deposit or a loan for a minute after it landed. A query of its own makes it a miss there.
    static func fresh(_ path: String) async throws -> Data {
        let url = URL(string: base + path + "?env=mainnet-beta&fresh=\(Int(Date().timeIntervalSince1970 * 1000))")!
        return try await URLSession.shared.data(for: URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 20)).0
    }

    static func get(_ path: String) async throws -> Data {
        try await URLSession.shared.data(for: URLRequest(url: URL(string: base + path)!, timeoutInterval: 20)).0
    }
}

public struct KaminoError: LocalizedError {
    public let message: String
    init(_ data: Data) { message = String(decoding: data.prefix(240), as: UTF8.self) }
    public var errorDescription: String? { "Kamino: \(message)" }
}

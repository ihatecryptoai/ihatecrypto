import Foundation

/// Kamino Lend, the main market: rates, deposit and withdraw transactions built by Kamino's own API, positions.
/// The first deposit also creates the user's Kamino accounts and a lookup table: rent in SOL, shown by simulation.
public enum Kamino {
    public static let mainMarket = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF"
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
    }

    /// One reserve per token: the market has several for USDC, the one holding the money is the real one.
    public static func reserves() async throws -> [Reserve] {
        let data = try await get("/kamino-market/\(mainMarket)/reserves/metrics")
        guard let rows = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { throw KaminoError(data) }
        func dec(_ v: Any?) -> Decimal { (v as? String).flatMap { Decimal(string: $0) } ?? (v as? NSNumber)?.decimalValue ?? 0 }
        var best: [String: Reserve] = [:]
        for r in rows {
            guard let address = r["reserve"] as? String, let symbol = r["liquidityToken"] as? String, let mint = r["liquidityTokenMint"] as? String else { continue }
            let supplied = dec(r["totalSupply"])
            let reserve = Reserve(address: address, symbol: symbol, mint: mint, supplyAPY: dec(r["supplyApy"]) * 100, tvlUSD: dec(r["totalSupplyUsd"]),
                                  priceUSD: supplied > 0 ? dec(r["totalSupplyUsd"]) / supplied : 0)
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

    /// Unsigned transaction depositing `amount` (in tokens, e.g. 1.5 USDC) into `reserve` for `wallet`.
    public static func depositTransaction(wallet: String, reserve: String, amount: Decimal) async throws -> Data {
        try await transaction("/ktx/klend/deposit", wallet: wallet, reserve: reserve, amount: amount)
    }

    public static func withdrawTransaction(wallet: String, reserve: String, amount: Decimal) async throws -> Data {
        try await transaction("/ktx/klend/withdraw", wallet: wallet, reserve: reserve, amount: amount)
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
        let data = try await get("/kamino-market/\(mainMarket)/users/\(wallet)/obligations")
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

    static func transaction(_ path: String, wallet: String, reserve: String, amount: Decimal) async throws -> Data {
        try await post(path, ["wallet": wallet, "market": mainMarket, "reserve": reserve, "amount": "\(amount)"])
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

    static func get(_ path: String) async throws -> Data {
        try await URLSession.shared.data(for: URLRequest(url: URL(string: base + path)!, timeoutInterval: 20)).0
    }
}

public struct KaminoError: LocalizedError {
    public let message: String
    init(_ data: Data) { message = String(decoding: data.prefix(240), as: UTF8.self) }
    public var errorDescription: String? { "Kamino: \(message)" }
}

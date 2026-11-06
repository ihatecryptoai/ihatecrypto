import DeFiJupiterLend
import DeFiKamino
import Foundation

/// Where an asset earns right now, across the protocols ihatecrypto knows, with what ihatecrypto can do there and the catch.
public enum Yields {
    public struct Option: Sendable {
        public let asset: String
        public let place: String
        /// % a year.
        public let apy: Decimal
        public let tvlUSD: Decimal?
        /// The chat phrase that puts money there, when ihatecrypto can.
        public let how: String?
        public let note: String

        public init(asset: String, place: String, apy: Decimal, tvlUSD: Decimal?, how: String?, note: String) {
            self.asset = asset; self.place = place; self.apy = apy; self.tvlUSD = tvlUSD; self.how = how; self.note = note
        }
    }

    static let jitoSOL = "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn"
    static let hlp = "0xdfc24b077bc1425ad1dea75bcb6f8158e10df303"

    /// All options for the stablecoins and SOL; `asset` narrows to one («USDC», «SOL»). Sorted by yield.
    /// A source that does not answer is skipped rather than failing the whole list.
    public static func options(for asset: String? = nil) async -> [Option] {
        async let jup = try? JupiterLend.vaults()
        async let kam = try? Kamino.reserves()
        async let staking = try? Kamino.stakingYields()
        async let hlpAPR = try? hyperliquidVaultAPR(hlp)
        var out: [Option] = []
        for v in await jup ?? [] {
            let symbol = v.assetSymbol == "WSOL" ? "SOL" : v.assetSymbol
            out.append(Option(asset: symbol, place: "Jupiter Lend", apy: v.apy, tvlUSD: nil, how: "deposit 10 \(symbol) to the vault",
                              note: "lending, withdraw any time; the first deposit opens a share account for ≈ 0.002 SOL, refunded on full withdrawal"))
        }
        for r in (await kam ?? []) where r.supplyAPY > 0 {
            out.append(Option(asset: r.symbol, place: "Kamino Lend", apy: r.supplyAPY, tvlUSD: r.tvlUSD, how: "deposit 50 \(r.symbol) to Kamino",
                              note: "lending; the first deposit opens Kamino accounts for ≈ 0.03 SOL rent, worth it from ≈ $35"))
        }
        if let apy = (await staking)?[jitoSOL] {
            out.append(Option(asset: "SOL", place: "JitoSOL staking", apy: apy, tvlUSD: nil, how: "stake 0.1 SOL",
                              note: "liquid staking: SOL becomes JitoSOL, which earns by rising in price against SOL; swap back in seconds"))
        }
        if let apr = await hlpAPR {
            out.append(Option(asset: "USDC", place: "HLP (Hyperliquid)", apy: apr, tvlUSD: nil, how: "deposit 5 USDC to HLP",
                              note: "the exchange's market making: some weeks lose money; from $5; withdraw 4 days after a deposit"))
        }
        let wanted = asset?.uppercased()
        return out.filter { wanted == nil || $0.asset.uppercased() == wanted }.sorted { $0.apy > $1.apy }
    }

    /// A Hyperliquid vault's APR, % a year.
    public static func hyperliquidVaultAPR(_ vault: String) async throws -> Decimal {
        var request = URLRequest(url: URL(string: "https://api.hyperliquid.xyz/info")!, timeoutInterval: 20)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["type": "vaultDetails", "vaultAddress": vault])
        let (data, _) = try await URLSession.shared.data(for: request)
        guard let apr = (try JSONSerialization.jsonObject(with: data) as? [String: Any])?["apr"] as? NSNumber else { throw URLError(.cannotParseResponse) }
        return apr.decimalValue * 100
    }
}

import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore

/// The vault's net rate (after its fee) from Morpho's public API (api.morpho.org, no key); nil when it doesn't answer.
public enum MorphoRates {
    /// The vault's net APY (after its fee), in %, from Morpho's API.
    public static func netAPY(_ v: MorphoVault.Vault = MorphoVault.base) async -> Decimal? {
        let vault = v.address
        var request = URLRequest(url: URL(string: "https://api.morpho.org/graphql")!)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try? JSONSerialization.data(withJSONObject: [
            "query": "{ vaultByAddress(address: \"\(vault)\", chainId: \(v.rpc.chainId)) { state { netApy } } }",
        ])
        guard let (data, _) = try? await URLSession.shared.data(for: request),
              let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let apy = (((root["data"] as? [String: Any])?["vaultByAddress"] as? [String: Any])?["state"] as? [String: Any])?["netApy"] as? NSNumber
        else { return nil }
        return (apy.decimalValue * 100).rounded(2, .plain)
    }
}

/// A Morpho vault on any network ihatecrypto knows, like Aave: USDC missing there comes from Solana over Relay; a withdrawal
/// goes back to Solana.
extension Executor {
    /// A vault deposit or redeem is ≈ 450k gas — several times an Aave supply — so Base keeps more ETH for it:
    /// 0.000012 ETH covers one at twice today's fee, and $0.05 of ETH lands above that at ETH under ≈ $4,000.
    /// On the other networks their own floor is already above it (Ethereum's covers a vault call at today's fee).
    // ponytail: fixed floor and top-up; derive them from the estimate and the ETH price if Base fees jump.
    static let morphoGas: (floor: UInt64, topupUSD: Decimal) = (12_000_000_000_000, Decimal(string: "0.05")!)

    public func morpho(deposit: Bool, usdc requested: Decimal?, vault v: MorphoVault.Vault = MorphoVault.base) async throws -> String {
        guard let net = EVMNetwork.named(v.chain) else { throw ExecutionError("ihatecrypto doesn't move money on \(v.chainName). Nothing was signed.") }
        let rpc = v.rpc, vault = v.address, usdc = v.usdc, me = wallet.evm
        // The vault's asset is re-read from its own network before anything is signed.
        try await MorphoVault.verify(v)
        func units(_ d: Decimal) -> UInt64 { NSDecimalNumber(decimal: (d * 1_000_000).rounded(0, .down)).uint64Value }
        func show(_ u: UInt64) -> String { Fmt.num(Decimal(u) / 1_000_000, maxFraction: 6) }

        guard deposit else {
            let inside = try await MorphoVault.value(me, v)
            guard inside > 0 else { throw ExecutionError("Nothing is in the Morpho vault on \(v.chainName).") }
            // Everything: redeem every share, so no dust stays; a part: exactly that much USDC.
            let data: Data
            if let part = requested.map(units), part < inside {
                data = try MorphoVault.withdraw(assets: part, receiver: me, owner: me)
            } else {
                let shares = try await rpc.read(vault, try ERC20.balanceOf(me)).prefix(32)
                data = try MorphoVault.redeem(shares: Data(shares), receiver: me, owner: me)
            }
            try await ensureEVM(net, usdc: 0, gas: Self.morphoGas)
            let before = try await rpc.tokenBalance(usdc, owner: me)
            _ = try await sendEVM(rpc, to: vault, data: data, note: "Morpho withdraw \(requested.map { Fmt.num($0) } ?? "all") USDC on \(v.chainName)")
            return try await bridgeBack(net, since: before, expected: requested.map(units).map { min($0, inside) } ?? inside)
        }

        guard let requested, requested > 0 else { throw ExecutionError("Say how much, e.g. “deposit 2 USDC into Morpho”.") }
        let want = units(requested)
        try await ensureEVM(net, usdc: want, gas: Self.morphoGas)
        let before = try await MorphoVault.value(me, v)
        let hash = try await approveAndSend(rpc, token: usdc, spender: vault, units: Decimal(want), approveNote: "Approve exactly \(show(want)) USDC to Morpho on \(v.chainName)",
                                            to: vault, data: try MorphoVault.deposit(assets: want, receiver: me),
                                            note: "Morpho deposit \(show(want)) USDC on \(v.chainName)")
        // A node can answer from a block before the receipt's for a moment: give it up to ten seconds.
        var after = try await MorphoVault.value(me, v)
        for _ in 0..<10 where after <= before {
            try await Task.sleep(for: .seconds(1))
            after = try await MorphoVault.value(me, v)
        }
        guard after > before else { throw ExecutionError("The deposit went through but the vault doesn't show it. \(net.explorer)/tx/\(hash)") }
        let apy = await MorphoRates.netAPY(v)
        return "Deposited \(show(want)) USDC into Morpho · \(v.name) on \(v.chainName)\(apy.map { " at ≈ \(Fmt.num($0))% a year" } ?? ""): "
            + "\(show(after)) USDC there now. \(net.explorer)/tx/\(hash)"
    }
}

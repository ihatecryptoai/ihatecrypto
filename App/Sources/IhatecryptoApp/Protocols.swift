import DeFiKamino
import Foundation
import IhatecryptoCore
import SolanaChain
import SwiftUI
import WalletCore

/// Money put in on a site, so a deposit made in the browser still shows: Kamino's vaults (Earn) from Kamino, and Jupiter's
/// staked JUP (from its escrow on chain) and prediction markets (from Jupiter's API). Shown on the portfolio, counted in
/// the total, told to the chat. Kamino's vaults come out with «withdraw [N] USDC from kamino» (Wallet.kaminoVault).
struct ProtocolPosition: Equatable, Identifiable {
    var id: String { "\(platform) · \(name)" }
    /// «Kamino», «Jupiter».
    let platform: String
    /// «Allez USDC», «Staked JUP».
    let name: String
    let logo: String
    let usd: Decimal
    /// «5.00 USDC» when the protocol says in what.
    let amount: String?
    /// % a year.
    let apy: Decimal?
    /// Where it's managed, for the chat and the row's help.
    let site: String
    /// The token and how much of it, for a withdrawal by amount (Kamino's vaults).
    var symbol: String? = nil
    var tokens: Decimal? = nil
}

extension Workspace {
    var protocolUSD: Decimal { protocolPositions.reduce(0) { $0 + $1.usd } }
    /// What «withdraw … from kamino» takes from: Lend when it holds anything, else the vault holding the token.
    func kaminoVault(_ symbol: String) -> ProtocolPosition? {
        kamino > 0 ? nil : protocolPositions.first { $0.platform == "Kamino" && $0.symbol == symbol }
    }

    /// Read at most every two minutes with the balances; `force` after a site's transaction was signed. A source that
    /// doesn't answer keeps its last reading.
    func loadProtocols(force: Bool = false) async {
        guard force || Date().timeIntervalSince(protocolsReadAt ?? .distantPast) > 120, isSolanaAddress(solanaAddress) else { return }
        protocolsReadAt = Date()
        let wallet = solanaAddress
        async let kamino = try? Kamino.vaultPositions(wallet: wallet)
        // Jupiter's own platforms without its Portfolio API (closed to the public on 2026-10-08). Earn is `lend`'s.
        // ponytail: Jupiter perps aren't read (no API: 9 Position accounts a wallet, P/L and borrow fee by hand), add when asked.
        async let staked = Self.stakedJUP(wallet)
        async let bets = Self.predictions(wallet)
        let vaults = (await kamino)?.map { v in
            ProtocolPosition(platform: "Kamino", name: v.name, logo: "kamino", usd: v.valueUSD, amount: "\(Fmt.num(v.tokens)) \(v.symbol)",
                             apy: v.apy, site: "kamino.com", symbol: v.symbol, tokens: v.tokens)
        } ?? protocolPositions.filter { $0.platform == "Kamino" }
        let jupiter = protocolPositions.filter { $0.platform == "Jupiter" }
        let jup = (await staked ?? jupiter.filter { $0.name == Self.stakedName }) + (await bets ?? jupiter.filter { $0.name != Self.stakedName })
        let found = (vaults + jup).filter { $0.usd >= Decimal(string: "0.01")! }
        if found != protocolPositions { protocolPositions = found }
    }

    static let jupMint = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", stakedName = "Staked JUP"

    /// The wallet's Escrow in Jupiter's locked-voter program (vote.jup.ag): one getAccountInfo, priced through Jupiter's
    /// search. [] when nothing is staked, nil when it can't be read (the last reading stays).
    static func stakedJUP(_ wallet: String) async -> [ProtocolPosition]? {
        let voter = "voTpe3tHQ7AjQHMapgSue2HJFAh2cGsdokqN3XqmVSj", locker = "CVMdMd79no569tjc5Sq7kzz8isbfCcFyBS5TLGsrZ5dN"
        guard let owner = try? Base58.decode(wallet),
              let escrow = try? SPL.programAddress(seeds: [Data("Escrow".utf8), Base58.decode(locker), owner], program: Base58.decode(voter)),
              let reply = try? await SolanaRPC().call("getAccountInfo", [Base58.encode(escrow), ["encoding": "base64"]]) as? [String: Any] else { return nil }
        guard let account = reply["value"] as? [String: Any] else { return reply["value"] is NSNull ? [] : nil }
        guard let b64 = (account["data"] as? [Any])?.first as? String, let data = Data(base64Encoded: b64),
              let jup = escrowJUP(data, owner: owner) else { return nil }
        guard jup > 0 else { return [] }
        guard let usd = await Solana.holdings([jupMint: jup]).tokens.first?.usd else { return nil }
        return [ProtocolPosition(platform: "Jupiter", name: stakedName, logo: "jup", usd: usd, amount: "\(Fmt.num(jup)) JUP", apy: nil,
                                 site: "vote.jup.ag", symbol: "JUP", tokens: jup)]
    }

    /// JUP an Escrow account holds: `amount` (u64 at 105) plus `partial_unstaking_amount` (u64 at 162), which a partial
    /// unstake moves out of `amount` while it stays in the escrow; 6 decimals. nil: not this owner's escrow.
    static func escrowJUP(_ data: Data, owner: Data) -> Decimal? {
        guard data.count >= 170, data.subdata(in: 40..<72) == owner else { return nil }
        func u64(_ at: Int) -> UInt64 { data.subdata(in: at..<at + 8).enumerated().reduce(0) { $0 | UInt64($1.element) << (8 * $1.offset) } }
        return (Decimal(u64(105)) + Decimal(u64(162))) / 1_000_000
    }

    /// Jupiter's prediction markets for the wallet (keyless). nil when it doesn't answer: IPs in the US and Korea are refused.
    static func predictions(_ wallet: String) async -> [ProtocolPosition]? {
        guard let data = try? await TokenResearch.get("https://api.jup.ag/prediction/v1/positions?ownerPubkey=\(wallet)") else { return nil }
        return predictions(data)
    }

    /// A position is worth its `valueUsd` (micro-USD) while its market trades, and $1 a contract once it won and waits
    /// to be claimed. A lost or claimed one is worth nothing here. nil: not Jupiter's list.
    /// ponytail: the first page only, as many as Jupiter returns by default.
    static func predictions(_ data: Data) -> [ProtocolPosition]? {
        guard let rows = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["data"] as? [[String: Any]] else { return nil }
        return rows.compactMap { p in
            let contracts = (p["contractsDecimal"] as? String).flatMap { Decimal(string: $0) }
            let usd: Decimal
            if let micro = (p["valueUsd"] as? String).flatMap({ Decimal(string: $0) }) { usd = micro / 1_000_000 }
            else if p["claimable"] as? Bool == true, p["claimed"] as? Bool != true, let contracts { usd = contracts }
            else { return nil }
            let side = p["isYes"] as? Bool == false ? "NO" : "YES"
            let title = (p["marketMetadata"] as? [String: Any])?["title"] as? String ?? (p["eventMetadata"] as? [String: Any])?["title"] as? String ?? "Prediction"
            return ProtocolPosition(platform: "Jupiter", name: "\(title) · \(side)", logo: "jup", usd: usd,
                                    amount: contracts.map { "\(Fmt.num($0)) \(side)" }, apy: nil, site: "jup.ag/prediction")
        }
    }
}

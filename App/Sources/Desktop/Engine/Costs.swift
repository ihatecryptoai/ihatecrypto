import Foundation
import IhatecryptoCore
import OneClickKit
import SolanaChain
import TradeExecution
import WalletCore

/// The P/L of each coin held, as on the Mac: its average cost from what the wallet paid on-chain (the explorer's or
/// the node's history, priced on the day for what came in unpaid), from the app's own buys, or — when nothing tells —
/// from when the app first saw it.
extension Engine {
    /// Native NEAR bought through NEAR Intents: each swap the journal signed, as 1Click settled it (the Mac's
    /// nearCoinChanges); nil when the journal or the account can't be read.
    func nearCoinChanges() async -> [CostBasis.Change]? {
        guard let account = nearAddress, let entries = try? journal.entries() else { return nil }
        var swaps: [(id: String, at: Date)] = []
        for e in entries where e.chain == "1click" && e.kind == .signed && e.note.contains("NEAR") && !swaps.contains(where: { $0.id == e.id }) {
            swaps.append((e.id, e.at))
        }
        let settled = await withTaskGroup(of: (String, OneClick.Settled?).self) { group in
            for s in swaps { group.addTask { (s.id, try? await OneClick.settled(depositAddress: s.id)) } }
            var out: [String: OneClick.Settled] = [:]
            for await (id, s) in group { out[id] = s }
            return out
        }
        return swaps.compactMap { s in
            settled[s.id]?.near(for: account).map { CostBasis.Change(at: s.at, tokens: $0.amount, paid: $0.amount > 0 ? $0.dollars : nil) }
        }
    }

    /// The cost a coin's P/L is measured from: the chain's or the app's, whichever covers more of it; else «seen».
    func cost(_ key: String) -> CostBasis? {
        [chainCost[key]?.cost, appCosts[key]].compactMap { $0 }.filter { $0.amount > 0 }.max { $0.amount < $1.amount }
            ?? seenCosts[key].flatMap { $0.amount > 0 ? $0 : nil }
    }

    /// The rows' P/L from the costs known, for the window.
    func withProfit(_ p: PortfolioView) -> PortfolioView {
        var p = p
        p.networks = p.networks.map { n in
            var n = n
            n.rows = n.rows.map { r in
                var r = r
                if let key = r.key, let usd = r.usd, r.value > 0, let c = cost(key), let (gain, pct) = c.pnl(held: r.value, price: usd / r.value) {
                    r.pnl = "\(signedUSD(gain)) (\(pct >= 0 ? "+" : "")\(Fmt.num(pct, maxFraction: 1))%)"
                    r.gain = gain
                }
                return r
            }
            return n
        }
        return p
    }

    /// Reads what each coin cost, for the coins whose amount changed since the last read; in the background after a
    /// refresh, one coin at a time (a token's history is up to 50 transactions).
    func loadCosts() async {
        guard !costsLoading, let owner = solanaAddress else { return }
        costsLoading = true
        defer { costsLoading = false }
        let evm = evmAddress
        let wanted = portfolio.networks.flatMap { n in n.rows.compactMap { r -> (key: String, chain: String, contract: String, held: Decimal, price: Decimal?)? in
            guard let key = r.key, let contract = r.contract, r.value > 0, chainCost[key]?.held != r.value else { return nil }
            return (key, n.key, contract, r.value, r.usd.map { $0 / r.value })
        } }
        var solPrices: [Hyperliquid.Candle]?, spot: [String: CostBasis]?, nearTokens: [String: [CostBasis.Change]]?
        for w in wanted {
            guard owner == solanaAddress else { return }   // another wallet since: its own read follows
            var changes: [CostBasis.Change]?, basis: CostBasis?
            switch w.chain {
            case "solana":
                let moves = await SolanaMoves.shared.moves(owner: owner, mint: w.contract)
                if solPrices == nil, w.contract != CostBasis.wsol, moves.contains(where: { ($0.change[w.contract] ?? 0) > 0 }) {
                    solPrices = (try? await Hyperliquid.candles(coin: "SOL", interval: "4h", from: Date().addingTimeInterval(-800 * 86_400))) ?? []
                }
                changes = moves.isEmpty ? nil : CostBasis.replaySolana(mint: w.contract, moves: moves) { at in solPrices?.last { $0.time <= at }?.close }
            case "hyperliquid":
                guard let evm else { break }
                if spot == nil { spot = await Holdings.hyperliquidEntries(user: evm) }
                basis = spot.map { $0[w.contract] ?? CostBasis() }
            case "near":
                if w.contract == "native" { changes = await nearCoinChanges(); break }
                guard let account = nearAddress else { break }
                if nearTokens == nil { nearTokens = await Holdings.nearTokenChanges(account: account) }
                changes = nearTokens.map { $0[w.contract] ?? [] }
            default:
                guard let evm else { break }
                changes = w.contract == "native" ? await Holdings.coinChanges(w.chain, address: evm)
                    : await Holdings.tokenChanges(w.chain, address: evm, contract: w.contract)
            }
            if let changes {
                let days = CostBasis.unpaid(changes)
                var prices: [Date: Decimal]? = [:]
                if !days.isEmpty, let id = Holdings.llamaID(chain: w.chain, contract: w.contract) { prices = await Holdings.pastPrices(id, at: days) }
                basis = prices.map { p in CostBasis.fold(changes, held: w.held) { p[$0] } }
            }
            // A switch during this coin's requests: its cost isn't the new wallet's.
            guard owner == solanaAddress else { return }
            if let basis { chainCost[w.key] = (w.held, basis) }
            // Nothing tells what it cost: from when the app first saw it.
            if (basis?.amount ?? 0) == 0, (appCosts[w.key]?.amount ?? 0) == 0, let price = w.price { seen(w.key, held: w.held, price: price) }
        }
        portfolio = withProfit(portfolio)
        changed()
    }

    func seen(_ key: String, held: Decimal, price: Decimal) {
        var s = seenCosts[key] ?? CostBasis()
        if held > s.amount { s.came(held - s.amount, worth: (held - s.amount) * price) } else if held < s.amount { s.sold(s.amount - held) }
        if s != seenCosts[key] { seenCosts[key] = s }
    }
}

/// Each Solana transaction a wallet's token account took part in, read once (a transaction never changes): the Mac's.
actor SolanaMoves {
    static let shared = SolanaMoves()
    private var read: [String: CostBasis.SolanaMove] = [:]

    /// The owner's moves in `mint`, oldest first: the 50 newest transactions of its token account; for SOL, the wallet's.
    // ponytail: 50 a token, as on the EVM networks; what was held before them counts at the price then.
    func moves(owner: String, mint: String) async -> [CostBasis.SolanaMove] {
        let rpc = SolanaRPC()
        let accounts = mint == CostBasis.wsol ? [owner]
            : [SPL.tokenProgram, SPL.token2022Program].compactMap { try? SPL.associatedAccount(owner: owner, mint: mint, program: $0) }
        var signatures: [String] = []
        for account in accounts {
            let list = (try? await rpc.call("getSignaturesForAddress", [account, ["limit": 50]])) as? [[String: Any]] ?? []
            signatures += list.filter { $0["err"] == nil || $0["err"] is NSNull }.compactMap { $0["signature"] as? String }
        }
        var out: [CostBasis.SolanaMove] = []
        for sig in Set(signatures) {
            if read[sig] == nil,
               let tx = (try? await rpc.call("getTransaction", [sig, ["encoding": "jsonParsed", "maxSupportedTransactionVersion": 0]])) as? [String: Any],
               let move = CostBasis.SolanaMove(tx, owner: owner) {
                read[sig] = move
            }
            if let m = read[sig] { out.append(m) }
        }
        return out.sorted { $0.at < $1.at }
    }
}

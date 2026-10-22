import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// The balances the window shows: a card per network, the coins on it with their dollar value, and the total.
nonisolated struct PortfolioView: Encodable, Equatable {
    struct Row: Encodable, Equatable {
        var symbol: String
        var amount: String
        /// The amount as a number, for the «money arrived» check.
        var value: Decimal
        var usd: Decimal?
        var icon: String?
    }
    struct Network: Encodable, Equatable {
        var key: String
        var name: String
        var usd: Decimal
        var rows: [Row]
    }
    var networks: [Network] = []
    var total: Decimal = 0
    var updatedAt: Date?
    var errors: [String] = []
}

extension Engine {
    /// Every network read at once, then drawn together. A network that didn't answer keeps its last card.
    func refresh() async {
        guard !refreshing, let me = tradeWallet else { return }
        refreshing = true
        defer { refreshing = false }
        let near = nearAddress
        async let marketsRead = try? Hyperliquid.markets()
        async let solanaRead = Self.solana(me.solana)
        async let hlRead = Self.hyperliquid(me.evm)
        async let evmRead = Self.evm(me.evm)
        async let nearRead = Self.near(near)
        if let m = await marketsRead { markets = m }
        let parts = [await solanaRead, await hlRead] + (await evmRead) + [await nearRead]
        let last = Dictionary(uniqueKeysWithValues: portfolio.networks.map { ($0.key, $0) })
        var networks: [PortfolioView.Network] = [], errors: [String] = []
        for part in parts {
            switch part {
            case .success(let n?): if !n.rows.isEmpty { networks.append(priced(n)) }
            case .success(nil): break
            case .failure(let key, let why):
                errors.append(why)
                if let kept = last[key] { networks.append(kept) }
            }
        }
        // Solana, then Hyperliquid, then the rest by their money, as on the Mac.
        let order = ["solana": 0, "hyperliquid": 1]
        networks.sort { (order[$0.key] ?? 2, -$0.usd) < (order[$1.key] ?? 2, -$1.usd) }
        let next = PortfolioView(networks: networks, total: networks.reduce(0) { $0 + $1.usd }, updatedAt: Date(), errors: errors)
        // Another wallet since the last read: its balances aren't money that arrived.
        if portfolio.updatedAt != nil, portfolioOwner == me.solana { announceIncoming(from: portfolio, to: next) }
        portfolio = next
        portfolioOwner = me.solana
        if errors.isEmpty { recordEquity(next.total, wallet: me.solana) }
        positions = (try? await HyperliquidAccount.positions(me.evm)) ?? positions
        await checkAlerts()
        changed()
    }

    /// Dollars and SOL that came in since the last read, said in the chat, as the Mac does. Not after a move of the
    /// user's own (two minutes), whose change the chat already told.
    func announceIncoming(from old: PortfolioView, to new: PortfolioView) {
        guard running == nil, Date().timeIntervalSince(lastOwnMove) > 120 else { return }
        func amounts(_ p: PortfolioView) -> [String: Decimal] {
            Dictionary(p.networks.flatMap { n in n.rows.filter { ["USDC", "USDG", "SOL"].contains($0.symbol) }.map { ("\(n.name)|\($0.symbol)", $0.value) } },
                       uniquingKeysWith: +)
        }
        let before = amounts(old), after = amounts(new)
        let lines = after.sorted { $0.key < $1.key }.compactMap { key, now -> String? in
            let was = before[key] ?? 0, parts = key.split(separator: "|")
            let least = parts[1] == "SOL" ? Decimal(string: "0.0005")! : Decimal(string: "0.01")!
            guard now - was >= least, old.networks.contains(where: { $0.name == parts[0] }) || was == 0 else { return nil }
            return "+\(Fmt.num(now - was, maxFraction: parts[1] == "SOL" ? 4 : 2)) \(parts[1]) " + say("on", "на") + " \(parts[0])"
        }
        guard !lines.isEmpty else { return }
        // What came in, in dollars, so the P/L counts it as money moved in, not as profit.
        for (key, now) in after {
            let parts = key.split(separator: "|"), was = before[key] ?? 0
            guard now > was else { continue }
            let usd = parts[1] == "SOL" ? (markets["SOL"]?.markPx ?? 0) * (now - was) : now - was
            recordFlow(usd, "\(parts[1]) in on \(parts[0])")
        }
        let text = say("💸 Received ", "💸 Пришло ") + lines.joined(separator: ", ") + "."
        out.event("notify", ["title": say("Money arrived", "Пришли деньги"), "body": text])
        reply(text, note: "incoming")
    }

    /// Coins with no price of their own take Hyperliquid's mark for the same coin (SOL, ETH, HYPE…).
    private func priced(_ n: PortfolioView.Network) -> PortfolioView.Network {
        var n = n
        n.rows = n.rows.map { r in
            var r = r
            if r.usd == nil, let mark = markets[r.symbol]?.markPx { r.usd = mark * r.value }
            return r
        }
        n.usd = n.rows.reduce(0) { $0 + ($1.usd ?? 0) }
        return n
    }

    nonisolated enum Read: Sendable {
        case success(PortfolioView.Network?)
        case failure(String, String)
    }

    nonisolated static func row(_ symbol: String, _ amount: Decimal, usd: Decimal? = nil, icon: URL? = nil) -> PortfolioView.Row {
        .init(symbol: symbol, amount: Fmt.num(amount, maxFraction: amount < 1 ? 6 : 4), value: amount, usd: usd, icon: icon?.absoluteString)
    }

    nonisolated static func solana(_ owner: String) async -> Read {
        do {
            let b = try await Solana.balance(owner: owner)
            var rows: [PortfolioView.Row] = []
            if b.usdc > 0 { rows.append(row("USDC", b.usdc, usd: b.usdc)) }
            if b.sol > 0 { rows.append(row("SOL", b.sol)) }
            rows += b.tokens.filter(\.looksReal).map { row($0.symbol, $0.amount, usd: $0.usd, icon: $0.icon) }
            return .success(.init(key: "solana", name: "Solana", usd: 0, rows: rows))
        } catch {
            return .failure("solana", "Solana: \(error.localizedDescription)")
        }
    }

    nonisolated static func hyperliquid(_ user: String) async -> Read {
        do {
            let b = try await Hyperliquid.balance(user: user)
            var rows: [PortfolioView.Row] = []
            if b.accountValue > 0 { rows.append(row("Perps", b.accountValue, usd: b.accountValue)) }
            if b.spotUSDC > 0 { rows.append(row("USDC", b.spotUSDC, usd: b.spotUSDC)) }
            rows += ((await Holdings.hyperliquidSpot(user: user)) ?? []).filter { $0.symbol != "USDC" && $0.looksReal }
                .map { row($0.symbol, $0.amount, usd: $0.usd, icon: $0.icon) }
            return .success(.init(key: "hyperliquid", name: "Hyperliquid", usd: 0, rows: rows))
        } catch {
            return .failure("hyperliquid", "Hyperliquid: \(error.localizedDescription)")
        }
    }

    /// Each EVM network: its dollar (USDC, USDG on Robinhood), its gas coin, and the other tokens an explorer lists.
    nonisolated static func evm(_ owner: String) async -> [Read] {
        await withTaskGroup(of: (Int, Read).self) { group in
            for (i, net) in EVMNetwork.all.enumerated() {
                group.addTask {
                    do {
                        async let dollars = net.usdcBalance(owner)
                        async let gas = net.feeToken == nil ? net.rpc.balance(owner) : 0
                        let usd = Decimal(try await dollars) / 1_000_000
                        let coin = Decimal(try await gas) / Decimal(sign: .plus, exponent: 18, significand: 1)
                        var rows: [PortfolioView.Row] = []
                        if usd > 0 { rows.append(row(net.key == "robinhood" ? "USDG" : "USDC", usd, usd: usd)) }
                        if coin > 0 { rows.append(row(net.gasCoin, coin)) }
                        let others = (await Holdings.evm(net.key, address: owner, native: false, skip: [net.usdc.lowercased()])) ?? []
                        rows += others.filter(\.looksReal).map { row($0.symbol, $0.amount, usd: $0.usd, icon: $0.icon) }
                        return (i, .success(.init(key: net.key, name: net.name, usd: 0, rows: rows)))
                    } catch {
                        return (i, .failure(net.key, "\(net.name): \(error.localizedDescription)"))
                    }
                }
            }
            var out: [(Int, Read)] = []
            for await r in group { out.append(r) }
            return out.sorted { $0.0 < $1.0 }.map(\.1)
        }
    }

    nonisolated static func near(_ account: String?) async -> Read {
        guard let account else { return .success(nil) }
        do {
            let b = try await Near.balance(account: account)
            let total = b.near + b.wrapped
            return .success(.init(key: "near", name: "NEAR", usd: 0, rows: total > 0 ? [row("NEAR", total)] : []))
        } catch {
            return .failure("near", "NEAR: \(error.localizedDescription)")
        }
    }
}

import DeFiJupiterLend
import DeFiKamino
import EVMChain
import Foundation
import IhatecryptoCore
import SignerAPI
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
        /// Where its cost is kept (CostBasis): the mint on Solana, «chain:contract» elsewhere; nil for dollars and perps.
        var key: String?
        var contract: String?
        /// «+$1.20 (+5.0%)» against what it cost, when known.
        var pnl: String?
        var gain: Decimal?
        /// How far the price moved in 24 hours, in %, where its market says; nil for dollars.
        var change24h: Decimal?
        /// A market list vouches for it (true), or nothing does (false: «unverified»); nil for dollars and gas.
        var verified: Bool?
        /// «tokens», or «defi» for money put in somewhere (Jupiter Lend, Kamino, Aave, Morpho).
        var section = "tokens"
        /// Under a deposit's name: «earning», «Allez USDC · 5.42%».
        var note: String?
        /// A token found by listing the address (Holding.id): the person can hide it or show it again; nil for the
        /// network's dollars, gas and deposits, which always show.
        var id: String?
        /// It looks real (Holding.looksReal): verified, or a real pool and a cent's worth. Shown unless the person hid it;
        /// the others are tucked until the person shows them.
        var real: Bool?
        /// A deposit managed on its own site (Jupiter's other platforms): a click opens it in the browser, as the Mac's
        /// opens it beside the chat.
        var site: String?
    }
    struct Network: Encodable, Equatable {
        var key: String
        var name: String
        var usd: Decimal
        var rows: [Row]
        /// What its money does, as the Mac's network card says it: «idle $757.59 · earning $2.51», «$0.01 · margin».
        var note: String?
        /// Tokens tucked under «N hidden · show»: hidden by the person, or airdrops that look like scams. Out of `usd`.
        var hidden: [Row] = []
        /// Tucked beyond the first hundred (a spammed address has hundreds): counted, not sent.
        var hiddenMore = 0
    }
    struct Point: Encodable, Equatable { var at: Date; var usd: Decimal }
    var networks: [Network] = []
    var total: Decimal = 0
    var updatedAt: Date?
    var errors: [String] = []
    /// The wallet's value over the last 7 days, oldest first, the last point now: the sparkline.
    var equity: [Point] = []
    /// Its change over those days; nil with fewer than three points, as the Mac draws no line then.
    var change7d: Decimal?
}

extension Engine {
    /// Every network read at once, then drawn together. A network that didn't answer keeps its last card.
    func refresh() async {
        guard !refreshing, let me = readWallet else { return }
        refreshing = true
        defer { refreshing = false }
        let near = nearAddress
        async let automationRead = Self.automationMoney(automationWallet)
        async let marketsRead = try? Hyperliquid.markets()
        // A wallet of one chain (a key, a watched address) has nothing to read on the other.
        async let solanaRead = me.solana.isEmpty ? .success(nil) : Self.solana(me.solana)
        async let hlRead: (Read, PlaceFacts.HL?) = me.evm.isEmpty ? (.success(nil), nil) : Self.hyperliquid(me.evm)
        async let evmRead = me.evm.isEmpty ? [] : Self.evm(me.evm)
        async let nearRead = Self.near(near)
        if let m = await marketsRead { markets = m }
        let (hlPart, hlFacts) = await hlRead
        // A read that failed keeps what the details board showed.
        if hlFacts != nil || me.evm.isEmpty { placeFacts.hyperliquid = hlFacts }
        let parts = [await solanaRead, hlPart] + (await evmRead) + [await nearRead]
        let last = Dictionary(uniqueKeysWithValues: portfolio.networks.map { ($0.key, $0) })
        var networks: [PortfolioView.Network] = [], errors: [String] = []
        for part in parts {
            switch part {
            case .success(let n?):
                // A network with nothing on show (only airdrops) has no card, as before they were tucked.
                let n = Self.tucked(priced(n), choices: settings.tokenChoices ?? [:])
                if !n.rows.isEmpty { networks.append(noted(n)) }
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
        let owner = me.solana + me.evm
        let sameWallet = portfolioOwner == owner
        if portfolio.updatedAt != nil, portfolioOwner == owner { announceIncoming(from: portfolio, to: next) }
        portfolio = withProfit(next)
        portfolioOwner = owner
        Task { await loadCosts() }
        if errors.isEmpty { recordEquity(next.total, wallet: me.solana.isEmpty ? me.evm : me.solana) }
        loadEquity()
        // A read that failed keeps the last one: read as it came, the balance would drop by Automation's money.
        if automationWallet == nil { automationUSD = nil } else if let m = await automationRead { automationUSD = m.dollars + m.sol * (markets["SOL"]?.markPx ?? 0) }
        if !me.evm.isEmpty {
            // A request that failed keeps what was read before: read as «none», a position would look closed by its stop.
            async let ordersRead = try? HyperliquidAccount.openOrders(me.evm)
            let before = positions.map(\.coin)
            positions = (try? await HyperliquidAccount.positions(me.evm))?.filter { $0.size != 0 } ?? positions
            hlOrders = await ordersRead ?? hlOrders
            // Another wallet since the last read: its positions aren't this one's, closed.
            if sameWallet { await noteClosedPositions(before: before) }
            await guardPositions()
        }
        await refreshRates()
        await refreshFavorites()
        await checkAlerts()
        await noticeHyperliquid()
        await readTransit()
        // Every network's gas coin is known now: dollars without it get their way out offered (once a network).
        offerGasWays()
        changed()
    }

    /// Dollars and SOL that came in since the last read, said in the chat, as the Mac does. Not after a move of the
    /// user's own (two minutes), whose change the chat already told.
    func announceIncoming(from old: PortfolioView, to new: PortfolioView) {
        guard running == nil, Date().timeIntervalSince(lastOwnMove) > 120 else { return }
        func amounts(_ p: PortfolioView) -> [String: Decimal] {
            Dictionary(p.networks.flatMap { n in n.rows.filter { $0.id == nil && ["USDC", "USDG", "SOL"].contains($0.symbol) }.map { ("\(n.name)|\($0.symbol)", $0.value) } },
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
        notify(.incoming, say("Money arrived", "Пришли деньги"), text)
        reply(text, note: "incoming")
    }

    /// Coins with no price of their own take Hyperliquid's mark for the same coin (SOL, ETH, HYPE…), and its day's move.
    private func priced(_ n: PortfolioView.Network) -> PortfolioView.Network {
        var n = n
        n.rows = n.rows.map { r in
            var r = r
            if r.usd == nil, let market = markets[r.symbol] {
                r.usd = market.markPx * r.value
                r.change24h = r.change24h ?? market.change24h
            }
            return r
        }
        n.usd = n.rows.reduce(0) { $0 + ($1.usd ?? 0) }
        return n
    }

    func noted(_ n: PortfolioView.Network) -> PortfolioView.Network {
        var n = n
        n.note = Self.note(n, russian: russian)
        // A deposit's «earning» in the window's language; a vault's own name and rate stay as they are.
        if russian { n.rows = n.rows.map { r in var r = r; if r.note == "earning" { r.note = "приносит доход" }; return r } }
        return n
    }

    /// The line under a network's name, as the Mac's NetworkCard writes it: Hyperliquid's margin, Tempo's purpose, else
    /// what's idle (dollars and the big stablecoins) and what's earning (deposits), the bigger first; nil when neither.
    nonisolated static func note(_ n: PortfolioView.Network, russian: Bool = false) -> String? {
        switch n.key {
        case "hyperliquid":
            let hlp = n.rows.first { $0.symbol == "HLP" }?.usd ?? 0
            return "\(Fmt.usd(n.rows.first { $0.symbol == "Perps" }?.usd ?? 0)) · " + (russian ? "маржа" : "margin") + (hlp > 0 ? " · HLP \(Fmt.usd(hlp))" : "")
        case "tempo": return russian ? "USDC для платных сервисов" : "USDC for paid services"
        default:
            // Dollars carry no cost key (Engine.row): USDC, USDG and the genuine stablecoins.
            let idle = n.rows.filter { $0.section == "tokens" && $0.key == nil }.reduce(Decimal(0)) { $0 + ($1.usd ?? 0) }
            let earning = n.rows.filter { $0.section == "defi" }.reduce(Decimal(0)) { $0 + ($1.usd ?? 0) }
            let sums = [(russian ? "в работе" : "earning", earning), (russian ? "свободно" : "idle", idle)].filter { $0.1 >= Decimal(string: "0.01")! }.sorted { $0.1 > $1.1 }
            return sums.isEmpty ? nil : sums.map { "\($0.0) \(Fmt.usd($0.1))" }.joined(separator: " · ")
        }
    }

    /// The automation wallet's money, as the Mac counts it beside the balance: its Solana USDC and tokens and its USDC on
    /// every EVM network in dollars, and its SOL (priced by the caller). nil without the wallet, or when Solana didn't
    /// answer or came unpriced.
    nonisolated static func automationMoney(_ wallet: SignerWallet?) async -> (dollars: Decimal, sol: Decimal)? {
        guard let wallet else { return nil }
        async let evm: Decimal = withTaskGroup(of: Decimal.self) { group in
            guard let owner = wallet.evm else { return 0 }
            for net in EVMNetwork.all { group.addTask { Decimal((try? await net.usdcBalance(owner)) ?? 0) / 1_000_000 } }
            return await group.reduce(0, +)
        }
        var solana: (dollars: Decimal, sol: Decimal) = (0, 0)
        if let owner = wallet.solana {
            guard let b = try? await Solana.balance(owner: owner), !b.unpriced else { return nil }
            solana = (b.usdc + b.tokens.filter(\.looksReal).reduce(0) { $0 + ($1.usd ?? 0) }, b.sol)
        }
        return (solana.dollars + (await evm), solana.sol)
    }

    nonisolated enum Read: Sendable {
        case success(PortfolioView.Network?)
        case failure(String, String)
    }

    /// `chain` and `contract` for a coin whose P/L counts (not a dollar, not the perps' margin).
    nonisolated static func row(_ symbol: String, _ amount: Decimal, usd: Decimal? = nil, icon: URL? = nil, chain: String? = nil, contract: String? = nil,
                                change24h: Decimal? = nil, verified: Bool? = nil) -> PortfolioView.Row {
        var key: String?
        if let chain, let contract, !Stablecoins.isGenuine(contract, on: chain) {
            key = switch chain {
            case "solana": contract
            case "hyperliquid": "hyperliquid:\(contract.lowercased())"
            default: "\(chain):\(contract.lowercased())"
            }
        }
        // A dollar has no day's move to show.
        return .init(symbol: symbol, amount: Fmt.num(amount, maxFraction: amount < 1 ? 6 : 4), value: amount, usd: usd, icon: icon?.absoluteString,
                     key: key, contract: key == nil ? nil : contract, change24h: key == nil ? nil : change24h, verified: verified)
    }

    /// A token found by listing the address, with its day's move and whether a market list vouches for it.
    nonisolated static func row(_ h: Holding) -> PortfolioView.Row {
        // As the Mac marks it: verified, or «unverified» — never on Hyperliquid's spot book, where a name is one token.
        var r = row(h.symbol, h.amount, usd: h.usd, icon: h.icon, chain: h.chain, contract: h.contract, change24h: h.change24h,
                    verified: h.verified ? true : h.unverified ? false : nil)
        r.id = h.id
        r.real = h.looksReal
        return r
    }

    /// A deposit in the DeFi section: its dollars, and what it does under its name.
    nonisolated static func deposit(_ name: String, usd: Decimal, note: String) -> PortfolioView.Row {
        var r = row(name, usd, usd: usd)
        r.section = "defi"
        r.note = note
        return r
    }

    /// Jupiter's portfolio API (its own platforms only), as the Mac's jupiterPositions: every element but Earn, which
    /// Jupiter Lend already reads. nil when it doesn't answer.
    nonisolated static func jupiterPlatforms(_ wallet: String) async -> [PortfolioView.Row]? {
        struct Answer: Decodable {
            struct Element: Decodable { var fetcherId: String?; var platformId: String?; var name: String?; var label: String?; var value: Double?; var netApy: Double? }
            var elements: [Element]
        }
        guard let url = URL(string: "https://api.jup.ag/portfolio/v1/positions/\(wallet)"),
              let (data, _) = try? await URLSession.shared.data(for: URLRequest(url: url, timeoutInterval: 20)),
              let answer = try? JSONDecoder().decode(Answer.self, from: data) else { return nil }
        return jupiterRows(answer.elements.map { ($0.fetcherId, $0.platformId, $0.name, $0.label, $0.value, $0.netApy) })
    }

    /// The rows for Jupiter's elements: «Jupiter», its name and label, its rate a year when it earns one.
    nonisolated static func jupiterRows(_ elements: [(fetcher: String?, platform: String?, name: String?, label: String?, value: Double?, apy: Double?)]) -> [PortfolioView.Row] {
        elements.compactMap { e in
            guard e.fetcher != "jupiter-exchange-earn", let value = e.value, value.isFinite else { return nil }
            let name = [e.name, e.label].compactMap { $0 }.joined(separator: " · ")
            let apy = e.apy.flatMap { $0 > 0 ? " · \(Fmt.num((Decimal($0) * 100).rounded(2, .plain)))%" : nil } ?? ""
            var row = deposit("Jupiter", usd: Decimal(value).rounded(2, .plain), note: (name.isEmpty ? e.platform ?? "position" : name) + apy)
            row.icon = "logos/jup.png"
            row.site = "jup.ag"
            return row
        }
    }

    nonisolated static func solana(_ owner: String) async -> Read {
        do {
            async let lendRead = try? JupiterLend.positions(user: owner)
            async let kaminoRead = try? Kamino.positions(wallet: owner)
            async let vaultsRead = try? Kamino.vaultPositions(wallet: owner)
            async let jupiterRead = jupiterPlatforms(owner)
            let b = try await Solana.balance(owner: owner)
            var rows: [PortfolioView.Row] = []
            if b.usdc > 0 { rows.append(row("USDC", b.usdc, usd: b.usdc)) }
            if b.sol > 0 { rows.append(row("SOL", b.sol, chain: "solana", contract: CostBasis.wsol)) }
            rows += b.tokens.map(row)
            // What's put in, as the Mac's DeFi lines: Jupiter Lend and Kamino Lend earning, Kamino's vaults by name and rate.
            let cent = Decimal(string: "0.01")!
            let lend = ((await lendRead) ?? []).reduce(Decimal(0)) { $0 + Decimal($1.assets) / 1_000_000 }
            if lend >= cent { rows.append(deposit("Jupiter Lend", usd: lend, note: "earning")) }
            let kamino = ((await kaminoRead) ?? []).reduce(Decimal(0)) { $0 + $1.valueUSD }
            if kamino >= cent { rows.append(deposit("Kamino Lend", usd: kamino, note: "earning")) }
            for v in (await vaultsRead) ?? [] where v.valueUSD >= cent {
                rows.append(deposit("Kamino", usd: v.valueUSD, note: "\(v.name) · \(Fmt.num(v.apy.rounded(2, .plain)))%"))
            }
            // Jupiter's other platforms (staked JUP, perps, predictions…), so a deposit made on its site still shows.
            rows += ((await jupiterRead) ?? []).filter { ($0.usd ?? 0) >= cent }
            return .success(.init(key: "solana", name: "Solana", usd: 0, rows: rows))
        } catch {
            return .failure("solana", "Solana: \(error.localizedDescription)")
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
                        if coin > 0 { rows.append(row(net.gasCoin, coin, chain: net.key, contract: "native")) }
                        let others = (await Holdings.evm(net.key, address: owner, native: false, skip: [net.usdc.lowercased()])) ?? []
                        rows += others.map(row)
                        // USDC lent on Aave or in Morpho's vault here; a market that doesn't answer shows nothing this time.
                        for m in AaveV3.markets where m.chain == net.key {
                            if let raw = try? await m.rpc.tokenBalance(m.aUSDC, owner: owner), raw >= 10_000 {
                                rows.append(deposit("Aave", usd: Decimal(raw) / 1_000_000, note: "earning"))
                            }
                        }
                        for v in MorphoVault.vaults where v.chain == net.key {
                            if let raw = try? await MorphoVault.value(owner, v), raw >= 10_000 {
                                rows.append(deposit("Morpho", usd: Decimal(raw) / 1_000_000, note: "earning"))
                            }
                        }
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
            var rows = total > 0 ? [row("NEAR", total)] : []
            rows += ((await Holdings.near(account: account)) ?? []).filter { $0.contract != "native" && $0.contract != "wrap.near" }.map(row)
            return .success(.init(key: "near", name: "NEAR", usd: 0, rows: rows))
        } catch {
            return .failure("near", "NEAR: \(error.localizedDescription)")
        }
    }
}

// MARK: The token card's P/L

extension Engine {
    /// The token card's held P/L as the Mac's TokenBoard says it under the number: «Bought 0.3968 NEAR for $2.00 at
    /// $5.04 · 0.0766 more came in from elsewhere, counted at the price then: $5.10 · worth $2.39 now», and since when
    /// («since you bought on Oct 10»; «since Oct 13» when some of it came in unpaid).
    nonisolated static func heldPaid(symbol: String, amount: Decimal, gain: Decimal?, cost: CostBasis?, ru: Bool) -> (paid: String, since: String?) {
        func say(_ en: String, _ r: String) -> String { ru ? r : en }
        guard let gain, let cost, cost.amount > 0 else { return (say("No price yet to count its P/L from", "Пока нет цены, от которой считать прибыль"), nil) }
        let counted = min(amount, cost.amount), share = counted / cost.amount
        let came = (cost.received ?? 0) * share, cameUSD = (cost.receivedUSD ?? 0) * share
        let bought = counted - came, spent = cost.usd * share - cameUSD
        var parts: [String] = []
        if bought > 0 {
            parts.append(say("Bought \(Fmt.num(bought, maxFraction: 4)) \(symbol) for \(Fmt.usd(spent)) at \(tokenPrice(spent / bought))",
                             "Куплено \(Fmt.num(bought, maxFraction: 4)) \(symbol) за \(Fmt.usd(spent)) по \(tokenPrice(spent / bought))"))
        }
        if came > 0 {
            parts.append(say("\(Fmt.num(came, maxFraction: 4)) \(bought > 0 ? "more" : symbol) came in from elsewhere, counted at the price then: \(tokenPrice(cameUSD / came))",
                             "\(bought > 0 ? "ещё " : "")\(Fmt.num(came, maxFraction: 4))\(bought > 0 ? "" : " " + symbol) пришло со стороны, по цене того дня: \(tokenPrice(cameUSD / came))"))
        }
        parts.append(say("worth \(Fmt.usd(cost.usd * share + gain)) now", "сейчас стоит \(Fmt.usd(cost.usd * share + gain))"))
        if amount > counted {
            parts.append(say("\(Fmt.num(amount - counted, maxFraction: 4)) more not counted: no price for it", "ещё \(Fmt.num(amount - counted, maxFraction: 4)) не посчитано: для них нет цены"))
        }
        let since = cost.since.map { day in
            let f = DateFormatter()
            f.locale = Locale(identifier: ru ? "ru_RU" : "en_US")
            f.calendar = Calendar(identifier: .gregorian)
            f.setLocalizedDateFormatFromTemplate("d MMM")
            let d = f.string(from: day)
            return (cost.received ?? 0) > 0 ? say("since \(d)", "с \(d)") : say("since you bought on \(d)", "с покупки \(d)")
        }
        return (parts.joined(separator: " · "), since)
    }

    /// A token's price with the digits a small one needs: $5.04, $0.0001234 (the Mac's tokenPrice).
    nonisolated static func tokenPrice(_ d: Decimal) -> String {
        if d >= 1 { return Fmt.usd(d) }
        var digits = 2, x = d
        while x < 1, x > 0, digits < 10 { x *= 10; digits += 1 }
        return "$" + Fmt.num(d, maxFraction: min(digits + 2, 12))
    }
}

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
        /// One of Robinhood Chain's stock tokens: its Buy and Sell name the stock, not the contract (the Mac's tokenPhrases).
        var stock: Bool?
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
    /// When the networks were last read: every read, whole or not — what the checks and the chat go by.
    var updatedAt: Date?
    /// The balance on screen is whole (the Mac's balanceReady): this wallet's Solana and Hyperliquid and its Automation
    /// wallet have answered since launch or the switch. Until then the screen shows the last full balance, dimmed.
    var ready = false
    var errors: [String] = []
    /// The wallet's value over the last 7 days, oldest first, the last point now: the sparkline.
    var equity: [Point] = []
    /// Its change over those days; nil with fewer than three points, as the Mac draws no line then.
    var change7d: Decimal?
}

extension Engine {
    /// Every network read at once, then drawn together. A network that didn't answer keeps its last card. One read at a
    /// time; one asked for meanwhile (a wallet switch's, a move's) runs right after it instead of being dropped.
    /// Asked during a read, it returns once the next read has landed — one begun after the ask — as the Mac's
    /// `await ws.refresh()` does: the rail's spinner and a chain's «reads balances after this one» wait for it.
    func refresh() async {
        if refreshing {
            refreshAgain = true
            let pass = readsStarted + 1
            await withCheckedContinuation { readWaiters.append((pass, $0)) }
            return
        }
        refreshing = true
        defer { refreshing = false }
        repeat {
            refreshAgain = false
            readsStarted += 1
            await readOnce()
            landReads()
        } while refreshAgain
    }

    /// A read landed: the callers waiting for it, or for an earlier one, go on.
    func landReads() {
        let done = readsStarted
        let landed = readWaiters.filter { $0.pass <= done }
        readWaiters.removeAll { $0.pass <= done }
        landed.forEach { $0.done.resume() }
    }

    /// Nothing the last wallet's reads left is the new one's (the Mac's walletChanged clears solana, hl, dexBalances, hlp,
    /// its orders and recentlyClosed): the balances, positions and orders, what each network has answered since it came
    /// on screen, the Solana tokens that price an unpriced read, Hyperliquid's details, the closed trades and the
    /// notices' memory of its positions.
    func forgetWalletReads() {
        portfolio = PortfolioView()
        positions = []
        hlOrders = []
        positionsFor = nil
        seenSolanaPriced = nil
        seenHL = false
        solanaHoldings = []
        placeFacts.hyperliquid = nil
        recentlyClosed = []
        NoticeMemory.positions = nil
        NoticeMemory.entries = []
    }

    private func readOnce() async {
        guard let me = readWallet else { return }
        let near = nearAddress
        // Another wallet since the read began: nothing of it is this one's — not its balances, its positions or what the
        // guardian does with them — and the new wallet's own read follows (the Mac's same() / user == hlAddress).
        func same() -> Bool { readWallet.map { $0.solana + $0.evm } == me.solana + me.evm && nearAddress == near }
        async let automationRead = Self.automationMoney(automationWallet)
        async let marketsRead = try? Hyperliquid.markets()
        // A wallet of one chain (a key, a watched address) has nothing to read on the other.
        async let solanaRead: (Read, [Holding], Bool) = me.solana.isEmpty ? (.success(nil), [], false) : Self.solana(me.solana, last: solanaHoldings)
        async let hlRead: (Read, PlaceFacts.HL?) = me.evm.isEmpty ? (.success(nil), nil) : Self.hyperliquid(me.evm)
        async let evmRead = me.evm.isEmpty ? [] : Self.evm(me.evm)
        async let nearRead = Self.near(near)
        if let m = await marketsRead { markets = m }
        let (hlPart, hlFacts) = await hlRead
        let (solanaPart, holdings, unpriced) = await solanaRead
        let parts = [solanaPart, hlPart] + (await evmRead) + [await nearRead]
        let automation = await automationRead
        guard same() else { return }
        // A read that failed keeps what the details board showed.
        if hlFacts != nil || me.evm.isEmpty { placeFacts.hyperliquid = hlFacts }
        if case .success = solanaPart, !me.solana.isEmpty { solanaHoldings = holdings }
        let last = Dictionary(uniqueKeysWithValues: portfolio.networks.map { ($0.key, $0) })
        var networks: [PortfolioView.Network] = [], errors: [String] = []
        for part in parts {
            switch part {
            case .success(let n?):
                // Solana's and Hyperliquid's cards stay once their read answered, at $0 too, and a network whose tokens are
                // all tucked keeps its card for «N hidden · show» (the Mac's PortfolioNetworks.present).
                let n = Self.tucked(priced(n), choices: settings.tokenChoices ?? [:])
                if !n.rows.isEmpty || !n.hidden.isEmpty || n.hiddenMore > 0 || n.key == "solana" || n.key == "hyperliquid" { networks.append(noted(n)) }
            case .success(nil): break
            case .failure(let key, let why):
                errors.append(why)
                if let kept = last[key] { networks.append(kept) }
            }
        }
        // Shares a contract call brought that the explorer doesn't list yet (the Mac's missingDeposits).
        if !me.evm.isEmpty { networks = await withMissingDeposits(networks, owner: me.evm) }
        guard same() else { return }
        // Solana, then Hyperliquid, then the rest by their money, as on the Mac.
        let order = ["solana": 0, "hyperliquid": 1]
        networks.sort { (order[$0.key] ?? 2, -$0.usd) < (order[$1.key] ?? 2, -$1.usd) }
        // A read that failed keeps the last one: read as it came, the balance would drop by Automation's money.
        if automationWallet == nil { automationUSD = nil } else if let m = automation { automationUSD = m.dollars + m.sol * (markets["SOL"]?.markPx ?? 0) }
        // Another wallet since the last read: its balances aren't money that arrived.
        let owner = me.solana + me.evm
        // The screen stays «loading» under this wallet's last full balance until Solana (priced), Hyperliquid and the
        // Automation wallet have each answered since it came on screen — never the sum of what came so far (the Mac's
        // balanceReady). Only the screen: the checks and the chat read each read as it came, as the Mac's do.
        if portfolioOwner != owner { seenSolanaPriced = nil; seenHL = false }
        if case .success = solanaPart { seenSolanaPriced = !unpriced }
        if case .success = hlPart { seenHL = true }
        var next = PortfolioView(networks: networks, total: networks.reduce(0) { $0 + $1.usd }, updatedAt: Date(), errors: errors)
        next.ready = Self.balanceReady(solana: !me.solana.isEmpty, evm: !me.evm.isEmpty, solanaPriced: seenSolanaPriced, hl: seenHL,
                                       automation: automationWallet == nil || automationUSD != nil)
        let sameWallet = portfolioOwner == owner
        if portfolio.updatedAt != nil, portfolioOwner == owner { announceIncoming(from: portfolio, to: next) }
        portfolio = withProfit(next)
        portfolioOwner = owner
        Task { await loadCosts() }
        // The history and the 7-day line measure what the P/L does, the gas coins left out (the Mac's equity).
        if errors.isEmpty { recordEquity(ruleEquity ?? 0, wallet: me.solana.isEmpty ? me.evm : me.solana) }
        // The whole balance, gas coins and Automation's money in, for the next launch or switch (the Mac's rememberBalance).
        if next.ready { Self.rememberBalance(next.total + (automationUSD ?? 0), wallet: Self.balanceKey(me)) }
        loadEquity()
        if !me.evm.isEmpty {
            // A request that failed keeps what was read before: read as «none», a position would look closed by its stop.
            async let ordersRead = try? HyperliquidAccount.openOrders(me.evm)
            let before = positions.map(\.coin)
            let read = try? await HyperliquidAccount.positions(me.evm), orders = await ordersRead
            guard same() else { return }
            if let read { positions = read.filter { $0.size != 0 }; positionsFor = me.evm.lowercased() }
            hlOrders = orders ?? hlOrders
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
        if n.key == "robinhood" {
            // Robinhood's own registry tells a stock token from any other; read once, the rows marked from the next read on.
            if let stocks = Self.stockRegistry.map({ Set($0.map { $0.address.lowercased() }) }) {
                // A stock's day off its Hyperliquid market (xyz:TSLA), as the Mac's stockRow; none there, none shown.
                n.rows = n.rows.map { r in
                    var r = r
                    if let c = r.contract?.lowercased(), stocks.contains(c) { r.stock = true; r.change24h = Self.stockDay(r.symbol, markets) }
                    return r
                }
            } else { Task { _ = await robinhoodStock("") } }
        }
        // A deposit's «earning» in the window's language; a vault's own name and rate stay as they are.
        if russian { n.rows = n.rows.map { r in var r = r; if let note = r.note, note.hasPrefix("earning") { r.note = "приносит доход" + note.dropFirst("earning".count) }; return r } }
        return n
    }

    nonisolated static func stockDay(_ symbol: String, _ markets: [String: Market]) -> Decimal? {
        (markets["xyz:\(symbol)"] ?? markets["xyz:\(symbol.uppercased())"])?.change24h
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
    /// Until this wallet's first full read: its last full balance — every network, gas coins and the Automation wallet —
    /// shown dimmed (the Mac's lastBalance, kept in balances.json).
    var lastFullBalance: Decimal? {
        guard !portfolio.ready, let me = readWallet else { return nil }
        return Store.load([String: Decimal].self, "balances.json")?[Self.balanceKey(me)]
    }

    /// The address a wallet's balance and equity points are kept under: Solana's, else the EVM one.
    nonisolated static func balanceKey(_ me: TradeWallet) -> String { me.solana.isEmpty ? me.evm : me.solana }

    nonisolated static func rememberBalance(_ usd: Decimal, wallet: String) {
        var all = Store.load([String: Decimal].self, "balances.json") ?? [:]
        guard all[wallet] != usd else { return }
        all[wallet] = usd
        Store.save(all, "balances.json")
    }

    /// The balance on screen is whole (the Mac's balanceReady): Solana has answered priced and Hyperliquid has answered
    /// since the wallet came on screen (each kept through a later read that failed), and the Automation wallet is read.
    nonisolated static func balanceReady(solana: Bool, evm: Bool, solanaPriced: Bool?, hl: Bool, automation: Bool) -> Bool {
        (!solana || solanaPriced == true) && (!evm || hl) && automation
    }

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
        // A dollar has no day's move to show, and no P/L to keep; it keeps its contract, so USDT, USDS, USDe and DAI open
        // their card and trade from their row as any token (the Mac's opener).
        return .init(symbol: symbol, amount: Fmt.num(amount, maxFraction: amount < 1 ? 6 : 4), value: amount, usd: usd, icon: icon?.absoluteString,
                     key: key, contract: contract, change24h: key == nil ? nil : change24h, verified: verified)
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

    /// USDC in one Aave market or Morpho vault: «earning 4.12%» under its name (the Mac's NetworkLine), its last reading
    /// when the node doesn't answer, nil when nothing's lent. Off Base a click opens its site, as the Mac's; Base's has
    /// its details board. The rate is read again at most every 10 minutes.
    nonisolated static func lent(_ name: String, place: String, owner: String, onBase: Bool, read: () async -> UInt64?,
                                 apy: () async -> Decimal?, now: Date = Date()) async -> PortfolioView.Row? {
        let key = owner.lowercased() + "|" + place
        let last = LendingReadings.shared.get(key)
        var reading: LendingReadings.Reading?
        if let raw = await read() {
            let usd = Decimal(raw) / 1_000_000
            if raw >= 10_000 {
                let fresh = last.map { now.timeIntervalSince($0.apyAt) < 600 && $0.apy != nil } ?? false
                reading = .init(usd: usd, apy: fresh ? last?.apy : await apy(), apyAt: fresh ? last!.apyAt : now)
            }
            LendingReadings.shared.set(key, reading)
        } else {
            reading = last
        }
        guard let reading else { return nil }
        return lentRow(name, reading, onBase: onBase)
    }

    nonisolated static func lentRow(_ name: String, _ r: LendingReadings.Reading, onBase: Bool) -> PortfolioView.Row {
        var row = deposit(name, usd: r.usd, note: "earning" + (r.apy.map { " \(Fmt.num($0.rounded(2, .plain)))%" } ?? ""))
        if !onBase { row.site = name == "Aave" ? "app.aave.com" : "app.morpho.org" }
        return row
    }

    /// Robinhood's Stock Tokens as the Mac's loadRobinhood: the ones bought here (remembered by symbol) and any the explorer
    /// lists that Robinhood's registry knows, each read at Robinhood's bid; the explorer's copy is replaced, and a registry
    /// token is never tucked as an unknown airdrop. Without the registry the rows stay as the explorer gave them.
    nonisolated static func stocks(_ rows: [PortfolioView.Row], owner: String, rpc: EVMRPC) async -> [PortfolioView.Row] {
        let registry: [RobinhoodStocks.Token]
        if let known = Engine.stockRegistry { registry = known } else if let read = try? await RobinhoodStocks.registry() { registry = read } else { return rows }
        let bought = Set(boughtStocks())
        var out: [PortfolioView.Row] = []
        var seen = Set<String>()
        for r in rows {
            guard let c = r.contract?.lowercased(), let t = registry.first(where: { $0.address.lowercased() == c }) else { out.append(r); continue }
            seen.insert(c)
            let bid = (try? await RobinhoodStocks.quote(t.symbol))?.bid
            out.append(stockRow(t, amount: r.value, bid: bid, explorer: r))
        }
        for t in registry where bought.contains(t.symbol.uppercased()) && !seen.contains(t.address.lowercased()) {
            guard let raw = try? await rpc.read(t.address, try ERC20.balanceOf(owner)), let units = ABI.decimal(in: raw, word: 0), units > 0 else { continue }
            let amount = units / Decimal(sign: .plus, exponent: t.decimals, significand: 1)
            out.append(stockRow(t, amount: amount, bid: (try? await RobinhoodStocks.quote(t.symbol))?.bid, explorer: nil))
        }
        return out
    }

    /// A Stock Token's row: its symbol and Robinhood's logo, valued at the bid (the explorer's price when Robinhood gives
    /// none); kept on show, as the registry vouches for it.
    nonisolated static func stockRow(_ t: RobinhoodStocks.Token, amount: Decimal, bid: Decimal?, explorer: PortfolioView.Row?) -> PortfolioView.Row {
        var r = row(t.symbol, amount, usd: bid.map { $0 * amount } ?? explorer?.usd, icon: explorer?.icon.flatMap(URL.init(string:)) ?? t.logo,
                    chain: "robinhood", contract: t.address, change24h: explorer?.change24h, verified: true)
        r.id = explorer?.id
        r.real = true
        r.stock = true
        return r
    }

    /// The Stock Tokens bought through ihatecrypto, by symbol (the Mac's «stockTokens»): their balances are read with
    /// every refresh, listed by the explorer or not yet.
    nonisolated static func boughtStocks() -> [String] { Store.load([String].self, "stock-tokens.json") ?? [] }
    nonisolated static func rememberStock(_ symbol: String) {
        let s = symbol.uppercased()
        guard !s.hasPrefix("0X") else { return }
        var list = boughtStocks()
        if !list.contains(s) { list.append(s); Store.save(list, "stock-tokens.json") }
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
            // In dollars: a platform's position is no USDC (the Mac's line shows Fmt.usd of it).
            row.amount = Fmt.usd(Decimal(value).rounded(2, .plain))
            return row
        }
    }

    /// `last`: the last read's tokens, which price a read Jupiter didn't answer. With the read: its tokens (for the next
    /// one) and whether any is still unpriced — a coin new since the last read.
    nonisolated static func solana(_ owner: String, last: [Holding] = []) async -> (Read, [Holding], Bool) {
        do {
            async let lendRead = try? JupiterLend.positions(user: owner)
            async let kaminoRead = try? Kamino.positions(wallet: owner)
            async let vaultsRead = try? Kamino.vaultPositions(wallet: owner)
            async let jupiterRead = jupiterPlatforms(owner)
            let b = try await Solana.balance(owner: owner).pricing(from: last)
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
                // In its own coin and managed on its site, as the Mac's ProtocolPosition: «0.5 SOL», ≈ its dollars, kamino.com.
                var r = deposit("Kamino", usd: v.valueUSD, note: "\(v.name) · \(Fmt.num(v.apy.rounded(2, .plain)))%")
                r.amount = "\(Fmt.num(v.tokens)) \(v.symbol)"
                r.site = "kamino.com"
                rows.append(r)
            }
            // Jupiter's other platforms (staked JUP, perps, predictions…), so a deposit made on its site still shows.
            rows += ((await jupiterRead) ?? []).filter { ($0.usd ?? 0) >= cent }
            return (.success(.init(key: "solana", name: "Solana", usd: 0, rows: rows)), b.tokens, b.unpriced)
        } catch {
            return (.failure("solana", "Solana: \(error.localizedDescription)"), [], false)
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
                        // USDC lent on Aave or in Morpho's vault here, with its rate; a market that doesn't answer keeps its
                        // last reading (the Mac's loadLending), so the total and the curve don't dip.
                        for m in AaveV3.markets where m.chain == net.key {
                            if let r = await lent("Aave", place: m.place, owner: owner, onBase: net.key == "base",
                                                  read: { try? await m.rpc.tokenBalance(m.aUSDC, owner: owner) }, apy: { try? await AaveV3.verify(m) }) { rows.append(r) }
                        }
                        for v in MorphoVault.vaults where v.chain == net.key {
                            if let r = await lent("Morpho", place: v.place, owner: owner, onBase: net.key == "base",
                                                  read: { try? await MorphoVault.value(owner, v) }, apy: { await MorphoRates.netAPY(v) }) { rows.append(r) }
                        }
                        if net.key == "robinhood" { rows = await stocks(rows, owner: owner, rpc: net.rpc) }
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
            // Keyed «near:native» as the Mac's, so it has its P/L and isn't counted with the idle dollars.
            var rows = total > 0 ? [row("NEAR", total, chain: "near", contract: "native")] : []
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

/// Each Aave market's and Morpho vault's last reading, by owner and place, for the portfolio's reads that run side by side.
nonisolated final class LendingReadings: @unchecked Sendable {
    struct Reading: Equatable { var usd: Decimal; var apy: Decimal?; var apyAt: Date }
    static let shared = LendingReadings()
    private let lock = NSLock()
    private var all: [String: Reading] = [:]
    func get(_ key: String) -> Reading? { lock.lock(); defer { lock.unlock() }; return all[key] }
    func set(_ key: String, _ r: Reading?) { lock.lock(); all[key] = r; lock.unlock() }
}

import EVMChain
import ExecutionCore
import IhatecryptoCore
import OneClickKit
import RelayBridge
import SolanaChain
import SwiftUI
import TradeExecution

/// The portfolio by network (the user's pick of ten, P01 on the design canvas): each network is one card holding
/// everything on it — idle money, deposits, gas, tokens, positions. Solana, Hyperliquid and Robinhood Chain lead;
/// the others follow by money, and those with $100 or less fold under «Other networks» (PortfolioSplit.networkCards).
struct NetworkLine: Identifiable {
    enum Kind { case earning, idle, gas, held }
    var id: String { title + detail + (coin ?? "") }
    let kind: Kind
    let logo: String
    let title: String
    let detail: String
    let usd: Decimal
    /// Shown as USDC (its logo and the amount) rather than in dollars.
    var usdc = false
    /// The amount in its own coin when that says more than dollars («0.0766 NEAR»), with the dollars under it.
    var amount: String? = nil
    /// The details a click opens (Openable).
    var place: String? = nil
    /// Without details here: the site a click opens beside the chat, where it's managed.
    var site: String? = nil
    /// The coin's contract, for its board and chart (idle USDC, USDG); nil for a network's own coin.
    var contract: String? = nil
    var help: String? = nil
    /// A deposit's coin, the one put in («USDC», «SOL»); % a year; dollars earned so far, where the protocol says.
    var coin: String? = nil
    var apy: Decimal? = nil
    var earned: Decimal? = nil
    /// A deposit's own numbers under its dollars, in place of the interest (a loan: collateral, borrowed, health).
    var facts: [(value: String, label: String)]? = nil
}

/// A Jupiter Lend deposit: the coin put in, how much of it, its decimals and % a year; `address` is the vault's token.
struct LendDeposit: Equatable {
    let address: String
    let coin: String
    let amount: Decimal
    let decimals: Int
    let apy: Decimal?
}

/// A Kamino Lend deposit: its coin, what it's worth and % a year.
struct KaminoDeposit: Equatable {
    let coin: String
    let usd: Decimal
    let apy: Decimal?
}

extension Workspace {
    /// What each network holds in dollars: its places, its gas, the tokens on show there, Hyperliquid's account and
    /// Robinhood's stock tokens. Adds up to `total`.
    var networkUSD: [ModuleNetwork: Decimal] {
        var out: [ModuleNetwork: Decimal] = [:]
        for n in ModuleNetwork.allCases { out[n] = networkLines(n).reduce(0) { $0 + $1.usd } }
        for t in shownTokens { if let n = ModuleNetwork(rawValue: t.chain) { out[n, default: 0] += t.usd ?? 0 } }
        out[.hyperliquid, default: 0] += hlTotal ?? 0
        out[.robinhood, default: 0] += (robinhood?.stocks ?? []).reduce(0) { $0 + $1.usd }
        return out
    }

    /// A network's places beyond its tokens and stocks, the most money first.
    func networkLines(_ n: ModuleNetwork) -> [NetworkLine] {
        let cent = Decimal(string: "0.01")!
        func idle(_ amount: Decimal?, _ symbol: String = "USDC", contract: String) -> [NetworkLine] {
            guard let amount, amount >= cent else { return [] }
            return [NetworkLine(kind: .idle, logo: symbol.lowercased(), title: symbol, detail: Fmt.num(amount), usd: amount, contract: contract)]
        }
        func gas(_ coin: Decimal?, _ symbol: String) -> [NetworkLine] {
            guard let coin, coin > 0 else { return [] }
            let price = markets[symbol]?.markPx
            return [NetworkLine(kind: .gas, logo: symbol.lowercased(), title: symbol, detail: Fmt.num(coin, maxFraction: 6), usd: coin * (price ?? 0),
                                amount: price == nil ? "\(Fmt.num(coin, maxFraction: 6)) \(symbol)" : nil, help: "\(symbol) here pays the network fees on \(n.name)")]
        }
        // Every deposit the same way, with the coin that went in (the owner: «если ты usdc закинул, то чтобы он его и показывал»).
        func earning(_ logo: String, _ title: String, apy: Decimal?, usd: Decimal, coin: String = "USDC", amount: Decimal? = nil,
                     earned: Decimal? = nil, place: String? = nil, help: String? = nil) -> NetworkLine {
            NetworkLine(kind: .earning, logo: logo, title: title, detail: "earning" + (apy.map { " \(Fmt.num($0.rounded(2, .plain)))%" } ?? ""),
                        usd: usd, usdc: coin == "USDC", amount: Self.isDollar(coin) ? nil : amount.map { "\(Fmt.num($0, maxFraction: 6)) \(coin)" },
                        place: place, help: help, coin: coin, apy: apy, earned: earned)
        }
        // Aave and Morpho off Base.
        let elsewhere = lendingElsewhere.filter { ModuleNetwork(place: $0.network) == n }.map { p in
            let protocolName = p.place.hasPrefix("Morpho") ? "Morpho" : "Aave"
            var line = earning(protocolName.lowercased(), protocolName, apy: p.apy, usd: p.usdc,
                               help: "Say “withdraw USDC from \(protocolName) on \(p.network)” to bring it back to Solana")
            line.site = protocolName == "Aave" ? "app.aave.com" : "app.morpho.org"
            return line
        }
        var lines: [NetworkLine]
        switch n {
        case .solana:
            lines = idle(solana?.usdc, contract: SpotAsset.usdc.mint) + gas(solana?.sol, "SOL")
            lines += lendDeposits.map { d in
                let price = coinPrice(d.coin) ?? 0
                return earning("jup", "Jupiter Lend", apy: d.apy, usd: d.amount * price, coin: d.coin, amount: d.amount,
                               earned: lendEarned[d.address].map { $0 / pow(10, d.decimals) * price }, place: "jupiter")
            }
            lines += kaminoDeposits.map { earning("kamino", "Kamino Lend", apy: $0.apy, usd: $0.usd, coin: $0.coin, place: "kamino") }
            // Backpack's stocks put up on Kamino stay on the card, worth the collateral less the debt.
            if let loan = kaminoLoan, loan.depositUSD > 0 || loan.debtUSD > 0 {
                let main = loan.collateral.max { $0.valueUSD < $1.valueUSD }
                let amounts = loan.collateral.filter { $0.tokens > 0 }.map { "\(Fmt.num($0.tokens, maxFraction: 6)) \($0.symbol)" }.joined(separator: " · ")
                let debt = loan.debts.first { $0.symbol == "USDC" }?.tokens ?? 0
                lines.append(NetworkLine(kind: .earning, logo: "kamino", title: "Kamino Backpack", detail: "collateral", usd: loan.depositUSD - loan.debtUSD,
                                         amount: amounts.isEmpty ? nil : amounts, place: "kamino",
                                         help: "\(amounts) put up on Kamino's Backpack market, \(Fmt.num(debt, maxFraction: 6)) USDC borrowed against it"
                                            + (loan.health.map { ", health factor \(Fmt.num($0, maxFraction: 2)) (liquidation at 1)" } ?? "")
                                            + ". «repay all USDC on kamino» pays it back; «withdraw \(main?.symbol ?? "SPCX") from kamino» takes the stock back.",
                                         coin: main?.symbol ?? "USDC",
                                         facts: [(Fmt.usd(loan.depositUSD), "collateral"), ("−" + Fmt.usd(loan.debtUSD), "borrowed")]
                                            + (loan.health.map { [(Fmt.num($0, maxFraction: 2), "health")] } ?? [])))
            }
            // Deposits made on a site: Kamino's vaults, Jupiter's other platforms.
            lines += protocolPositions.map { p in
                let coin = p.symbol ?? p.amount?.split(separator: " ").last.map(String.init) ?? p.name
                var line = earning(p.logo, p.platform, apy: p.apy, usd: p.usd, coin: coin, amount: p.tokens,
                                   help: "\(p.name) · managed on \(p.site): a click opens it beside the chat")
                if line.amount == nil, !Self.isDollar(coin) { line.amount = p.amount }
                line.site = p.site
                return line
            }
        case .base:
            lines = idle(base?.usdc, contract: EVMRPC.baseUSDC) + gas(base?.eth, "ETH")
            if let b = base, b.morpho > 0 { lines.append(earning("morpho", "Morpho", apy: morphoAPY, usd: b.morpho, place: "morpho")) }
            if let b = base, b.aave > 0 { lines.append(earning("aave", "Aave", apy: aaveAPY, usd: b.aave, place: "aave")) }
        case .arbitrum:
            lines = idle(arbitrumUSDC, contract: EVMRPC.arbitrumUSDC) + elsewhere
        case .robinhood:
            lines = idle(robinhood?.usdg, "USDG", contract: EVMRPC.robinhoodUSDG) + gas(robinhood?.eth, "ETH")
        case .near:
            lines = near > 0 ? [NetworkLine(kind: .held, logo: "near", title: "NEAR", detail: "\(Fmt.num(near, maxFraction: 4)) · native", usd: nearUSD, amount: "\(Fmt.num(near, maxFraction: 4)) NEAR")] : []
        case .ethereum, .polygon, .optimism:
            lines = elsewhere
        // Their USDC and coins are tokens on the card (Holdings.tempo, Holdings.hyperevm, Holdings.bnb).
        case .hyperliquid, .tempo, .hyperevm, .bnb:
            lines = []
        }
        return lines.sorted { $0.usd > $1.usd }
    }

    /// A dollar stablecoin: its deposit needs no amount beside the dollars.
    static func isDollar(_ coin: String) -> Bool {
        (Stablecoins.top.map { $0.uppercased() } + ["USDG", "JUPUSD", "USDT0", "PYUSD"]).contains(coin.uppercased())
    }

    /// A coin's price in dollars: 1 for a dollar stablecoin, else Hyperliquid's market or the token's own price here.
    func coinPrice(_ coin: String) -> Decimal? {
        if Self.isDollar(coin) { return 1 }
        return markets[coin.uppercased()]?.markPx ?? holdings.first { $0.symbol.uppercased() == coin.uppercased() }?.price
    }
}

/// One coin on a network, whatever kind: idle money or gas (a place line), a Robinhood stock token, any other token.
enum NetworkCoin: Identifiable {
    case line(NetworkLine), stock(StockHolding), token(Holding)
    var id: String {
        switch self {
        case .line(let l): "line:" + l.id
        case .stock(let s): "stock:" + s.id
        case .token(let t): "token:" + t.id
        }
    }
    var usd: Decimal {
        switch self {
        case .line(let l): l.usd
        case .stock(let s): s.usd
        case .token(let t): t.usd ?? 0
        }
    }
}

extension Workspace {
    /// Every coin on the network in one list, the most money first (the user: «от большего к меньшему»): its idle money
    /// and gas among `lines`, Robinhood's stock tokens, the tokens on show. Deposits aren't coins.
    func networkCoins(_ n: ModuleNetwork, lines: [NetworkLine]) -> [NetworkCoin] {
        (lines.filter { $0.kind != .earning }.map(NetworkCoin.line)
         + (n == .robinhood ? robinhood?.stocks ?? [] : []).map(NetworkCoin.stock)
         + shownTokens.filter { $0.chain == n.rawValue }.map(NetworkCoin.token)).sorted { $0.usd > $1.usd }
    }

    /// What a click on a network's line opens, the same on its card and on its board (the user: everything on them is
    /// clickable): a deposit's board or its site beside the chat; a coin's board and chart.
    func opener(_ line: NetworkLine, chain: String) -> (() -> Void)? {
        if let place = line.place { return { self.toggleDetail(place) } }
        if let site = line.site { return { self.openSite(site) } }
        return line.kind == .earning ? nil : opener(chain: chain, contract: line.contract ?? "native", symbol: line.title)
    }
}

/// The network cards in the portfolio frame, in their order, with the small networks folded.
struct NetworkCards: View {
    @Environment(Workspace.self) private var ws
    @State private var unfolded = false

    var body: some View {
        let held = ws.networkUSD
        let present = Set([ws.solana != nil ? ModuleNetwork.solana : nil, ws.hl != nil ? .hyperliquid : nil].compactMap { $0 })
        let order = PortfolioSplit.networkCards(held, present: present)
        VStack(spacing: 10) {
            if !ws.inTransit.isEmpty { InTransit(entries: ws.inTransit) }
            ForEach(order.cards, id: \.self) { NetworkCard(network: $0, usd: held[$0] ?? 0) }
            ForEach(ws.multisigs) { MultisigPortfolioCard(multisig: $0) }
            if !order.folded.isEmpty {
                Button { withAnimation(.snappy) { unfolded.toggle() } } label: {
                    HStack(spacing: 10) {
                        // The name on its own line, the networks as logos under it: beside four logos in the narrow
                        // frame the name broke in two and the list of names ended in «Arbitrum ·…».
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Other networks").font(.ui(12, .semibold)).lineLimit(1).fixedSize()
                            // Side by side, each on its disc: overlapped, Tempo's «T» and HyperEVM's mark ran together.
                            // Five at most, or the row outgrew the name and cut it to «Other ne…».
                            HStack(spacing: 3) {
                                ForEach(order.folded.prefix(5), id: \.self) { TokenLogo(name: $0.logo, size: 14, round: true) }
                                if order.folded.count > 5 {
                                    Text("+\(order.folded.count - 5)").font(.ui(10, .semibold)).foregroundStyle(.secondary).fixedSize()
                                }
                            }
                        }
                        .layoutPriority(1)
                        Spacer(minLength: 4)
                        Text(masked(Fmt.usd(order.folded.reduce(0) { $0 + (held[$1] ?? 0) }))).font(.ui(12, .bold)).monospacedDigit().fixedSize()
                        Image(systemName: unfolded ? "chevron.down" : "chevron.right").font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
                    }
                    .padding(.horizontal, 11).padding(.vertical, 9)
                    .background(RoundedRectangle(cornerRadius: 16).strokeBorder(Brand.cardBorder))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .modifier(HoverLift(radius: 16))
                .help(unfolded ? "Fold them again" : order.folded.map(\.name).joined(separator: " · ") + ": $100 or less each, open to see what's on them")
                if unfolded {
                    ForEach(order.folded, id: \.self) { NetworkCard(network: $0, usd: held[$0] ?? 0) }.transition(.opacity)
                }
            }
            AutomationPortfolioCard()
        }
        // Every held token's cost from the chain, for the P/L on its row; read again when an amount changes.
        .task(id: ws.holdings.map(\.amount) + (ws.robinhood?.stocks ?? []).map(\.amount) + [ws.near, ws.solana?.sol ?? 0, ws.base?.eth ?? 0, ws.robinhood?.eth ?? 0]) {
            await ws.loadChainCosts()
        }
    }
}

/// One network: its logo, name, what its money does, its total; then everything on it.
struct NetworkCard: View {
    @Environment(Workspace.self) private var ws
    @Environment(\.colorScheme) private var scheme
    let network: ModuleNetwork
    let usd: Decimal

    /// The networks whose details board exists (PlaceDetailBoard).
    static let opens: Set<ModuleNetwork> = [.solana, .hyperliquid, .robinhood, .base, .arbitrum, .near]

    /// The card's fill, the line under its name, and its color on the ring.
    static func style(_ n: ModuleNetwork) -> (tint: Color, ink: Color, dot: Color) {
        switch n {
        case .solana: (Brand.solanaTint, Brand.solanaInk, Color(hex: 0x9945FF))
        case .hyperliquid: (Brand.hl, Brand.hlMint, Color(hex: 0x2FB39A))
        case .robinhood: (Color(light: 0xF3FFD6, dark: 0x1D2608), Color(light: 0x5A7A00, dark: 0xA8CC3C), Color(light: 0x7FA600, dark: 0xB5D94A))
        case .base: (Color(light: 0xEEF3FF, dark: 0x151C2E), Color(light: 0x0052FF, dark: 0x7EA2FF), Color(hex: 0x0052FF))
        case .arbitrum: (Color(light: 0xEAF5FF, dark: 0x12243A), Color(light: 0x1A6FB0, dark: 0x7CC8FF), Color(hex: 0x28A0F0))
        case .ethereum: (Color(light: 0xEEF0FB, dark: 0x1A1D33), Color(light: 0x4C5FD5, dark: 0xA5B4FC), Color(hex: 0x627EEA))
        case .polygon: (Color(light: 0xF3EDFF, dark: 0x221A36), Color(light: 0x6B32C9, dark: 0xC4A7FF), Color(hex: 0x8247E5))
        case .near: (Color(light: 0xE6FBF3, dark: 0x0E2A20), Color(light: 0x05785A, dark: 0x00EC97), Color(hex: 0x00C08B))
        case .tempo: (Color(light: 0xF1F2F5, dark: 0x22252C), Color(light: 0x5F6470, dark: 0xC5CAD3), Color(hex: 0x8A8F98))
        case .hyperevm: (Color(light: 0xE9F6F2, dark: 0x10261F), Color(light: 0x0E4D3F, dark: 0x8FE3CC), Color(light: 0x0E4D3F, dark: 0x5FD3B7))
        case .optimism: (Color(light: 0xFFEDEE, dark: 0x2E1416), Color(light: 0xC2020F, dark: 0xFF8A92), Color(hex: 0xFF0420))
        case .bnb: (Color(light: 0xFFF8E1, dark: 0x2B2410), Color(light: 0x8A6500, dark: 0xF3CD5A), Color(hex: 0xF0B90B))
        }
    }

    var body: some View {
        let lines = ws.networkLines(network), style = Self.style(network)
        let deposits = lines.filter { $0.kind == .earning }, coins = ws.networkCoins(network, lines: lines)
        let tokens = !coins.isEmpty || ws.tuckedTokens.contains { $0.chain == network.rawValue }
        let card = VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                TokenLogo(name: network.logo, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    // «Robinhood Chain» beside its amount takes two lines rather than ending in «…».
                    Text(network.name).font(.ui(14, .semibold)).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                    // Wraps rather than ends in «…» next to a wide amount.
                    if let line = subtitle(lines, tokens: tokens) {
                        Text(line).font(.ui(10)).foregroundStyle(style.ink).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 4)
                Text(masked(Fmt.usd(usd))).font(.ui(13, .bold)).monospacedDigit().lineLimit(1).fixedSize()
            }
            if network == .hyperliquid { positions }
            // Coins first, deposits under their own name (the user: «непонятно разделение дефай и токены»).
            if tokens, !deposits.isEmpty { SectionName(title: "Tokens") }
            ForEach(coins) { c in
                switch c {
                case .line(let l): NetworkLineRow(line: l, chain: network.rawValue)
                case .stock(let s): stockRow(s)
                case .token(let t): HoldingRow(token: t, shown: true)
                }
            }
            HiddenTokens(chain: network.rawValue)
            if !deposits.isEmpty {
                if tokens { Rectangle().fill(.primary.opacity(0.1)).frame(height: 1).padding(.top, 2) }
                SectionName(title: "DeFi")
                ForEach(deposits) { NetworkLineRow(line: $0, chain: network.rawValue) }
            }
        }
        .padding(11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).fill(style.tint))
        // Hyperliquid's card is dark green in both modes: its text and rows read as on a dark page.
        .environment(\.colorScheme, network == .hyperliquid ? .dark : scheme)
        if Self.opens.contains(network) { card.modifier(Openable(place: network.rawValue)) } else { card }
    }

    /// What the network's money does («idle $2,532.82 · earning $150.10»), or what it is; nil when its rows say it all.
    private func subtitle(_ lines: [NetworkLine], tokens: Bool) -> String? {
        switch network {
        case .hyperliquid:
            return ws.hlMargin.map { masked("\(Fmt.usd($0)) · margin" + (ws.hlp.equity > 0 ? " · HLP \(Fmt.usd(ws.hlp.equity))" : "")) } ?? "no address"
        case .tempo:
            return "USDC for paid services"
        default:
            // Idle is every dollar not earning: USDC's line and the other big stablecoins held there (USDT, USDe…).
            let stables = ws.shownTokens.filter { $0.chain == network.rawValue && Stablecoins.isGenuine($0.contract, on: $0.chain) }.reduce(Decimal(0)) { $0 + ($1.usd ?? 0) }
            let sums = [("earning", NetworkLine.Kind.earning), ("idle", .idle)]
                .map { name, kind in (name, lines.filter { $0.kind == kind }.reduce(Decimal(0)) { $0 + $1.usd } + (kind == .idle ? stables : 0)) }
                .filter { $0.1 >= Decimal(string: "0.01")! }.sorted { $0.1 > $1.1 }
            // No count of tokens: the rows under it show them (the user: «убери слово 5 tokens»).
            if !sums.isEmpty { return sums.map { "\($0.0) \(masked(Fmt.usd($0.1)))" }.joined(separator: " · ") }
            return tokens ? nil : "nothing here yet"
        }
    }

    /// Hyperliquid's open positions (a click opens the chart) and a limit entry waiting on the book.
    @ViewBuilder private var positions: some View {
        ForEach(ws.positions, id: \.coin) { p in
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("\(p.size > 0 ? "Long" : "Short") \(IntentParser.pretty(p.coin))").font(.ui(13, .semibold)).lineLimit(1)
                    Spacer()
                    Text(masked(signed(p.unrealizedPnl))).font(.ui(13, .bold)).monospacedDigit()
                        .foregroundStyle(p.unrealizedPnl < 0 ? Color(hex: 0xFF9C8F) : Brand.hlMint)
                }
                Text(ws.positionSummary(p)).font(.ui(10)).foregroundStyle(Brand.hlSoft).lineLimit(2)
                if let rules = ws.exitRules[p.coin] {
                    Label(rules.duties.joined(separator: " "), systemImage: "shield.lefthalf.filled")
                        .font(.ui(10)).foregroundStyle(Brand.hlMint).lineLimit(2)
                }
            }
            .padding(9)
            .background(RoundedRectangle(cornerRadius: 12).fill(Brand.hlChip))
            .contentShape(Rectangle())
            .modifier(HoverLift(radius: 12))
            .onTapGesture { Task { await ws.chartCommand(.chart(coin: p.coin, range: .day)) } }
            .help("Open the \(IntentParser.pretty(p.coin)) chart")
        }
        if let o = ws.pendingEntry {
            Text("Limit waiting: \(o.side == "B" ? "long" : "short") \(IntentParser.pretty(o.coin)) at \(o.limitPx.map(Fmt.price) ?? "?")")
                .font(.ui(10)).foregroundStyle(Brand.hlSoft)
        }
    }

    /// A Robinhood Chain stock token or token bought by contract; a click opens the token, right-click asks the chat
    /// about it or trades it.
    private func stockRow(_ s: StockHolding) -> some View {
        let stock = TokenBoard.isStock(s.symbol)
        return HStack(spacing: 8) {
                StockLogo(symbol: s.symbol, url: s.logo, size: 20)
                CoinName(symbol: s.symbol, amount: Fmt.num(s.amount, maxFraction: 6))
                Spacer(minLength: 4)
                // A stock's day off its Hyperliquid market (xyz:TSLA); a token bought by contract off its DexScreener pool.
                CoinValue(value: Fmt.usd(s.usd),
                          change: stock ? ws.markets["xyz:\(s.symbol)"]?.change24h : ws.tokenInfo["robinhood:" + s.address.lowercased()]?.change24h)
            }
            .modifier(RowLink(action: { ws.openToken(s.address) },
                              help: "\(s.symbol) at \(s.price.map { Fmt.usd($0) } ?? "—"): details and chart · right-click to ask the chat, buy or sell",
                              selected: ws.detail == TokenBoard.place(chain: "robinhood", address: s.address)))
            .modifier(TokenMenu(chain: "robinhood", address: s.address, symbol: s.symbol, stock: stock, verified: true))
    }
}

/// A row that opens something: the hand cursor and the accent edge a little off the row (the user's rule for anything
/// clickable), then the click. Without an action it's just the row.
struct RowLink: ViewModifier {
    let action: (() -> Void)?
    var help = ""
    /// What the row opens is open now: the accent edge shows then, not on hover.
    var selected = false

    func body(content: Content) -> some View {
        if let action {
            content.padding(.horizontal, 5).padding(.vertical, 3)
                .contentShape(Rectangle())
                .modifier(HoverLift(radius: 8, selected: selected))
                .onTapGesture(perform: action)
                .help(help)
                .padding(.horizontal, -5).padding(.vertical, -3)
        } else {
            content
        }
    }
}

/// A place on a network's card: logo, name, what it does, the money; every one opens something. A deposit opens its
/// details board, or its site beside the chat; idle USDC opens its own board and chart, like any token (the user: «не
/// открывается его график»); a network's own coin (SOL, ETH, NEAR) opens its chart. Right-click asks the chat about a coin.
struct NetworkLineRow: View {
    @Environment(Workspace.self) private var ws
    let line: NetworkLine
    let chain: String

    var body: some View {
        if line.kind == .earning { deposit } else { coin }
    }

    /// Idle money, gas, a held coin: laid out like a token (the amount under the name, the day's move on the right).
    @ViewBuilder private var coin: some View {
        // A network's own coin has its P/L too, gas included (Workspace.loadChainCosts).
        let cost = line.kind == .idle ? nil : ws.cost(ws.coinKey(line.title, chain: chain))
        let held = ws.coinHeld(line.title, chain: chain) ?? 0
        let row = HStack(spacing: 8) {
            TokenLogo(name: line.logo, size: 20)
            CoinName(symbol: line.title, amount: line.detail, pnl: (ws.markets[line.title]?.markPx).flatMap { cost?.pnl(held: held, price: $0) })
            Spacer(minLength: 4)
            // A coin without a price shows its amount; a stablecoin has no day's move to show.
            CoinValue(value: line.usd > 0 ? Fmt.usd(line.usd) : line.amount ?? Fmt.usd(0),
                      change: line.kind == .idle ? nil : ws.markets[line.title.uppercased()]?.change24h)
        }
        .contentShape(Rectangle())
        row.modifier(RowLink(action: ws.opener(line, chain: chain),
                             help: (line.help ?? "\(line.title): \(line.contract == nil ? "chart" : "details and chart")") + " · right-click to ask the chat",
                             selected: ws.isOpen(chain: chain, contract: line.contract ?? "native", symbol: line.title)))
            .modifier(TokenMenu(chain: chain, address: line.contract, symbol: line.title))
    }

    @ViewBuilder private var deposit: some View {
        let tile = DepositTile(line: line).help(line.help ?? "")
        if let place = line.place {
            tile.modifier(Openable(place: place, radius: DepositTile.radius))
        } else {
            tile.modifier(RowLink(action: ws.opener(line, chain: chain), help: line.help ?? ""))
        }
    }
}

/// A deposit that earns, the same for every protocol (the owner picked J04a on the design canvas): the protocol's logo
/// with its name and, under it, the coin that went in; % a year on the right; the dollars; then a day's and a month's
/// interest and what it has earned so far (or a year's, where the protocol doesn't say). Dark in both modes, in the
/// protocol's colors (ProtocolStyle: DefiLlama's top 50), a common dark tile for any other.
struct DepositTile: View {
    let line: NetworkLine
    static let radius: CGFloat = 12

    /// By the line's logo name, its title («Rocket Pool» → «rocketpool») or the title's first word («Kamino Lend»).
    static func style(_ line: NetworkLine) -> (fill: Color, accent: Color) {
        let keys = [line.logo, line.title, String(line.title.split(separator: " ").first ?? "")]
            .map { $0.lowercased().filter { $0.isASCII && ($0.isLetter || $0.isNumber) } }
        let c = keys.lazy.compactMap { ProtocolStyle.colors[$0] }.first ?? ProtocolStyle.common
        return (Color(hex: c.fill), Color(hex: c.accent))
    }

    /// A day's and a month's interest at the rate now, then what it earned (or a year's).
    private var stats: [(value: String, label: String)] {
        func plus(_ usd: Decimal) -> String { "+" + Fmt.usd(usd) }
        if let facts = line.facts { return facts }
        let earned = line.earned.map { (plus($0), "earned so far") }
        guard let apy = line.apy, apy > 0 else { return earned.map { [$0] } ?? [] }
        let year = line.usd * apy / 100
        return [(plus(year / 365), "a day"), (plus(year / 12), "a month"), earned ?? (plus(year), "a year")]
    }

    var body: some View {
        let style = Self.style(line), coin = line.coin ?? line.title
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 7) {
                TokenLogo(name: line.logo, size: 29, round: true)
                VStack(alignment: .leading, spacing: 2.5) {
                    Text(line.title).font(.ui(12, .semibold)).lineLimit(1)
                    HStack(spacing: 3.5) {
                        TokenLogo(name: TokenLogo.name(forAsset: coin) ?? coin.lowercased(), size: 11, round: true)
                        // A coin that isn't a dollar says how much of it went in.
                        Text(masked(line.amount ?? coin)).font(.ui(10, .semibold)).foregroundStyle(Color(hex: 0xC9D3DC)).lineLimit(1)
                    }
                }
                Spacer(minLength: 4)
                if let apy = line.apy, apy > 0 {
                    Text("\(Fmt.num(apy.rounded(2, .plain)))% APY").font(.ui(9, .bold)).foregroundStyle(style.fill).lineLimit(1).fixedSize()
                        .padding(.horizontal, 6).padding(.vertical, 2.5).background(Capsule().fill(style.accent))
                }
            }
            Text(masked(Fmt.usd(line.usd))).font(.ui(18, .heavy)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.6)
            let stats = stats
            if !stats.isEmpty {
                HStack(alignment: .top, spacing: 5) {
                    ForEach(stats, id: \.label) { s in
                        VStack(alignment: .leading, spacing: 1) {
                            Text(masked(s.value)).font(.ui(11, .bold)).foregroundStyle(style.accent).monospacedDigit().lineLimit(1).minimumScaleFactor(0.7)
                            // The app's small text size, not smaller: 7.5 pt read as fine print (the design review).
                            Text(s.label).font(.ui(9)).foregroundStyle(Color(hex: 0x9AA5B1)).lineLimit(1)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: Self.radius).fill(style.fill))
        .environment(\.colorScheme, .dark)
        .foregroundStyle(.white)
    }
}

extension TokenBoard {
    /// The network's own dollar the wallet keeps (USDC, Robinhood's USDG): its board shows the chart and what's held,
    /// with no buy or sale of it.
    static func walletDollar(chain: String, address: String) -> Bool {
        address == SpotAsset.usdc.mint || EVMNetwork.all.contains { $0.key == chain && $0.usdc.lowercased() == address.lowercased() }
    }
}

/// A part of a network's card: «Tokens», «DeFi».
struct SectionName: View {
    let title: String

    var body: some View {
        Text(title.uppercased()).font(.ui(9, .semibold)).tracking(1).foregroundStyle(.secondary)
    }
}

/// A coin's symbol with how much of it is held under it (the user: the amount under the name, the day's move on the
/// right). `verified` nil: no mark (a network's own coin, a stock token).
struct CoinName: View {
    let symbol: String
    let amount: String
    var verified: Bool? = nil
    /// What it made or lost since it was bought (Workspace.purchasePnL), after the amount, in its color.
    var pnl: (usd: Decimal, pct: Decimal)? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 4) {
                Text(symbol).font(.ui(12, .semibold)).lineLimit(1)
                if verified == true { VerifiedBadge(size: 12) }
                if verified == false { Text("unverified").font(.ui(9)).foregroundStyle(.secondary).lineLimit(1) }
            }
            if let pnl {
                let sign = pnl.usd < 0 ? "−" : "+"
                Text("\(Text(masked(amount)).foregroundStyle(.secondary)) \(Text("\(sign)\(Fmt.pct(abs(pnl.pct).rounded(1, .plain)))").foregroundStyle(pnl.usd < 0 ? Brand.loss : Brand.profit))")
                    .font(.ui(9.5)).monospacedDigit().lineLimit(1)
                    .help(masked("\(pnl.usd < 0 ? "Loss" : "Profit") \(Fmt.usd(abs(pnl.usd))) since it came in"))
            } else {
                Text(masked(amount)).font(.ui(9.5)).foregroundStyle(.secondary).monospacedDigit().lineLimit(1)
            }
        }
    }
}

extension Workspace {
    /// What a held token cost: what the app's own buys recorded or what the chain shows (`chainCost`), whichever covers
    /// more of it — a Relay buy pays in one transaction and delivers in another, so only the record knows its cost; a buy
    /// in another wallet app only the chain. Solana's by mint, the others by «network:contract», NEAR by «near:native».
    func cost(_ key: String) -> CostBasis? {
        // The same amount both ways: the chain's, which knows when it was bought.
        [chainCost[key]?.cost, costBasis[key]].compactMap { $0 }.filter { $0.amount > 0 }.max { $0.amount < $1.amount }
            ?? costBasis["seen:" + key].flatMap { $0.amount > 0 ? $0 : nil }
    }

    /// The key a network's own coin's cost is kept under: SOL as wrapped SOL's mint (the app's swaps record it so), a
    /// Hyperliquid spot token by its name (its Holding.id), the others «network:native».
    func coinKey(_ coin: String, chain: String) -> String {
        switch chain {
        case "solana": CostBasis.wsol
        case "hyperliquid": "hyperliquid:\(coin.lowercased())"
        default: "\(chain):native"
        }
    }

    /// The network coins held that the portfolio shows as lines (gas, NEAR) rather than as tokens; NEAR first, its
    /// read is one quick one.
    var coinLines: [(chain: String, coin: String, held: Decimal)] {
        [("near", "NEAR", near), ("solana", "SOL", solana?.sol), ("base", "ETH", base?.eth), ("robinhood", "ETH", robinhood?.eth)]
            .compactMap { chain, coin, held in held.flatMap { $0 > 0 ? (chain, coin, $0) : nil } }
    }

    /// What a held token made or lost since it came in; nil for one with no cost yet.
    func purchasePnL(_ t: Holding) -> (usd: Decimal, pct: Decimal)? {
        guard let price = t.price else { return nil }
        return (cost(t.id) ?? cost(t.contract))?.pnl(held: t.amount, price: price)
    }

    /// What every held coin cost, gas too (the owner, 2026-10-15: «для всех токенов ЛЮБЫХ чтобы был пнл», «ЛЮБОЙ ТОКЕН
    /// ДАЖЕ ГАЗ»), from the chain: Solana's from the wallet's own transactions (SOL from all of them), the EVM networks'
    /// tokens and Robinhood's stocks from Blockscout's transfers and their coins from its balance history, NEAR from
    /// 1Click, NEAR's tokens from NearBlocks; what came in unpaid (or before the history read) at DefiLlama's price that
    /// day. Hyperliquid's spot at the cost it keeps itself. A network with no history (BNB Chain, HyperEVM) counts from
    /// when the app first saw it. Only a coin whose amount changed since it was read; dollars have no P/L to show. One
    /// at a time: the public Solana node throttles bursts.
    func loadChainCosts() async {
        // Not cut short when the view that asked goes or its amounts change (its .task is cancelled then): a cancelled
        // read answered nothing, which counted as no history and kept the coin's P/L from today.
        await Task { await readChainCosts() }.value
    }

    private func readChainCosts() async {
        typealias Wanted = (key: String, chain: String, contract: String, held: Decimal, price: Decimal?)
        var wanted: [Wanted] = coinLines.map { c in
            (coinKey(c.coin, chain: c.chain), c.chain, c.chain == "solana" ? CostBasis.wsol : "native", c.held, markets[c.coin]?.markPx)
        }
        wanted += holdings.filter { $0.amount > 0 && !Stablecoins.isGenuine($0.contract, on: $0.chain) }.map { ($0.id, $0.chain, $0.contract, $0.amount, $0.price) }
        wanted += (robinhood?.stocks ?? []).filter { $0.amount > 0 }.map { ("robinhood:\($0.address.lowercased())", "robinhood", $0.address, $0.amount, $0.price) }
        wanted = wanted.filter { chainCost[$0.key]?.held != $0.held }
        // Taken: a run started meanwhile (the portfolio and a card both ask) leaves these to this one.
        for w in wanted { chainCost[w.key] = (w.held, CostBasis()) }
        let owner = solanaAddress, evm = isEVMAddress(hlAddress) ? hlAddress : nil
        var solPrices: [Hyperliquid.Candle]?, spot: [String: CostBasis]?, nearTokens: [String: [CostBasis.Change]]?
        for w in wanted {
            var changes: [CostBasis.Change]?, basis: CostBasis?
            switch w.chain {
            case "solana":
                guard !owner.isEmpty else { break }
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
                if nearTokens == nil, let account = nearAddress { nearTokens = await Holdings.nearTokenChanges(account: account) }
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
                basis = prices.map { p in CostBasis.fold(changes, held: w.held) { p[$0] } }   // DefiLlama didn't answer: read again
            }
            guard owner == solanaAddress else { return }   // another wallet since: its own read follows
            chainCost[w.key] = basis.map { (w.held, $0) }   // didn't answer: read again next time
            // Nothing tells what it cost (no history to read, or none that priced it): from when the app first saw it.
            let noHistory = Holdings.blockscout[w.chain] == nil && !["solana", "near", "hyperliquid"].contains(w.chain)
            if basis != nil || noHistory, (basis?.amount ?? 0) == 0, (costBasis[w.key]?.amount ?? 0) == 0, let price = w.price {
                seen(w.key, held: w.held, price: price)
            }
        }
    }

    /// Re-reads NEAR's cost after a NEAR swap: its chart's P/L.
    func loadNearCost() async {
        chainCost["near:native"] = nil
        await loadChainCosts()
    }

    /// The NEAR account's changes in NEAR: every 1Click swap in the journal that touched NEAR, as 1Click itself settled
    /// it, oldest first. Buying NEAR never kept a cost, so its chart said «not bought here» for NEAR bought in the app
    /// (the owner, 2026-10-15: «почему не показывает что я купил тут»). NEAR from elsewhere is what was held before.
    func nearCoinChanges() async -> [CostBasis.Change]? {
        guard let account = nearAddress, let entries = try? Journal(url: journalURL).entries() else { return nil }
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

    /// A coin nothing tells the cost of, counted from when the app first saw it: what's new since at the price then,
    /// what's gone taking its share. Kept with the app's records as «seen:<key>».
    func seen(_ key: String, held: Decimal, price: Decimal) {
        var s = costBasis["seen:" + key] ?? CostBasis()
        if held > s.amount { s.came(held - s.amount, worth: (held - s.amount) * price) } else if held < s.amount { s.sold(s.amount - held) }
        if s != costBasis["seen:" + key] { costBasis["seen:" + key] = s }
    }
}

/// Each Solana transaction a wallet's token account took part in, read once: a transaction never changes, so a new read
/// asks the node only for the newer ones.
actor SolanaMoves {
    static let shared = SolanaMoves()
    private var read: [String: CostBasis.SolanaMove] = [:]

    /// The owner's moves in `mint`, oldest first: the 50 newest transactions of its token account (either program's);
    /// for SOL, of the wallet itself.
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

/// A coin's dollars and, under them, how its price moved in 24 hours.
struct CoinValue: View {
    let value: String
    var change: Decimal? = nil

    var body: some View {
        VStack(alignment: .trailing, spacing: 0) {
            Text(masked(value)).font(.ui(12, .bold)).monospacedDigit().lineLimit(1)
            if let change {
                let move = Text("\(change < 0 ? "−" : "+")\(Fmt.pct(abs(change).rounded(1, .plain)))").foregroundStyle(change < 0 ? Brand.loss : Brand.profit)
                Text("\(move) \(Text("24h").foregroundStyle(.secondary))")
                    .font(.ui(9, .semibold)).monospacedDigit().lineLimit(1)
                    .help("How far the price moved in the last 24 hours")
            }
        }
    }
}

/// Money between networks: each bridge order signed and not landed yet (Journal.inTransit), with how long it's been on
/// its way; it leaves the list once its bridge says it settled or went back.
struct InTransit: View {
    @Environment(Workspace.self) private var ws
    let entries: [Journal.Entry]
    /// Rows opened by a click: the whole description, the bridge's own word on it now, and its links.
    @State private var open: Set<String> = []

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label("In transit", systemImage: "arrow.triangle.swap").font(.ui(12, .semibold))
            ForEach(entries, id: \.id) { e in
                VStack(alignment: .leading, spacing: 6) {
                    Button {
                        withAnimation(.easeOut(duration: 0.15)) { if open.contains(e.id) { open.remove(e.id) } else { open.insert(e.id) } }
                    } label: { HStack(spacing: 8) {
                        TokenLogo(name: Self.bridge(e).logo, size: 18)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(Self.what(e.note)).font(.ui(11.5)).lineLimit(open.contains(e.id) ? nil : 1)
                                .fixedSize(horizontal: false, vertical: open.contains(e.id))
                            Text("\(Self.bridge(e).name) · \(Self.receiving(e) ? "waiting for your deposit" : "on its way") \(Self.since(e.at))")
                                .font(.ui(10.5)).foregroundStyle(.secondary).lineLimit(open.contains(e.id) ? nil : 1)
                                .fixedSize(horizontal: false, vertical: open.contains(e.id))
                        }
                        Spacer(minLength: 4)
                        Image(systemName: open.contains(e.id) ? "chevron.up" : "chevron.down").font(.system(size: 9, weight: .semibold)).foregroundStyle(.secondary)
                    }
                    .contentShape(Rectangle()) }
                    .buttonStyle(.plain)
                    .accessibilityLabel("In transit: \(Self.what(e.note))")
                    if open.contains(e.id) { TransitDetails(entry: e) }
                }
                .help("Signed at \(Fmt.date(e.at)); it shows here until the bridge says it landed or went back. Click for details.")
            }
        }
        .padding(.horizontal, 11).padding(.vertical, 9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).strokeBorder(Brand.accent.opacity(0.5)))
    }

    static func bridge(_ e: Journal.Entry) -> (name: String, logo: String) {
        switch e.chain {
        case "1click": ("NEAR Intents", "near")
        case "cctp": ("Circle's CCTP", "usdc")
        case "layerzero": ("LayerZero", "usdt")
        default: ("Relay", "relay")
        }
    }

    /// A deposit address ihatecrypto opened for money coming in from elsewhere (receiveAcross), not a move of its own.
    static func receiving(_ e: Journal.Entry) -> Bool { e.note.hasPrefix("Receive:") }

    /// The journal's note without its transaction: «Base USDC→Solana 5, tx 0x…» → «Base USDC→Solana 5».
    static func what(_ note: String) -> String { note.components(separatedBy: ", tx ").first ?? note }

    static func since(_ at: Date, now: Date = Date()) -> String {
        let minutes = Int(now.timeIntervalSince(at) / 60)
        return minutes < 1 ? "for under a minute" : minutes < 60 ? "for \(minutes) min" : "for \(minutes / 60) h \(minutes % 60) min"
    }
}

/// An in-transit row opened: the bridge's state asked now, when it was signed, the order, its links; for an incoming
/// deposit address, the address itself with Copy and its QR card.
private struct TransitDetails: View {
    @Environment(Workspace.self) private var ws
    let entry: Journal.Entry
    @State private var state = "asking…"
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            line("State", state)
            line("Signed", Fmt.date(entry.at))
            if let tx = Self.transaction(entry) { line("Transaction", "\(tx.prefix(10))…\(tx.suffix(6))") }
            if InTransit.receiving(entry) {
                Text(entry.id).font(.system(size: 10, design: .monospaced)).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 8) {
                    Button(copied ? "Copied" : "Copy address") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(entry.id, forType: .string)
                        copied = true
                    }
                    if ws.acrossDeposit?.address == entry.id {
                        Button("Show QR") { withAnimation(StudioView.spring) { ws.detail = "receive-across" } }
                    }
                }
                .controlSize(.small)
            } else {
                line("Order", "\(entry.id.prefix(12))…")
            }
            HStack(spacing: 10) {
                ForEach(Self.links(entry), id: \.title) { l in Link(l.title, destination: l.url).font(.ui(10.5)) }
            }
        }
        .padding(.leading, 26)
        .task(id: entry.id) { state = await Self.state(entry) }
    }

    private func line(_ label: String, _ value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(label).font(.ui(10)).foregroundStyle(.secondary).frame(width: 70, alignment: .leading)
            Text(value).font(.ui(10.5)).fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The source transaction the note carries («…, tx 0x…» or «…, tx <signature>»).
    static func transaction(_ e: Journal.Entry) -> String? {
        e.note.components(separatedBy: ", tx ").dropFirst().first.map { String($0.prefix { !$0.isWhitespace }) }
    }

    /// The bridge's own word on the order now.
    static func state(_ e: Journal.Entry) async -> String {
        switch e.chain {
        case "1click":
            guard let s = try? await OneClick.status(depositAddress: e.id) else { return "NEAR Intents didn't answer" }
            return Executor.oneClickWords[s.status] ?? s.status.lowercased()
        case "cctp":
            guard let colon = e.id.firstIndex(of: ":"), let source = UInt32(e.id[..<colon]) else { return "unknown" }
            return await Executor.cctpForward(source: source, burn: String(e.id[e.id.index(after: colon)...])).map { "minted by Circle (\($0.prefix(10))…)" }
                ?? "burned, waiting for Circle's mint"
        case "layerzero":
            guard let s = await Executor.layerZeroStatus(tx: e.id) else { return "LayerZero hasn't seen it yet" }
            return ["INFLIGHT": "on its way: waiting for the source network's finality", "CONFIRMING": "verified, being delivered",
                    "DELIVERED": "landed", "FAILED": "failed"][s.status] ?? s.status.lowercased()
        default:
            guard let s = try? await Relay.status(requestId: e.id).status else { return "Relay didn't answer" }
            return ["waiting": "waiting for the deposit", "pending": "deposit seen, filling", "submitted": "fill sent", "success": "landed",
                    "delayed": "delayed — Relay is still on it", "refund": "refunded", "failure": "failed"][s] ?? s
        }
    }

    static func links(_ e: Journal.Entry) -> [(title: String, url: URL)] {
        var out: [(title: String, url: URL)] = []
        switch e.chain {
        case "1click": if let u = URL(string: "https://explorer.near-intents.org/transactions/\(e.id)") { out.append(("NEAR Intents explorer", u)) }
        case "cctp":
            if let colon = e.id.firstIndex(of: ":"), let source = UInt32(e.id[..<colon]),
               let net = EVMNetwork.all.first(where: { CCTP.domain($0.rpc.chainId) == source }),
               let u = URL(string: "\(net.explorer)/tx/\(e.id[e.id.index(after: colon)...])") { out.append(("Burn on \(net.name)", u)) }
        case "layerzero": if let u = URL(string: "https://layerzeroscan.com/tx/\(e.id)") { out.append(("LayerZero Scan", u)) }
        default: if let u = URL(string: "https://relay.link/transaction/\(e.id)") { out.append(("Relay", u)) }
        }
        if e.chain != "cctp", let tx = transaction(e) {
            let url = tx.hasPrefix("0x") ? nil : URL(string: "https://solscan.io/tx/\(tx)")
            if let url { out.append(("Solscan", url)) }
        }
        return out
    }
}

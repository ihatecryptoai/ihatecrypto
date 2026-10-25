import AppKit
import Charts
import IhatecryptoCore
import SwiftUI
import TradeExecution

extension Workspace {
    /// A token held on Robinhood Chain, opened on the canvas in place of the chain's card.
    func openToken(_ address: String, chain: String = "robinhood") {
        withAnimation(StudioView.spring) { detail = TokenBoard.place(chain: chain, address: address) }
        UserDefaults.standard.set("plan", forKey: "canvasMode")
    }

    /// What `opener` opens is open on the canvas now: the token's board, or the chart of that network's own coin.
    func isOpen(chain: String, contract: String, symbol: String) -> Bool {
        if contract != "native", chain != "hyperliquid" { return detail == TokenBoard.place(chain: chain, address: contract) }
        return detail == "chart" && chart?.coin == symbol.uppercased() && chart?.network == chain
    }

    /// What a click on a held token opens: the board of a token with a contract; the market's chart for a network's own
    /// coin (SOL, ETH, NEAR) or a Hyperliquid spot token that trades as a perp; nil when there's nothing to open. Both
    /// open in the same place on the canvas, each replacing the last.
    func opener(chain: String, contract: String, symbol: String) -> (() -> Void)? {
        if contract != "native", chain != "hyperliquid" { return { self.openToken(contract, chain: chain) } }
        let coin = symbol.uppercased()
        guard markets[coin] != nil else { return nil }
        return {
            withAnimation(StudioView.spring) {
                if self.chart?.coin != coin || self.chart?.range != .day { self.chart = ChartState(coin: coin, range: .day) }
                self.chart?.network = chain
                self.detail = "chart"
            }
            Task { await self.chartCommand(.chart(coin: coin, range: .day)) }
        }
    }

    /// The Buy and Sell of a network's coin, as the chat takes them — on every coin, never hidden (the user: «газ токен
    /// открывался бы как любой другой токен с бай и селл», «не отключай вообще кнопки»): SOL through Jupiter, NEAR
    /// through 1Click, a Hyperliquid spot token on its book, ETH, POL, BNB and HYPE on their network (the row's, else
    /// the first that pays gas in it: ETH → Base; USDG on Robinhood Chain), any other coin wherever the chat finds it.
    /// A move that can't go says so when it's sent.
    func coinTrades(_ coin: String, chain: String?) -> [(title: String, icon: String, phrase: String)] {
        let c = coin.uppercased()
        func pair(_ buy: String, _ sell: String) -> [(title: String, icon: String, phrase: String)] {
            [("Buy \(c)", "cart", buy), ("Sell", "arrow.down.right.circle", sell)]
        }
        if c == "SOL", chain == nil || chain == "solana" { return pair("buy SOL for 2 USDC", "sell 0.01 SOL") }
        if c == "NEAR" { return pair("buy NEAR for 2 USDC", "sell 1 NEAR") }
        if chain == "hyperliquid" { return pair("buy \(c) on hyperliquid spot for 10 USDC", "sell all \(c) on hyperliquid spot") }
        if let net = EVMNetwork.named(chain ?? "").flatMap({ $0.gasCoin == c ? $0 : nil })
            ?? EVMNetwork.all.first(where: { $0.gasCoin == c && $0.feeToken == nil }) {
            let some = ["POL": "5", "BNB": "0.002", "HYPE": "0.02"][c] ?? "0.001", dollar = net.key == "robinhood" ? "USDG" : "USDC"
            return pair("swap 2 \(dollar) for \(c) on \(net.key)", "swap \(some) \(c) for \(dollar) on \(net.key)")
        }
        return pair("buy \(c) for 2 USDC", "sell all \(c)")
    }

    /// How much of a coin the wallet holds on a network, as its portfolio line counts it: SOL, ETH on Base and Robinhood
    /// Chain and NEAR from the network's balance, the other networks' coins and Hyperliquid's spot tokens from Holdings.
    func coinHeld(_ coin: String, chain: String) -> Decimal? {
        switch (chain, coin.uppercased()) {
        case ("solana", "SOL"): solana?.sol
        case ("base", "ETH"): base?.eth
        case ("robinhood", "ETH"): robinhood?.eth
        case ("near", "NEAR"): near
        default: holdings.first { $0.chain == chain && $0.symbol.uppercased() == coin.uppercased() && ($0.contract == "native" || chain == "hyperliquid") }?.amount
        }
    }

    /// The buy and the sale of a token as the chat takes them, by its network: Robinhood's stock tokens by ticker, a
    /// Jupiter-verified Solana token by ticker (an unverified one by its mint only with Safe search off), on the EVM
    /// networks by contract. Each goes into the chat's field for the user to send.
    func tokenPhrases(chain: String, address: String, symbol: String, stock: Bool, verified: Bool) -> [(title: String, icon: String, phrase: String)] {
        // The network's own dollar: buying it is selling the network's coin for it, selling it is buying that coin (the
        // user: «для соланы SOL, для всех остальных сетей эфириум»); Robinhood's USDG against its ETH.
        if TokenBoard.walletDollar(chain: chain, address: address) {
            // Buying the dollar is selling the coin, selling it buying the coin.
            let coin = coinTrades(chain == "solana" ? "SOL" : EVMNetwork.named(chain)?.gasCoin ?? "ETH", chain: chain)
            return [("Buy \(symbol)", "cart", coin[1].phrase), ("Sell", "arrow.down.right.circle", coin[0].phrase)]
        }
        // Never hidden: an unverified Solana token with Safe search on, say, hears why when the phrase is sent.
        let pair: (buy: String, sell: String) = switch chain {
        case "robinhood": stock ? ("buy \(symbol) stock for 2 USDC", "sell \(symbol) stock") : ("buy token \(address) for 2 USDC", "sell token \(address)")
        case "solana": verified ? ("buy \(symbol) on solana for 2 USDC", "sell \(symbol) on solana") : ("buy token \(address) for 2 USDC", "sell tokens \(address)")
        case "tempo": ("swap 2 USDC for \(symbol) on tempo", "swap all \(symbol) for USDC on tempo")
        default: EVMNetwork.named(chain) != nil ? ("buy token \(address) on \(chain) for 2 USDC", "sell tokens \(address) on \(chain)")
            : ("buy \(symbol) for 2 USDC", "sell all \(symbol)")
        }
        // Bridged USDC: first, the swap for Circle's own on the same network (Uniswap's 0.01% pool).
        let bridged: [(title: String, icon: String, phrase: String)] = symbol == "USDC.e"
            ? [("Swap to Circle's USDC", "arrow.triangle.2.circlepath", "swap all USDC.e for USDC on \(chain)")] : []
        return bridged + [("Buy \(symbol)", "cart", pair.buy), ("Sell", "arrow.down.right.circle", pair.sell)]
    }

    /// Hands a token to the chat (the user: «чтобы контекст передавался»): a chip over the field, and the next message
    /// reaches the model as being about it — its network, its contract, what the wallet holds of it, its price.
    /// `address` nil: the network's own coin.
    func askAbout(symbol: String, chain: String, address: String?) {
        let network = ModuleNetwork(rawValue: chain)?.name ?? chain.capitalized
        let held = address.flatMap { a in holdings.first { $0.chain == chain && $0.contract.lowercased() == a.lowercased() } }
        let stock = address.flatMap { a in robinhood?.stocks.first { $0.address.lowercased() == a.lowercased() } }
        var facts = ["\(symbol) on \(network)"] + (address.map { ["contract \($0)"] } ?? [])
        if let amount = stock?.amount ?? held?.amount {
            facts.append("the wallet holds \(Fmt.num(amount, maxFraction: 6))" + ((stock?.usd ?? held?.usd).map { " ≈ \(Fmt.usd($0))" } ?? ""))
        }
        if let price = stock?.price ?? held?.price ?? markets[symbol.uppercased()]?.markPx { facts.append("price \(Fmt.price(price))") }
        attach(StepContext(title: "\(symbol) · \(network)", logo: TokenLogo.name(forAsset: symbol) ?? ModuleNetwork(rawValue: chain)?.logo,
                           about: "About the token «\(facts.joined(separator: "; "))»"))
    }
}

/// A held token's right-click menu, wherever it's listed: its details (or chart), the chat with the token attached, a
/// buy and a sale put into the chat's field.
struct TokenMenu: ViewModifier {
    @Environment(Workspace.self) private var ws
    let chain: String
    /// nil: the network's own coin (SOL, ETH, NEAR) or a Hyperliquid spot token.
    let address: String?
    let symbol: String
    var stock = false
    var verified = false

    func body(content: Content) -> some View {
        content.contextMenu {
            Group {
                if let open = ws.opener(chain: chain, contract: address ?? "native", symbol: symbol) {
                    Button(action: open) { Label(address == nil ? "Chart" : "Details", systemImage: address == nil ? "chart.xyaxis.line" : "info.circle") }
                }
                Button { ws.askAbout(symbol: symbol, chain: chain, address: address) } label: { Label("Ask in chat", systemImage: "bubble.left") }
                ForEach(address.map { ws.tokenPhrases(chain: chain, address: $0, symbol: symbol, stock: stock, verified: verified) }
                        ?? ws.coinTrades(symbol, chain: chain), id: \.phrase) { p in
                    Button { ws.suggest(p.phrase) } label: { Label(p.title, systemImage: p.icon) }
                }
            }
            .labelStyle(.titleAndIcon)
        }
    }
}

/// One token on the canvas: its price and chart, the market around it (DexScreener's deepest pair, GeckoTerminal's
/// history of that pool) and what you hold. It lives in `ws.detail` as «token:<chain>:<contract>».
struct TokenBoard: View {
    @Environment(Workspace.self) private var ws
    let chain: String
    let address: String
    @State private var market: TokenMarket.Info?
    @State private var points: [TokenMarket.Point] = []
    @State private var range: ChartCommand.Range = .day
    @State private var error: String?
    /// The chart point under the pointer. The board holds it but never reads it: only the price line and the marker
    /// do, so a mouse move over the chart redraws those two, not the chart, the stats and the rest of the board.
    @State private var pointer = ChartPointer()
    /// Checked by a list a copy can't get onto: Jupiter's on Solana, Robinhood's registry for its stock tokens.
    @State private var verified: (by: String, ok: Bool)?

    static func place(chain: String, address: String) -> String { "token:\(chain):\(address)" }

    static func parse(_ place: String) -> (chain: String, address: String)? {
        let parts = place.split(separator: ":")
        return parts.count == 3 && parts[0] == "token" ? (String(parts[1]), String(parts[2])) : nil
    }

    private var holding: StockHolding? { ws.robinhood?.stocks.first { $0.address.lowercased() == address.lowercased() } }
    /// The token as the wallet holds it on any other network (Holdings).
    private var heldToken: Holding? { ws.holdings.first { $0.chain == chain && $0.contract.lowercased() == address.lowercased() } }
    private var favorite: FavoriteToken? { ws.favoriteTokens.first { $0.chain == chain && $0.address.lowercased() == address.lowercased() } }
    private var symbol: String { holding?.symbol ?? heldToken?.symbol ?? favorite?.symbol ?? market?.symbol ?? "Token" }
    private var fullName: String { holding?.name ?? favorite?.name ?? market?.name ?? symbol }
    /// «Space Exploration Technologies Corp. Class A Common Stock» → «Space Exploration Technologies».
    private var name: String {
        let cut = [" Corp", ", Inc", " Inc.", " Class ", " Common Stock", " Ltd"].compactMap { fullName.range(of: $0)?.lowerBound }.min()
        return cut.map { String(fullName[..<$0]) } ?? fullName
    }
    /// A Robinhood Stock Token (bought by its ticker); anything else here was bought by its contract.
    private var isStock: Bool { Self.isStock(symbol) }
    static func isStock(_ symbol: String) -> Bool { (UserDefaults.standard.stringArray(forKey: "stockTokens") ?? []).contains(symbol.uppercased()) }
    private var chainTitle: String { ModuleNetwork(rawValue: chain)?.name ?? chain.capitalized }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            header
            AssetPrice(pointer: pointer, now: market?.priceUSD ?? points.last?.close ?? holding?.price, change: rangeChange, range: $range)
            AssetChart(points: points, range: range, pointer: pointer, error: error)
            if let m = market { stats(m) }
            if let h = holding {
                HeldRow(symbol: symbol, amount: h.amount, usd: h.usd,
                        how: isStock ? "valued at Robinhood’s bid \(h.price.map(tokenPrice) ?? "—")" : "valued at the pool price",
                        pnl: h.price.flatMap { ws.cost("robinhood:\(h.address.lowercased())")?.pnl(held: h.amount, price: $0) },
                        cost: ws.cost("robinhood:\(h.address.lowercased())"))
            } else if let t = heldToken {
                HeldRow(symbol: symbol, amount: t.amount, usd: t.usd, how: t.price == nil ? "no price yet" : "valued at the pool price",
                        pnl: ws.purchasePnL(t), cost: ws.cost(t.id) ?? ws.cost(t.contract), today: Self.today(t))
            } else if Self.walletDollar(chain: chain, address: address), let usdc = ws.usdc(on: chain), usdc > 0 {
                HeldRow(symbol: symbol, amount: usdc, usd: usdc, how: "not earning")
            }
            links
            actions
        }
        .assetCard()
        .task { await ws.loadChainCosts() }
        .task(id: address) {
            market = try? await TokenMarket.info(address, chain: chain)
            if market == nil { error = "No pool trades this token on \(chainTitle)." }
            if chain == "solana" { verified = await TokenResearch.solanaToken(address).map { ("Jupiter", $0.verified) } }
            else if holding != nil || isStock { verified = ("Robinhood's registry", true) }
            tell()
        }
        .task(id: "\(market?.pool ?? "")·\(range)") {
            guard let pool = market?.pool else { return }
            do {
                points = try await TokenMarket.history(chain: chain, pool: pool, range: range)
                error = points.count > 1 ? nil : "No trades in this period."
            } catch {
                self.error = error.localizedDescription
            }
            tell()
        }
    }

    /// What the card shows, for the chat's model: its numbers, the chart in the range picked, what the wallet holds.
    private func tell() {
        let held = holding.map { ($0.amount, $0.usd) } ?? heldToken.map { ($0.amount, $0.usd) }
        let lines = ["\(fullName) (\(symbol)), contract \(address) on \(chainTitle)" + (verified.map { $0.ok ? ", verified by \($0.by)" : ", not verified by \($0.by)" } ?? "") + "."]
            + (market.map { TokenResearch.poolFacts($0) } ?? [error ?? "Its market is loading."])
            + [TokenResearch.chartFacts(points, range: range, of: "Its chart (shown now)")].compactMap { $0 }
            + (held.map { ["The wallet holds \(Fmt.num($0.0, maxFraction: 6))" + ($0.1.map { " ≈ \(Fmt.usd($0))" } ?? "") + "."] } ?? [])
        ws.tokenCard = (TokenBoard.place(chain: chain, address: address), lines.joined(separator: " "))
    }

    // MARK: Parts

    private var header: some View {
        HStack(spacing: 10) {
            if chain == "robinhood" {
                Button { withAnimation(StudioView.spring) { ws.detail = "robinhood" } } label: { Image(systemName: "chevron.left") }
                    .buttonStyle(.hoverIcon).help("Back to Robinhood Chain")
            }
            AssetHeader(name: name, help: fullName, subtitle: [symbol, chainTitle, market.map { "\($0.dex) · \($0.quote) pool" }].compactMap { $0 }.joined(separator: " · "),
                        close: { withAnimation(StudioView.spring) { ws.detail = nil } }) {
                if let h = holding {
                    StockLogo(symbol: h.symbol, url: market?.imageURL ?? h.logo, size: 38)
                } else if let f = favorite {
                    TokenIcon(token: f, size: 38)
                } else {
                    StockLogo(symbol: symbol, url: market?.imageURL, size: 38)
                }
            } badge: {
                if let v = verified {
                    if v.ok { VerifiedBadge(size: 15, source: v.by) }
                    else { Text("not verified").font(.ui(10, .semibold)).foregroundStyle(Brand.jevInk).help("\(v.by) hasn't verified it: it may be a copy under a famous name") }
                }
            } tools: {}
        }
    }

    private var rangeChange: Decimal? {
        guard let first = points.first?.close, let last = points.last?.close, first > 0 else { return market?.changes["h24"] }
        return (last - first) / first * 100
    }

    private func stats(_ m: TokenMarket.Info) -> some View {
        let compact = AssetStats.compact
        return AssetStats(changes: [("m5", "5m"), ("h1", "1h"), ("h6", "6h"), ("h24", "24h")].compactMap { key, title in m.changes[key].map { (title, $0) } },
                          tiles: [(isStock ? "Tokenized value" : "Market cap", (m.marketCap ?? m.fdv).map(compact)),
                                  ("Liquidity", m.liquidityUSD.map(compact)),
                                  ("Volume 24h", m.volume24h.map(compact)),
                                  ("Buys 24h", m.buys24h.map { Fmt.num(Decimal($0), maxFraction: 0) }),
                                  ("Sells 24h", m.sells24h.map { Fmt.num(Decimal($0), maxFraction: 0) }),
                                  ("Pool since", m.created.map { $0.formatted(Date.FormatStyle(locale: Locale(identifier: "en_US")).day().month(.abbreviated)) })])
    }

    /// Today's move of what's held, in dollars, from its 24-hour change.
    static func today(_ t: Holding) -> Decimal? {
        guard let usd = t.usd, let change = t.change24h, change > -100 else { return nil }
        return usd - usd / (1 + change / 100)
    }

    /// Wraps: a token with many socials must not spill past the card.
    private var links: some View {
        Flow(spacing: 14) {
            CopyButton(title: "\(address.prefix(6))…\(address.suffix(4))", text: address, help: "Copy \(address)")
            ForEach(market?.links ?? [], id: \.url) { Link($0.label, destination: $0.url).pointerStyle(.link) }
            if let pair = market?.pairURL { Link("DexScreener", destination: pair).pointerStyle(.link) }
            if chain == "robinhood", let url = URL(string: "https://robinhoodchain.blockscout.com/token/\(address)") { Link("Explorer", destination: url).pointerStyle(.link) }
        }
        .buttonStyle(.borderless).font(.ui(10)).tint(Brand.accent).foregroundStyle(Brand.accentInk)
    }

    /// Ask in the chat with the token attached, then its buy and sale (or watching it) put into the chat's field.
    private var actions: some View {
        let trades = ws.tokenPhrases(chain: chain, address: address, symbol: symbol, stock: isStock,
                                     verified: verified?.ok == true || heldToken?.verified == true)
        let watch = trades.isEmpty && favorite == nil && !Self.walletDollar(chain: chain, address: address) ? [(title: "Watch \(symbol)", icon: "star", phrase: "watch token \(address) on \(chain)")] : []
        return AssetActions(symbol: symbol, ask: { ws.askAbout(symbol: symbol, chain: chain, address: address) }, trades: trades + watch,
                            source: "DexScreener · GeckoTerminal")
    }
}

// MARK: The asset card

// One look for every asset on the canvas, a token with a contract (TokenBoard, DexScreener + GeckoTerminal) and a
// network's coin or a Hyperliquid market (ChartBoard, Hyperliquid candles): only where the numbers come from differs.

extension View {
    /// The asset card's frame. Always its full height: squeezed, the price (the one text allowed to shrink) went half size.
    /// `width` nil: as wide as the column it stands in (a plan drawn on its chart, PlanChartBoard).
    func assetCard(width: CGFloat? = 470) -> some View {
        fixedSize(horizontal: false, vertical: true)
            .padding(16)
            .frame(width: width, alignment: .leading)
            .frame(maxWidth: width == nil ? .infinity : nil, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 24).fill(Brand.card).shadow(color: .black.opacity(0.06), radius: 14, y: 8))
            .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(Brand.accent.opacity(0.4), lineWidth: 1.5))
            .transition(.opacity)
    }
}

extension ChartCommand.Range {
    /// The range picker's word.
    var short: String { switch self { case .day: "24h"; case .week: "7d"; case .month: "30d" } }
}

/// The logo, the name with its badge and a line about the asset; the card's own buttons, then the close.
struct AssetHeader<Logo: View, Badge: View, Tools: View>: View {
    let name: String
    var help: String?
    let subtitle: String
    let close: () -> Void
    @ViewBuilder let logo: Logo
    @ViewBuilder let badge: Badge
    @ViewBuilder let tools: Tools

    var body: some View {
        HStack(spacing: 10) {
            logo
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Text(name).font(.ui(16, .bold)).lineLimit(1).help(help ?? name)
                    badge
                }
                Text(subtitle).font(.ui(10)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            tools
            Button(action: close) { Image(systemName: "xmark") }.buttonStyle(.hoverIcon)
        }
    }
}

/// The price now, or at the pointer on the chart, and the move over the range; the range picker.
struct AssetPrice: View {
    let pointer: ChartPointer
    let now: Decimal?
    let change: Decimal?
    @Binding var range: ChartCommand.Range

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            PointerPrice(pointer: pointer, now: now, change: change, rangeTitle: range.title).layoutPriority(1)
            Spacer()
            Picker("", selection: $range) {
                ForEach(ChartCommand.Range.allCases, id: \.self) { Text($0.short).tag($0) }
            }
            .pickerStyle(.segmented).labelsHidden().fixedSize().pointerStyle(.link)
        }
    }
}

/// A line across the chart at a price: a position's entry, its stop and take, an alert.
struct ChartLevel: Identifiable {
    let id: String
    let price: Double
    let label: String
    let color: Color
    let dashed: Bool
}

/// A band across the chart between two prices: what a plan loses from its entry to the stop, or makes to the take.
struct ChartZone: Identifiable {
    let id: String
    let from: Double
    let to: Double
    let color: Color
}

/// The price over the range, green up and red down, with its levels; the point under the pointer shows in the price above.
struct AssetChart: View {
    let points: [TokenMarket.Point]
    let range: ChartCommand.Range
    let pointer: ChartPointer
    var levels: [ChartLevel] = []
    var zones: [ChartZone] = []
    var height: CGFloat = 190
    var error: String?

    var body: some View {
        let closes = points.map { NSDecimalNumber(decimal: $0.close).doubleValue }
        if closes.count > 1, let lo = closes.min(), let hi = closes.max(), let first = closes.first, let last = closes.last {
            let levels = levels.filter { $0.price > lo * 0.85 && $0.price < hi * 1.15 }
            let bottom = min(lo, levels.map(\.price).min() ?? lo), top = max(hi, levels.map(\.price).max() ?? hi)
            let pad = max((top - bottom) * 0.1, top * 0.002)
            let tint = last < first ? Brand.loss : Brand.profit
            let lowest = bottom - pad, highest = top + pad
            Chart {
                // Under the price, inside the chart's range.
                ForEach(zones) { z in
                    RectangleMark(yStart: .value("From", min(max(z.from, lowest), highest)), yEnd: .value("To", min(max(z.to, lowest), highest)))
                        .foregroundStyle(z.color.opacity(0.1))
                }
                ForEach(points, id: \.time) { p in
                    AreaMark(x: .value("Time", p.time), yStart: .value("Base", bottom - pad), yEnd: .value("Price", NSDecimalNumber(decimal: p.close).doubleValue))
                        .foregroundStyle(LinearGradient(colors: [tint.opacity(0.22), .clear], startPoint: .top, endPoint: .bottom))
                    LineMark(x: .value("Time", p.time), y: .value("Price", NSDecimalNumber(decimal: p.close).doubleValue))
                        .foregroundStyle(tint)
                        .lineStyle(StrokeStyle(lineWidth: 1.8, lineCap: .round, lineJoin: .round))
                }
                ForEach(levels) { l in
                    RuleMark(y: .value(l.label, l.price))
                        .foregroundStyle(l.color)
                        .lineStyle(StrokeStyle(lineWidth: 1.2, dash: l.dashed ? [4, 3] : []))
                        .annotation(position: .top, alignment: .leading) {
                            Text("\(l.label) \(Fmt.price(Decimal(l.price)))").font(.ui(9.5, .bold)).foregroundStyle(l.color)
                        }
                }
            }
            .chartYScale(domain: (bottom - pad)...(top + pad))
            .priceAxes(range)
            .chartOverlay { proxy in TokenChartHover(proxy: proxy, points: points, pointer: pointer, tint: tint) }
            .frame(height: height)
            // A point from the previous range or load isn't on this chart: the price above goes back to the price now.
            .onChange(of: points) { pointer.point = nil }
        } else if let error {
            Label(error, systemImage: "chart.xyaxis.line").font(.ui(10)).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 120)
        } else {
            ProgressView().frame(maxWidth: .infinity, minHeight: height)
        }
    }
}

/// The moves as chips («24h +1.2%»), then the market's numbers in tiles, three a row (`perRow`).
struct AssetStats: View {
    let changes: [(String, Decimal)]
    let tiles: [(String, String?)]
    var perRow = 3

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if !changes.isEmpty {
                Flow(spacing: 6) {
                    ForEach(changes, id: \.0) { title, c in
                        HStack(spacing: 4) {
                            Text(title).foregroundStyle(.secondary)
                            Text(signedPct(c)).foregroundStyle(c < 0 ? Brand.loss : Brand.profit)
                        }
                        .font(.ui(11, .semibold)).monospacedDigit()
                        .padding(.horizontal, 8).padding(.vertical, 4)
                        .background(Capsule().fill(Color.primary.opacity(0.05)))
                    }
                }
            }
            Grid(horizontalSpacing: 8, verticalSpacing: 8) {
                ForEach(Array(stride(from: 0, to: tiles.count, by: perRow)), id: \.self) { row in
                    GridRow {
                        ForEach(tiles[row..<min(row + perRow, tiles.count)], id: \.0) { title, value in tile(title, value) }
                    }
                }
            }
        }
    }

    private func tile(_ title: String, _ value: String?) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title.uppercased()).font(.ui(9.5, .semibold)).tracking(0.5).foregroundStyle(.secondary)
            Text(value ?? "—").font(.ui(14, .bold)).monospacedDigit()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.primary.opacity(0.04)))
    }

    /// $218.7M, $1.3M, $12K.
    static func compact(_ d: Decimal) -> String {
        d.formatted(.currency(code: "USD").notation(.compactName).precision(.significantDigits(3)).locale(Locale(identifier: "en_US")))
    }
}

/// What's held, and how it went, the P/L first: since it was
/// bought, in dollars and percent, beside what it's worth now; under it what was paid, at what price, and what that is
/// worth now. Bought anywhere: the cost comes from the app's own buys or the chain (`Workspace.cost`). One with no
/// cost to count from (it came in by a transfer) shows its worth and today's move.
struct HeldRow: View {
    let symbol: String
    let amount: Decimal
    let usd: Decimal?
    let how: String
    var pnl: (usd: Decimal, pct: Decimal)?
    var cost: CostBasis?
    var today: Decimal?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(heading).font(.ui(9.5, .semibold)).tracking(0.5).foregroundStyle(.secondary).lineLimit(1).minimumScaleFactor(0.8)
                Spacer(minLength: 4)
                if let today, today != 0 {
                    Text("Today \(masked(signed(today)))").font(.ui(11, .semibold)).foregroundStyle(today < 0 ? Brand.loss : Brand.profit).fixedSize()
                }
            }
            HStack(alignment: .center, spacing: 10) {
                if let pnl {
                    let tint = pnl.usd < 0 ? Brand.loss : Brand.profit
                    Text(masked(signed(pnl.usd))).font(.ui(26, .bold)).foregroundStyle(tint).lineLimit(1).minimumScaleFactor(0.6)
                    Text("\(pnl.pct < 0 ? "−" : "+")\(Fmt.pct(abs(pnl.pct).rounded(1, .plain)))").font(.ui(12, .bold)).foregroundStyle(tint)
                        .padding(.horizontal, 9).padding(.vertical, 3).background(Capsule().fill(tint.opacity(0.14))).fixedSize()
                    Spacer(minLength: 4)
                    VStack(alignment: .trailing, spacing: 1) {
                        Text(masked(usd.map(Fmt.usd) ?? "—")).font(.ui(15, .bold))
                        Text("\(masked(Fmt.num(amount, maxFraction: 6))) \(symbol)").font(.ui(10)).foregroundStyle(.secondary)
                    }
                    .lineLimit(1).fixedSize()
                } else {
                    Text(masked(usd.map(Fmt.usd) ?? "—")).font(.ui(22, .bold)).lineLimit(1).minimumScaleFactor(0.6)
                    Spacer(minLength: 4)
                    Text("\(masked(Fmt.num(amount, maxFraction: 6))) \(symbol)").font(.ui(11, .semibold)).foregroundStyle(.secondary).lineLimit(1).fixedSize()
                }
            }
            .monospacedDigit()
            Text(masked(paid)).font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 14).fill(Brand.accent.opacity(0.08)))
        .help(how)
    }

    /// «YOUR NEAR · SINCE YOU BOUGHT ON OCT 10»; «· SINCE OCT 13» when some of it came in unpaid.
    private var heading: String {
        let day = pnl == nil ? nil : cost?.since?.formatted(.dateTime.month(.abbreviated).day())
        let since = day.map { (cost?.received ?? 0) > 0 ? " · since \($0)" : " · since you bought on \($0)" }
        return ("Your \(symbol)" + (since ?? "")).uppercased()
    }

    /// «Bought 0.3968 NEAR for $2.00 at $5.04 · 0.0766 more came in from elsewhere, counted at the price then: $5.10 ·
    /// worth $2.39 now».
    private var paid: String {
        guard let pnl, let cost, cost.amount > 0 else { return "No price yet to count its P/L from" }
        let counted = min(amount, cost.amount), share = counted / cost.amount
        let came = (cost.received ?? 0) * share, cameUSD = (cost.receivedUSD ?? 0) * share
        let bought = counted - came, spent = cost.usd * share - cameUSD
        var parts: [String] = []
        if bought > 0 { parts.append("Bought \(Fmt.num(bought, maxFraction: 4)) \(symbol) for \(Fmt.usd(spent)) at \(tokenPrice(spent / bought))") }
        if came > 0 {
            parts.append("\(Fmt.num(came, maxFraction: 4)) \(bought > 0 ? "more" : symbol) came in from elsewhere, counted at the price then: \(tokenPrice(cameUSD / came))")
        }
        parts.append("worth \(Fmt.usd(cost.usd * share + pnl.usd)) now")
        if amount > counted { parts.append("\(Fmt.num(amount - counted, maxFraction: 4)) more not counted: no price for it") }
        return parts.joined(separator: " · ")
    }

    private func signed(_ d: Decimal) -> String { (d < 0 ? "−" : "+") + Fmt.usd(abs(d)) }
}

/// Ask in the chat with the asset attached, then its buy and sale (or watching it) put into the chat's field; where the
/// card's numbers come from under them.
struct AssetActions: View {
    @Environment(Workspace.self) private var ws
    let symbol: String
    let ask: () -> Void
    let trades: [(title: String, icon: String, phrase: String)]
    /// Where its numbers come from, small under the buttons; the chart says it in its header instead.
    var source: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Flow(spacing: 6) {
                pill("Ask in chat", "bubble.left", sell: false,
                     help: "Your next message in the chat is about \(symbol): its network, price and what you hold go along", action: ask)
                ForEach(trades, id: \.phrase) { p in
                    pill(p.title, p.icon, sell: p.title == "Sell", help: "Puts “\(p.phrase)” into the chat") { ws.suggest(p.phrase) }
                }
            }
            if let source { Text(source).font(.ui(10)).foregroundStyle(.tertiary).lineLimit(1) }
        }
    }

    private func pill(_ title: String, _ icon: String, sell: Bool, help: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: icon).font(.ui(12, .semibold)).foregroundStyle(sell ? Brand.loss : Brand.accentInk)
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(Capsule().fill((sell ? Brand.loss : Brand.accent).opacity(0.1)))
        }
        .buttonStyle(.plain).pointerStyle(.link).help(help)
    }
}

/// The token chart's point under the pointer, shared by the marker on the chart and the price line above it.
@Observable final class ChartPointer {
    var point: TokenMarket.Point?
}

extension View {
    /// Both price charts' axes: times short enough to read whole under the chart — the hour on a day's chart, the date on a
    /// week's or a month's («Sep 29 at 1…» was cut) — and the prices on the right in the text's own colour, not faded.
    func priceAxes(_ range: ChartCommand.Range) -> some View {
        chartXAxis {
            AxisMarks(values: .automatic(desiredCount: 4)) { value in
                AxisGridLine()
                AxisValueLabel {
                    if let date = value.as(Date.self) {
                        Text(date, format: range == .day ? Date.FormatStyle(locale: Locale(identifier: "en_US")).hour()
                                                         : Date.FormatStyle(locale: Locale(identifier: "en_US")).month(.abbreviated).day())
                            .font(.ui(10)).foregroundStyle(.secondary).fixedSize()
                    }
                }
            }
        }
        .chartYAxis {
            AxisMarks(position: .trailing, values: .automatic(desiredCount: 4)) { value in
                AxisGridLine()
                AxisValueLabel().font(.ui(10, .semibold)).foregroundStyle(.primary)
            }
        }
    }
}

/// The price at the pointer (or now) and under it the pointer's time (or the move over the range).
private struct PointerPrice: View {
    let pointer: ChartPointer
    let now: Decimal?
    let change: Decimal?
    let rangeTitle: String

    var body: some View {
        let shown = pointer.point
        VStack(alignment: .leading, spacing: 2) {
            Text((shown?.close ?? now).map(tokenPrice) ?? "—").font(.ui(28, .heavy)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.5).layoutPriority(1)
            if let shown {
                Text(shown.time.formatted(Date.FormatStyle(locale: Locale(identifier: "en_US")).day().month(.abbreviated).hour().minute()))
                    .font(.ui(10)).foregroundStyle(.secondary).lineLimit(1)
            } else if let change {
                Text("\(signedPct(change)) · \(rangeTitle)").font(.ui(10, .bold)).foregroundStyle(change < 0 ? Brand.loss : Brand.profit).lineLimit(1)
            }
        }
    }
}

/// A dashed line and a dot at the pointer. Its own view, so moving the pointer redraws the marker,
/// not the chart's hundreds of marks.
private struct TokenChartHover: View {
    let proxy: ChartProxy
    let points: [TokenMarket.Point]
    let pointer: ChartPointer
    let tint: Color

    var body: some View {
        GeometryReader { geo in
            let plot = proxy.plotFrame.map { geo[$0] } ?? .zero
            ZStack(alignment: .topLeading) {
                Color.clear.contentShape(Rectangle())
                    .onContinuousHover { phase in
                        var near: TokenMarket.Point?
                        if case .active(let p) = phase, let at: Date = proxy.value(atX: p.x - plot.minX) {
                            near = points.min { abs($0.time.timeIntervalSince(at)) < abs($1.time.timeIntervalSince(at)) }
                        }
                        // Moving within one point's slot changes nothing: no redraw.
                        if near != pointer.point { pointer.point = near }
                    }
                if let p = pointer.point, let x = proxy.position(forX: p.time), let y = proxy.position(forY: NSDecimalNumber(decimal: p.close).doubleValue) {
                    Group {
                        Path { $0.move(to: CGPoint(x: plot.minX + x, y: plot.minY)); $0.addLine(to: CGPoint(x: plot.minX + x, y: plot.maxY)) }
                            .stroke(.secondary.opacity(0.5), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                        Circle().fill(tint).frame(width: 7, height: 7).position(x: plot.minX + x, y: plot.minY + y)
                    }
                    // The marker must not take the pointer: under it the hover area below ends, the marker goes, the hover
                    // starts again — it blinked once per candle.
                    .allowsHitTesting(false)
                }
            }
        }
    }
}

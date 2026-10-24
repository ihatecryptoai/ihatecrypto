import DeFiYields
import Foundation
import IhatecryptoCore
import JupiterSwap
import OneClickKit
import SignerAPI
import TradeExecution
import WalletCore

/// What the Mac's result boards draw, as data: the window draws them as the Mac does (TokenBoard, PnLBoard, YieldBubbles,
/// MoversBoard, ApprovalsBoard, JupiterDCABoard, ReceiveAcrossBoard, ServiceAnswerBoard). A board keeps its `lines` too:
/// they're what the model reads about it, and what an older window draws.
nonisolated struct BlockData: Codable, Equatable {
    var token: TokenCard?
    var pnl: PnLCard?
    var rates: [RateRow]?
    var movers: MoversCard?
    var approvals: ApprovalsCard?
    var dca: [DCARow]?
    var receive: ReceiveCard?
    var service: ServiceCard?

    struct Fact: Codable, Equatable { var title: String; var value: String }
    struct Link: Codable, Equatable { var label: String; var url: String }

    /// One token, as the Mac's TokenBoard: DexScreener's deepest pool and GeckoTerminal's history of it, what's held.
    struct TokenCard: Codable, Equatable {
        var chain: String
        var address: String
        var symbol: String
        var name: String
        var fullName: String
        var image: String?
        /// «SPCX · Robinhood Chain · Uniswap · USDC pool».
        var subtitle: String
        var verified: Bool?
        var verifiedBy: String?
        var stock = false
        var favorite = false
        /// The network's own dollar (USDC, Robinhood's USDG): its Buy sells the network's coin, there's nothing to watch.
        var dollar = false
        var price: Decimal?
        var range = "day"
        var closes: [Double] = []
        /// Each close's time, seconds since 1970 (the time axis).
        var times: [Double]? = nil
        /// The move over the range shown, or the pool's 24h when there's no history.
        var change: Decimal?
        /// «5m», «1h», «6h», «24h» → %.
        var changes: [Fact] = []
        var tiles: [Fact] = []
        var held: Held?
        var links: [Link] = []
        var loading = true
        var error: String?

        struct Held: Codable, Equatable {
            var amount: String
            var usd: Decimal?
            var how: String
            var pnl: String?
            var gain: Decimal?
            var today: Decimal?
            /// «Bought 0.3968 NEAR for $2.00 at $5.04 · … · worth $2.39 now» and «since you bought on Oct 10» (Portfolio.swift).
            var paid: String? = nil
            var since: String? = nil
        }
    }

    /// The Mac's PnLBoard: every period read at once, so the picker switches without asking again.
    struct PnLCard: Codable, Equatable {
        var coin: String?
        var period: String
        var reports: [Report]
        var held: [Held]

        struct Report: Codable, Equatable {
            var period: String
            var title: String
            var trading: Decimal
            var realized: Decimal
            var funding: Decimal
            var unrealized: Decimal
            var change: Decimal?
            var changeSince: Date?
            var flows: Decimal
            var closed: Int
            var wins: Int
            var byAsset: [Asset]
            var best: Trade?
            var worst: Trade?
        }
        struct Asset: Codable, Equatable { var coin: String; var name: String; var realized: Decimal; var closed: Int }
        struct Trade: Codable, Equatable { var coin: String; var name: String; var long: Bool; var closed: Date; var realized: Decimal }
        struct Held: Codable, Equatable {
            var symbol: String
            var network: String
            var place: String
            var amount: String
            var usd: Decimal?
            var gain: Decimal?
            var paid: Decimal?
            /// «(0.5 of it)»: the cost counts only this much of what's held (the Mac's partly).
            var counted: String? = nil
        }
    }

    struct RateRow: Codable, Equatable {
        var place: String
        var asset: String
        var apy: Decimal
        /// The phrase a tap puts in the chat; nil: nothing to do from here.
        var how: String?
        var note: String
    }

    struct MoversCard: Codable, Equatable {
        var up: [Mover]
        var down: [Mover]
        struct Mover: Codable, Equatable { var coin: String; var name: String; var price: String; var change: Decimal }
    }

    struct ApprovalsCard: Codable, Equatable {
        var list: [Approval]
        var unread: [String]
        /// Drawn before the first read is back: «Reading every network's approvals…» (the Mac's ApprovalsBoard loading).
        var loading: Bool? = nil
        struct Approval: Codable, Equatable {
            var chain: String
            var place: String
            var symbol: String
            var spender: String
            var spenderName: String?
            var token: String
            var unlimited: Bool
            var amount: String
            var permit2: Bool
            /// The revoke's phrase, put in the chat field.
            var revoke: String
        }
    }

    struct DCARow: Codable, Equatable {
        var coin: String
        var perCycle: Decimal
        var every: String
        var used: Decimal
        var deposited: Decimal
        var fills: Int
        var received: Decimal
        var cancel: String
    }

    struct ReceiveCard: Codable, Equatable {
        var network: String
        var title: String
        var token: String
        var amount: Decimal
        var address: String
        var minimum: Decimal
        var until: Date?
        var arrives: String
        /// 1Click's word for it, asked every 10 s until it's over.
        var state: String
    }

    struct ServiceCard: Codable, Equatable {
        var service: String
        var usdc: Decimal?
        var proof: String?
        var answer: MPP.Answer
        var raw: String
    }
}

/// A trade from Hyperliquid's fills closed in the last 7 days (the Mac's recentlyClosed, under the History block).
/// What a chart board shows beside its candles (the Mac's ChartBoard): read for the window on each state, never kept.
nonisolated struct ChartExtras: Codable, Equatable {
    var network: String?
    var change24h: Decimal?
    var tiles: [BlockData.Fact] = []
    var levels: [Level] = []
    var held: BlockData.TokenCard.Held?

    /// A line across the chart: the position's entry, its stop or take order, an alert.
    struct Level: Codable, Equatable { var price: Double; var label: String; var tint: String; var dashed: Bool }
}

nonisolated struct ClosedTrade: Codable, Equatable {
    var coin: String
    var name: String
    var long: Bool
    var closed: Date
    var realized: Decimal
}

/// What the Mac keeps beside a saved address that the signer's book doesn't: its picture, category and note.
nonisolated struct AddressExtra: Codable, Equatable {
    var icon: CanvasIcon?
    var category: String?
    var note: String?
    /// «chat» when it was saved from a chat message: the row says «from chat», as the Mac's SavedAddress.source.
    var source: String? = nil
}

extension Engine {
    /// The request that drew a board, not a «yes» or «cancel» said after (the Mac's Refresh asked «cancel» again).
    var lastRequest: String? {
        messages.last(where: { $0.fromUser && !ChatWords.confirms($0.text) && !ChatWords.declines($0.text) })?.text
    }

    // MARK: The token card

    /// A token by its contract opens its card on the canvas; the market is read after, into the same card.
    func openToken(_ address: String, chain: String) async -> [String: String] {
        // A dollar row has no contract in the portfolio (no P/L to keep): it's the network's own dollar.
        let address = address.isEmpty ? Self.walletDollar(chain) ?? "" : address
        guard !address.isEmpty else { return [:] }
        let held = heldRow(chain: chain, contract: address)
        let fav = favorites.first { $0.chain == chain && $0.address.lowercased() == address.lowercased() }
        let symbol = held?.symbol ?? fav?.symbol ?? "Token"
        // The same token's card again comes to the top instead of a second one.
        if let old = boards.first(where: { $0.data?.token?.address.lowercased() == address.lowercased() && $0.data?.token?.chain == chain }) {
            boards.removeAll { $0.id == old.id }
            boards.insert(old, at: 0)
            Task { await loadToken(old.id, range: .day) }
            return ["board": old.id]
        }
        var card = BlockData.TokenCard(chain: chain, address: address, symbol: symbol, name: fav?.name ?? symbol, fullName: fav?.name ?? symbol,
                                       image: held?.icon ?? fav?.image, subtitle: [symbol, networkTitle(chain)].joined(separator: " · "))
        card.favorite = fav != nil
        card.dollar = Self.walletDollar(chain)?.lowercased() == address.lowercased()
        var b = Board(kind: "token", title: "\(symbol) · \(networkTitle(chain))", lines: ["\(symbol) on \(networkTitle(chain)), contract \(address). Its market is loading."])
        b.data = BlockData(token: card)
        let id = pin(b)
        changed()
        Task { await loadToken(id, range: .day) }
        return ["board": id]
    }

    /// The card's market, history in `range`, the list that vouches for it, and what's held of it.
    func loadToken(_ id: String, range: ChartCommand.Range) async {
        guard var card = boards.first(where: { $0.id == id })?.data?.token else { return }
        let chain = card.chain, address = card.address
        card.range = range.rawValue
        let market = try? await TokenMarket.info(address, chain: chain)
        var points: [TokenMarket.Point] = []
        if let pool = market?.pool {
            do {
                points = try await TokenMarket.history(chain: chain, pool: pool, range: range)
                card.error = points.count > 1 ? nil : say("No trades in this period.", "За этот период сделок нет.")
            } catch { card.error = error.localizedDescription }
        } else {
            card.error = say("No pool trades this token on \(networkTitle(chain)).", "Ни один пул не торгует этим токеном в \(networkTitle(chain)).")
        }
        let stock = chain == "robinhood" ? await robinhoodStock(address) : nil
        var verified: (by: String, ok: Bool)?
        if chain == "solana" { verified = await TokenResearch.solanaToken(address).map { ("Jupiter", $0.verified) } }
        else if stock != nil { verified = ("Robinhood's registry", true) }
        let held = heldRow(chain: chain, contract: address)
        card.stock = stock != nil
        if let m = market {
            card.symbol = held?.symbol ?? stock?.symbol ?? m.symbol
            card.fullName = stock?.name ?? m.name
            card.name = Self.shortName(card.fullName)
            card.image = card.image ?? m.imageURL?.absoluteString
            card.subtitle = [card.symbol, networkTitle(chain), "\(m.dex) · \(m.quote) pool"].joined(separator: " · ")
            card.price = m.priceUSD
            card.changes = [("m5", "5m"), ("h1", "1h"), ("h6", "6h"), ("h24", "24h")].compactMap { key, title in m.changes[key].map { .init(title: title, value: "\($0)") } }
            let compact = Self.compactUSD
            card.tiles = [(card.stock ? say("Tokenized value", "Стоимость токенов") : say("Market cap", "Капитализация"), (m.marketCap ?? m.fdv).map(compact)),
                          (say("Liquidity", "Ликвидность"), m.liquidityUSD.map(compact)),
                          (say("Volume 24h", "Объём 24ч"), m.volume24h.map(compact)),
                          (say("Buys 24h", "Покупок 24ч"), m.buys24h.map { "\($0)" }),
                          (say("Sells 24h", "Продаж 24ч"), m.sells24h.map { "\($0)" }),
                          (say("Pool since", "Пул с"), m.created.map { Self.dayMonth($0) })].map { .init(title: $0.0, value: $0.1 ?? "—") }
            card.links = m.links.map { .init(label: $0.label, url: $0.url.absoluteString) } + (m.pairURL.map { [.init(label: "DexScreener", url: $0.absoluteString)] } ?? [])
        }
        if chain == "robinhood" { card.links.append(.init(label: say("Explorer", "Обозреватель"), url: "https://robinhoodchain.blockscout.com/token/\(address)")) }
        card.closes = points.map { NSDecimalNumber(decimal: $0.close).doubleValue }
        card.times = points.map { $0.time.timeIntervalSince1970 }
        if let first = points.first?.close, let last = points.last?.close, first > 0 { card.change = (last - first) / first * 100 } else { card.change = market?.changes["h24"] }
        card.verified = verified?.ok ?? held?.verified
        card.verifiedBy = verified?.by
        if let h = held {
            card.held = .init(amount: h.amount, usd: h.usd, how: h.usd == nil ? say("no price yet", "цены пока нет") : card.dollar ? say("not earning", "не приносит дохода") : card.stock ? say("valued at Robinhood's bid", "по цене покупки Robinhood") : say("valued at the pool price", "по цене пула"),
                              pnl: h.pnl, gain: h.gain, today: h.usd.flatMap { usd in h.change24h.flatMap { $0 > -100 ? usd - usd / (1 + $0 / 100) : nil } })
            if !card.dollar, let key = h.key {
                let told = Self.heldPaid(symbol: card.symbol, amount: h.value, gain: h.gain, cost: cost(key), ru: russian)
                card.held?.paid = told.paid
                card.held?.since = told.since
            }
        }
        card.favorite = favorites.contains { $0.chain == chain && $0.address.lowercased() == address.lowercased() }
        card.loading = false
        // What the card shows, for the model: its numbers, the chart in the range picked, what the wallet holds.
        let facts = ["\(card.fullName) (\(card.symbol)), contract \(address) on \(networkTitle(chain))" + (verified.map { $0.ok ? ", verified by \($0.by)" : ", not verified by \($0.by)" } ?? "") + "."]
            + (market.map { TokenResearch.poolFacts($0) } ?? [card.error ?? ""])
            + [TokenResearch.chartFacts(points, range: range, of: "Its chart (shown now)")].compactMap { $0 }
            + (held.map { ["The wallet holds \($0.amount)" + ($0.usd.map { " ≈ \(Fmt.usd($0))" } ?? "") + "."] } ?? [])
        updateBoard(id) { b in
            b.title = "\(card.symbol) · \(networkTitle(chain))"
            b.lines = facts.filter { !$0.isEmpty }
            b.data = BlockData(token: card)
        }
    }

    /// A Robinhood Chain token that is one of its stock tokens (its own registry).
    func robinhoodStock(_ address: String) async -> RobinhoodStocks.Token? {
        if Self.stockRegistry == nil { Self.stockRegistry = try? await RobinhoodStocks.registry() }
        return Self.stockRegistry?.first { $0.address.lowercased() == address.lowercased() }
    }
    nonisolated(unsafe) static var stockRegistry: [RobinhoodStocks.Token]?

    /// The wallet's row of a token on a network, shown or hidden.
    func heldRow(chain: String, contract: String) -> PortfolioView.Row? {
        guard let n = portfolio.networks.first(where: { $0.key == chain }) else { return nil }
        let dollar = Self.walletDollar(chain)?.lowercased() == contract.lowercased()
        return (n.rows + n.hidden).first { $0.contract?.lowercased() == contract.lowercased() }
            ?? (dollar ? n.rows.first { $0.contract == nil && $0.section == "tokens" && ["USDC", "USDG"].contains($0.symbol) } : nil)
    }

    /// The network's own dollar: Circle's USDC on Solana and each EVM network, USDG on Robinhood Chain (the Mac's walletDollar).
    nonisolated static func walletDollar(_ chain: String) -> String? {
        chain == "solana" ? SpotAsset.usdc.mint : EVMNetwork.named(chain)?.usdc
    }

    func networkTitle(_ key: String) -> String {
        portfolio.networks.first { $0.key == key }?.name ?? AddressCheck.Network(rawValue: key)?.title ?? WalletCommand.placeName(key)
    }

    /// «Space Exploration Technologies Corp. Class A Common Stock» → «Space Exploration Technologies».
    nonisolated static func shortName(_ full: String) -> String {
        let cut = [" Corp", ", Inc", " Inc.", " Class ", " Common Stock", " Ltd"].compactMap { full.range(of: $0)?.lowerBound }.min()
        return cut.map { String(full[..<$0]) } ?? full
    }

    /// $1.2B, $34.5M, $12.3K, $950.
    nonisolated static func compactUSD(_ d: Decimal) -> String {
        let v = NSDecimalNumber(decimal: d).doubleValue
        for (limit, suffix) in [(1e9, "B"), (1e6, "M"), (1e3, "K")] where abs(v) >= limit {
            return "$" + String(format: "%.1f", v / limit).replacingOccurrences(of: ".0", with: "") + suffix
        }
        return Fmt.usd(d.rounded(0, .plain))
    }

    nonisolated static func dayMonth(_ d: Date) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US")
        f.setLocalizedDateFormatFromTemplate("d MMM")
        return f.string(from: d)
    }

    // MARK: A chart opened by a click

    /// A coin row or a mover opens its market's chart on the canvas, as the Mac's opener: no line in the chat.
    func openChart(_ coin: String, range: ChartCommand.Range = .day, network: String? = nil) async throws -> [String: String] {
        var line = try await chartLine(coin, range: range)
        // Opened again in another range it keeps the network it was opened from.
        line.network = network ?? boards.first { $0.kind == "chart" && $0.chart?.coin == coin }?.chart?.network
        return ["board": placeChart(line)]
    }

    func chartLine(_ coin: String, range: ChartCommand.Range) async throws -> ChartLine {
        let candles = try await Hyperliquid.candles(coin: coin, range: range)
        guard let first = candles.first, let last = candles.last, first.close > 0 else { throw WalletError(say("No candles for \(IntentParser.pretty(coin)).", "Нет свечей по \(IntentParser.pretty(coin)).")) }
        let change = (last.close - first.close) / first.close * 100
        let low = candles.map(\.low).min() ?? last.close, high = candles.map(\.high).max() ?? last.close
        return ChartLine(title: "\(IntentParser.pretty(coin)) · \(range.title)", closes: candles.map { NSDecimalNumber(decimal: $0.close).doubleValue },
                         times: candles.map { $0.time.timeIntervalSince1970 }, low: Fmt.price(low), high: Fmt.price(high), change: signed(change), coin: coin)
    }

    /// One step of a route as its own block for the chat (the Mac's RouteCards.sendToChat): its name, money and rows.
    func stepBoard(_ b: Board, _ i: Int) -> Board? {
        guard let steps = b.steps, steps.indices.contains(i) else { return nil }
        let s = steps[i]
        let name = "Step \(i + 1) · \(s.label) · \(s.title)"
        let facts = [s.title] + (s.money.map { ["\($0.place) \($0.was) → \($0.after) \($0.symbol)"] } ?? []) + s.lines.filter { !$0.isEmpty }.map { $0.replacingOccurrences(of: "\t", with: " ") }
        return Board(kind: "result", title: name, lines: ["About the step «\(name)» of the route «\(b.title)» on the canvas (\(facts.joined(separator: " · ")))"])
    }

    /// With a trade plan on the canvas a price alert is for that plan: 2% its way from the price now, the plan as it
    /// stands (the Mac's BlocksPanel example; Alerts.swift ties the two).
    var planAlertExample: String? {
        let managed = tradedPlan == intent || positions.contains { $0.coin == intent.asset }
        guard intent != TradeIntent(), !managed, let mark = markets[intent.asset]?.markPx else { return nil }
        let long = intent.side == .long
        let level = mark * (long ? Decimal(string: "0.98")! : Decimal(string: "1.02")!)
        let asset = intent.asset.hasPrefix("xyz:") ? String(intent.asset.dropFirst(4)) : intent.asset
        return "when \(asset) is \(long ? "below" : "above") \(Fmt.price(level)): \(intent.command)"
    }

    // MARK: The chart's live parts

    /// The chart boards as the Mac's ChartBoard reads them on screen: the market's numbers, the position's entry, its
    /// stop and take orders and the alerts across the line, and what's held of the coin at Hyperliquid's price.
    func withChartExtras(_ boards: [Board]) -> [Board] {
        boards.map { b in
            guard b.kind == "chart", var line = b.chart, let coin = line.coin else { return b }
            var b = b
            line.extras = chartExtras(coin, network: line.network)
            b.chart = line
            return b
        }
    }

    func chartExtras(_ coin: String, network: String?) -> ChartExtras {
        func d(_ x: Decimal) -> Double { NSDecimalNumber(decimal: x).doubleValue }
        var x = ChartExtras(network: network)
        let m = markets[coin]
        if let m {
            x.change24h = m.change24h
            x.tiles = [.init(title: say("Volume 24h", "Объём 24ч"), value: m.volume24h > 0 ? Self.compactUSD(m.volume24h) : "—"),
                       .init(title: say("Funding 1h", "Фандинг 1ч"), value: m.funding.map { "\(Fmt.num($0 * 100, maxFraction: 4))%" } ?? "—"),
                       .init(title: say("Max leverage", "Макс. плечо"), value: "\(m.maxLeverage)×")]
        }
        if let p = positions.first(where: { $0.coin == coin }) {
            x.levels.append(.init(price: d(p.entry), label: p.size > 0 ? say("Long entry", "Вход в лонг") : say("Short entry", "Вход в шорт"), tint: "entry", dashed: false))
        }
        for o in hlOrders where o.coin == coin && o.isTrigger {
            guard let px = o.triggerPx else { continue }
            let stop = (o.orderType ?? "").contains("Stop")
            x.levels.append(.init(price: d(px), label: stop ? say("Stop", "Стоп") : say("Take", "Тейк"), tint: stop ? "loss" : "profit", dashed: false))
        }
        for a in alerts where a.coin == coin {
            x.levels.append(.init(price: d(a.price), label: say("Alert", "Оповещение"), tint: "alert", dashed: true))
        }
        // A coin with one home network is held there all the same when the chart came from the chat.
        let name = IntentParser.pretty(coin)
        guard let home = network ?? ["NEAR": "near", "SOL": "solana"][coin],
              let row = portfolio.networks.first(where: { $0.key == home })?.rows.first(where: { $0.symbol.uppercased() == name.uppercased() && $0.section == "tokens" }),
              row.value > 0 else { return x }
        let price = m?.markPx
        let usd = price.map { row.value * $0 }
        let basis = row.key.flatMap(cost)
        let pnl = price.flatMap { basis?.pnl(held: row.value, price: $0) }
        x.held = .init(amount: row.amount, usd: usd, how: price == nil ? say("no price yet", "цены пока нет") : say("valued at Hyperliquid's price", "по цене Hyperliquid"),
                       pnl: pnl.map { "\(signedUSD($0.usd)) (\($0.pct >= 0 ? "+" : "")\(Fmt.num($0.pct, maxFraction: 1))%)" }, gain: pnl?.usd,
                       today: usd.flatMap { u in m?.change24h.flatMap { $0 > -100 ? u - u / (1 + $0 / 100) : nil } })
        if let key = row.key {
            let told = Self.heldPaid(symbol: name, amount: row.value, gain: pnl?.usd, cost: cost(key), ru: russian)
            x.held?.paid = told.paid
            x.held?.since = told.since
        }
        return x
    }

    // MARK: P/L

    /// Every period's report and what's held of the coin asked about, for the board.
    func pnlCard(_ command: PnLCommand) async -> BlockData.PnLCard? {
        // Hyperliquid's fills and funding once, then each period from them (Profit.swift's pnlReport asks per period).
        guard let me = evmAddress, let wallet = solanaAddress else { return nil }
        async let fillsCall = TradeHistory.fills(user: me)
        async let fundingCall = TradeHistory.funding(user: me)
        let fills = (try? await fillsCall) ?? [], funding = (try? await fundingCall) ?? []
        let unrealized = Dictionary(positions.map { ($0.coin, $0.unrealizedPnl) }, uniquingKeysWith: +)
        let points = (Store.load([EquityPoint].self, "equity.json") ?? []).filter { $0.wallet == wallet }.map { (at: $0.at, usd: $0.usd) }
            + (portfolio.updatedAt != nil ? [(at: Date(), usd: ruleEquity ?? 0)] : [])
        let flows = (Store.load([PnL.Flow].self, "flows.json") ?? []).filter { $0.wallet == wallet }
        var reports: [BlockData.PnLCard.Report] = []
        for period in PnL.Period.allCases {
            let r = PnL.report(period, coin: command.coin, fills: fills, funding: funding, unrealized: unrealized, equity: points, flows: flows)
            reports.append(.init(period: period.rawValue, title: periodTitle(period), trading: r.trading, realized: r.realized, funding: r.funding,
                                 unrealized: r.unrealized, change: r.change, changeSince: r.changeSince, flows: r.flows, closed: r.closedTrades, wins: r.wins,
                                 byAsset: r.byAsset.prefix(6).map { .init(coin: $0.coin, name: IntentParser.pretty($0.coin), realized: $0.realized, closed: $0.closedTrades) },
                                 best: r.best.map { .init(coin: $0.coin, name: IntentParser.pretty($0.coin), long: $0.long, closed: $0.closed, realized: $0.realized) },
                                 worst: r.worst.map { .init(coin: $0.coin, name: IntentParser.pretty($0.coin), long: $0.long, closed: $0.closed, realized: $0.realized) }))
        }
        var held: [BlockData.PnLCard.Held] = []
        if let coin = command.coin {
            let symbol = (coin.split(separator: ":").last.map(String.init) ?? coin).uppercased()
            for n in portfolio.networks {
                for row in n.rows where row.symbol.uppercased() == symbol && row.value > 0 {
                    let basis = row.key.flatMap(cost)
                    let partly = row.gain != nil && (basis?.amount ?? 0) > 0 && basis!.amount < row.value * Decimal(string: "0.999")!
                    held.append(.init(symbol: row.symbol, network: n.key, place: n.name, amount: row.amount, usd: row.usd, gain: row.gain,
                                      paid: row.gain.flatMap { g in row.usd.map { $0 - g } }, counted: partly ? Fmt.num(basis!.amount, maxFraction: 6) : nil))
                }
            }
        }
        return .init(coin: command.coin, period: command.period.rawValue, reports: reports, held: held)
    }

    func periodTitle(_ p: PnL.Period) -> String {
        russian ? (["day": "24 ч", "week": "7 дней", "month": "30 дней", "all": "всё время"][p.rawValue] ?? p.title) : p.title
    }

    // MARK: Approvals, rates, Jupiter DCA

    func approvalsCard(_ list: [TokenApproval], unread: [String]) -> BlockData.ApprovalsCard {
        .init(list: list.prefix(40).map { a in
            .init(chain: a.chain, place: WalletCommand.placeName(a.chain), symbol: a.symbol, spender: a.spender, spenderName: a.spenderName, token: a.token,
                  unlimited: a.unlimited, amount: a.unlimited ? say("unlimited", "без лимита") : Fmt.num(a.amount / Decimal(sign: .plus, exponent: a.decimals, significand: 1), maxFraction: 6),
                  permit2: a.permit2, revoke: WalletCommand.revokeApproval(a.chain, token: a.token, spender: a.spender, permit2: a.permit2).summary)
        }, unread: unread.map(WalletCommand.placeName))
    }

    /// The approvals board's ↻: read every network again into the same board.
    func refreshApprovals(_ id: String) async {
        guard let e = try? executor() else { return }
        let (list, unread) = await e.approvals()
        fillApprovals(id, list, unread: unread)
    }

    /// The approvals board at once, saying it reads every network, as the Mac's does on the first ask; `fillApprovals`
    /// puts what was read into it.
    func readingApprovals() -> String {
        var b = Board(kind: "result", title: say("Approvals", "Разрешения"))
        b.data = BlockData(approvals: .init(list: [], unread: [], loading: true))
        b.request = lastRequest
        return pin(b)
    }

    func fillApprovals(_ id: String, _ list: [TokenApproval], unread: [String]) {
        updateBoard(id) { b in
            b.lines = list.isEmpty ? [say("No token approvals.", "Разрешений нет.")]
                : list.prefix(30).map { "\($0.symbol) on \($0.chain) → \($0.spenderName ?? $0.spender)" + ($0.unlimited ? " · unlimited" : "") }
            b.actions = list.isEmpty ? nil : list.prefix(12).map { a in
                Board.Action(title: say("Revoke \(a.symbol) · \(a.chain)", "Отозвать \(a.symbol) · \(a.chain)"),
                             command: WalletCommand.revokeApproval(a.chain, token: a.token, spender: a.spender, permit2: a.permit2).summary)
            }
            b.data = BlockData(approvals: approvalsCard(list, unread: unread))
            b.at = Date()
        }
    }

    func dcaRows(_ orders: [JupiterRecurring.Order]) -> [BlockData.DCARow] {
        orders.map { o in
            let coin = SpotAsset.all.first { $0.mint == o.outputMint }?.symbol ?? String(o.outputMint.prefix(4)) + "…"
            return .init(coin: coin, perCycle: o.perCycle, every: WalletCommand.everyText(o.every), used: o.used, deposited: o.deposited, fills: o.fills,
                         received: o.received, cancel: "cancel Jupiter DCA \(coin)")
        }
    }

    // MARK: Spreading and rebalancing as route cards

    /// The spread as the Mac's route cards (allocationBubbles): what it takes from each network, the moves between them,
    /// then into each place with its rate; what's skipped goes under them.
    func allocationSteps(_ a: AllocationPreview) -> (steps: [Board.Step], notes: [String]) {
        func amount(_ d: Decimal) -> String { Fmt.num(d, maxFraction: a.asset == "SOL" ? 6 : 2) + " " + a.asset }
        let placed = a.slices.reduce(Decimal(0)) { $0 + $1.amount }
        let drawn = a.drawn.isEmpty ? [(place: "Solana", amount: placed)] : a.drawn
        let from = drawn.enumerated().map { k, d in
            Board.Step(label: "FROM · \(d.place.uppercased())", title: amount(d.amount),
                       lines: k == 0 && a.leftover > 0 ? [say("stays free", "остаётся свободным") + "\t" + amount(a.leftover)] : [],
                       money: a.sources.first { $0.place == d.place }.map {
                           .init(place: d.place, symbol: a.asset, was: Fmt.num($0.amount, maxFraction: 6), after: Fmt.num(max(0, $0.amount - d.amount), maxFraction: 6))
                       })
        }
        let moves = a.moves.map { Board.Step(label: "BRIDGE · RELAY", title: amount($0.amount), lines: ["\($0.from) → \($0.to)"]) }
        let year = "≈ \(amount(a.yearly))" + (a.yearlyUSD.map { " (≈ \(Fmt.usd($0)))" } ?? "")
        let best = Board.Step(label: "INTO · BEST RATES", title: say("\(a.slices.count) place\(a.slices.count == 1 ? "" : "s")", "мест: \(a.slices.count)"),
                              lines: [say("a year", "за год") + "\t" + year, say("max per place", "максимум на место") + "\t" + Fmt.pct(a.maxShare)])
        let into = a.slices.map {
            Board.Step(label: "INTO · \($0.place.uppercased())", title: amount($0.amount), lines: [say("rate", "ставка") + "\t" + say("\(Fmt.num($0.apy.rounded(2, .plain)))% a year", "\(Fmt.num($0.apy.rounded(2, .plain)))% годовых")])
        }
        let notes = a.skipped.map { say("\($0.place) · \(Fmt.num($0.apy.rounded(2, .plain)))% — skipped: \($0.reason)", "\($0.place) · \(Fmt.num($0.apy.rounded(2, .plain)))% — пропущено: \($0.reason)") }
        return (from + moves + [best] + into, notes)
    }

    /// The rebalance as the Mac's RebalanceBoard: the whole, then each move out of one place and into the next with its
    /// rates, the extra interest a year, what it costs and when it pays back; the moves not worth it go under them.
    func rebalanceSteps(_ p: Rebalance.Plan, maxShare: Decimal) -> (steps: [Board.Step], notes: [String]) {
        let whole = Board.Step(label: "REBALANCE · USDC", title: say("+\(Fmt.usd(p.yearlyGain)) a year", "+\(Fmt.usd(p.yearlyGain)) в год"),
                               lines: [say("max per place", "максимум на место") + "\t" + Fmt.pct(maxShare), say("costs", "стоит") + "\t≈ " + Fmt.usd(p.cost)])
        let moves = p.moves.flatMap { m in [
            Board.Step(label: "FROM · \(m.from.uppercased())", title: "\(Fmt.num(m.amount, maxFraction: 2)) USDC",
                       lines: [say("rate", "ставка") + "\t\(Fmt.num(m.fromAPY.rounded(2, .plain)))%"]),
            Board.Step(label: "INTO · \(m.to.uppercased())", title: "\(Fmt.num(m.amount, maxFraction: 2)) USDC",
                       lines: [say("rate", "ставка") + "\t\(Fmt.num(m.toAPY.rounded(2, .plain)))%", say("a year", "за год") + "\t+" + Fmt.usd(m.yearlyGain),
                               say("costs", "стоит") + "\t≈ " + Fmt.usd(m.cost)]
                           + (m.paybackDays.map { [say("pays back", "окупится") + "\t" + say("\(Fmt.num($0.rounded(0, .up))) days", "за \(Fmt.num($0.rounded(0, .up))) дн.")] } ?? [])),
        ] }
        let notes = (p.moves.isEmpty ? [] : [say("Together ≈ +\(Fmt.usd(p.yearlyGain)) a year for ≈ \(Fmt.usd(p.cost)). Each move goes through Solana: out of one place, into the next, checked and simulated like the single commands.",
                                                 "Вместе ≈ +\(Fmt.usd(p.yearlyGain)) в год за ≈ \(Fmt.usd(p.cost)). Каждый перенос идёт через Solana: из одного места в другое, с теми же проверками и симуляцией, что и у отдельных команд.")])
            + p.skipped.map { say("\($0.move.from) → \($0.move.to) not worth it: \($0.reason).", "\($0.move.from) → \($0.move.to) не стоит того: \($0.reason).") }
            + p.notes
        return ([whole] + moves, notes)
    }

    // MARK: Receive from another network

    /// 1Click's word for the deposit, every 10 s while the board is on a canvas, until it's done or over.
    func followAcross(_ id: String, address: String) async {
        while boards.contains(where: { $0.id == id }) || canvases.contains(where: { $0.boards.contains { $0.id == id } }) {
            if let s = try? await OneClick.status(depositAddress: address) {
                let word = s.status == "SUCCESS" ? say("done: \(s.amountOutFormatted ?? "?") USDC on Solana", "готово: \(s.amountOutFormatted ?? "?") USDC на Solana")
                    : Executor.oneClickWords[s.status] ?? s.status.lowercased()
                updateBoard(id) { $0.data?.receive?.state = word }
                if s.isFinal { return }
            }
            try? await Task.sleep(for: .seconds(10))
        }
    }

    // MARK: Closed trades

    /// Hyperliquid's trades closed in the last 7 days, read with the balances (the Mac reads them with its account).
    func readClosed() async {
        guard let me = evmAddress else { return }
        guard let fills = try? await TradeHistory.fills(user: me) else { return }
        // Another wallet since it was asked: its trades aren't this one's (the Mac checks the account, Workspace.swift:2148).
        guard evmAddress?.lowercased() == me.lowercased() else { return }
        let week = Date().addingTimeInterval(-7 * 86_400)
        let closed: [ClosedTrade] = TradeHistory.stories(fills).compactMap { s in
            guard let at = s.closed, at > week else { return nil }
            return ClosedTrade(coin: s.coin, name: IntentParser.pretty(s.coin), long: s.long, closed: at, realized: s.realized)
        }.sorted { $0.closed > $1.closed }
        if closed != recentlyClosed { recentlyClosed = closed; changed() }
    }

    // MARK: The address book's own extras and the form's checks

    nonisolated static func extraKey(name: String, network: String) -> String { network + ":" + name.lowercased() }

    /// Read once, then kept: every state carries them.
    var addressExtras: [String: AddressExtra] {
        get {
            if let e = Self.extrasCache { return e }
            let e = Store.load([String: AddressExtra].self, "address-extras.json") ?? [:]
            Self.extrasCache = e
            return e
        }
        set { Self.extrasCache = newValue; Store.save(newValue, "address-extras.json") }
    }
    static var extrasCache: [String: AddressExtra]?

    /// A picture, a category or a note for a saved name; nil fields leave what's there, "" clears it.
    func setAddressExtra(name: String, network: String, icon: String?, category: String?, note: String?) throws {
        var all = addressExtras, key = Self.extraKey(name: name, network: network)
        var e = all[key] ?? AddressExtra()
        if let icon {
            if icon.isEmpty { e.icon = nil } else {
                guard let data = icon.data(using: .utf8), let parsed = try? JSONDecoder().decode(CanvasIcon.self, from: data), let ok = CanvasIcon.checked(parsed)
                else { throw WalletError(say("That picture can't be used.", "Эту картинку нельзя использовать.")) }
                e.icon = ok
            }
        }
        if let category { e.category = category.isEmpty ? nil : String(category.prefix(40)) }
        if let note { e.note = note.isEmpty ? nil : String(note.prefix(200)) }
        all[key] = e == AddressExtra() ? nil : e
        addressExtras = all
    }

    /// The form's line under the address as it's typed (the Mac's AddressForm.status): what's wrong with it, or its ends to
    /// compare, whether it's saved already, whether it's this wallet's own. Reads only.
    func checkAddress(_ address: String, network: String, editing: String?) -> [String: String] {
        let a = address.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !a.isEmpty else { return [:] }
        guard let net = AddressCheck.Network(rawValue: network) else { return ["problem": say("Pick the address's network.", "Выбери сеть адреса.")] }
        if var p = AddressCheck.problem(a, network: net) {
            let evm = a.hasPrefix("0x")
            if net == .solana, evm { p += say(" It looks like an EVM address — pick its network.", " Похоже на EVM-адрес — выбери его сеть.") }
            else if net != .solana, !evm, AddressCheck.problem(a, network: .solana) == nil { p += say(" It looks like a Solana address — pick Solana.", " Похоже на адрес Solana — выбери Solana.") }
            return ["problem": p]
        }
        let ends = AddressCheck.ends(a)
        var out = ["start": ends.start, "end": ends.end]
        if let dup = book.first(where: { $0.network == network && $0.address.lowercased() == a.lowercased() && $0.name.lowercased() != (editing ?? "").lowercased() }) {
            out["duplicate"] = dup.name
        }
        if a == solanaAddress || (evmAddress.map { a.lowercased() == $0.lowercased() } ?? false) { out["own"] = "true" }
        return out
    }

    /// A saved address changed in the form: a new name, address, network or memo goes into the signer's book first (Windows
    /// Hello, the whole address on its window), then the old entry goes; its picture and the rest follow the name.
    func editAddress(from old: (name: String, network: String), to entry: SignerAddress) async throws {
        guard let previous = book.first(where: { $0.name.lowercased() == old.name.lowercased() && $0.network == old.network }) else {
            throw WalletError(say("That saved address is gone.", "Этого сохранённого адреса уже нет."))
        }
        let changed = previous.name != entry.name || previous.address != entry.address || previous.network != entry.network || previous.memo != entry.memo
        guard changed else { return }
        try await saveAddress(entry)
        if previous.name.lowercased() != entry.name.lowercased() || previous.network != entry.network {
            try await removeAddress(name: previous.name, network: previous.network)
            var all = addressExtras
            if let e = all.removeValue(forKey: Self.extraKey(name: previous.name, network: previous.network)) { all[Self.extraKey(name: entry.name, network: entry.network)] = e }
            addressExtras = all
            let was = Self.starKey(previous)
            if starred.remove(was) != nil { starred.insert(Self.starKey(entry)) }
        }
    }
}

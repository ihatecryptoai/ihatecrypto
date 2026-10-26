import DeFiYields
import EVMChain
import Foundation
import IhatecryptoCore
import LLMCore
import TradeExecution
import WalletCore

/// One thing that happened with ihatecrypto: what the user asked, how it was understood, the bubbles on the canvas at that
/// moment, and what ihatecrypto said and did. Kept on this Mac only (Application Support/ihatecrypto/activity.json, owner-only).
nonisolated struct Activity: Codable, Identifiable, Sendable {
    enum Kind: String, Codable, Sendable { case request, execution, automatic }
    var id = UUID()
    var at = Date()
    var kind: Kind
    var request: String?
    var understood: String?
    var title: String?
    var coin: String?
    var bubbles: [ActivityBubble] = []
    var lines: [String] = []
    /// What ran on its own, for the Schedule canvas: «task:<id>», «buy:SOL», «perp:BTC». Optional, so older files still load.
    var schedule: String?
    /// The canvas a scheduled task ran on.
    var canvas: UUID?
    /// A run that failed (its notice went out as failed): a red mark on the Schedule's history.
    var failed: Bool?
    /// A run that had nothing to do or tell (no free USDC to spread, a check with nothing to say): a grey mark on the
    /// Schedule's history, not a green one, and no notification.
    var idle: Bool?
    /// A message the guard stopped: it stays in the history, and never reaches a model through the portfolio's facts.
    var stopped: Bool?

    /// `idle`, or a spread from before it that found nowhere to put the money.
    var didNothing: Bool { idle == true || lines.contains { $0.hasPrefix("Nowhere to put it now") || $0.hasPrefix("Сейчас некуда положить") } }
}

/// A canvas bubble frozen in time: enough to draw it again the same way in the history.
nonisolated struct ActivityBubble: Codable, Hashable, Sendable {
    /// A plan node kind (source, bridge, position, …) or deposit / skipped / yield / choice / free / result.
    var kind: String
    var label: String
    var title: String
    var lines: [String] = []
    var status: String = "info"
    var asset: String = ""
    var logo: String?
    var signature: Int?
    /// For position bubbles: long or short.
    var long: Bool?
    /// For a money move's route steps: the place's color.
    var fill: RouteFill?
    /// A paid request's last step: what the service answered, kept with the move (Payments.swift).
    var answer: ServiceAnswer?
    /// A money move's step: the balances it changes, read when the route was drawn (the card's «now → after»).
    var money: [MoneyMove]?
}

extension ActivityBubble {
    /// As a route card's step: a bubble kept from before moves were drawn as route cards (a transfer, a save, a skipped
    /// place, an old plan's blocks) becomes one, in its network's colour, so a saved canvas or the history shows no older
    /// card. A chart, rates and take-profit choices aren't steps.
    var routeStep: ActivityBubble? {
        if kind == "step" { return self }
        guard !["chart", "yield", "choice"].contains(kind) else { return nil }
        var s = self
        s.kind = "step"
        s.label = label.uppercased()
        s.fill = fill ?? logo.flatMap { RouteFill(rawValue: $0 == "sol" ? "solana" : $0) } ?? .check
        return s
    }

    /// The bubbles as a route, when every one of them is a step.
    static func route(_ bubbles: [ActivityBubble]) -> [ActivityBubble]? {
        let steps = bubbles.compactMap(\.routeStep)
        return steps.count == bubbles.count ? steps : nil
    }
}

/// A balance a route step changes: on `place`, `symbol` goes from `was` to `after`; `approx` when the after is a quote.
nonisolated struct MoneyMove: Codable, Hashable, Sendable, Identifiable {
    var place: String
    var symbol: String
    var was: Decimal
    var after: Decimal
    var approx = false

    var id: String { place + "·" + symbol }
}

extension Workspace {
    /// Starts a new activity; replies that follow are added to it.
    func beginActivity(_ kind: Activity.Kind, request: String? = nil, title: String? = nil, coin: String? = nil, schedule: String? = nil) {
        activities.append(Activity(kind: kind, request: request, title: title, coin: coin, schedule: schedule))
        currentActivity = activities.last?.id
    }

    func updateActivity(_ change: (inout Activity) -> Void) {
        guard let id = currentActivity, let i = activities.lastIndex(where: { $0.id == id }) else { return }
        change(&activities[i])
    }

    /// An address saved to the book, as a route card: while it waits for Touch ID and as Done.
    static func saveStep(_ entry: SavedAddress) -> ActivityBubble {
        let ends = AddressCheck.ends(entry.address)
        return ActivityBubble(kind: "step", label: "SAVE · \(entry.network.title.uppercased())", title: entry.name, lines: ["\(ends.start) … \(ends.end)"],
                              logo: entry.network.logo, fill: RouteFill(rawValue: entry.network.rawValue) ?? .check)
    }

    /// The canvas as it is now, as bubbles: the plan, an allocation, rates or take-profit choices.
    func canvasBubbles() -> (title: String, coin: String?, bubbles: [ActivityBubble])? {
        // A move waiting for «do it»: the canvas and the history name it the way the route does.
        switch pendingAction {
        case .wallet(let c)?:
            if let r = route(c) { return (r.title, nil, r.steps) }
            let title = c.summary.prefix(1).uppercased() + c.summary.dropFirst()
            return (title, nil, [ActivityBubble(kind: "step", label: "SIGN · CONFIRM", title: title, logo: Self.logo(for: title), fill: .check)])
        case .transfer(let entry, let amount)?:
            let r = transferRoute(entry, amount)
            return (r.title, nil, r.steps)
        case .address(let entry)?:
            return ("Save “\(entry.name)” to Addresses", nil, [Self.saveStep(entry)])
        default: break
        }
        if let f = finishedAction {
            return (f.title, nil, f.route.isEmpty
                    ? [ActivityBubble(kind: "step", label: "SIGN · CONFIRM", title: f.title, logo: Self.logo(for: f.title), fill: .check)]
                    : f.route)
        }
        if let c = chart, intent == TradeIntent(), allocationPreview == nil, yieldOptions == nil {
            let name = IntentParser.pretty(c.coin)
            return ("Chart · \(name)", c.coin, [ActivityBubble(kind: "chart", label: "Chart · \(c.range.title)", title: name,
                                                               lines: c.candles.last.map { ["\(Fmt.price($0.close))"] } ?? [], asset: c.coin)])
        }
        // Top up isn't the canvas's board: saved as one, it renamed the canvas «Top up ihatecrypto» and drew two
        // receive cards under the live Top up card.
        if let a = allocationPreview { return ("Spread \(a.asset) across the best rates", nil, allocationBubbles(a)) }
        if let y = yieldOptions, !y.isEmpty {
            return ("Rates", nil, y.prefix(8).map { ActivityBubble(kind: "yield", label: $0.asset, title: $0.place, lines: ["\(Fmt.num($0.apy.rounded(2, .plain)))%"], logo: Self.logo(for: $0.place)) })
        }
        if let choices = takeChoices {
            let risk = evaluation.sizing?.lossAtStopUSD
            return ("Take-profit choices", intent.asset, choices.map { ActivityBubble(kind: "choice", label: $0.title, title: risk.map($0.payoff) ?? $0.title, lines: [$0.detail]) })
        }
        if intent != TradeIntent(), !managesPosition {
            // The same cards as on the canvas, so the history and a reopened canvas draw it the same way.
            return ("Plan · \(intent.side == .long ? "long" : "short") \(IntentParser.pretty(intent.asset))", intent.asset,
                    planRoute(StudioNode.build(self)).steps)
        }
        return nil
    }

    /// A spread as route cards: what it takes from each network, the straight moves between them, then into each place
    /// with its rate. The waiting board (AllocationBoard), the route a confirmed spread runs and stays on the canvas as
    /// (Execution.swift) and the history are these same cards, so Done keeps the arrows: route cards are drawn with
    /// arrows only when every card is a step, and the history kinds (free, deposit, result) fell back to a plain grid.
    func allocationBubbles(_ a: AllocationPreview) -> [ActivityBubble] {
        func amount(_ d: Decimal) -> String { Fmt.num(d, maxFraction: a.asset == "SOL" ? 6 : 2) + " " + a.asset }
        let placed = a.slices.reduce(Decimal(0)) { $0 + $1.amount }
        let year = "≈ \(amount(a.yearly))" + (a.yearlyUSD.map { " (≈ \(Fmt.usd($0)))" } ?? "")
        let drawn = a.drawn.isEmpty ? [(place: "Solana", amount: placed)] : a.drawn
        func fill(_ place: String) -> RouteFill { RouteFill(rawValue: place.lowercased().components(separatedBy: " ")[0]) ?? .check }
        let from = drawn.enumerated().map { k, s in
            let there = a.sources.first { $0.place == s.place }?.amount
            return ActivityBubble(kind: "step", label: "FROM · \(s.place.uppercased())", title: amount(s.amount),
                                  lines: k == 0 && a.leftover > 0 ? ["stays free\t\(amount(a.leftover))"] : [],
                                  logo: a.asset == "SOL" ? "sol" : "usdc", fill: fill(s.place),
                                  money: there.map { [MoneyMove(place: s.place, symbol: a.asset, was: $0, after: max(0, $0 - s.amount))] })
        }
        let moves = a.moves.map { m in
            ActivityBubble(kind: "step", label: "BRIDGE · RELAY", title: amount(m.amount), lines: ["\(m.from) → \(m.to)"], logo: "relay", fill: .relay)
        }
        return from + moves + [
            ActivityBubble(kind: "step", label: "INTO · BEST RATES", title: "\(a.slices.count) place\(a.slices.count == 1 ? "" : "s")",
                           lines: ["a year\t\(year)", "max per place\t\(Fmt.pct(a.maxShare))"], logo: "usdc", fill: .jupiter),
        ] + a.slices.map { slice in
            ActivityBubble(kind: "step", label: "INTO · \(slice.place.uppercased())", title: amount(slice.amount),
                           lines: ["rate\t\(Fmt.num(slice.apy.rounded(2, .plain)))% a year"], logo: Self.logo(for: slice.place), fill: .place(slice.place),
                           money: a.asset == "USDC" ? deposited(in: slice.place).map { [MoneyMove(place: slice.place, symbol: "USDC", was: $0, after: $0 + slice.amount)] } : nil)
        }
    }

    /// Freezes the canvas into the current activity (the plan is kept as it was when it was acted on).
    func snapshotCanvas() {
        guard let (title, coin, bubbles) = canvasBubbles() else { return }
        updateActivity { a in
            if a.bubbles.isEmpty || a.kind == .request { a.bubbles = bubbles }
            if a.title == nil { a.title = title }
            if a.coin == nil { a.coin = coin }
        }
    }

    static func logo(for place: String) -> String {
        place.contains("Jupiter") || place.contains("JUP") ? "jup" : place.contains("Kamino") ? "kamino" : place.contains("Jito") ? "jitosol"
            : place.contains("HLP") || place.contains("Hyperliquid") ? "hype" : place.contains("Aave") ? "aave" : place.contains("Morpho") ? "morpho" : "usdc"
    }
}

extension Workspace {
    /// Answers from what ihatecrypto knows: balances, open positions, closed ones from the exchange, recent activity.
    /// With a model the answer is written by it from these facts only; without one, from the same facts directly.
    func answer(_ question: String) async {
        busy = true
        defer { busy = false }
        let stories = (try? await TradeHistory.fills(user: hlAddress)).map(TradeHistory.stories) ?? []
        var facts = answerFacts(stories) + (await pnlFacts())
        // The market the question names (and those of open positions): price, the day's move, funding.
        let named = IntentParser.namedAsset(question, markets: Set(markets.keys))
        // The trade the question is about: the market it names, else the plan's.
        let asked = named ?? (intent != TradeIntent() ? intent.asset : nil)
        facts += Set([asked].compactMap { $0 } + positions.map(\.coin)).compactMap(marketLine)
        // «Should I short oil now?»: that trade by the book and what ihatecrypto's rules say about entering it now.
        let q = question.lowercased()
        let side: Side? = q.contains(/short|шорт|прода|sell/) ? .short : q.contains(/\blong|лонг|купи|buy/) ? .long
            : intent != TradeIntent() ? intent.side : nil
        if let coin = asked, let side, let book = bookPlan(asset: coin, side: side) {
            facts.append("By the book, a \(side == .long ? "long" : "short") \(IntentParser.pretty(coin)) now: \(book.line).")
            facts += book.advice.map { "Rule check (\($0.level == .ok ? "ok" : $0.level == .warn ? "warning" : "blocked")): \($0.text)" }
        }
        if let coin = asked, let c = chart, c.coin == coin, let first = c.candles.first, let last = c.candles.last, first.close > 0 {
            facts.append("\(IntentParser.pretty(coin)) over \(c.range.title): \(Fmt.price(first.close)) → \(Fmt.price(last.close)) (\(signedPct((last.close - first.close) / first.close * 100))).")
        }
        updateActivity { $0.title = "Question" }
        if let client = llmClient() {
            let system = """
            You are ihatecrypto, the user's crypto trading assistant. Answer the question using ONLY the facts below — balances, open positions, \
            closed positions with results, markets, ihatecrypto's rule checks and recent activity. The user may write in any language; answer in \
            \(replyLanguage), in 1–4 short sentences with the numbers. If the facts don't contain the answer, say so and point to History (clock \
            button in the rail). Never invent trades, prices or addresses.
            If the user asks whether to open, close or keep a trade, give a clear view from these facts — the day's move, the chart, \
            volatility, funding and the rule checks — say what would make a better entry (for example a pullback) and what is lost at \
            the stop by the book. The decision is the user's; never promise an outcome.
            """
            do {
                let text = try await client.complete(LLMRequest(model: model, messages: [
                    LLMMessage(.system, system), LLMMessage(.user, "Facts:\n" + facts.joined(separator: "\n") + "\n\nQuestion: " + question),
                ]))
                if !text.isEmpty {
                    reply(text.trimmingCharacters(in: .whitespacesAndNewlines), note: "\(model) · from your portfolio and history")
                    return
                }
            } catch {
                FileHandle.standardError.write(Data("ihatecrypto answer: model failed: \(error)\n".utf8))
            }
        }
        if llmClient() == nil { FileHandle.standardError.write(Data("ihatecrypto answer: no model connected (\(providerID))\n".utf8)) }
        reply(localAnswer(question, stories: stories), note: "from your portfolio and history")
    }

    /// «Market ETH: mark 2,512.3, 24h +1.2%, funding 0.0012% an hour, 4h ATR 1.8%.»
    func marketLine(_ coin: String) -> String? {
        guard let m = markets[coin] else { return nil }
        var line = "Market \(IntentParser.pretty(coin)): mark \(Fmt.price(m.markPx))"
        if let prev = m.prevDayPx, prev > 0 { line += ", 24h \(signedPct((m.markPx - prev) / prev * 100))" }
        if let f = m.funding { line += ", funding \(Fmt.num((f * 100).rounded(4, .plain)))% an hour" }
        if let atr = atr[coin] { line += ", 4h ATR \(Fmt.num(atr.rounded(2, .plain)))%" }
        return line + "."
    }

    func answerFacts(_ stories: [TradeHistory.Story]) -> [String] {
        var facts: [String] = []
        if let s = solana { facts.append("Solana: \(Fmt.num(s.usdc)) USDC, \(Fmt.num(s.sol, maxFraction: 4)) SOL.") }
        if !shownTokens.isEmpty {
            facts.append("Other tokens: " + shownTokens.map { "\(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol)\($0.native ? "" : " (\($0.contract))") on \($0.chain)\($0.usd.map { " ≈ \(Fmt.usd($0))" } ?? "")" }.joined(separator: ", ") + ".")
        }
        if let t = total { facts.append("Main wallet total: \(Fmt.usd(t)), gas included (\(Fmt.usd(solUSD + gasETHUSD)) in SOL and ETH for fees).") }
        if let b = balance { facts.append("Balance on screen (sidebar, portfolio, menu bar; both wallets): \(Fmt.usd(b)).") }
        if let w = automationWallet, let b = automationBudget, let usd = automationUSD {
            let coins = ([b.solana.map { "\(Fmt.num($0)) USDC on Solana" }] + b.tokens.map { "\(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol) on Solana" }
                         + b.evm.filter { $0.value > 0 }.map { "\(Fmt.num($0.value)) USDC on \(EVMNetwork.named($0.key)?.name ?? $0.key)" }).compactMap { $0 }
            facts.append("Automation wallet «\(w.label)» (the user's own, signs the Schedule's tasks; counted in the balance on screen): "
                         + "\(Fmt.usd(usd)) — \(coins.isEmpty ? "nothing" : coins.joined(separator: ", ")); \(Fmt.num(b.sol ?? 0, maxFraction: 4)) SOL for fees.")
        }
        let week = equitySeries()
        if week.count > 2, let first = week.first, let last = week.last {
            facts.append("Portfolio since \(Fmt.date(first.at)): \(Fmt.usd(first.usd)) → \(Fmt.usd(last.usd)) (\(signed(last.usd - first.usd)), top-ups and transfers included).")
        }
        let pnl = pnlSeries()
        if pnl.count > 2, let first = pnl.first, let last = pnl.last {
            facts.append("Portfolio P/L since \(Fmt.date(first.at)) (the card's line; price moves of what was held, top-ups and transfers not counted): \(signed(last.usd - first.usd)).")
        }
        if let h = hlTotal { facts.append("Hyperliquid account: \(Fmt.usd(h)).") }
        for d in lendDeposits {
            let earned = lendEarned[d.address].map { ", earned \(Fmt.num($0 / pow(10, d.decimals), maxFraction: 6)) \(d.coin) so far" } ?? ""
            facts.append("Jupiter Lend: \(Fmt.num(d.amount, maxFraction: 6)) \(d.coin)\(d.apy.map { " at ≈ \(Fmt.num($0.rounded(2, .plain)))% a year" } ?? "")\(earned).")
        }
        if kamino > 0 { facts.append("Kamino Lend: \(Fmt.usd(kamino)).") }
        if near > 0 { facts.append("NEAR on the user's own NEAR account: \(Fmt.num(near, maxFraction: 6)) NEAR (≈ \(Fmt.usd(nearUSD))).") }
        if hlp.equity > 0 { facts.append("HLP vault on Hyperliquid: \(Fmt.usd(hlp.equity))\(hlp.lockedUntil.map { ", locked until \(Fmt.date($0)) (4 days after a deposit)" } ?? ", can be withdrawn").") }
        if let b = base {
            if b.morpho > 0 { facts.append("Morpho vault \(MorphoVault.name) on Base: \(Fmt.num(b.morpho)) USDC\(morphoAPY.map { " at ≈ \(Fmt.num($0))% a year" } ?? "").") }
            facts.append("Aave v3 on Base: \(Fmt.num(b.aave)) USDC supplied\(aaveAPY.map { " at ≈ \(Fmt.num($0.rounded(2, .plain)))% a year" } ?? ""); \(Fmt.num(b.usdc)) USDC idle on Base; \(Fmt.num(b.eth, maxFraction: 7)) ETH for gas there.")
        }
        for p in protocolPositions {
            facts.append("\(p.platform) \(p.name): \(p.amount ?? Fmt.usd(p.usd)) (\(Fmt.usd(p.usd)))\(p.apy.map { " at ≈ \(Fmt.num($0.rounded(2, .plain)))% a year" } ?? ""), deposited on \(p.site) — withdrawn there, not by ihatecrypto's forms.")
        }
        for p in lendingElsewhere {
            facts.append("\(p.place): \(Fmt.num(p.usdc)) USDC\(p.apy.map { " at ≈ \(Fmt.num($0.rounded(2, .plain)))% a year" } ?? "").")
        }
        if positions.isEmpty { facts.append("Open positions: none.") }
        for p in positions {
            facts.append("Open: \(p.size > 0 ? "long" : "short") \(IntentParser.pretty(p.coin)), \(positionSummary(p)), unrealized \(signed(p.unrealizedPnl)).")
        }
        for s in stories.filter({ !$0.isOpen }).prefix(6) {
            let open = s.fills.first, close = s.fills.last
            facts.append("Closed: \(s.long ? "long" : "short") \(IntentParser.pretty(s.coin)), opened \(Fmt.date(s.opened)) at \(open.map { Fmt.price($0.px) } ?? "?"), closed \(s.closed.map(Fmt.date) ?? "?") at \(close.map { Fmt.price($0.px) } ?? "?"), realized \(signed(s.realized)) after fees.")
        }
        if intent != TradeIntent(), !managesPosition {
            let sizing = evaluation.sizing.map { s in
                ", at the stop −\(Fmt.usd(s.lossAtStopUSD))" + (s.profitAtTakeUSD.map { ", at the take +\(Fmt.usd($0))" } ?? "")
            } ?? ""
            facts.append("Plan on the canvas (not executed): \(intent.side == .long ? "long" : "short") \(IntentParser.pretty(intent.asset))\(sizing).")
        }
        // Where money went, as the History board says it: the latest day's moves summed, then each request with what was
        // done (the first line of a «do it» was the swap's quote, and the sends after it never reached the model).
        let moved = activities.filter { $0.stopped != true }.flatMap { a in a.lines.compactMap(MovedLine.init).map { (at: a.at, line: $0) } }
        if let last = moved.last {
            let day = moved.filter { Calendar.current.isDate($0.at, inSameDayAs: last.at) }.map(\.line)
            facts.append("Money moved by ihatecrypto on \(HistoryBoard.day(last.at).lowercased()): " + MovedLine.summary(day).joined(separator: "; ") + ".")
            let own = Set(day.compactMap(\.to)).filter { name in addressBook.contains { $0.name == name && $0.label == "Your wallet" } }
            if !own.isEmpty {
                facts.append("\(own.sorted().joined(separator: ", ")) is the user's own wallet: what went there is still theirs, outside the portfolio total.")
            }
        }
        for a in activities.suffix(12) where (a.request != nil || a.title != nil) && a.stopped != true {
            let done = a.lines.compactMap(MovedLine.init).map(\.text)
            facts.append("Activity \(Fmt.date(a.at)): \(a.request.map { "user said “\($0.prefix(120))”" } ?? a.title ?? "")"
                         + (done.isEmpty ? " → \((a.lines.first ?? "").prefix(200))" : " → done: \(done.joined(separator: "; "))"))
        }
        // Names only: a model never sees or writes an address.
        if !addressBook.isEmpty {
            facts.append("Address book (names only; the addresses are in the Addresses tab): " + addressBook.map { "\($0.name) — \([$0.label?.lowercased(), $0.network.title].compactMap { $0 }.joined(separator: ", "))\($0.favorite ? ", favorite" : "")" }.joined(separator: "; ") + ".")
        }
        if let r = robinhood {
            let stocks = r.stocks.map { "\(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol) (\($0.name)) ≈ \(Fmt.usd($0.usd))" }.joined(separator: ", ")
            facts.append("Robinhood Chain: \(stocks.isEmpty ? "no Stock Tokens" : "Stock Tokens " + stocks); \(Fmt.num(r.usdg)) USDG.")
        }
        for t in favoriteTokens {
            let i = tokenInfo[t.id]
            facts.append("Favorite token \(t.name) (\(t.symbol)) on \(t.chainTitle): \(i?.priceUSD.map(tokenPrice) ?? "no price")\(i?.change24h.map { ", \(signedPct($0)) in 24 h" } ?? "")\(tokenBalances[t.id].map { ", you hold \(Fmt.num($0, maxFraction: 4))" } ?? "").")
        }
        for a in alerts { facts.append("Price alert: \(IntentParser.pretty(a.coin)) \(a.above ? "above" : "below") \(Fmt.price(a.price))\(markets[a.coin].map { ", now \(Fmt.price($0.markPx))" } ?? "").") }
        for s in schedules { facts.append("Recurring buy: \(s.symbol) for \(Fmt.usd(s.usdc)) every \(s.days) days, next \(Fmt.date(s.next)).") }
        return facts
    }

    /// Without a model: the questions that matter most — where is a position, what happened to it, what is open.
    private func localAnswer(_ question: String, stories: [TradeHistory.Story]) -> String {
        if let coin = IntentParser.namedAsset(question, markets: Set(markets.keys)) {
            if let p = positions.first(where: { $0.coin == coin }) {
                return "Your \(p.size > 0 ? "long" : "short") \(IntentParser.pretty(coin)) is open: \(positionSummary(p)), unrealized \(signed(p.unrealizedPnl))."
            }
            if let s = stories.first(where: { $0.coin == coin && !$0.isOpen }), let close = s.fills.last {
                return "The \(s.long ? "long" : "short") \(IntentParser.pretty(coin)) is closed: it closed \(s.closed.map(Fmt.date) ?? "") at \(Fmt.price(close.px)), realized \(signed(s.realized)) after fees. The whole story is in History → Positions."
            }
            return "There is no \(IntentParser.pretty(coin)) position, open or closed, on Hyperliquid."
        }
        if positions.isEmpty {
            let last = stories.first { !$0.isOpen }
            return "No open positions." + (last.map { " The last one: \($0.long ? "long" : "short") \(IntentParser.pretty($0.coin)), realized \(signed($0.realized))." } ?? "")
        }
        return "Open: " + positions.map { "\($0.size > 0 ? "long" : "short") \(IntentParser.pretty($0.coin)) \(signed($0.unrealizedPnl))" }.joined(separator: ", ") + ". For anything else, connect a model in Settings."
    }
}

/// «+1.2%» / «−0.4%».
func signedPct(_ d: Decimal) -> String { (d < 0 ? "−" : "+") + Fmt.num(abs(d).rounded(2, .plain)) + "%" }

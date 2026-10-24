import Foundation
import IhatecryptoCore
import WalletCore

/// A block on a canvas: a move's route (waiting, running, done), a result the chat made (a chart, the rates, approvals,
/// P/L, a token), or a live block the person added (the portfolio, Receive, the address book…). The newest is on top, and
/// nothing leaves a canvas unless the person closes it.
nonisolated struct Board: Codable, Equatable, Identifiable {
    /// One card of a move's route: «BUY · Solana», what it does, and the balance it changes, now → after.
    struct Step: Codable, Equatable {
        var label: String
        var title: String
        var lines: [String] = []
        var money: Money?
        /// «then»: a step after this move, the model's next one (ChatTurns.boardsShown), drawn pale; nil for the move's own.
        var status: String?
    }
    /// Read from the balances on screen when the route was drawn, never from the step's words.
    struct Money: Codable, Equatable {
        var place: String
        var symbol: String
        var was: String
        var after: String
    }
    /// A button on a block: it puts the command in the chat field, and the person sends it; it never moves money itself.
    struct Action: Codable, Equatable {
        var title: String
        var command: String
    }
    var id = UUID().uuidString
    /// «move», «result», «chart», or a live block: portfolio, receive, addresses, history, favorites, alerts, schedule,
    /// positions, multisig.
    var kind: String
    var title: String
    var lines: [String] = []
    var steps: [Step]?
    /// A move's: waiting, running, done, failed, cancelled.
    var state: String?
    var chart: ChartLine?
    var actions: [Action]?
    var at = Date()
    /// What the route signs and how it's checked, as the Mac's MoveRoute footer: «Why this way?» quotes it.
    var note: String? = nil
    /// A trade plan's levels and numbers, for drawing it on its coin's chart (RouteEdit.swift).
    var trade: TradeView? = nil
    /// A note's card shown on another canvas: the note canvas «Open» goes to (ChatCommands.folderCommand).
    var opens: String? = nil
    /// What a result board draws, as data (Blocks.swift); `lines` stay what the model reads about it.
    var data: BlockData? = nil
    /// The message that drew it: Refresh on a board left from before asks it again (the Mac's StudioNodeView).
    var request: String? = nil
    /// A trade plan's route extras: what stops it and its fix, the take-profit ways, the market picker (TradePlan.swift).
    var plan: PlanView? = nil
}

/// One canvas: its own chat and its own blocks, as on the Mac.
nonisolated struct CanvasDoc: Codable, Equatable, Identifiable {
    var id = UUID().uuidString
    /// Its automatic title (the first message's words); the list shows the newest move's title over it, and the
    /// person's own `name` over both (the Mac's CanvasSession.title and .name).
    var title: String
    var name: String?
    var messages: [Message] = []
    var boards: [Board] = []
    var at = Date()
    /// The canvas list's (Folders.swift): pinned on top since, its folder, its picture, the model it was talked through.
    var pinnedAt: Date?
    var folder: String?
    var icon: CanvasIcon?
    var provider: String?
    var model: String?
    /// A scheduled item's own canvas: «task:<id>» or «perp:<coin>» (Schedule.swift).
    var schedule: String?
    /// What its chat was in when the person left it, back when they come back and across a restart (the Mac's
    /// CanvasSession): the trade plan, its board and the plan a trade came from, the skills loaded, the last reply's cards.
    var intent: TradeIntent?
    var planBoard: String?
    var tradedPlan: TradeIntent?
    var skills: [String]?
    var offers: [ChatRouter.Option]?
}

extension Engine {
    static let liveBoards = ["portfolio", "receive", "addresses", "history", "favorites", "alerts", "schedule", "positions", "multisig", "ledger"]

    /// Canvases from canvases.json; the chat of a build before canvases becomes the first one.
    static func loadCanvases() -> [CanvasDoc] {
        if var saved = Store.load([CanvasDoc].self, "canvases.json"), !saved.isEmpty {
            // A card can't wait across a restart, and a move that was running then may or may not have gone through.
            for c in saved.indices {
                for i in saved[c].boards.indices {
                    switch saved[c].boards[i].state {
                    case "waiting":
                        saved[c].boards[i].state = "cancelled"
                        saved[c].boards[i].lines.append("ihatecrypto restarted before Confirm: nothing was signed.")
                    case "running":
                        saved[c].boards[i].state = "failed"
                        saved[c].boards[i].lines.append("ihatecrypto stopped while this ran: check History before doing it again.")
                    default: break
                    }
                }
            }
            return saved
        }
        let old = Store.load([Message].self, "chat.json") ?? []
        return [CanvasDoc(title: old.first(where: \.fromUser).map { String($0.text.prefix(40)) } ?? "Canvas", messages: old)]
    }

    var canvasIndex: Int { canvases.firstIndex { $0.id == canvasID } ?? 0 }

    var boards: [Board] {
        get { canvases[canvasIndex].boards }
        set { canvases[canvasIndex].boards = newValue; saveCanvases(); changed() }
    }

    func saveCanvases() {
        keepCanvasState()
        keepBoardTitle()
        // The History keeps its own copy, so nothing said leaves it with a canvas (HistoryStore).
        HistoryStore.record(canvases)
        var kept = canvases
        for i in kept.indices { kept[i].messages = Array(kept[i].messages.suffix(300)) }
        Store.save(kept, "canvases.json")
    }

    /// A block on top of the canvas. A live block that's already there moves to the top instead of showing twice.
    @discardableResult
    func pin(_ board: Board) -> String {
        var board = board
        // A move keeps the request that drew it: Refresh asks it again once it's left unsigned (Blocks.swift).
        if board.kind == "move", board.request == nil { board.request = lastRequest }
        var list = boards
        if Self.liveBoards.contains(board.kind), let i = list.firstIndex(where: { $0.kind == board.kind }) {
            var b = list.remove(at: i)
            b.at = Date()
            list.insert(b, at: 0)
            boards = list
            return b.id
        }
        list.insert(board, at: 0)
        // Nothing leaves the canvas (the Mac's EarlierBoards keeps every finished move): no cap.
        boards = list
        return board.id
    }

    func pin(_ kind: String, _ title: String, lines: [String] = [], chart: ChartLine? = nil, actions: [Board.Action]? = nil, data: BlockData? = nil) {
        var b = Board(kind: kind, title: title, lines: lines, chart: chart, actions: actions)
        b.data = data
        // The request that drew it, not a «yes» or «cancel» said after.
        b.request = lastRequest
        pin(b)
    }

    func updateBoard(_ id: String?, _ change: (inout Board) -> Void) {
        guard let id else { return }
        for c in canvases.indices {
            if let i = canvases[c].boards.firstIndex(where: { $0.id == id }) {
                change(&canvases[c].boards[i])
                saveCanvases()
                changed()
                return
            }
        }
    }

    // MARK: Canvases

    /// Another canvas may open: nothing is being answered or carried out (the Mac's canSwitchCanvas), so a reply or a
    /// move's outcome can't land on a canvas it wasn't asked on. The chat's own «new canvas» passes `fromChat`.
    var canSwitchCanvas: Bool { !thinking && running == nil && runningBoard == nil }

    func newCanvas(fromChat: Bool = false) throws {
        try leaveCanvas(fromChat: fromChat)
        forgetUnkept()
        let c = CanvasDoc(title: say("New canvas", "Новый холст"))
        canvases.insert(c, at: 0)
        canvasID = c.id
        afterSwitch()
    }

    func openCanvas(_ id: String) throws {
        guard id != canvasID, canvases.contains(where: { $0.id == id }) else { return }
        try leaveCanvas()
        forgetUnkept()
        canvasID = id
        afterSwitch()
    }

    /// The person's own name for a canvas; an empty one gives the automatic title back (the Mac's renameCanvas).
    func renameCanvas(_ id: String, _ title: String) {
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let i = canvases.firstIndex(where: { $0.id == id }) else { return }
        canvases[i].name = t.isEmpty ? nil : String(t.prefix(80))
        saveCanvases()
    }

    func removeCanvas(_ id: String) throws {
        guard let i = canvases.firstIndex(where: { $0.id == id }) else { return }
        declineSite(onCanvas: id == canvasID ? boards : canvases[i].boards)
        let wasOpen = id == canvasID
        if wasOpen { try leaveCanvas() }
        // Its plan's price alert goes with it (the Mac's Canvases.swift:361).
        alerts.removeAll { $0.plan?.uuidString == id }
        canvases.remove(at: i)
        // The open one gives way to a new blank canvas, as the Mac's deleteCanvas opens newCanvas(), not another.
        if wasOpen || canvases.isEmpty {
            canvases.insert(CanvasDoc(title: say("New canvas", "Новый холст")), at: 0)
            canvasID = canvases[0].id
            afterSwitch()
        }
        saveCanvases()
    }

    /// A running move stays where it was started; a waiting card stays on its canvas, marked as not confirmed. What the
    /// chat was in stays with the canvas (keepCanvasState).
    private func leaveCanvas(fromChat: Bool = false) throws {
        guard running == nil, runningBoard == nil else { throw WalletError(say("A move is running: wait for it to finish.", "Идёт перевод: дождись, пока он закончится.")) }
        guard fromChat || !thinking else {
            throw WalletError(say("The chat is still answering: wait for it, or press Stop.", "Чат ещё отвечает: подожди или нажми «Стоп»."))
        }
        if card != nil, !parkPending() {
            updateBoard(card?.id) { $0.state = "cancelled"; $0.lines.append(say("Left without Confirm: nothing was signed.", "Ушёл без «Подтвердить»: ничего не подписано.")) }
            pending = nil
            card = nil
        }
        keepCanvasState()
        options = []
        awaitingAmount = nil
        intent = TradeIntent()
        planBoard = nil
        tradedPlan = nil
        loadedSkills = []
    }

    private func afterSwitch() {
        restoreCanvasState()
        unparkPending()
        settings.canvas = canvasID
        saveCanvases()
        changed()
    }

    /// The open canvas keeps its plan, skills and live cards (saved with canvases.json).
    func keepCanvasState() {
        let i = canvasIndex
        canvases[i].intent = intent == TradeIntent() ? nil : intent
        canvases[i].planBoard = planBoard
        canvases[i].tradedPlan = tradedPlan
        canvases[i].skills = loadedSkills.isEmpty ? nil : loadedSkills
        // The last reply's cards while none is picked and nothing was said after them.
        let last = canvases[i].messages.last
        canvases[i].offers = !options.isEmpty && last?.id == optionsMessage && last?.picked == nil ? options : nil
    }

    /// Back on a canvas (or at launch): its plan, skills, model and the last reply's cards as they were; a chart older
    /// than five minutes read again, as the Mac reloads a stale snapshot (Canvases.load).
    func restoreCanvasState() {
        let c = canvases[canvasIndex]
        intent = c.intent ?? TradeIntent()
        planBoard = c.planBoard.flatMap { id in c.boards.contains { $0.id == id } ? id : nil }
        tradedPlan = c.tradedPlan
        loadedSkills = c.skills ?? []
        if let offers = c.offers, let last = c.messages.last, !last.fromUser, last.picked == nil, !offers.isEmpty { options = offers }
        // The model the canvas was talked through, while that account is still connected (C3).
        if let id = c.provider, id != providerID, let p = provider(id), p.info.unavailable == nil, Store.credentials()[id] != nil || p.info.kind == .local {
            settings.provider = id
            if let m = c.model { setModel(m, for: id) }
        }
        let stale = c.boards.filter { $0.kind == "chart" && $0.chart?.coin != nil && Date().timeIntervalSince($0.at) > 300 }.prefix(3)
        for b in stale { Task { await reloadChart(b.id) } }
    }

    /// A chart board drawn again from fresh candles, in its own range; it keeps its place and time on the canvas.
    func reloadChart(_ id: String) async {
        guard let b = canvases.lazy.flatMap(\.boards).first(where: { $0.id == id }), let coin = b.chart?.coin else { return }
        let range = ChartCommand.Range.allCases.first { b.title.hasSuffix($0.title) } ?? .day
        guard var line = try? await chartLine(coin, range: range) else { return }
        line.network = b.chart?.network
        updateBoard(id) { $0.chart = line }
    }

    /// The first thing said on a canvas names it, as the Mac's canvas list does.
    func nameCanvas(from text: String) {
        let i = canvasIndex
        guard canvases[i].messages.filter(\.fromUser).count == 1, canvases[i].title == say("New canvas", "Новый холст") || canvases[i].title == "New canvas" || canvases[i].title == "Новый холст" else { return }
        canvases[i].title = String(text.prefix(40))
    }

    /// A block in words, as the model gets it: its title, its state, what it shows.
    func blockFacts(_ b: Board) -> String {
        let live: [String] = switch b.kind {
        case "portfolio": portfolioFacts()
        case "addresses": book.map { "\($0.name) on \($0.network)" }
        case "alerts": alerts.map(alertText)
        case "schedule": tasks.map(taskText)
        case "favorites": favorites.map { "\($0.symbol) on \($0.chain)" }
        case "multisig": multisigs.map { "\($0.address): any \($0.threshold) of \($0.keys.map(\.name).joined(separator: ", ")) on \($0.networks.joined(separator: ", "))" }
        case "history": history().prefix(10).map { "\($0.note) (\($0.chain))" }
        case "scheduleItem": scheduleItemFacts(b.title)
        case "recurring": recurringFacts(b.title)
        case "note": ["\(b.opens == nil ? "this canvas is" : "a card opens") \(b.lines.count == 1 ? "a note" : "\(b.lines.count) notes") in folder «\(b.title)»"]
        default: []
        }
        let steps = (b.steps ?? []).map { s in "\(s.label): \(s.title)" + (s.money.map { " (\($0.place) \($0.was) → \($0.after) \($0.symbol))" } ?? "") }
        // A chart: its range in numbers, and the live price now when Hyperliquid has the market.
        let chart = b.chart.map { c -> [String] in
            let coin = String(c.title.split(separator: " ").first ?? "")
            let live = markets[coin].map { ", live price now \(Fmt.price($0.markPx))" } ?? ""
            return ["low \(c.low), high \(c.high), \(c.change) over the range\(live)"]
        } ?? []
        return "«\(b.kind == "move" || b.kind == "result" || b.kind == "chart" ? b.title : b.kind)»" + (b.state.map { " [\($0)]" } ?? "") + ": "
            + (steps + chart + b.lines + live).prefix(14).joined(separator: "; ")
    }

    // MARK: A move's route

    /// The card's steps: one per step of a chain, else the command's own, with the balance it spends.
    func steps(for pending: Pending, title: String, lines: [String]) -> [Board.Step] {
        switch pending {
        case .chain(let list):
            // As the Mac draws a chain (Canvases.swift chainSteps): the first move as its whole route, each later one as
            // its route without its own start and approve, pale «then»; no bridge where a move before brings the money.
            var arrived = Set<String>(), first = true
            // And how much: a later step is drawn paid as it will be, by the balances the moves before it leave (the Mac's
            // `ahead`): «buy on Base» after «move 10 to Base» pays on Base, not Solana's ways.
            var ahead: [String: Decimal] = [:]
            func count(_ c: WalletCommand) {
                let sent: Decimal?? = switch c {
                case .bridge(_, _, let a), .cctp(_, _, let a), .evmToSolana(_, let a), .solanaTo(_, let a), .baseToSolana(let a),
                     .arbitrumToSolana(let a), .robinhoodToSolana(let a), .withdrawToSolana(let a): a
                default: nil
                }
                if let sent, let from = Self.source(c), let moved = sent ?? usdcFree(on: from) {
                    ahead[from, default: 0] -= moved
                    if let to = Self.arrives(c) { ahead[to, default: 0] += moved }
                } else if let spent = Self.solanaUSDC(c) {
                    ahead["solana", default: 0] -= spent
                }
            }
            func route(_ c: WalletCommand) -> [Board.Step]? {
                guard !ahead.isEmpty else { return routeSteps(c) }
                let kept = portfolio
                defer { portfolio = kept }
                for (net, delta) in ahead {
                    let dollar = net == "robinhood" ? "USDG" : "USDC"
                    if portfolio.networks.firstIndex(where: { $0.key == net }) == nil {
                        portfolio.networks.append(PortfolioView.Network(key: net, name: WalletCommand.placeName(net), usd: 0, rows: []))
                    }
                    let n = portfolio.networks.firstIndex { $0.key == net }!
                    if let r = portfolio.networks[n].rows.firstIndex(where: { $0.section == "tokens" && $0.symbol == dollar && $0.contract == nil }) {
                        portfolio.networks[n].rows[r].value = max(0, portfolio.networks[n].rows[r].value + delta)
                    } else if delta > 0 {
                        portfolio.networks[n].rows.append(Self.row(dollar, delta, usd: delta))
                    }
                }
                return routeSteps(c)
            }
            func later(_ steps: [Board.Step], spends: String?) -> [Board.Step] {
                let brought = spends.map(arrived.contains) ?? false
                let skip = ["FROM", "ОТКУДА", "APPROVE", "РАЗРЕШЕНИЕ"] + (brought ? ["BRIDGE", "МОСТ", "GAS", "ГАЗ"] : [])
                let kept = steps.filter { s in !skip.contains { s.label.hasPrefix($0) } }
                return (kept.isEmpty ? steps : kept).map { var s = $0; s.status = "then"; return s }
            }
            return list.flatMap { step -> [Board.Step] in
                defer { first = false }
                switch step {
                case .wallet(let c):
                    defer { if let to = Self.arrives(c) { arrived.insert(to) }; count(c) }
                    let steps = (first ? routeSteps(c) : route(c)) ?? [Board.Step(label: label(c), title: c.summary, money: money(c))]
                    return first ? steps : later(steps, spends: Self.spends(c))
                // From its own network, with that network's balance, to the saved name (MoveRoutes.swift).
                case .transfer(let e, let amount):
                    return first ? transferSteps(e, amount) : later(transferSteps(e, amount), spends: e.network)
                case .said(let text): return [saidStep(text)]
                }
            }
        case .wallet(let c), .recurring(let c, _):
            return routeSteps(c) ?? [Board.Step(label: label(c), title: title, lines: lines, money: money(c))]
        case .transfer(let entry, let amount): return transferSteps(entry, amount)
        case .position(let command, let coin):
            if case .close(let share) = command, let steps = closeSteps(share, coin: coin) { return steps }
            return [Board.Step(label: "HYPERLIQUID", title: title, lines: lines)]
        case .orders: return [Board.Step(label: "HYPERLIQUID", title: title, lines: lines)]
        case .trade: return [Board.Step(label: say("TRADE · Hyperliquid", "СДЕЛКА · Hyperliquid"), title: title, lines: lines)]
        case .address(let e): return [Board.Step(label: say("SAVE · ", "СОХРАНИТЬ · ") + networkName(e.network), title: title, lines: lines)]
        }
    }

    private func label(_ c: WalletCommand) -> String {
        switch c {
        case .multisigCreate: return say("MULTISIG · Safe", "МУЛЬТИСИГ · Safe")
        case .multisigSend(_, _, _, let net): return say("PAY · ", "ОПЛАТА · ") + networkName(net ?? multisigs.last?.networks.first ?? "base")
        default: break
        }
        let verb = c.summary.split(separator: " ").first.map { $0.uppercased() } ?? "MOVE"
        return verb + " · " + networkName(place(c) ?? "solana")
    }

    private func networkName(_ key: String) -> String {
        portfolio.networks.first { $0.key == key }?.name ?? AddressCheck.Network(rawValue: key)?.title ?? WalletCommand.placeName(key)
    }

    /// Where a command's money comes from, as a portfolio network key.
    private func place(_ c: WalletCommand) -> String? {
        switch c {
        case .buyOn(let p, _, _, let from): from ?? p
        case .swapOn(let p, _, _, _), .sellOn(let p, _, _, _): p
        case .aave(_, _, let net), .morpho(_, _, let net): net
        case .sendToken(_, _, _, let net): net ?? "solana"
        case .hlp, .hlSpotBuy, .hlSpotSell: "hyperliquid"
        default: nil
        }
    }

    /// What the command spends, read against the balance on screen: «Solana · 45.97 → 35.97 USDC».
    private func money(_ c: WalletCommand) -> Board.Money? {
        switch c {
        case .buy(_, let u?): money(spending: u, "USDC", on: "solana")
        case .sell(let a, let x?): money(spending: x, a.symbol, on: "solana")
        case .lendDeposit(let s, let x?), .kamino(true, let s, let x?): money(spending: x, s, on: "solana")
        case .stake(let x?): money(spending: x, "SOL", on: "solana")
        case .solanaTo(_, let x?): money(spending: x, "USDC", on: "solana")
        case .sendSol(_, let x): money(spending: x, "SOL", on: "solana")
        case .hlp(true, let x?): money(spending: x, "USDC", on: "hyperliquid")
        case .aave(true, let x?, let net), .morpho(true, let x?, let net): money(spending: x, "USDC", on: net)
        case .buyOn(let p, _, let u?, let from): money(spending: u, "USDC", on: from ?? p)
        case .swapOn(let p, let from, _, let x?): money(spending: x, from, on: p)
        case .sellOn(let p, let t, let x?, _): money(spending: x, t, on: p)
        case .sendToken(_, let t, let x?, let net): money(spending: x, t, on: net ?? "solana")
        default: nil
        }
    }

    func money(spending amount: Decimal, _ symbol: String, on place: String) -> Board.Money? {
        guard let net = portfolio.networks.first(where: { $0.key == place }),
              let row = net.rows.first(where: { $0.symbol.uppercased() == symbol.uppercased() }) else { return nil }
        return Board.Money(place: net.name, symbol: row.symbol, was: Self.amount(row.value, row.symbol), after: Self.amount(max(row.value - amount, 0), row.symbol))
    }
}

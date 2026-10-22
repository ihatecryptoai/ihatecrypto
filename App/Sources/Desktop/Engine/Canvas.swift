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
}

/// One canvas: its own chat and its own blocks, as on the Mac.
nonisolated struct CanvasDoc: Codable, Equatable, Identifiable {
    var id = UUID().uuidString
    var title: String
    var messages: [Message] = []
    var boards: [Board] = []
    var at = Date()
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
        var kept = canvases
        for i in kept.indices { kept[i].messages = Array(kept[i].messages.suffix(300)) }
        Store.save(kept, "canvases.json")
    }

    /// A block on top of the canvas. A live block that's already there moves to the top instead of showing twice.
    @discardableResult
    func pin(_ board: Board) -> String {
        var list = boards
        if Self.liveBoards.contains(board.kind), let i = list.firstIndex(where: { $0.kind == board.kind }) {
            var b = list.remove(at: i)
            b.at = Date()
            list.insert(b, at: 0)
            boards = list
            return b.id
        }
        list.insert(board, at: 0)
        boards = Array(list.prefix(60))
        return board.id
    }

    func pin(_ kind: String, _ title: String, lines: [String] = [], chart: ChartLine? = nil, actions: [Board.Action]? = nil) {
        pin(Board(kind: kind, title: title, lines: lines, chart: chart, actions: actions))
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

    func newCanvas() throws {
        try leaveCanvas()
        let c = CanvasDoc(title: say("New canvas", "Новый холст"))
        canvases.insert(c, at: 0)
        canvasID = c.id
        afterSwitch()
    }

    func openCanvas(_ id: String) throws {
        guard id != canvasID, canvases.contains(where: { $0.id == id }) else { return }
        try leaveCanvas()
        canvasID = id
        afterSwitch()
    }

    func renameCanvas(_ id: String, _ title: String) {
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, let i = canvases.firstIndex(where: { $0.id == id }) else { return }
        canvases[i].title = String(t.prefix(60))
        saveCanvases()
    }

    func removeCanvas(_ id: String) throws {
        guard let i = canvases.firstIndex(where: { $0.id == id }) else { return }
        if id == canvasID { try leaveCanvas() }
        canvases.remove(at: i)
        if canvases.isEmpty { canvases = [CanvasDoc(title: say("Canvas", "Холст"))] }
        if !canvases.contains(where: { $0.id == canvasID }) { canvasID = canvases[0].id; afterSwitch() }
        saveCanvases()
    }

    /// A running move stays where it was started; a waiting card stays on its canvas, marked as not confirmed.
    private func leaveCanvas() throws {
        guard running == nil else { throw WalletError(say("A move is running: wait for it to finish.", "Идёт перевод: дождись, пока он закончится.")) }
        if card != nil {
            updateBoard(card?.id) { $0.state = "cancelled"; $0.lines.append(say("Left without Confirm: nothing was signed.", "Ушёл без «Подтвердить»: ничего не подписано.")) }
            pending = nil
            card = nil
        }
        options = []
        awaitingAmount = nil
        intent = TradeIntent()
        loadedSkills = []
    }

    private func afterSwitch() {
        settings.canvas = canvasID
        saveCanvases()
        changed()
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
        default: []
        }
        let steps = (b.steps ?? []).map { s in "\(s.label): \(s.title)" + (s.money.map { " (\($0.place) \($0.was) → \($0.after) \($0.symbol))" } ?? "") }
        return "«\(b.kind == "move" || b.kind == "result" || b.kind == "chart" ? b.title : b.kind)»" + (b.state.map { " [\($0)]" } ?? "") + ": "
            + (steps + b.lines + live).prefix(14).joined(separator: "; ")
    }

    // MARK: A move's route

    /// The card's steps: one per step of a chain, else the command's own, with the balance it spends.
    func steps(for pending: Pending, title: String, lines: [String]) -> [Board.Step] {
        switch pending {
        case .chain(let list):
            return list.map { step in
                switch step {
                case .wallet(let c): Board.Step(label: label(c), title: c.summary, money: money(c))
                case .transfer(_, let amount): Board.Step(label: say("SEND · ", "ПЕРЕВОД · ") + networkName("solana"), title: step.title,
                                                              money: money(spending: amount, "USDC", on: "solana"))
                }
            }
        case .wallet(let c), .recurring(let c, _):
            return [Board.Step(label: label(c), title: title, lines: lines, money: money(c))]
        case .transfer(_, let amount):
            return [Board.Step(label: say("SEND · ", "ПЕРЕВОД · ") + networkName("solana"), title: title, lines: lines, money: money(spending: amount, "USDC", on: "solana"))]
        case .position: return [Board.Step(label: "HYPERLIQUID", title: title, lines: lines)]
        case .trade: return [Board.Step(label: say("TRADE · Hyperliquid", "СДЕЛКА · Hyperliquid"), title: title, lines: lines)]
        case .address(let e): return [Board.Step(label: say("SAVE · ", "СОХРАНИТЬ · ") + networkName(e.network), title: title, lines: lines)]
        case .site: return [Board.Step(label: say("SITE · browser", "САЙТ · браузер"), title: title, lines: lines)]
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
        return Board.Money(place: net.name, symbol: row.symbol, was: Fmt.num(row.value, maxFraction: 6), after: Fmt.num(max(row.value - amount, 0), maxFraction: 6))
    }
}

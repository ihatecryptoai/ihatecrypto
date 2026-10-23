import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution

/// A route step's menu and details (the Mac's RouteCards: «Why this way?», «Edit step…»): what of the waiting move can
/// change — its amount, and for a trade on an EVM network the way (Relay's swap, or Relay's bridge + Uniswap) — and the
/// answer from the step's own facts, which goes in the chat without the model.
nonisolated enum RouteEdit {
    /// What the window may change in the move waiting for Confirm: the amount as the command holds it and what it's in,
    /// and whether the way can be picked (and which one is).
    struct Edit: Encodable, Equatable {
        var board: String
        var amount: String?
        var unit: String
        var ways: Bool
        var way: String?
        /// Both ways as quoted now, the better first; nil while they're still being asked.
        var quotes: [Way]?
    }

    /// One way of an EVM trade as quoted: «relay» or «uniswap» (or «here» on the network's own USDC), what arrives,
    /// and its worth in dollars after gas.
    struct Way: Encodable, Equatable {
        var way: String
        var gets: String
        var worth: String
    }

    /// The field as one amount: «2», «0,5», «1 000», «1,000.50», «1.000,50». Anything else is nil, never the number it
    /// starts with («5abc» isn't 5), and so is «1,000» alone: a thousand, or one with three decimals. (RouteCards.amount)
    static func amount(_ text: String) -> Decimal? {
        var s = text.trimmingCharacters(in: .whitespaces)
        // Thousands apart by spaces: «1 000», «12 500,5».
        if s.wholeMatch(of: /\d{1,3}(?:\s\d{3})+(?:[.,]\d+)?/) != nil { s.removeAll { $0.isWhitespace } }
        // Thousands apart by the other mark: with a decimal mark after them, or more than one of them.
        if s.wholeMatch(of: /\d{1,3}(?:,\d{3})+(?:\.\d+)?/) != nil, s.contains(/\.|,.*,/) { s.removeAll { $0 == "," } }
        else if s.wholeMatch(of: /\d{1,3}(?:\.\d{3})+(?:,\d+)?/) != nil, s.contains(/,|\..*\./) { s.removeAll { $0 == "." } }
        guard let m = s.wholeMatch(of: /(\d+)(?:([.,])(\d+))?/) else { return nil }
        if m.2 == ",", m.3?.count == 3, m.1.contains(where: { $0 != "0" }) { return nil }
        return Decimal(string: s.replacingOccurrences(of: ",", with: "."))
    }

    /// The way key of a trade on an EVM network (Workspace.wayKey): «base:aero»; nil for any other move.
    static func wayKey(_ command: WalletCommand) -> String? {
        switch command {
        case .buyOn(let place, let token, _, _), .sellOn(let place, let token, _, _): "\(place):\(token.lowercased())"
        default: nil
        }
    }

    /// What the amount is in: USDC for most moves, SOL to stake, the token for a deposit.
    static func unit(_ command: WalletCommand) -> String {
        switch command {
        case .stake: "SOL"
        case .lendDeposit(let symbol, _), .kamino(_, let symbol, _): symbol
        default: "USDC"
        }
    }

    /// «BRIDGE · RELAY» → «Bridge · Relay» (StepDetails.pretty).
    static func pretty(_ label: String) -> String {
        label.lowercased().capitalized.replacingOccurrences(of: " Or ", with: " or ").replacingOccurrences(of: "Usdc", with: "USDC")
            .replacingOccurrences(of: "Jitosol", with: "JitoSOL").replacingOccurrences(of: "Hlp", with: "HLP").replacingOccurrences(of: "V3", with: "v3")
            .replacingOccurrences(of: "Layerzero", with: "LayerZero").replacingOccurrences(of: "Cctp", with: "CCTP")
    }

    /// The step's facts as «Why this way?» quotes them: its title, the balance it changes, its lines.
    static func facts(_ step: Board.Step) -> String {
        ([step.title] + (step.money.map { ["\($0.place) \($0.was) → \($0.after) \($0.symbol)"] } ?? [])
         + step.lines.filter { !$0.isEmpty }.map { $0.replacingOccurrences(of: "\t", with: " ") }).joined(separator: " · ")
    }
}

extension Engine {
    /// The waiting move's step that can change, as the window shows it; nil when nothing can (no move waits, it's a
    /// chain or a site's request, or the command has no amount and no way).
    var stepEdit: RouteEdit.Edit? {
        guard case .wallet(let command)? = pending, let id = card?.id else { return nil }
        let key = RouteEdit.wayKey(command)
        guard command.editableAmount != nil || key != nil else { return nil }
        return RouteEdit.Edit(board: id, amount: command.editableAmount.map { NSDecimalNumber(decimal: $0).stringValue },
                              unit: RouteEdit.unit(command), ways: key != nil, way: key.flatMap { k in routeWay?.key == k ? routeWay?.way : nil },
                              quotes: wayQuotes?.command == command ? wayQuotes?.ways : nil)
    }

    /// Both ways of a Base or Arbitrum trade quoted while its card waits (the Mac's quoteSwap): once, asked again every
    /// 15 s while it fails, until the card goes. Nothing is signed; the executor quotes again right before signing.
    func quoteWays(_ command: WalletCommand) async {
        let (place, token, usdc, sell, from, keep): (String, String, Decimal?, Decimal?, String?, Bool)
        switch command {
        case .buyOn(let p, let t, let u?, let f): (place, token, usdc, sell, from, keep) = (p, t, u, nil, f, false)
        case .sellOn(let p, let t, let a, let k): (place, token, usdc, sell, from, keep) = (p, t, nil, a, nil, k)
        default: return
        }
        guard let net = EVMNetwork.named(place), let e = try? executor() else { return }
        while case .wallet(let waiting)? = pending, waiting == command {
            if let ways = try? await e.swapQuotes(net, token, usdc: usdc, sell: sell, from: from, keep: keep), !ways.isEmpty {
                guard case .wallet(let still)? = pending, still == command else { return }
                wayQuotes = (command, ways.map { RouteEdit.Way(way: $0.way, gets: $0.gets, worth: Fmt.usd($0.worth)) })
                changed()
                return
            }
            try? await Task.sleep(for: .seconds(15))
        }
    }

    /// Save in a step's details (StepDetails.save): the way is kept for this move; a new amount is a new plan, through
    /// the same checks and the same Confirm as if typed.
    func editStep(board: String?, amount text: String?, way: String?) async throws {
        guard case .wallet(let command)? = pending, let id = card?.id, board == nil || board == id else {
            throw ExecutionError(say("That move isn't waiting for Confirm any more.", "Этот перевод уже не ждёт подтверждения."))
        }
        var edited: WalletCommand?
        if let old = command.editableAmount, let text {
            guard let value = RouteEdit.amount(text), value > 0 else {
                throw ExecutionError(say("Type the amount as one number, for example 2.5 or 1000.", "Напиши сумму одним числом, например 2,5 или 1000."))
            }
            if value != old { edited = command.with(amount: value) }
        }
        if let key = RouteEdit.wayKey(command) {
            routeWay = (way == "relay" || way == "uniswap") ? (key, way!) : nil
        }
        if let edited { await handleWallet(edited) } else { changed() }
    }

    /// The way picked on the route's step for this trade, taken once: Confirm runs with it, the next move without.
    func takeWay(_ place: String, _ token: String) -> String? {
        guard let picked = routeWay, picked.key == "\(place):\(token.lowercased())" else { return nil }
        routeWay = nil
        return picked.way
    }

    /// «Why this way?»: the step's own facts in the chat, without the model; a move waiting for Confirm keeps waiting.
    func whyStep(board: String?, index: Int?) throws {
        guard let b = boards.first(where: { $0.id == board }), let steps = b.steps, let i = index, steps.indices.contains(i) else {
            throw ExecutionError(say("That step isn't on the canvas any more.", "Этого шага на холсте уже нет."))
        }
        let s = steps[i], name = RouteEdit.pretty(s.label)
        reply(say("Why this way — \(name): \(RouteEdit.facts(s)).", "Почему так — \(name): \(RouteEdit.facts(s))."), note: "from the route")
    }
}

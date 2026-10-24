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
        /// A Robinhood Chain trade: its ways are compared and the best is taken, nothing to pick (ROUTES · BEST IS TAKEN).
        var stock = false
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
        /// How much less than the best this way leaves, for a Robinhood Chain trade's list; nil for the best.
        var less: String? = nil
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
        guard command.editableAmount != nil || key != nil || command.isStockTrade else { return nil }
        return RouteEdit.Edit(board: id, amount: command.editableAmount.map { NSDecimalNumber(decimal: $0).stringValue },
                              unit: RouteEdit.unit(command), ways: key != nil, stock: command.isStockTrade,
                              way: key.flatMap { k in routeWay?.key == k ? routeWay?.way : nil },
                              quotes: wayQuotes?.command == command ? wayQuotes?.ways : nil)
    }

    /// Both ways of a Base or Arbitrum trade quoted while its card waits (the Mac's quoteSwap): once, asked again every
    /// 15 s while it fails, until the card goes. Nothing is signed; the executor quotes again right before signing.
    func quoteWays(_ command: WalletCommand) async {
        if command.isStockTrade { await quoteStock(command); return }
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
                // The best way and what it gets, on the step it names (MoveRoutes.swift).
                if let id = card?.id { redrawRoute(id, command) } else { changed() }
                return
            }
            try? await Task.sleep(for: .seconds(15))
        }
    }

    /// A Robinhood Chain trade's ways while its card waits (the Mac's quoteStock): Uniswap v4, v3 and Relay's swap, the
    /// best first, which Confirm takes; its step says the best and what the others give (stockBuyStep).
    func quoteStock(_ command: WalletCommand) async {
        let (symbol, usdc, sell): (String, Decimal?, Decimal?)
        switch command {
        case .buyStock(let s, let u?): (symbol, usdc, sell) = (s, u, nil)
        case .sellStock(let s, let a): (symbol, usdc, sell) = (s, nil, a)
        default: return
        }
        guard let e = try? executor() else { return }
        while case .wallet(let waiting)? = pending, waiting == command {
            if let ways = try? await e.stockQuotes(symbol, usdc: usdc, sell: sell), let best = ways.first {
                guard case .wallet(let still)? = pending, still == command, let id = card?.id else { return }
                wayQuotes = (command, ways.enumerated().map { i, q in
                    RouteEdit.Way(way: q.way, gets: q.gets, worth: Fmt.usd(q.worth), less: i == 0 ? nil : Fmt.usd(best.worth - q.worth))
                })
                // The best names the BUY (or SELL) step, the others are its line (MoveRoutes.swift, the Mac's stockBuyStep).
                redrawRoute(id, command)
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
        // A trade plan keeps its footer in its plan view (the Mac's footer from the plan's route, RouteCards.swift:247).
        let footer = b.note ?? b.plan.flatMap { $0.footer.isEmpty ? nil : $0.footer }
        reply(say("Why this way — \(name): \(RouteEdit.facts(s)).", "Почему так — \(name): \(RouteEdit.facts(s)).") + (footer.map { " " + $0 } ?? ""), note: "from the route")
    }
}

/// A trade plan as its coin's chart draws it (the Mac's PlanChartBoard, the owner's pick C, 2026-10-23: «план нарисован
/// на чарте … при трединге»): the entry, stop and take across the line with the loss and the gain between them, and the
/// trade's numbers under it. Prices as numbers for the drawing, the rest as the card says it.
nonisolated struct TradeView: Codable, Equatable {
    var coin: String
    var side: String
    var leverage: String
    /// A limit order's price rather than the market's now («Limit» against «Entry ≈»).
    var limit: Bool
    var entry: Double
    var stop: Double
    var take: Double?
    var notional: String
    var size: String
    var margin: String
    var liquidation: String
    var lossAtStop: String
    /// The loss at the stop as a share of the portfolio, «1.2»; nil without a portfolio read.
    var lossPct: String?
    var profitAtTake: String?
    /// The coin's price alerts, drawn as dashed lines.
    var alerts: [Double]
    /// When a card's numbers were read, seconds since 1970: «numbers refreshed N s ago» (the Mac's Execute sheet).
    var prepared: Double? = nil
}

nonisolated extension RouteEdit {
    /// Which waiting moves the window gets prices for: a Base or Arbitrum trade's two ways, a Robinhood Chain trade's.
    static func quoted(_ command: WalletCommand) -> Bool { wayKey(command) != nil || command.isStockTrade }

    /// A Robinhood Chain trade's way by name (Workspace.stockWayName).
    static func stockWayName(_ way: String) -> String { way == "v4" ? "Uniswap v4" : way == "v3" ? "Uniswap v3" : "Relay swap" }
}

nonisolated extension WalletCommand {
    var isStockTrade: Bool {
        switch self {
        case .buyStock, .sellStock: true
        default: false
        }
    }
}

extension Engine {
    /// The trade plan's numbers on its board, for its chart (Moves.plan calls this after its card is shown).
    func drawTrade(_ board: String?, _ intent: TradeIntent, _ sizing: Sizing?, market: Market?) {
        guard let board, let s = sizing else { return }
        func d(_ x: Decimal) -> Double { NSDecimalNumber(decimal: x).doubleValue }
        let pct = ruleEquity.flatMap { $0 > 0 ? Fmt.num((s.lossAtStopUSD / $0 * 100).rounded(1, .plain)) : nil }
        let view = TradeView(coin: intent.asset, side: intent.side == .long ? "long" : "short", leverage: Fmt.num(intent.leverage ?? 1),
                             limit: intent.entryPx != nil, entry: d(s.entry), stop: d(s.stopPrice), take: s.takePrice.map(d),
                             notional: Fmt.usd(s.notional),
                             size: "\(Fmt.num(s.size, maxFraction: market?.szDecimals ?? 4)) \(IntentParser.pretty(intent.asset))",
                             margin: Fmt.usd(s.margin), liquidation: "≈ \(Fmt.price(s.liquidationPrice))", lossAtStop: "−\(Fmt.usd(s.lossAtStopUSD))",
                             lossPct: pct, profitAtTake: s.profitAtTakeUSD.map { "+\(Fmt.usd($0))" },
                             alerts: alerts.filter { $0.coin == intent.asset }.map { d($0.price) })
        var drawn = view
        if case .trade(let p)? = pending, card?.id == board { drawn.prepared = p.preparedAt.timeIntervalSince1970 }
        updateBoard(board) { $0.trade = drawn }
    }
}

nonisolated extension RouteEdit {
    /// What the route signs and how it's checked, as the Mac's MoveRoute footers say it (Canvases.swift): «Why this
    /// way?» and a finished step's details quote it. nil where the card says it itself (a payment, a call, a multisig).
    static func note(_ c: WalletCommand, say: (String, String) -> String) -> String? {
        func name(_ key: String) -> String { WalletCommand.placeName(key) }
        let confirm = say(" Press Confirm.", " Нажми «Подтвердить».")
        let drops = say(" Press Confirm; Cancel or “cancel” drops it.", " Нажми «Подтвердить»; «Отмена» или «отмена» в чате её снимет.")
        switch c {
        case .withdrawToSolana:
            return say("Signed with the main key — the trading key can’t withdraw.", "Подписывает основной ключ — торговый ключ выводить не может.") + drops
        case .aave(true, let x?, let n), .morpho(true, let x?, let n):
            let morpho = if case .morpho = c { true } else { false }
            return say("What's missing on \(name(n)) comes from Solana over Relay first; then two \(name(n)) transactions: approve exactly \(Fmt.num(x)) USDC, then \(morpho ? "deposit; the shares come to this wallet only" : "supply"). \(morpho ? "The vault's asset is" : "Pool and rate are") re-read from \(name(n)) first.",
                       "Если на \(name(n)) не хватает, недостающее сначала придёт с Solana через Relay; потом две транзакции на \(name(n)): разрешение ровно на \(Fmt.num(x)) USDC и \(morpho ? "депозит — доли приходят только на этот кошелёк" : "вклад"). \(morpho ? "Актив хранилища" : "Пул и ставка") перечитываются с \(name(n)) перед этим.") + confirm
        case .aave(false, _, let n), .morpho(false, _, let n):
            let what = if case .morpho = c { "Morpho" } else { "Aave" }
            return say("Three \(name(n)) transactions: withdraw from \(what), an exact approve and Relay’s deposit — checked field by field before signing.",
                       "Три транзакции на \(name(n)): вывод из \(what), точное разрешение и депозит Relay — каждое поле проверяется перед подписью.") + confirm
        case .evmToSolana(let n, _):
            return say("Two \(name(n)) transactions: an exact approve and Relay’s deposit — checked field by field before signing.",
                       "Две транзакции на \(name(n)): точное разрешение и депозит Relay — каждое поле проверяется перед подписью.") + confirm
        case .baseToSolana, .arbitrumToSolana, .robinhoodToSolana:
            let n = if case .baseToSolana = c { "base" } else if case .arbitrumToSolana = c { "arbitrum" } else { "robinhood" }
            return note(.evmToSolana(n, nil), say: say)
        case .solanaTo(let n, _):
            return say("One Solana signature for the Relay bridge, checked and simulated before it’s signed; it pays only your own address on \(name(n)).",
                       "Одна подпись в Solana для моста Relay, с проверкой и симуляцией перед подписью; платит только на твой же адрес в \(name(n)).") + confirm
        case .bridge(_, let to, _), .cctp(_, let to, _):
            let cctp = if case .cctp = c { true } else { false }
            return (cctp ? say("Circle's CCTP: an exact approve and the burn (one transaction where it batches); Circle mints on \(name(to)) itself, fast-transfer fee plus its gas, a minute or so. ",
                               "CCTP от Circle: точное разрешение и сжигание (одной транзакцией, где сеть это умеет); Circle сам выпускает на \(name(to)), комиссия быстрого перевода плюс газ, около минуты. ")
                    : say("An exact approve and the bridge's deposit, checked field by field before signing: Relay, unless NEAR Intents' 1Click or Circle's CCTP lands more right then. ",
                          "Точное разрешение и депозит моста, каждое поле проверяется перед подписью: Relay, если только 1Click от NEAR Intents или CCTP от Circle не доставят больше прямо сейчас. "))
                + say("It pays only your own address on \(name(to)).", "Платит только на твой же адрес в \(name(to)).") + confirm
        case .usdt(_, let to, _, let layerZero):
            return (layerZero ? say("Tether's own bridge, USDT0 on LayerZero: one call sends the USDT with LayerZero's fee in the network's coin, and the same amount is minted on \(name(to)) later — in transit on the portfolio until then. ",
                                    "Собственный мост Tether, USDT0 на LayerZero: один вызов отправляет USDT с комиссией LayerZero в монете сети, а та же сумма выпускается на \(name(to)) позже — до тех пор она «в пути» в портфеле. ")
                    : say("Relay's swap of USDT for USDT: an exact approve and its deposit, checked before signing, seconds. Tether's own bridge on LayerZero moves it for cents of gas but takes longer: say «via LayerZero» for it. ",
                          "Обмен USDT на USDT через Relay: точное разрешение и депозит, проверка перед подписью, секунды. Собственный мост Tether на LayerZero берёт центы газа, но идёт дольше: скажи «через LayerZero». "))
                + say("It pays only your own address on \(name(to)).", "Платит только на твой же адрес в \(name(to)).") + confirm
        case .gas(let n, let usd):
            return say("One Solana signature for the Relay bridge: \(Fmt.usd(usd ?? 1)) of the gas coin and half a dollar of USDC arrive at your own address on \(name(n)). When Solana is short, Relay swaps \(name(n))'s own USDC for it there.",
                       "Одна подпись в Solana для моста Relay: \(Fmt.usd(usd ?? 1)) монеты для газа и полдоллара USDC придут на твой же адрес в \(name(n)). Если в Solana не хватает, Relay обменяет на неё USDC самой \(name(n)).") + confirm
        case .buyStock(let s, _):
            // A token by its contract isn't Robinhood's registered one (the Mac's stockRoute footer).
            return say("Two signatures on Robinhood Chain (with USDG short there, the bridge comes first): an exact USDG approve and the swap the best way takes (Uniswap v4, v3 or Relay’s swap, compared in the step), which may only pay this wallet. ",
                       "Две подписи в Robinhood Chain (если USDG там не хватает, сначала мост): точное разрешение на USDG и обмен лучшим путём (Uniswap v4, v3 или обмен Relay — сравнение в шаге), который может платить только этому кошельку. ")
                + (s.hasPrefix("0x") ? say("The token is taken by its contract: it must trade on Robinhood Chain with $50k+ of liquidity, and the price must sit within 5% of DexScreener's.",
                                           "Токен взят по контракту: он должен торговаться в Robinhood Chain с ликвидностью от $50 тыс., а цена — не дальше 5% от DexScreener.")
                                     : say("The token is Robinhood's registered contract, checked on-chain, and the price must sit within 5% of the market.",
                                           "Токен — зарегистрированный контракт Robinhood, проверенный в сети, а цена не дальше 5% от рынка.")) + confirm
        case .sellStock:
            return say("Two Robinhood Chain transactions: an exact approve and the Uniswap swap, which may only pay this wallet.",
                       "Две транзакции в Robinhood Chain: точное разрешение и обмен в Uniswap, который может платить только этому кошельку.") + drops
        case .buyOn(let n, let token, _, _), .sellOn(let n, let token, _, _):
            let buy = if case .buyOn = c { true } else { false }
            return (buy ? say("Relay’s swap is one Solana signature; the other way bridges USDC over Relay and swaps on Uniswap with an exact approve, the swap paying only this wallet. ",
                              "Обмен Relay — одна подпись в Solana; другой путь везёт USDC по мосту Relay и меняет в Uniswap с точным разрешением, обмен платит только этому кошельку. ")
                        : say("Relay’s swap is an exact approve and its deposit on \(name(n)); the other way swaps on Uniswap with an exact approve, then bridges the USDC over Relay, paying only this wallet. ",
                              "Обмен Relay — точное разрешение и депозит на \(name(n)); другой путь меняет в Uniswap с точным разрешением, потом везёт USDC по мосту Relay, платя только этому кошельку. "))
                + (token.hasPrefix("0x") ? say("A token by its contract must trade on \(name(n)) with $50k+ of liquidity.", "Токен по контракту должен торговаться на \(name(n)) с ликвидностью от $50 тыс.")
                                         : say("The ticker is taken from Uniswap’s token list, its contract checked on-chain.", "Тикер берётся из списка токенов Uniswap, контракт проверяется в сети."))
                + confirm
        case .swapOn("solana", _, _, _):
            return say("One swap on Solana through Jupiter's best route, no USDC in the middle: simulated before signing, and at least Jupiter's minimum must arrive in this wallet. SOL keeps 0.01 for fees.",
                       "Один обмен в Solana лучшим путём Jupiter, без USDC посередине: симуляция перед подписью, и на этот кошелёк должно прийти не меньше минимума Jupiter. На комиссии остаётся 0,01 SOL.") + confirm
        case .swapOn("near", _, let to, _):
            return say("One deposit to the 1Click address NEAR Intents binds to this quote — checked with 1Click before signing — and \(to.uppercased()) comes back to your own NEAR account; a refund goes there too.",
                       "Один депозит на адрес 1Click, который NEAR Intents привязывает к этой котировке, — сверяется с 1Click перед подписью, — и \(to.uppercased()) возвращается на твой аккаунт NEAR; туда же и возврат.") + confirm
        case .swapOn("hyperliquid", let from, let to, _):
            return say("Two spot orders on Hyperliquid's book, each IOC within 1% of the mid: \(from.uppercased()) sold for USDC, that USDC buys \(to.uppercased()). The book takes orders from 10 USDC.",
                       "Два спотовых ордера в стакане Hyperliquid, каждый IOC в пределах 1% от середины: \(from.uppercased()) продаётся за USDC, на эти USDC покупается \(to.uppercased()). Ордера — от 10 USDC.") + confirm
        case .swapOn(let n, _, _, _):
            return say("One swap on \(name(n)), no USDC in the middle: the best of Uniswap v2, v3 and v4, at most 0.5–3% under its quote by the price impact, paying only this wallet.",
                       "Один обмен на \(name(n)), без USDC посередине: лучший из Uniswap v2, v3 и v4, не хуже котировки больше чем на 0,5–3% по влиянию на цену, платит только этому кошельку.") + confirm
        case .crossSwap(_, let from, let b, let to, _):
            return say("One signature on \(name(from)): Relay's solver swaps on the way and pays \(b.uppercased()) to your own address on \(name(to)), usually within seconds. If more than 3% (or 15 cents) of the value would be lost, nothing is signed.",
                       "Одна подпись на \(name(from)): решатель Relay обменивает по пути и платит \(b.uppercased()) на твой же адрес в \(name(to)), обычно за секунды. Если потерялось бы больше 3% (или 15 центов) стоимости, ничего не подписывается.") + confirm
        case .hlSpotBuy:
            return say("One spot order on Hyperliquid's book, IOC within 1% of the mid; the spot USDC comes from your perp balance first when short (one more signature of yours). The exchange takes orders from 10 USDC.",
                       "Один спотовый ордер в стакане Hyperliquid, IOC в пределах 1% от середины; если спотовых USDC не хватает, они сначала придут с баланса фьючерсов (ещё одна твоя подпись). Биржа берёт ордера от 10 USDC.") + confirm
        case .hlSpotSell:
            return say("One spot order on Hyperliquid's book, IOC within 1% of the mid; the USDC lands on your spot balance. The exchange takes orders from 10 USDC.",
                       "Один спотовый ордер в стакане Hyperliquid, IOC в пределах 1% от середины; USDC придут на спотовый баланс. Биржа берёт ордера от 10 USDC.") + confirm
        case .revokeApproval(let n, _, let spender, let permit2):
            return say("One \(name(n)) transaction: \(permit2 ? "Permit2's own approve" : "the token's approve") set to zero for \(spender). Nothing is spent and nobody gains a right; only the gas is paid.",
                       "Одна транзакция на \(name(n)): \(permit2 ? "разрешение Permit2" : "разрешение токена") для \(spender) обнуляется. Ничего не тратится, и никто не получает прав; платится только газ.") + confirm
        case .sweepDust(let under):
            return say("Each verified token worth $0.20 to \(Fmt.usd(under ?? 5)) on Base, Arbitrum and Polygon: quoted on Uniswap first and sold for USDC on its own network with an exact approve, only when the gas takes less than half of it — the rest stays, said by name.",
                       "Каждый проверенный токен стоимостью от $0,20 до \(Fmt.usd(under ?? 5)) на Base, Arbitrum и Polygon: сначала котировка в Uniswap, потом продажа за USDC в своей сети с точным разрешением, только если газ съедает меньше половины — остальное остаётся, по названию.") + confirm
        case .buyNear:
            return say("One transfer: exactly this USDC to the 1Click deposit address the quote names, checked before signing. Native NEAR arrives on your NEAR account in ≈ 30–50 s.",
                       "Один перевод: ровно эти USDC на адрес депозита 1Click из котировки, с проверкой перед подписью. NEAR придёт на твой аккаунт NEAR за ≈ 30–50 с.") + confirm
        case .sellNear(_, let to):
            return say("One NEAR transaction: exactly this NEAR, wrapped and sent to the 1Click deposit address the quote names, checked before signing. USDC arrives at your own address on \(name(to ?? "solana")) in ≈ 30–40 s.",
                       "Одна транзакция NEAR: ровно этот NEAR, обёрнутый и отправленный на адрес депозита 1Click из котировки, с проверкой перед подписью. USDC придут на твой же адрес в \(name(to ?? "solana")) за ≈ 30–40 с.") + confirm
        case .sendNear:
            return say("One NEAR transfer, built and signed by the signer; only this saved address may receive it.", "Один перевод NEAR, его собирает и подписывает подписант; получить его может только этот сохранённый адрес.") + confirm
        case .closeNear(let n):
            return say("One NEAR transaction: your NEAR account is deleted and NEAR sends everything on it to \(n); buying NEAR later opens the account again.",
                       "Одна транзакция NEAR: твой аккаунт NEAR удаляется, и всё, что на нём, уходит на \(n); если потом купить NEAR, аккаунт откроется снова.") + confirm
        case .sendSol:
            return say("One Solana transfer: only this saved address may receive it, and the simulation must show exactly this SOL leaving plus the fee.",
                       "Один перевод в Solana: получить его может только этот сохранённый адрес, а симуляция должна показать, что уходит ровно этот SOL плюс комиссия.") + confirm
        case .sendToken(_, _, _, let n):
            return say("One transfer on \(name(n ?? "solana")), the token as it is: only this saved address may receive it.", "Один перевод в \(name(n ?? "solana")), токен как есть: получить его может только этот сохранённый адрес.") + confirm
        case .aaveRepay:
            return say("An approve of what's owed (a tenth of a percent over, for the interest of the next blocks), then the repay — of this wallet's own debt only.",
                       "Разрешение на сумму долга (на десятую процента больше, на проценты следующих блоков), потом погашение — только собственного долга этого кошелька.") + confirm
        case .aaveCollateral:
            return say("ETH wrapped into WETH, an exact approve, the supply to this wallet: it earns a little and can be borrowed against.",
                       "ETH оборачивается в WETH, точное разрешение, вклад на этот кошелёк: приносит немного и может быть залогом.") + confirm
        case .aaveBorrow:
            return say("One transaction: Aave lends USDC to this wallet against what it supplied there. If the health factor reaches 1, Aave sells the collateral at a penalty; ihatecrypto won't borrow below 1.5. Pay it back with “repay all USDC on Aave”.",
                       "Одна транзакция: Aave даёт этому кошельку USDC в долг под то, что на нём лежит. Если показатель здоровья дойдёт до 1, Aave продаст залог со штрафом; ihatecrypto не берёт в долг ниже 1,5. Вернуть: «верни весь USDC на Aave».") + confirm
        case .lendDeposit, .lendWithdraw, .kamino, .stake, .unstake, .hlp, .buy, .sell, .jupiterDCA, .jupiterDCACancel:
            return say("One Solana signature, checked and simulated before it’s signed.", "Одна подпись в Solana, с проверкой и симуляцией перед подписью.") + drops
        default:
            return nil
        }
    }
}

extension Engine {
    /// The route's note on its board when its card is shown (Moves.show calls this).
    func noteRoute(_ board: String, _ pending: Pending) {
        let note: String? = switch pending {
        case .wallet(let c), .recurring(let c, _): routeNote(c)
        case .transfer(let e, let amount): transferNote(e, amount)
        // A chain's card reads as its first move's (the Mac draws the pending action's route and footer), then the chain's
        // own words from its card (Chains.swift): which step asks how much, and that they run in order.
        case .chain(let list):
            Optional(([list.lazy.compactMap { step -> String? in
                switch step {
                case .wallet(let c): self.routeNote(c)
                case .transfer(let e, let amount): self.transferNote(e, amount)
                case .said: nil
                }
            }.first].compactMap { $0 } + (card?.id == board ? card?.lines ?? [] : []).filter { $0.wholeMatch(of: /\d+\. .*/) == nil })
                .joined(separator: " ")).flatMap { $0.isEmpty ? nil : $0 }
        // An address to save from the chat (the Mac's PendingAddressBoard): where it came from and what Save asks.
        case .address(let e): addressNote(e)
        default: nil
        }
        if case .address = pending, card?.id == board { card?.confirm = say("Save", "Сохранить") }
        if case .wallet(let c) = pending { nameTokens(board, c) }
        guard let note else { return }
        updateBoard(board) { $0.note = note }
    }
}

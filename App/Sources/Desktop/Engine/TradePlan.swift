import EVMChain
import Foundation
import HyperliquidSigning
import IhatecryptoCore
import LLMCore
import TradeExecution
import WalletCore

/// A trade plan as the Mac draws it (Studio.swift planRoute, Canvases.swift planBlocker/planFix, TradeAdvice.swift):
/// every message about it redraws its route — FROM, BRIDGE, CHECK RULES, SECOND CHECK, OPEN, PROTECT, SIMULATION, SIGN,
/// RESULT — and the chat checks it item by item. A plan the rules stop stays on the canvas with the one change that
/// passes; one that passes becomes the card waiting for Confirm in its place.
nonisolated struct PlanView: Codable, Equatable {
    /// The plan's market and its group, for the picker in its header.
    var market: String
    var group: String
    /// Why it can't go, as a sentence and in a few words for its status; nil when it can.
    var stopped: String?
    var short: String?
    /// A rule stops it (red); false while it only waits for a detail such as the stop (orange).
    var blocked = false
    /// The one change that passes every rule, the $10 minimum and the money (Plan.fix), or «By the book».
    var fix: Board.Action?
    var fixDetail: String?
    /// The per-trade risk limit is what stops it: Settings › Trading agent can raise it, at the user's own risk.
    var overLimit = false
    /// «Take profit like the pros»: the three ways, while asked for.
    var takes: [TakeChoice]?
    /// What Confirm does, under the route.
    var footer: String
    /// Watched addresses: the simulation is shown and nothing can sign here (the Mac's Preview).
    var preview = false
}

nonisolated struct TakeChoice: Codable, Equatable {
    var id: String
    var title: String
    var detail: String
    var note: String
    /// What it pays at the take: «+$2.00», or «no cap».
    var payoff: String
    var recommended: Bool
}

/// The plan's dry bridge quote and the EVM network the USDC leaves from when Solana is short (nil: Solana).
nonisolated struct PlanQuote: Sendable {
    var quote: BridgeQuote
    var from: EVMNetwork?
}

/// Signs nothing: a plan for watched addresses is prepared and simulated, as the Mac's Preview, and stops there.
nonisolated struct PreviewSigner: TradeSigner {
    let refusal: String
    func signSolana(_ wire: Data, accounts: [String], recipients: [String], tokenOwners: [String: String]) async throws -> Data { throw ExecutionError(refusal) }
    func signHyperliquid(_ action: MsgPack, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError(refusal) }
    func signHyperliquidUser(_ action: MsgPack, recipients: [String]) async throws -> EVMSignature { throw ExecutionError(refusal) }
    func signRelayMapping(wallet: String, depositor: String, id: Data, nonce: UInt64) async throws -> EVMSignature { throw ExecutionError(refusal) }
    func signEVM(chainId: UInt64, nonce: UInt64, maxPriorityFee: UInt64, maxFee: UInt64, gasLimit: UInt64,
                 to: String, data: Data, recipients: [String]) async throws -> Data { throw ExecutionError(refusal) }
    func signNearIntentDeposit(amount: UInt128, depositAddress: String, nonce: UInt64, blockHash: Data) async throws -> Data { throw ExecutionError(refusal) }
}

extension Engine {
    // MARK: The numbers the plan is checked with

    /// ATR(14) of the 4-hour candles, for a stop by the book: kept ten minutes, asked again when Hyperliquid refuses
    /// (it does when many readers ask at once).
    func planATR(_ coin: String) async -> Decimal? {
        if let kept = Self.atrKept[coin], Date().timeIntervalSince(kept.at) < 600 { return kept.pct }
        let v = await Self.retried { try await Hyperliquid.atrPct(coin: coin) }
        if let v { Self.atrKept[coin] = (v, Date()) }
        return v
    }
    static var atrKept: [String: (pct: Decimal, at: Date)] = [:]

    /// Hyperliquid's markets, read once and asked again on a refusal, for the plan and its picker.
    func planMarketsLoaded() async {
        if markets.isEmpty, let read = await Self.retried({ try await Hyperliquid.markets() }) { markets = read }
    }

    /// Up to three tries while the refusals come back quickly; a request that hung its whole timeout isn't asked again,
    /// so the chat never waits more than one timeout and a few seconds.
    nonisolated static func retried<T: Sendable>(_ read: @Sendable () async throws -> T) async -> T? {
        let start = Date()
        for attempt in 1...3 {
            if let v = try? await read() { return v }
            guard attempt < 3, Date().timeIntervalSince(start) < 8 else { break }
            try? await Task.sleep(for: .milliseconds(400 * attempt))
        }
        return nil
    }

    /// What Plan.blocks reads besides the plan: the balances, the market, the open risk, the rules.
    func planLive(market: Market?, notes: [String], quoted: PlanQuote? = nil) -> Live {
        var live = Live()
        live.quote = quoted?.quote
        live.from = quoted?.from.map { ($0.name, usdcFree(on: $0.key) ?? 0) }
        live.equity = ruleEquity
        live.policy = policy
        live.openRisk = HyperliquidAccount.riskAtStops(positions: positions, orders: hlOrders)
        live.solanaAddressSet = solanaAddress != nil
        live.solanaUSDC = usdcFree(on: "solana")
        live.hlAddressSet = evmAddress != nil
        // A HIP-3 market can use what is free on its own exchange as well as the main balance.
        live.hlWithdrawable = placeFacts.hyperliquid.map { hl in hl.withdrawable + (market?.dex.flatMap { d in hl.dexes.first { $0.dex == d }?.withdrawable } ?? 0) }
        live.marketsLoaded = !markets.isEmpty
        live.market = market
        live.notes = notes
        return live
    }

    /// The bridge as the plan would take it, quoted dry — no address is sent (the Mac's quoteIfNeeded): what goes over,
    /// what arrives, the fee, and the EVM network the USDC leaves from when Solana is short. Kept a minute for the same
    /// amount and the same USDC on Solana; nil when nothing needs bridging or no bridge answered.
    func planQuote(_ r: TradeIntent, _ e: Evaluation, market: Market?) async -> PlanQuote? {
        let solana = usdcFree(on: "solana")
        let amount: Bridge.Amount? = if let a = r.amountUSDC { .send(a) }
            else if let need = Plan.bridgeNeed(r, e, hlWithdrawable: planLive(market: market, notes: []).hlWithdrawable, mark: market?.markPx) {
                .arrive(Executor.bridgeArrive(need: need, margin: e.sizing?.margin ?? 0, solanaUSDC: solana))
            } else { nil }
        guard let amount else { return nil }
        let key = "\(amount) \(solana ?? 0) \(openVia1Click == true)"
        if let kept = Self.quoteKept, kept.key == key, Date().timeIntervalSince(kept.at) < 60 { return kept.quoted }
        // A new Hyperliquid account pays a one-time opening fee on its first deposit; the plan shows it up front.
        let newAccount = if let evm = evmAddress { (try? await Hyperliquid.isNewAccount(evm)) ?? false } else { false }
        guard var q = try? await Bridge.dryQuote(amount, newAccount: newAccount, oneClickFirst: newAccount && openVia1Click == true) else { return nil }
        // The executor picks the same way, from the same quote.
        let net = q.amountIn > (solana ?? 0) ? EVMNetwork.funding(q.amountIn) { usdcFree(on: $0) } : nil
        if let net, let other = try? await Bridge.dryQuote(.send(q.amountIn), newAccount: newAccount, from: (net.relayChain, net.usdc, net.usdcDecimals)) { q = other }
        let quoted = PlanQuote(quote: q, from: net)
        Self.quoteKept = (key, quoted, Date())
        return quoted
    }
    static var quoteKept: (key: String, quoted: PlanQuote, at: Date)?

    /// Why a plan can't go, in a few words for its status and a sentence for the chat (Canvases.swift planBlocker); nil
    /// when nothing stops it or a detail is still missing (that has its own «By the book»).
    func planBlocker(_ e: Evaluation, _ blocks: [Block], equity: Decimal?, from: String = "Solana") -> (short: String, sentence: String)? {
        guard e.missing.isEmpty else { return nil }
        if let s = e.sizing, s.notional < 10, e.problem != nil {
            return (say("order under the $10 minimum", "ордер меньше $10"),
                    say("The order is \(Fmt.usd(s.notional)), under Hyperliquid’s $10 minimum.", "Ордер \(Fmt.usd(s.notional)) — меньше минимума Hyperliquid $10."))
        }
        if let p = e.problem { return (say("the position can’t be placed", "позицию не выставить"), p) }
        if let source = blocks.first(where: { $0.id == "source" }), source.status == .blocked, let short = source.lines.last?.dropLast() {
            return (say("not enough USDC", "не хватает USDC"),
                    say("\(short) on \(from).", "\(short.replacingOccurrences(of: "Short by", with: "Не хватает")) на \(from)."))
        }
        if let s = e.sizing, let equity, equity > 0, s.lossAtStopUSD / equity * 100 > policy.maxRiskPct {
            return (say("risk over the \(Fmt.pct(policy.maxRiskPct)) limit", "риск выше лимита \(Fmt.pct(policy.maxRiskPct))"),
                    say("At the stop \(Fmt.usd(s.lossAtStopUSD)) is lost, over the limit of \(Fmt.pct(policy.maxRiskPct)) of capital (\(Fmt.usd(equity * policy.maxRiskPct / 100))).",
                        "На стопе теряется \(Fmt.usd(s.lossAtStopUSD)) — больше лимита \(Fmt.pct(policy.maxRiskPct)) капитала (\(Fmt.usd(equity * policy.maxRiskPct / 100)))."))
        }
        guard let stopped = blocks.first(where: { $0.status == .blocked }) else { return nil }
        let line = stopped.lines.first { $0.hasPrefix("✗") }.map { String($0.dropFirst(2)) } ?? stopped.lines.first ?? stopped.title
        return (say("a rule stops it", "его стопорит правило"), line)
    }

    /// For a plan the rules stop: the one change that passes every rule, the $10 minimum and the money (Plan.fix), said as
    /// what changes in the trade.
    func planFix(_ raw: TradeIntent, market m: Market, now: Sizing?) -> (title: String, detail: String, command: String)? {
        // ponytail: Solana's USDC (or an EVM network's, the richest) less a bridge-fee cushion, plus what's free on
        // Hyperliquid, as the Mac counts it; Confirm's preparation quotes it exactly.
        let richest = portfolio.networks.filter { $0.key != "hyperliquid" }.compactMap { usdcFree(on: $0.key) }.max() ?? 0
        let free = planLive(market: m, notes: []).hlWithdrawable ?? 0
        let available = max(0, richest - Decimal(string: "0.1")!) + free
        guard let f = Plan.fix(raw, market: m, equity: ruleEquity, policy: policy,
                               openRisk: HyperliquidAccount.riskAtStops(positions: positions, orders: hlOrders), available: available) else { return nil }
        let s = f.sizing
        let title = f.command.hasSuffix("USDC")
            ? say("Put \(f.command) in the trade", "Вложить \(f.command) в сделку")
            : (now.map { s.notional > $0.notional } ?? true)
                ? say("Raise the order to \(Fmt.usd(s.notional))", "Поднять ордер до \(Fmt.usd(s.notional))")
                : say("Cut the order to \(Fmt.usd(s.notional))", "Уменьшить ордер до \(Fmt.usd(s.notional))")
        let was = now.map { " " + say("instead of", "вместо") + " −\(Fmt.usd($0.lossAtStopUSD))" } ?? ""
        return (title, say("At the stop −\(Fmt.usd(s.lossAtStopUSD))\(was), margin \(Fmt.usd(s.margin)) — passes every rule.",
                           "На стопе −\(Fmt.usd(s.lossAtStopUSD))\(was), маржа \(Fmt.usd(s.margin)) — проходит все правила."), f.command)
    }

    // MARK: The checklist

    /// The plan checked item by item, the way its route checks it (TradeAdvice.planChecklist): market, direction, risk,
    /// stop, take, entry, size, money, the rules and the result, then the one next step. Lines start with ✓ ✗ ⚠ ·, and
    /// the chat draws them as a checklist card. `ready`: the card waits for Confirm.
    /// `failed`: the preparation's refusal, said in the next step's place.
    func planChecklist(_ asked: TradeIntent, _ r: TradeIntent, _ e: Evaluation, _ blocks: [Block], market: Market?, atrRead: Bool, ready: Bool,
                       quoted: PlanQuote? = nil, failed: String? = nil) -> String {
        let sizing = e.sizing, equity = ruleEquity
        var trial = asked
        trial.autoRules = true
        let book = Plan.resolve(trial, market: market, equity: equity, policy: policy).intent
        func share(_ v: Decimal) -> String {
            equity.map { $0 > 0 ? say(" (\(Fmt.pct((v / $0 * 100).rounded(1, .plain))) of capital)", " (\(Fmt.pct((v / $0 * 100).rounded(1, .plain))) капитала)") : "" } ?? ""
        }
        let side = r.side == .long ? say("long", "лонг") : say("short", "шорт")
        guard let m = market else {
            return say("✗ Market: \(IntentParser.pretty(r.asset)) has no price yet\n· Direction: \(side)\n· The rest is checked once the price loads.",
                       "✗ Рынок: у \(IntentParser.pretty(r.asset)) пока нет цены\n· Направление: \(side)\n· Остальное проверю, когда загрузится цена.")
        }
        var out: [String] = []
        let entry = r.entryPx ?? m.markPx
        out.append(say("✓ Market: ", "✓ Рынок: ") + "\(IntentParser.pretty(r.asset)) \(Fmt.price(m.markPx))"
                   + (m.change24h.map { ", 24h \($0 >= 0 ? "+" : "")\(Fmt.pct($0.rounded(2, .plain)))" } ?? ""))
        out.append(say("✓ Direction: ", "✓ Направление: ") + side)
        if let risk = r.riskUSD {
            let from = asked.riskUSD != nil ? "" : asked.amountUSDC.map { say(" from \(Fmt.num($0)) USDC in the trade", " из \(Fmt.num($0)) USDC в сделке") } ?? say(" by the book", " по правилам")
            out.append(say("✓ Risk: ", "✓ Риск: ") + Fmt.usd(risk) + share(risk) + from)
        } else {
            out.append(say("✗ Risk: not set", "✗ Риск: не задан")
                       + (book.riskUSD.map { say(" — by the book \(Fmt.usd($0)), \(Fmt.pct(policy.defaultRiskPct)) of capital", " — по правилам \(Fmt.usd($0)), \(Fmt.pct(policy.defaultRiskPct)) капитала") } ?? ""))
        }
        if let stop = r.stopPct {
            let direction: Decimal = r.side == .short ? 1 : -1
            let price = sizing?.stopPrice ?? entry * (1 + direction * stop / 100)
            out.append(say("✓ Stop: \(Fmt.pct(stop)) at \(Fmt.price(price))", "✓ Стоп: \(Fmt.pct(stop)) на \(Fmt.price(price))")
                       + (asked.stopPct == nil ? say(" by the book", " по правилам") : ""))
        } else if let b = book.stopPct {
            out.append(say("✗ Stop: not set — by the book \(Fmt.pct(b)), \(Fmt.num(policy.atrStopMultiple)) × 4h volatility",
                           "✗ Стоп: не задан — по правилам \(Fmt.pct(b)), \(Fmt.num(policy.atrStopMultiple)) × волатильность за 4 часа"))
        } else {
            out.append(atrRead ? say("✗ Stop: not set", "✗ Стоп: не задан")
                               : say("✗ Stop: not set — Hyperliquid’s 4h candles didn’t load, so the book can’t size it; say your own, e.g. “stop 3%”",
                                     "✗ Стоп: не задан — свечи Hyperliquid за 4 часа не загрузились, поэтому правила его не посчитают; скажи свой, например «стоп 3%»"))
        }
        if let take = r.takePct {
            out.append(say("✓ Take: \(Fmt.pct(take))", "✓ Тейк: \(Fmt.pct(take))") + (sizing?.takePrice.map { say(" at \(Fmt.price($0))", " на \(Fmt.price($0))") } ?? "")
                       + (r.partialTake == true ? say(", half at 1R", ", половина на 1R") : ""))
        } else {
            out.append(say("· Take: none — by the book \(Fmt.num(policy.defaultRewardRisk))R", "· Тейк: нет — по правилам \(Fmt.num(policy.defaultRewardRisk))R"))
        }
        out.append(r.entryPx.map { say("✓ Entry: limit \(Fmt.price($0))", "✓ Вход: лимит \(Fmt.price($0))") + (r.pullbackPct.map { say(" (\(Fmt.pct($0)) pullback)", " (откат \(Fmt.pct($0)))") } ?? "") }
                   ?? say("✓ Entry: at market", "✓ Вход: по рынку"))
        if let s = sizing {
            out.append("\(e.problem == nil ? "✓" : "✗") " + say("Size: ", "Размер: ") + "\(Fmt.usd(s.notional)) · \(Fmt.num(r.leverage ?? 1))× · "
                       + say("margin", "маржа") + " \(Fmt.usd(s.margin))" + (e.problem.map { ". \($0)" } ?? ""))
            let source = blocks.first { $0.id == "source" }, bridge = blocks.first { $0.id == "bridge" }
            let free = placeFacts.hyperliquid.map { Fmt.usd($0.withdrawable) } ?? "—"
            let place = quoted?.from?.name ?? "Solana"
            if source?.status == .blocked, let short = source?.lines.last {
                out.append(say("✗ Money: \(short.dropLast()) on \(place).", "✗ Деньги: \(short.dropLast().replacingOccurrences(of: "Short by", with: "не хватает")) на \(place)."))
            } else if let bridge, bridge.status == .ready || bridge.status == .warning, let q = quoted?.quote {
                out.append(say("✓ Money: \(free) free on Hyperliquid + \(Fmt.num(q.amountIn)) USDC from \(place) over \(q.provider) (≈ \(Fmt.num(q.amountOut)) arrive)",
                               "✓ Деньги: \(free) свободно на Hyperliquid + \(Fmt.num(q.amountIn)) USDC с \(place) через \(q.provider) (придёт ≈ \(Fmt.num(q.amountOut)))")
                           + (quoted?.from == nil ? "" : say(", straight: Solana has only \(Fmt.num(usdcFree(on: "solana") ?? 0))", ", напрямую: на Solana только \(Fmt.num(usdcFree(on: "solana") ?? 0))")))
                // The bridge's warning (it costs more than the take earns) belongs next to the money.
                if bridge.status == .warning, let costly = bridge.lines.first(where: { $0.hasPrefix("The bridge costs more") }) { out.append("⚠ " + say("Money: ", "Деньги: ") + costly) }
            } else if bridge?.status == .info {
                out.append(say("✓ Money: \(free) free on Hyperliquid covers the margin", "✓ Деньги: \(free) свободно на Hyperliquid — маржи хватает"))
            } else if bridge?.status == .blocked, let why = bridge?.lines.first {
                out.append(say("✗ Money: ", "✗ Деньги: ") + why)
            } else {
                out.append(say("· Money: \(free) free on Hyperliquid; the rest comes from Solana over Relay, quoted on the card",
                               "· Деньги: \(free) свободно на Hyperliquid; остальное придёт с Solana через Relay, цена — на карточке"))
            }
            let advice = RiskRules.check(r, s, market: m, equity: equity, policy: policy,
                                         openRisk: HyperliquidAccount.riskAtStops(positions: positions, orders: hlOrders))
            let problems = advice.filter { $0.level != .ok }
            out += problems.map { "\($0.level == .block ? "✗" : "⚠") " + say("Rules: ", "Правила: ") + $0.text }
            if problems.isEmpty { out.append(say("✓ Rules: all met", "✓ Правила: все соблюдены")) }
            var result = say("✓ Result: at the stop −\(Fmt.usd(s.lossAtStopUSD))", "✓ Итог: на стопе −\(Fmt.usd(s.lossAtStopUSD))") + share(s.lossAtStopUSD)
            if let p = s.profitAtTakeUSD, s.lossAtStopUSD > 0 {
                result += say(", at the take +\(Fmt.usd(p)), 1:\(Fmt.num((p / s.lossAtStopUSD).rounded(1, .plain)))",
                              ", на тейке +\(Fmt.usd(p)), 1:\(Fmt.num((p / s.lossAtStopUSD).rounded(1, .plain)))")
            }
            out.append(result)
        } else {
            out.append(say("· Size, money and rules: checked once the risk and the stop are set", "· Размер, деньги и правила: проверю, когда будут риск и стоп"))
        }
        let next: String
        if !e.missing.isEmpty {
            let (en, ru) = Self.missingExample(e.missing)
            next = asked.autoRules == true
                ? say("Say what’s missing, e.g. “\(en)”.", "Скажи, чего не хватает, например «\(ru)».")
                : say("Press “By the book” or say your own, e.g. “risk $1 stop 3%”.", "Нажми «По правилам» или скажи свои, например «риск $1 стоп 3%».")
        } else if out.contains(where: { $0.hasPrefix("✗") }) {
            next = say("Fix the ✗ above — for example a smaller risk or a wider stop — and I'll check again.",
                       "Исправь ✗ выше — например, риск поменьше или стоп пошире, — и я проверю снова.")
        } else if let failed {
            let why = failed.hasSuffix(".") ? failed : failed + "."
            next = say("Can't open it: \(why) Nothing was signed; ask again in a moment.", "Не получится открыть: \(why) Ничего не подписано; попробуй ещё раз чуть позже.")
        } else if ready {
            next = say("Ready: the card is on the canvas with its simulation — press Confirm, or change it in words.",
                       "Готово: карточка на холсте вместе с симуляцией — нажми «Подтвердить» или измени словами.")
        } else {
            next = say("Ready to check: the simulation runs next, before anything is signed.", "Можно проверять: дальше симуляция, до любой подписи.")
        }
        return out.joined(separator: "\n") + "\n" + next
    }

    // MARK: The route

    /// The trade plan as route cards, in the order it happens (Workspace.planRoute): the money from Solana (or already on
    /// Hyperliquid), the bridge, the rules and the second check, the position, its stop and take, the simulation, the
    /// signatures, and what it comes to. A step a rule stops, or that waits for a detail, says so on its card.
    func tradeRoute(_ r: TradeIntent, _ e: Evaluation, _ blocks: [Block], market: Market?, prepared p: Executor.Prepared?,
                    review: String?, reviewed: Bool, quoted: PlanQuote? = nil) -> (steps: [Board.Step], footer: String) {
        func block(_ id: String) -> Block? { blocks.first { $0.id == id } }
        func state(_ s: Block.Status, check: Bool = false) -> String {
            switch s {
            case .blocked: "blocked"
            case .needsInput: "attention"
            case .ready, .warning, .info: check ? "passed" : "info"
            case .noData: "info"
            }
        }
        // Rows are «label\tvalue»: the eye finds the number without reading a sentence.
        func row(_ label: String, _ value: String) -> String { "\(label)\t\(value)" }
        let sizing = e.sizing
        let name = IntentParser.pretty(r.asset).components(separatedBy: " (").first ?? r.asset
        var steps: [Board.Step] = []
        let free = placeFacts.hyperliquid?.withdrawable ?? 0
        // «Not needed»: the margin is on Hyperliquid already, and the route starts there.
        let bridge = block("bridge").flatMap { $0.status == .info ? nil : $0 }
        if let source = block("source") {
            if let bridge, p == nil || p?.bridge != nil || p?.fundFrom != nil || p?.openVia1Click != nil {
                // What goes over: the prepared bridge's, the amount asked for, or what the margin and fees come to.
                let need = sizing.map { max(0, $0.margin + $0.feesUSD - free).rounded(2, .up) }
                let q = p == nil ? quoted?.quote : nil
                let send = p?.bridge?.spend ?? p?.fundFrom?.usdc ?? p?.openVia1Click.map { Decimal($0.units) / 1_000_000 } ?? q?.amountIn ?? r.amountUSDC ?? need
                let approx = p == nil && q == nil && r.amountUSDC == nil ? "≈ " : ""
                // Solana short: the USDC leaves straight from the EVM network that has it.
                let net = p.map { $0.fundFrom?.net } ?? quoted?.from
                let from = net?.name ?? "Solana"
                let have = usdcFree(on: net?.key ?? "solana")
                let money = have.flatMap { h in send.map { s in
                    Board.Money(place: from, symbol: "USDC", was: Fmt.num(h), after: approx + Fmt.num(max(0, h - s)))
                } }
                var lines: [String] = money == nil ? have.map { [row(say("wallet", "кошелёк"), Fmt.num($0))] } ?? source.lines : []
                if source.status == .blocked, let short = source.lines.last { lines.insert(say(short, short.replacingOccurrences(of: "Short by", with: "Не хватает")), at: 0) }
                steps.append(Board.Step(label: "FROM · \(from.uppercased())", title: send.map { "\(approx)\(Fmt.num($0)) USDC" } ?? "USDC",
                                        lines: lines, money: money, status: state(source.status)))
                var lines2: [String]
                if let b = p?.bridge {
                    lines2 = [row(say("sends", "отправит"), "\(Fmt.num(b.spend)) USDC"), row(say("arrives", "придёт"), "\(Fmt.num(b.arriving)) USDC"),
                              row(say("fee", "комиссия"), b.feesUSD)]
                    if b.opening > 0 { lines2.append(say("incl. Hyperliquid’s one-time account fee", "вкл. разовую плату Hyperliquid за открытие счёта")) }
                } else if let o = p?.openVia1Click {
                    lines2 = [row(say("sends", "отправит"), "\(Fmt.num(Decimal(o.units) / 1_000_000)) USDC"), row(say("arrives", "придёт"), "\(Fmt.num(o.arriving)) USDC"),
                              say("NEAR Intents opens the account without its fee, in 2–4 min", "NEAR Intents открывает счёт без платы, за 2–4 мин")]
                } else if let f = p?.fundFrom {
                    lines2 = [row(say("sends", "отправит"), "\(Fmt.num(f.usdc)) USDC"), say("straight from \(f.net.name): Solana is short", "прямо из \(f.net.name): на Solana не хватает")]
                } else if let q {
                    lines2 = [row(say("sends", "отправит"), "\(Fmt.num(q.amountIn)) USDC"), row(say("arrives", "придёт"), "\(Fmt.num(q.amountOut)) USDC"),
                              row(say("fee", "комиссия"), Fmt.usd(q.amountIn - q.reserve - q.amountOut) + (q.time.firstMatch(of: /\d+\s*s\b/).map { " · \($0.0)" } ?? ""))]
                    if q.opening { lines2.append(q.provider == "Relay" ? say("incl. Hyperliquid’s one-time account fee", "вкл. разовую плату Hyperliquid за открытие счёта")
                                                                       : say("opens the account, no opening fee", "открывает счёт без платы")) }
                } else {
                    lines2 = (send.map { [row(say("sends", "отправит"), "≈ \(Fmt.num($0)) USDC")] } ?? [])
                        + [say("quoted once the position passes the checks", "цена — когда позиция пройдёт проверки")]
                }
                let lands = p?.bridge?.arriving ?? p?.openVia1Click?.arriving ?? q?.amountOut ?? send
                let oneClick = p?.openVia1Click != nil || (q.map { $0.provider != "Relay" } ?? false)
                steps.append(Board.Step(label: oneClick ? "BRIDGE · 1CLICK" : "BRIDGE · RELAY", title: "→ Hyperliquid", lines: lines2,
                                        money: lands.map { Board.Money(place: "Hyperliquid", symbol: "USDC", was: Fmt.num(free), after: (p == nil && q == nil ? "≈ " : "") + Fmt.num(free + $0)) },
                                        status: bridge.status == .needsInput ? "info" : state(bridge.status)))
            } else {
                steps.append(Board.Step(label: "FROM · HYPERLIQUID", title: "\(Fmt.num(free)) USDC",
                                        lines: [say("the margin is on Hyperliquid already", "маржа уже на Hyperliquid")], status: "info"))
            }
        }
        if let risk = block("risk") {
            var lines: [String] = []
            if let s = sizing {
                let pct = ruleEquity.flatMap { $0 > 0 ? (s.lossAtStopUSD / $0 * 100).rounded(1, .plain) : nil }
                lines.append(row(say("risk", "риск"), Fmt.usd(s.lossAtStopUSD) + (pct.map { " · \(Fmt.num($0))%" } ?? "")))
                lines.append(row(say("limit", "лимит"), say("\(Fmt.num(policy.maxRiskPct))% a trade", "\(Fmt.num(policy.maxRiskPct))% на сделку")))
                if let profit = s.profitAtTakeUSD, s.lossAtStopUSD > 0 { lines.append(row(say("reward/risk", "прибыль/риск"), "1:\(Fmt.num((profit / s.lossAtStopUSD).rounded(1, .plain)))")) }
            }
            // A rule it breaks, in its own words, first.
            lines.insert(contentsOf: risk.lines.filter { $0.hasPrefix("✗") }, at: 0)
            steps.append(Board.Step(label: "CHECK RULES", title: risk.title, lines: lines.isEmpty ? risk.lines : lines, status: state(risk.status, check: true)))
        }
        if reviewed, p != nil {
            var lines = [review ?? say("Matches your request.", "Совпадает с твоей просьбой.")]
            if p?.bridge != nil { lines.append(say("USDC: Circle's token on Solana, EPjF…Dt1v.", "USDC: токен Circle на Solana, EPjF…Dt1v.")) }
            if p?.bridge != nil { lines.append(say("Bridge: Relay, its program on the allowed list; each signature is checked again at Confirm.",
                                                   "Мост: Relay, его программа в списке разрешённых; каждая подпись проверяется снова при «Подтвердить».")) }
            if let evm = evmAddress { lines.append(say("To: your own Hyperliquid account \(evm.prefix(6))…\(evm.suffix(4)).", "Куда: твой собственный счёт Hyperliquid \(evm.prefix(6))…\(evm.suffix(4)).")) }
            lines.append(say("Market: \(name) on Hyperliquid.", "Рынок: \(name) на Hyperliquid."))
            steps.append(Board.Step(label: "SECOND CHECK", title: review == nil ? say("All correct", "Всё верно") : say("Has doubts", "Есть сомнения"),
                                    lines: lines, status: review == nil ? "passed" : "attention"))
        }
        if let position = block("position") {
            let leverage = "\(Fmt.num(r.leverage ?? 1))×"
            var lines = position.lines
            if let s = sizing, let m = market {
                lines = [row(say("amount", "объём"), "\(Fmt.num(s.size, maxFraction: m.szDecimals)) \(name)"),
                         row(r.entryPx == nil ? say("entry", "вход") : say("limit", "лимит"), "\(r.entryPx == nil ? "≈ " : "")\(Fmt.price(s.entry))"),
                         row(say("margin", "маржа"), Fmt.usd(s.margin)),
                         row(say("liquidation", "ликвидация"), "≈ \(Fmt.price(s.liquidationPrice))")]
            }
            // What stops it goes first, in red.
            if position.status == .blocked, let problem = e.problem ?? position.lines.last { lines.insert(problem, at: 0) }
            steps.append(Board.Step(label: "OPEN \(r.side == .long ? "LONG" : "SHORT") · \(name.uppercased())",
                                    title: sizing.map { "\(Fmt.usd($0.notional)) · \(leverage)" } ?? "\(name) \(leverage)",
                                    lines: lines, status: state(position.status)))
        }
        if let protection = block("protection") {
            var lines = protection.lines
            if let z = sizing, let stop = r.stopPct {
                lines = [row(say("stop", "стоп"), "\(Fmt.price(z.stopPrice)) · −\(Fmt.pct(stop))")]
                if let tp = z.takePrice { lines.append(row(say("take", "тейк"), "\(Fmt.price(tp)) · +\(Fmt.pct((abs(tp - z.entry) / z.entry * 100).rounded(1, .plain)))")) }
                lines.append(say("both wait on the exchange", "оба ждут на бирже"))
            }
            steps.append(Board.Step(label: "PROTECT", title: sizing?.takePrice != nil ? say("Stop + take", "Стоп + тейк") : say("Stop", "Стоп"),
                                    lines: lines, status: state(protection.status)))
        }
        // What Confirm signs: the simulation ran first, then Windows Hello for every signature.
        let signing = p?.steps.count ?? blocks.filter { ["bridge", "position", "protection"].contains($0.id) && $0.status != .info }.count
        if sizing != nil, signing > 0 {
            if let p {
                steps.append(Board.Step(label: "SIMULATION · PASSED", title: say("As planned", "Как задумано"), lines: Array(p.changes.prefix(3)), status: "passed"))
            } else {
                steps.append(Board.Step(label: "SIMULATION · FIRST", title: say("Runs before the card", "До карточки"),
                                        lines: [say("every balance, before and after", "каждый баланс — до и после"), say("nothing is signed before it", "до неё ничего не подписывается")], status: "info"))
            }
            steps.append(Board.Step(label: ledgerActive ? "SIGN · LEDGER" : "SIGN · WINDOWS HELLO",
                                    title: say("\(signing) signature\(signing == 1 ? "" : "s")", "подписей: \(signing)"),
                                    lines: p?.steps ?? [ledgerActive ? say("each one approved on the device", "каждая — на устройстве") : say("one Windows Hello covers them all", "одно Windows Hello на все")],
                                    status: "info"))
        }
        if let s = sizing {
            var lines = [row(say("at the stop", "на стопе"), "−\(Fmt.usd(s.lossAtStopUSD))")]
            if let profit = s.profitAtTakeUSD { lines.append(row(say("at the take", "на тейке"), "+\(Fmt.usd(profit))")) }
            lines.append(row(say("fees", "комиссии"), "≈ \(Fmt.usd(s.feesUSD)) · ihatecrypto \(Fmt.usd(s.builderFeesUSD))"))
            steps.append(Board.Step(label: "RESULT · ESTIMATE", title: "−\(Fmt.usd(s.lossAtStopUSD))" + (s.profitAtTakeUSD.map { " / +\(Fmt.usd($0))" } ?? ""),
                                    lines: lines, status: "info"))
        }
        let footer = watching
            ? say("Watching only: the simulation shows what would happen; nothing can sign here.", "Только просмотр: симуляция показывает, что было бы; подписать здесь нечем.")
            : p != nil
                ? say("Confirm signs \(signing) signature\(signing == 1 ? "" : "s") after \(ledgerActive ? "your Ledger" : "Windows Hello"); the simulation above ran first.",
                      "«Подтвердить» подпишет \(signing) — после \(ledgerActive ? "Ledger" : "Windows Hello"); симуляция выше уже прошла.")
                : say("Nothing is signed until the plan passes its checks: the simulation runs first, then \(ledgerActive ? "your Ledger" : "Windows Hello").",
                      "Ничего не подписывается, пока план не пройдёт проверки: сначала симуляция, потом \(ledgerActive ? "Ledger" : "Windows Hello").")
        return (steps, footer)
    }

    // MARK: Planning

    /// A plan's board in these states is the plan's own and gives way to its next version; a move that ran stays.
    static let planStates: Set<String> = ["plan", "waiting", "cancelled"]

    /// A trade plan by the risk rules (Moves.plan forwards here): its route drawn whether the rules stop it or not, the
    /// chat's checklist, and — once it passes — the card with every signature and the simulation, in the plan's place.
    func planTrade(_ next: TradeIntent, note: String?, said: String) async {
        let before = intent
        // After a trade «stop 2%» or «take closer» move that position's orders instead of starting a new plan.
        if let traded = tradedPlan, await editTraded(from: traded, to: next, note: note) { return }
        intent = next
        await planMarketsLoaded()
        var market = markets[next.asset]
        var atrRead = true
        if var m = market, m.atrPct == nil {
            m.atrPct = await planATR(m.name)
            atrRead = m.atrPct != nil
            market = m
        }
        let resolved = Plan.resolve(next, market: market, equity: ruleEquity, policy: policy)
        let r = resolved.intent
        let e = Plan.evaluate(r, market: market)
        let quoted = await planQuote(r, e, market: market)
        let blocks = Plan.blocks(r, e, planLive(market: market, notes: resolved.notes, quoted: quoted))
        let changes = next.changes(from: before)
        let lead = (said.isEmpty ? "" : said + "\n")
            + (changes.isEmpty ? say("That’s already in the plan.", "Это уже есть в плане.")
               : say("Plan updated: ", "План обновлён: ") + changes.map { russian ? Self.changeRu($0) : $0 }.joined(separator: ", ") + ".")
        let title = say("Plan · \(r.side == .long ? "long" : "short") \(IntentParser.pretty(r.asset))", "План · \(r.side == .long ? "лонг" : "шорт") \(IntentParser.pretty(r.asset))")
        var view = PlanView(market: r.asset, group: market?.group.rawValue ?? Market.Group.crypto.rawValue, footer: "")

        // Stopped, or waiting for a detail: the route with what stops it, the fix, the checklist.
        let off = Modules.current.refusal(trade: market?.dex != nil, bridges: next.amountUSDC != nil, ru: russian)
        let blocker = off.map { (say("off in Modules", "выключено в модулях"), $0) } ?? planBlocker(e, blocks, equity: ruleEquity, from: quoted?.from?.name ?? "Solana")
        if blocker != nil || !e.missing.isEmpty || market == nil {
            let route = tradeRoute(r, e, blocks, market: market, prepared: nil, review: nil, reviewed: false, quoted: quoted)
            view.footer = route.footer
            if let blocker {
                view.stopped = blocker.1
                view.short = blocker.0
                view.blocked = true
                if off == nil, let m = market, let fix = planFix(next, market: m, now: e.sizing) {
                    view.fix = Board.Action(title: fix.title, command: fix.command)
                    view.fixDetail = fix.detail
                }
                view.overLimit = e.sizing.map { s in (ruleEquity ?? 0) > 0 && s.lossAtStopUSD / ruleEquity! * 100 > policy.maxRiskPct } ?? false
            } else if !e.missing.isEmpty {
                let missing = e.missing.map { $0 == "risk in $" ? say("the risk", "риска") : say("a stop", "стопа") }.joined(separator: say(" and ", " и "))
                view.short = say("needs \(missing)", "нет \(missing)")
                view.stopped = say("The plan needs \(missing) before it can be checked.", "Плану не хватает \(missing), чтобы его проверить.")
                if next.autoRules == true {
                    // The book is on and still can't fill it: no capital to take the risk from, or no candles for the stop.
                    let (en, ru) = Self.missingExample(e.missing)
                    view.stopped = say("The plan needs \(missing): the rules have nothing to size it from yet — say it, e.g. “\(en)”.",
                                       "Плану не хватает \(missing): правилам пока не от чего его посчитать — скажи сам, например «\(ru)».")
                } else {
                    view.fix = Board.Action(title: say("By the book", "По правилам"), command: "by the book")
                    view.fixDetail = say("Stop from the 4h volatility, take at \(Fmt.num(policy.defaultRewardRisk))R, \(Fmt.pct(policy.defaultRiskPct)) of capital at risk.",
                                         "Стоп по волатильности за 4 часа, тейк \(Fmt.num(policy.defaultRewardRisk))R, риск \(Fmt.pct(policy.defaultRiskPct)) капитала.")
                }
            }
            drawPlan(title: title, steps: route.steps, view: view, r, e, market: market)
            let list = planChecklist(next, r, e, blocks, market: market, atrRead: atrRead, ready: false, quoted: quoted)
            reply(lead + "\n" + list, note: note)
            if said.isEmpty { commentOnPlan(list, note: note) }
            return
        }

        // Code first, second check on or off: the plan never holds more money or more risk than its command named.
        var warnings: [String] = []
        if let market, let sizing = e.sizing, let why = Plan.moneyProblem(named: next, sizing: sizing, market: market) { warnings.append(why) }
        // A new Hyperliquid account opens with this trade: Relay or NEAR Intents, picked on two cards first (MoveChecks).
        if !watching, await askOpeningRoute(r, e, market: market, note: note) { return }
        do {
            let p = try await (watching ? previewExecutor() : executor()).prepare(next, policy: policy, openVia1Click: openVia1Click == true)
            var review: String?
            if warnings.isEmpty, secondCheck, let concern = await reviewPlan(next, p) { review = concern; warnings.append(concern) }
            let route = tradeRoute(r, e, blocks, market: market, prepared: p, review: review, reviewed: secondCheck)
            view.footer = route.footer
            if watching {
                // A watched address: the simulation on the canvas, nothing to confirm.
                view.preview = true
                drawPlan(title: title, steps: route.steps, view: view, r, e, market: market)
            } else {
                let replaced = planBoard
                show(.trade(p), title: title, lines: p.steps + p.changes, warnings: warnings)
                updateBoard(card?.id) { $0.steps = route.steps; $0.plan = view }
                drawTrade(card?.id, r, e.sizing, market: market)
                // The stopped plan, or the card it was before an edit: the new card takes its place.
                if let replaced, replaced != card?.id { boards.removeAll { $0.id == replaced && Self.planStates.contains($0.state ?? "") } }
                planBoard = card?.id
            }
            let list = planChecklist(next, r, e, blocks, market: market, atrRead: atrRead, ready: !watching, quoted: quoted)
            reply(lead + "\n" + list, note: note)
            if said.isEmpty { commentOnPlan(list, note: note) }
        } catch {
            // The preparation refused it (a balance moved, the bridge couldn't quote): the route, stopped by that.
            let route = tradeRoute(r, e, blocks, market: market, prepared: nil, review: nil, reviewed: false, quoted: quoted)
            view.footer = route.footer
            view.stopped = error.localizedDescription
            view.short = say("not prepared", "не подготовлен")
            view.blocked = true
            drawPlan(title: title, steps: route.steps, view: view, r, e, market: market)
            let list = planChecklist(next, r, e, blocks, market: market, atrRead: atrRead, ready: false, quoted: quoted, failed: error.localizedDescription)
            reply(lead + "\n" + list, note: note)
        }
    }

    /// The plan's board, stopped or previewed: drawn in the place of the one before it, the newest on top.
    private func drawPlan(title: String, steps: [Board.Step], view: PlanView, _ r: TradeIntent, _ e: Evaluation, market: Market?) {
        // A card from an earlier version of this plan stops waiting: what's on the canvas now is this one.
        if case .trade? = pending, card?.id == planBoard { cancelPending() }
        if let id = planBoard, boards.contains(where: { $0.id == id && Self.planStates.contains($0.state ?? "") }) { boards.removeAll { $0.id == id } }
        planBoard = pin(Board(kind: "move", title: title, steps: steps, state: "plan"))
        updateBoard(planBoard) { $0.plan = view }
        drawTrade(planBoard, r, e.sizing, market: market)
    }

    /// The plan's board goes with the plan («clear the plan», its Cancel), and «stop 2%» no longer edits the last trade.
    func dropPlanBoard() {
        tradedPlan = nil
        guard let id = planBoard else { return }
        if card?.id == id { cancelPending() }
        boards.removeAll { $0.id == id && Self.planStates.contains($0.state ?? "") }
        planBoard = nil
    }

    /// The model's own word on the plan it built (ChatRouter.comment, the Mac's replyPlan): it takes the first line's
    /// place, with the changes worth making as cards. Dropped when anything was said or changed since.
    func commentOnPlan(_ list: String, note: String?) {
        guard let client = llmClient(), let sent = messages.last, !sent.fromUser else { return }
        let plan = intent, context = routerContext(), model = model, command = intent.command
        let userText = messages.last(where: \.fromUser)?.text ?? command
        // The work in a method of its own: Swift 6.4.1 for Windows crashed optimizing it inside the Task's closure (CSE).
        Task { await applyPlanComment(list, plan: plan, after: sent.id, command: command, userText: userText, context: context, client: client, model: model) }
    }

    private func applyPlanComment(_ list: String, plan: TradeIntent, after sentID: String, command: String, userText: String,
                                  context: ChatRouter.Context, client: any LLMClient, model: String) async {
        let reading = try? await ChatRouter.comment(on: list, command: command, userText, context: context, client: client, model: model)
        guard let reading, case .chat(let words) = reading.route, !words.isEmpty, intent == plan, messages.last?.id == sentID else { return }
        messages[messages.count - 1].text = words + "\n" + list
        saveChat()
        changed()
        offer(reading.options, userText: userText, found: reading.found)
    }

    /// A plan for watched addresses: prepared and simulated against them, never signed (the Mac's previewWallet).
    func previewExecutor() throws -> Executor {
        guard let solana = settings.watchSolana, let evm = settings.watchEVM else {
            throw ExecutionError(say("A preview needs both watched addresses, Solana and EVM: the money comes from one and trades on the other.",
                                     "Для просмотра нужны оба адреса — Solana и EVM: деньги идут с одного, сделка — на другом."))
        }
        return Executor(wallet: TradeWallet(solana: solana, evm: evm), signer: PreviewSigner(refusal: watchOnly), journal: journal) { _ in }
    }

    /// The plan the next words change: the one on the canvas, or the traded one while its position is open.
    var planBase: TradeIntent {
        guard intent == TradeIntent(), let traded = tradedPlan, positions.contains(where: { $0.coin == traded.asset }) else { return intent }
        return traded
    }

    // MARK: After a trade

    /// «stop 2%», «take closer» after a trade: only stop and take may change, and that position's orders move on the
    /// exchange after Confirm (the Mac's editExecuted); anything else is a new plan.
    func editTraded(from before: TradeIntent, to next: TradeIntent, note: String?) async -> Bool {
        guard let p = positions.first(where: { $0.coin == before.asset }) else { tradedPlan = nil; return false }
        var same = next
        same.stopPct = before.stopPct; same.takePct = before.takePct; same.takeR = before.takeR
        guard same == before, next != before else { return false }
        var market = markets[before.asset]
        if var m = market, m.atrPct == nil { m.atrPct = await planATR(m.name); market = m }
        let resolved = Plan.resolve(next, market: market, equity: ruleEquity, policy: policy).intent
        let edit = Self.orderEdit(before: before, asked: next, resolved: resolved, size: p.size, entry: p.entry, equity: ruleEquity, maxRiskPct: policy.maxRiskPct)
        if let loss = edit.tooWide, let equity = ruleEquity, let stopPct = resolved.stopPct {
            let pct = Fmt.pct((loss / equity * 100).rounded(1, .plain))
            reply(say("A \(Fmt.pct(stopPct)) stop means −\(Fmt.usd(loss)), \(pct) of capital. The rule: at most \(Fmt.pct(policy.maxRiskPct)). Stop not moved.",
                      "Стоп \(Fmt.pct(stopPct)) — это −\(Fmt.usd(loss)), \(pct) капитала. Правило: не больше \(Fmt.pct(policy.maxRiskPct)). Стоп не сдвинут."), note: note)
            return true
        }
        let moves = edit.moves
        guard !moves.isEmpty else { return false }
        let what = moves.map(describeOrder).joined(separator: ", ")
        show(.orders(moves, coin: p.coin, intent: next), title: "\(IntentParser.pretty(p.coin)): \(what)")
        reply(say("\(IntentParser.pretty(p.coin)): \(what). Press Confirm to move the orders.",
                  "\(IntentParser.pretty(p.coin)): \(what). Нажми «Подтвердить», чтобы переставить ордера."), note: note)
        return true
    }

    /// Confirm on «stop 2%» after a trade: each order moved on the exchange, the traded plan now with the new levels.
    func moveOrders(_ moves: [PositionCommand], coin: String, intent traded: TradeIntent) async throws -> String {
        var said: [String] = []
        for move in moves { said.append(try await executor().manage(move, asset: coin, policy: policy, equity: ruleEquity)) }
        tradedPlan = traded
        return said.joined(separator: "\n")
    }

    /// The orders «stop 2%», «take 6%» move on an open position: only what the words changed, at the new levels from its
    /// entry. `tooWide`: what the new stop would lose when that's over the per-trade limit; nothing moves then.
    nonisolated static func orderEdit(before: TradeIntent, asked: TradeIntent, resolved: TradeIntent, size: Decimal, entry: Decimal,
                                      equity: Decimal?, maxRiskPct: Decimal) -> (moves: [PositionCommand], tooWide: Decimal?) {
        let dir: Decimal = size > 0 ? 1 : -1
        var moves: [PositionCommand] = []
        if asked.stopPct != before.stopPct, let stopPct = resolved.stopPct {
            let stop = entry * (1 - dir * stopPct / 100)
            let loss = abs(entry - stop) * abs(size) + abs(size) * (entry + stop) * RiskCalc.tradeFee
            if let equity, equity > 0, loss / equity * 100 > maxRiskPct { return ([], loss) }
            moves.append(.stopTo(stop))
        }
        if asked.takePct != before.takePct || asked.takeR != before.takeR, let takePct = resolved.takePct {
            moves.append(.takeTo(entry * (1 + dir * takePct / 100)))
        }
        return (moves, nil)
    }

    func describeOrder(_ command: PositionCommand) -> String {
        switch command {
        case .close(let share): share >= 1 ? say("close", "закрыть") : say("close \(Fmt.num(share * 100, maxFraction: 0))%", "закрыть \(Fmt.num(share * 100, maxFraction: 0))%")
        case .stopToBreakeven: say("stop to breakeven", "стоп в безубыток")
        case .stopTo(let px): say("stop to \(Fmt.price(px))", "стоп на \(Fmt.price(px))")
        case .takeTo(let px): say("take-profit to \(Fmt.price(px))", "тейк на \(Fmt.price(px))")
        case .trail(let pct): say("trail the stop \(Fmt.pct(pct))", "трейлинг-стоп \(Fmt.pct(pct))")
        case .cancelOrders: say("cancel orders", "снять ордера")
        }
    }

    // MARK: Take profit, the market

    /// «Take profit like the pros»: the three ways, on the plan's board; true when the message asked for them.
    func askTakeChoices(_ text: String, note: String? = nil) async -> Bool {
        guard TakeStyle.asksForChoice(text) else { return false }
        guard intent != TradeIntent() else {
            reply(say("Describe the trade first — then I’ll show the ways to take profit on it.", "Сначала опиши сделку — потом покажу, как на ней взять прибыль."), note: note)
            return true
        }
        let risk = Plan.evaluate(Plan.resolve(intent, market: markets[intent.asset], equity: ruleEquity, policy: policy).intent, market: markets[intent.asset]).sizing?.lossAtStopUSD
        updateBoard(planBoard) { $0.plan?.takes = takeChoices(risk: risk) }
        reply(say("Three ways to take profit are on the canvas. The stop stays the same; pick one or say your own, e.g. “take 3R”.",
                  "На холсте три способа взять прибыль. Стоп остаётся прежним; выбери один или скажи свой, например «тейк 3R»."), note: note)
        return true
    }

    func takeChoices(risk: Decimal?) -> [TakeChoice] {
        TakeStyle.allCases.map { s in
            let ru: (String, String, String) = switch s {
            case .classic: ("Классика 2R", "Всё на двойном риске.", "В плюсе, если выигрываешь больше трети сделок.")
            case .partial: ("По частям", "Половина на 1R, стоп в безубыток, остальное на 2R.", "После первой половины риск почти ноль.")
            case .trend: ("За трендом", "Половина на 1R, остальное идёт за ценой с трейлинг-стопом 2%.", "Без потолка, но на развороте часть отдашь.")
            }
            let payoff = s == .trend ? say("no cap", "без потолка") : risk.map(s.payoff) ?? ""
            return TakeChoice(id: s.rawValue, title: say(s.title, ru.0), detail: say(s.detail, ru.1), note: say(s.note, ru.2), payoff: payoff, recommended: s == .partial)
        }
    }

    /// A take-profit way picked on the board: the plan takes it, and is checked again.
    func chooseTake(_ id: String) async {
        guard let style = TakeStyle(rawValue: id), intent != TradeIntent() else { return }
        await planTrade(style.apply(to: intent), note: nil, said: say("Take: \(style.title.lowercased()).", "Тейк: \(takeChoices(risk: nil).first { $0.id == id }?.title.lowercased() ?? id)."))
    }

    /// The market picked in the plan's header (the Mac's MarketPicker): the same plan on that market, checked again.
    func choosePlanMarket(_ name: String) async {
        guard intent != TradeIntent(), name != intent.asset, let m = markets[name] else { return }
        var next = intent
        next.asset = m.name
        let group = m.group.rawValue.lowercased()
        await planTrade(next, note: nil, said: say("Market: \(IntentParser.pretty(m.name)) · Hyperliquid\(m.dex.map { " \($0)" } ?? "") — \(group).",
                                                   "Рынок: \(IntentParser.pretty(m.name)) · Hyperliquid\(m.dex.map { " \($0)" } ?? "") — \(Self.groupRu[m.group] ?? group)."))
    }

    /// What to say for what the plan lacks: «risk $1», «stop 3%» or both.
    nonisolated static func missingExample(_ missing: [String]) -> (String, String) {
        switch missing {
        case ["risk in $"]: ("risk $1", "риск $1")
        case ["stop in %"]: ("stop 3%", "стоп 3%")
        default: ("risk $1 stop 3%", "риск $1 стоп 3%")
        }
    }

    /// TradeIntent.changes in Russian: its fragments are the core's English.
    nonisolated static func changeRu(_ change: String) -> String {
        let whole = ["long": "лонг", "short": "шорт", "the rest by the risk rules": "остальное по правилам риска", "market entry": "вход по рынку",
                     "take in parts: half at 1R, then stop to breakeven": "тейк по частям: половина на 1R, потом стоп в безубыток"]
        if let ru = whole[change] { return ru }
        let starts = [("market ", "рынок "), ("risk ", "риск "), ("stop ", "стоп "), ("take ", "тейк "), ("leverage ", "плечо "),
                      ("money in the trade ", "деньги в сделке "), ("trailing ", "трейлинг "), ("limit entry at ", "лимитный вход на "), ("entry on a ", "вход на откате ")]
        for (en, ru) in starts where change.hasPrefix(en) {
            return ru + change.dropFirst(en.count).replacingOccurrences(of: " pullback", with: "")
        }
        return change
    }

    static let groupRu: [Market.Group: String] = [.crypto: "крипта", .commodities: "сырьё", .stocks: "акции", .indices: "индексы и валюты"]

    /// The window's requests about the plan: the picker's markets (read only), a market or a take-profit way picked, Cancel.
    func planOp(_ r: Request) async -> [String: String] {
        switch r.op {
        case "planMarkets": return ["json": await planMarkets()]
        case "planMarket": await choosePlanMarket(r.name ?? "")
        case "planTake": await chooseTake(r.name ?? "")
        default:
            clearPlan(note: nil)
            dropPlanBoard()
        }
        return [:]
    }

    /// Hyperliquid's markets by group, sorted by name as the Mac's picker: {"Crypto": ["BTC", …], "Stocks": […]}.
    func planMarkets() async -> String {
        await planMarketsLoaded()
        var out: [String: [String]] = [:]
        for g in Market.Group.allCases {
            out[g.rawValue] = markets.values.filter { $0.group == g }.map(\.name).sorted()
        }
        return (try? JSONEncoder().encode(out)).flatMap { String(data: $0, encoding: .utf8) } ?? "{}"
    }
}

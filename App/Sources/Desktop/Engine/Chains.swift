import EVMChain
import Foundation
import IhatecryptoCore
import OneClickKit
import SignerAPI
import SolanaChain
import TradeExecution

/// A chain waiting on the person: the move whose «How much?» is open, with the steps held behind it; and whether the last
/// chain stopped before its card (a scheduled run's History marks it failed).
struct ChainState {
    var held: (command: WalletCommand, rest: [String])?
    var stopped = false
    /// A contract call's card in a chain, and the steps after it: that card's Confirm names them, and once the call is
    /// through they run on it (the Mac's runSteps → continueTask), on the same canvas within half an hour.
    var afterCall: (board: String, canvas: String, rest: [String], since: Date)?
}

/// «withdraw 5 USDC to Solana then deposit 5 USDC to kamino», «chart SOL then buy SOL for 10 USDC»: the Mac's runSteps
/// (Tasks.swift). Steps that show or set something run at once; the moves go on one card and one Confirm covers them, the
/// steps between and after them run in turn once it's pressed. A move without its amount asks «How much?» and holds the
/// rest. After Confirm a bridge leg signs and goes on while its money lands, a step waits only for the legs landing where
/// it spends, and spends what arrived when a fee left it a little short.
extension Engine {
    // MARK: What a chain holds

    /// Whether a step reads as a command ihatecrypto carries out: the parsers `runCanonical` tries (the Mac's knows).
    func knows(_ step: String) -> Bool {
        let m = Set(markets.keys), s = step.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".! "))
        return s == "lock" || s == "clear the plan" || TokenCommand.parse(step) != nil || ChartCommand.parse(step, markets: m) != nil
            || PnLCommand.parse(step, markets: m) != nil || AlertCommand.parse(step, markets: m) != nil
            || AddressBookCommand.parse(step) != nil || FolderCommand.parse(step) != nil || WalletCommand.parse(step) != nil || PositionCommand.parse(step) != nil
            || (IntentParser.startsPlan(step, markets: m) && IntentParser.apply(step, to: TradeIntent(), markets: m) != nil)
    }

    enum ChainKind {
        /// It shows or sets something: run when it's reached, before the card if it comes first.
        case now
        /// A move the chain's card signs.
        case move(WalletCommand)
        /// It has a card of its own (a position, a trade plan, a recurring buy): on the chain's card, run once it's confirmed.
        case later
    }

    /// In the order `runCanonical` reads a step: what shows or sets something comes before the moves («chart SOL» names a
    /// coin a plan could start on).
    func chainKind(_ step: String, _ command: WalletCommand?) -> ChainKind {
        let m = Set(markets.keys), s = step.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".! "))
        if s == "lock" || s == "clear the plan" || FolderCommand.parse(step) != nil || AlertCommand.parse(step, markets: m) != nil
            || TokenCommand.parse(step) != nil || ChartCommand.parse(step, markets: m) != nil || PnLCommand.parse(step, markets: m) != nil
            || AddressBookCommand.parse(step) != nil {
            return .now
        }
        if let c = command {
            switch c {
            case .receive, .yields, .showApprovals, .jupiterDCAOrders, .multisigShow, .stopSchedules, .receiveAcross: return .now
            case .schedule: return .later
            default: return .move(c)
            }
        }
        if PositionCommand.parse(step) != nil || (IntentParser.startsPlan(step, markets: m) && IntentParser.apply(step, to: TradeIntent(), markets: m) != nil) {
            return .later
        }
        return .now
    }

    // MARK: Before Confirm

    /// Several steps from one message. False when a step isn't something ihatecrypto does (the caller says so).
    /// `first`: the first step as a command already (a «How much?» answered), its words only for the record.
    /// `confirmed`: the steps a confirmed card named (a contract call's, holdAfterCall): their card is carried out at once.
    func chain(_ steps: [String], first: WalletCommand? = nil, note: String?, confirmed: Bool = false) async -> Bool {
        func command(_ i: Int) -> WalletCommand? { i == 0 && first != nil ? first : WalletCommand.parse(steps[i]) }
        guard steps.indices.allSatisfy({ ($0 == 0 && first != nil) || knows(steps[$0]) }) else { return false }
        chainState.stopped = false
        func stop(_ why: String) {
            reply(why, note: note)
            chainState.stopped = true
        }
        // Up to the first step with a card: each runs now, in order.
        var k = 0
        while k < steps.count, case .now = chainKind(steps[k], command(k)) {
            guard await runCanonical(steps[k], userText: steps[k]) else {
                stop(say("ihatecrypto can’t do “\(steps[k])” — the steps stop here.", "ihatecrypto не умеет «\(steps[k])» — на этом шаги останавливаются."))
                return true
            }
            k += 1
        }
        guard k < steps.count else { return true }
        let rest = Array(steps[(k + 1)...])
        // One step with a card left: its own card, as when it's asked alone.
        if rest.isEmpty {
            let before = card?.id
            switch command(k) {
            case .sendTo(let name, let amount, let net)?: prepareTransfer(amount, to: name, network: net, note: note)
            case let c?: await handleWallet(c, note: note)
            case nil: _ = await runCanonical(steps[k], userText: steps[k])
            }
            if confirmed, let id = card?.id, id != before { await confirm(card: id) }
            return true
        }
        // The first move without its amount: asked now, the steps after it held for the answer.
        if case .move(let c) = chainKind(steps[k], command(k)), let question = c.amountQuestion {
            chainState.held = (c, rest)
            awaitingAmount = c
            reply(say(question, "Сколько? Напиши число.") + amountHint(c), note: note)
            return true
        }
        var parsed: [ChainStep] = []
        // A later move without its amount: asked when it's reached; it and the steps after it run by their words.
        var asks: String?
        for i in k..<steps.count {
            let step = steps[i]
            guard asks == nil, case .move(var c) = chainKind(step, command(i)) else { parsed.append(.said(step)); continue }
            if c.amountQuestion != nil { asks = step; parsed.append(.said(step)); continue }
            if let missing = missingKey(c) { stop(missing); return true }
            if let off = Modules.current.refusal(c, ru: russian) { stop(off); return true }
            // A SOL or NEAR send's recipient before the card, as asked alone: a memo/tag address is refused here.
            do { _ = try nativeRecipient(c) } catch { stop(error.localizedDescription); return true }
            // A contract call is checked and simulated on its own card, never carried out on a chain's Confirm: after the
            // moves before it, its card comes for its own Confirm (the Mac's runSteps). One that comes first is asked alone.
            if case .contractCall = c {
                guard i > k else {
                    // First: its own card now, checked and simulated; the steps after it ride on that card's Confirm.
                    await prepareContractCall(c, note: note)
                    holdAfterCall(Array(steps[(i + 1)...]))
                    return true
                }
                parsed.append(.said(step))
                continue
            }
            // A Solana token is Jupiter-verified before the card, as a buy asked alone is.
            switch c {
            case .buy(let a, let u) where !a.resolved:
                guard let token = await spotToken(a, note: note) else { chainState.stopped = true; return true }
                c = .buy(token, usdc: u)
            case .sell(let a, let x) where !a.resolved:
                guard let token = await spotToken(a, note: note) else { chainState.stopped = true; return true }
                c = .sell(token, amount: x)
            case .jupiterDCA(let a, let u, let o, let e) where !a.resolved:
                guard let token = await spotToken(a, note: note) else { chainState.stopped = true; return true }
                c = .jupiterDCA(token, usdc: u, orders: o, every: e)
            default: break
            }
            // A paid request first: its price asked now, so the chain's card shows what it pays (the Mac's runSteps →
            // preparePayment); pay() takes this quote at Confirm.
            if i == k, case .pay = c {
                do {
                    switch try await askPrice(c) {
                    case .answered(let text):
                        reply(text, note: note)
                        stop(say("“\(step)” didn’t get to its Confirm, so the steps after it don’t run.", "«\(step)» не дошёл до подтверждения, поэтому следующие шаги не запускаю."))
                        return true
                    case .quoted(let q): payment = q
                    }
                } catch {
                    stop(error.localizedDescription)
                    return true
                }
            }
            if case .sendTo(let name, let amount, let network) = c {
                let found = chainTransfer(name, network: network)
                guard let e = found.entry else { stop(found.why ?? ""); return true }
                // The first step reads the balances as they are now; later ones spend what the steps before bring.
                if i == k, let short = transferShortfall(e.network, amount) {
                    stop(short)
                    noteRefusal(step, short, note: note)
                    return true
                }
                parsed.append(.transfer(e, amount))
            } else {
                if i == k {
                    // Its source read fresh first, as a move asked alone is (freshenSource).
                    await freshenSource(c)
                    if let why = refusalBeforeCard(c) {
                        stop(why)
                        noteRefusal(c.summary, why, note: note)
                        return true
                    }
                }
                parsed.append(.wallet(c))
            }
        }
        let later = asks.map { [say("“\($0)” asks how much when it's reached, then comes on a card of its own; the steps after it wait for that.",
                                    "«\($0)» спросит сумму, когда до него дойдёт, и придёт своей карточкой; шаги после него ждут этого.")] } ?? []
        show(.chain(parsed), title: say("\(parsed.count) steps, one Confirm", "Шагов: \(parsed.count), одно подтверждение", "\(parsed.count) pasos, un Confirm"),
             lines: parsed.enumerated().map { "\($0.offset + 1). \(chainLine($0.element))" } + later
                + [say("They run in order; the first that fails stops the rest. Windows Hello asks for each move that signs.",
                       "Идут по порядку; первый неудачный останавливает остальные. Windows Hello спросит на каждом подписываемом шаге.")])
        if confirmed { await confirm(card: card?.id); return true }
        reply(say("In steps: ", "По шагам: ", "Por pasos: ") + parsed.map(chainLine).joined(separator: " → ")
              + say(". Press Confirm, or Cancel.", ". Нажми «Подтвердить» или «Отмена».", ". Pulsa Confirm o Cancel."), note: note)
        return true
    }

    /// A contract call's card just drawn with steps after it: its card names them, its Confirm covers them, and they run
    /// once the call is through. Nothing is held when the call didn't get to its card.
    func holdAfterCall(_ rest: [String]) {
        guard !rest.isEmpty, case .wallet(.contractCall)? = pending, let id = card?.id else { return }
        let then = [say("Then, on this Confirm, once the call is through: ", "Потом, на этом же «Подтвердить», когда вызов пройдёт: ")
                    + rest.joined(separator: " → ")]
        card?.lines += then
        updateBoard(id) { $0.lines += then }
        chainState.afterCall = (id, canvasID, rest, Date())
        reply(say("The call waits on its card; its Confirm covers the steps after it too — they run once it’s through.",
                  "Вызов ждёт на своей карточке; его «Подтвердить» распространяется и на следующие шаги — они пойдут, когда он пройдёт."))
    }

    /// The call's Confirm went through: the steps it named run now, as on the chain's own Confirm.
    func continueAfterCall(_ board: String?) async {
        guard let held = chainState.afterCall, held.board == board else { return }
        chainState.afterCall = nil
        // The call's own Confirm finishes first (its block marked, the signing session ended): then the rest.
        for _ in 0..<100 where running != nil || runningBoard != nil { try? await Task.sleep(for: .milliseconds(100)) }
        guard running == nil, runningBoard == nil else { return }
        guard held.canvas == canvasID, Date().timeIntervalSince(held.since) < 1800 else { return }
        _ = await chain(held.rest, note: nil, confirmed: true)
    }

    /// A step as the chain's card names it: a paid request with its price («pay 0.01 USDC to StableEnrich»).
    func chainLine(_ s: ChainStep) -> String {
        guard case .wallet(let c) = s, case .pay = c, let q = payment, q.command == c else { return s.title }
        return say("pay \(Fmt.num(q.challenge.usdc, maxFraction: 6)) USDC to \(q.service)", "оплатить \(Fmt.num(q.challenge.usdc, maxFraction: 6)) USDC · \(q.service)")
    }

    /// A USDC transfer to a saved name as a step of a chain: the entry on the network the step names (a 0x address saved
    /// on one EVM network is the same on another), or why it can't be one.
    func chainTransfer(_ name: String, network: String?) -> (entry: SignerAddress?, why: String?) {
        guard var e = entry(name, network: network), e.network != "near", OneClick.Asset.across[e.network] == nil, e.memo.isEmpty || e.network != "solana" else {
            return (nil, say("“\(name)” can't take a USDC transfer in a chain: send it on its own.", "«\(name)» не может получить USDC в цепочке: отправь отдельно."))
        }
        let evm = Self.evmPlaces.union(["hyperliquid"])
        if let net = network, net != e.network {
            guard evm.contains(net), evm.contains(e.network) else {
                return (nil, say("“\(e.name)” is a \(e.network) address: it can't receive on \(net).", "«\(e.name)» — адрес в \(e.network): на \(net) он получить не может."))
            }
            e.network = net
        }
        return (e, nil)
    }

    /// The answer to a chain's «How much?»: that step with its amount, and the steps held behind it, go on as a chain again.
    /// Anything else is left to the plain «How much?» (MoveChecks.answeredAmount), and the held steps go with its question.
    func answeredChainAmount(_ text: String, declines: Bool) async -> Bool {
        guard let held = chainState.held else { return false }
        guard awaitingAmount == held.command else { chainState.held = nil; return false }
        guard !declines, let value = WalletCommand.amountAnswer(text, for: held.command) else { return false }
        chainState.held = nil
        awaitingAmount = nil
        let filled = held.command.with(amount: value)
        _ = await chain([filled.summary] + held.rest, first: filled, note: nil)
        return true
    }

    /// A send no balance can cover, said at once instead of a card whose Confirm could only fail (the Mac's
    /// transferShortfall). It pays from its network, what that lacks from Solana or another EVM network (Executor.sendUSDC);
    /// one on Solana only from Solana. A balance not read yet, or a read that failed (its network then reads as empty),
    /// leaves it to the move.
    func transferShortfall(_ net: String, _ asked: Decimal) -> String? {
        guard portfolio.errors.isEmpty, let have = usdcFree(on: net) else { return nil }
        let others = net == "solana" ? [] : [usdcFree(on: "solana")] + EVMNetwork.searched.filter { $0.key != net }.map { usdcFree(on: $0.key) }
        guard !others.contains(where: { $0 == nil }) else { return nil }
        let all = others.compactMap { $0 }.reduce(have, +)
        guard asked > all else { return nil }
        return say("All the USDC this send can draw on is \(Fmt.num(all)), less than \(Fmt.num(asked)) — nothing was prepared. Say up to that, or top up first.",
                   "Всего USDC, из которых может пойти этот перевод, — \(Fmt.num(all)), меньше \(Fmt.num(asked)). Ничего не готовлю: назови сумму не больше или сначала пополни.")
    }

    /// A step the chain's card runs by its words, drawn as them: a chain never loses a step on the canvas.
    func saidStep(_ text: String) -> Board.Step {
        if WalletCommand.parse(text) == nil, PositionCommand.parse(text) != nil { return Board.Step(label: "HYPERLIQUID", title: text) }
        if WalletCommand.parse(text)?.amountQuestion != nil {
            return Board.Step(label: say("THEN", "ПОТОМ"), title: text, lines: [say("how much\tasked when it's reached", "сколько\tспрошу, когда дойдёт")])
        }
        return Board.Step(label: say("THEN", "ПОТОМ"), title: text)
    }

    // MARK: After Confirm

    /// The chain's Confirm: its steps in order with the bridge legs landing side by side (Executor.landings).
    func runChain(_ steps: [ChainStep]) async throws {
        let landings = Landings()
        try await Executor.$landings.withValue(landings) { try await runChainSteps(steps, landings) }
    }

    private func runChainSteps(_ steps: [ChainStep], _ landings: Landings) async throws {
        var arrived = Set<String>()
        steps: for (i, step) in steps.enumerated() {
            running?.lines.append(say("Step \(i + 1): ", "Шаг \(i + 1): ") + step.title)
            changed()
            let started = Date()
            do {
                switch step {
                case .wallet(var c):
                    // The legs still landing where this step spends (all of them for one that names no place).
                    try await landed(landings, at: Self.source(c))
                    // Money a move before brought to Solana: the step waits until Solana's balance shows it.
                    if arrived.contains("solana"), Self.spends(c) == "solana", let need = Self.solanaUSDC(c) { await waitForDeposit(need, then: c.summary) }
                    if i > 0 { c = await fittedToArrival(c) }
                    try await move(c)
                    if let to = Self.arrives(c) { arrived.insert(to) }
                case .transfer(let entry, let amount):
                    // Its network switched off in Modules after the card was drawn: refused now (Consent.confirmRefusal).
                    if let off = confirmRefusal(.transfer(entry, amount)) { throw ExecutionError(off) }
                    try await landed(landings, at: nil)
                    reply(try await transfer(amount, to: entry), note: "transfer")
                    recordFlow(-amount, "sent to \(entry.name)")
                case .said(let text):
                    try await landed(landings, at: nil)
                    let rest = steps[(i + 1)...].map(\.title)
                    guard try await runSaid(text, rest: rest) else { break steps }
                    // It drew a card of its own (a contract call): that waits for its own Confirm, and the steps after it
                    // run on it once the call is through (the Mac's continueTask).
                    if pending != nil {
                        holdAfterCall(rest)
                        break steps
                    }
                }
            } catch where i > 0 {
                // Steps before went through: said, and the rest stop. Never «Try again» on the whole chain, which would
                // lay out the steps already done once more. Sent with no outcome is «unknown», never «failed».
                let sent = (try? journal.unresolved())?.filter { $0.at >= started.addingTimeInterval(-1) } ?? []
                updateBoard(runningBoard) { $0.state = sent.isEmpty ? "failed" : "unknown" }
                reply(sent.isEmpty ? say("Failed: ", "Не вышло: ") + error.localizedDescription
                      : say("Sent, status unknown: ", "Отправлено, статус неизвестен: ") + error.localizedDescription + " "
                        + say("Check before doing it again: ", "Проверь, прежде чем повторять: ") + sent.map { "\($0.chain) \($0.id)" }.joined(separator: " · "))
                if i + 1 < steps.count {
                    reply(say("“\(step.title)” didn’t go through, so the steps after it don’t run.", "«\(step.title)» не прошёл, поэтому следующие шаги не запускаю."))
                }
                // The model works out why and the way that works, as after a failed Confirm (not in a scheduled run).
                if sent.isEmpty { noteRefusal(step.title, error, note: nil) }
                break
            }
            // The next step reads balances after this one.
            await refresh()
        }
        // What's still landing when the chain ends (it ended on a bridge, or a step stopped it) is waited for and said.
        do { try await landed(landings, at: nil) } catch {
            updateBoard(runningBoard) { $0.state = "failed" }
            reply(error.localizedDescription)
        }
    }

    /// A step of a confirmed chain run by its words: a position managed at once (the card listed it), a move without its
    /// amount asked for with the rest held for the answer, anything else as typed. False when the chain stops here.
    private func runSaid(_ text: String, rest: [String]) async throws -> Bool {
        let command = WalletCommand.parse(text)
        if let c = command, let question = c.amountQuestion {
            chainState.held = (c, rest)
            awaitingAmount = c
            reply(say(question, "Сколько? Напиши число.") + amountHint(c))
            return false
        }
        if command == nil, let p = PositionCommand.parse(text) {
            guard let coin = targetCoin(for: text) else {
                reply(say("There’s no open position for “\(text)”, so the steps stop here.", "Для «\(text)» нет открытой позиции — на этом шаги останавливаются."))
                return false
            }
            if case .trail(let pct) = p { reply(trail(coin, pct), note: "Hyperliquid") }
            else { reply(try await executor().manage(p, asset: coin, policy: policy, equity: ruleEquity), note: "Hyperliquid") }
            return true
        }
        guard await runCanonical(text, userText: text) else {
            reply(say("ihatecrypto can’t do “\(text)” — the steps stop here.", "ihatecrypto не умеет «\(text)» — на этом шаги останавливаются."))
            return false
        }
        return true
    }

    /// Waits for the legs still landing at `place` (nil: all of them), the running card saying so; each landing says how
    /// it went. One that failed stops the chain.
    private func landed(_ landings: Landings, at place: String?) async throws {
        guard landings.count(at: place) > 0 else { return }
        running?.lines.append(say("Waiting for the moves before to land", "Жду, пока придут деньги с прошлых шагов", "Esperando a que lleguen los pasos anteriores"))
        changed()
        do {
            for line in try await landings.wait(for: place) { reply(line) }
        } catch {
            throw ExecutionError(say("A move before didn't land, so the steps after it don't run: ", "Деньги с прошлого шага не пришли, дальше не иду: ")
                                 + error.localizedDescription)
        }
    }

    /// A step that spends on Solana what a move before it brought: an «ARRIVES · SOLANA» card on the route until Solana's
    /// balance shows the money, two minutes at most; then the step goes on and its own check says what's short.
    private func waitForDeposit(_ need: Decimal, then step: String) async {
        guard let owner = solanaAddress else { return }
        let rpc = SolanaRPC()
        func usdc() async -> Decimal { (try? await rpc.ownedAmount(owner: owner, mint: SpotAsset.usdc.mint)) ?? 0 }
        let have = await usdc()
        guard have < need else { return }
        func card(_ have: Decimal) -> Board.Step {
            Board.Step(label: "ARRIVES · SOLANA", title: "\(Fmt.num(need)) USDC",
                       lines: [say("on Solana now", "на Solana сейчас", "en Solana ahora") + "\t\(Fmt.num(have)) USDC", say("Then: ", "Потом: ", "Luego: ") + step])
        }
        let id = runningBoard
        var at = 0
        updateBoard(id) { b in
            var list = b.steps ?? []
            at = list.firstIndex { $0.title == step } ?? list.count
            list.insert(card(have), at: at)
            b.steps = list
        }
        running?.lines.append(say("Waiting for the deposit on Solana", "Жду депозит на Solana", "Esperando el depósito en Solana"))
        changed()
        for _ in 0..<60 {
            try? await Task.sleep(for: .seconds(2))
            let now = await usdc()
            updateBoard(id) { b in if b.steps?.indices.contains(at) == true { b.steps?[at] = card(now) } }
            if now >= need { break }
        }
        // It was a wait, not a step: the route goes back to the moves.
        updateBoard(id) { b in if b.steps?.indices.contains(at) == true, b.steps?[at].label == "ARRIVES · SOLANA" { b.steps?.remove(at: at) } }
    }

    /// A step that spends money on a network, run after a move landed there: when the move's fee left it a little short
    /// of the step's amount (cents from an L2, under a dollar from Ethereum), the step spends what is there. A bigger gap
    /// is left for the step to refuse.
    func fittedToArrival(_ c: WalletCommand) async -> WalletCommand {
        // «buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit»: the swap gave 749.8, and the step takes
        // what it gave rather than failing on the cents.
        let token: (symbol: String, asked: Decimal)? = switch c {
        case .sendToken(_, let t, let x?, "solana"?): (t, x)
        case .lendDeposit(let t, let x?) where !["USDC", "SOL"].contains(t.uppercased()): (t, x)
        default: nil
        }
        let rpc = SolanaRPC()
        if let token, let owner = solanaAddress {
            // The mint from the dollar coins' table or the tokens already read.
            let mint = Stablecoins.byNetwork["solana"]?.first { $0.symbol.uppercased() == token.symbol.uppercased() }?.address
                ?? portfolio.networks.first { $0.key == "solana" }?.rows.first { $0.symbol.uppercased() == token.symbol.uppercased() || $0.contract == token.symbol }?.contract
            if let mint, let have = try? await rpc.ownedAmount(owner: owner, mint: mint) {
                return Self.fitted(c, asked: token.asked, there: have, dollars: false) ?? c
            }
            return c
        }
        guard let net = Self.spends(c), let asked = c.editableAmount, asked > 0 else { return c }
        var there: Decimal?
        if net == "solana", let owner = solanaAddress {
            there = try? await rpc.ownedAmount(owner: owner, mint: SpotAsset.usdc.mint)
        } else if let n = EVMNetwork.named(net), let owner = evmAddress, let units = try? await n.usdcBalance(owner) {
            // Tempo's fees come out of the USDC itself: a cent stays.
            there = max(0, Decimal(units) / 1_000_000 - (n.key == "tempo" ? Decimal(string: "0.01")! : 0))
        }
        guard let there else { return c }
        return Self.fitted(c, asked: asked, there: there, dollars: true) ?? c
    }

    /// The step for what arrived, when it's within 2 % of what it asked (dollars: or within $1); nil when the gap is
    /// bigger, nothing arrived, or the fitted step wouldn't read back the same from its words.
    nonisolated static func fitted(_ c: WalletCommand, asked: Decimal, there: Decimal, dollars: Bool) -> WalletCommand? {
        let slack = asked * Decimal(string: "0.02")!
        guard there > 0, there < asked, asked - there <= (dollars ? max(1, slack) : slack) else { return nil }
        let fitted = c.with(amount: dollars ? there.rounded(2, .down) : there)
        return fitted != c && WalletCommand.parse(fitted.summary) == fitted ? fitted : nil
    }

    // MARK: Where a step's money is

    /// The network a move delivers USDC to («solana» for Solana); nil for anything else.
    nonisolated static func arrives(_ c: WalletCommand) -> String? {
        switch c {
        case .bridge(_, let to, _): to == "hyperliquid" ? nil : to
        case .evmToSolana, .baseToSolana, .arbitrumToSolana, .robinhoodToSolana, .withdrawToSolana, .sellNear: "solana"
        case .solanaTo(let to, _): to
        default: nil
        }
    }

    /// The network a step spends USDC on; nil when it spends none.
    nonisolated static func spends(_ c: WalletCommand) -> String? {
        switch c {
        case .aave(true, _, let n), .morpho(true, _, let n), .buyOn(let n, _, _, _): n
        case .lendDeposit, .kamino(true, _, _), .buy, .hlp(true, _), .allocate(_, _, _, _, from: "solana"), .solanaTo, .jupiterDCA: "solana"
        case .buyStock: "robinhood"
        default: nil
        }
    }

    /// Where a step takes its money from: what it spends on, or the place a bridge leg leaves; nil when it names none.
    nonisolated static func source(_ c: WalletCommand) -> String? {
        switch c {
        case .bridge(let from, _, _), .cctp(let from, _, _), .evmToSolana(let from, _), .usdt(let from, _, _, _): from
        case .baseToSolana: "base"
        case .arbitrumToSolana: "arbitrum"
        case .robinhoodToSolana: "robinhood"
        case .withdrawToSolana: "hyperliquid"
        case .sellNear: "near"
        default: spends(c)
        }
    }

    /// The USDC a step spends on Solana: what it waits for when a move before it brings the money.
    nonisolated static func solanaUSDC(_ c: WalletCommand) -> Decimal? {
        switch c {
        case .lendDeposit("USDC", let a), .kamino(deposit: true, symbol: "USDC", amount: let a), .hlp(deposit: true, amount: let a), .buy(_, usdc: let a),
             .allocate("USDC", maxShare: _, amount: let a, without: _, from: "solana"), .solanaTo(_, let a), .jupiterDCA(_, let a, _, _): a
        default: nil
        }
    }

    // MARK: A scheduled move at Windows Hello

    /// A scheduled task's move signing with no one at the window: while the signer still waits after a few seconds —
    /// Windows Hello for a new address or a fresh confirmation — a notification says so (the Mac's `.signature`).
    /// Cancel it when the move is through.
    func watchForHello(_ title: String) -> Task<Void, Never> {
        Task { [weak self] in await self?.noticeWhileHelloWaits(title) }
    }

    private func noticeWhileHelloWaits(_ title: String) async {
        var seen = 0
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            seen = signing > 0 ? seen + 1 : 0
            guard seen >= 2 else { continue }
            // A notice only, as the Mac's (record: false): the run itself is what History keeps.
            notify(.signature, say("Scheduled: \(title)", "По расписанию: \(title)"),
                   ledgerActive ? say("Waiting for your Ledger — approve it on the device.", "Ждёт Ledger — подтверди на устройстве.")
                                : say("Waiting for Windows Hello — confirm it when Windows asks.", "Ждёт Windows Hello — подтверди, когда Windows спросит."), record: false)
            return
        }
    }
}

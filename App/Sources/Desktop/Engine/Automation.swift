import Foundation
import IhatecryptoCore
import SignerAPI
import SolanaChain
import TradeExecution
import WalletCore

/// The automation wallet a scheduled task runs on, for the moves inside that run only (`executor` picks it up).
nonisolated enum TaskRun {
    @TaskLocal static var automation: SignerWallet?
    /// Below this the automation wallet can't pay Solana's fee plus a new account's rent.
    static let solFloor = Decimal(string: "0.005")!
}

extension ScheduledTask {
    /// A step that moves money: without the automation wallet it waits for Confirm and Windows Hello.
    var movesMoney: Bool {
        steps.contains { step in
            switch WalletCommand.parse(step) {
            case nil, .yields?, .receive?, .schedule?, .stopSchedules?: false
            default: true
            }
        }
    }

    /// It sells a coin: from the wallet that's on, with Windows Hello, never by itself from the automation wallet.
    var sells: Bool {
        steps.contains { step in
            switch WalletCommand.parse(step) {
            case .sell?, .sellOn?, .sellNear?, .sellStock?, .hlSpotSell?: true
            default: false
            }
        }
    }
}

/// The Mac's automation wallet: a budget beside the main wallet that scheduled tasks spend from without Windows Hello.
/// The signer lets it sign only a task's moves, and only to the user's own wallets or addresses already confirmed in
/// person; what its tasks buy goes on to the wallet that's on, and selling it takes Windows Hello there.
extension Engine {
    var automationWallet: SignerWallet? { status?.wallets?.first { $0.automation == true } }

    /// A task runs by itself from the automation wallet when there is one, it was typed in this window, it isn't set to pay
    /// from the wallet that's on, and it sells nothing.
    func usesAutomation(_ t: ScheduledTask) -> Bool { t.untrusted != true && t.fromActive != true && !t.sells && automationWallet != nil }

    /// Its moves would go with no one at the computer: a new one starts paused, and only its switch in the window — a
    /// click, never a chat message — turns it on.
    func waitsForSwitch(_ t: ScheduledTask) -> Bool { usesAutomation(t) && (t.movesMoney || t.steps.contains { PositionCommand.parse($0) != nil }) }

    func createAutomationWallet(note: String? = nil) async {
        guard automationWallet == nil else {
            reply(say("There's an automation wallet already: «\(automationWallet?.label ?? "")».", "Кошелёк автоматизации уже есть: «\(automationWallet?.label ?? "")»."), note: note)
            return
        }
        do {
            try await signerCall(.createAutomationWallet)
            // A task made before this wallet would now spend from it by itself: each waits for its switch, as a new one does.
            let held = tasks.indices.filter { tasks[$0].paused != true && waitsForSwitch(tasks[$0]) }
            for i in held { tasks[i].paused = true }
            // Into Addresses on Solana and Base, so «send 20 USDC to Automation» tops it up: the signer counts its own
            // wallets as known, so no Windows Hello for these.
            if let w = automationWallet {
                for (address, network) in [(w.solana, "solana"), (w.evm, "base")] {
                    guard let address, !book.contains(where: { $0.address.lowercased() == address.lowercased() && $0.network == network }) else { continue }
                    try? await saveAddress(SignerAddress(name: w.label, network: network, address: address, memo: ""))
                }
            }
            pin(Board(kind: "schedule", title: "schedule"))
            reply(say("Made «\(automationWallet?.label ?? "Automation")»: scheduled tasks spend from it without Windows Hello. Keep only their budget there — send it USDC and a little SOL (its address is on the Schedule block).",
                      "Создан «\(automationWallet?.label ?? "Automation")»: задачи по расписанию тратят с него без Windows Hello. Держи там только их бюджет — отправь на него USDC и немного SOL (адрес на блоке «Расписание»).")
                  + (held.isEmpty ? "" : say(" Tasks made before it would now spend from it by themselves, so they’re paused (\(held.count)): each starts only when you switch it on in the Schedule.",
                                             " Задачи, созданные раньше, теперь тратили бы с него сами, поэтому они на паузе (\(held.count)): каждая начнёт работать, только когда ты включишь её в «Расписании».")),
                  note: note)
            if let w = automationWallet {
                options = [ChatRouter.Option(title: say("Top up 20 USDC", "Пополнить на 20 USDC"),
                                             comment: say("From your wallet on Solana: Windows Hello once for the new address.", "С твоего кошелька на Solana: Windows Hello один раз на новый адрес."),
                                             command: "send 20 USDC to \(w.label)", recommended: true)]
            }
        } catch {
            reply(say("No automation wallet: ", "Кошелёк автоматизации не создан: ") + error.localizedDescription, note: note)
        }
    }

    /// The switch on the Schedule block.
    func setTask(_ id: String, on: Bool) {
        guard let i = tasks.firstIndex(where: { $0.id.uuidString == id }) else { return }
        tasks[i].paused = on ? nil : true
        changed()
    }

    // MARK: What the automation wallet bought, owed to the wallet that's on

    /// «solana:<mint>» (SOL as «solana:SOL») or «<network>:<contract>» → amount; kept per main wallet, so a send that
    /// failed goes with the next run.
    var forMain: [String: Decimal] {
        get { (Store.load([String: [String: Decimal]].self, "for-main.json") ?? [:])[activeWallet?.id ?? ""] ?? [:] }
        set {
            var all = Store.load([String: [String: Decimal]].self, "for-main.json") ?? [:]
            all[activeWallet?.id ?? ""] = newValue.isEmpty ? nil : newValue
            Store.save(all, "for-main.json")
        }
    }

    /// A swap on the automation wallet during a task: what it got is owed to the wallet that's on; spending a coin owed
    /// before owes that much less. Dollars stay: they're the budget.
    func owe(_ key: String, _ amount: Decimal) {
        let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2, !(Stablecoins.byNetwork[parts[0]] ?? []).contains(where: { $0.address.lowercased() == parts[1].lowercased() }) else { return }
        var owed = forMain
        let left = (owed[key] ?? 0) + amount
        owed[key] = left > 0 ? left : nil
        forMain = owed
    }

    /// What its tasks bought goes on to the wallet that's on, as it is, signed by the automation wallet: the signer counts
    /// the user's own wallets as known, so nothing waits on a prompt with no one at the computer.
    func sendBoughtToMain(from automation: SignerWallet) async {
        await Task.yield() // the swaps' `owe` hops to the main actor after the move returns
        let owed = forMain
        guard !owed.isEmpty, let main = activeWallet, main.id != automation.id else { return }
        var held: Solana.Balance?
        if let a = automation.solana { held = try? await Solana.balance(owner: a) }
        for (key, amount) in owed.sorted(by: { $0.key < $1.key }) {
            let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
            var left = forMain
            guard parts.count == 2 else { left[key] = nil; forMain = left; continue }
            let (network, token) = (parts[0], parts[1])
            var send = amount
            // What's there now: a later step may have spent some; SOL keeps the floor its fees need.
            if network == "solana", let held {
                send = min(amount, token == "SOL" ? held.sol - TaskRun.solFloor : held.tokens.first { $0.contract == token }?.amount ?? 0)
            }
            guard send > 0 else { left[key] = nil; forMain = left; continue }
            guard let to = network == "solana" ? main.solana : main.evm else { continue }
            do {
                let line = try await TaskRun.$automation.withValue(automation) {
                    try await executor().sendToken(token, amount: send, on: network, to: to, label: main.label)
                }
                left[key] = nil
                forMain = left
                reply(line, note: say("to «\(main.label)»", "на «\(main.label)»"))
            } catch {
                reply(say("It stays on «\(automation.label)» for now: \(error.localizedDescription) The next run sends it on to «\(main.label)».",
                          "Пока остаётся на «\(automation.label)»: \(error.localizedDescription) Следующий запуск отправит это на «\(main.label)»."))
            }
        }
    }

    /// A task's steps on the automation wallet, as the Mac's runSteps(confirmed: true): one confirmed chain, each move
    /// carried out at once and drawn on the canvas as it runs — a send to a saved name, a payment, a step that only shows
    /// something, the next step spending what the one before brought; the first that fails stops the rest.
    /// False when the run stopped before anything moved (a step it can't take, said in the chat).
    @discardableResult
    func runOnAutomation(_ task: ScheduledTask, _ automation: SignerWallet) async -> Bool {
        let started = Date()
        reply(say("From the automation wallet «\(automation.label)», no Windows Hello.", "С кошелька автоматизации «\(automation.label)», без Windows Hello."), note: "schedule")
        guard await TaskRun.$automation.withValue(automation, operation: { await runAutomationSteps(task) }) else { return false }
        // Short of SOL, it fails the same way every run: said with what its tasks need for 4 weeks, and that as a card.
        if boards.contains(where: { $0.kind == "move" && $0.state == "failed" && $0.at >= started }),
           let a = automation.solana, let sol = try? await Solana.balance(owner: a).sol, sol < TaskRun.solFloor {
            let need = Fmt.num(max(solShort(sol), TaskRun.solFloor - sol), maxFraction: 9)
            reply(say("The automation wallet «\(automation.label)» has \(Fmt.num(sol, maxFraction: 6)) SOL — too little for Solana’s fees and the accounts a move opens. Send it \(need) SOL: enough for its tasks for the next 4 weeks.",
                      "На кошельке автоматизации «\(automation.label)» \(Fmt.num(sol, maxFraction: 6)) SOL — мало для комиссий Solana и счетов, которые открывает движение. Отправь на него \(need) SOL — хватит на его задачи на 4 недели вперёд."), note: "schedule")
            options = [ChatRouter.Option(title: say("Send \(need) SOL to \(automation.label)", "Отправить \(need) SOL на \(automation.label)"),
                                         comment: say("From your main wallet: Confirm and Windows Hello.", "С основного кошелька: «Подтвердить» и Windows Hello."),
                                         command: "send \(need) SOL to \(automation.label)", recommended: true)]
        }
        await sendBoughtToMain(from: automation)
        return true
    }

    /// The task's steps as the chain a Confirm would run: moves resolved and checked as their cards would be, then
    /// carried out in order on one block. False when a step stopped it before anything ran.
    private func runAutomationSteps(_ task: ScheduledTask) async -> Bool {
        guard let parsed = await automationSteps(task) else { return false }
        let title = parsed.count == 1 ? parsed[0].title : task.steps.joined(separator: say(" then ", " потом "))
        await runAutomationChain(parsed, title: title)
        return true
    }

    /// A task's steps as chain steps: a Solana token Jupiter-verified first, Modules asked, a send to a saved name as the
    /// transfer to that entry, a contract call or a step that moves nothing run by its words. Nil when one can't go (said).
    func automationSteps(_ task: ScheduledTask) async -> [ChainStep]? {
        var parsed: [ChainStep] = []
        for step in task.steps {
            // A contract call is never carried out unseen: its card waits for Confirm (the Mac's runSteps).
            guard task.movesMoney, var c = WalletCommand.parse(step), case .move = chainKind(step, c), ContractCall.parse(step) == nil else { parsed.append(.said(step)); continue }
            // A move that names no amount never runs as «all of it»: the Mac's run asks «How much?» and stops there.
            // Nobody may be at the computer, so nothing moves until the task says how much.
            if let question = c.amountQuestion {
                reply(say("“\(step)” doesn't say how much (\(question)), so this run stops before anything moves. Change the task to name the amount.",
                          "В «\(step)» нет суммы, поэтому запуск остановлен, ничего не ушло. Измени задачу и укажи сумму."), note: "schedule")
                return nil
            }
            switch c {
            case .buy(let a, let u) where !a.resolved:
                guard let token = await spotToken(a, note: "schedule") else { return nil }
                c = .buy(token, usdc: u)
            case .sell(let a, let x) where !a.resolved:
                guard let token = await spotToken(a, note: "schedule") else { return nil }
                c = .sell(token, amount: x)
            case .jupiterDCA(let a, let u, let o, let e) where !a.resolved:
                guard let token = await spotToken(a, note: "schedule") else { return nil }
                c = .jupiterDCA(token, usdc: u, orders: o, every: e)
            default: break
            }
            if let off = Modules.current.refusal(c, ru: russian) { reply(off, note: "schedule"); return nil }
            if case .sendTo(let name, let amount, let network) = c {
                let found = chainTransfer(name, network: network)
                guard let e = found.entry else { reply(found.why ?? "", note: "schedule"); return nil }
                parsed.append(.transfer(e, amount))
            } else {
                parsed.append(.wallet(c))
            }
        }
        return parsed
    }

    private func runAutomationChain(_ parsed: [ChainStep], title: String) async {
        let id = pin(Board(kind: "move", title: title, steps: steps(for: .chain(parsed), title: title, lines: []), state: "running"))
        runningBoard = id
        running = (title, [])
        changed()
        let said = messages.count, started = Date()
        // No one may be at the window: Windows Hello waiting on a move is told as a notification.
        let hello = watchForHello(title)
        do {
            try await runChain(parsed)
            updateBoard(id) { if $0.state == "running" { $0.state = "done" } }
        } catch {
            let sent = (try? journal.unresolved())?.filter { $0.at >= started.addingTimeInterval(-1) } ?? []
            updateBoard(id) { $0.state = sent.isEmpty ? "failed" : "unknown" }
            reply(sent.isEmpty ? say("Failed: ", "Не вышло: ") + error.localizedDescription
                  : say("Sent, status unknown: ", "Отправлено, статус неизвестен: ") + error.localizedDescription + " "
                    + say("Check before doing it again: ", "Проверь, прежде чем повторять: ") + sent.map { "\($0.chain) \($0.id)" }.joined(separator: " · "), note: "schedule")
        }
        hello.cancel()
        let outcome = messages.dropFirst(said).last { !$0.fromUser }?.text
        updateBoard(id) { b in if let outcome { b.lines.append(String(outcome.prefix(400))) } }
        runningBoard = nil
        running = nil
        lastOwnMove = Date()
        await refresh()
    }

    // MARK: Recurring buys (the Mac's runSchedules)

    /// The tasks that are recurring buys: made by a recurring buy's first Confirm, they run at their time from the wallet
    /// that's on with Windows Hello only, as the Mac's schedules do — no card, no waiting for the person to step away.
    /// Kept per wallet, as its tasks are (the Mac keeps schedules per wallet): a switch never turns them into plain tasks.
    var recurringBuys: Set<UUID> {
        get { Store.load(Set<UUID>.self, Self.scoped("recurring-buys.json", WalletScope.tasks)) ?? [] }
        set { Store.save(newValue, Self.scoped("recurring-buys.json", WalletScope.tasks)) }
    }

    /// Due recurring buys, one run each; the next date moves on even after a failure, so it doesn't retry in a loop.
    func runRecurringBuys() async {
        let marked = recurringBuys
        guard !marked.isEmpty, settings.schedulePaused != true, !watching else { return }
        // A marker whose task is gone goes with it.
        if !marked.isSubset(of: Set(tasks.map(\.id))) { recurringBuys = marked.intersection(Set(tasks.map(\.id))) }
        for id in tasks.filter({ marked.contains($0.id) && $0.paused != true && $0.next <= Date() }).map(\.id) {
            // Found again after every await: «stop recurring buys» or a deletion meanwhile changes the list.
            guard running == nil, signing == 0, let i = tasks.firstIndex(where: { $0.id == id }), tasks[i].next <= Date(),
                  case .buy(let asset, let usdc?)? = WalletCommand.parse(tasks[i].steps.first ?? "") else { continue }
            // On a Ledger a purchase needs the device and the person: while it's away the buy stays due and says so once;
            // when it's back, it runs and asks for its approval there (the Mac's runSchedules).
            if ledgerActive, !(await ledgerReachable()) {
                if Self.ledgerWaitNotified.insert(id).inserted {
                    notify(.signature, say("Scheduled \(asset.symbol) buy waits for your Ledger", "Регулярная покупка \(asset.symbol) ждёт Ledger"),
                           say("Connect and unlock the Ledger: the buy runs then and asks you to approve it on the device.",
                               "Подключи и разблокируй Ledger: покупка запустится и попросит подтверждение на устройстве."))
                }
                continue
            }
            Self.ledgerWaitNotified.remove(id)
            guard let i = tasks.firstIndex(where: { $0.id == id }) else { continue }
            var task = tasks[i]
            _ = task.advance()
            if let kept = Self.keptNext.removeValue(forKey: id.uuidString), kept > Date() { task.next = kept }
            Self.runNowKeys.remove("task:\(id)")
            tasks[i] = task
            await recurringBuy(asset, usdc: usdc, task: task)
        }
    }

    /// The symbol each recurring buy kept by its mint goes by (its step names only the mint), from its first buy and each
    /// run since: the Schedule's row and the notices say «SAMO», not the mint.
    var tokenNames: [String: String] {
        get { Store.load([String: String].self, "token-names.json") ?? [:] }
        set { Store.save(newValue, "token-names.json") }
    }

    /// «buy token 7xKX… for 5 USDC» as the person reads it: «buy SAMO for 5 USDC».
    func shownStep(_ step: String) -> String {
        guard case .buy(let a, let u)? = WalletCommand.parse(step), !a.mint.isEmpty, let name = tokenNames[a.mint] else { return step }
        return "buy \(name) for \(u.map { Fmt.num($0, maxFraction: 6) } ?? "all") USDC"
    }

    /// A recurring buy's coin by its symbol, kept as it's made: its step keeps only the mint.
    func rememberName(_ buy: WalletCommand) {
        guard case .buy(let a, _) = buy, !a.mint.isEmpty, !a.symbol.contains("…"), !SpotAsset.all.contains(where: { $0.mint == a.mint }) else { return }
        tokenNames[a.mint] = a.symbol
    }

    /// Recurring buys told once that they wait for the Ledger.
    static var ledgerWaitNotified: Set<UUID> = []

    /// The step a recurring buy keeps: a coin off the fixed list by its mint, so each run buys that token and never
    /// another one trading under its ticker (the Mac keeps the schedule's asset).
    static func recurringStep(_ buy: WalletCommand) -> String {
        guard case .buy(let a, let u) = buy, !a.mint.isEmpty, a != .usdc, !SpotAsset.all.contains(where: { $0.mint == a.mint }) else { return buy.summary }
        return "buy token \(a.mint) for \(u.map { Fmt.num($0, maxFraction: 6) } ?? "all") USDC"
    }

    /// One run: no chat line when it goes through, a notification and its record either way (the Mac's notify with
    /// `schedule:`) — the Schedule's History, the task's own canvas and History › Agent list it.
    private func recurringBuy(_ asset: SpotAsset, usdc: Decimal, task: ScheduledTask) async {
        // A coin kept by its mint shows the symbol it had (its step names only the mint).
        var name = tokenNames[asset.mint] ?? asset.symbol
        var title = say("\(name) purchase", "Покупка \(name)")
        let started = Date()
        func record(_ result: String, _ line: String) {
            scheduleRuns.append(ScheduleRun(at: started, title: task.steps.map(shownStep).joined(separator: " then "), result: result, line: String(line.prefix(300)), canvas: nil, task: task.id.uuidString))
            if scheduleRuns.count > 100 { scheduleRuns.removeFirst(scheduleRuns.count - 100) }
        }
        // Jupiter or Solana switched off: this run is skipped, the schedule stays.
        if let off = Modules.current.refusal(.buy(asset, usdc: usdc), ru: russian) {
            notify(.moves, say("Scheduled \(name) buy skipped", "Регулярная покупка \(name) пропущена"), off, failed: true)
            record("failed", off)
            return
        }
        guard let token = await spotToken(asset, note: "schedule") else {
            record("failed", messages.last { !$0.fromUser }?.text ?? title)
            return
        }
        if !asset.mint.isEmpty, token.symbol != name, !SpotAsset.all.contains(where: { $0.mint == asset.mint }) {
            tokenNames[asset.mint] = token.symbol
            (name, title) = (token.symbol, say("\(token.symbol) purchase", "Покупка \(token.symbol)"))
        }
        running = (title, [])
        changed()
        // Nobody may be at the computer: a session opened for a few minutes doesn't cover this buy, Windows Hello asks.
        _ = try? await SignerLink.shared.send(.endSession)
        let hello = watchForHello(title)
        do {
            let line = try await executor().swap(token, buy: true, amount: usdc, equity: portfolio.total)
            notify(.moves, title, line)
            record("done", line)
        } catch {
            let text = say("Scheduled \(name) purchase failed: \(error.localizedDescription) Next one: \(Fmt.date(task.next)).",
                           "Регулярная покупка \(name) не прошла: \(error.localizedDescription) Следующая: \(Fmt.date(task.next)).")
            reply(text, note: "schedule")
            notify(.moves, say("\(name) purchase failed", "Покупка \(name) не прошла"), text, failed: true)
            record("failed", text)
        }
        hello.cancel()
        running = nil
        lastOwnMove = Date()
        changed()
        await refresh()
    }
}

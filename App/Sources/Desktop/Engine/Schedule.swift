import CWinSec
import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// One run of a scheduled task, for the Schedule's History tab (the Mac's activities with a schedule): when, what, how it
/// ended and the canvas it ran on.
nonisolated struct ScheduleRun: Codable, Equatable {
    var at: Date
    var title: String
    /// «done», «failed», «unknown» (sent, its outcome not known), or «nothing»: a check whose condition wasn't met.
    var result: String
    /// What it said last: the row's tooltip.
    var line: String?
    var canvas: String?
    /// The task's id: its own canvas lists its runs (the Mac's ScheduleItemBoard «LAST RUNS»).
    var task: String?
}

/// The automation wallet's budget as the Schedule shows it (the Mac's AutomationBudget and the numbers drawn from it):
/// its money on Solana and on each EVM network, how many weeks of its buys that covers, and the gas it lacks.
extension ScheduledTask {
    /// It buys coins: what it gets goes on to the wallet that's on after a run from the automation wallet.
    var buysCoins: Bool {
        steps.contains { step in
            switch WalletCommand.parse(step) {
            case .buy?, .buyStock?, .buyOn?: true
            default: false
            }
        }
    }

    /// It spreads USDC across the best rates: it spends what's free where it pays from.
    var spreadsUSDC: Bool {
        steps.contains { if case .allocate("USDC", _, _, _, _)? = WalletCommand.parse($0) { true } else { false } }
    }
}

nonisolated struct AutomationBudget: Encodable, Equatable {
    struct Coin: Encodable, Equatable { var symbol: String; var amount: Decimal; var usd: Decimal? }
    struct Gas: Encodable, Equatable { var network: String; var coin: String; var amount: Decimal }
    /// USDC on Solana, SOL for its fees, the other Solana coins; USDC on each EVM network that has some.
    var solana: Decimal?
    var sol: Decimal?
    var tokens: [Coin] = []
    var evm: [String: Decimal] = [:]
    /// Circle's USDC at its NEAR account.
    var near: Decimal?
    /// The USDC everywhere; Solana's and the EVM networks' money in dollars, for the Wallet tab's rows.
    var total: Decimal = 0
    var solanaUSD: Decimal?
    var evmUSD: Decimal = 0
    /// Whole weeks of the tasks' buys it covers (nil: they buy nothing), and the USDC short for the next 7 days.
    var weeks: Int?
    var shortForWeek: Decimal?
    /// What it lacks for gas: SOL, then each EVM network's coin where its USDC is. Empty: enough.
    var gas: [Gas] = []
}

/// «every week: buy SOL for 10 USDC», «in 2 hours: withdraw 5 USDC to Solana»: the shared ScheduledTask, run by the
/// minute while ihatecrypto runs (in the tray too). With an automation wallet a task the person switched on runs its moves
/// from it by itself; without one, every money move a task makes comes as a card that waits for Confirm, with a system
/// notification.
extension Engine {
    func taskText(_ t: ScheduledTask) -> String {
        t.steps.map(shownStep).joined(separator: " then ") + " · " + t.when + (t.paused == true ? say(" · paused", " · на паузе") : "")
    }

    /// The steps must be what this app runs.
    func runnable(_ step: String) -> Bool {
        knows(step) || AgentCheck.parse(step) != nil
            || IntentParser.apply(step, to: TradeIntent(), markets: Set(markets.keys)) != nil
    }

    func schedule(_ task: ScheduledTask, note: String?) {
        if let bad = task.steps.first(where: { !runnable($0) }) {
            reply(say("“\(bad)” isn't something the Windows app can do yet. Nothing was scheduled.", "«\(bad)» Windows-версия пока не умеет. Ничего не запланировано."), note: note)
            return
        }
        let what = task.steps.joined(separator: say(", then ", ", потом "))
        // The same steps at the same times again (a repeated message, a test run twice) would run them twice.
        if let same = tasks.first(where: { $0.steps == task.steps && $0.every == task.every && abs($0.next.timeIntervalSince(task.next)) < 60 }) {
            pin(Board(kind: "schedule", title: "schedule"))
            if same.paused == true, waitsForSwitch(same) {
                reply(say("That’s scheduled and paused: \(what) — \(same.when). It starts only when you switch it on in the Schedule.",
                          "Это уже запланировано и стоит на паузе: \(what) — \(same.when). Задача начнёт работать, только когда ты включишь её в «Расписании»."), note: note)
            } else if same.paused == true {
                setTask(same.id.uuidString, on: true)
                reply(say("That was scheduled and paused: \(what) — \(same.when). It’s on again.", "Это уже было запланировано и стояло на паузе: \(what) — \(same.when). Снова включено."), note: note)
            } else {
                reply(say("That’s already scheduled: \(what) — \(same.when). It’s on the canvas.", "Это уже запланировано: \(what) — \(same.when). Это на холсте."), note: note)
            }
            return
        }
        var task = task
        // A top-up of the automation wallet pays from the wallet that's on, with Confirm: never from itself.
        if let a = automationWallet, task.steps.contains(where: { step in
            switch WalletCommand.parse(step) {
            case .sendTo(let name, _, _)?, .sendToken(let name, _, _, _)?, .sendSol(let name, _)?: name.lowercased() == a.label.lowercased()
            default: false
            }
        }) { task.fromActive = true }
        let paused = waitsForSwitch(task)
        if paused { task.paused = true }
        tasks.append(task)
        pin(Board(kind: "schedule", title: "schedule"))
        let how = paused
            ? say(" Its moves run by themselves from «\(automationWallet?.label ?? "Automation")», no Windows Hello — so it starts paused: switch it on in the Schedule block.",
                  " Движения пойдут сами с «\(automationWallet?.label ?? "Automation")», без Windows Hello — поэтому задача на паузе: включи её в блоке «Расписание».")
            : task.sells && automationWallet != nil
            ? say(" A sale waits for your Confirm and Windows Hello.", " Продажа будет ждать «Подтвердить» и Windows Hello.")
            : task.movesMoney ? say(" A money move comes as a card for Confirm; to run them by themselves, say «create an automation wallet».",
                                    " Перевод придёт карточкой на подтверждение; чтобы шли сами, скажи «создай кошелёк автоматизации».") : ""
        reply(say("Scheduled: \(taskText(task)). It runs while ihatecrypto is open or in the tray.", "Запланировано: \(taskText(task)). Выполнится, пока ihatecrypto открыт или в трее.") + how, note: note)
    }

    func cancelTasks(note: String?) {
        let n = tasks.count
        tasks = []
        reply(n == 0 ? say("Nothing was scheduled.", "Ничего не было запланировано.") : say("Scheduled tasks cancelled (\(n)).", "Запланированное отменено (\(n))."), note: note)
    }

    /// The automation wallet's money, read with the balances: Solana from its node, USDC and the gas coin on every EVM
    /// network from theirs (an explorer lists a new address minutes late).
    func refreshAutomationBudget() async {
        guard let w = automationWallet else { automationBudget = nil; return }
        var b = AutomationBudget()
        if let owner = w.solana, let read = try? await Solana.balance(owner: owner) {
            b.solana = read.usdc
            b.sol = read.sol
            b.tokens = read.tokens.filter(\.looksReal).map { .init(symbol: $0.symbol, amount: $0.amount, usd: $0.usd) }
            b.solanaUSD = read.usdc + b.tokens.compactMap(\.usd).reduce(0, +) + read.sol * (markets["SOL"]?.markPx ?? 0)
        }
        if let owner = w.evm {
            for net in EVMNetwork.all {
                guard let raw = try? await net.usdcBalance(owner), raw > 0 else { continue }
                let usdc = Decimal(raw) / 1_000_000
                b.evm[net.key] = usdc
                // Three moves at today's fee (an approve and a swap or a deposit, ≈ 300k gas each); after that a move
                // buys its own gas with the USDC.
                if usdc >= 1, net.feeToken == nil, let held = try? await net.rpc.balance(owner), let fee = try? await net.rpc.fees() {
                    let wei: Decimal = 1_000_000_000_000_000_000
                    let short = Self.gasShort(held: Decimal(held) / wei, target: Decimal(900_000) * Decimal(fee.maxFee) / wei)
                    if short > 0 { b.gas.append(.init(network: net.key, coin: net.gasCoin, amount: short)) }
                }
            }
        }
        if let account = w.near, let usdc = Stablecoins.byNetwork["near"]?.first(where: { $0.symbol == "USDC" })?.address,
           let held = await Holdings.near(account: account) {
            b.near = held.first { $0.contract == usdc }?.amount ?? 0
        }
        b.evmUSD = b.evm.values.reduce(0, +)
        b.total = (b.solana ?? 0) + b.evmUSD + (b.near ?? 0)
        automationBudget = b
    }

    /// The budget as read, with what the tasks on now take from it: worked out at every state, so a task switched on or
    /// the schedule paused shows at once.
    /// The USDC a task could spread where it pays from (the Mac's freeUSDC): the automation wallet's budget, or the idle
    /// USDC of the wallet that's on; nil until read.
    func taskFreeUSDC(_ t: ScheduledTask) -> Decimal? {
        if usesAutomation(t) { return automationBudget.map { ($0.solana ?? 0) + $0.evm.values.reduce(0, +) } }
        return usdcFree(on: "solana").map { $0 + EVMNetwork.searched.compactMap { usdcFree(on: $0.key) }.reduce(0, +) }
    }

    /// The task paying from the other wallet: the one that's on instead of the automation wallet, or the other way.
    func otherPayer(_ t: ScheduledTask) -> ScheduledTask {
        var o = t
        o.fromActive = usesAutomation(t) ? true : nil
        return o
    }

    var automationBudgetNow: AutomationBudget? {
        guard automationWallet != nil, var b = automationBudget else { return nil }
        let spend = automationSpend(until: Date().addingTimeInterval(7 * 86_400))
        // Whole weeks: 20 / 3 unrounded read as 0 («not enough for the next run»).
        if spend > 0 { b.weeks = NSDecimalNumber(decimal: (b.total / spend).rounded(0, .down)).intValue }
        if spend > b.total { b.shortForWeek = (spend - b.total).rounded(0, .up) }
        if let sol = b.sol, case let short = solShort(sol), short > 0 { b.gas.insert(.init(network: "solana", coin: "SOL", amount: short), at: 0) }
        return b
    }

    /// What the Schedule and the multisig show, read with the balances each minute.
    func refreshLive() async {
        await refreshMultisigs()
        await refreshAutomationBudget()
    }

    /// What the tasks on the automation wallet take from its budget by `date`: each one's buys times its runs until then.
    // ponytail: buys with an amount only; a spread or a deposit spends what's there, unpriced (as on the Mac).
    func automationSpend(until date: Date) -> Decimal {
        guard automationWallet != nil, settings.schedulePaused != true else { return 0 }
        return tasks.filter { $0.paused != true && usesAutomation($0) }.reduce(0) { sum, t in
            sum + t.steps.reduce(0) { s, step in
                switch WalletCommand.parse(step) {
                case .buy(_, let u?)?, .buyNear(let u?)?, .buyStock(_, let u?)?, .buyOn(_, _, let u?, _)?: s + u
                default: s
                }
            } * Decimal(t.runs(until: date))
        }
    }

    /// SOL it lacks for the next 4 weeks of its tasks: the floor for the accounts a move opens plus 0.00025 SOL a step a
    /// run (Jupiter's priority fee cap, then sending what it bought on to Main); nothing while it holds half of that.
    // ponytail: every step priced as a Solana swap, as on the Mac.
    func solShort(_ held: Decimal) -> Decimal {
        let until = Date().addingTimeInterval(28 * 86_400)
        let steps = settings.schedulePaused == true ? 0 : tasks.filter { $0.paused != true && $0.movesMoney && usesAutomation($0) }
            .reduce(0) { $0 + $1.runs(until: until) * $1.steps.count }
        return Self.gasShort(held: held, target: TaskRun.solFloor + Decimal(steps) * Decimal(string: "0.00025")!, floor: TaskRun.solFloor)
    }

    /// Nothing while `held` is at least half of `target` and the `floor` under which moves fail, else up to the target,
    /// rounded up to two significant digits (topped up exactly, a dearer fee next minute asked for dust).
    static func gasShort(held: Decimal, target: Decimal, floor: Decimal = 0) -> Decimal {
        guard held < max(floor, target / 2) else { return 0 }
        let x = target - held, d = NSDecimalNumber(decimal: x).doubleValue
        guard d > 0 else { return 0 }
        return x.rounded(max(0, 1 - Int(log10(d).rounded(.down))), .up)
    }

    /// A price alert as its block draws it (the Mac's AlertsBoard): when, how far the price is now, then its steps.
    func alertView(_ a: PriceAlert) -> Snapshot.Alert {
        let name = IntentParser.pretty(a.coin).components(separatedBy: " (").first ?? a.coin
        let now = (markets[a.coin]?.markPx).flatMap { $0 > 0 ? $0 : nil }.map { px in
            let away = Fmt.num(abs(((a.price - px) / px * 100).rounded(1, .plain)))
            return say("now \(Fmt.price(px)) · \(away)% away", "сейчас \(Fmt.price(px)) · до цели \(away)%")
        }
        // A trade plan reads as the plan it becomes («THEN · PLAN ON HYPERLIQUID»); anything else as the command it is.
        let known = Set(markets.keys)
        let blocks: [Snapshot.AlertStep] = (a.steps ?? []).map { step in
            guard IntentParser.startsPlan(step, markets: known), let t = IntentParser.apply(step, to: TradeIntent(), markets: known) else {
                return .init(plan: false, coin: nil, title: step, line: nil)
            }
            let rest = t.changes(from: TradeIntent()).filter { !$0.hasPrefix("market") && $0 != "long" && $0 != "short" && !$0.hasPrefix("leverage") }
            return .init(plan: true, coin: t.asset, title: "\(t.side == .long ? say("Long", "Лонг") : say("Short", "Шорт")) \(IntentParser.pretty(t.asset))\(t.leverage.map { " ×" + Fmt.num($0) } ?? "")",
                         line: rest.isEmpty ? nil : rest.joined(separator: " · "))
        }
        return .init(id: a.id.uuidString, text: alertText(a), coin: a.coin,
                     when: "\(name) \(a.above ? say("above", "выше") : say("below", "ниже")) \(Fmt.price(a.price))", now: now, steps: a.steps, blocks: blocks)
    }

    /// «Pause schedule»: nothing scheduled runs until it's resumed; each task keeps its own switch and its next time.
    func pauseSchedule(_ on: Bool) { settings.schedulePaused = on ? true : nil }

    /// The History tab: the newest 30 runs first.
    var recentRuns: [ScheduleRun] { Array(scheduleRuns.suffix(30).reversed()) }

    /// The first task whose time has come, as the Mac's runDueTask: on a canvas of its own (the last run's makes way), and
    /// back to the canvas the person was on once it's done, unless it needs them (a Confirm, a failure). Only while nothing
    /// else is going on: a card waiting for Confirm, a reply being written, a move running.
    /// The due recurring buys and tasks in a task of their own, beside the 30 s loop (the Mac's scheduledRun, d8cc366): an
    /// unanswered Windows Hello holds only this run, while balances, «closed outside» and the trading-key offer go on. One
    /// run at a time; a call while one runs leaves it be, and the next 30 s picks up what's due then.
    func startScheduledRun() {
        guard scheduledRun == nil else { return }
        scheduledRun = Task { await scheduledRunBody() }
    }

    private func scheduledRunBody() async {
        await runDueTasks()
        scheduledRun = nil
    }

    func runDueTasks() async {
        remindTopUp()
        // Recurring buys first: at their time, Windows Hello only, no canvas of their own (Automation.swift).
        await runRecurringBuys()
        guard settings.schedulePaused != true, pending == nil, card == nil, running == nil, !thinking else { return }
        guard let i = tasks.firstIndex(where: { $0.paused != true && $0.next <= Date() }) else { return }
        let key = "task:\(tasks[i].id)"
        // With the person at the computer a clock task waits for them to step away (half an hour at most), so the canvas
        // doesn't change under them. An alert's steps and «Run now» go at once.
        let asked = Self.runNowKeys.contains(key) || tasks[i].because != nil
        guard asked || Self.userAway || Date().timeIntervalSince(tasks[i].next) > 1800 else { return }
        Self.runNowKeys.remove(key)
        var task = tasks[i]
        if task.advance() {
            // «Run now» on a repeating one keeps its next run as it was (the Mac's TaskRun.kept).
            if let kept = Self.keptNext.removeValue(forKey: task.id.uuidString), kept > Date() { task.next = kept }
            tasks[i] = task
        } else { tasks.remove(at: i) }
        let back = canvasID
        // The runs before left their canvases; what they did stays in the History. One the person wrote on stays.
        let before = Set(scheduleRuns.filter { $0.task == task.id.uuidString }.compactMap(\.canvas))
        for c in canvases where c.id != canvasID && before.contains(c.id) && c.schedule == nil && c.messages.filter(\.fromUser).count <= 1 { try? removeCanvas(c.id) }
        guard (try? newCanvas()) != nil else { return }
        let request = (task.because.map { $0 + ": " } ?? say("Scheduled: ", "По расписанию: ")) + task.steps.map(shownStep).joined(separator: say(", then ", ", потом "))
        messages.append(Message(fromUser: true, text: request, note: "schedule"))
        nameCanvas(from: request)
        let started = Date(), mark = messages.count, canvas = canvasID
        var ran = true, stepFailed = false
        let check = task.steps.allSatisfy { AgentCheck.parse($0) != nil }
        if check {
            // A check says something only when its condition is met.
            quietCheck = false
            // Its model answers as a message's would: the chat is busy meanwhile, so no canvas switch or message lands in
            // the middle of it (the Mac's routeWithModel marks itself busy).
            thinking = true
            changed()
            for step in task.steps { _ = await runCanonical(step, userText: step) }
            thinking = false
            changed()
            ran = !quietCheck
        } else if usesAutomation(task), let automation = automationWallet, task.movesMoney {
            if !(await runOnAutomation(task, automation)) { stepFailed = true }
            await refreshAutomationBudget()
        } else if task.steps.count > 1 {
            // A step that can't go on or a first move that can't be paid stops the run before any card: it failed.
            if !(await chain(task.steps, note: "schedule")) || chainState.stopped { stepFailed = true }
        } else if let step = task.steps.first {
            // A spread with nothing free stays quiet too, with a grey mark (the Mac's checkQuiet, Earn.preparePlan):
            // every day's «see the canvas» led to a canvas already gone (the owner, 2026-10-16).
            quietCheck = false
            if !(await runCanonical(step, userText: step)) {
                reply(say("“\(step)” didn't run.", "«\(step)» не выполнилось."), note: "schedule")
                stepFailed = true
            }
            ran = !quietCheck
        }
        logRun(task, started: started, mark: mark, canvas: canvas, ran: ran, failed: stepFailed)
        let failed = scheduleRuns.last?.result == "failed"
        // A card waits for Confirm the way the Mac's move waits on its canvas; a trade plan does too, and one a rule stops
        // (no card) needs the person just as much (the Mac's runDueTask).
        let plan = canvasID == canvas && intent != TradeIntent() && tradedPlan != intent && !positions.contains { $0.coin == intent.asset }
        let waits = card != nil || pending != nil || plan
        let told = messages.dropFirst(mark).last { !$0.fromUser }?.text ?? ""
        // A check with nothing to tell stays quiet: the run is in the History with a grey mark.
        if ran {
            // A check that speaks is a task's run like any other (the Mac's runDueTask), not a price alert.
            notify(waits ? .confirm : .moves, request,
                   plan && card == nil ? say("The plan is on its canvas, but a rule stops it — open it to fix.", "План на своём холсте, но его останавливает правило — открой и поправь.")
                   : waits ? say("Ready on its canvas — press Confirm.", "Готово на своём холсте — нажми «Подтвердить».") : String(told.prefix(200)), failed: failed)
        }
        // Nothing here for the person: back where they were. «Run now» and an alert's task stay to be seen.
        guard !asked, !waits, !failed, canvasID == canvas, messages.filter(\.fromUser).count <= 1,
              canvases.contains(where: { $0.id == back }) else { return }
        try? openCanvas(back)
    }

    /// «Run now» pressed: these go at once, even with the person at the computer.
    static var runNowKeys: Set<String> = []

    /// Nobody at the computer: no key or mouse for five minutes (winsec reads the whole session's input), or ihatecrypto's
    /// window not in front for that long, as the Mac counts another app in front (main.rs sends `windowFocus`).
    static var userAway: Bool { Int(winsec_idle_ms()) >= 300_000 || (!windowFront && Date().timeIntervalSince(windowLeft) >= 300) }
    nonisolated(unsafe) static var windowFront = true
    nonisolated(unsafe) static var windowLeft = Date()

    /// The day before the automation wallet runs short for its buys, once a day: a notification, and with no one at the
    /// computer the Schedule comes up. The wallet that's on signs only with Windows Hello, so a top-up can't go by itself;
    /// asked ahead, a week of buys then runs while the computer is left alone.
    func remindTopUp() {
        guard let w = automationWallet, let b = automationBudgetNow, let short = b.shortForWeek else { return }
        let need = automationSpend(until: Date().addingTimeInterval(86_400))
        let today = Date().formatted(.iso8601.year().month().day())
        guard need > b.total, Store.load(String.self, "top-up-asked.json") != today else { return }
        Store.save(today, "top-up-asked.json")
        let main = activeWallet?.label ?? "Main"
        notify(.confirm, say("«\(w.label)» is short for the next buys", "На «\(w.label)» не хватит на ближайшие покупки"),
               say("The buys in the next 24 hours need \(Fmt.num(need)) USDC; it has \(Fmt.num(b.total)). Top it up with \(Fmt.num(short)) USDC for the week from «\(main)»: Confirm and Windows Hello once, then they run by themselves.",
                   "Покупкам в ближайшие сутки нужно \(Fmt.num(need)) USDC, на нём \(Fmt.num(b.total)). Пополни его на \(Fmt.num(short)) USDC на неделю с «\(main)»: один раз «Подтвердить» и Windows Hello, дальше покупки пойдут сами."))
        if Self.userAway { pin(Board(kind: "schedule", title: "schedule")) }
    }

    /// Where a task pays from: the wallet that's on (Confirm and Windows Hello each run) or the automation wallet. Only
    /// a money task typed in this window that sells nothing can pick, and only with an automation wallet.
    func canPickPays(_ t: ScheduledTask) -> Bool { t.movesMoney && t.untrusted != true && !t.sells && automationWallet != nil }

    func setFromActive(_ id: String, _ on: Bool) {
        guard let i = tasks.firstIndex(where: { $0.id.uuidString == id }), canPickPays(tasks[i]) else { return }
        tasks[i].fromActive = on ? true : nil
    }

    /// A run in the log: failed when one of its moves did (or a step didn't run at all), «nothing» when a check had
    /// nothing to tell. The last 100 are kept.
    private func logRun(_ task: ScheduledTask, started: Date, mark: Int, canvas: String, ran: Bool, failed stepFailed: Bool) {
        let failed = stepFailed || boards.contains { $0.kind == "move" && $0.at >= started && $0.state == "failed" }
        // Sent with no answer back: not a failure to do again (it may have gone through), not done either.
        let unknown = boards.contains { $0.kind == "move" && $0.at >= started && $0.state == "unknown" }
        let line = canvas == canvasID ? messages.dropFirst(mark).last { !$0.fromUser }?.text : nil
        scheduleRuns.append(ScheduleRun(at: started, title: task.steps.joined(separator: " then "), result: failed ? "failed" : unknown ? "unknown" : ran ? "done" : "nothing",
                                        line: line.map { String($0.prefix(300)) }, canvas: canvas, task: task.id.uuidString))
        if scheduleRuns.count > 100 { scheduleRuns.removeFirst(scheduleRuns.count - 100) }
    }
}

// MARK: A scheduled item's own canvas

/// A task (or an open position) on a canvas of its own, as the Mac's ScheduleItemBoard: made the first time, then the
/// same one; its block on top, the chat on it can change it; Close goes back to the canvas it was opened from.
extension Engine {
    /// «Run now» on a repeating task: its next run, put back after this one.
    static var keptNext: [String: Date] = [:]
    /// The canvas an item's canvas was opened from.
    static var scheduleReturn: String?

    /// The open canvas's item: «task:<id>» or «perp:<coin>».
    var scheduleKey: String? { canvases.indices.contains(canvasIndex) ? canvases[canvasIndex].schedule : nil }

    func scheduleTitle(_ key: String) -> String {
        if key.hasPrefix("perp:") {
            let coin = String(key.dropFirst(5))
            let long = positions.first { $0.coin == coin }.map { $0.size > 0 } ?? true
            return "\(long ? say("Long", "Лонг") : say("Short", "Шорт")) \(IntentParser.pretty(coin))"
        }
        return tasks.first { "task:\($0.id)" == key }?.steps.joined(separator: " → ") ?? say("Scheduled task", "Задача по расписанию")
    }

    func openSchedule(_ key: String) throws {
        guard key.hasPrefix("task:") || key.hasPrefix("perp:") else { throw WalletError("Unknown scheduled item «\(key)».") }
        if scheduleKey == nil { Self.scheduleReturn = canvasID }
        if let c = canvases.first(where: { $0.schedule == key }) {
            try openCanvas(c.id)
        } else {
            try newCanvas()
            canvases[canvasIndex].title = String(scheduleTitle(key).prefix(60))
            canvases[canvasIndex].schedule = key
        }
        // Its block on top, put back if it was closed.
        if !boards.contains(where: { $0.kind == "scheduleItem" && $0.title == key }) { pin(Board(kind: "scheduleItem", title: key)) }
    }

    /// Back to the canvas it came from (or the newest other one); an item's canvas nothing was written on leaves the list.
    func closeSchedule() throws {
        guard let key = scheduleKey else { return }
        let id = canvasID, written = canvases[canvasIndex].messages.contains(where: \.fromUser)
        if let back = Self.scheduleReturn, canvases.contains(where: { $0.id == back && $0.schedule == nil }) { try openCanvas(back) }
        else if let other = canvases.first(where: { $0.schedule == nil }) { try openCanvas(other.id) }
        else { try newCanvas() }
        Self.scheduleReturn = nil
        if !written, canvases.contains(where: { $0.id == id && $0.schedule == key }) { try removeCanvas(id) }
    }

    /// «Run now»: due at once, within the minute's run (sooner if nothing else is going on).
    func runNow(_ key: String) async {
        guard let i = tasks.firstIndex(where: { "task:\($0.id)" == key }) else { return }
        if tasks[i].every != nil, tasks[i].next > Date() { Self.keptNext[tasks[i].id.uuidString] = tasks[i].next }
        tasks[i].next = Date()
        Self.runNowKeys.insert(key)
        // After a run already going (one at a time), then this one.
        await scheduledRun?.value
        startScheduledRun()
        await scheduledRun?.value
    }

    /// The item's block in words, for the model: what it is, when, how it signs, how its runs went.
    func scheduleItemFacts(_ key: String) -> [String] {
        if key.hasPrefix("perp:") {
            let coin = String(key.dropFirst(5))
            guard let p = positions.first(where: { $0.coin == coin }) else { return ["closed"] }
            return ["\(p.size > 0 ? "long" : "short") \(Fmt.num(abs(p.size), maxFraction: 6)) at \(Fmt.price(p.entry)), P/L \(Fmt.usd(p.unrealizedPnl)); its stop and take are on the exchange"]
        }
        guard let t = tasks.first(where: { "task:\($0.id)" == key }) else { return ["no longer scheduled"] }
        let runs = scheduleRuns.filter { $0.task == t.id.uuidString }.suffix(5).map { "\(Fmt.date($0.at)): \($0.result)" }
        return [taskText(t), "steps: " + t.steps.joined(separator: " then "),
                usesAutomation(t) && t.movesMoney ? "signs by itself from the automation wallet" : "each move waits for Confirm"] + runs
    }
}

import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution

/// One run of a scheduled task, for the Schedule's History tab (the Mac's activities with a schedule): when, what, how it
/// ended and the canvas it ran on.
nonisolated struct ScheduleRun: Codable, Equatable {
    var at: Date
    var title: String
    /// «done», «failed», or «nothing»: a check whose condition wasn't met.
    var result: String
    /// What it said last: the row's tooltip.
    var line: String?
    var canvas: String?
}

/// The automation wallet's budget as the Schedule shows it (the Mac's AutomationBudget and the numbers drawn from it):
/// its money on Solana and on each EVM network, how many weeks of its buys that covers, and the gas it lacks.
nonisolated struct AutomationBudget: Encodable, Equatable {
    struct Coin: Encodable, Equatable { var symbol: String; var amount: Decimal; var usd: Decimal? }
    struct Gas: Encodable, Equatable { var network: String; var coin: String; var amount: Decimal }
    /// USDC on Solana, SOL for its fees, the other Solana coins; USDC on each EVM network that has some.
    var solana: Decimal?
    var sol: Decimal?
    var tokens: [Coin] = []
    var evm: [String: Decimal] = [:]
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
        t.steps.joined(separator: " then ") + " · " + t.when + (t.paused == true ? say(" · paused", " · на паузе") : "")
    }

    /// The steps must be what this app runs.
    func runnable(_ step: String) -> Bool {
        WalletCommand.parse(step) != nil || PositionCommand.parse(step) != nil || AlertCommand.parse(step, markets: Set(markets.keys)) != nil
            || AgentCheck.parse(step) != nil
            || IntentParser.apply(step, to: TradeIntent(), markets: Set(markets.keys)) != nil
    }

    func schedule(_ task: ScheduledTask, note: String?) {
        if let bad = task.steps.first(where: { !runnable($0) }) {
            reply(say("“\(bad)” isn't something the Windows app can do yet. Nothing was scheduled.", "«\(bad)» Windows-версия пока не умеет. Ничего не запланировано."), note: note)
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
        b.evmUSD = b.evm.values.reduce(0, +)
        b.total = (b.solana ?? 0) + b.evmUSD
        automationBudget = b
    }

    /// The budget as read, with what the tasks on now take from it: worked out at every state, so a task switched on or
    /// the schedule paused shows at once.
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
        return .init(id: a.id.uuidString, text: alertText(a), coin: a.coin,
                     when: "\(name) \(a.above ? say("above", "выше") : say("below", "ниже")) \(Fmt.price(a.price))", now: now, steps: a.steps)
    }

    /// «Pause schedule»: nothing scheduled runs until it's resumed; each task keeps its own switch and its next time.
    func pauseSchedule(_ on: Bool) { settings.schedulePaused = on ? true : nil }

    /// The History tab: the newest 30 runs first.
    var recentRuns: [ScheduleRun] { Array(scheduleRuns.suffix(30).reversed()) }

    /// Tasks that are due run now, one at a time: not over a card already waiting for Confirm, which they'd replace.
    func runDueTasks() async {
        guard settings.schedulePaused != true, pending == nil, running == nil, !thinking else { return }
        guard var task = tasks.first(where: { $0.paused != true && $0.next <= Date() }) else { return }
        tasks.removeAll { $0.id == task.id }
        if task.advance() { tasks.append(task) }
        let started = Date(), mark = messages.count, canvas = canvasID
        var ran = true, stepFailed = false
        defer { logRun(task, started: started, mark: mark, canvas: canvas, ran: ran, failed: stepFailed) }
        // A check says something only when its condition is met: then that is the notification.
        if task.steps.allSatisfy({ AgentCheck.parse($0) != nil }) {
            let mark = messages.count
            quietCheck = false
            for step in task.steps { _ = await runCanonical(step, userText: step) }
            ran = !quietCheck
            if !quietCheck, let said = messages.dropFirst(mark).last(where: { !$0.fromUser }) {
                out.event("notify", ["title": say("Check", "Проверка"), "body": String(said.text.prefix(200))])
            }
            return
        }
        let text = "⏰ " + task.steps.joined(separator: " then ")
        out.event("notify", ["title": say("Scheduled", "По расписанию"), "body": text])
        reply(text, note: "schedule")
        if usesAutomation(task), let automation = automationWallet, task.movesMoney {
            await runOnAutomation(task, automation)
        } else if task.steps.count > 1 {
            if !(await chain(task.steps, note: "schedule")) { stepFailed = true }
        } else if let step = task.steps.first, !(await runCanonical(step, userText: step)) {
            reply(say("“\(step)” didn't run.", "«\(step)» не выполнилось."), note: "schedule")
            stepFailed = true
        }
    }

    /// A run in the log: failed when one of its moves did (or a step didn't run at all), «nothing» when a check had
    /// nothing to tell. The last 100 are kept.
    private func logRun(_ task: ScheduledTask, started: Date, mark: Int, canvas: String, ran: Bool, failed stepFailed: Bool) {
        let failed = stepFailed || boards.contains { $0.kind == "move" && $0.at >= started && $0.state == "failed" }
        let line = canvas == canvasID ? messages.dropFirst(mark).last { !$0.fromUser }?.text : nil
        scheduleRuns.append(ScheduleRun(at: started, title: task.steps.joined(separator: " then "), result: failed ? "failed" : ran ? "done" : "nothing",
                                        line: line.map { String($0.prefix(300)) }, canvas: canvas))
        if scheduleRuns.count > 100 { scheduleRuns.removeFirst(scheduleRuns.count - 100) }
    }
}

import AppKit
import IhatecryptoCore
import SignerAPI
import SolanaChain
import SwiftUI
import TradeExecution
import WalletCore

/// The automation wallet a scheduled task is running on, for the moves inside that run only (`executor` picks it up):
/// a Confirm pressed meanwhile in the window is another task and still signs with the active wallet.
nonisolated enum TaskRun {
    @TaskLocal static var automation: SignerWallet?
    /// «Run now» pressed: these go at once, even with the user at the window.
    @MainActor static var now: Set<String> = []
    /// The next run a repeating item had when «Run now» made its run an extra one: it gets it back after that run, so
    /// «every month on the 1st» run by hand on the 14th still runs on the 1st (it moved to the 14th).
    @MainActor static var kept: [String: Date] = [:]
    /// Below this the automation wallet can't pay Solana's fee plus a new account's rent (a Jupiter Lend deposit failed
    /// with 0.00198 SOL on 2026-10-06; rent-exempt minimum 0.00089 + a token account 0.00204 + fees).
    static let solFloor = Decimal(string: "0.005")!
}

// Scheduled tasks («in 1 week: rebalance USDC», «every day at 09:00: sell SOL then lend USDC» — ScheduledTask) and
// Settings › General › Keep running, which keeps the app up so they run.

extension Workspace {
    /// Keeps a task if every step is something ihatecrypto does, and opens the Schedule board (the rail's button) on the canvas.
    func schedule(_ task: ScheduledTask, note: String? = nil) {
        if let unknown = task.steps.first(where: { !knows($0) }) {
            reply(say("I can schedule that, but ihatecrypto can’t do “\(unknown)” yet — nothing was scheduled.",
                      "Запланировать могу, но «\(unknown)» ihatecrypto пока не умеет — ничего не запланировано."), note: note)
            return
        }
        let what = task.steps.joined(separator: say(", then ", ", потом "))
        // The same steps at the same times again (a repeated message, a test run twice) would run them twice.
        if let same = tasks.first(where: { $0.steps == task.steps && $0.every == task.every && abs($0.next.timeIntervalSince(task.next)) < 60 }) {
            scheduleItem = "task:\(same.id)"
            showSchedule = true
            // A paused one is out of sight on the Schedule: asked for again, it's switched back on, not made twice — unless
            // only the switch may start it (waitsForSwitch).
            if same.paused == true, waitsForSwitch(same) {
                reply(say("That’s scheduled and paused: \(what) — \(same.when). It starts only when you switch it on in the Schedule.",
                          "Это уже запланировано и стоит на паузе: \(what) — \(same.when). Задача начнёт работать, только когда ты включишь её в Schedule."), note: note)
                return
            }
            if same.paused == true {
                setPaused("task:\(same.id)", false)
                reply(say("That was scheduled and paused: \(what) — \(same.when). It’s on again.", "Это уже было запланировано и стояло на паузе: \(what) — \(same.when). Снова включено."), note: note)
                return
            }
            reply(say("That’s already scheduled: \(what) — \(same.when). It’s on the canvas.", "Это уже запланировано: \(what) — \(same.when). Это на канвасе."), note: note)
            return
        }
        var task = task
        if SignerClient.untrusted { task.untrusted = true }
        let paused = waitsForSwitch(task)
        if paused { task.paused = true }
        tasks.append(task)
        // The Schedule canvas shows it in detail: the chat keeps one line.
        scheduleItem = "task:\(task.id)"
        showSchedule = true
        let how = task.steps.allSatisfy({ AgentCheck.parse($0) != nil })
            ? say(" I'll look myself each time and write only when it's met; what I suggest waits for your Confirm.",
                  " Я буду смотреть сам и напишу, только когда условие выполнится; что предложу — будет ждать твоего Confirm.")
            : task.sells && automationWallet != nil
            ? say(" A sale waits for your Confirm and \(Presence.name): what tasks buy lands on this wallet, and it sells from here.",
                  " Продажа будет ждать твоего Confirm и \(Presence.ruGen): купленное задачами лежит на этом кошельке, и продаётся оно отсюда.")
            : usesAutomation(task)
            ? say(" Its moves run by themselves from the automation wallet, no \(Presence.short)", " Движения пойдут сами с кошелька автоматизации, без \(Presence.ruGen)")
                + (task.buysCoins ? say("; what it buys goes on to «\(activeWallet?.label ?? "your wallet")».", "; купленное придёт на «\(activeWallet?.label ?? "твой кошелёк")».") : ".")
                + (automationShortForWeek.map { say(" With it, a week of the Schedule’s buys is \(Fmt.num($0)) USDC short: the Schedule has the top-up.",
                                                    " С ней на неделю покупок не хватает \(Fmt.num($0)) USDC: пополнить можно в Schedule.") } ?? "")
            : say(" Its moves will wait for your Confirm; to run them by themselves, say «create an automation wallet».",
                  " Движения будут ждать твоего Confirm; чтобы шли сами, скажи «создай кошелёк автоматизации».")
        reply(paused ? say("Scheduled and paused: \(what) — \(task.when). It’s on the canvas.", "Запланировано, на паузе: \(what) — \(task.when). Это на канвасе.") + how
                        + say(" It starts only when you switch it on in the Schedule.", " Задача начнёт работать, только когда ты включишь её в Schedule.")
                     : say("Scheduled: \(what) — \(task.when). It’s on the canvas.", "Запланировано: \(what) — \(task.when). Это на канвасе.") + how, note: note)
    }

    /// Its moves would go from the automation wallet with no one at the window: a new one starts paused, and only the
    /// switch on the Schedule — a click in the window, never a chat message or the debug API — turns it on.
    func waitsForSwitch(_ task: ScheduledTask) -> Bool {
        usesAutomation(task) && (task.movesMoney || task.steps.contains { PositionCommand.parse($0) != nil })
    }

    func cancelTasks(note: String? = nil) {
        let n = tasks.count
        tasks = []
        taskQueue = nil
        if n > 0 { showSchedule = true }
        reply(n == 0 ? say("Nothing was scheduled.", "Ничего не было запланировано.")
                     : say("Cancelled \(n) scheduled task\(n == 1 ? "" : "s").", "Отменено запланированных задач: \(n)."), note: note)
    }

    /// Whether a step reads as a command ihatecrypto carries out: the parsers `runCanonical` tries, in its order.
    func knows(_ step: String) -> Bool {
        let m = Set(markets.keys), s = step.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".! "))
        return s == "lock" || s == "clear the plan" || TokenCommand.parse(step) != nil || ChartCommand.parse(step, markets: m) != nil
            || PnLCommand.parse(step, markets: m) != nil || AlertCommand.parse(step, markets: m) != nil
            || AddressBookCommand.parse(step) != nil || FolderCommand.parse(step) != nil || WalletCommand.parse(step) != nil || PositionCommand.parse(step) != nil
            || (IntentParser.startsPlan(step, markets: m) && IntentParser.apply(step, to: TradeIntent(), markets: m) != nil)
    }

    /// The first task whose time has come, in the background: on a canvas of its own (one per task — the last run's makes
    /// way), and back to the canvas the user was on once it's done, unless it needs them (a Confirm, a plan's Execute, a
    /// failure to fix). Only once nothing else is going on: the user's card waiting for Confirm, a reply being written, a
    /// move running or half-typed text come first.
    func runDueTask() async {
        guard !schedulePaused, let i = tasks.firstIndex(where: { $0.next <= Date() && $0.paused != true }), canSwitchCanvas, !thinking, !preparing,
              pendingAction == nil, pendingAllocation == nil, prepared == nil, awaitingAmount == nil, draft.isEmpty else { return }
        let key = "task:\(tasks[i].id)"
        // With the user at the window, a clock task waits for them to step away (half an hour at most), so the canvas
        // doesn't change under them («при переключении вижу каждый раз скедуал»). An alert's task and «Run now» go at once.
        let asked = TaskRun.now.contains(key) || tasks[i].because != nil
        guard asked || Self.userAway || Date().timeIntervalSince(tasks[i].next) > 1800 else { return }
        TaskRun.now.remove(key)
        var task = tasks[i]
        if task.advance() {
            if let kept = TaskRun.kept.removeValue(forKey: key) { task.next = kept }
            tasks[i] = task
        } else { tasks.remove(at: i) }
        let back = canvasID, frame = showSchedule, place = detail
        // The runs before left their canvases: what they did stays in the Schedule's history. One the user wrote on stays.
        let before = Set(activities.filter { $0.schedule == key }.compactMap(\.canvas))
        for c in canvases where c.id != canvasID && before.contains(c.id) && c.messages.filter(\.fromUser).count <= 1 { deleteCanvas(c.id) }
        keepViewport = true
        newCanvas()
        keepViewport = false
        let request = (task.because.map { $0 + ": " } ?? say("Scheduled: ", "По расписанию: ")) + task.steps.joined(separator: say(", then ", ", потом "))
        messages.append(Message(fromUser: true, text: request))
        beginActivity(.request, request: request, schedule: key)
        let run = currentActivity
        // With an automation wallet the task's moves go through at once from it, without Touch ID — never one typed
        // through the debug API. Without one, each move waits on its canvas for Confirm.
        let automation = usesAutomation(task) ? automationWallet : nil
        if let automation { reply(say("From the automation wallet «\(automation.label)», no \(Presence.short).", "С кошелька автоматизации «\(automation.label)», без \(Presence.ruGen).")) }
        checkQuiet = false
        // One made through the debug API runs as if it came through it again: nothing in it signs or sets up a recurring buy.
        await SignerClient.$untrusted.withValue(task.untrusted == true) {
            await TaskRun.$automation.withValue(automation) { await runSteps(task.steps, confirmed: automation != nil) }
        }
        let failed = finishedAction?.outcome.ok == false
        // Short of SOL, it fails the same way every run: said with what to do, and the way to do it as a card.
        if let automation, failed, let address = automation.solana, let sol = try? await Solana.balance(owner: address).sol, sol < TaskRun.solFloor {
            // What its tasks need for 4 weeks, as «Top up gas» works it out.
            let need = Fmt.num(solShort(sol), maxFraction: 9)
            reply(say("The automation wallet «\(automation.label)» has \(Fmt.num(sol, maxFraction: 6)) SOL — too little for Solana’s fees and the accounts a move opens. Send it \(need) SOL: enough for its tasks for the next 4 weeks.",
                      "На кошельке автоматизации «\(automation.label)» \(Fmt.num(sol, maxFraction: 6)) SOL — мало для комиссий Solana и счетов, которые открывает движение. Отправь на него \(need) SOL — хватит на его задачи на 4 недели вперёд."))
            offerCards([
                ChatRouter.Option(title: say("Send \(need) SOL to \(automation.label)", "Отправить \(need) SOL на \(automation.label)"),
                                  comment: say("From your main wallet: Confirm and \(Presence.short).", "С основного кошелька: Confirm и \(Presence.ru)."),
                                  command: "send \(need) SOL to \(automation.label)", recommended: true),
            ])
        }
        // A trade plan waits for Execute the way a move waits for Confirm; one a rule stops needs the user just as much.
        let plan = intent != TradeIntent() && !managesPosition
        let waits = pendingAction != nil || pendingAllocation != nil || plan
        var told = messages.last.map(\.text) ?? ""
        if let automation, !waits {
            if await sendBoughtToMain(from: automation), let main = activeWallet { told += say(" It’s on «\(main.label)» now.", " Уже на «\(main.label)».") }
            await loadAutomationBudget()
        }
        // A check with nothing to tell, or a spread with nothing free, says so on its canvas and stays quiet; the run is
        // already in the history, with a grey mark.
        let quiet = checkQuiet && !waits && !failed
        // The Schedule canvas opens the canvas it ran on (saved by now).
        if let i = activities.lastIndex(where: { $0.id == run }) {
            activities[i].canvas = canvasID
            if quiet { activities[i].idle = true }
        }
        if !quiet { notify(waits ? .confirm : .moves, request,
               plan ? (planIsReady ? say("The plan is ready on its canvas — press Execute.", "План готов на своём канвасе — нажми Execute.")
                                   : say("The plan is on its canvas, but a rule stops it — open it to fix.", "План на своём канвасе, но его стопорит правило — открой и поправь."))
                    : waits ? say("Ready on its canvas — press Confirm.", "Готово на своём канвасе — нажми Confirm.")
                            : told, record: false) }
        // Nothing here for the user: back where they were, as it was. «Run now» and an alert's task stay to be seen.
        guard !asked, !waits, !failed, draft.isEmpty, messages.filter(\.fromUser).count <= 1,
              let back, let c = canvases.first(where: { $0.id == back }) else { return }
        keepViewport = true
        openCanvas(c)
        showSchedule = frame
        detail = place
        keepViewport = false
    }

    /// What tasks on the automation wallet bought and haven't sent on to the wallet that's on yet: «solana:<mint>» (SOL
    /// as «solana:SOL») or «<network>:<contract>» → amount. Saved, so a send that failed goes with the next run.
    var forMain: [String: Decimal] {
        get { ((UserDefaults.standard.dictionary(forKey: Workspace.scoped("forMain", activeWalletID)) as? [String: String]) ?? [:]).compactMapValues { Decimal(string: $0) } }
        set { UserDefaults.standard.set(newValue.mapValues { "\($0)" }, forKey: Workspace.scoped("forMain", activeWalletID)) }
    }

    /// A swap on the automation wallet during a task (Executor's `swapped` / `traded`): what it got is owed to the
    /// wallet that's on; spending a coin owed before (staked, sold) owes that much less. Dollars stay: they're the budget.
    func owe(_ key: String, _ amount: Decimal) {
        let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
        guard parts.count == 2, !(Stablecoins.byNetwork[parts[0]] ?? []).contains(where: { $0.address.lowercased() == parts[1].lowercased() }) else { return }
        let left = (forMain[key] ?? 0) + amount
        forMain[key] = left > 0 ? left : nil
    }

    /// What tasks bought on the automation wallet goes on to the wallet that's on, as it is, signed by Automation without
    /// Touch ID: Automation keeps only the budget, the coins are the user's, and selling them takes Touch ID
    /// (usesAutomation). The signer counts the user's own wallets as known (OwnStore), so nothing waits on a prompt
    /// with no one at the Mac. True when something went.
    func sendBoughtToMain(from automation: SignerWallet) async -> Bool {
        await Task.yield() // the swaps' `owe` hops to the main actor after the move returns
        let owed = forMain
        guard !owed.isEmpty, let main = activeWallet, let wallet = signerWallet else { return false }
        var held: Solana.Balance?
        if let a = automation.solana { held = try? await Solana.balance(owner: a) }
        executing = true
        defer { executing = false }
        var moved = false
        for (key, amount) in owed.sorted(by: { $0.key < $1.key }) {
            let parts = key.split(separator: ":", maxSplits: 1).map(String.init)
            guard parts.count == 2 else { forMain[key] = nil; continue }
            let (network, token) = (parts[0], parts[1])
            var send = amount
            // What's there now: a later step may have spent some; SOL keeps the floor its fees need.
            if network == "solana", let held {
                send = min(amount, token == "SOL" ? held.sol - TaskRun.solFloor : held.tokens.first { $0.contract == token }?.amount ?? 0)
            }
            guard send > 0 else { forMain[key] = nil; continue }
            guard let to = network == "solana" ? main.solana : main.evm else { continue }
            do {
                let line = try await TaskRun.$automation.withValue(automation) {
                    try await executor(wallet).sendToken(token, amount: send, on: network, to: to, label: main.label)
                }
                forMain[key] = nil
                reply(line)
                moved = true
            } catch {
                reply(say("It stays on «\(automation.label)» for now: \(error.localizedDescription) The next run sends it on to «\(main.label)».",
                          "Пока остаётся на «\(automation.label)»: \(error.localizedDescription) Следующий запуск отправит это на «\(main.label)»."))
            }
        }
        return moved
    }

    /// The day before the automation wallet runs short for its buys, once a day: a notification, and with no one at the
    /// window the Schedule opens on its top-up. Main signs only with Touch ID, so a top-up can't go by itself; asked
    /// ahead, a week of buys then runs while the Mac is left alone.
    func remindTopUp() {
        guard let w = automationWallet, let total = automationBudget?.total, let short = automationShortForWeek else { return }
        let need = automationSpend(until: Date().addingTimeInterval(86_400))
        let today = Date().formatted(.iso8601.year().month().day())
        guard need > total, UserDefaults.standard.string(forKey: "topUpAsked") != today else { return }
        UserDefaults.standard.set(today, forKey: "topUpAsked")
        let main = activeWallet?.label ?? "Main"
        notify(.confirm, say("«\(w.label)» is short for the next buys", "На «\(w.label)» не хватит на ближайшие покупки"),
               say("The buys in the next 24 hours need \(Fmt.num(need)) USDC; it has \(Fmt.num(total)). Top it up with \(Fmt.num(short)) USDC for the week from «\(main)»: Confirm and \(Presence.short) once, then they run by themselves.",
                   "Покупкам в ближайшие сутки нужно \(Fmt.num(need)) USDC, на нём \(Fmt.num(total)). Пополни его на \(Fmt.num(short)) USDC на неделю с «\(main)»: один раз Confirm и \(Presence.ru), дальше покупки пойдут сами."),
               record: false)
        if Self.userAway { showSchedule = true }
    }

    /// Nobody at the window: another app in front, or no key or mouse for five minutes.
    static var userAway: Bool {
        !NSApp.isActive || CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: CGEventType(rawValue: ~0)!) >= 300
    }

    /// Steps one after another. The first that waits for Confirm holds the rest until its move has gone through
    /// (`continueTask`): they may need the money it moves. That one Confirm covers the chain: once `confirmed`, a step's
    /// move is carried out at once, and one that doesn't go through stops the rest.
    /// `brought`: the networks a move before these steps delivered USDC to (the first step of the chain, gone through).
    func runSteps(_ steps: [String], confirmed: Bool = false, brought: Set<String> = []) async {
        thinking = true
        var rest: [String] = []
        var arrived = brought
        // A confirmed chain signs its bridge legs one after another and lets them land together: each step waits only for
        // the landings where it spends (Landings), so three bridges take as long as the slowest, not the three in a row.
        let landings = confirmed ? Landings() : nil
        var stopped = false
        for (k, step) in steps.enumerated() {
            let command = WalletCommand.parse(step)
            defer { if let c = command, let to = Self.arrives(c) { arrived.insert(to) } }
            // The move before this step is done: let it go first, so the canvas keeps it as an earlier move. Cleared only
            // after, the next step's card was saved over it with its «Done» (a Base → Robinhood bridge vanished under
            // «Buy AI», and «Buy MOO» showed Done though it never ran).
            if confirmed {
                finishedAction = nil
                chainAhead = (canvasID, Array(steps[(k + 1)...]))
                showNext(step)
                if let landings, !(await landed(landings, before: command, step: step)) { stopped = true; break }
                // Money a move before brought to Solana: this step waits until Solana's balance shows it.
                if arrived.contains("solana"), let c = command, Self.spends(c) == "solana", let need = Self.solanaUSDC(c) {
                    await waitForDeposit(need, then: step)
                }
            }
            // The first step reads the balances as they are now: one that can't be paid is said before any card
            // («Move 0 USDC · Ethereum → Arbitrum» was drawn). Later steps wait for the money the ones before bring.
            if k == 0, !confirmed, TaskRun.automation == nil, let c = WalletCommand.parse(step) { await freshenSource(c) }
            if k == 0, !confirmed, TaskRun.automation == nil, let c = WalletCommand.parse(step), let why = refusalBeforeCard(c) {
                reply(why)
                noteFailure(steps.joined(separator: " then "), ExecutionError(why))
                break
            }
            // After a move, a step spends what the move brought, Relay's cents taken off.
            let step = confirmed ? await fittedToArrival(step) : step
            guard await runCanonical(step, userText: step, before: intent, inChain: true) else {
                reply(say("ihatecrypto can’t do “\(step)” — the steps stop here.", "ihatecrypto не умеет «\(step)» — на этом шаги останавливаются."))
                break
            }
            // A contract call is never carried out on a chain's Confirm: its own card, simulated, waits for its own.
            if confirmed, pendingAction != nil || pendingAllocation != nil, ContractCall.parse(step) == nil, AgentCheck.parse(step) == nil {
                finishedAction = nil
                if let p = pendingAction {
                    pendingAction = nil
                    pendingText = nil
                    await Executor.$landings.withValue(landings) { await carryOut(p) }
                } else if let p = pendingAllocation {
                    pendingAllocation = nil
                    await Executor.$landings.withValue(landings) { await move(p, confirmed: true) }
                }
                if finishedAction?.outcome.ok == false {
                    if k + 1 < steps.count { reply(say("“\(step)” didn’t go through, so the steps after it don’t run.", "«\(step)» не прошёл, поэтому следующие шаги не запускаю.")) }
                    break
                }
                continue
            }
            if pendingAction != nil || pendingAllocation != nil || awaitingAmount != nil {
                rest = Array(steps[(k + 1)...])
                // The Confirm covers the chain, so it names what follows.
                if let p = pendingText, !rest.isEmpty { pendingText = ([p] + rest).joined(separator: " → ") }
                // The step's card said nothing of its own (runCanonical inChain): one line for the chain.
                if pendingAction != nil {
                    reply(say("It’s on the canvas, every step in a row — one Confirm covers them all",
                              "Все шаги на холсте — один Confirm на всю цепочку",
                              "Todos los pasos están en el lienzo: un Confirm para toda la cadena")
                          + orCancel(say(", or say anything else to cancel.", ", или напиши что-нибудь другое, чтобы отменить.", ", o di otra cosa para cancelar.")))
                }
                break
            }
            // A move that didn't get to its Confirm (short of money, a rule, switched off) stops the rest: they may need its money.
            switch WalletCommand.parse(step) {
            case nil, .yields?, .schedule?, .stopSchedules?, .receive?: continue
            default:
                if k + 1 < steps.count {
                    reply(say("“\(step)” didn’t get to its Confirm, so the steps after it don’t run.", "«\(step)» не дошёл до подтверждения, поэтому следующие шаги не запускаю."))
                }
            }
            break
        }
        // What's still landing when the chain ends (it ended on a bridge, or stopped) is waited for and said.
        if let landings, landings.count(at: nil) > 0, !stopped { _ = await landed(landings, before: nil, step: nil) }
        thinking = false
        if confirmed {
            // A step that stopped before its move leaves its «Next» board; the outcome board shows from here.
            chainAhead = nil
            runningAction = nil
        }
        snapshotCanvas()
        saveCanvas()
        taskQueue = rest.isEmpty ? nil : (canvasID, rest, Date(), arrived)
        // A confirmed chain is through (or stopped): the model goes on with the request it was a step of.
        if confirmed { await afterMove() }
    }

    /// A step that spends money on a network, run after a move landed there: when the move's fee left it a little short
    /// of the step's amount (cents from an L2, under a dollar from Ethereum), the step spends what is there. «Move 5 then
    /// spread 5», «move 100 then lend 100» spend what arrived, and nothing has to be held back for the fee (a dollar was:
    /// «переведи 100 и положи в aave» lent 99 and left a dollar on Arbitrum). A bigger gap is left for the step to refuse.
    func fittedToArrival(_ step: String) async -> String {
        // «buy USDT on solana for 750 USDC then send 750 USDT on solana to Bybit» (or «then lend 750 USDT»): the swap gave
        // 749.8 USDT, and the step takes what it gave rather than failing on the cents (or the model writing «all», which
        // takes what was there before).
        let token: (symbol: String, asked: Decimal)? = switch WalletCommand.parse(step) {
        case .sendToken(_, let t, let x?, "solana"?): (t, x)
        case .lendDeposit(let t, let x?) where !["USDC", "SOL"].contains(t.uppercased()): (t, x)
        default: nil
        }
        // The mint from the dollar coins' table or the tokens already read, then one light read of that account.
        let mint = token.flatMap { t in
            Stablecoins.byNetwork["solana"]?.first { $0.symbol.uppercased() == t.symbol.uppercased() }?.address
                ?? solana?.tokens.first { $0.symbol.uppercased() == t.symbol.uppercased() || $0.contract == t.symbol }?.contract
        }
        if let token, token.asked > 0, let mint, let have = try? await SolanaRPC().ownedAmount(owner: solanaAddress, mint: mint),
           have < token.asked, token.asked - have <= token.asked * Decimal(string: "0.02")!, let c = WalletCommand.parse(step) {
            let fitted = c.with(amount: have)
            return WalletCommand.parse(fitted.summary) == fitted ? fitted.summary : step
        }
        guard let c = WalletCommand.parse(step), let net = Self.spends(c), let asked = c.editableAmount, asked > 0 else { return step }
        let there: Decimal?
        if net == "solana" {
            await loadSolana()
            there = solana?.usdc
        } else if let n = EVMNetwork.named(net), let units = try? await n.usdcBalance(hlAddress) {
            // Tempo's fees come out of the USDC itself: a cent stays.
            there = max(0, Decimal(units) / 1_000_000 - (n.key == "tempo" ? Decimal(string: "0.01")! : 0))
        } else {
            there = nil
        }
        guard let there, there < asked, asked - there <= max(1, asked * Decimal(string: "0.02")!) else { return step }
        let fitted = c.with(amount: there.rounded(2, .down))
        // Run again from its words: only a command that reads back the same.
        return WalletCommand.parse(fitted.summary) == fitted ? fitted.summary : step
    }

    /// When a move finishes: the rest of a scheduled task, if the move was on its canvas, went through, and came
    /// within half an hour (a card waits 15 minutes; a later move on that canvas is something else).
    func continueTask() {
        guard let q = taskQueue else { return }
        taskQueue = nil
        guard q.canvas == canvasID, finishedAction?.outcome.ok == true, Date().timeIntervalSince(q.since) < 1800, let first = q.steps.first else { return }
        // The next step's board in the same breath as the move's goes: no frame where the chain's cards are gone.
        chainAhead = (q.canvas, Array(q.steps.dropFirst()))
        showNext(first)
        Task { await runSteps(q.steps, confirmed: true, brought: q.brought) }
    }

    /// The step a confirmed chain goes to, lit, before its move's own board takes its place.
    private func showNext(_ step: String) {
        runningAction = (say("Next: ", "Дальше: ", "Siguiente: ") + readable(ScheduleCanvas.readable(step)), chainSteps(of: [step]), [], Date())
    }

    /// A step that spends on Solana what a move before it brought: a card of its own («ARRIVES · SOLANA») until
    /// Solana's balance shows the money, two minutes at most; then the step goes on and its own check says what's short.
    private func waitForDeposit(_ need: Decimal, then step: String) async {
        guard let have = solana?.usdc, have < need else { return }
        func card(_ have: Decimal) -> ActivityBubble {
            ActivityBubble(kind: "step", label: "ARRIVES · SOLANA", title: "\(Fmt.num(need)) USDC",
                           lines: [say("on Solana now", "на Solana сейчас", "en Solana ahora") + "\t\(Fmt.num(have)) USDC",
                                   say("then", "потом", "luego") + "\t" + readable(ScheduleCanvas.readable(step))],
                           logo: "sol", fill: .solana)
        }
        runningAction = (say("Waiting for the deposit on Solana", "Жду депозит на Solana", "Esperando el depósito en Solana"),
                         [card(have)] + chainSteps(of: [step]), [], Date())
        for _ in 0..<60 {
            try? await Task.sleep(for: .seconds(2))
            await loadSolana()
            let now = solana?.usdc ?? 0
            if now >= need { break }
            runningAction?.steps[0] = card(now)
        }
    }

    /// The USDC a step spends on Solana: what it waits for when a move before it brings the money.
    /// Waits for the legs still landing where `command` spends (all of them for a step that names no place, or at the
    /// end), the running card saying so; false when one of them failed — said in the chat, and the chain stops there.
    private func landed(_ landings: Landings, before command: WalletCommand?, step: String?) async -> Bool {
        let place = command.flatMap(Self.source)
        guard landings.count(at: place) > 0 else { return true }
        let waiting = say("Waiting for the moves before to land", "Жду, пока придут деньги с прошлых шагов", "Esperando a que lleguen los pasos anteriores")
        runningAction = (waiting, step.map { chainSteps(of: [$0]) } ?? [], [], Date())
        do {
            // Each landing says how it went, as a move done in one go would have.
            for line in try await landings.wait(for: place) { reply(line) }
            return true
        } catch {
            reply(say("A move before didn't land, so the steps after it don't run: ", "Деньги с прошлого шага не пришли, дальше не иду: ") + error.localizedDescription)
            return false
        }
    }

    /// Where a step takes its money from: what it spends on, or the place a bridge leg leaves; nil when it names none.
    static func source(_ c: WalletCommand) -> String? {
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

    static func solanaUSDC(_ c: WalletCommand) -> Decimal? {
        switch c {
        case .lendDeposit("USDC", let a), .kamino(deposit: true, symbol: "USDC", amount: let a), .hlp(deposit: true, amount: let a), .buy(_, usdc: let a),
             .allocate("USDC", maxShare: _, amount: let a, without: _, from: "solana"), .solanaTo(_, let a), .jupiterDCA(_, let a, _, _): a
        default: nil
        }
    }
}

extension Workspace {
    /// «create an automation wallet»: a wallet the signer makes (Touch ID first) that signs scheduled tasks' moves
    /// without Touch ID. The active wallet stays; «Automation» goes into Addresses (Solana and Base) so «send 20 USDC
    /// to Automation» tops it up — the signer asks Touch ID for that new address once, with the address on the prompt.
    func createAutomationWallet(note: String? = nil) async {
        do {
            let r = try await SignerClient().send(.createAutomationWallet)
            guard r.ok, let w = r.wallets?.first(where: \.isAutomation) else { throw WalletError(r.error ?? "The signer refused.") }
            await loadSigner()
            // A task made before this wallet would now spend from it by itself: each waits for its switch, as a new one does.
            let held = tasks.indices.filter { tasks[$0].paused != true && waitsForSwitch(tasks[$0]) }
            for i in held { tasks[i].paused = true }
            for (address, network) in [(w.solana, AddressCheck.Network.solana), (w.evm, .base)] {
                guard let address, !addressBook.contains(where: { $0.address == address && $0.network == network }) else { continue }
                addressBook.append(SavedAddress(name: w.label, address: address, network: network, category: "Your wallet",
                                                icon: .symbol("clock.arrow.circlepath"), source: "signer"))
            }
            reply(say("The automation wallet is made: scheduled tasks now spend only from it, without \(Presence.name). Put their budget on it — "
                      + "keep only that there; if this Mac's keychain is lost, so is what's on it.",
                      "Кошелёк автоматизации создан: задачи по расписанию тратят только с него, без \(Presence.ruGen). Положи туда их бюджет — "
                      + "и только его: если связка ключей этого Мака пропадёт, пропадёт и то, что на нём.")
                  + (held.isEmpty ? "" : say(" Tasks made before it would now spend from it by themselves, so they’re paused (\(held.count)): each starts only when you switch it on in the Schedule.",
                                             " Задачи, созданные раньше, теперь тратили бы с него сами, поэтому они на паузе (\(held.count)): каждая начнёт работать, только когда ты включишь её в Schedule.")),
                  note: note)
            offerCards([
                ChatRouter.Option(title: say("Top up 20 USDC", "Пополнить на 20 USDC"),
                                  comment: say("From your wallet on Solana: \(Presence.short) once for the new address.", "С твоего кошелька на Solana: \(Presence.ru) один раз на новый адрес."),
                                  command: "send 20 USDC to \(w.label)", recommended: true),
            ])
        } catch {
            reply(say("No automation wallet: ", "Кошелёк автоматизации не создан: ") + error.localizedDescription, note: note)
        }
    }
}

/// Settings › General › Keep running: a launch agent that starts ihatecrypto at login and again within a minute after
/// it quits or crashes, so scheduled tasks, stops and alerts keep going. It starts the app only when no copy runs, and
/// hidden (`open -g -j`): the window stays closed, the coin is in the menu bar.
enum KeepRunning {
    static let label = Flavor.id + ".keeprunning"
    static var plist: URL { FileManager.default.homeDirectoryForCurrentUser.appending(path: "Library/LaunchAgents/\(label).plist") }
    static var isOn: Bool { FileManager.default.fileExists(atPath: plist.path) }

    static func set(_ on: Bool) throws {
        let domain = "gui/\(getuid())"
        launchctl("bootout", "\(domain)/\(label)")
        guard on else { try? FileManager.default.removeItem(at: plist); return }
        let job: [String: Any] = [
            "Label": label,
            // The app as it is now; a moved or rebuilt copy elsewhere re-registers when the switch is set again.
            "ProgramArguments": ["/bin/sh", "-c", "/usr/bin/pgrep -qx '\(ProcessInfo.processInfo.processName)' || /usr/bin/open -g -j '\(Bundle.main.bundlePath)'"],
            "RunAtLoad": true,
            "StartInterval": 60,
            "ProcessType": "Background",
            "AssociatedBundleIdentifiers": [Flavor.id],
        ]
        try FileManager.default.createDirectory(at: plist.deletingLastPathComponent(), withIntermediateDirectories: true)
        try PropertyListSerialization.data(fromPropertyList: job, format: .xml, options: 0).write(to: plist)
        guard launchctl("bootstrap", domain, plist.path) == 0 else {
            try? FileManager.default.removeItem(at: plist)
            throw WalletError("macOS didn’t accept the background item. Check System Settings › General › Login Items.")
        }
    }

    /// Sparkle is about to swap the app: the agent rests, or it could open the old copy mid-swap (Updater.swift).
    static func pause() { launchctl("bootout", "gui/\(getuid())/\(label)") }
    /// At launch of a copy that updates itself: the agent runs again if the switch is on (already running is fine).
    static func resume() { if isOn { launchctl("bootstrap", "gui/\(getuid())", plist.path) } }

    @discardableResult private static func launchctl(_ args: String...) -> Int32 {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/launchctl")
        p.arguments = args
        p.standardOutput = FileHandle.nullDevice
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return -1 }
        p.waitUntilExit()
        return p.terminationStatus
    }
}

/// The rail's Schedule button: a canvas of its own in place of the plan. On the left what runs on its own, next first
/// — scheduled tasks, recurring buys, the stops and takes of open perps — and every time one of them ran; on the
/// right the one picked, in detail, with its runs. While nothing is scheduled, examples go into the chat with a click.
/// What the Schedule says about its items, shared by the frame, an item's canvas and the alerts.
enum ScheduleCanvas {
    /// «Move deposited USDC to the best rates (max 40% per protocol)» for «rebalance USDC max 40% per protocol».
    static func readable(_ step: String) -> String {
        let s = WalletCommand.parse(step)?.summary ?? step
        return s.prefix(1).uppercased() + s.dropFirst()
    }

    static func period(_ days: Int) -> String { days == 1 ? "day" : days == 7 ? "week" : days == 30 ? "month" : "\(days) days" }
}

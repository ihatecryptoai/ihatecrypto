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

    /// A task's steps on the automation wallet: each move carried out at once, drawn on the canvas as it runs; the first
    /// that fails stops the rest.
    func runOnAutomation(_ task: ScheduledTask, _ automation: SignerWallet) async {
        let started = Date()
        reply(say("From the automation wallet «\(automation.label)», no Windows Hello.", "С кошелька автоматизации «\(automation.label)», без Windows Hello."), note: "schedule")
        await TaskRun.$automation.withValue(automation) {
            for step in task.steps {
                guard var command = WalletCommand.parse(step), task.movesMoney else {
                    if !(await runCanonical(step, userText: step)) { reply(say("“\(step)” didn't run.", "«\(step)» не выполнилось."), note: "schedule") }
                    continue
                }
                // A Solana token is Jupiter-verified first, as a card's is.
                switch command {
                case .buy(let a, let u) where !a.resolved:
                    guard let token = await spotToken(a, note: "schedule") else { return }
                    command = .buy(token, usdc: u)
                default: break
                }
                if let off = Modules.current.refusal(command, ru: russian) { reply(off, note: "schedule"); return }
                let id = pin(Board(kind: "move", title: command.summary, steps: steps(for: .wallet(command), title: command.summary, lines: []), state: "running"))
                runningBoard = id
                running = (command.summary, [])
                changed()
                let said = messages.count
                // No one may be at the window: Windows Hello waiting on this move is told as a notification.
                let hello = watchForHello(command.summary)
                do {
                    try await move(command)
                    updateBoard(id) { $0.state = "done" }
                } catch {
                    updateBoard(id) { $0.state = "failed" }
                    reply(say("Failed: ", "Не вышло: ") + error.localizedDescription, note: "schedule")
                }
                hello.cancel()
                let outcome = messages.dropFirst(said).last { !$0.fromUser }?.text
                updateBoard(id) { b in if let outcome { b.lines.append(String(outcome.prefix(400))) } }
                runningBoard = nil
                running = nil
                lastOwnMove = Date()
                if boards.first(where: { $0.id == id })?.state == "failed" { break }
                await refresh()
            }
        }
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
    }
}

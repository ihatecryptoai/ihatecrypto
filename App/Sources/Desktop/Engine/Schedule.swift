import Foundation
import IhatecryptoCore

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

    /// Tasks that are due run now, one at a time: not over a card already waiting for Confirm, which they'd replace.
    func runDueTasks() async {
        guard pending == nil, running == nil, !thinking else { return }
        guard var task = tasks.first(where: { $0.paused != true && $0.next <= Date() }) else { return }
        tasks.removeAll { $0.id == task.id }
        if task.advance() { tasks.append(task) }
        let text = "⏰ " + task.steps.joined(separator: " then ")
        out.event("notify", ["title": say("Scheduled", "По расписанию"), "body": text])
        reply(text, note: "schedule")
        if usesAutomation(task), let automation = automationWallet, task.movesMoney {
            await runOnAutomation(task, automation)
        } else if task.steps.count > 1 {
            _ = await chain(task.steps, note: "schedule")
        } else if let step = task.steps.first, !(await runCanonical(step, userText: step)) {
            reply(say("“\(step)” didn't run.", "«\(step)» не выполнилось."), note: "schedule")
        }
    }
}

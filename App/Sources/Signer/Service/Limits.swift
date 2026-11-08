import Foundation
import SignerAPI

/// The person's limits (Settings), kept in the signer's own settings: the app can lower them, and raises them only
/// through a Touch ID that names the new numbers (`Signer.handle(.setLimits)`).
enum Limits {
    static var current: SignerLimits {
        Signer.defaults.data(forKey: "limits").flatMap { try? JSONDecoder().decode(SignerLimits.self, from: $0) }
            .flatMap { $0.problem == nil ? $0 : nil } ?? .standard
    }

    static func save(_ limits: SignerLimits) { Signer.defaults.set(try? JSONEncoder().encode(limits), forKey: "limits") }

    /// Dollars signed without the person, newest last, each with its time: what the day and the week add up. Kept for
    /// 7 days. ponytail: one list per budget for the whole signer (every wallet's trading key shares «agent»), stricter
    /// than one per wallet; split it per wallet if someone runs several trading keys at full size.
    struct Budget {
        let key: String

        static let agent = Budget(key: "spent-agent"), automation = Budget(key: "spent-automation")

        private func entries(now: Date = Date()) -> [[Double]] {
            let all = (Signer.defaults.object(forKey: key) as? [[Double]]) ?? []
            return all.filter { $0.count == 2 && now.timeIntervalSince1970 - $0[0] < 7 * 86_400 }
        }

        /// Spent in the last `seconds`.
        func spent(within seconds: TimeInterval, now: Date = Date()) -> Decimal {
            entries(now: now).filter { now.timeIntervalSince1970 - $0[0] < seconds }.reduce(Decimal()) { $0 + Decimal($1[1]) }
        }

        /// Call under `lock`: the list is read and written back.
        fileprivate func record(_ dollars: Decimal, now: Date = Date()) {
            guard dollars > 0 else { return }
            Signer.defaults.set(entries(now: now) + [[now.timeIntervalSince1970, NSDecimalNumber(decimal: dollars).doubleValue]], forKey: key)
        }

        fileprivate func remove(_ dollars: Decimal) {
            var list = entries()
            let amount = NSDecimalNumber(decimal: dollars).doubleValue
            if let i = list.lastIndex(where: { $0[1] == amount }) { list.remove(at: i) }
            Signer.defaults.set(list, forKey: key)
        }
    }

    /// Requests come on several connections at once: checking a budget and taking from it is one step under this lock,
    /// so fifty orders sent together can't each pass before any is counted (the 2026-11-08 review).
    private static let lock = NSLock()

    /// Takes `dollars` from the trading key's day if they fit: nil, else why not.
    static func takeAgent(_ dollars: Decimal) -> String? {
        lock.withLock {
            if let why = agentOver(dollars) { return why }
            Budget.agent.record(dollars)
            return nil
        }
    }

    /// Takes `dollars` from the automation wallet's day and week if they fit: nil, else why not (nothing taken).
    static func takeAutomation(_ dollars: Decimal) -> String? {
        lock.withLock {
            if let why = automationOver(dollars) { return why }
            Budget.automation.record(dollars)
            return nil
        }
    }

    /// Counts `dollars` the person allowed in person, past the limits.
    static func take(_ budget: Budget, _ dollars: Decimal) { lock.withLock { budget.record(dollars) } }

    /// Gives back what a move took that wasn't signed after all.
    static func refund(_ budget: Budget, _ dollars: Decimal) {
        guard dollars > 0 else { return }
        lock.withLock { budget.remove(dollars) }
    }

    /// Why the trading key can't open `dollars` more on its own, or nil: over one order, or over the day with what it
    /// already opened.
    static func agentOver(_ dollars: Decimal, limits: SignerLimits = current) -> String? {
        if dollars > Decimal(limits.agentOrder) { return "over the trading key's $\(limits.agentOrder) an order (this one ≈ $\(usd(dollars)))" }
        let today = Budget.agent.spent(within: 86_400)
        if today + dollars > Decimal(limits.agentDay) {
            return "over the trading key's $\(limits.agentDay) a day ($\(usd(today)) opened today, this one ≈ $\(usd(dollars)))"
        }
        return nil
    }

    /// Why the automation wallet can't spend `dollars` more without the person, or nil.
    static func automationOver(_ dollars: Decimal, limits: SignerLimits = current) -> String? {
        let day = Budget.automation.spent(within: 86_400), week = Budget.automation.spent(within: 7 * 86_400)
        if day + dollars > Decimal(limits.automationDay) {
            return "over the automation wallet's $\(limits.automationDay) a day ($\(usd(day)) spent today, this move ≈ $\(usd(dollars)))"
        }
        if week + dollars > Decimal(limits.automationWeek) {
            return "over the automation wallet's $\(limits.automationWeek) a week ($\(usd(week)) spent this week, this move ≈ $\(usd(dollars)))"
        }
        return nil
    }

    static func usd(_ d: Decimal) -> String {
        var v = d, r = Decimal()
        NSDecimalRound(&r, &v, 2, .up)
        return NSDecimalNumber(decimal: r).stringValue
    }
}

import DeFiKamino
import EVMChain
import Foundation
import IhatecryptoCore
import SignerAPI
import SwiftUI
import TradeExecution

extension Workspace {
    /// «guard my loan on kamino», «stop guarding my loans».
    func loanGuardCommand(_ command: LoanGuardCommand, note: String? = nil) async {
        switch command {
        case .clear:
            let mine = loanGuards.filter { $0.owner == nil || $0.owner == solanaAddress }
            // A repay one of them already prepared goes too: they asked for no more of it.
            tasks.removeAll { t in mine.contains { $0.fired == t.id } }
            loanGuards.removeAll { g in mine.contains { $0.id == g.id } }
            reply(mine.isEmpty ? say("No loan was guarded.", "Ни один займ не был под защитой.")
                               : say("Stopped guarding loans (\(mine.count)).", "Защита займов снята (\(mine.count))."), note: note)
        case .set(let named, let network, let below):
            let below = below ?? LoanGuard.defaultBelow
            // The loan named, or the one the wallet has: Kamino's, then Aave's on the first network with a debt.
            var guarded: LoanGuard?
            if named == "kamino" || (named == nil && kaminoLoan?.health != nil) {
                guarded = LoanGuard(loan: "kamino", below: below)
            } else if named == "aave", let network {
                guarded = LoanGuard(loan: "aave", network: network, below: below)
            } else if let wallet = previewWallet {
                for m in AaveV3.markets where (try? await executor(wallet).aaveAccount(m))?.health != nil {
                    guarded = LoanGuard(loan: "aave", network: m.chain, below: below)
                    break
                }
                if guarded == nil, named == "aave" { guarded = LoanGuard(loan: "aave", network: "base", below: below) }
            }
            guard var g = guarded else {
                reply(say("There's no loan to guard yet: borrow on Kamino or Aave first, then say “guard my loan”.",
                          "Пока нечего защищать: сначала займи на Kamino или Aave, потом скажи «следи за займом»."), note: note)
                return
            }
            g.owner = solanaAddress
            if SignerClient.untrusted { g.untrusted = true }
            if let old = loanGuards.first(where: { $0.loan == g.loan && $0.network == g.network && $0.owner == g.owner }) {
                // A test message never replaces the user's own guard (tests' guards go at /reset).
                if g.untrusted == true, old.untrusted != true {
                    reply(say("That loan is already guarded.", "Этот займ уже под защитой."), note: note)
                    return
                }
                // The same guard with a new line: a fired one stays fired, so it can't prepare a second repay.
                (g.id, g.armed, g.fired, g.created) = (old.id, old.armed, old.fired, old.created)
                loanGuards.removeAll { $0.id == old.id }
            }
            loanGuards.append(g)
            withAnimation(StudioView.spring) { showAlerts = true }
            if let read = await loanHealth(g) { guardHealth[g.id] = .some(read.health) }
            // One line: the guard itself is on the alerts board, with its health.
            reply(say("Guarding your \(guardName(g)) loan: under \(Fmt.num(g.below)), a repay back to \(Fmt.num(g.target)) comes to you to confirm.",
                      "Слежу за займом на \(guardName(g)): ниже \(Fmt.num(g.below)) пришлю погашение до \(Fmt.num(g.target)) на подтверждение."), note: note)
        }
    }

    func guardName(_ g: LoanGuard) -> String {
        g.loan == "aave" ? "Aave (\(WalletCommand.placeName(g.network ?? "base")))" : "Kamino"
    }

    /// The loan's health and its USDC debt; `health` nil: no debt there. nil: it couldn't be read this time.
    func loanHealth(_ g: LoanGuard) async -> (health: Decimal?, debt: Decimal)? {
        if g.loan == "kamino" {
            guard kaminoLoanRead else { return nil }
            guard let loan = kaminoLoan else { return (nil, 0) }
            // The token count lags the interest the health already counts (the executor's repay takes the larger too).
            return (loan.health, max(loan.debts.first { $0.symbol == "USDC" }?.tokens ?? 0, loan.debtUSD))
        }
        // ponytail: one getUserAccountData a refresh per Aave guard; cache it if guards pile up.
        guard let wallet = previewWallet, let m = AaveV3.markets.first(where: { $0.chain == g.network }),
              let account = try? await executor(wallet).aaveAccount(m) else { return nil }
        return (account.health, account.debt)
    }

    /// Runs with every refresh: a guarded loan under its line gets its repay prepared as a task due now, which waits on
    /// its own canvas for Confirm with a notification (runDueTask), like a price alert's steps.
    func checkLoanGuards() async {
        for g in loanGuards where g.owner == nil || g.owner == solanaAddress {
            guard let read = await loanHealth(g), let i = loanGuards.firstIndex(where: { $0.id == g.id }) else { continue }
            guardHealth[g.id] = .some(read.health)
            guard let health = read.health, health < g.rearm else {
                if !loanGuards[i].armed { loanGuards[i].armed = true }
                continue
            }
            guard loanGuards[i].armed, health < g.below else { continue }
            loanGuards[i].armed = false
            let h = Fmt.num(health, maxFraction: 2)
            var usdc = LoanGuard.repay(debt: read.debt, health: health, target: g.target)
            // The USDC the place lacks comes first, from wherever it is (FundingPlan: Relay or NEAR Intents, whichever
            // lands more; NEAR sold through 1Click), and the repay takes what arrived (fittedToArrival). With too little
            // anywhere, it repays what the place has and says what's missing.
            let net = g.loan == "aave" ? g.network ?? "base" : "solana"
            let have = self.usdc(on: net) ?? 0, short = usdc - have
            var legs: [String] = []
            if short > 0 {
                if let found = fundingLegs(short, net) { legs = found } else {
                    var cents = have * 100, down = Decimal()
                    NSDecimalRound(&down, &cents, 0, .down)
                    let missing = Fmt.num(usdc - down / 100)
                    usdc = down / 100
                    notify(.alerts, "🛡 \(guardName(g))", say("Loan health \(h): \(missing) USDC more is needed to bring it back to \(Fmt.num(g.target)). Top up USDC.",
                                                            "Здоровье займа \(h): чтобы вернуть его к \(Fmt.num(g.target)), не хватает \(missing) USDC. Пополни USDC."))
                    guard usdc > 0 else { continue }
                }
            }
            var task = ScheduledTask(steps: legs + [g.repayCommand(usdc)], next: Date(), every: nil,
                                     because: say("\(guardName(g)) loan health \(h) is under \(Fmt.num(g.below)); liquidation is at 1",
                                                  "здоровье займа на \(guardName(g)) \(h), ниже \(Fmt.num(g.below)); ликвидация при 1"))
            task.untrusted = g.untrusted
            // As the guard promised: the repay comes to the user to confirm, never off the automation wallet by itself.
            task.fromActive = true
            loanGuards[i].fired = task.id
            tasks.append(task)
        }
    }
}

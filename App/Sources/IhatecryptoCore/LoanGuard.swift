import Foundation

/// «Don't let my Kamino loan get liquidated»: the loan's health factor (liquidation at 1) read with every refresh. Under
/// `below` it prepares a repay that brings the health back to `target`, the USDC brought first from wherever it is, and
/// sends it to the user to confirm like a price alert's steps. One warning per dip: once it fires it waits until the
/// health is back over `rearm`, halfway to `target`, so a repay that lands just short of the target still re-arms it.
public struct LoanGuard: Codable, Equatable, Sendable, Identifiable {
    public var id = UUID()
    /// "kamino" (its Backpack market) or "aave".
    public let loan: String
    /// The Aave network ("base", "arbitrum", "polygon", "ethereum"); nil on Kamino.
    public let network: String?
    public let below: Decimal
    public let target: Decimal
    /// The wallet it watches, by its Solana address: another wallet's loan is never read as this one.
    public var owner: String?
    public var created = Date()
    /// false from the moment it fires until the health is back over `rearm`.
    public var armed = true
    /// The repay task it made last: «stop guarding» takes it away too.
    public var fired: UUID?
    /// Set through the local debug API: its repay never runs on the automation wallet by itself (ScheduledTask).
    public var untrusted: Bool?

    /// Under this the repay is prepared; ihatecrypto's own borrows stop at 1.5 (Executor.minimumHealth).
    public static let defaultBelow = Decimal(string: "1.25")!
    public static let defaultTarget = Decimal(string: "1.5")!

    public init(loan: String, network: String? = nil, below: Decimal = defaultBelow, target: Decimal = defaultTarget) {
        self.loan = loan
        self.network = loan == "aave" ? network ?? "base" : nil
        self.below = below
        // A gap between the line and the target, or every small dip would make another tiny repay.
        self.target = max(target, below + Decimal(string: "0.2")!)
    }

    public var rearm: Decimal { below + (target - below) / 2 }

    /// The repay as the router's command.
    public func repayCommand(_ usdc: Decimal) -> String {
        "repay \(usdc) USDC on " + (loan == "aave" ? "Aave on \(network ?? "base")" : "kamino")
    }

    /// The USDC to repay so the health rises from `health` to `target`. With one debt asset the health is a fixed amount
    /// over the debt (Aave: collateral × threshold over the debt; Kamino: the liquidation limit over the borrow-factor
    /// adjusted debt), so the debt that leaves `target` is debt × health / target. Rounded up to the cent, at most the debt.
    public static func repay(debt: Decimal, health: Decimal, target: Decimal) -> Decimal {
        guard debt > 0, target > 0, health < target else { return 0 }
        var r = debt * (1 - health / target) * 100, up = Decimal()
        NSDecimalRound(&up, &r, 0, .up)
        return min(up / 100, debt)
    }
}

/// «guard my loan on kamino», «guard my loan on aave on arbitrum below 1.3», «stop guarding my loans»; with no model
/// connected also «следи за займом на камино», «не дай ликвидировать займ на aave», «убери защиту займа».
public enum LoanGuardCommand: Equatable, Sendable {
    /// `loan` nil: the one loan the wallet has; `network` nil on Aave: the network its debt is on. `below` nil:
    /// LoanGuard.defaultBelow.
    case set(loan: String?, network: String?, below: Decimal?)
    case clear

    public static func parse(_ text: String) -> LoanGuardCommand? {
        let q = text.lowercased()
        if q.contains(/(?:stop|cancel|remove|delete|clear)\s+(?:all\s+|my\s+|the\s+)*(?:guarding\b|loan\s+guards?)|(?:don'?t|do\s+not|no\s+longer)\s+guard\b|(?:убери|отмени|сними|выключи|останови)\w*\s+(?:все\s+)?(?:защит|слежк|охран)\w*\s+(?:за\s+|для\s+)?займ|(?:перестань|хватит|не\s+надо|не\s+нужно|больше\s+не)\s+(?:следить|защищать|охранять|смотреть)\w*\s+(?:за\s+)?(?:моим\s+|мой\s+)?займ/) {
            return .clear
        }
        // The router's form starts the line; in Russian the request itself, the loan named in it.
        guard q.contains(/^\s*(?:please\s+)?(?:guard|protect)\s+(?:my\s+|the\s+)?(?:\w+\s+)?loans?\b|(?:следи|присматривай|смотри)\w*\s+за\s+(?:моим\s+|мой\s+)?займ|(?:защити|охраняй|спаси)\w*\s+(?:мой\s+)?займ|не\s+да\w*\s+(?:[^.]*\s)?ликвидир[^.]*займ|не\s+да\w*\s+(?:[^.]*\s)?займ[^.]*ликвидир/)
        else { return nil }
        let loan: String? = q.contains(/kamino|камино/) ? "kamino" : q.contains(/aave|ааве|аав/) ? "aave" : nil
        let network: String? = q.contains(/arbitrum|арбитрум/) ? "arbitrum" : q.contains(/polygon|полигон/) ? "polygon"
            : q.contains(/ethereum|mainnet|эфириум|мейннет/) ? "ethereum" : q.contains(/\bbase\b|\bбейс/) ? "base" : nil
        let below = q.firstMatch(of: /(?:below|under|\bat|ниже|меньше)\s*(\d+(?:[.,]\d+)?)/)
            .flatMap { Decimal(string: $0.1.replacingOccurrences(of: ",", with: ".")) }
            // Any line above liquidation the user names (3.5 was dropped for the default 1.25); 10+ is a typo, not a health.
            .flatMap { $0 > 1 && $0 < 10 ? $0 : nil }
        return .set(loan: loan, network: loan == "aave" ? network : nil, below: below)
    }
}

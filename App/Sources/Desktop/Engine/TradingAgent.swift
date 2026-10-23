import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// Settings › Trading agent, as on the Mac (SettingsView.agent): a style that sets every rule at once, the limits a plan
/// may not break, what «by the book» uses when no numbers are named, and Safe search. Code checks every plan against
/// these (Plan.resolve, RiskRules in the Executor), as the Mac's Workspace.policy does.
nonisolated struct TradingView: Encodable, Equatable {
    struct Style: Encodable, Equatable { var id: String; var policy: RiskPolicy }
    var policy: RiskPolicy
    var styles: [Style]
    var safeSearch: Bool
    /// Settings › Wallet: «One transaction for approve and swap» (EIP-7702) and the cap for unknown protocols.
    var batches: Bool
    var contractCap: Double
    /// Settings › Wallet › Hyperliquid (TradingKey.swift).
    var key: TradingKeyView
}

extension Engine {
    /// The rules the trades are planned and checked by.
    var policy: RiskPolicy { settings.policy ?? RiskPolicy() }
    /// Jupiter-verified tokens only, unless turned off.
    var safeSearch: Bool { settings.safeSearch ?? true }

    /// Three ready sets of the rules (AgentStyle); Balanced is the default.
    static let tradingStyles: [TradingView.Style] = {
        func d(_ s: String) -> Decimal { Decimal(string: s)! }
        var careful = RiskPolicy(), bold = RiskPolicy()
        (careful.maxRiskPct, careful.defaultRiskPct, careful.maxOpenRiskPct) = (1, d("0.5"), 3)
        (careful.minRewardRisk, careful.defaultRewardRisk, careful.maxLeverage, careful.atrStopMultiple) = (2, d("2.5"), 2, 2)
        (bold.maxRiskPct, bold.defaultRiskPct, bold.maxOpenRiskPct) = (3, 2, 10)
        (bold.minRewardRisk, bold.defaultRewardRisk, bold.maxLeverage, bold.atrStopMultiple) = (d("1.2"), d("1.5"), 10, 1)
        return [.init(id: "careful", policy: careful), .init(id: "balanced", policy: RiskPolicy()), .init(id: "bold", policy: bold)]
    }()

    func tradingView() -> TradingView {
        TradingView(policy: policy, styles: Self.tradingStyles, safeSearch: safeSearch, batches: settings.batches == true, contractCap: settings.contractCap ?? 50,
                    key: tradingKeyView())
    }

    /// New rules from Settings, kept within the Mac's steppers' bounds and in line with each other.
    func setPolicy(_ p: RiskPolicy?) throws {
        guard var p else { throw WalletError("No rules given.") }
        func clamp(_ v: Decimal, _ lo: Decimal, _ hi: Decimal) -> Decimal { min(max(v, lo), hi) }
        p.maxRiskPct = clamp(p.maxRiskPct, Decimal(string: "0.5")!, 10)
        p.maxOpenRiskPct = clamp(max(p.maxOpenRiskPct, p.maxRiskPct), p.maxRiskPct, 30)
        p.minRewardRisk = clamp(p.minRewardRisk, 1, 5)
        p.maxLeverage = clamp(p.maxLeverage, 1, 20).rounded(0, .plain)
        p.defaultRiskPct = clamp(p.defaultRiskPct, Decimal(string: "0.25")!, p.maxRiskPct)
        p.defaultRewardRisk = clamp(max(p.defaultRewardRisk, p.minRewardRisk), p.minRewardRisk, 10)
        p.atrStopMultiple = clamp(p.atrStopMultiple, Decimal(string: "0.5")!, 5)
        settings.policy = p == RiskPolicy() ? nil : p
    }

    /// The cap for a contract outside ihatecrypto's own list, in dollars: one of the Mac's choices.
    func setContractCap(_ usd: Double?) throws {
        guard let usd, [10, 25, 50, 100, 250, 500, 1000].contains(usd) else { throw WalletError("Pick one of the caps.") }
        settings.contractCap = usd == 50 ? nil : usd
    }

    /// The plan with what the person left to the rules filled in (Plan.resolve: «by the book» risk, the stop by
    /// volatility, the take, the leverage cap), as the Mac's Workspace.planned resolves it before it's checked.
    func resolved(_ intent: TradeIntent, market: Market?) async -> (intent: TradeIntent, market: Market?) {
        guard var m = market else { return (intent, market) }
        if m.atrPct == nil { m.atrPct = try? await Hyperliquid.atrPct(coin: m.name) }
        return (Plan.resolve(intent, market: m, equity: portfolio.total > 0 ? portfolio.total : nil, policy: policy).intent, m)
    }

    /// What the risk rules measure against for a position's stop: the whole portfolio, as the Mac's `equity`.
    var ruleEquity: Decimal? { portfolio.total > 0 ? portfolio.total : nil }
}

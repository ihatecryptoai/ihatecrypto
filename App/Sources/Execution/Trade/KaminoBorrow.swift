import DeFiKamino
import ExecutionCore
import Foundation
import IhatecryptoCore
import SolanaChain

/// Borrowing USDC on Kamino against what the wallet put up there — Backpack's tokenized stocks on the Backpack market,
/// or its deposits on the main market — and paying it back. A borrow keeps the health factor (the debt at which
/// liquidation starts, over the debt) at `Executor.minimumHealth` or more, the same line as on Aave, and never past
/// Kamino's own borrow limit; the card says at which price of the stock the position would be liquidated.
public struct KaminoLoanPreview: Sendable, Equatable {
    public let market: String
    /// The stock the loan stands on, when one does («SPCX»), and how much of it is (or will be) collateral.
    public let stock: String?
    public let stockTokens: Decimal
    /// Dollars, after the deposit the borrow brings in first.
    public let collateralUSD: Decimal
    /// What Kamino lets the wallet borrow more, after that deposit and before this borrow.
    public let availableUSD: Decimal
    /// Owed after the borrow.
    public let debtUSD: Decimal
    public let healthNow: Decimal?
    public let healthAfter: Decimal?
    /// The stock's price at which the health factor reaches 1, the rest of the collateral held steady; nil without one.
    public let liquidationPrice: Decimal?
    /// The USDC loan's rate, % a year.
    public let borrowAPY: Decimal
}

extension Executor {
    /// Tokens of a collateral that may leave a Kamino position with debt: what's left must still carry the debt at the
    /// borrow limit (Kamino's own rule for a withdrawal), with 1% to spare for the price moving meanwhile.
    static func kaminoFreeCollateral(_ loan: Kamino.Loan, maxLTV: Decimal, priceUSD: Decimal) -> Decimal {
        guard maxLTV > 0, priceUSD > 0 else { return 0 }
        return max(0, (loan.borrowLimitUSD - loan.adjustedDebtUSD) / maxLTV / priceUSD * Decimal(string: "0.99")!)
    }

    /// The position after putting up `adding` of a stock at `price` (its reserve's `risk`) and borrowing `usdc` (a debt
    /// counted `borrowFactor` times): its limits, health, and the stock's liquidation price. `held`: that stock's
    /// tokens already in the position.
    static func kaminoAfter(_ loan: Kamino.Loan?, adding: Decimal, held: Decimal, price: Decimal, risk: Kamino.Risk?, usdc: Decimal,
                            borrowFactor: Decimal) -> (collateral: Decimal, available: Decimal, debt: Decimal, health: Decimal?, liquidation: Decimal?) {
        let added = adding * price
        let collateral = (loan?.depositUSD ?? 0) + added
        let limit = (loan?.borrowLimitUSD ?? 0) + added * (risk?.maxLTV ?? 0)
        let liquidationLimit = (loan?.liquidationLimitUSD ?? 0) + added * (risk?.liquidationLTV ?? 0)
        let debt = (loan?.adjustedDebtUSD ?? 0) + usdc * borrowFactor
        let health = debt > 0 ? liquidationLimit / debt : nil
        // The stock's share of the liquidation limit moves with its price; the rest stays.
        let tokens = held + adding, ltv = risk?.liquidationLTV ?? 0
        var liquidation: Decimal?
        if tokens > 0, ltv > 0, debt > 0 {
            let rest = liquidationLimit - tokens * price * ltv
            let p = (debt - rest) / (tokens * ltv)
            liquidation = p > 0 ? p : nil
        }
        return (collateral, max(0, limit - (loan?.adjustedDebtUSD ?? 0)), (loan?.debtUSD ?? 0) + usdc, health, liquidation)
    }

    /// The mint's token program, SPL Token or Token-2022: its owner on chain.
    func tokenProgram(_ mint: String) async throws -> String {
        let info = try await rpc.call("getAccountInfo", [mint, ["encoding": "base64", "commitment": "confirmed"]]) as? [String: Any]
        guard let program = (info?["value"] as? [String: Any])?["owner"] as? String, [SPL.tokenProgram, SPL.token2022Program].contains(program) else {
            throw ExecutionError("\(mint) isn't a token mint on Solana.")
        }
        return program
    }

    /// What borrowing `usdc` would do, read now; `adding` of `stock` goes in as collateral first. Nothing is signed.
    /// Loans are taken on the Backpack market only: a debt on the main market would show nowhere in the portfolio.
    public func kaminoBorrowPreview(usdc: Decimal, against stock: String?, adding: Decimal?) async throws -> KaminoLoanPreview {
        if let stock, Kamino.market(for: stock) != Kamino.backpackMarket {
            throw ExecutionError("ihatecrypto borrows on Kamino against Backpack's stocks (\(Kamino.backpackStocks.keys.sorted().joined(separator: ", "))), not against \(stock).")
        }
        let market = Kamino.backpackMarket
        let loan = try await Kamino.loan(wallet: wallet.solana, market: market)
        let reserves = try await Kamino.reserves(market: market)
        guard let usdcReserve = reserves.first(where: { $0.symbol == "USDC" }) else { throw ExecutionError("Kamino lends no USDC on that market.") }
        // The stock the loan stands on: the one named, else the biggest collateral there.
        let symbol = stock ?? loan?.collateral.max(by: { $0.valueUSD < $1.valueUSD })?.symbol
        let reserve = symbol.flatMap { s in reserves.first { $0.symbol.uppercased() == s.uppercased() } }
        if let symbol, reserve == nil { throw ExecutionError("Kamino takes no \(symbol) as collateral.") }
        async let usdcRisk = Kamino.risk(reserve: usdcReserve.address)
        var risk: Kamino.Risk?
        if let reserve { risk = try await Kamino.risk(reserve: reserve.address) }
        let held = loan?.collateral.first { $0.reserve == reserve?.address }?.tokens ?? 0
        let after = Self.kaminoAfter(loan, adding: adding ?? 0, held: held, price: reserve?.priceUSD ?? 0, risk: risk, usdc: usdc,
                                     borrowFactor: try await usdcRisk.borrowFactor)
        return KaminoLoanPreview(market: market, stock: reserve?.symbol, stockTokens: held + (adding ?? 0), collateralUSD: after.collateral,
                                 availableUSD: after.available, debtUSD: after.debt, healthNow: loan?.health, healthAfter: after.health,
                                 liquidationPrice: after.liquidation, borrowAPY: usdcReserve.borrowAPY)
    }

    /// The position once Kamino's API shows the move just made (`done`): it lags behind the chain — a first deposit
    /// read as no position at all, and a repaid debt as still owed for over 10 s (live, 2026-10-29). 30 seconds at most.
    func kaminoLoan(_ market: String, until done: (Kamino.Loan?) -> Bool) async throws -> Kamino.Loan? {
        var now = try await Kamino.loan(wallet: wallet.solana, market: market)
        for _ in 0..<30 where !done(now) {
            try await Task.sleep(for: .seconds(1))
            now = try await Kamino.loan(wallet: wallet.solana, market: market)
        }
        return now
    }

    static func usdcOwed(_ loan: Kamino.Loan?) -> Decimal { loan?.debts.first { $0.symbol == "USDC" }?.tokens ?? 0 }

    static func kaminoMarketName(_ market: String) -> String { market == Kamino.backpackMarket ? "Kamino's Backpack market" : "Kamino" }

    /// Borrows `usdc` on Kamino to this wallet; `adding` of `stock` goes in as collateral first (its own transaction,
    /// the first one on the market pays the position's rent). Refused when the health factor would fall under
    /// `minimumHealth` or past Kamino's borrow limit.
    public func kaminoBorrow(usdc: Decimal, against stock: String?, adding: Decimal?) async throws -> String {
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard units > 0 else { throw ExecutionError("Say how many USDC to borrow.") }
        let p = try await kaminoBorrowPreview(usdc: usdc, against: stock, adding: adding)
        let place = Self.kaminoMarketName(p.market)
        guard p.collateralUSD > 0 else {
            throw ExecutionError("Nothing on Kamino to borrow against: put a stock up first — «borrow \(Fmt.num(usdc)) USDC on kamino against 0.05 SPCX» "
                                 + "puts 0.05 SPCX in as collateral and borrows against it. Nothing was signed.")
        }
        guard usdc <= p.availableUSD else {
            throw ExecutionError("\(place) lets this wallet borrow \(Fmt.usd(p.availableUSD)) more against \(Fmt.usd(p.collateralUSD)) of collateral; "
                                 + "\(Fmt.num(usdc)) USDC is past it. Borrow less, or put more up. Nothing was signed.")
        }
        guard let health = p.healthAfter, health >= Self.minimumHealth else {
            throw ExecutionError("That would leave a health factor of \(Fmt.num(p.healthAfter ?? 0, maxFraction: 2)); ihatecrypto borrows only down to "
                                 + "\(Self.minimumHealth) (Kamino liquidates at 1.0). Borrow less, or put more up. Nothing was signed.")
        }
        guard let usdcReserve = try await Kamino.reserves(market: p.market).first(where: { $0.symbol == "USDC" }) else {
            throw ExecutionError("Kamino lends no USDC on that market just now. Nothing was signed.")
        }
        var deposited = false
        if let stock, let adding, adding > 0 { _ = try await kamino(stock, amount: adding, deposit: true); deposited = true }
        do {
            // Kamino builds a borrow only once it sees the position a deposit just opened: a few seconds after it lands.
            var wire: Data?
            for attempt in 0..<10 {
                do { wire = try await Kamino.borrowTransaction(wallet: wallet.solana, reserve: usdcReserve.address, amount: usdc, market: p.market); break }
                catch where attempt < 9 && "\(error)".contains("NOT_FOUND") { try await Task.sleep(for: .seconds(2)) }
            }
            guard let wire else { throw ExecutionError("Kamino didn't build the borrow.") }
            let myUSDC = try SPL.associatedAccount(owner: wallet.solana, mint: usdcReserve.mint)
            let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
            report("\(place): \(Fmt.usd(p.collateralUSD)) of collateral, health factor \(p.healthNow.map { Fmt.num($0, maxFraction: 2) } ?? "∞") → \(Fmt.num(health, maxFraction: 2))"
                   + (p.liquidationPrice.map { "; liquidation if \(p.stock ?? "the collateral") ≤ \(Fmt.usd($0))" } ?? "") + ".")
            // The first borrow also opens the position's account in Kamino's farm for the debt (init_obligation_farms_for_reserve):
            // ≈ 0.0053 SOL of rent on top of the fee (measured live 2026-10-28), so the floor is 0.012 SOL, not the usual 0.005.
            _ = try await signAndSend(wire, lastValid: lastValid, owners: [myUSDC: wallet.solana], rules: [(myUSDC, "USDC", true, Int64(units) * 99 / 100)],
                                      note: "Kamino borrow \(Fmt.num(usdc)) USDC", lamportsAtLeast: -12_000_000)
        } catch where deposited {
            // The stock is up already: the same words again would put it up twice.
            throw ExecutionError("The \(Fmt.num(adding ?? 0, maxFraction: 6)) \(stock ?? "") is up as collateral on \(place) now, but the borrow didn't go through: "
                                 + "\(error.localizedDescription) «borrow \(Fmt.num(usdc)) USDC on kamino against \(stock ?? "")» borrows against it without putting more up.")
        }
        let owed = Self.usdcOwed(try await Kamino.loan(wallet: wallet.solana, market: p.market)), was = p.debtUSD - usdc
        let now = try await kaminoLoan(p.market) { Self.usdcOwed($0) > owed || ($0?.debtUSD ?? 0) > was + usdc / 2 }
        let line = "Borrowed \(Fmt.num(usdc)) USDC on \(place) at \(Fmt.num(p.borrowAPY.rounded(2, .plain)))% a year: health factor "
            + "\(now?.health.map { Fmt.num($0, maxFraction: 2) } ?? Fmt.num(health, maxFraction: 2))"
            + (p.liquidationPrice.map { ", liquidation if \(p.stock ?? "the collateral") ≤ \(Fmt.usd($0))" } ?? "")
            + ", owed \(Fmt.num(max(Self.usdcOwed(now), usdc), maxFraction: 6)) USDC. "
            + (p.market == Kamino.backpackMarket ? "Stock prices stand still while US markets are closed; a gap at the open can reach that line at once. " : "")
            + "Say «repay all USDC on kamino» to pay it back."
        report(line)
        return line
    }

    /// Pays back `usdc` of the USDC debt on Kamino (nil = all of it, interest included) from the USDC on Solana, on the
    /// market that holds the debt. Kamino takes no more than is owed.
    public func kaminoRepay(usdc: Decimal?) async throws -> String {
        var found: (market: String, loan: Kamino.Loan, owed: Decimal, reserve: Kamino.Reserve)?
        for m in [Kamino.backpackMarket, Kamino.mainMarket] {
            guard let loan = try await Kamino.loan(wallet: wallet.solana, market: m), let d = loan.debts.first(where: { $0.symbol == "USDC" }), d.tokens > 0,
                  let r = try await Kamino.reserves(market: m).first(where: { $0.symbol == "USDC" }) else { continue }
            // The stored amount stands still between the position's refreshes on chain; Kamino's refreshed dollars
            // carry the interest since (USDC at its price there).
            let refreshed = loan.debts.count == 1 && r.priceUSD > 0 ? loan.debtUSD / r.priceUSD : 0
            found = (m, loan, max(d.tokens, refreshed), r); break
        }
        guard let found else { throw ExecutionError("No USDC debt on Kamino.") }
        let place = Self.kaminoMarketName(found.market), usdcReserve = found.reserve
        // All of it: the interest of the next minutes too, a tenth of a percent over; Kamino takes only what's owed.
        var asked = usdc.map { min($0, found.owed) } ?? (found.owed * Decimal(string: "1.001")!).rounded(6, .up)
        let held = Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: usdcReserve.mint)) / 1_000_000
        if usdc == nil, held < asked, held >= found.owed { asked = held }   // exactly the debt is enough
        let units = NSDecimalNumber(decimal: (asked * 1_000_000).rounded(0, .up)).uint64Value
        guard held >= asked else {
            throw ExecutionError("Paying back \(Fmt.num(asked, maxFraction: 6)) USDC needs it on Solana; the wallet has \(Fmt.num(held)) USDC there. Nothing was signed.")
        }
        let wire = try await Kamino.repayTransaction(wallet: wallet.solana, reserve: usdcReserve.address, amount: asked, market: found.market)
        let myUSDC = try SPL.associatedAccount(owner: wallet.solana, mint: usdcReserve.mint)
        let lastValid = try await rpc.latestBlockhash().lastValidBlockHeight
        _ = try await signAndSend(wire, lastValid: lastValid, owners: [myUSDC: wallet.solana], rules: [(myUSDC, "USDC", true, -Int64(units))],
                                  note: "Kamino repay \(Fmt.num(asked, maxFraction: 6)) USDC")
        let stored = found.loan.debts.first { $0.symbol == "USDC" }?.tokens ?? found.owed
        let now = try await kaminoLoan(found.market) { Self.usdcOwed($0) < stored }
        // What left the wallet, not what was asked: Kamino takes only what's owed. The chain says it before the API.
        let paid = held - Decimal(try await rpc.tokenBalance(owner: wallet.solana, mint: usdcReserve.mint)) / 1_000_000
        let caughtUp = Self.usdcOwed(now) < stored
        // «All» asked a tenth of a percent over what's owed, and Kamino takes no more: nothing is left then.
        let left = caughtUp ? Self.usdcOwed(now) : usdc == nil && paid > 0 ? 0 : max(0, stored - paid)
        let stocks = (now?.collateral ?? []).filter { $0.tokens > 0 }.map(\.symbol)
        let line = "Paid back \(Fmt.num(paid > 0 ? paid : max(found.owed - left, 0), maxFraction: 6)) USDC on \(place): "
            + (left > Decimal(string: "0.00001")! ? "\(Fmt.num(left, maxFraction: 6)) USDC still owed" + (caughtUp ? ", health factor \(now?.health.map { Fmt.num($0, maxFraction: 2) } ?? "∞")." : ".")
               : "no debt left." + (stocks.isEmpty ? "" : " The \(stocks.joined(separator: ", ")) stays as collateral: «withdraw \(stocks[0]) from kamino» takes it back."))
        report(line)
        return line
    }
}

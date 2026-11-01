import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import WalletCore

/// Borrowing USDC on Aave against what the wallet supplied there (ETH as collateral, or its own USDC), and paying it
/// back. A borrow leaves a health factor of at least `minimumHealth` — ihatecrypto's own risk limit, above Aave's 1.0
/// at which the collateral is sold at a penalty — and the card says at which price of the collateral that would happen.
public struct AaveAccount: Sendable, Equatable {
    /// USD, as Aave's oracle prices them.
    public let collateral: Decimal
    public let debt: Decimal
    public let available: Decimal
    /// The collateral's weighted liquidation threshold, 0…1.
    public let threshold: Decimal
    /// nil without debt.
    public var health: Decimal? { Self.health(collateral: collateral, debt: debt, threshold: threshold) }

    public static func health(collateral: Decimal, debt: Decimal, threshold: Decimal) -> Decimal? {
        debt > 0 ? collateral * threshold / debt : nil
    }

    /// The price of a volatile collateral (`amount` of it at `price` now, the rest of the collateral steady) at which the
    /// health factor reaches 1 with `debt`; nil when the rest alone covers the debt. Uses the account's weighted threshold.
    // ponytail: one threshold for all the collateral; per-asset thresholds if positions mix assets with different ones.
    public static func liquidationPrice(debt: Decimal, threshold: Decimal, collateral: Decimal, amount: Decimal, price: Decimal) -> Decimal? {
        guard threshold > 0, amount > 0, price > 0 else { return nil }
        let rest = collateral - amount * price
        let needed = debt / threshold - rest
        return needed > 0 ? needed / amount : nil
    }
}

extension Executor {
    /// What a borrow may leave: Aave liquidates at 1.0; ihatecrypto stops at 1.5.
    public static let minimumHealth: Decimal = Decimal(string: "1.5")!

    /// The wallet's account on an Aave market.
    public func aaveAccount(_ m: AaveV3.Market) async throws -> AaveAccount {
        let d = try await m.rpc.read(m.pool, try AaveV3.accountData(wallet.evm))
        // Six words, always: a node answering null or 0x would read as an account with nothing on it.
        guard d.count >= 6 * 32 else { throw ExecutionError("Aave on \(m.chainName) didn't answer. Try again in a minute.") }
        func usd(_ w: Int) -> Decimal { (ABI.decimal(in: d, word: w) ?? 0) / 100_000_000 }
        return AaveAccount(collateral: usd(0), debt: usd(1), available: usd(2), threshold: (ABI.decimal(in: d, word: 3) ?? 0) / 10_000)
    }

    /// The account once its debt shows a change from `before`: a node a block behind answered the old one right after the
    /// receipt («Paid back 0 USDC… debt $1.00» for a repay that had gone through). Ten seconds at most.
    func aaveAccount(_ m: AaveV3.Market, changedFrom before: AaveAccount) async throws -> AaveAccount {
        var now = try await aaveAccount(m)
        for _ in 0..<10 where now.debt == before.debt {
            try await Task.sleep(for: .seconds(1))
            now = try await aaveAccount(m)
        }
        return now
    }

    /// ETH supplied there, by its aToken, and its price on Aave's oracle; nil without any.
    func aaveETHCollateral(_ m: AaveV3.Market) async throws -> (amount: Decimal, price: Decimal)? {
        guard let weth = UniswapV2.on(m.rpc.chainId)?.weth,
              let aWETH = ABI.address(in: try await m.rpc.read(m.pool, try AaveV3.getReserveData(weth)), word: 8) else { return nil }
        let held = try await balance(aWETH, on: m.rpc) / 1_000_000_000_000_000_000
        guard held > 0, let oracle = ABI.address(in: try await m.rpc.read(m.provider, AaveV3.getPriceOracle), word: 0),
              let price = ABI.decimal(in: try await m.rpc.read(oracle, try AaveV3.assetPrice(weth)), word: 0) else { return nil }
        return (held, price / 100_000_000)
    }

    /// What borrowing `usdc` would do, read now: the health factor before and after, and the price of ETH at which the
    /// position would be liquidated. Nothing is signed.
    public func aaveBorrowPreview(_ m: AaveV3.Market, usdc: Decimal) async throws -> (now: AaveAccount, after: Decimal?, liquidation: Decimal?) {
        let account = try await aaveAccount(m)
        let debt = account.debt + usdc
        let after = AaveAccount.health(collateral: account.collateral, debt: debt, threshold: account.threshold)
        let eth = try await aaveETHCollateral(m)
        let liquidation = eth.flatMap { AaveAccount.liquidationPrice(debt: debt, threshold: account.threshold, collateral: account.collateral, amount: $0.amount, price: $0.price) }
        return (account, after, liquidation)
    }

    /// Borrows `usdc` on Aave (variable rate) to this wallet, refused when the health factor would fall under
    /// `minimumHealth` or past what Aave lets it borrow.
    public func aaveBorrow(_ m: AaveV3.Market, usdc: Decimal) async throws -> String {
        let units = NSDecimalNumber(decimal: (usdc * 1_000_000).rounded(0, .down)).uint64Value
        guard units > 0 else { throw ExecutionError("Say how many USDC to borrow.") }
        let p = try await aaveBorrowPreview(m, usdc: usdc)
        guard p.now.collateral > 0 else {
            throw ExecutionError("Nothing on Aave on \(m.chainName) to borrow against: supply collateral first — «supply 0.01 ETH as collateral on Aave on \(m.chainName)».")
        }
        guard usdc <= p.now.available else {
            throw ExecutionError("Aave lets this wallet borrow \(Fmt.usd(p.now.available)) more on \(m.chainName) now; \(Fmt.num(usdc)) USDC is past it. Nothing was signed.")
        }
        guard let after = p.after, after >= Self.minimumHealth else {
            throw ExecutionError("That would leave a health factor of \(Fmt.num(p.after ?? 0, maxFraction: 2)); ihatecrypto borrows only down to \(Self.minimumHealth) "
                                 + "(Aave sells the collateral at 1.0). Borrow less, or add collateral. Nothing was signed.")
        }
        report("Aave on \(m.chainName): \(Fmt.usd(p.now.collateral)) of collateral, health factor \(p.now.health.map { Fmt.num($0, maxFraction: 2) } ?? "∞") → \(Fmt.num(after, maxFraction: 2))"
               + (p.liquidation.map { "; liquidation if ETH ≤ \(Fmt.usd($0))" } ?? "") + ".")
        let before = try await m.rpc.tokenBalance(m.usdc, owner: wallet.evm)
        let hash = try await sendEVM(m.rpc, to: m.pool, data: try AaveV3.borrow(asset: m.usdc, amount: units, onBehalfOf: wallet.evm),
                                     note: "Borrow \(Fmt.num(usdc)) USDC on Aave on \(m.chainName)")
        let now = try await aaveAccount(m, changedFrom: p.now)
        let got = (Decimal(try await m.rpc.tokenBalance(m.usdc, owner: wallet.evm)) - Decimal(before)) / 1_000_000
        return "Borrowed \(Fmt.num(max(got, 0) > 0 ? got : usdc)) USDC on Aave on \(m.chainName): health factor \(now.health.map { Fmt.num($0, maxFraction: 2) } ?? "∞")"
            + (p.liquidation.map { ", liquidation if ETH ≤ \(Fmt.usd($0))" } ?? "") + ", debt \(Fmt.usd(now.debt)). Say “repay all USDC on Aave” to pay it back. "
            + "\(EVMNetwork.of(chainId: m.rpc.chainId)?.explorer ?? "")/tx/\(hash)"
    }

    /// Pays back `usdc` of the USDC debt on Aave (nil = all of it, interest included) from the USDC on that network: an
    /// approve of what's owed (a tenth of a percent over, for the interest of the next blocks), then the repay.
    public func aaveRepay(_ m: AaveV3.Market, usdc: Decimal?) async throws -> String {
        guard let debtToken = ABI.address(in: try await m.rpc.read(m.pool, try AaveV3.getReserveData(m.usdc)), word: 10) else {
            throw ExecutionError("Aave's USDC reserve on \(m.chainName) didn't answer. Nothing was signed.")
        }
        let owed = try await balance(debtToken, on: m.rpc)
        guard owed > 0 else { throw ExecutionError("No USDC debt on Aave on \(m.chainName).") }
        let units = usdc.map { min(($0 * 1_000_000).rounded(0, .down), owed) } ?? owed
        let approve = usdc == nil ? (owed + owed / 1000 + 10).whole : units
        let held = Decimal(try await m.rpc.tokenBalance(m.usdc, owner: wallet.evm)), was = try await aaveAccount(m)
        guard held >= approve else {
            throw ExecutionError("\(Fmt.num(held / 1_000_000)) USDC on \(m.chainName); paying back \(Fmt.num(approve / 1_000_000)) needs it there first. Nothing was signed.")
        }
        let hash = try await approveAndSend(m.rpc, token: m.usdc, spender: m.pool, units: approve, approveNote: "Approve \(Fmt.num(approve / 1_000_000)) USDC to Aave",
                                            to: m.pool, data: try AaveV3.repay(asset: m.usdc, amount: usdc == nil ? nil : NSDecimalNumber(decimal: units).uint64Value,
                                                                                onBehalfOf: wallet.evm),
                                            note: "Repay \(usdc == nil ? "all the" : Fmt.num(units / 1_000_000)) USDC debt on Aave on \(m.chainName)")
        let now = try await aaveAccount(m, changedFrom: was)
        // Aave counts the debt in dollars on its oracle (USDC ≈ $0.9999): said as dollars.
        return "Paid back \(Fmt.usd(max(was.debt - now.debt, 0))) of the USDC debt on Aave on \(m.chainName): "
            + (now.debt > 0 ? "debt \(Fmt.usd(now.debt)), health factor \(now.health.map { Fmt.num($0, maxFraction: 2) } ?? "∞")." : "no debt left.")
            + " \(EVMNetwork.of(chainId: m.rpc.chainId)?.explorer ?? "")/tx/\(hash)"
    }

    /// ETH as collateral on Aave: wrapped into the network's WETH, an exact approve, the supply — to this wallet.
    public func aaveSupplyETH(_ m: AaveV3.Market, eth: Decimal) async throws -> String {
        guard let weth = UniswapV2.on(m.rpc.chainId)?.weth else { throw ExecutionError("No WETH ihatecrypto knows on \(m.chainName).") }
        let wei = (eth * 1_000_000_000_000_000_000).rounded(0, .down)
        guard wei > 0, let value = UInt64(exactly: NSDecimalNumber(decimal: wei).uint64Value), Decimal(value) == wei else { throw ExecutionError("Say how much ETH, under 18.") }
        let have = try await m.rpc.balance(wallet.evm)
        guard have > value else { throw ExecutionError("\(Fmt.num(Decimal(have) / 1_000_000_000_000_000_000, maxFraction: 6)) ETH on \(m.chainName): its fees come out of it too. Nothing was signed.") }
        _ = try await sendEVM(m.rpc, to: weth, data: Data([0xd0, 0xe3, 0x0d, 0xb0]), note: "Wrap \(Fmt.num(eth, maxFraction: 6)) ETH", value: value)
        let hash = try await approveAndSend(m.rpc, token: weth, spender: m.pool, units: wei, approveNote: "Approve exactly \(Fmt.num(eth, maxFraction: 6)) WETH to Aave",
                                            to: m.pool, data: try AaveV3.supply(asset: weth, amount: value, onBehalfOf: wallet.evm),
                                            note: "Supply \(Fmt.num(eth, maxFraction: 6)) ETH as collateral on Aave on \(m.chainName)")
        let now = try await aaveAccount(m)
        return "Supplied \(Fmt.num(eth, maxFraction: 6)) ETH as collateral on Aave on \(m.chainName): \(Fmt.usd(now.collateral)) of collateral, "
            + "\(Fmt.usd(now.available)) borrowable. \(EVMNetwork.of(chainId: m.rpc.chainId)?.explorer ?? "")/tx/\(hash)"
    }
}

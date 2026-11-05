import DeFiJupiterLend
import DeFiKamino
import DeFiYields
import EVMChain
import ExecutionCore
import Foundation
import HyperliquidSigning
import IhatecryptoCore
import RelayBridge
import TxGuard

/// «Переложи в лучшие ставки»: where the USDC earns now, the plan (`Rebalance.plan`), and with `execute` each move
/// through Solana — out of one place, into the next — by the same checked paths as the single commands.
extension Executor {
    public func rebalance(maxShare: Decimal = 50, execute: Bool) async throws -> (text: String, ready: Bool, plan: Rebalance.Plan) {
        let places = try await rebalancePlaces()
        let plan = Rebalance.plan(places: places, maxShare: maxShare)
        var lines = ["USDC in \(places.filter { $0.amount > 0 }.count) places, at most \(Fmt.pct(maxShare)) per protocol; a move is made only if it pays back within 30 days:"]
        lines += plan.moves.map { "· \($0.from) → \($0.to): \(Fmt.num($0.amount, maxFraction: 2)) USDC, \(Fmt.num($0.fromAPY.rounded(2, .plain)))% → \(Fmt.num($0.toAPY.rounded(2, .plain)))%, +\(Fmt.usd($0.yearlyGain)) a year for ≈ \(Fmt.usd($0.cost))" }
        lines += plan.skipped.map { "· skipped \($0.move.from) → \($0.move.to): \($0.reason)" }
        lines += plan.notes.map { "· " + $0 }
        guard execute, !plan.moves.isEmpty else { return (lines.joined(separator: "\n"), !plan.moves.isEmpty, plan) }
        report(lines.joined(separator: "\n"))
        var done: [String] = []
        for (i, move) in plan.moves.enumerated() {
            let step = "Step \(i + 1) of \(plan.moves.count), \(move.from) → \(move.to)"
            let before = try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)
            do {
                try await rebalanceWithdraw(move.from, move.amount, from: places)
            } catch {
                throw ExecutionError("\(step) stopped before any money left \(move.from): \(error.localizedDescription)" + Self.moved(done))
            }
            // A bridge back delivers a little less than it took: deposit what actually arrived, never more than planned.
            let arrived = Decimal(Int64(try await rpc.tokenBalance(owner: wallet.solana, mint: Relay.Currency.solanaUSDC)) - Int64(before)) / 1_000_000
            let piece = min(move.amount, arrived > 0 ? arrived : move.amount).rounded(2, .down)
            do {
                try await rebalanceDeposit(move.to, piece)
            } catch {
                throw ExecutionError("\(step) stopped after the withdrawal: \(Fmt.num(piece)) USDC is on your Solana wallet, not in \(move.to). \(error.localizedDescription)" + Self.moved(done))
            }
            done.append("\(Fmt.num(piece)) USDC \(move.from) → \(move.to)")
            report("\(step): done.")
        }
        let line = "Rebalanced: \(done.joined(separator: "; ")). About +\(Fmt.usd(plan.yearlyGain)) a year for ≈ \(Fmt.usd(plan.cost)) of costs."
        report(line)
        return (line, false, plan)
    }

    private static func moved(_ done: [String]) -> String { done.isEmpty ? "" : " Already moved: \(done.joined(separator: "; "))." }

    /// Every place ihatecrypto can move USDC between, with what is in it, its rate and what getting out and in costs.
    func rebalancePlaces() async throws -> [Rebalance.Place] {
        async let optionsCall = Yields.options(for: "USDC")
        async let vaultsCall = try? JupiterLend.vaults()
        async let jupCall = try? JupiterLend.positions(user: wallet.solana)
        async let kamCall = try? Kamino.positions(wallet: wallet.solana)
        async let evmCall = evmPlaces()
        let options = await optionsCall
        let usdcVault = (await vaultsCall ?? []).first { $0.assetSymbol.uppercased() == "USDC" }
        let jupPositions = await jupCall ?? []
        let jupiter = usdcVault.flatMap { v in jupPositions.first { $0.vault == v.symbol }.map { Decimal($0.assets) / 1_000_000 } } ?? 0
        let kamino = (await kamCall ?? []).filter { $0.symbol.uppercased() == "USDC" }.reduce(0) { $0 + $1.valueUSD }
        let hlp: (equity: Decimal, withdrawable: Decimal, lockedUntil: Date?) = (try? await HyperliquidAccount.vaultEquity(user: wallet.evm, vault: HLGuard.hlpVault)) ?? (0, 0, nil)
        let solPrice = (try? await Hyperliquid.markets()["SOL"]?.markPx) ?? 0
        func apy(_ place: String) -> Decimal? { options.first { $0.place == place }?.apy }
        // Estimates, shown as such: Kamino's first deposit opens accounts for ≈ 0.03 SOL of rent at September's rate, less as
        // rent falls (kaminoFirstRent; 0.0176 of it comes back on a full exit); an EVM place's round trip is split half out,
        // half in (EVMPlaces.swift).
        let kaminoRent = kamino > 0 ? 0 : await Self.kaminoFirstRent(rpc)
        var places: [Rebalance.Place] = []
        if let a = apy("Jupiter Lend") { places.append(.init(name: "Jupiter Lend", apy: a, amount: jupiter)) }
        if let a = apy("Kamino Lend") {
            places.append(.init(name: "Kamino Lend", apy: a, amount: kamino, entryCost: kaminoRent * solPrice))
        }
        for p in await evmCall {
            places.append(.init(name: p.name, apy: p.apy, amount: p.supplied, exitCost: p.roundTripUSD / 2, entryCost: p.roundTripUSD / 2,
                                protocol: p.protocolName))
        }
        if let a = apy("HLP (Hyperliquid)"), hlp.equity > 0 {
            let locked = hlp.lockedUntil.flatMap { $0 > Date() ? "HLP locks deposits for 4 days, until \(Fmt.date($0))" : nil }
            places.append(.init(name: "HLP (Hyperliquid)", apy: a, amount: hlp.equity, exitCost: Decimal(string: "0.02")!, locked: locked,
                                closed: "HLP takes deposits by its own command («deposit 5 USDC to HLP»): $5 minimum and a 4-day lock"))
        }
        // A vault switched off in Settings › Modules is left alone: nothing moves into it or out of it.
        let modules = Modules.current
        return places.filter { modules.refusal(vault: $0.name) == nil }
    }

    private func rebalanceWithdraw(_ place: String, _ amount: Decimal, from places: [Rebalance.Place]) async throws {
        // Everything that is there goes out whole (no dust, no rounding against the protocol's own accounting).
        let all = places.first { $0.name == place }.map { amount >= $0.amount - Decimal(string: "0.01")! } ?? false
        let asked: Decimal? = all ? nil : amount.rounded(2, .down)
        switch place {
        case "Jupiter Lend": _ = try await lend("USDC", amount: asked, deposit: false)
        case "Kamino Lend": _ = try await kamino("USDC", amount: asked, deposit: false)
        case "HLP (Hyperliquid)":
            // Only what left HLP goes on to Solana: free trading margin and a HIP-3 exchange's stay where they are.
            let before = try await Hyperliquid.balance(user: wallet.evm).withdrawable
            let out = min(asked ?? .greatestFiniteMagnitude, try await HyperliquidAccount.vaultEquity(user: wallet.evm, vault: HLGuard.hlpVault).withdrawable)
            _ = try await hlp(deposit: false, amount: asked)
            var free = before
            for _ in 0..<10 {
                free = try await Hyperliquid.balance(user: wallet.evm).withdrawable
                if free > before { break }
                try await Task.sleep(for: .seconds(1))
            }
            let usdc = min(out, free - before).rounded(2, .down)
            guard usdc > 0 else { throw ExecutionError("HLP's withdrawal isn't on the Hyperliquid balance yet, so nothing was bridged; «withdraw to Solana» brings it once it shows.") }
            _ = try await withdrawToSolana(usdc: usdc)
        default:
            if let m = AaveV3.market(place: place) { _ = try await aave(deposit: false, usdc: asked, market: m) }
            else if let v = MorphoVault.vault(place: place) { _ = try await morpho(deposit: false, usdc: asked, vault: v) }
            else { throw ExecutionError("ihatecrypto can't take money out of \(place).") }
        }
    }

    private func rebalanceDeposit(_ place: String, _ amount: Decimal) async throws {
        switch place {
        case "Jupiter Lend": _ = try await lend("USDC", amount: amount, deposit: true)
        // The plan already counted Kamino's first-deposit rent against the move.
        case "Kamino Lend": _ = try await kamino("USDC", amount: amount, deposit: true, acceptRent: true)
        default:
            if let m = AaveV3.market(place: place) { _ = try await aave(deposit: true, usdc: amount, market: m) }
            else if let v = MorphoVault.vault(place: place) { _ = try await morpho(deposit: true, usdc: amount, vault: v) }
            else { throw ExecutionError("ihatecrypto can't put money into \(place) this way.") }
        }
    }
}

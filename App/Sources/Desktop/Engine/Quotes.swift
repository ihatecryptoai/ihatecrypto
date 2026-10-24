import Foundation
import IhatecryptoCore
import JupiterSwap

/// A Solana swap's price while its card waits for Confirm, as the Mac's quoteJupiter: Jupiter is asked once (again every
/// 15 s while it fails), the card's step shows what comes out, and Confirm holds the swap to it — the Executor won't take
/// 1% less than the card showed (`shown`).
extension Engine {
    func quoteCard(_ command: WalletCommand, board: String) async {
        // Everything but Jupiter's: the bridge, the swap off Solana, Aave's health factor, a coin's cost (MoveRoutes.swift).
        if await quoteRoute(command, board: board) { return }
        let pair: (SpotAsset, SpotAsset, Decimal)?
        switch command {
        case .buy(let asset, let usdc?): pair = (.usdc, asset, usdc)
        case .sell(let asset, let amount?): pair = (asset, .usdc, amount)
        case .swapOn("solana", let a, let b, let x?):
            if let from = await solanaAsset(a), let to = await solanaAsset(b) { pair = (from, to, x) } else { pair = nil }
        default: pair = nil
        }
        let jupiter: Bool = switch command { case .buy(_, _?), .sell(_, _?), .swapOn("solana", _, _, _?): true; default: false }
        let units = pair.map { NSDecimalNumber(decimal: ($0.2 * Decimal(sign: .plus, exponent: $0.0.decimals, significand: 1)).rounded(0, .down)).uint64Value } ?? 0
        guard let (from, to, _) = pair, units > 0 else {
            // A token Jupiter can't name, or under one unit: Confirm shows, the run says why if it fails (the Mac).
            if jupiter, quotedMove == command, card?.id == board { unpriced = command; changed() }
            return
        }
        while !Task.isCancelled, card?.id == board {
            if let q = try? await Jupiter.quote(input: from.mint, output: to.mint, amount: units) {
                guard card?.id == board, quotedMove == command else { return }
                let out = Decimal(q.outAmount) / Decimal(sign: .plus, exponent: to.decimals, significand: 1)
                jupiterShown = (command, q.outAmount, "≈ \(Fmt.num(out, maxFraction: 6)) \(to.symbol)", "Jupiter: \(from.symbol) → \(to.symbol)")
                // On its route's last step (routeSteps), a single move's or a chain's first: every redraw keeps it.
                redrawRoute(board, command)
                return
            }
            quoteFailed(command)
            try? await Task.sleep(for: .seconds(15))
        }
    }

    /// A priced swap's quote asked and not given: the card shows Refresh and «the price is over 15 s old» while it keeps
    /// asking, as the Mac's empty swapQuote — never «Quoting…» for as long as the place doesn't answer.
    func quoteFailed(_ c: WalletCommand) {
        guard quotedMove == c, priced(c), wayQuotes?.command != c, jupiterShown?.command != c else { return }
        wayQuotes = (c, [])
        changed()
    }

    /// The move a waiting card prices: a single move, or a chain's first, as the Mac's chain card is its first move's
    /// (Canvases.swift:3001-3005): quoted, counted down and held to its price the same way.
    var quotedMove: WalletCommand? {
        switch pending {
        case .wallet(let c)?: return c
        case .chain(let list)?: if case .wallet(let c)? = list.first { return c } else { return nil }
        default: return nil
        }
    }

    /// The route's steps with Jupiter's quote on the step that gets it: the last of the route (the token bought, or the
    /// USDC back).
    func withJupiter(_ c: WalletCommand, _ steps: [Board.Step]) -> [Board.Step] {
        guard let j = jupiterShown, j.command == c, let last = steps.indices.last else { return steps }
        var steps = steps
        steps[last].title = j.title
        if !steps[last].lines.contains(j.route) { steps[last].lines.insert(j.route, at: 0) }
        return steps
    }

    /// The output the card showed for this very command, for the Executor's floor; nil for any other.
    func shownOut(_ command: WalletCommand) -> UInt64? {
        jupiterShown.flatMap { $0.command == command ? $0.out : nil }
    }
}

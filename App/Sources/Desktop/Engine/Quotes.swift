import Foundation
import IhatecryptoCore
import JupiterSwap

/// A Solana swap's price while its card waits for Confirm, as the Mac's quoteJupiter: Jupiter is asked once (again every
/// 15 s while it fails), the card's step shows what comes out, and Confirm holds the swap to it — the Executor won't take
/// 1% less than the card showed (`shown`).
extension Engine {
    func quoteCard(_ command: WalletCommand, board: String) async {
        let pair: (SpotAsset, SpotAsset, Decimal)?
        switch command {
        case .buy(let asset, let usdc?): pair = (.usdc, asset, usdc)
        case .sell(let asset, let amount?): pair = (asset, .usdc, amount)
        case .swapOn("solana", let a, let b, let x?):
            if let from = await solanaAsset(a), let to = await solanaAsset(b) { pair = (from, to, x) } else { pair = nil }
        default: pair = nil
        }
        guard let (from, to, amount) = pair else { return }
        let units = NSDecimalNumber(decimal: (amount * Decimal(sign: .plus, exponent: from.decimals, significand: 1)).rounded(0, .down)).uint64Value
        guard units > 0 else { return }
        while !Task.isCancelled, card?.id == board {
            if let q = try? await Jupiter.quote(input: from.mint, output: to.mint, amount: units) {
                guard card?.id == board else { return }
                jupiterShown = (command, q.outAmount)
                let out = Decimal(q.outAmount) / Decimal(sign: .plus, exponent: to.decimals, significand: 1)
                updateBoard(board) { b in
                    guard var step = b.steps?.first else { return }
                    step.title = "≈ \(Fmt.num(out, maxFraction: 6)) \(to.symbol)"
                    let route = "Jupiter: \(from.symbol) → \(to.symbol)"
                    if !step.lines.contains(route) { step.lines.insert(route, at: 0) }
                    b.steps?[0] = step
                }
                return
            }
            try? await Task.sleep(for: .seconds(15))
        }
    }

    /// The output the card showed for this very command, for the Executor's floor; nil for any other.
    func shownOut(_ command: WalletCommand) -> UInt64? {
        jupiterShown.flatMap { $0.command == command ? $0.out : nil }
    }
}

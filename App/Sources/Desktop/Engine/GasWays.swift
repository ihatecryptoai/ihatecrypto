import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution

/// Dollars where the fee coin is missing, as the Mac's ReceiveBoard.stuck/offerGasWays: the first move there would stop,
/// so the chat says so once per wallet and network, with the ways out as cards (the owner, 2026-10-23: «делай так чтобы
/// это работало и предлагало если нету газа»).
nonisolated struct StuckDollars: Equatable {
    let network: String
    let key: String
    let coin: String
    /// What to add of the coin, in dollars.
    let usd: Decimal
    /// The dollars there.
    let usdc: Decimal
    /// Cards that get the dollars moving without adding the coin first; empty: only adding it works there.
    let ways: [ChatRouter.Option]
}

extension Engine {
    /// The least USDC Jupiter's wallet pays a swap's fee for (gasless, for a wallet that can't pay): $5.
    nonisolated static let gaslessSwapUSD: Decimal = 5

    nonisolated static func gasUSD(_ key: String) -> Decimal { max(1, (EVMNetwork.named(key)?.topupUSD ?? 1).rounded(0, .up)) }

    /// The Mac's rule from plain numbers: Solana's USDC and SOL, Hyperliquid's free dollars, and each EVM network's dollars
    /// and gas coin (`held`). Solana without SOL swaps for it with Jupiter paying; an EVM network without its gas coin,
    /// where Solana and Hyperliquid can't bring it, moves its dollars by signature (Relay, or BNB Chain's MegaFuel).
    nonisolated static func stuck(solanaUSDC usd: Decimal, solanaSOL: Decimal, hyperliquidFree: Decimal, say: (String, String) -> String,
                                  held: (EVMNetwork) -> (usdc: Decimal, gas: Decimal)) -> [StuckDollars] {
        let solanaPays = solanaSOL >= Decimal(TopUpOrder.solanaFeeFloor) / 1_000_000_000
        var out: [StuckDollars] = []
        if usd >= 1, !solanaPays {
            let swap = gaslessSwapUSD
            out.append(StuckDollars(network: "Solana", key: "solana", coin: "SOL", usd: 1, usdc: usd, ways: usd >= swap ? [
                ChatRouter.Option(title: say("Swap \(Fmt.num(swap)) USDC for SOL", "Обменять \(Fmt.num(swap)) USDC на SOL"),
                                  comment: say("Jupiter pays this swap's fee; then SOL pays the rest", "Комиссию этого обмена платит Jupiter, дальше платит SOL"),
                                  command: "buy SOL for \(Fmt.num(swap)) USDC", recommended: true),
            ] : []))
        }
        // Gas comes over by itself where Solana (USDC and SOL) or Hyperliquid can bring it (Executor.topUpGas): not stuck.
        guard !(solanaPays && usd >= 2), hyperliquidFree < 2 else { return out }
        for n in EVMNetwork.searched where n.feeToken == nil {
            let (there, gas) = held(n)
            guard there >= 1, gas < Decimal(n.gasFloor) / 1_000_000_000_000_000_000 else { continue }
            let stable = EVMRPC.dollars[n.usdc] ?? "USDC", all = Fmt.num(there.rounded(2, .down))
            let signs = Permit3009.supports(chainId: n.rpc.chainId, token: n.usdc), sponsored = SponsoredTransfer.supports(chainId: n.rpc.chainId, token: n.usdc)
            let bySignature = signs || sponsored ? [
                ChatRouter.Option(title: say("Move \(all) \(stable) to Solana", "Перевести \(all) \(stable) на Solana"),
                                  comment: signs ? say("By signature: Relay pays the gas on \(n.name)", "По подписи: газ на \(n.name) платит Relay")
                                                 : say("BNB Chain pays the gas for this transfer", "Газ за этот перевод платит BNB Chain"),
                                  command: "move \(all) USDC from \(n.key) to Solana", recommended: true),
                ChatRouter.Option(title: say("Get \(n.gasCoin) for gas on \(n.name)", "Взять \(n.gasCoin) на газ в \(n.name)"),
                                  comment: say("Out of these \(stable), over Solana", "Из этих \(stable), через Solana"),
                                  command: "add \(Fmt.usd(gasUSD(n.key))) of gas on \(n.key)"),
            ] : []
            out.append(StuckDollars(network: n.name, key: n.key, coin: n.gasCoin, usd: gasUSD(n.key), usdc: there, ways: bySignature))
        }
        return out
    }

    /// The wallet on screen's stuck dollars, from the portfolio's rows.
    var stuckDollars: [StuckDollars] {
        func value(_ key: String, _ match: (PortfolioView.Row) -> Bool) -> Decimal {
            portfolio.networks.first { $0.key == key }?.rows.filter(match).reduce(0) { $0 + $1.value } ?? 0
        }
        return Self.stuck(solanaUSDC: value("solana") { $0.symbol == "USDC" }, solanaSOL: value("solana") { $0.symbol == "SOL" },
                          // Hyperliquid's perps withdrawable, as the Mac's hl.withdrawable: what brings gas from there.
                          hyperliquidFree: placeFacts.hyperliquid?.withdrawable ?? 0, say: { say($0, $1) }) { n in
            (value(n.key) { $0.contract?.lowercased() == n.usdc.lowercased() || ($0.symbol == "USDC" && $0.section == "tokens" && $0.contract != "native") },
             value(n.key) { $0.contract == "native" })
        }
    }

    /// Said once per wallet and network, when nothing else is going on, with the ways out as cards.
    func offerGasWays() {
        guard !watching, !onboarding, card == nil, running == nil, !thinking, let wallet = solanaAddress ?? evmAddress else { return }
        var offered = Store.load(Set<String>.self, "gas-ways-offered.json") ?? []
        guard let stuck = stuckDollars.first(where: { !offered.contains("\(wallet)-\($0.key)") }) else { return }
        offered.insert("\(wallet)-\(stuck.key)")
        Store.save(offered, "gas-ways-offered.json")
        let dollars = Fmt.num(stuck.usdc.rounded(2, .down)), add = Fmt.usd(stuck.usd)
        reply(stuck.ways.isEmpty
              ? say("\(dollars) USDC on \(stuck.network) can't move yet: every move there pays its fee in \(stuck.coin), and the wallet has none. Add ≈ \(add) of \(stuck.coin).",
                    "\(dollars) USDC на \(stuck.network) пока не сдвинуть: каждое действие там платит комиссию в \(stuck.coin), а его на кошельке нет. Добавьте ≈ \(add) \(stuck.coin).")
              : stuck.key == "solana"
              ? say("\(dollars) USDC on Solana, and no SOL for network fees. Swap a part for SOL: Jupiter pays that one swap's fee, and the SOL pays every move after it.",
                    "\(dollars) USDC на Solana, а SOL на комиссии нет. Обменяйте часть на SOL: комиссию этого обмена платит Jupiter, а дальше все действия оплачивает SOL.")
              : stuck.key == "bnb"
              ? say("\(dollars) USDC on BNB Chain, and no BNB for gas. It can still move: BNB Chain pays the gas for a dollar transfer, so it goes to Solana that way, or brings BNB for gas back from there.",
                    "\(dollars) USDC на BNB Chain, а BNB на газ нет. Сдвинуть их всё равно можно: газ за перевод долларов платит сама BNB Chain. Так они уйдут на Solana или вернут оттуда BNB на газ.")
              : say("\(dollars) USDC on \(stuck.network), and no \(stuck.coin) for gas. It can still move by signature, with Relay paying the gas: bring it to Solana, or take \(stuck.coin) for \(stuck.network) out of it.",
                    "\(dollars) USDC на \(stuck.network), а \(stuck.coin) на газ нет. Сдвинуть их можно и так, по подписи, газ заплатит Relay: перевести на Solana или взять из них \(stuck.coin) для \(stuck.network)."))
        options = stuck.ways + [ChatRouter.Option(title: say("Top up \(stuck.coin)", "Пополнить \(stuck.coin)"),
                                                  comment: say("≈ \(add) from a browser wallet, your phone or an exchange", "≈ \(add) из кошелька в браузере, с телефона или с биржи"),
                                                  command: WalletCommand.receive.summary, recommended: stuck.ways.isEmpty)]
    }
}

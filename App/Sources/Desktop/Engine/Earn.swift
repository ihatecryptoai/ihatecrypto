import DeFiYields
import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// The rates, spreading money across them, rebalancing, Merkl's rewards, coins from networks without a wallet here,
/// token approvals and Jupiter's DCA list — the Mac's, over the same Executor and DeFi modules. What moves money is a
/// card; Confirm runs it.
extension Engine {
    /// Where money earns now: the DeFi modules' rates with Aave's and Morpho's on every network.
    func loadYields(_ asset: String?) async -> [Yields.Option] {
        var options = await Yields.options(for: asset)
        if asset != "SOL" {
            options += await withTaskGroup(of: Yields.Option?.self) { group in
                for m in AaveV3.markets {
                    group.addTask {
                        guard let apy = try? await AaveV3.verify(m) else { return nil }
                        return Yields.Option(asset: "USDC", place: m.place, apy: apy, tvlUSD: nil, how: "lend 2 USDC on Aave" + (m.chain == "base" ? "" : " on \(m.chainName)"), note: "")
                    }
                }
                for v in MorphoVault.vaults {
                    group.addTask {
                        guard let apy = await MorphoRates.netAPY(v) else { return nil }
                        return Yields.Option(asset: "USDC", place: v.place, apy: apy, tvlUSD: nil, how: "deposit 2 USDC into Morpho" + (v.chain == "base" ? "" : " on \(v.chainName)"), note: v.name)
                    }
                }
                var out: [Yields.Option] = []
                for await o in group { if let o { out.append(o) } }
                return out
            }
        }
        return options.sorted { $0.apy > $1.apy }
    }

    func showYields(_ asset: String?, note: String?) async {
        let rates = await loadYields(asset)
        guard !rates.isEmpty else { reply(say("The rate sources didn't answer right now.", "Источники ставок сейчас не ответили."), note: note); return }
        reply(say("Where money earns now:\n", "Где сейчас платят больше всего:\n") + rates.prefix(10).map { o in
            "\(o.place) (\(o.asset)): \(Fmt.num(o.apy.rounded(2, .plain)))% " + say("a year", "годовых")
                + (o.tvlUSD.map { ", \(Fmt.usd(($0 / 1_000_000).rounded(1, .plain)))M " + say("deposited", "в депозитах") } ?? "")
        }.joined(separator: "\n"), note: note ?? "DeFi")
        options = rates.prefix(4).compactMap { o in o.how.map { ChatRouter.Option(title: "\(o.place) · \(Fmt.num(o.apy.rounded(2, .plain)))%", comment: o.note, command: $0) } }
        pin("result", say("Rates", "Ставки") + (asset.map { " · \($0)" } ?? ""), lines: rates.prefix(12).map { "\($0.place) (\($0.asset)) · \(Fmt.num($0.apy.rounded(2, .plain)))%" },
            actions: rates.prefix(4).compactMap { o in o.how.map { Board.Action(title: "\(o.place) · \(Fmt.num(o.apy.rounded(2, .plain)))%", command: $0) } },
            data: BlockData(rates: rates.prefix(8).map { .init(place: $0.place, asset: $0.asset, apy: $0.apy, how: $0.how, note: $0.note) }))
        changed()
    }

    /// «spread USDC across the best rates», «rebalance»: the plan first, as a card; the deposits only after Confirm.
    func preparePlan(_ command: WalletCommand, note: String?) async {
        do {
            let e = try executor()
            let (text, ready): (String, Bool)
            var route: (steps: [Board.Step], notes: [String])?
            // With nothing to do: nothing free is said in words; a plan with nowhere to go, or a rebalance with nothing
            // worth moving, is drawn with its reasons (the Mac's allocationPreview and RebalanceBoard).
            var idle: String?
            switch command {
            case .allocate(let symbol, let share, let limit, let without, let from):
                let r = try await e.allocate(symbol, maxShare: share ?? 50, limit: limit, without: without, from: from, execute: false)
                (text, ready) = (r.text, r.ready && r.preview.free > 0)
                route = allocationSteps(r.preview)
                if !ready, r.preview.free > 0 { idle = say("Nowhere to put it now — see the canvas.", "Сейчас некуда положить — смотри холст.", "Ahora no hay dónde ponerlo: mira el lienzo.") }
            case .rebalance(let share):
                let r = try await e.rebalance(maxShare: share ?? 50, execute: false)
                (text, ready) = (r.text, r.ready)
                route = rebalanceSteps(r.plan, maxShare: share ?? 50)
                if !ready {
                    idle = r.plan.moves.isEmpty && r.plan.skipped.isEmpty && r.plan.notes.first?.hasPrefix("Nothing is deposited") == true
                        ? say("Nothing is deposited yet: «spread USDC across the best rates» puts free USDC to work.",
                              "Депозитов пока нет: «разложи USDC по лучшим ставкам» пустит свободные USDC в работу.",
                              "Aún no hay depósitos: «reparte USDC en las mejores tasas» pone a trabajar los USDC libres.")
                        : say("Nothing worth moving now — the reasons are on the canvas.", "Сейчас нечего перекладывать — причины на холсте.",
                              "Ahora no vale la pena mover nada: los motivos están en el lienzo.")
                }
            default: return
            }
            guard ready else {
                // A scheduled spread with nothing free says nothing (the Mac's quiet run).
                quietCheck = true
                if let idle {
                    pin(Board(kind: "move", title: command.summary, lines: text.split(separator: "\n").map(String.init) + (route?.notes ?? []),
                              steps: route?.steps.isEmpty == false ? route?.steps : nil, state: "nothing"))
                    reply(idle, note: note ?? "DeFi")
                } else {
                    reply(text, note: note ?? "DeFi")
                }
                return
            }
            show(.wallet(command), title: command.summary, lines: text.split(separator: "\n").map(String.init))
            // Drawn as the Mac's route cards (Blocks.swift): from each network, the bridges, into each place.
            if let route, !route.steps.isEmpty { updateBoard(card?.id) { $0.steps = route.steps; $0.lines += route.notes } }
            reply(say("The plan is on the card. Press Confirm to make it, or ask about it.", "План на карточке. Нажми «Подтвердить» или спроси о нём.",
                      "El plan está en la tarjeta. Pulsa Confirm para hacerlo o pregunta por él."), note: note ?? "DeFi")
        } catch {
            reply(say("Failed: ", "Не вышло: ") + error.localizedDescription, note: note)
        }
    }

    func prepareClaim(_ command: WalletCommand, note: String?) async {
        guard let me = evmAddress else { reply(say("This wallet has no EVM address to claim rewards to.", "У этого кошелька нет EVM-адреса, куда забрать награды."), note: note); return }
        do {
            let owed = try await Merkl.rewards(user: me)
            guard !owed.isEmpty else {
                reply(say("Nothing to claim: Merkl owes this wallet nothing on Base, Arbitrum, Ethereum or Polygon right now.",
                          "Забирать нечего: Merkl ничего не должен этому кошельку ни на Base, ни на Arbitrum, Ethereum или Polygon."), note: note)
                return
            }
            let worth = owed.compactMap(\.worthUSD).reduce(0, +)
            let shown = owed.map { "\(Fmt.num($0.claimable, maxFraction: 6)) \($0.symbol)" }
            if case .sweepRewards(let min, _) = command, worth < Merkl.sweepFloor(min) {
                reply(say("Merkl owes \(shown.joined(separator: ", ")), ≈ \(Fmt.usd(worth)): under the \(Fmt.usd(Merkl.sweepFloor(min))) floor, so nothing is swept yet — the gas would eat it.",
                          "Merkl должен \(shown.joined(separator: ", ")), ≈ \(Fmt.usd(worth)): меньше порога \(Fmt.usd(Merkl.sweepFloor(min))), поэтому пока ничего не забираю — газ съест."), note: note)
                return
            }
            show(.wallet(command), title: command.summary, lines: shown + (worth > 0 ? ["≈ \(Fmt.usd(worth))"] : []))
            reply(say("Merkl owes this wallet \(shown.joined(separator: ", ")). Press Confirm to claim it.", "Merkl должен кошельку \(shown.joined(separator: ", ")). Нажми «Подтвердить», чтобы забрать."), note: note ?? "Merkl")
        } catch {
            reply(say("Couldn't ask Merkl: ", "Не достучался до Merkl: ") + error.localizedDescription, note: note)
        }
    }

    /// Coins from a network without a wallet here (Bitcoin, TRON, Sui…): NEAR Intents' deposit address, for an hour;
    /// what lands is USDC on Solana. Nothing is signed.
    func receiveAcross(_ network: String, _ token: String, _ amount: Decimal, note: String?) async {
        let title = AddressCheck.Network(rawValue: network)?.title ?? network
        do {
            guard let account = try await SignerLink.shared.send(.nearAddress).nearAddress else { throw ExecutionError("The signer has no NEAR account for a refund.") }
            let r = try await executor().receiveAcross(token, amount: amount, network: network, nearAccount: account)
            pin("result", say("Receive \(token.uppercased()) from \(title)", "Получить \(token.uppercased()) из \(title)"),
                lines: [say("Send \(Fmt.num(amount)) \(token.uppercased()) on \(title) within an hour to:", "Отправь \(Fmt.num(amount)) \(token.uppercased()) в сети \(title) в течение часа на:"), r.deposit,
                        say("≈ \(r.quote.amountOutFormatted) USDC lands on Solana", "На Solana придёт ≈ \(r.quote.amountOutFormatted) USDC")],
                data: BlockData(receive: .init(network: network, title: title, token: token.uppercased(), amount: amount, address: r.deposit, minimum: r.minimum,
                                               until: r.quote.deadline.flatMap { try? Date($0, strategy: .iso8601) }, arrives: r.quote.amountOutFormatted,
                                               state: say("waiting for your deposit", "жду твой перевод"))))
            if let id = boards.first?.id { Task { await followAcross(id, address: r.deposit) } }
            reply(say("Send \(Fmt.num(amount)) \(token.uppercased()) on \(title) within an hour to:\n\(r.deposit)\n≈ \(r.quote.amountOutFormatted) USDC lands on Solana (at least \(Fmt.num(r.minimum)) \(token.uppercased())).",
                      "Отправь \(Fmt.num(amount)) \(token.uppercased()) в сети \(title) в течение часа на:\n\(r.deposit)\nНа Solana придёт ≈ \(r.quote.amountOutFormatted) USDC (не меньше \(Fmt.num(r.minimum)) \(token.uppercased()))."),
                  note: note ?? "NEAR Intents · \(title)")
        } catch {
            reply(error.localizedDescription, note: note)
        }
    }

    /// Who may still spend the wallet's tokens, on each network; each one can be revoked from its card.
    func showApprovals(note: String?) async {
        guard let e = try? executor() else { return }
        let (list, unread) = await e.approvals()
        guard !list.isEmpty else {
            reply(say("No token approvals: nothing may spend this wallet's tokens.", "Разрешений нет: никто не может тратить токены этого кошелька.")
                  + (unread.isEmpty ? "" : say(" Not read: ", " Не прочитано: ") + unread.joined(separator: ", ") + "."), note: note)
            return
        }
        reply(say("Token approvals:\n", "Разрешения на токены:\n") + list.prefix(20).map { a in
            "\(a.symbol) on \(a.chain) → \(a.spenderName ?? a.spender)" + (a.unlimited ? say(" · unlimited", " · без лимита") : "") + (a.permit2 ? " · Permit2" : "")
        }.joined(separator: "\n"), note: note ?? say("approvals", "разрешения"))
        pin("result", say("Approvals", "Разрешения"), lines: list.prefix(30).map { "\($0.symbol) on \($0.chain) → \($0.spenderName ?? $0.spender)" + ($0.unlimited ? " · unlimited" : "") },
            actions: list.prefix(12).map { a in Board.Action(title: say("Revoke \(a.symbol) · \(a.chain)", "Отозвать \(a.symbol) · \(a.chain)"),
                                                             command: WalletCommand.revokeApproval(a.chain, token: a.token, spender: a.spender, permit2: a.permit2).summary) },
            data: BlockData(approvals: approvalsCard(list, unread: unread)))
        options = list.prefix(6).map { a in
            let c = WalletCommand.revokeApproval(a.chain, token: a.token, spender: a.spender, permit2: a.permit2)
            return ChatRouter.Option(title: say("Revoke \(a.symbol) for \(a.spenderName ?? String(a.spender.prefix(10)))", "Отозвать \(a.symbol) у \(a.spenderName ?? String(a.spender.prefix(10)))"),
                                     comment: a.unlimited ? say("unlimited", "без лимита") : "", command: c.summary, recommended: a.unlimited && a.spenderName == nil)
        }
        changed()
    }

    func showJupiterDCA(note: String?) async {
        guard let orders = try? await executor().jupiterDCAOrders() else { reply(say("Jupiter didn't answer.", "Jupiter не ответил."), note: note); return }
        if !orders.isEmpty {
            let symbol: (String) -> String = { mint in mint == SpotAsset.usdc.mint ? "USDC" : SpotAsset.all.first { $0.mint == mint }?.symbol ?? String(mint.prefix(6)) + "…" }
            pin("result", "Jupiter DCA", lines: orders.prefix(10).map { o in
                "\(Fmt.num(o.used, maxFraction: 2)) / \(Fmt.num(o.deposited, maxFraction: 2)) \(symbol(o.inputMint)) → \(Fmt.num(o.received, maxFraction: 6)) \(symbol(o.outputMint)) · "
                    + say("\(o.fills) swaps, every \(o.every / 3600) h", "\(o.fills) обменов, раз в \(o.every / 3600) ч")
            }, data: BlockData(dca: dcaRows(Array(orders.prefix(10)))))
        }
        reply(orders.isEmpty ? say("No Jupiter DCA is running.", "Jupiter DCA сейчас не запущен.")
              : say("Running Jupiter DCA: \(orders.count).", "Запущено Jupiter DCA: \(orders.count)."), note: note ?? "Jupiter DCA")
    }
}

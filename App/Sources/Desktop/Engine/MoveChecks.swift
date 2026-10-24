import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import TxGuard
import WalletCore

/// What the Mac checks and says before a move's card (Canvases.swift, Execution.swift, Workspace.swift): a move that can't
/// go through with what the wallet holds is said at once, with the ways on as cards; a card shows its route step by step,
/// each with the balance it changes; a recurring buy is offered two ways; a waiting card waits on its canvas.
extension Engine {
    // MARK: What the wallet holds, as last read

    /// A network's rows on screen; nil before the first read (a balance not loaded yet leaves it to the move's checks).
    private func rowsRead(_ net: String) -> [PortfolioView.Row]? {
        guard portfolio.updatedAt != nil else { return nil }
        return portfolio.networks.first { $0.key == net }?.rows ?? []
    }

    /// USDC free at a place (USDG on Robinhood Chain, Hyperliquid's withdrawable), the Mac's usdc(on:).
    func usdcFree(on net: String) -> Decimal? {
        if net == "hyperliquid", let hl = placeFacts.hyperliquid { return hl.withdrawable }
        let dollar = net == "robinhood" ? "USDG" : "USDC"
        return rowsRead(net).map { $0.filter { $0.section == "tokens" && $0.symbol == dollar && $0.contract == nil }.reduce(0) { $0 + $1.value } }
    }

    /// A token held at a place, by its symbol or contract; nil when the place isn't read.
    func heldThere(_ symbol: String, on net: String) -> Decimal? {
        if ["USDC", "USDG"].contains(symbol.uppercased()) { return usdcFree(on: net) }
        return rowsRead(net).map { rows in
            rows.filter { $0.section == "tokens" && ($0.symbol.uppercased() == symbol.uppercased() || $0.contract?.lowercased() == symbol.lowercased()) }
                .reduce(0) { $0 + $1.value }
        }
    }

    /// The network's own coin under the floor a lending card reads, as the Mac's: Base's aaveRoute by 0.000005 ETH, the
    /// other networks' lendingRoute by the network's gas floor. Unread counts as none.
    func gasLow(on net: String) -> Bool {
        let gas = rowsRead(net)?.filter { $0.contract == "native" }.reduce(0) { $0 + $1.value } ?? 0
        return gas < (net == "base" ? Decimal(string: "0.000005")! : EVMNetwork.named(net).map { Decimal($0.gasFloor) / 1_000_000_000_000_000_000 } ?? 0)
    }

    /// What a deposit row holds: «Jupiter Lend», «Kamino Lend», «Kamino» (a vault), «Aave», «Morpho».
    private func depositThere(_ names: [String], on net: String) -> Decimal {
        rowsRead(net)?.filter { $0.section == "defi" && names.contains($0.symbol) }.reduce(0) { $0 + $1.value } ?? 0
    }

    private var hlpEquity: Decimal { placeFacts.hyperliquid?.hlpEquity ?? 0 }

    /// The Kamino Earn vault «withdraw … from kamino» takes from when Lend holds nothing: its row's name and «0.5 SOL».
    private func kaminoVault(_ symbol: String) -> (name: String, amount: String, tokens: Decimal)? {
        guard depositThere(["Kamino Lend"], on: "solana") == 0,
              let r = rowsRead("solana")?.first(where: { $0.section == "defi" && $0.symbol == "Kamino" && $0.amount.hasSuffix(" " + symbol.uppercased()) }),
              let tokens = Decimal(string: String(r.amount.split(separator: " ").first ?? "").replacingOccurrences(of: ",", with: "")) else { return nil }
        return (String((r.note ?? "Kamino").split(separator: "·").first ?? "Kamino").trimmingCharacters(in: .whitespaces), r.amount, tokens)
    }

    /// One of this computer's wallets, by its Solana, EVM or NEAR address.
    func isOwnWallet(_ address: String) -> Bool {
        let a = address.lowercased()
        return (status?.wallets ?? []).contains { [$0.solana, $0.evm, $0.near].contains { $0?.lowercased() == a } }
    }

    // MARK: Sweep dust (5)

    /// What «sweep dust» sells: the verified tokens on Base, Arbitrum and Polygon worth from $0.20 up to `under` (nil = $5),
    /// the most first; the network's own coin stays, it's gas (the Mac's dustTokens).
    func dustTokens(under: Decimal?) -> [Executor.Dust] { Self.dust(in: portfolio.networks, under: under) }

    nonisolated static func dust(in networks: [PortfolioView.Network], under: Decimal?) -> [Executor.Dust] {
        networks.filter { Executor.dustNetworks.contains($0.key) }.flatMap { n in
            n.rows.compactMap { r -> Executor.Dust? in
                guard r.section == "tokens", r.verified == true, let c = r.contract, c != "native", let u = r.usd,
                      u >= Decimal(string: "0.2")!, u < (under ?? 5) else { return nil }
                return Executor.Dust(chain: n.key, contract: c, symbol: r.symbol, usd: u)
            }
        }.sorted { $0.usd > $1.usd }
    }

    // MARK: Before a card (7, 8)

    /// A move that can't go through with what the wallet holds, said at once instead of a card whose Confirm could only
    /// fail (the Mac's refusalBeforeCard).
    func refusalBeforeCard(_ command: WalletCommand) -> String? {
        if let why = fundingShortfall(command) { return why }
        func lacks(_ have: Decimal?, _ asked: Decimal?, _ place: String, _ unit: String = "USDC") -> String? {
            guard let have else { return nil }
            if have <= 0 {
                return say("\(place) has \(Fmt.num(have)) \(unit), so there’s nothing to move from there — nothing was prepared.",
                           "На \(place) \(Fmt.num(have)) \(unit) — оттуда нечего переводить, ничего не готовлю.")
            }
            if let asked, asked > have {
                return say("Only \(Fmt.num(have)) \(unit) is on \(place), less than \(Fmt.num(asked)). Say an amount up to that.",
                           "На \(place) только \(Fmt.num(have)) \(unit) — меньше, чем \(Fmt.num(asked)). Назови сумму не больше.")
            }
            return nil
        }
        let solanaUSDC = usdcFree(on: "solana")
        switch command {
        case .withdrawToSolana(let asked): return lacks(usdcFree(on: "hyperliquid"), asked, "Hyperliquid")
        case .kamino(deposit: false, symbol: "USDC", amount: let asked):
            return lacks(rowsRead("solana") == nil ? nil : depositThere(["Kamino Lend", "Kamino"], on: "solana"), asked, "Kamino")
        case .lendWithdraw("USDC", let asked): return lacks(rowsRead("solana") == nil ? nil : depositThere(["Jupiter Lend"], on: "solana"), asked, "Jupiter Lend")
        case .baseToSolana(let asked): return lacks(usdcFree(on: "base"), asked, "Base")
        case .arbitrumToSolana(let asked): return lacks(usdcFree(on: "arbitrum"), asked, "Arbitrum")
        case .robinhoodToSolana(let asked): return lacks(usdcFree(on: "robinhood"), asked, "Robinhood Chain", "USDG")
        case .evmToSolana(let net, let asked), .bridge(from: let net, to: _, let asked):
            return lacks(usdcFree(on: net), asked, WalletCommand.placeName(net), net == "robinhood" ? "USDG" : "USDC")
        case .usdt(let net, _, let asked, _): return lacks(heldThere("USDT", on: net), asked, WalletCommand.placeName(net), "USDT")
        case .buyOn(_, _, let asked?, let from?) where from != "solana":
            return lacks(usdcFree(on: from), asked, WalletCommand.placeName(from), from == "robinhood" ? "USDG" : "USDC")
        case .hlp(deposit: false, amount: let asked):
            if let until = placeFacts.hyperliquid?.hlpLockedUntil, until > Date(), hlpEquity > 0 {
                return say("HLP is locked until \(Fmt.date(until)): Hyperliquid lets deposits out 4 days after they go in.",
                           "HLP заблокирован до \(Fmt.date(until)): Hyperliquid выпускает депозит через 4 дня после внесения.")
            }
            return lacks(placeFacts.hyperliquid == nil ? nil : hlpEquity, asked, "HLP")
        case .lendDeposit("USDC", let asked?), .kamino(deposit: true, symbol: "USDC", amount: let asked?): return lacks(solanaUSDC, asked, "Solana")
        // All of it goes in at once, and Jupiter starts at 100: «say less» would only meet its minimum.
        case .jupiterDCA(_, let asked?, _, _):
            if let have = solanaUSDC, asked > have {
                return have < WalletCommand.jupiterDCAMinimum
                    ? say("Only \(Fmt.num(have)) USDC is on Solana, and Jupiter's DCA starts at 100 — all of it goes in at once. Top up Solana first.",
                          "На Solana только \(Fmt.num(have)) USDC, а DCA у Jupiter — от 100, и вся сумма вносится сразу. Сначала пополни Solana.")
                    : say("Only \(Fmt.num(have)) USDC is on Solana, and the DCA puts all \(Fmt.num(asked)) in at once. Say up to \(Fmt.num(have)).",
                          "На Solana только \(Fmt.num(have)) USDC, а DCA вносит все \(Fmt.num(asked)) сразу. Назови сумму до \(Fmt.num(have)).")
            }
            if let sol = heldThere("SOL", on: "solana"), sol < Decimal(string: "0.007")! {
                return say("The DCA's order account takes ≈ 0.006 SOL of rent, back when it ends; Solana has \(Fmt.num(sol, maxFraction: 4)) SOL.",
                           "Аккаунт ордера DCA берёт ≈ 0.006 SOL залога (вернётся в конце), а на Solana \(Fmt.num(sol, maxFraction: 4)) SOL.")
            }
            return nil
        // «move 100 USDC to arbitrum» read as from Solana, where little lay: said before the card, with where the money is.
        case .solanaTo(_, let asked?):
            guard let why = lacks(solanaUSDC, asked, "Solana") else { return nil }
            guard let net = EVMNetwork.funding(asked, held: { self.usdcFree(on: $0) }) else { return why }
            return why + say(" \(net.name) has \(Fmt.num(usdcFree(on: net.key) ?? 0)) USDC: say «from \(net.name)» to move it from there.",
                             " На \(net.name) — \(Fmt.num(usdcFree(on: net.key) ?? 0)) USDC: скажи «с \(net.name)», чтобы перевести оттуда.")
        case .lendDeposit("SOL", let asked?), .kamino(deposit: true, symbol: "SOL", amount: let asked?), .stake(let asked?):
            return lacks(heldThere("SOL", on: "solana"), asked, "Solana", "SOL")
        case .sell(let a, let asked) where a.resolved:
            guard let have = heldThere(a.symbol == "SOL" ? "SOL" : a.mint, on: "solana") else { return nil }
            if have <= 0 { return say("There’s no \(a.symbol) on Solana to sell — nothing was prepared.", "На Solana нет \(a.symbol) — продавать нечего, ничего не готовлю.") }
            return lacks(have, asked, "Solana", a.symbol)
        case .sweepDust(let under):
            guard portfolio.updatedAt != nil, dustTokens(under: under).isEmpty else { return nil }
            return say("No verified token worth $0.20 to \(Fmt.usd(under ?? 5)) on Base, Arbitrum or Polygon — no dust to sweep, nothing was prepared.",
                       "На Base, Arbitrum и Polygon нет проверенных токенов от $0.20 до \(Fmt.usd(under ?? 5)) — пыли нет, ничего не готовлю.")
        case .sellNear(let asked, _):
            // What a sale keeps on the account while it is sent: storage and prepaid gas.
            guard let near = heldThere("NEAR", on: "near") else { return nil }
            let sellable = max(0, near - Decimal(string: "0.09")!)
            let amount = asked ?? sellable
            guard amount < Decimal(string: "0.05")! || amount > sellable else { return nil }
            return say("You have \(Fmt.num(near, maxFraction: 4)) NEAR. A sale needs at least 0.05 NEAR, and ≈ 0.09 NEAR stays on the account for storage and gas while it is sent, so \(sellable < Decimal(string: "0.05")! ? "there is nothing to sell now" : "at most \(Fmt.num(sellable, maxFraction: 4)) NEAR can be sold").",
                       "У тебя \(Fmt.num(near, maxFraction: 4)) NEAR. Продажа — от 0.05 NEAR, и ≈ 0.09 NEAR остаётся на аккаунте под хранение и газ, пока она идёт, так что \(sellable < Decimal(string: "0.05")! ? "сейчас продавать нечего" : "можно продать не больше \(Fmt.num(sellable, maxFraction: 4)) NEAR").")
        default:
            return nil
        }
    }

    /// What a deposit or a buy spends and where, and how much of it that place lacks; nil when it has it, or the move brings
    /// its own money (a source the user named).
    private func fundingNeed(_ c: WalletCommand) -> (short: Decimal, net: String)? {
        let amount: Decimal, net: String
        switch c {
        case .aave(true, let a?, let n), .morpho(true, let a?, let n), .buyOn(let n, _, let a?, nil): (amount, net) = (a, n)
        case .buyStock(_, let a?): (amount, net) = (a, "robinhood")
        case .lendDeposit("USDC", let a?), .kamino(true, "USDC", let a?), .buy(_, let a?), .jupiterDCA(_, let a?, _, _): (amount, net) = (a, "solana")
        default: return nil
        }
        guard let have = usdcFree(on: net) else { return nil }
        let short = amount - have
        return short > 0 ? (short, net) : nil
    }

    /// One place pays all of it inside the move itself, one card: Solana for a move elsewhere, or one EVM network sending
    /// straight (EVMNetwork.source, as the Executor signs it).
    private func oneSource(_ short: Decimal, _ net: String) -> Bool {
        let solana = usdcFree(on: "solana") ?? 0
        return net != "solana" && (solana >= short + Decimal(string: "0.1")!
            || EVMNetwork.source(arrive: short, onSolana: solana, target: net, held: { self.usdcFree(on: $0) }) != nil)
    }

    private func fundingLegs(_ short: Decimal, _ net: String) -> [String]? {
        let evm = EVMNetwork.searched.map { FundingPlan.Place(key: $0.key, name: $0.name, usdc: usdcFree(on: $0.key) ?? 0) }
        return FundingPlan.legs(short: short, target: net, targetName: WalletCommand.placeName(net), solana: usdcFree(on: "solana") ?? 0, evm: evm,
                                hyperliquid: usdcFree(on: "hyperliquid") ?? 0, near: heldThere("NEAR", on: "near") ?? 0, nearPrice: markets["NEAR"]?.markPx)
    }

    /// All the wallet's free money together can't pay the move: said before a card that would fail.
    func fundingShortfall(_ c: WalletCommand) -> String? {
        guard let (short, net) = fundingNeed(c), !oneSource(short, net), fundingLegs(short, net) == nil else { return nil }
        var free: [(String, Decimal)] = [("Solana", usdcFree(on: "solana") ?? 0)]
        free += EVMNetwork.searched.compactMap { n in usdcFree(on: n.key).flatMap { $0 > 0 ? (n.name, $0) : nil } }
        free.append(("Hyperliquid", usdcFree(on: "hyperliquid") ?? 0))
        if let price = markets["NEAR"]?.markPx, let near = heldThere("NEAR", on: "near"), near > 0 { free.append(("NEAR", near * price)) }
        let listed = free.filter { $0.1 >= Decimal(string: "0.01")! }.map { "\($0.0) \(Fmt.usd($0.1))" }.joined(separator: ", ")
        return say("This needs \(Fmt.num(short)) USDC more on \(WalletCommand.placeName(net)), and all the free money together isn't enough: \(listed). Top up, or say a smaller amount.",
                   "Нужно ещё \(Fmt.num(short)) USDC на \(WalletCommand.placeName(net)), а всех свободных денег не хватает: \(listed). Пополни или назови сумму меньше.")
    }

    /// A move asking for more than its place holds: what there is in a line, and the ways on as cards — bring the rest from
    /// elsewhere first, take all there is, or top up (the Mac's shortChoices). nil when there's enough, the move pays it
    /// within itself, or the holding isn't known.
    func shortChoices(_ c: WalletCommand) -> (line: String, options: [ChatRouter.Option])? {
        if let (short, net) = fundingNeed(c), !oneSource(short, net) {
            let have = usdcFree(on: net) ?? 0, place = WalletCommand.placeName(net)
            var options: [ChatRouter.Option] = []
            if let legs = fundingLegs(short, net), !legs.isEmpty, WalletCommand.parse(c.summary) == c {
                options.append(ChatRouter.Option(title: say("Bring \(Fmt.num(short)) USDC to \(place) first", "Сначала довезти \(Fmt.num(short)) USDC на \(place)"),
                                                 comment: (legs + [c.summary]).joined(separator: " → "), command: (legs + [c.summary]).joined(separator: " then "),
                                                 recommended: true))
            }
            options += allThere(c, have: have, unit: "USDC", recommended: options.isEmpty)
            options.append(topUpFirst(place))
            return (say("On \(place) there's \(Fmt.num(have)) USDC, and this asks \(Fmt.num(have + short)). Pick a way below.",
                        "На \(place) \(Fmt.num(have)) USDC, а нужно \(Fmt.num(have + short)). Выбери ниже, как быть."), options)
        }
        // «send all USDT on solana to …» with none there: said at once, not a card that fails at the signature.
        if case .sendToken(_, let token, nil, let net?) = c, heldThere(token, on: net) == 0 {
            let place = WalletCommand.placeName(net)
            return (say("There's no \(token.uppercased()) on \(place) to send.", "На \(place) нет \(token.uppercased()) — отправлять нечего."), [topUpFirst(place)])
        }
        let asked: (symbol: String, amount: Decimal, net: String)
        switch c {
        case .swapOn(let net, let from, _, let x?): asked = (from, x, net)
        case .sellOn(let net, let token, let x?, _): asked = (token, x, net)
        case .crossSwap(let from, let on, _, _, let x?): asked = (from, x, on)
        case .sell(let a, let x?): asked = (a.symbol == "SOL" || a.mint.isEmpty ? a.symbol : a.mint, x, "solana")
        case .sendToken(_, let token, let x?, let net?): asked = (token, x, net)
        default: return nil
        }
        guard let have = heldThere(asked.symbol, on: asked.net), asked.amount > have else { return nil }
        let place = WalletCommand.placeName(asked.net)
        let symbol = asked.symbol.count > 12 ? (portfolio.networks.first { $0.key == asked.net }?.rows.first { $0.contract == asked.symbol }?.symbol ?? asked.symbol) : asked.symbol.uppercased()
        var bring: [ChatRouter.Option] = []
        if symbol == "USDC", let legs = fundingLegs(asked.amount - have, asked.net), !legs.isEmpty {
            bring = [ChatRouter.Option(title: say("Bring \(Fmt.num(asked.amount - have)) USDC to \(place) first", "Сначала довезти \(Fmt.num(asked.amount - have)) USDC на \(place)"),
                                       comment: (legs + [c.summary]).joined(separator: " → "), command: (legs + [c.summary]).joined(separator: " then "), recommended: true)]
        }
        // Dollars short on Solana where its USDC can buy them: the buy first, then the same move.
        let missing = (asked.amount - have).rounded(2, .up)
        if symbol != "USDC", Stablecoins.isDollar(symbol), asked.net == "solana", (usdcFree(on: "solana") ?? 0) >= missing {
            let buy = "buy \(symbol) on solana for \(Fmt.num(missing)) USDC"
            bring = [ChatRouter.Option(title: say("Buy \(Fmt.num(missing)) \(symbol) for USDC first", "Сначала купить \(Fmt.num(missing)) \(symbol) за USDC"),
                                       comment: buy + " → " + c.summary, command: buy + " then " + c.summary, recommended: true)]
        }
        return (say("On \(place) there's \(Fmt.num(have, maxFraction: 6)) \(symbol), and this asks \(Fmt.num(asked.amount, maxFraction: 6)). Pick a way below.",
                    "На \(place) \(Fmt.num(have, maxFraction: 6)) \(symbol), а нужно \(Fmt.num(asked.amount, maxFraction: 6)). Выбери ниже, как быть."),
                bring + allThere(c, have: have, unit: symbol, recommended: bring.isEmpty) + [topUpFirst(place)])
    }

    /// «All there is»: the same move for what the place holds, when that's worth a move.
    private func allThere(_ c: WalletCommand, have: Decimal, unit: String, recommended: Bool) -> [ChatRouter.Option] {
        let amount = unit == "USDC" ? have.rounded(2, .down) : have
        guard amount > 0, unit != "USDC" || amount >= Decimal(string: "0.5")! else { return [] }
        let all = c.with(amount: amount)
        guard all != c else { return [] }
        return [ChatRouter.Option(title: say("All there is: \(Fmt.num(amount, maxFraction: 6)) \(unit)", "Всё, что есть: \(Fmt.num(amount, maxFraction: 6)) \(unit)"),
                                  comment: all.summary, command: all.summary, recommended: recommended)]
    }

    private func topUpFirst(_ place: String) -> ChatRouter.Option {
        ChatRouter.Option(title: say("Top up first", "Сначала пополнить"),
                          comment: say("Your addresses on the canvas; send the rest to \(place) and ask again.", "Адреса на холсте: пришли недостающее на \(place) и попроси снова."),
                          command: "top up", recommended: false)
    }

    /// Everything handleWallet checks before a card, in the Mac's order: true when it answered and nothing else is drawn.
    func checkedBeforeCard(_ command: inout WalletCommand, note: String?) -> Bool {
        // Jupiter's DCA: the swaps when none were named, and Jupiter's own minimums before a card that would fail.
        if case .jupiterDCA(let a, let u?, let o, let e) = command {
            let orders = o ?? WalletCommand.jupiterDCAOrders(for: u)
            if let problem = WalletCommand.jupiterDCAProblem(usdc: u, orders: orders) { reply(problem, note: note); return true }
            command = .jupiterDCA(a, usdc: u, orders: orders, every: e)
        }
        if let short = shortChoices(command) {
            reply(short.line, note: note)
            options = short.options
            return true
        }
        if let why = refusalBeforeCard(command) {
            reply(why, note: note)
            noteRefusal(command.summary, why, note: note)
            return true
        }
        return false
    }

    /// A move refused before its card: once the message is through, the model works out why from the balances and offers
    /// the way that works, as the Mac's noteFailure → explainFailure. Not inside a request the model carries on (it hears
    /// it there), a scheduled run or a run on the automation wallet.
    func noteRefusal(_ what: String, _ why: String, note: String?) {
        guard agentGoal == nil, TaskRun.automation == nil, note != "schedule" else { return }
        Task { await explainOnceFree(what, why) }
    }

    /// A failure caught as an error. A stop by the guard, the signer or a scam check (GuardStop) and a costly move with its
    /// own «Send anyway» (CostlyMove) get nothing offered around them (the Mac's noteFailure, a3f7edb).
    func noteRefusal(_ what: String, _ error: any Error, note: String?) {
        guard Self.explainsAfter(error) else { return }
        noteRefusal(what, error.localizedDescription, note: note)
    }

    /// Whether the model may offer a way after this failure: not after the guard, the signer or a scam check, and not
    /// after a costly move, which has its own «Send anyway» (the owner's rule, a3f7edb).
    nonisolated static func explainsAfter(_ error: any Error) -> Bool { !(error is GuardStop) && !(error is CostlyMove) }

    private func explainOnceFree(_ what: String, _ why: String) async {
        // ponytail: polls the turn's end (30 s at most); a hook at the end of send() if it ever misses one.
        for _ in 0..<150 where thinking { try? await Task.sleep(for: .milliseconds(200)) }
        guard !thinking, pending == nil, running == nil, agentGoal == nil else { return }
        await explainFailure(what, why: why)
    }

    /// The USDC (or USDT0) a move takes from an EVM network, read from its node now: the last balance read may be a minute
    /// old, and a move right after money landed was refused with «0 there» (the Mac's freshenSource). Only that row changes.
    func freshenSource(_ command: WalletCommand) async {
        guard TaskRun.automation == nil, let owner = evmAddress, !owner.isEmpty, portfolio.updatedAt != nil else { return }
        let place: String?, usdt: Bool
        switch command {
        case .evmToSolana(let n, _), .bridge(from: let n, to: _, _), .cctp(from: let n, to: _, _): (place, usdt) = (n, false)
        case .buyOn(_, _, _, let from?): (place, usdt) = (from, false)
        case .usdt(let from, _, _, _): (place, usdt) = (from, true)
        case .baseToSolana: (place, usdt) = ("base", false)
        case .arbitrumToSolana: (place, usdt) = ("arbitrum", false)
        case .robinhoodToSolana: (place, usdt) = ("robinhood", false)
        default: (place, usdt) = (nil, false)
        }
        guard let place, let net = EVMNetwork.named(place) else { return }
        let token = usdt ? Stablecoins.find("USDT", on: place)?.address : nil
        guard !usdt || token != nil else { return }
        let units: UInt64?
        if let token { units = try? await net.rpc.tokenBalance(token, owner: owner) } else { units = try? await net.usdcBalance(owner) }
        guard let units else { return }
        let amount = Decimal(units) / 1_000_000
        patchDollar(place, symbol: usdt ? "USDT" : (place == "robinhood" ? "USDG" : "USDC"), contract: usdt ? token : nil, amount: amount)
    }

    /// A network's dollar row set to what was just read, the rest of its card as it was.
    private func patchDollar(_ place: String, symbol: String, contract: String?, amount: Decimal) {
        var p = portfolio
        guard let i = p.networks.firstIndex(where: { $0.key == place }) ?? (amount > 0 ? p.networks.endIndex : nil) else { return }
        if i == p.networks.endIndex { p.networks.append(PortfolioView.Network(key: place, name: WalletCommand.placeName(place), usd: 0, rows: [])) }
        var rows = p.networks[i].rows
        let match = { (r: PortfolioView.Row) in r.section == "tokens" && r.symbol == symbol && (contract == nil ? r.contract == nil : r.contract?.lowercased() == contract?.lowercased()) }
        let old = rows.firstIndex(where: match)
        if let old {
            if amount > 0 { rows[old].value = amount; rows[old].amount = Fmt.num(amount); rows[old].usd = amount } else { rows.remove(at: old) }
        } else if amount > 0 {
            var r = Self.row(symbol, amount, usd: amount)
            r.contract = contract
            rows.insert(r, at: 0)
        }
        p.networks[i].rows = rows
        portfolio = p
    }

    // MARK: «How much?» (14)

    /// What there is to use, so the answer to «How much?» is easy.
    func amountHint(_ command: WalletCommand) -> String {
        switch command {
        case .lendDeposit("SOL", _), .kamino(_, "SOL", _), .stake:
            return heldThere("SOL", on: "solana").map { say(" You have \(Fmt.num($0, maxFraction: 4)) SOL on Solana — answer with a number, e.g. “0.01”.", " На Solana \(Fmt.num($0, maxFraction: 4)) SOL — ответь числом, например «0.01».") } ?? ""
        case .hlp: return usdcFree(on: "hyperliquid").map { say(" \(Fmt.usd($0)) is free on Hyperliquid — answer with a number, e.g. “5”.", " На Hyperliquid свободно \(Fmt.usd($0)) — ответь числом, например «5».") } ?? ""
        default: return usdcFree(on: "solana").map { say(" You have \(Fmt.num($0)) USDC on Solana — answer with a number, e.g. “1”.", " На Solana \(Fmt.num($0)) USDC — ответь числом, например «1».") } ?? ""
        }
    }

    /// The reply to «How much …?» while a command waits for its amount: true when it was the answer (or asked again).
    /// An amount in another unit («$5» to a SOL stake) or under the coin's smallest one is asked again in the question's
    /// unit, never read as that coin.
    func answeredAmount(_ text: String, declines: Bool) async -> Bool {
        guard let waiting = awaitingAmount else { return false }
        if declines { awaitingAmount = nil; reply(say("OK, not doing that.", "Хорошо, не делаю.")); return true }
        if let value = WalletCommand.amountAnswer(text, for: waiting) { awaitingAmount = nil; await handleWallet(waiting.with(amount: value)); return true }
        if let value = WalletCommand.amountAnswer(text) {
            let unit = waiting.amountAsset, least = unit == "SOL" ? "0.000000001" : "0.000001", example = unit == "SOL" ? "0.01" : "1"
            reply(value < Decimal(string: least)!
                  ? say("That's less than the smallest \(unit) amount (\(least)). How much \(unit)? Write a number in \(unit), for example “\(example)”.",
                        "Это меньше самой малой суммы \(unit) (\(least)). Сколько \(unit)? Напиши число в \(unit), например «\(example)».")
                  : say("The question is in \(unit). How much \(unit)? Write a number in \(unit), for example “\(example)”.",
                        "Вопрос в \(unit). Сколько \(unit)? Напиши число в \(unit), например «\(example)»."))
            return true
        }
        if ChatWords.confirms(text) { reply(say("How much? Just the number, for example “1”.", "Сколько? Просто число, например «1».")); return true }
        awaitingAmount = nil
        return false
    }

    // MARK: Addresses (9, 10, 11)

    /// The message with each address already saved in Addresses written as its name, when it asks to send: a saved address
    /// was confirmed on its card, so writing it is the same as writing the name. On several networks, the one the message
    /// names wins (the Mac's savedNames).
    func savedNames(in text: String) -> String {
        let found = InjectionHeuristics.findings(text)
        guard found.transferVerb else { return text }
        let want = AddressBookCommand.network(text.lowercased())
        return found.addresses.reduce(text) { out, address in
            let saved = book.filter { address.hasPrefix("0x") ? $0.address.lowercased() == address.lowercased() : $0.address == address }
            guard let entry = saved.first(where: { $0.network == want }) ?? saved.first else { return out }
            return out.replacingOccurrences(of: address, with: entry.name)
        }
    }

    /// The addresses money already went to from here: the first transfer to any other is a card with a caution.
    private var sentTo: Set<String> {
        get { Set(Store.load([String].self, "sent-to.json") ?? []) }
        set { Store.save(Array(newValue).sorted(), "sent-to.json") }
    }

    private static func key(_ address: String) -> String { address.hasPrefix("0x") ? address.lowercased() : address }

    /// «Send a small test amount first»: for someone else's address the first time — never for one of the user's own wallets.
    func firstSendWarning(_ address: String) -> String? {
        guard !sentTo.contains(Self.key(address)), !isOwnWallet(address) else { return nil }
        return say("First transfer to this address. Sending a small test amount first is the safe habit.",
                   "Первый перевод на этот адрес. Безопаснее сначала отправить небольшую пробную сумму.")
    }

    /// An entry saved for one EVM network, sent on another: the user's own wallet has it there too, an exchange may not.
    func otherNetworkWarning(saved: String, sent: String) -> String? {
        guard saved != sent else { return nil }
        let a = AddressCheck.Network(rawValue: saved)?.title ?? saved, b = AddressCheck.Network(rawValue: sent)?.title ?? sent
        return say("Saved for \(a), sent on \(b) to the same address. Your own wallet has it there too; an exchange's deposit address may not — check that it takes \(b).",
                   "Сохранён для \(a), отправка в \(b) на тот же адрес. Твой кошелёк там тоже есть, а адрес биржи может не принимать \(b) — проверь.")
    }

    /// The cautions on a move's card: whom a send goes to the first time, an XRP exchange address without its tag, and
    /// what the move itself risks (MoveRoutes.swift).
    func sendCautions(_ c: WalletCommand) -> [String] {
        let address: String?
        switch c {
        case .sendSol, .sendNear, .closeNear: address = (try? nativeRecipient(c))?.address
        case .sendToken(let name, let token, _, let net):
            // Through NEAR Intents (Bitcoin, XRP, TRON…): an exchange's XRP address without its tag loses the money.
            if let r = try? acrossRecipient(name, token: token, network: net) {
                if let tag = xrpTagWarning(r.entry) { return [tag] }
                address = r.entry.address
            } else {
                address = (try? tokenRecipient(name, network: net))?.entry.address
            }
        default: address = nil
        }
        return (address.flatMap(firstSendWarning).map { [$0] } ?? []) + moveCautions(c)
    }

    /// After a move that paid someone: the address is known from now on.
    func noteSent(_ done: Pending) {
        var to: [String] = []
        switch done {
        case .transfer(let e, _): to = [e.address]
        // The address the send went to, found as the send found it (a partial name, another wallet here).
        case .wallet(let c) where (try? nativeRecipient(c)) != nil: to = ((try? nativeRecipient(c)) ?? nil).map { [$0.address] } ?? []
        case .wallet(.sendToken(let name, let token, _, let net)):
            to = ((try? acrossRecipient(name, token: token, network: net))?.entry.address ?? (try? tokenRecipient(name, network: net))?.entry.address).map { [$0] } ?? []
        case .chain(let steps):
            to = steps.compactMap { step -> String? in
                switch step {
                case .transfer(let e, _): e.address
                case .wallet(let c): ((try? nativeRecipient(c)) ?? nil)?.address
                default: nil
                }
            }
        default: break
        }
        guard !to.isEmpty else { return }
        sentTo = sentTo.union(to.map(Self.key))
    }

    /// The guard's «money goes to saved addresses»: the save phrase goes into the chat field, ready to send.
    func offerSave(_ address: String, in text: String) {
        let net = AddressBookCommand.network(text.lowercased()) ?? (address.hasPrefix("0x") ? "base" : "solana")
        out.event("suggest", ["text": say("save \(address) as NAME on \(net)", "сохрани \(address) как ИМЯ на \(net)")])
        reply(say("Money goes only to saved addresses, so a swapped address can't receive it. Save this one first: the phrase is in the field — put a name instead of NAME (and another network if it isn't \(net)), send it, and check the address on the Windows Hello window. Then say “send <amount> USDC to <name>”.",
                  "Деньги уходят только на сохранённые адреса — так подменённый адрес ничего не получит. Сначала сохрани этот: фраза уже в поле — впиши имя вместо ИМЯ (и другую сеть, если не \(net)), отправь и сверь адрес в окне Windows Hello. Потом скажи «переведи <сумма> USDC <имя>»."),
              note: "guard: sends go to saved addresses")
    }

    // MARK: A recurring buy, two ways (12)

    /// Buying over time, the two ways as cards: Jupiter's DCA on-chain (a budget in rounds of at least 50, runs with the app
    /// closed) or the app's own recurring buys (the money stays in the wallet, no minimum, the app must run). True when
    /// offered; the pick comes back as its command and goes straight.
    func offerRecurringWays(_ command: WalletCommand, note: String?) -> Bool {
        guard case .schedule(let asset, let usdc, let days) = command else { return false }
        if dcaChoice == command || !Modules.current.allows(.jupiterDCA) { dcaChoice = nil; return false }
        let period = days == 1 ? say("day", "день") : days == 7 ? say("week", "неделю") : days == 30 ? say("month", "месяц") : say("\(days) days", "\(days) дн.")
        let (total, rounds) = usdc >= WalletCommand.jupiterDCAMinimum / 2 ? (usdc * 4, 4) : (WalletCommand.jupiterDCAMinimum, 2)
        let jupiter = WalletCommand.jupiterDCA(asset, usdc: total, orders: rounds, every: days * 86400)
        dcaChoice = command
        let each = Fmt.num(total / Decimal(rounds))
        reply(say("\(asset.symbol) over time can be bought two ways — pick one below.", "Покупать \(asset.symbol) регулярно можно двумя способами — выбери ниже."), note: note)
        options = [
            ChatRouter.Option(title: "Jupiter DCA: \(rounds) × \(each) USDC → \(asset.symbol)",
                              comment: say("On-chain: \(Fmt.num(total)) USDC goes in now and Jupiter buys every \(period), even with the app closed. From 100 USDC, 50 a round; 0.1% fee; cancel returns the rest.",
                                           "На блокчейне: \(Fmt.num(total)) USDC вносятся сразу, Jupiter покупает раз в \(period), даже при закрытом приложении. От 100 USDC, по 50 за раз; комиссия 0.1%; отмена вернёт остаток."),
                              command: jupiter.summary, recommended: usdc >= WalletCommand.jupiterDCAMinimum / 2),
            ChatRouter.Option(title: say("In the app: \(Fmt.num(usdc)) USDC → \(asset.symbol) every \(period)", "В приложении: \(Fmt.num(usdc)) USDC → \(asset.symbol) раз в \(period)"),
                              comment: say("The money stays in your wallet until each buy; no minimum. Buys while ihatecrypto runs, each signed with Windows Hello or by the automation wallet.",
                                           "Деньги лежат у тебя до каждой покупки; минимума нет. Покупает, пока ihatecrypto работает; каждая покупка — с Windows Hello или с кошелька автоматизации."),
                              command: command.summary, recommended: usdc < WalletCommand.jupiterDCAMinimum / 2),
        ]
        return true
    }

    // MARK: A new Hyperliquid account, two ways (13)

    static let openVia = "open hyperliquid via "

    /// A new Hyperliquid account opens with this trade: the user picks the bridge on two cards, with what each costs and how
    /// long it takes. Relay lands in seconds and carries Hyperliquid's one-time opening fee; NEAR Intents charges none but
    /// takes 2–4 min (the Mac's askOpeningRoute). True when asked: the plan waits for the pick.
    func askOpeningRoute(_ resolved: TradeIntent, _ evaluation: Evaluation, market: Market?, note: String?) async -> Bool {
        guard openVia1Click == nil, let evm = evmAddress else { return false }
        let amount: Bridge.Amount? = if let a = resolved.amountUSDC { .send(a) }
            else if let need = Plan.bridgeNeed(resolved, evaluation, hlWithdrawable: usdcFree(on: "hyperliquid"), mark: market?.markPx) {
                .arrive(Executor.bridgeArrive(need: need, margin: evaluation.sizing?.margin ?? 0, solanaUSDC: usdcFree(on: "solana")))
            } else { nil }
        guard let amount, (try? await Hyperliquid.isNewAccount(evm)) == true, let q = try? await Bridge.dryQuote(amount, newAccount: true),
              q.opening, q.alternativeFee != nil, q.amountIn <= (usdcFree(on: "solana") ?? 0) else { return false }
        let fee = q.amountIn - q.reserve - q.amountOut, other = q.alternativeFee ?? 0
        let (relay, near) = q.provider == "Relay" ? (fee, other) : (other, fee)
        reply(say("Your Hyperliquid account opens with this trade. How should the USDC come from Solana?",
                  "Аккаунт Hyperliquid откроется этой сделкой. Как довезти USDC с Solana?"), note: note)
        options = [
            ChatRouter.Option(title: say("Open via Relay", "Открыть через Relay"),
                              comment: say("seconds · ≈ \(Fmt.usd(relay)), incl. Hyperliquid's one-time opening fee", "секунды · ≈ \(Fmt.usd(relay)), включая разовую плату Hyperliquid за открытие"),
                              command: Self.openVia + "relay"),
            ChatRouter.Option(title: say("Open via NEAR Intents", "Открыть через NEAR Intents"),
                              comment: say("2–4 min · ≈ \(Fmt.usd(near)), no opening fee; the entry waits for the price to hold", "2–4 мин · ≈ \(Fmt.usd(near)), без платы за открытие; вход — если цена устоит"),
                              command: Self.openVia + "oneclick"),
        ]
        return true
    }

    /// The card picked: the plan is prepared with that way.
    func openingRouteCommand(_ text: String) async -> Bool {
        guard text.lowercased().hasPrefix(Self.openVia) else { return false }
        openVia1Click = text.lowercased().hasSuffix("oneclick")
        if intent != TradeIntent() { await plan(intent) }
        return true
    }

    // MARK: A waiting card on a canvas left (15)

    /// Leaving a canvas: its waiting card waits there, a quarter of an hour (the Mac's parkedPending). A site's request
    /// isn't a card: it waits in its own block until answered (Consent.swift).
    func parkPending() -> Bool {
        guard let pending, let card else { return false }
        parked[canvasID] = (pending, card, intent, Date())
        self.pending = nil
        self.card = nil
        return true
    }

    /// Back on a canvas whose move was waiting for Confirm: it waits again; after a quarter of an hour it's said gone.
    func unparkPending() {
        guard let p = parked.removeValue(forKey: canvasID) else { return }
        guard Date().timeIntervalSince(p.at) < 15 * 60 else {
            updateBoard(p.card.id) { $0.state = "cancelled"; $0.lines.append(say("Waited a quarter of an hour: nothing was signed.", "Ждала четверть часа: ничего не подписано.")) }
            if chainState.afterCall?.board == p.card.id { chainState.afterCall = nil }
            return
        }
        pending = p.pending
        card = p.card
        intent = p.intent
        if case .wallet(let c) = p.pending { Task { await quoteCard(c, board: p.card.id) } }
    }

    // MARK: The route, step by step (6)

    /// A balance a step changes, read now: `symbol` on `place` and what `delta` makes of it (nil: all of it leaves).
    func moved(_ symbol: String, on place: String, by delta: Decimal?, approx: Bool = false) -> Board.Money? {
        guard let was = heldThere(symbol, on: place) else { return nil }
        let after = max(0, was + (delta ?? -was))
        let shown = symbol.count > 12 ? (portfolio.networks.first { $0.key == place }?.rows.first { $0.contract == symbol }?.symbol ?? symbol) : symbol
        return Board.Money(place: WalletCommand.placeName(place), symbol: shown,
                           was: Self.amount(was, shown), after: (approx ? "≈ " : "") + Self.amount(after, shown))
    }

    /// Dollars to the cent; another coin to 4 places from 1 up (2.4722 AERO), 6 under it (0.074852 SOL): the Mac's MoneyLine.amount.
    nonisolated static func amount(_ d: Decimal, _ symbol: String) -> String {
        let size = abs(d)
        return Fmt.num(d, maxFraction: ["USDC", "USDG", "USDT", "USDC.e", "pathUSD"].contains(symbol) || size >= 100 ? 2 : size >= 1 ? 4 : 6)
    }

    /// Money into a deposit: its balance there now → after.
    private func into(_ place: String, _ symbol: String, was: Decimal, by delta: Decimal?) -> Board.Money? {
        guard let delta else { return nil }
        return Board.Money(place: place, symbol: symbol, was: Self.amount(was, symbol), after: Self.amount(max(0, was + delta), symbol))
    }

    func st(_ label: String, _ title: String, _ line: String = "", _ money: Board.Money? = nil) -> Board.Step {
        Board.Step(label: label, title: title, lines: line.isEmpty ? [] : [line], money: money)
    }

    func approve(_ amount: String, for spender: String) -> Board.Step {
        st(say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), amount, say("exactly this · only \(spender) may take it", "ровно столько · взять может только \(spender)"))
    }

    /// The Mac's route for a money move: where from, through what, where to, each with the balance it changes; nil for a
    /// command drawn as one step (its quotes and way picker live on that step). «all USDC» reads as the amount itself.
    func routeSteps(_ c: WalletCommand) -> [Board.Step]? { rawRouteSteps(c).map(amountsForAll) }

    /// «all USDT» on a step reads as the number held, «all available» small under it (the Mac's step(): the user wants
    /// the number, not «all»); the later steps saying the same «all» take the same number. Left as words when unread.
    func amountsForAll(_ steps: [Board.Step]) -> [Board.Step] {
        var found: [String: Decimal] = [:]
        let all = say("all available", "всё, что есть")
        return steps.map { step in
            var s = step
            let word = ["all ", "всё "].first { s.title.hasPrefix($0) }
            guard let word else { return s }
            let unit = String(s.title.dropFirst(word.count))
            guard let x = found[s.title] ?? heldForAll(unit, label: s.label), x > 0 else { return s }
            found[s.title] = x
            s.title = "\(Fmt.num(x, maxFraction: 8)) \(unit)"
            // An approve says exactly what it allows; the others say it's all there is.
            if !s.label.hasPrefix("APPROVE"), !s.label.hasPrefix("РАЗРЕШЕНИЕ") {
                let first = s.lines.first ?? ""
                s.lines = [first.isEmpty ? all : "\(all) · \(first)"] + s.lines.dropFirst()
            }
            return s
        }
    }

    /// What «all <unit>» comes to at the step's place («FROM · KAMINO», «FROM · BASE»…); nil when that isn't read.
    private func heldForAll(_ unit: String, label: String) -> Decimal? {
        let place = (label.components(separatedBy: "·").dropFirst().last ?? "").trimmingCharacters(in: .whitespaces).uppercased()
        // «FROM · AAVE · BASE»: what's lent there, not the wallet's free dollars.
        for what in ["Aave", "Morpho"] where label.uppercased().contains(what.uppercased()) {
            let keys = EVMNetwork.all.map(\.key)
            guard let key = keys.first(where: { WalletCommand.placeName($0).uppercased() == place }), rowsRead(key) != nil else { return nil }
            return depositThere([what], on: key)
        }
        switch place {
        case "": return nil
        case "KAMINO", "KAMINO LEND": return rowsRead("solana") == nil ? nil : depositThere(["Kamino Lend"], on: "solana")
        case "KAMINO EARN": return kaminoVault(unit)?.tokens
        case "JUPITER LEND": return rowsRead("solana") == nil ? nil : depositThere(["Jupiter Lend"], on: "solana")
        case "HLP VAULT", "ХРАНИЛИЩЕ HLP": return placeFacts.hyperliquid == nil ? nil : hlpEquity
        case "JITOSOL": return heldThere("JitoSOL", on: "solana")
        default:
            let keys = ["solana", "hyperliquid", "near"] + EVMNetwork.all.map(\.key)
            guard let key = keys.first(where: { WalletCommand.placeName($0).uppercased() == place || $0.uppercased() == place }) else { return nil }
            // A contract as tokenName writes it, «AI (0x2E8c…1e18)» (⚠ before a lookalike): its row by the contract's ends.
            if let m = unit.firstMatch(of: /\((0x[0-9a-fA-F]{4})…([0-9a-fA-F]{4})\)$/) {
                let (head, tail) = (String(m.1).lowercased(), String(m.2).lowercased())
                return rowsRead(key).map { rows in
                    rows.filter { $0.section == "tokens" && ($0.contract.map { $0.lowercased().hasPrefix(head) && $0.lowercased().hasSuffix(tail) } ?? false) }
                        .reduce(0) { $0 + $1.value }
                }
            }
            return heldThere(unit, on: key)
        }
    }

    private func rawRouteSteps(_ c: WalletCommand) -> [Board.Step]? {
        func n(_ d: Decimal?, _ unit: String) -> String { d.map { "\(Fmt.num($0, maxFraction: 6)) \(unit)" } ?? say("all \(unit)", "всё \(unit)") }
        let solUSDC = usdcFree(on: "solana").map { say("free: \(Fmt.num($0)) USDC", "свободно: \(Fmt.num($0)) USDC") } ?? ""
        let solSOL = heldThere("SOL", on: "solana").map { say("free: \(Fmt.num($0, maxFraction: 4)) SOL", "свободно: \(Fmt.num($0, maxFraction: 4)) SOL") } ?? ""
        let own = say("your own address ✓", "твой собственный адрес ✓")
        let relay = st(say("BRIDGE · RELAY", "МОСТ · RELAY"), "", say("a few seconds", "несколько секунд"))
        func bridged(_ title: String, _ line: String) -> Board.Step { var s = relay; s.title = title; s.lines = [line]; return s }
        switch c {
        case .withdrawToSolana(let asked):
            let x = asked ?? usdcFree(on: "hyperliquid")
            return [st(say("FROM · HYPERLIQUID", "ОТКУДА · HYPERLIQUID"), n(x, "USDC"), say("position margin isn’t touched", "маржа позиций не трогается"), moved("USDC", on: "hyperliquid", by: x.map { -$0 })),
                    bridged(n(x, "USDC"), say("≈ 4 s · small Relay fee", "≈ 4 с · небольшая комиссия Relay")),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), x.map { "≈ \(Fmt.num($0)) USDC" } ?? "USDC", own, x.flatMap { moved("USDC", on: "solana", by: $0, approx: true) })]
        case .solanaTo(let place, let x?):
            let name = WalletCommand.placeName(place), stable = place == "robinhood" ? "USDG" : "USDC"
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(x, "USDC"), solUSDC, moved("USDC", on: "solana", by: -x)),
                    bridged(n(x, "USDC"), place == "hyperliquid" ? say("a few seconds · a first deposit pays Hyperliquid's ≈ $1 opening fee", "несколько секунд · первый депозит платит ≈ $1 Hyperliquid за открытие")
                            : say("a few seconds · + \(EVMNetwork.named(place)?.gasCoin ?? "ETH") for gas when there's none", "несколько секунд · + \(EVMNetwork.named(place)?.gasCoin ?? "ETH") на газ, если его нет")),
                    st(say("TO · ", "КУДА · ") + name.uppercased(), "≈ \(Fmt.num(x)) \(stable)", own, moved(stable, on: place, by: x, approx: true))]
        case .evmToSolana(let net, let x), .bridge(from: let net, to: "solana", let x):
            return backRoute(net, x)
        case .baseToSolana(let x): return backRoute("base", x)
        case .arbitrumToSolana(let x): return backRoute("arbitrum", x)
        case .robinhoodToSolana(let x): return backRoute("robinhood", x)
        case .bridge(let from, let to, let x):
            let a = WalletCommand.placeName(from), b = WalletCommand.placeName(to)
            let moving = x ?? usdcFree(on: from)
            // The bridge it goes by once quoted (Relay, unless CCTP or 1Click lands more), with what each costs and lands.
            let ways = quotes(c), taken = ways?.first?.way ?? "relay"
            return [st(say("FROM · ", "ОТКУДА · ") + a.uppercased(), n(moving, "USDC"), usdcFree(on: from).map { say("free there: \(Fmt.num($0)) USDC", "свободно там: \(Fmt.num($0)) USDC") } ?? "",
                       moved("USDC", on: from, by: moving.map { -$0 })),
                    approve(n(moving, "USDC"), for: taken == "cctp" ? "Circle's CCTP" : taken == "1click" ? "NEAR Intents" : "Relay"),
                    bridgeStep(ways, title: n(moving, "USDC"), fallback: (say("BRIDGE · RELAY", "МОСТ · RELAY"), say("\(a) → \(b) · no stop on Solana", "\(a) → \(b) · без остановки на Solana"))),
                    st(say("TO · ", "КУДА · ") + b.uppercased(), ways?.first?.gets.components(separatedBy: " · ").first ?? moving.map { "≈ \(Fmt.num($0)) USDC" } ?? "USDC", own,
                       moving.flatMap { moved(to == "robinhood" ? "USDG" : "USDC", on: to, by: $0, approx: true) })]
        case .aave(true, let x?, let net), .morpho(true, let x?, let net):
            let morpho: Bool = if case .morpho = c { true } else { false }
            let name = WalletCommand.placeName(net), what = morpho ? "Morpho" : "Aave"
            let have = usdcFree(on: net) ?? 0, short = max(0, x - have), needGas = gasLow(on: net)
            if net == "base" {
                // The Mac's aaveRoute on Base: what Solana sends with Relay's fee, the gas coming along, the rate read.
                let over = max(short, needGas ? Decimal(string: "0.5")! : 0), sends = over + (needGas ? Decimal(string: "0.14")! : Decimal(string: "0.03")!)
                let rate = placeFacts.rates[morpho ? "morpho" : "aave"].map { say("≈ \(Fmt.num($0))% a year", "≈ \(Fmt.num($0))% в год") } ?? say("rate from the pool", "ставка пула")
                let from: [Board.Step] = short > 0 || needGas
                    ? [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), usdcFree(on: "solana").map { "\(Fmt.num($0)) USDC" } ?? "—",
                          say("sends ≈ \(Fmt.num(sends)) USDC", "отправит ≈ \(Fmt.num(sends)) USDC"), moved("USDC", on: "solana", by: -sends, approx: true)),
                       st(say("BRIDGE · RELAY", "МОСТ · RELAY"), "\(Fmt.num(over)) USDC → Base",
                          needGas ? say("+ $0.03 of ETH for gas · ≈ 2 s", "+ $0.03 в ETH на газ · ≈ 2 с") : say("≈ 2 s", "≈ 2 с"), moved("USDC", on: "base", by: over, approx: true))]
                      + (needGas ? [st(say("GAS · COMES ALONG", "ГАЗ · ПРИДЁТ С МОСТОМ"), say("≈ $0.03 of ETH", "≈ $0.03 в ETH"),
                                       say("there’s no ETH on Base yet · it comes with the bridge", "ETH на Base пока нет · он придёт с мостом"))] : [])
                    : [st(say("ON BASE", "В BASE"), "\(Fmt.num(have)) USDC", say("already there, gas too", "уже там, газ тоже"), moved("USDC", on: net, by: -x))]
                return from + [approve(n(x, "USDC"), for: what),
                               st(morpho ? "MORPHO · " + MorphoVault.name.uppercased() : "AAVE V3 · BASE", n(x, "USDC"), rate,
                                  into(what, "USDC", was: depositThere([what], on: net), by: x))]
            }
            let coin = EVMNetwork.named(net)?.gasCoin ?? "ETH", arrive = max(short, Decimal(string: "0.5")!)
            // The Mac's lendingRoute: no gas there brings it from Solana too, with half a dollar at least.
            let from: [Board.Step] = short > 0 || needGas
                ? [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(arrive, "USDC"),
                      (have > 0 ? say("\(Fmt.num(have)) already on \(name) · ", "\(Fmt.num(have)) уже в \(name) · ") : "") + solUSDC,
                      moved("USDC", on: "solana", by: -arrive)),
                   st(say("BRIDGE · RELAY", "МОСТ · RELAY"), "USDC → \(name)",
                      needGas ? say("+ \(coin) for gas · ≈ 2 s", "+ \(coin) на газ · ≈ 2 с") : say("≈ 2 s", "≈ 2 с"),
                      moved("USDC", on: net, by: arrive, approx: true))]
                : [st(say("ON ", "В ") + name.uppercased(), "\(Fmt.num(have)) USDC", say("already there, gas too", "уже там, газ тоже"), moved("USDC", on: net, by: -x))]
            return from + [approve(n(x, "USDC"), for: what),
                           st((morpho ? "MORPHO · " : "AAVE V3 · ") + name.uppercased(), n(x, "USDC"), say("rate from the \(morpho ? "vault" : "pool")", "ставка \(morpho ? "хранилища" : "пула")"),
                              into(what, "USDC", was: depositThere([what], on: net), by: x))]
        case .aave(false, let x, let net), .morpho(false, let x, let net):
            let what: String = if case .morpho = c { "Morpho" } else { "Aave" }
            let lent = depositThere([what], on: net), moving = x ?? (lent > 0 ? lent : nil)
            return [st(say("FROM · ", "ОТКУДА · ") + "\(what.uppercased()) · \(WalletCommand.placeName(net).uppercased())", n(moving, "USDC"), say("with the interest", "с процентами"),
                       into(what, "USDC", was: lent, by: moving.map { -$0 })),
                    approve(n(moving, "USDC"), for: "Relay"),
                    bridged(n(moving, "USDC"), say("a few seconds", "несколько секунд")),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), moving.map { "≈ \(Fmt.num($0)) USDC" } ?? "USDC", own, moving.flatMap { moved("USDC", on: "solana", by: $0, approx: true) })]
        case .lendDeposit(let s, let x):
            let a = x ?? heldThere(s, on: "solana")
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(x, s), s == "SOL" ? solSOL : solUSDC, moved(s, on: "solana", by: a.map { -$0 })),
                    st(say("INTO · JUPITER LEND", "В · JUPITER LEND"), n(x, s), say("earns the vault rate · withdraw any time", "приносит ставку хранилища · вывести можно в любой момент"),
                       s == "USDC" ? into("Jupiter Lend", s, was: depositThere(["Jupiter Lend"], on: "solana"), by: a) : nil)]
        case .lendWithdraw(let s, let x):
            let lent = depositThere(["Jupiter Lend"], on: "solana"), moving = x ?? (s == "USDC" && lent > 0 ? lent : nil)
            return [st(say("FROM · JUPITER LEND", "ОТКУДА · JUPITER LEND"), n(moving, s), lent > 0 ? say("in the vault: \(Fmt.num(lent)) USDC", "в хранилище: \(Fmt.num(lent)) USDC") : "",
                       s == "USDC" ? into("Jupiter Lend", s, was: lent, by: moving.map { -$0 }) : nil),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), n(moving, s), say("your wallet", "твой кошелёк"), moving.flatMap { moved(s, on: "solana", by: $0) })]
        case .kamino(true, let s, let x):
            let a = x ?? heldThere(s, on: "solana")
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(x, s), s == "SOL" ? solSOL : solUSDC, moved(s, on: "solana", by: a.map { -$0 })),
                    st(say("INTO · KAMINO LEND", "В · KAMINO LEND"), n(x, s), say("first deposit pays ≈ 0.03 SOL rent", "первый депозит берёт ≈ 0.03 SOL залога"),
                       s == "USDC" ? into("Kamino Lend", s, was: depositThere(["Kamino Lend"], on: "solana"), by: a) : nil)]
        case .kamino(false, let s, let x):
            // From Lend when it holds anything, else the Earn vault holding the coin (the Mac's kaminoVault).
            let lent = depositThere(["Kamino Lend"], on: "solana"), vault = kaminoVault(s)
            let moving = x ?? vault?.tokens ?? (s == "USDC" && lent > 0 ? lent : nil)
            return [st(vault == nil ? say("FROM · KAMINO LEND", "ОТКУДА · KAMINO LEND") : say("FROM · KAMINO EARN", "ОТКУДА · KAMINO EARN"), n(moving, s),
                       vault.map { "\($0.name): \($0.amount)" } ?? "", vault == nil && s == "USDC" ? into("Kamino Lend", s, was: lent, by: moving.map { -$0 }) : nil),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), n(moving, s), say("your wallet", "твой кошелёк"), moving.flatMap { moved(s, on: "solana", by: $0) })]
        case .stake(let x):
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(x, "SOL"), solSOL, moved("SOL", on: "solana", by: x.map { -$0 })),
                    st(say("INTO · JITOSOL", "В · JITOSOL"), say("liquid staking", "ликвидный стейкинг"), say("earns the staking yield · swap back any time", "приносит доход стейкинга · обменять обратно можно в любой момент"))]
        case .unstake(let x):
            return [st(say("FROM · JITOSOL", "ОТКУДА · JITOSOL"), n(x, "JitoSOL")),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), "SOL", say("your wallet", "твой кошелёк"))]
        case .hlp(true, let x):
            let a = x ?? usdcFree(on: "hyperliquid")
            let out = Calendar.current.date(byAdding: .day, value: 4, to: Date()) ?? Date()
            return [st(say("FROM · HYPERLIQUID", "ОТКУДА · HYPERLIQUID"), n(x, "USDC"), usdcFree(on: "hyperliquid").map { say("free: \(Fmt.usd($0))", "свободно: \(Fmt.usd($0))") } ?? "",
                       moved("USDC", on: "hyperliquid", by: a.map { -$0 })),
                    st(say("INTO · HLP VAULT", "В · ХРАНИЛИЩЕ HLP"), n(x, "USDC"), say("min $5 · 4-day lockup", "от $5 · блокировка 4 дня"), into("HLP", "USDC", was: hlpEquity, by: a)),
                    st(say("LOCKED · 4 DAYS", "ЗАБЛОКИРОВАНО · 4 ДНЯ"), n(x, "USDC"), say("HLP lets it out from \(Fmt.date(out))", "HLP выпустит его с \(Fmt.date(out))"))]
        case .hlp(false, let x):
            let moving = x ?? (hlpEquity > 0 ? hlpEquity : nil)
            return [st(say("FROM · HLP VAULT", "ОТКУДА · ХРАНИЛИЩЕ HLP"), n(moving, "USDC"), "", into("HLP", "USDC", was: hlpEquity, by: moving.map { -$0 })),
                    st(say("TO · HYPERLIQUID", "КУДА · HYPERLIQUID"), "USDC", say("trading balance", "торговый баланс"), moving.flatMap { moved("USDC", on: "hyperliquid", by: $0) })]
        case .buy(let a, let u):
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(u, "USDC"), solUSDC, moved("USDC", on: "solana", by: u.map { -$0 })),
                    st(say("BUY · JUPITER", "ПОКУПКА · JUPITER"), a.symbol, say("best route, simulated first", "лучший маршрут, сначала симуляция"))]
        case .sell(let a, let x):
            let key = a.symbol == "SOL" || a.mint.isEmpty ? a.symbol : a.mint
            let held = heldThere(key, on: "solana")
            return [st(say("SELL · JUPITER", "ПРОДАЖА · JUPITER"), n(x, a.symbol), held.map { say("held: \(Fmt.num($0, maxFraction: 6)) \(a.symbol)", "есть: \(Fmt.num($0, maxFraction: 6)) \(a.symbol)") } ?? "",
                       moved(key, on: "solana", by: x.map { -$0 })),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), "USDC", say("your wallet", "твой кошелёк"))]
        case .swapOn("solana", let from, let to, let x):
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(x, from.uppercased()), say("your wallet", "твой кошелёк"), moved(from, on: "solana", by: x.map { -$0 })),
                    st(say("SWAP · JUPITER", "ОБМЕН · JUPITER"), to.uppercased(), say("Jupiter's best route", "лучший маршрут Jupiter")),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), to.uppercased(), say("your wallet ✓", "твой кошелёк ✓"))]
        case .jupiterDCA(let a, let u, let o, let e):
            let orders = o ?? u.map(WalletCommand.jupiterDCAOrders) ?? 2
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(u, "USDC"), say("all goes in now", "вносится всё сразу"), moved("USDC", on: "solana", by: u.map { -$0 })),
                    st(say("EVERY ", "КАЖДЫЕ ") + WalletCommand.everyText(e).uppercased() + " · JUPITER DCA", "\(orders) × \(u.map { Fmt.num($0 / Decimal(orders)) } ?? "?") USDC → \(a.symbol)",
                       say("Jupiter's keepers buy on-chain, the first now · 0.1% fee · cancel any time", "Jupiter покупает на блокчейне, первая покупка сейчас · комиссия 0.1% · отменить можно в любой момент"))]
        case .jupiterDCACancel(let a):
            return [st(say("FROM · JUPITER DCA", "ОТКУДА · JUPITER DCA"), a.map { say("\($0.symbol) orders", "ордера \($0.symbol)") } ?? say("all orders", "все ордера"),
                       say("what they haven't spent", "то, что они не потратили")),
                    st(say("TO · SOLANA", "КУДА · SOLANA"), "USDC", say("your wallet · the ≈ 0.005 SOL rent too", "твой кошелёк · и ≈ 0.005 SOL залога"))]
        case .sendSol(_, let x):
            guard let e = try? nativeRecipient(c) else { return nil }
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), n(x, "SOL"), solSOL, moved("SOL", on: "solana", by: -x)),
                    st(say("TO · ", "КУДА · ") + say("SAVED ADDRESS", "СОХРАНЁННЫЙ АДРЕС"), e.name, AddressCheck.ends(e.address).start + " … " + AddressCheck.ends(e.address).end)]
        case .sendToken(let name, let token, let x, let net):
            if let r = try? acrossRecipient(name, token: token, network: net) { return acrossSteps(c, r, x) }
            guard let (e, network) = try? tokenRecipient(name, network: net) else { return nil }
            let place = WalletCommand.placeName(network)
            return [st(say("FROM · ", "ОТКУДА · ") + place.uppercased(), n(x, token.uppercased()), heldThere(token, on: network).map { say("held: \(Fmt.num($0, maxFraction: 6))", "есть: \(Fmt.num($0, maxFraction: 6))") } ?? "",
                       moved(token, on: network, by: x.map { -$0 })),
                    st(say("TO · ", "КУДА · ") + say("SAVED ADDRESS", "СОХРАНЁННЫЙ АДРЕС"), e.name, AddressCheck.ends(e.address).start + " … " + AddressCheck.ends(e.address).end)]
        case .sweepDust(let under):
            let dust = dustTokens(under: under)
            guard !dust.isEmpty else { return nil }
            var steps = dust.prefix(8).map { d in
                st(say("SELL · ", "ПРОДАЖА · ") + WalletCommand.placeName(d.chain).uppercased(), d.symbol, "≈ \(Fmt.usd(d.usd)) → USDC", moved(d.contract, on: d.chain, by: nil))
            }
            if dust.count > 8 { steps.append(st(say("AND \(dust.count - 8) MORE", "И ЕЩЁ \(dust.count - 8)"), say("\(dust.count - 8) tokens", "\(dust.count - 8) токенов"), say("the same way", "так же"))) }
            return steps + [st(say("TO · USDC", "В · USDC"), "≈ \(Fmt.usd(dust.reduce(0) { $0 + $1.usd }))", say("on each network · your own address ✓", "в каждой сети · твой собственный адрес ✓"))]
        case .gas(let place, let usd):
            let name = WalletCommand.placeName(place), coin = EVMNetwork.named(place)?.gasCoin ?? "ETH", u = usd ?? 1
            return [st(say("FROM · SOLANA", "ОТКУДА · SOLANA"), "\(Fmt.num(u + Decimal(string: "0.58")!)) USDC", solUSDC, moved("USDC", on: "solana", by: -(u + Decimal(string: "0.58")!), approx: true)),
                    bridged("\(Fmt.usd(u)) of \(coin)", say("+ 0.5 USDC along · a few seconds", "+ 0.5 USDC вместе · несколько секунд")),
                    st(say("GAS · ", "ГАЗ · ") + name.uppercased(), "≈ \(Fmt.usd(u)) of \(coin)", say("for the fees of the next moves there", "на комиссии следующих действий там"))]
        case .hlSpotBuy(let symbol, let u?):
            let gets = quotes(c)?.first?.gets
            return [st(say("FROM · HYPERLIQUID", "ОТКУДА · HYPERLIQUID"), n(u, "USDC"), say("spot balance · perp tops it up when short", "спотовый баланс · при нехватке доливается с perp")),
                    st(say("BUY · SPOT ORDER", "ПОКУПКА · СПОТ"), symbol.uppercased(), gets.map { say("≈ \($0) · at most 1% over the mid", "≈ \($0) · не дороже середины на 1%") }
                       ?? say("IOC · at most 1% over the mid", "IOC · не дороже середины на 1%")),
                    st(say("TO · SPOT BALANCE", "КУДА · СПОТОВЫЙ БАЛАНС"), symbol.uppercased(), say("on Hyperliquid, yours ✓", "на Hyperliquid, твой ✓"),
                       Self.quotedAmount(gets).map { gained(symbol, symbol.uppercased(), on: "hyperliquid", by: $0) })]
        case .hlSpotSell(let symbol, let x):
            let gets = quotes(c)?.first?.gets
            return [st(say("FROM · SPOT BALANCE", "ОТКУДА · СПОТОВЫЙ БАЛАНС"), n(x, symbol.uppercased()), say("on Hyperliquid", "на Hyperliquid"), moved(symbol, on: "hyperliquid", by: x.map { -$0 })),
                    st(say("SELL · SPOT ORDER", "ПРОДАЖА · СПОТ"), "→ USDC", gets.map { say("≈ \($0) · at most 1% under the mid", "≈ \($0) · не дешевле середины на 1%") }
                       ?? say("IOC · at most 1% under the mid", "IOC · не дешевле середины на 1%")),
                    st(say("TO · SPOT USDC", "КУДА · СПОТ USDC"), "USDC", say("on Hyperliquid, yours ✓", "на Hyperliquid, твой ✓"))]
        default:
            return cardRoute(c)
        }
    }

    /// An EVM network's USDC (USDG on Robinhood Chain) back to Solana: an exact approve and Relay's deposit.
    private func backRoute(_ net: String, _ x: Decimal?) -> [Board.Step] {
        let name = WalletCommand.placeName(net), stable = net == "robinhood" ? "USDG" : "USDC"
        let held = usdcFree(on: net), moving = x.map { min($0, held ?? $0) } ?? held
        func n(_ d: Decimal?) -> String { d.map { "\(Fmt.num($0)) \(stable)" } ?? say("all \(stable)", "всё \(stable)") }
        return [st(say("FROM · ", "ОТКУДА · ") + name.uppercased(), n(moving), held.map { say("free there: \(Fmt.num($0)) \(stable)", "свободно там: \(Fmt.num($0)) \(stable)") } ?? "",
                   moved(stable, on: net, by: moving.map { -$0 })),
                approve(n(moving), for: "Relay"),
                st(say("BRIDGE · RELAY", "МОСТ · RELAY"), n(moving), say("≈ $0.02 · a few seconds", "≈ $0.02 · несколько секунд")),
                st(say("TO · SOLANA", "КУДА · SOLANA"), moving.map { "≈ \(Fmt.num($0)) USDC" } ?? "USDC", say("your own address ✓", "твой собственный адрес ✓"),
                   moving.flatMap { moved("USDC", on: "solana", by: $0, approx: true) })]
    }
}

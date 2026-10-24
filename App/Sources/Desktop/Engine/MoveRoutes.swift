import EVMChain
import Foundation
import IhatecryptoCore
import SignerAPI
import TradeExecution
import WalletCore

/// The Mac's route for the moves Windows drew as one card (Canvases.swift route(_:)): a stock or a token bought or sold
/// on an EVM network, swaps off Solana and across networks, a revoke, NEAR, a coin sent through NEAR Intents, a paid
/// request, a multisig, CCTP, USDT0, Aave's debt, Merkl's rewards, USDC to a saved address, a close. Each quote lands on
/// its own step as it comes (`quoteRoute`), and the card says what the move risks (`moveCautions`).
extension Engine {
    // MARK: What a card risks

    /// «A token by its contract is anyone's coin»: a buy or a swap into a token named only by its contract.
    var byContract: String {
        say("A token by its contract is anyone’s coin: its price can halve in a day and nobody vouches for it. Put in only what you can lose.",
            "Токен по контракту может выпустить кто угодно: его цена может упасть вдвое за день, и никто за него не ручается. Вкладывай только то, что не жалко потерять.")
    }

    /// The cautions under a move's route that aren't about whom it pays (the Mac's MoveRoute.warning): a token taken by its
    /// contract, and what a Robinhood Stock Token is.
    func moveCautions(_ c: WalletCommand) -> [String] {
        switch c {
        case .buyStock(let s, _):
            return [s.hasPrefix("0x") ? byContract
                    : say("Robinhood Stock Tokens give economic exposure, not the shares, and aren’t offered to US persons or in Canada, the UK and Switzerland.",
                          "Токены акций Robinhood повторяют цену акции, но акцией не являются, и не продаются жителям США, Канады, Великобритании и Швейцарии.")]
        case .buyOn(_, let token, _, _) where token.hasPrefix("0x"): return [byContract]
        case .swapOn(let place, _, let to, _) where to.hasPrefix("0x") && EVMNetwork.named(place) != nil && place != "tempo": return [byContract]
        default: return []
        }
    }

    /// An exchange's XRP address credits a deposit only by its destination tag: without it the money is lost to the user.
    func xrpTagWarning(_ entry: SignerAddress) -> String? {
        guard entry.network == "xrp", entry.memo.trimmingCharacters(in: .whitespaces).isEmpty else { return nil }
        return say("An exchange’s XRP address needs its destination tag: without it the exchange can’t credit you. Save the tag as this address’s memo first.",
                   "XRP-адресу биржи нужен тег назначения: без него биржа не зачислит перевод. Сначала сохрани тег как memo этого адреса.")
    }

    /// Kamino's first-deposit rent stopped by the loss cap: Jupiter Lend takes the same deposit without it, one card away
    /// (the Mac's choice beside «Send anyway», Execution.swift:735).
    func rentFreeInstead(_ p: Pending) -> [ChatRouter.Option] {
        guard case .wallet(.kamino(deposit: true, symbol: let s, amount: let a?)) = p, ["USDC", "SOL"].contains(s) else { return [] }
        return [ChatRouter.Option(title: say("Jupiter Lend instead · no rent", "Лучше в Jupiter Lend · без аренды"),
                                  comment: say("The same deposit, no accounts to pay for", "Тот же депозит, без платы за аккаунты"),
                                  command: WalletCommand.lendDeposit(s, a).summary)]
    }

    // MARK: Quotes on the route

    /// The waiting move's quotes as they came (RouteEdit.quoteWays, quoteRoute), the best first; nil until then.
    func quotes(_ c: WalletCommand) -> [RouteEdit.Way]? { wayQuotes?.command == c ? wayQuotes?.ways : nil }

    /// The amount a quote says arrives: «2.41 AERO», «1,234.5 USDC · 0.3% impact» → 2.41, 1234.5. Our own quotes'
    /// wording, never the user's (the Mac's quotedAmount).
    nonisolated static func quotedAmount(_ gets: String?) -> Decimal? {
        guard let m = gets?.firstMatch(of: /\d[\d,]*(?:\.\d+)?/) else { return nil }
        return Decimal(string: String(m.0).replacingOccurrences(of: ",", with: ""), locale: Locale(identifier: "en_US_POSIX"))
    }

    /// The route drawn again from what is known now: a quote that came lands on its step, and the footer says the way it
    /// takes now (a cross swap found through NEAR Intents, Tempo's exchange or Relay's swap there).
    func redrawRoute(_ board: String, _ c: WalletCommand) {
        if redrawChain(board) { return }
        guard let steps = routeSteps(c) else { changed(); return }
        let note = routeNote(c)
        updateBoard(board) { b in b.steps = steps; if let note { b.note = note } }
        nameTokens(board, c)
    }

    /// A chain's waiting card drawn again whole (Canvas.swift steps(for:)) once its first move's quote or a token's
    /// symbol comes: the Mac quotes and names the chain's first move as any card's (WithdrawBoard, chainSteps).
    @discardableResult
    func redrawChain(_ board: String) -> Bool {
        guard case .chain? = pending, let p = pending, card?.id == board else { return false }
        let steps = steps(for: p, title: "", lines: [])
        updateBoard(board) { $0.steps = steps }
        return true
    }

    /// A move's title with each token it names by contract as people read it, «⚠ USDC (0x3C44…93BC)» (the Mac's
    /// readable title); only a title that is the move's own words, and only its tokens — never a recipient's address.
    func nameTokens(_ board: String, _ c: WalletCommand) {
        let tokens: [(String, String)] = switch c {
        case .buyOn(let p, let t, _, _), .sellOn(let p, let t, _, _): [(t, p)]
        case .swapOn(let p, let a, let b, _): [(a, p), (b, p)]
        case .crossSwap(let a, let src, let b, let dst, _): [(a, src), (b, dst)]
        case .buyStock(let s, _), .sellStock(let s, _): [(s, "robinhood")]
        default: []
        }
        let contracts = tokens.filter { $0.0.hasPrefix("0x") && $0.0.count == 42 }
        guard !contracts.isEmpty else { return }
        let named = contracts.reduce(c.summary) { $0.replacingOccurrences(of: $1.0, with: tokenName($1.0, on: $1.1), options: .caseInsensitive) }
        // Named before its symbol was known: the contract shortened.
        let unnamed = contracts.reduce(c.summary) { $0.replacingOccurrences(of: $1.0, with: "\($1.0.prefix(6))…\($1.0.suffix(4))", options: .caseInsensitive) }
        let ours: (String?) -> Bool = { $0 == c.summary || $0 == unnamed }
        updateBoard(board) { b in if ours(b.title) { b.title = named } }
        if card?.id == board, ours(card?.title) { card?.title = named }
    }

    /// The waiting move's quote for its route (the Mac's quoteSwap, all but Jupiter's): asked once and again every 15 s
    /// while it fails; Aave's health factor every 30 s while the card waits, as it moves with the price. Nothing is signed.
    /// False for a move this doesn't quote.
    func quoteRoute(_ c: WalletCommand, board: String) async -> Bool {
        switch c {
        case .claimRewards, .sweepRewards:
            // What Merkl owes, by network, is the route itself.
            guard let me = evmAddress, let owed = try? await Merkl.rewards(user: me), !owed.isEmpty, card?.id == board else { return true }
            let steps = claimSteps(owed, c), note = claimNote(owed, c)
            // Its title names what's claimed (the Mac's claimRoute), not the command's words.
            let sweep: Bool = if case .sweepRewards = c { true } else { false }
            let title = (sweep ? say("Sweep rewards · ", "Собрать награды · ") : say("Claim rewards · ", "Забрать награды · "))
                + owed.map { "\(Fmt.num($0.claimable, maxFraction: 6)) \($0.symbol)" }.joined(separator: ", ")
            updateBoard(board) { $0.steps = steps; $0.note = note; $0.title = title }
            card?.title = title
            return true
        default: break
        }
        guard routeQuoted(c) else { return false }
        while !Task.isCancelled, card?.id == board {
            if let ways = await routeQuote(c) {
                guard card?.id == board else { return true }
                wayQuotes = (c, ways)
                redrawRoute(board, c)
                guard case .aaveBorrow = c else { return true }
                liquidationCaution(board, ways)
                try? await Task.sleep(for: .seconds(30))
                continue
            }
            try? await Task.sleep(for: .seconds(15))
        }
        return true
    }

    /// The moves `routeQuote` can quote: Solana's own go to Jupiter (quoteCard), the EVM trades to RouteEdit.quoteWays.
    private func routeQuoted(_ c: WalletCommand) -> Bool {
        switch c {
        case .hlSpotBuy(_, _?), .hlSpotSell(_, _?), .crossSwap, .aaveBorrow(_?, _): true
        case .swapOn(let place, _, _, let x): place == "hyperliquid" || place == "near" ? x != nil : place != "solana" && EVMNetwork.named(place) != nil
        case .usdt(let a, let b, _, _), .bridge(let a, let b, _): EVMNetwork.named(a) != nil && EVMNetwork.named(b) != nil
        case .sendToken(let name, let token, _?, let net): (try? acrossRecipient(name, token: token, network: net)) != nil
        default: false
        }
    }

    /// One read of the quote; nil when it failed or the move has none.
    private func routeQuote(_ c: WalletCommand) async -> [RouteEdit.Way]? {
        guard let e = try? executor() else { return nil }
        func one(_ way: String, _ gets: String) -> [RouteEdit.Way] { [RouteEdit.Way(way: way, gets: gets, worth: "")] }
        switch c {
        case .hlSpotBuy(let s, let u?): return (try? await Executor.spotQuote(s, usdc: u, sell: nil)).map { one("Hyperliquid", $0) }
        case .hlSpotSell(let s, let x?): return (try? await Executor.spotQuote(s, usdc: nil, sell: x)).map { one("Hyperliquid", $0) }
        case .swapOn("hyperliquid", let a, let b, let x?): return (try? await Executor.spotSwapQuote(from: a, to: b, amount: x)).map { one("Hyperliquid", $0) }
        case .swapOn("near", let a, let b, let x?):
            return (try? await e.crossQuoteText(a, on: "near", for: b, on: "near", amount: x, near: nearAddress)).map { one($0.way, $0.gets) }
        case .swapOn(let place, let a, let b, let x):
            guard let net = EVMNetwork.named(place) else { return nil }
            return (try? await e.pairQuote(net, from: a, to: b, amount: x)).map { one($0.way, $0.gets) }
        case .crossSwap(let a, let from, let b, let to, let x):
            return (try? await e.crossQuoteText(a, on: from, for: b, on: to, amount: x, near: nearAddress)).map { one($0.way, $0.gets) }
        case .usdt(let from, let to, let x, let layerZero):
            guard let a = EVMNetwork.named(from), let b = EVMNetwork.named(to),
                  let ways = try? await e.usdtWays(from: a, to: b, usdt: x, layerZero: layerZero), !ways.isEmpty else { return nil }
            return ways.map { RouteEdit.Way(way: $0.way, gets: $0.gets, worth: Fmt.usd($0.worth)) }
        case .bridge(let from, let to, let x):
            guard let a = EVMNetwork.named(from), let b = EVMNetwork.named(to),
                  let ways = try? await e.bridgeWays(from: a, to: b, usdc: x), !ways.isEmpty else { return nil }
            return ways.map { RouteEdit.Way(way: $0.way, gets: $0.gets, worth: Fmt.usd($0.worth)) }
        case .aaveBorrow(let x?, let net):
            guard let m = AaveV3.markets.first(where: { $0.chain == net }), let p = try? await e.aaveBorrowPreview(m, usdc: x) else { return nil }
            let now = p.now.health.map { Fmt.num($0, maxFraction: 2) } ?? "∞", after = p.after.map { Fmt.num($0, maxFraction: 2) } ?? "∞"
            return one("health", "\(now) → \(after)") + one("liquidation", p.liquidation.map { "ETH ≤ \(Fmt.usd($0))" } ?? "")
                + one("collateral", Fmt.usd(p.now.collateral))
        case .sendToken(let name, let token, let x?, let net):
            guard let r = try? acrossRecipient(name, token: token, network: net),
                  let q = try? await e.acrossQuote(r.symbol, amount: x, network: r.entry.network, to: r.address) else { return nil }
            // What it's worth there, and what 1Click and the network keep: a small BTC send loses a fifth to fees.
            let cost = Decimal(string: q.quote.amountInFormatted) ?? 0, worth = q.quote.amountOutUSD
            let fees = worth.map { cost - $0 }.map { $0 > 0 && cost > 0 ? say(" · fees ≈ \(Fmt.usd($0)) (\(Fmt.num($0 / cost * 100, maxFraction: 0))%)",
                                                                                  " · комиссии ≈ \(Fmt.usd($0)) (\(Fmt.num($0 / cost * 100, maxFraction: 0))%)") : "" } ?? ""
            let time = q.quote.timeEstimate.map { $0 >= 120 ? say(" · ≈ \($0 / 60) min", " · ≈ \($0 / 60) мин") : say(" · ≈ \($0) s", " · ≈ \($0) с") } ?? ""
            return one(q.source?.name ?? "Solana", say("≈ \(Fmt.num(cost)) USDC for \(worth.map(Fmt.usd) ?? "?") of it", "≈ \(Fmt.num(cost)) USDC за \(worth.map(Fmt.usd) ?? "?")") + fees + time)
        default: return nil
        }
    }

    /// «Liquidation if ETH ≤ $X» under a borrow's route, as its last quote says; replaced by each new one.
    func liquidationCaution(_ board: String, _ ways: [RouteEdit.Way]) {
        let mark = say("Liquidation if", "Ликвидация, если")
        let text = ways.first { $0.way == "liquidation" }.flatMap { $0.gets.isEmpty ? nil : $0.gets }.map {
            say("Liquidation if \($0): Aave sells your collateral at a penalty. Keep an eye on it, or borrow less.",
                "Ликвидация, если \($0): Aave продаст твой залог со штрафом. Следи за ценой или займи меньше.")
        }
        card?.warnings.removeAll { $0.hasPrefix(mark) }
        if let text { card?.warnings.append(text) }
        updateBoard(board) { b in
            b.lines.removeAll { $0.hasPrefix("⚠ " + mark) }
            if let text { b.lines.append("⚠ " + text) }
        }
    }

    // MARK: The routes

    /// A token as people read it (the Mac's tokenLabel): a contract as «AI (0x2E8c…1e18)», with ⚠ before a symbol that
    /// imitates another — a ticker the app moves money in, on any other contract — and shortened until its symbol is
    /// known (a favorite, the portfolio, else looked up once); a ticker as written.
    func tokenName(_ token: String, on place: String) -> String {
        guard token.hasPrefix("0x") else { return Stablecoins.find(token, on: place)?.symbol ?? token.uppercased() }
        let key = token.lowercased(), short = "\(token.prefix(6))…\(token.suffix(4))"
        let symbol = favorites.first { $0.address.lowercased() == key }?.symbol
            ?? portfolio.networks.first { $0.key == place }?.rows.first { $0.contract?.lowercased() == key }?.symbol
            ?? ContractNames.known[key]
        guard let symbol else { learnName(token); return short }
        return "\(TokenLookup.suspicious(symbol, contract: token, genuine: Self.genuineContracts) ? "⚠ " : "")\(symbol) (\(short))"
    }

    /// The real dollars and wrapped ether where the app moves money: their tickers on any other contract are a fake.
    nonisolated static let genuineContracts = Set(EVMNetwork.all.map { $0.usdc.lowercased() } + Stablecoins.byNetwork.values.joined().map { $0.address.lowercased() } + [
        "0x4200000000000000000000000000000000000006",   // WETH · Base
        "0x82af49447d8a07e3bd95bd0d56f35241523fbab1",   // WETH · Arbitrum
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",   // WETH · Ethereum
        "0xdac17f958d2ee523a2206206994597c13d831ec7",   // USDT · Ethereum
        "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9",   // USDT · Arbitrum
    ])

    /// An unknown contract's symbol, asked once (DexScreener); the waiting card is drawn again with it.
    private func learnName(_ contract: String) {
        guard ContractNames.asked.insert(contract.lowercased()).inserted else { return }
        Task { await learnedName(contract) }
    }

    private func learnedName(_ contract: String) async {
        guard let symbol = try? await TokenLookup.lookup(contract).first?.symbol else { return }
        ContractNames.known[contract.lowercased()] = symbol
        if case .wallet(let c)? = pending, let id = card?.id { redrawRoute(id, c) } else if let id = card?.id { redrawChain(id) }
    }

    /// A token that arrives: what's held of it now (0 when none) and that plus what arrives, «≈».
    func gained(_ token: String, _ label: String, on place: String, by delta: Decimal) -> Board.Money {
        let was = heldThere(token, on: place) ?? 0
        return Board.Money(place: WalletCommand.placeName(place), symbol: label, was: Self.amount(was, label), after: "≈ " + Self.amount(was + delta, label))
    }

    /// A price per token from the portfolio's row, when it's held.
    private func rowPrice(_ token: String, on place: String) -> Decimal? {
        portfolio.networks.first { $0.key == place }?.rows.first { $0.symbol.uppercased() == token.uppercased() || $0.contract?.lowercased() == token.lowercased() }
            .flatMap { r in r.usd.flatMap { r.value > 0 ? $0 / r.value : nil } }
    }

    private func from(_ place: String) -> String { say("FROM · ", "ОТКУДА · ") + WalletCommand.placeName(place).uppercased() }
    private func to(_ place: String) -> String { say("TO · ", "КУДА · ") + WalletCommand.placeName(place).uppercased() }
    private var savedLabel: String { say("TO · SAVED ADDRESS", "КУДА · СОХРАНЁННЫЙ АДРЕС") }
    private func ends(_ address: String) -> String { let e = AddressCheck.ends(address); return "\(e.start) … \(e.end)" }
    private var quoting: String { say("quoting…", "считаю котировку…") }
    private var noQuote: String { say("no quote now · asked again every 30 s", "котировки сейчас нет · спрошу снова через 30 с") }

    /// The moves `routeSteps` hands on: nil for one drawn as one step.
    func cardRoute(_ c: WalletCommand) -> [Board.Step]? {
        switch c {
        case .buyStock(let s, let u?): return stockBuy(s, u)
        case .sellStock(let s, let x): return stockSale(s, x)
        case .buyOn(let place, let token, let u?, let src): return evmTrade(place, token, usdc: u, sell: nil, from: src, keep: false)
        case .sellOn(let place, let token, let x, let keep): return evmTrade(place, token, usdc: nil, sell: x, from: nil, keep: keep)
        case .swapOn(let place, let a, let b, let x): return pair(c, place, a, b, x)
        case .crossSwap(let a, let src, let b, let dst, let x): return cross(c, a, src, b, dst, x)
        case .revokeApproval(let place, let token, let spender, let permit2):
            let symbol = tokenName(token, on: place), short = "\(spender.prefix(6))…\(spender.suffix(4))"
            let who = EVMNetwork.named(place).flatMap { Executor.spenderName(spender, chainId: $0.rpc.chainId) } ?? short
            return [st(say("TOKEN · ", "ТОКЕН · ") + WalletCommand.placeName(place).uppercased(), symbol, permit2 ? say("through Permit2", "через Permit2") : say("its own approve", "его собственное разрешение")),
                    st(say("REVOKE · TO ZERO", "ОТЗЫВ · В НОЛЬ"), who, say("\(short) can't spend it after", "\(short) больше не сможет его тратить"))]
        case .buyNear(let u?): return nearBuy(u)
        case .sellNear(let x, let dst): return nearSale(x, to: dst)
        case .sendNear(let name, let x): return nearOut(name, x, close: false)
        case .closeNear(let name): return nearOut(name, nil, close: true)
        case .pay: return payment(c)
        case .multisigCreate: return multisigCreation()
        case .multisigSend: return multisigSending(c)
        case .cctp(let src, let dst, let x):
            let have = usdcFree(on: src), moving = x.map { min($0, have ?? $0) } ?? have
            let shown = moving.map { "\(Fmt.num($0)) USDC" } ?? say("all USDC", "всё USDC")
            return [st(from(src), shown, say("USDC on \(WalletCommand.placeName(src))", "USDC в \(WalletCommand.placeName(src))"), moved("USDC", on: src, by: moving.map { -$0 })),
                    st(say("BURN & MINT · CIRCLE'S CCTP", "СЖЕЧЬ И ВЫПУСТИТЬ · CCTP CIRCLE"), shown, say("≈ 1.3 bp + ≈ $0.08 of gas · Circle mints there", "≈ 0.013% + ≈ $0.08 газа · Circle выпускает там")),
                    st(to(dst), moving.map { "≈ \(Fmt.num($0)) \(dst == "robinhood" ? "USDG" : "USDC")" } ?? "USDC", say("your own address ✓", "твой собственный адрес ✓"),
                       moving.flatMap { moved(dst == "robinhood" ? "USDG" : "USDC", on: dst, by: $0, approx: true) })]
        case .usdt(let src, let dst, let x, let layerZero): return usdt(c, src, dst, x, layerZero: layerZero)
        case .aaveBorrow, .aaveRepay, .aaveCollateral: return aaveDebt(c)
        default: return nil
        }
    }

    /// What pays a stock buy: the USDG on Robinhood Chain, and what's missing brought from Solana, or straight from the EVM
    /// network holding USDC when Solana is short. A few cents short ($1 or 2% at most) isn't worth a bridge: the buy spends
    /// what's there (Executor.buyStock).
    func stockFunding(_ asked: Decimal) -> (usdg: Decimal, solana: Decimal, fits: Bool, usdc: Decimal, bring: Decimal, elsewhere: (net: EVMNetwork, send: Decimal)?) {
        let usdg = usdcFree(on: "robinhood") ?? 0, solana = usdcFree(on: "solana") ?? 0
        let fits = usdg > 0 && usdg < asked && asked - usdg <= max(1, asked / 50)
        let usdc = fits ? usdg : asked, bring = max(0, usdc - usdg)
        let elsewhere = bring > 0 ? EVMNetwork.source(arrive: bring, onSolana: solana, target: "robinhood", held: { self.usdcFree(on: $0) }) : nil
        return (usdg, solana, fits, usdc, bring, elsewhere)
    }

    /// A Robinhood Stock Token (or a token by its contract) bought on Robinhood Chain: the USDG there pays first, Solana or
    /// the EVM network holding USDC brings the rest over Relay, an exact approve, and the way that gives the most.
    private func stockBuy(_ s: String, _ asked: Decimal) -> [Board.Step] {
        let ways = quotes(.buyStock(symbol: s, usdc: asked)), symbol = tokenName(s, on: "robinhood")
        let price = s.hasPrefix("0x") ? rowPrice(s, on: "robinhood") : (markets["xyz:\(s)"] ?? markets["xyz:\(s.uppercased())"])?.markPx ?? rowPrice(s, on: "robinhood")
        let (usdg, solana, fits, usdc, bring, elsewhere) = stockFunding(asked)
        var steps: [Board.Step]
        if bring == 0 {
            steps = [st(say("FROM · ROBINHOOD CHAIN", "ОТКУДА · ROBINHOOD CHAIN"), "\(Fmt.num(usdc)) USDG",
                        fits ? say("what's there · the cents short aren't worth a bridge", "что есть · ради нехватки в центы мост не нужен") : say("already there · no bridge", "уже там · без моста"),
                        moved("USDG", on: "robinhood", by: -usdc))]
        } else if let e = elsewhere {
            steps = [st(from(e.net.key), "≈ \(Fmt.num(e.send)) USDC",
                        say("Solana has only \(Fmt.num(solana)) · free there: \(Fmt.num(usdcFree(on: e.net.key) ?? 0)) USDC",
                            "на Solana только \(Fmt.num(solana)) · свободно там: \(Fmt.num(usdcFree(on: e.net.key) ?? 0)) USDC"),
                        moved("USDC", on: e.net.key, by: -e.send, approx: true))]
        } else {
            steps = [st(from("solana"), "\(Fmt.num(bring)) USDC",
                        (usdg > 0 ? say("the rest of \(Fmt.num(usdc)) is on Robinhood · ", "остальное из \(Fmt.num(usdc)) — на Robinhood · ") : "")
                            + (usdcFree(on: "solana").map { say("free: \(Fmt.num($0)) USDC", "свободно: \(Fmt.num($0)) USDC") } ?? ""),
                        moved("USDC", on: "solana", by: -bring))]
        }
        if bring > 0 {
            steps.append(st(say("BRIDGE · RELAY", "МОСТ · RELAY"), elsewhere.map { "\($0.net.name) → Robinhood" } ?? "USDG → Robinhood",
                            (elsewhere == nil ? "" : say("straight, no stop on Solana · ", "напрямую, без Solana · ")) + say("+ ETH for gas when there's none", "+ ETH на газ, если его нет"),
                            moved("USDG", on: "robinhood", by: bring, approx: true)))
        }
        let best = ways?.first
        steps.append(best?.way == "v4"
            ? st(say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), "≈ \(Fmt.num(usdc)) USDG", say("exactly this · Permit2 lets Uniswap v4 take it for 30 min", "ровно столько · Permit2 даёт Uniswap v4 взять это на 30 мин"))
            : approve("≈ \(Fmt.num(usdc)) USDG", for: best?.way == "relay" ? "Relay" : "Uniswap"))
        var buy: Board.Step
        if let ways, let best {
            let others = ways.dropFirst().map { "\(RouteEdit.stockWayName($0.way)) ≈ \($0.gets)" }
            buy = st(say("BUY · ", "ПОКУПКА · ") + RouteEdit.stockWayName(best.way).uppercased() + say(" · BEST", " · ЛУЧШИЙ"), symbol,
                     "≈ \(best.gets)" + (others.isEmpty ? say(" · the only way that quotes it", " · единственный путь с котировкой") : " · " + others.joined(separator: " · ")))
        } else if ways != nil {
            buy = st(say("BUY · UNISWAP", "ПОКУПКА · UNISWAP"), symbol, price.map { p in
                say("≈ \(Fmt.num(usdc / p, maxFraction: 2)) \(symbol) worth ≈ \(Fmt.usd(usdc)) · 1 \(symbol) = $\(Fmt.price(p))",
                    "≈ \(Fmt.num(usdc / p, maxFraction: 2)) \(symbol) на ≈ \(Fmt.usd(usdc)) · 1 \(symbol) = $\(Fmt.price(p))") } ?? say("at the pool price", "по цене пула"))
        } else {
            buy = st(say("BUY · COMPARING", "ПОКУПКА · СРАВНИВАЮ"), symbol, say("Uniswap v4, v3 and Relay’s swap…", "Uniswap v4, v3 и обмен Relay…"))
        }
        if let got = Self.quotedAmount(best?.gets) ?? price.flatMap({ $0 > 0 ? usdc / $0 : nil }) { buy.money = gained(s, symbol, on: "robinhood", by: got) }
        return steps + [buy]
    }

    /// A stock token sold on Robinhood Chain: approved exactly, sold the way that gives the most, USDG back there.
    private func stockSale(_ s: String, _ x: Decimal?) -> [Board.Step] {
        let name = tokenName(s, on: "robinhood"), held = heldThere(s, on: "robinhood"), amount = x ?? held
        let price = (markets["xyz:\(s)"] ?? markets["xyz:\(s.uppercased())"])?.markPx ?? rowPrice(s, on: "robinhood")
        let usd = amount.flatMap { a in price.map { a * $0 } }
        let ways = quotes(.sellStock(symbol: s, amount: x)), best = ways?.first
        var sell = st(say("SELL · ", "ПРОДАЖА · ") + (best.map { RouteEdit.stockWayName($0.way).uppercased() + say(" · BEST", " · ЛУЧШИЙ") } ?? "UNISWAP"),
                      usd.map { say("≈ \(Fmt.usd($0)) of \(name)", "≈ \(Fmt.usd($0)) в \(name)") } ?? (x.map { "\(Fmt.num($0, maxFraction: 6)) \(name)" } ?? say("all \(name)", "всё \(name)")),
                      amount.map { "\(Fmt.num($0, maxFraction: 6)) \(name) · Robinhood Chain" } ?? "Robinhood Chain", moved(s, on: "robinhood", by: x.map { -$0 }))
        if let ways, let best {
            let others = ways.dropFirst().map { "\(RouteEdit.stockWayName($0.way)) ≈ \($0.gets)" }
            sell.lines.insert("≈ \(best.gets)" + (others.isEmpty ? "" : " · " + others.joined(separator: " · ")), at: 0)
        }
        let back = Self.quotedAmount(best?.gets) ?? usd
        return [approve(amount.map { "\(Fmt.num($0, maxFraction: 6)) \(name)" } ?? say("all \(name)", "всё \(name)"), for: best?.way == "relay" ? "Relay" : "Uniswap"),
                sell,
                st(say("TO · USDG", "КУДА · USDG"), back.map { "≈ \(Fmt.num($0.rounded(2, .plain))) USDG" } ?? "USDG",
                   say("on Robinhood Chain · then “move USDG from Robinhood to Solana”", "в Robinhood Chain · потом «move USDG from Robinhood to Solana»"),
                   back.flatMap { moved("USDG", on: "robinhood", by: $0, approx: true) })]
    }

    /// A token on an EVM network (the Mac's swapRoute): bought with the USDC already there (or brought straight from the
    /// network holding it), else with USDC from Solana by Relay's swap or Relay's bridge and Uniswap; sold back to Solana,
    /// or for USDC that stays there (`keep`).
    /// Where a buy or sale on an EVM network pays from: `payer`, another EVM network whose USDC Relay brings first; `here`,
    /// done on the network itself with Uniswap (the USDC there, or brought from that network) — else Relay's ways from Solana.
    func evmTradeSource(_ place: String, usdc: Decimal?, from src: String?, keep: Bool, command: WalletCommand) -> (payer: String?, here: Bool) {
        let quoted = quotes(command), key = RouteEdit.wayKey(command)
        let picked = key.flatMap { k in routeWay?.key == k ? routeWay?.way : nil }
        // Solana short and the network without the USDC: it comes straight from the EVM network holding it (EVMNetwork.source).
        let auto = usdc.flatMap { u in
            src == nil && picked == nil && (usdcFree(on: place) ?? 0) < u
                ? EVMNetwork.source(arrive: u, onSolana: usdcFree(on: "solana") ?? 0, target: place, held: { self.usdcFree(on: $0) })?.net.key : nil
        }
        let payer = (src ?? auto).flatMap { $0 == "solana" || $0 == place ? nil : $0 }
        let here = keep || ((src ?? auto) != nil && (src ?? auto) != "solana")
            || (usdc.map { u in quoted?.first.map { $0.way == "here" } ?? (src != "solana" && (usdcFree(on: place) ?? 0) >= u) } ?? false)
        return (payer, here)
    }

    private func evmTrade(_ place: String, _ token: String, usdc: Decimal?, sell: Decimal?, from src: String?, keep: Bool) -> [Board.Step] {
        let name = WalletCommand.placeName(place), net = name.uppercased(), label = tokenName(token, on: place)
        let command: WalletCommand = usdc != nil ? .buyOn(place, token: token, usdc: usdc, from: src) : .sellOn(place, token: token, amount: sell, keep: keep)
        let quoted = quotes(command), key = RouteEdit.wayKey(command)
        let picked = key.flatMap { k in routeWay?.key == k ? routeWay?.way : nil }
        let (payer, here) = evmTradeSource(place, usdc: usdc, from: src, keep: keep, command: command)
        let moving = usdc.map { "\(Fmt.num($0)) USDC" } ?? (sell.map { "\(Fmt.num($0, maxFraction: 8)) \(label)" } ?? say("all \(label)", "всё \(label)"))
        if here {
            let gets = quoted?.first { $0.way == "here" }?.gets
            var start: [Board.Step]
            if let payer {
                start = [st(from(payer), moving, usdcFree(on: payer).map { say("free there: \(Fmt.num($0)) USDC", "свободно там: \(Fmt.num($0)) USDC") } ?? "",
                            usdc.flatMap { moved("USDC", on: payer, by: -$0) }),
                         st(say("BRIDGE · RELAY", "МОСТ · RELAY"), "USDC → \(name)", say("a few seconds · no stop on Solana", "несколько секунд · без Solana"),
                            usdc.flatMap { moved("USDC", on: place, by: $0, approx: true) })]
            } else if let u = usdc {
                start = [st(from(place), "\(Fmt.num(u)) USDC", say("already there · no bridge", "уже там · без моста"), moved("USDC", on: place, by: -u))]
            } else {
                start = [st(from(place), moving, say("on \(name)", "в \(name)"), moved(token, on: place, by: sell.map { -$0 }))]
            }
            if let u = usdc {
                let approved = payer == nil ? approve("\(Fmt.num(u)) USDC", for: "Uniswap")
                    : st(say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), "\(Fmt.num(u)) USDC", say("what arrives · only Uniswap may take it", "то, что придёт · взять может только Uniswap"))
                return start + [approved,
                                st(say("BUY · UNISWAP ON ", "ПОКУПКА · UNISWAP В ") + net, label, gets.map { "≈ \($0)" } ?? say("at the pool price", "по цене пула"),
                                   Self.quotedAmount(gets).map { gained(token, label, on: place, by: $0) })]
            }
            return start + [approve(moving, for: "Uniswap"),
                            st(say("TO · USDC ON ", "КУДА · USDC В ") + net, gets.map { "≈ \($0)" } ?? "USDC", say("stays on \(name) · your own address ✓", "остаётся в \(name) · твой собственный адрес ✓"),
                               Self.quotedAmount(gets).flatMap { moved("USDC", on: place, by: $0, approx: true) })]
        }
        // The way picked in the step's details, else the one that leaves more now.
        let way: Board.Step = switch picked {
        case "relay": st(say("RELAY SWAP", "ОБМЕН RELAY"), say("one step", "один шаг"),
                         usdc != nil ? say("Relay swaps on the way · one Solana signature", "Relay меняет по пути · одна подпись в Solana") : say("Relay swaps it back to USDC on Solana", "Relay меняет обратно в USDC на Solana"))
        case "uniswap": st("RELAY + UNISWAP", usdc != nil ? say("bridge, then Uniswap", "мост, потом Uniswap") : say("Uniswap, then bridge", "Uniswap, потом мост"),
                           usdc != nil ? say("USDC to \(name), then Uniswap with an exact approve", "USDC в \(name), потом Uniswap с точным разрешением")
                                       : say("Uniswap on \(name), then Relay's bridge to Solana", "Uniswap в \(name), потом мост Relay на Solana"))
        default: bestWay(quoted)
        }
        let viaUniswap = (picked ?? quoted?.first?.way) == "uniswap", gets = quoted?.first?.gets
        if let u = usdc {
            return [st(from("solana"), "\(Fmt.num(u)) USDC", usdcFree(on: "solana").map { say("free: \(Fmt.num($0)) USDC", "свободно: \(Fmt.num($0)) USDC") } ?? "", moved("USDC", on: "solana", by: -u)), way]
                + (viaUniswap ? [st(say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), "≈ \(Fmt.num(u)) USDC", say("exactly what arrives · only Uniswap may take it", "ровно то, что придёт · взять может только Uniswap"))] : [])
                + [st(say("BUY · ON ", "ПОКУПКА · В ") + net, label, gets.map { say("≈ \($0) arrives", "придёт ≈ \($0)") } ?? say("at the best price found", "по лучшей найденной цене"),
                      Self.quotedAmount(gets).map { gained(token, label, on: place, by: $0) })]
        }
        return [st(from(place), moving, say("\(EVMNetwork.named(place)?.gasCoin ?? "ETH") for gas comes along when there’s none", "\(EVMNetwork.named(place)?.gasCoin ?? "ETH") на газ придёт, если его нет"),
                   moved(token, on: place, by: sell.map { -$0 }))]
            + (viaUniswap ? [approve(moving, for: "Uniswap")] : [])
            + [way, st(say("TO · SOLANA", "КУДА · SOLANA"), "USDC", gets.map { say("≈ \($0) · your wallet", "≈ \($0) · твой кошелёк") } ?? say("your wallet", "твой кошелёк"),
                       Self.quotedAmount(gets).flatMap { moved("USDC", on: "solana", by: $0, approx: true) })]
    }

    nonisolated static func wayName(_ way: String) -> String { way == "relay" ? "Relay swap" : "Relay + Uniswap" }

    /// The way that leaves more right now and by how much it beats the other; comparing while both are quoted.
    private func bestWay(_ ways: [RouteEdit.Way]?) -> Board.Step {
        let label = say("RELAY OR UNISWAP · COMPARING", "RELAY ИЛИ UNISWAP · СРАВНИВАЮ")
        guard let ways else { return st(label, say("comparing…", "сравниваю…"), say("Relay swap vs Relay + Uniswap", "обмен Relay или Relay + Uniswap")) }
        guard let best = ways.first else { return st(label, say("no quote now", "котировки нет"), say("asking again every 30 s", "спрошу снова через 30 с")) }
        let other = Self.wayName(best.way == "relay" ? "uniswap" : "relay")
        func dollars(_ s: String) -> Decimal? { Decimal(string: s.filter { $0.isNumber || $0 == "." }) }
        let margin = ways.dropFirst().first.flatMap { o in dollars(best.worth).flatMap { b in dollars(o.worth).map { b - $0 } } }
        let line = switch margin {
        case nil: say("\(other) doesn't quote it now", "\(other) сейчас не даёт котировку")
        case let m? where m < Decimal(string: "0.01")!: say("about the same as \(other)", "примерно как \(other)")
        case let m?: say("\(Fmt.usd(m)) more than \(other)", "на \(Fmt.usd(m)) больше, чем \(other)")
        }
        return st(Self.wayName(best.way).uppercased() + say(" · BEST NOW", " · ЛУЧШЕ СЕЙЧАС"), "≈ \(best.gets)", line)
    }

    /// One token for another off Solana (the Mac's pairRoute): NEAR Intents on NEAR, two spot orders on Hyperliquid,
    /// Tempo's exchange (or Relay's swap there), and Uniswap's best on an EVM network; what comes out once quoted.
    private func pair(_ c: WalletCommand, _ place: String, _ a: String, _ b: String, _ x: Decimal?) -> [Board.Step]? {
        let quoted = quotes(c), gets = quoted?.first.map { "≈ \($0.gets)" }
        let moving = x.map { "\(Fmt.num($0, maxFraction: 8)) \(tokenName(a, on: place))" } ?? say("all \(tokenName(a, on: place))", "всё \(tokenName(a, on: place))")
        let toName = tokenName(b, on: place)
        switch place {
        case "near":
            let near = heldThere("NEAR", on: "near")
            return [st(from("near"), moving, say("your NEAR account", "твой аккаунт NEAR"), a.uppercased() == "NEAR" ? moved("NEAR", on: "near", by: -(x ?? near ?? 0)) : nil),
                    st(say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), toName, gets ?? quoting),
                    st(to("near"), toName, say("your NEAR account ✓", "твой аккаунт NEAR ✓"),
                       b.uppercased() == "NEAR" ? Self.quotedAmount(gets).flatMap { moved("NEAR", on: "near", by: $0, approx: true) } : nil)]
        case "hyperliquid":
            return [st(say("FROM · SPOT BALANCE", "ОТКУДА · СПОТОВЫЙ БАЛАНС"), moving, x == nil ? say("all available · on Hyperliquid", "всё, что есть · на Hyperliquid") : say("on Hyperliquid", "на Hyperliquid"),
                       moved(a, on: "hyperliquid", by: x.map { -$0 })),
                    st(say("SELL · SPOT ORDER", "ПРОДАЖА · СПОТ"), "→ USDC", say("IOC · at most 1% under the mid", "IOC · не дешевле середины на 1%")),
                    st(say("BUY · SPOT ORDER", "ПОКУПКА · СПОТ"), toName, gets.map { say("\($0) · at the mids", "\($0) · по серединам") } ?? say("IOC · at most 1% over the mid", "IOC · не дороже середины на 1%"),
                       Self.quotedAmount(gets).map { gained(b, toName, on: "hyperliquid", by: $0) })]
        case "tempo":
            // Where Tempo's exchange has no liquidity for the pair, Relay's swap on Tempo is the way the quote found.
            let relay = quoted?.first?.way == "Relay's swap"
            let label = relay ? say("SWAP · RELAY ON TEMPO", "ОБМЕН · RELAY В TEMPO") : say("SWAP · TEMPO'S EXCHANGE", "ОБМЕН · БИРЖА TEMPO")
            return [st(from("tempo"), moving, x == nil ? say("all available · on Tempo", "всё, что есть · в Tempo") : say("on Tempo", "в Tempo"), moved(a, on: "tempo", by: x.map { -$0 })),
                    approve(moving, for: relay ? "Relay" : say("Tempo's exchange", "биржа Tempo")),
                    st(label, toName, quoted.map { $0.first.map { "≈ \($0.gets)" } ?? say("no liquidity now · asked again every 30 s", "ликвидности сейчас нет · спрошу снова через 30 с") } ?? quoting,
                       Self.quotedAmount(gets).map { gained(b, toName, on: "tempo", by: $0) })]
        default:
            guard let net = EVMNetwork.named(place) else { return nil }
            let coinIn = a.uppercased() == net.gasCoin.uppercased(), name = WalletCommand.placeName(place)
            var steps = [st(from(place), moving, coinIn ? say("your \(net.gasCoin) · its fees stay", "твой \(net.gasCoin) · на комиссии остаётся") : say("on \(name)", "в \(name)"),
                            moved(a, on: place, by: x.map { -$0 }))]
            if !coinIn { steps.append(approve(moving, for: "Uniswap")) }
            steps.append(st(say("SWAP · ", "ОБМЕН · ") + (quoted?.first?.way ?? "Uniswap").uppercased() + say(" ON ", " В ") + name.uppercased(), toName,
                            gets ?? say("the best of v2, v3 and v4", "лучший из v2, v3 и v4"), Self.quotedAmount(gets).map { gained(b, toName, on: place, by: $0) }))
            return steps
        }
    }

    /// A token on one network for one on another (the Mac's crossRoute): Relay's swap across, with an exact approve for a
    /// token on an EVM network; NEAR Intents for NEAR or a pair only 1Click quotes.
    private func cross(_ c: WalletCommand, _ a: String, _ src: String, _ b: String, _ dst: String, _ x: Decimal?) -> [Board.Step] {
        let quoted = quotes(c), moving = x.map { "\(Fmt.num($0, maxFraction: 8)) \(tokenName(a, on: src))" } ?? say("all \(tokenName(a, on: src))", "всё \(tokenName(a, on: src))")
        let toName = tokenName(b, on: dst), own = say("your own address there", "твой собственный адрес там")
        let line = quoted.map { $0.first.map { "≈ \($0.gets) · \($0.way)" } ?? noQuote } ?? quoting
        let out = src == "near" && a.uppercased() == "NEAR" ? moved("NEAR", on: "near", by: -(x ?? heldThere("NEAR", on: "near") ?? 0)) : moved(a, on: src, by: x.map { -$0 })
        let arrives = Self.quotedAmount(quoted?.first?.gets).map { gained(b, toName, on: dst, by: $0) }
        if src == "near" || dst == "near" || quoted?.first?.way.hasPrefix("NEAR Intents") == true {
            return [st(from(src), moving, own, out), st(say("SWAP ACROSS · NEAR INTENTS", "ОБМЕН МЕЖДУ СЕТЯМИ · NEAR INTENTS"), toName, line), st(to(dst), toName, own, arrives)]
        }
        let coinIn = a.uppercased() == (src == "solana" ? "SOL" : EVMNetwork.named(src)?.gasCoin.uppercased() ?? "ETH")
        var steps = [st(from(src), moving, coinIn ? say("your \(a.uppercased()) · its fees stay", "твой \(a.uppercased()) · на комиссии остаётся") : say("on \(WalletCommand.placeName(src))", "в \(WalletCommand.placeName(src))"), out)]
        if src != "solana", !coinIn { steps.append(approve(moving, for: "Relay")) }
        return steps + [st(say("SWAP ACROSS · RELAY", "ОБМЕН МЕЖДУ СЕТЯМИ · RELAY"), toName, line), st(to(dst), toName, own, arrives)]
    }

    /// USDC → native NEAR through NEAR Intents (1Click): from Solana, or straight from the EVM network holding it when
    /// Solana is short — the executor's own rule (Executor.nearSource).
    private func nearBuy(_ u: Decimal) -> [Board.Step] {
        let onSolana = usdcFree(on: "solana") ?? 0
        let src = Executor.nearSource(usdc: u, onSolana: onSolana, held: { self.usdcFree(on: $0) })
        let near = markets["NEAR"].flatMap { $0.markPx > 0 ? u / $0.markPx : nil }
        return [st(from(src?.key ?? "solana"), "\(Fmt.num(u)) USDC",
                   src.map { say("Solana has only \(Fmt.num(onSolana)) · straight from \($0.name), no bridge", "на Solana только \(Fmt.num(onSolana)) · напрямую из \($0.name), без моста") } ?? "",
                   moved("USDC", on: src?.key ?? "solana", by: -u)),
                st(say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), "1Click", say("the quote is checked before signing", "котировку проверю перед подписью")),
                st(to("near"), near.map { "≈ \(Fmt.num($0, maxFraction: 4)) NEAR" } ?? "NEAR", say("your own NEAR account ✓", "твой собственный аккаунт NEAR ✓"),
                   near.flatMap { moved("NEAR", on: "near", by: $0, approx: true) })]
    }

    /// Native NEAR → USDC through 1Click, on Solana or at our address on another network; «all» keeps ≈ 0.09 NEAR for
    /// storage and prepaid gas, most of it back after the sale.
    private func nearSale(_ asked: Decimal?, to dst: String?) -> [Board.Step] {
        let amount = asked ?? max(0, (heldThere("NEAR", on: "near") ?? 0) - Decimal(string: "0.09")!)
        let usd = markets["NEAR"].map { amount * $0.markPx }, place = dst ?? "solana", name = WalletCommand.placeName(place)
        return [st(from("near"), "\(Fmt.num(amount, maxFraction: 4)) NEAR",
                   usd.map { say("≈ \(Fmt.usd($0)) · ≈ 0.09 NEAR stays for storage and gas, most of it back after", "≈ \(Fmt.usd($0)) · ≈ 0.09 NEAR остаётся на хранение и газ, большая часть вернётся") } ?? "",
                   moved("NEAR", on: "near", by: -amount)),
                st(say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), "1Click", say("the quote is checked before signing", "котировку проверю перед подписью")),
                st(say("ARRIVES · ≈ 30–40 S", "ПРИДЁТ · ≈ 30–40 С"), "→ \(name)", say("1Click sends the USDC over", "1Click переводит USDC")),
                st(to(place), usd.map { "≈ \(Fmt.num($0.rounded(2, .plain))) USDC" } ?? "USDC", say("your own address ✓", "твой собственный адрес ✓"),
                   usd.flatMap { moved("USDC", on: place, by: $0, approx: true) })]
    }

    /// NEAR to a saved NEAR account, or the whole NEAR account closed into it.
    private func nearOut(_ name: String, _ asked: Decimal?, close: Bool) -> [Board.Step]? {
        guard let e = savedAddress(name, on: "near") else { return nil }
        let near = heldThere("NEAR", on: "near") ?? 0
        let amount = close ? near : asked ?? max(0, near - Decimal(string: "0.012")!)
        let usd = markets["NEAR"].map { amount * $0.markPx }
        let rest = close ? say(" · everything", " · всё") : asked == nil ? say(" · ≈ 0.012 NEAR stays", " · ≈ 0.012 NEAR остаётся") : ""
        return [st(from("near"), "\(Fmt.num(amount, maxFraction: 4)) NEAR", usd.map { "≈ \(Fmt.usd($0))" + rest } ?? "", moved("NEAR", on: "near", by: -amount)),
                st(savedLabel, e.name, ends(e.address))]
    }

    /// A coin to a saved address on a network only NEAR Intents reaches: the USDC it costs and where from (once quoted),
    /// 1Click's swap for exactly that much, and the saved address.
    func acrossSteps(_ c: WalletCommand, _ r: (entry: SignerAddress, symbol: String, address: String), _ amount: Decimal?) -> [Board.Step] {
        let quoted = quotes(c), payer = quoted?.first?.way ?? "Solana"
        let key = EVMNetwork.all.first { $0.name == payer }?.key ?? "solana"
        let what = "\(amount.map { Fmt.num($0, maxFraction: 9) } ?? "?") \(r.symbol)"
        return [st(say("FROM · ", "ОТКУДА · ") + payer.uppercased(), "USDC",
                   quoted.map { $0.first.map { say("\($0.gets) · the rest comes back", "\($0.gets) · остаток вернётся") } ?? noQuote } ?? quoting,
                   Self.quotedAmount(quoted?.first?.gets).flatMap { moved("USDC", on: key, by: -$0, approx: true) }),
                st(say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), what, say("exactly this much arrives", "придёт ровно столько")),
                st(savedLabel, r.entry.name, ends(r.entry.address) + " · " + (AddressCheck.Network(rawValue: r.entry.network)?.title ?? r.entry.network))]
    }

    /// Where a top-up of Tempo comes from, as the executor's ensureEVM decides (the Mac's tempoSource): nil = Solana, when
    /// it holds enough; else the EVM network that does, with what it sends (a dollar over for Relay).
    func tempoSource(_ need: Decimal) -> (net: EVMNetwork, send: Decimal)? {
        EVMNetwork.source(arrive: need, onSolana: usdcFree(on: "solana") ?? 0, target: "tempo", held: { self.usdcFree(on: $0) })
    }

    /// What's short on Tempo for a paid request: a dollar's float and a cent for fees (Executor.tempoFloat); nil when enough.
    func tempoNeed(_ price: Decimal) -> Decimal? {
        let float = Decimal(Executor.tempoFloat) / 1_000_000
        return tempoUSDC < price + Decimal(string: "0.01")! ? max(price, float) - tempoUSDC + Decimal(string: "0.01")! : nil
    }

    /// A paid request (MPP): USDC on Tempo (a dollar's float first when short, from Solana or the network holding it),
    /// the payment, the service.
    private func payment(_ c: WalletCommand) -> [Board.Step]? {
        guard case .pay(_, let url, _) = c, let q = payment, q.command == c else { return nil }
        let ch = q.challenge, host = URL(string: url)?.host ?? ch.realm, price = Fmt.num(ch.usdc, maxFraction: 6)
        let there = tempoUSDC, float = Decimal(Executor.tempoFloat) / 1_000_000
        var first: Board.Step
        if let need = tempoNeed(ch.usdc) {
            let requests = NSDecimalNumber(decimal: (max(ch.usdc, float) / ch.usdc).rounded(0, .down)).intValue
            let src = tempoSource(need), key = src?.net.key ?? "solana", name = src?.net.name ?? "Solana", sends = src?.send ?? need
            first = st(say("FROM · ", "ОТКУДА · ") + name.uppercased() + " → TEMPO", "\(Fmt.num(sends, maxFraction: 6)) USDC",
                       say("a float for ≈ \(requests) requests like this · Relay ≈ $0.02", "запас на ≈ \(requests) таких запросов · Relay ≈ $0.02"), moved("USDC", on: key, by: -sends))
            first.lines.append(usdcFree(on: key).map { say("\(Fmt.num($0)) USDC on \(name)", "\(Fmt.num($0)) USDC в \(name)") } ?? say("from your Solana USDC", "из твоих USDC на Solana"))
        } else {
            first = st(from("tempo"), "\(price) USDC", say("your USDC on Tempo", "твои USDC в Tempo"),
                       Board.Money(place: "Tempo", symbol: "USDC", was: Self.amount(there, "USDC"), after: Self.amount(max(0, there - ch.usdc), "USDC")))
        }
        return [first, st(say("PAY · MPP", "ОПЛАТА · MPP"), "\(price) USDC", ch.purpose ?? say("one request", "один запрос")),
                st(say("TO · ", "КУДА · ") + q.service.uppercased(), say("the answer comes here", "ответ придёт сюда"), host)]
    }

    /// A multisig made: its keys checked, then one Safe per network at the same address.
    private func multisigCreation() -> [Board.Step]? {
        guard let d = multisigDraft else { return nil }
        func short(_ a: String) -> String { "\(a.prefix(6))…\(a.suffix(4))" }
        let check = Board.Step(label: say("CHECK · KEYS", "ПРОВЕРКА · КЛЮЧИ"), title: say("\(d.threshold) of \(d.keys.count) keys", "\(d.threshold) из \(d.keys.count) ключей"),
                               lines: d.keys.map { "\($0.name)\t\(short($0.address))" })
        return [check] + d.networks.compactMap(EVMNetwork.named).map { net in
            Board.Step(label: say("MULTISIG · ", "МУЛЬТИСИГ · ") + net.name.uppercased(), title: say("Create on \(net.name)", "Создать в \(net.name)"),
                       lines: [say("address\t", "адрес\t") + (d.address.map(short) ?? "—"), say("Safe 1.4.1 · fee from this computer", "Safe 1.4.1 · комиссия с этого компьютера")])
        }
    }

    /// A payment out of a multisig: each key here signs it, then this computer sends it and pays the gas.
    private func multisigSending(_ c: WalletCommand) -> [Board.Step]? {
        guard let p = multisigPayment, p.command == c, let net = EVMNetwork.named(p.net) else { return nil }
        let amount = "\(Fmt.num(p.amount, maxFraction: 6)) \(p.symbol)"
        let signs = p.signers.enumerated().map { i, k in
            Board.Step(label: say("SIGN · ", "ПОДПИСЬ · ") + (k.kind == "ledger" ? "LEDGER" : say("THIS COMPUTER", "ЭТОТ КОМПЬЮТЕР")),
                       title: say("Signature \(i + 1) of \(p.threshold)", "Подпись \(i + 1) из \(p.threshold)"),
                       lines: [say("to\t", "кому\t") + p.to.name, say("amount\t", "сумма\t") + amount, say("network\t", "сеть\t") + net.name])
        }
        return signs + [Board.Step(label: say("TO · ", "КУДА · ") + p.to.name.uppercased(), title: "\(amount) → \(p.to.name)",
                                   lines: [say("from the multisig \(p.multisig.address.prefix(6))…\(p.multisig.address.suffix(4))", "из мультисига \(p.multisig.address.prefix(6))…\(p.multisig.address.suffix(4))"),
                                           say("gas from this computer", "газ с этого компьютера")],
                                   // The multisig's own balance going down, as the Mac's card shows it.
                                   money: Board.Money(place: say("Multisig", "Мультисиг"), symbol: p.symbol, was: Self.amount(p.held, p.symbol),
                                                      after: Self.amount(max(0, p.held - p.amount), p.symbol)))]
    }

    nonisolated static func bridgeName(_ way: String) -> String { ["relay": "Relay", "cctp": "CCTP", "1click": "NEAR Intents", "layerzero": "LayerZero"][way] ?? way }

    /// The bridge step of a move between networks once its bridges are quoted: the one it goes by with what it lands, then
    /// a row for each bridge — what it costs and how long it takes. `fallback` until then.
    func bridgeStep(_ ways: [RouteEdit.Way]?, title: String, fallback: (label: String, line: String)) -> Board.Step {
        guard let ways, let taken = ways.first else { return st(fallback.label, title, fallback.line) }
        func parts(_ w: RouteEdit.Way) -> [String] { w.gets.components(separatedBy: " · ") }
        return Board.Step(label: say("BRIDGE · ", "МОСТ · ") + Self.bridgeName(taken.way).uppercased(), title: parts(taken).first ?? title,
                          lines: ways.map { "\(Self.bridgeName($0.way))\t" + parts($0).dropFirst().joined(separator: " · ") }
                            + [say("prices now · asked again before signing", "цены сейчас · перед подписью спрошу снова")])
    }

    /// USDT from one of USDT0's networks to another, both our own address: Relay's swap of it, or Tether's own bridge on
    /// LayerZero when asked by name.
    private func usdt(_ c: WalletCommand, _ src: String, _ dst: String, _ x: Decimal?, layerZero: Bool) -> [Board.Step] {
        let token = { (p: String) in EVMNetwork.named(p).flatMap { USDT0.on($0.rpc.chainId)?.token } }
        let have = token(src).flatMap { heldThere($0, on: src) } ?? heldThere("USDT", on: src)
        let moving = x.map { min($0, have ?? $0) } ?? have, shown = moving.map { "\(Fmt.num($0)) USDT" } ?? say("all USDT", "всё USDT")
        let wait = EVMNetwork.named(src).flatMap { USDT0.on($0.rpc.chainId) }.map { USDT0.wait($0.minutes) } ?? say("a while", "какое-то время")
        let quoted = quotes(c)
        return [st(from(src), shown, say("USDT on \(WalletCommand.placeName(src))", "USDT в \(WalletCommand.placeName(src))"), moved(token(src) ?? "USDT", on: src, by: moving.map { -$0 })),
                bridgeStep(quoted, title: shown, fallback: layerZero ? (say("BRIDGE · LAYERZERO", "МОСТ · LAYERZERO"), say("Tether's USDT0 · cents of gas · \(wait)", "USDT0 от Tether · центы газа · \(wait)"))
                                                                     : (say("BRIDGE · RELAY", "МОСТ · RELAY"), say("a few cents · seconds", "несколько центов · секунды"))),
                st(to(dst), quoted?.first?.gets.components(separatedBy: " · ").first ?? moving.map { "≈ \(Fmt.num($0)) USDT" } ?? "USDT", say("your own address ✓", "твой собственный адрес ✓"),
                   moving.flatMap { moved(token(dst) ?? "USDT", on: dst, by: $0, approx: !layerZero) })]
    }

    /// Borrowing on Aave, paying it back, or ETH supplied as its collateral: the collateral, the move with the health factor
    /// before and after (once quoted), where the money ends.
    private func aaveDebt(_ c: WalletCommand) -> [Board.Step]? {
        let x: Decimal?, net: String
        switch c {
        case .aaveBorrow(let a, let n), .aaveRepay(let a, let n), .aaveCollateral(let a, let n): (x, net) = (a, n)
        default: return nil
        }
        let name = WalletCommand.placeName(net), what = x.map { Fmt.num($0, maxFraction: 6) } ?? say("all", "всё")
        func quoted(_ way: String) -> String? { quotes(c)?.first { $0.way == way }?.gets }
        switch c {
        case .aaveRepay:
            return [st(from(net), "\(what) USDC", say("USDC on \(name)", "USDC в \(name)"), moved("USDC", on: net, by: x.map { -$0 })),
                    approve("\(what) USDC", for: "Aave"),
                    st(say("REPAY · AAVE", "ВОЗВРАТ · AAVE"), "\(what) USDC", say("the debt goes down, the collateral stays", "долг уменьшается, залог остаётся"))]
        case .aaveCollateral:
            return [st(from(net), "\(what) ETH", say("its fees stay", "на комиссии остаётся"), moved("ETH", on: net, by: x.map { -$0 })),
                    st(say("WRAP · WETH", "ОБЁРТКА · WETH"), "\(what) WETH", say("the coin as a token, one to one", "монета как токен, один к одному")),
                    st(say("COLLATERAL · AAVE", "ЗАЛОГ · AAVE"), "\(what) ETH", say("borrowable against, ≈ 80% of its value", "под него можно занять ≈ 80% стоимости"))]
        default:
            return [st(say("COLLATERAL · AAVE", "ЗАЛОГ · AAVE"), quoted("collateral") ?? say("reading…", "читаю…"), say("what you supplied on Aave on \(name)", "что ты внёс в Aave в \(name)")),
                    st(say("BORROW · AAVE", "ЗАЁМ · AAVE"), "\(what) USDC", quoted("health").map { say("health factor \($0)", "фактор здоровья \($0)") } ?? say("health factor: reading…", "фактор здоровья: читаю…")),
                    st(to(net), "\(what) USDC", say("your own address", "твой собственный адрес"), x.flatMap { moved("USDC", on: net, by: $0) })]
        }
    }

    /// Merkl's rewards claimed: each reward by network, the wallet it lands in; a sweep then sells each worth a dollar.
    func claimSteps(_ owed: [Merkl.Reward], _ c: WalletCommand) -> [Board.Step] {
        var steps: [Board.Step] = []
        for chain in Merkl.chains where owed.contains(where: { $0.chainId == chain }) {
            let net = EVMNetwork.of(chainId: chain), name = net?.name ?? "chain \(chain)"
            for r in owed where r.chainId == chain {
                steps.append(Board.Step(label: say("CLAIM · MERKL ON ", "ЗАБРАТЬ · MERKL В ") + name.uppercased(), title: "\(Fmt.num(r.claimable, maxFraction: 6)) \(r.symbol)",
                                        lines: (r.worthUSD.map { [say("worth\t≈ ", "стоит\t≈ ") + Fmt.usd($0)] } ?? []) + [say("from Merkl's distributor", "из распределителя Merkl")],
                                        money: net.map { gained(r.token, r.symbol, on: $0.key, by: r.claimable) }))
            }
            steps.append(st(say("TO · ", "КУДА · ") + name.uppercased(), say("this wallet", "этот кошелёк"), say("the rewards stay there as tokens", "награды остаются там токенами")))
        }
        if case .sweepRewards(_, let into) = c {
            for r in Merkl.toSell(owed, into: into) {
                let name = EVMNetwork.of(chainId: r.chainId)?.name ?? "chain \(r.chainId)"
                steps.append(Board.Step(label: say("SELL · ON ", "ПРОДАЖА · В ") + name.uppercased(), title: "\(Fmt.num(r.claimable, maxFraction: 6)) \(r.symbol)",
                                        lines: [say("for USDC\ton Solana", "за USDC\tна Solana")] + (into.map { [say("then buys\t\($0) on \(name)", "потом покупает\t\($0) в \(name)")] } ?? [])))
            }
        }
        return steps
    }

    /// USDC to a saved address on its network, as every other transfer is drawn.
    func transferSteps(_ e: SignerAddress, _ amount: Decimal) -> [Board.Step] {
        let net = e.network, free = usdcFree(on: net)
        let line = free.map { $0 < amount && net != "solana" ? say("\(Fmt.num(amount - $0)) USDC short: it comes from Solana first", "не хватает \(Fmt.num(amount - $0)) USDC: сначала придут с Solana")
            : net == "hyperliquid" ? say("perp balance · no bridge, no gas", "баланс perp · без моста и газа") : say("free: \(Fmt.num($0)) USDC", "свободно: \(Fmt.num($0)) USDC") } ?? ""
        // What Solana gives when the money there is short (the Mac's `bridged`): its balance going down, a second row in the
        // same box under this network's.
        let short = max(0, amount - (free ?? amount))
        let topUp = net != "solana" && short > 0 && (usdcFree(on: "solana") ?? 0) >= short ? moved("USDC", on: "solana", by: -short, approx: true) : nil
        var from = st(say("FROM · ", "ОТКУДА · ") + (AddressCheck.Network(rawValue: net)?.title ?? WalletCommand.placeName(net)).uppercased(), "\(Fmt.num(amount)) USDC", line,
                      moved("USDC", on: net, by: -amount))
        if let m = topUp { from.lines.append("\(m.place)\t\(m.was) → \(m.after) \(m.symbol)") }
        return [from, st(savedLabel, e.name, ends(e.address))]
    }

    /// A position closed (all or a share): what it makes or loses now, its size and entry, the USDC back on Hyperliquid.
    func closeSteps(_ share: Decimal, coin: String) -> [Board.Step]? {
        guard let p = positions.first(where: { $0.coin == coin }) else { return nil }
        let name = coin.hasPrefix("xyz:") ? String(coin.dropFirst(4)) : coin, long = p.size > 0, pnl = p.unrealizedPnl * share
        return [Board.Step(label: say("CLOSE · ", "ЗАКРЫТИЕ · ") + name.uppercased() + " " + (long ? say("LONG", "ЛОНГ") : say("SHORT", "ШОРТ")),
                           title: "\(pnl < 0 ? "−" : "+")\(Fmt.usd(abs(pnl)))",
                           lines: [say("share\t", "доля\t") + Fmt.pct(share * 100), say("size\t", "размер\t") + "\(Fmt.num(abs(p.size) * share, maxFraction: 6)) \(name)",
                                   say("entry\t", "вход\t") + Fmt.price(p.entry)] + (markets[coin].map { [say("now\t≈ ", "сейчас\t≈ ") + Fmt.price($0.markPx)] } ?? [])),
                st(say("TO · HYPERLIQUID", "КУДА · HYPERLIQUID"), "USDC", say("back to the trading balance", "обратно на торговый баланс"))]
    }
}

/// Contracts' symbols looked up for a route's labels (the Mac's tokenNames), each asked once.
enum ContractNames {
    static var known: [String: String] = [:]
    static var asked: Set<String> = []
}

// MARK: The footer under each route

extension Engine {
    /// What the route signs and how it's checked, from what the route does now (the Mac's MoveRoute footers,
    /// Canvases.swift): where the money comes from, the way the quote took, a coin going in as itself. The rest as
    /// RouteEdit.note says it. With «One transaction for approve and swap» on, a footer that counts an approve says the
    /// two go out as one (the Mac's MoveRoute.init).
    func routeNote(_ c: WalletCommand) -> String? {
        guard let note = ownNote(c) ?? RouteEdit.note(c, say: { say($0, $1) }) else { return nil }
        return Executor.batchesOn && (note.contains("approve") || note.contains("разрешени"))
            ? note + say(" With «One transaction for approve and swap» on, the approve and what spends it go out as one transaction under one Windows Hello.",
                         " Включена «Одна транзакция для approve и обмена»: разрешение и то, что его тратит, уйдут одной транзакцией под одним Windows Hello.")
            : note
    }

    private func ownNote(_ c: WalletCommand) -> String? {
        func name(_ k: String) -> String { WalletCommand.placeName(k) }
        let confirm = say(" Press Confirm.", " Нажми «Подтвердить».")
        switch c {
        case .buyOn(let place, _, let u?, let src): return hereNote(c, place, usdc: u, from: src, keep: false)
        case .sellOn(let place, _, _, let keep): return hereNote(c, place, usdc: nil, from: nil, keep: keep)
        case .buyStock(let s, let u?):
            let f = stockFunding(u)
            let start = f.bring == 0 ? say("Two signatures on Robinhood Chain with the USDG already there: ", "Две подписи в Robinhood Chain с USDG, которые уже там: ")
                : f.elsewhere.map { say("An exact approve and Relay's deposit on \($0.net.name), then two on Robinhood Chain: ", "Точное разрешение и депозит Relay в \($0.net.name), потом две в Robinhood Chain: ") }
                    ?? say("One Solana signature for the bridge, then two on Robinhood Chain: ", "Одна подпись в Solana для моста, потом две в Robinhood Chain: ")
            return start
                + say("an exact USDG approve and the swap the best way takes (Uniswap v4, v3 or Relay’s swap, compared in the step), which may only pay this wallet. ",
                      "точное разрешение на USDG и обмен лучшим путём (Uniswap v4, v3 или обмен Relay — сравнение в шаге), который может платить только этому кошельку. ")
                + (s.hasPrefix("0x")
                    ? say("The token is taken by its contract: it must trade on Robinhood Chain with $50k+ of liquidity, and the price must sit within 5% of DexScreener's.",
                          "Токен взят по контракту: он должен торговаться в Robinhood Chain с ликвидностью от $50 тыс., а цена — не дальше 5% от DexScreener.")
                    : say("The token is Robinhood's registered contract, checked on-chain, and the price must sit within Robinhood's own quote.",
                          "Токен — зарегистрированный контракт Robinhood, проверенный в сети, а цена должна быть в пределах собственной котировки Robinhood."))
                + confirm
        case .swapOn("tempo", _, _, _):
            let relay = quotes(c)?.first?.way == "Relay's swap"
            return say("An exact approve to \(relay ? "Relay's swap on Tempo" : "Tempo's own exchange"), then the swap at no less than \(relay ? "Relay's checked quote" : "0.1% under its quote"), paying only this wallet; Tempo's fees come out of the USDC, a tenth of a cent each.",
                       "Точное разрешение для \(relay ? "обмена Relay в Tempo" : "собственной биржи Tempo"), потом обмен не хуже \(relay ? "проверенной котировки Relay" : "котировки на 0,1%"), платит только этому кошельку; комиссии Tempo — из USDC, по десятой цента.") + confirm
        case .swapOn(let place, let a, let b, _) where !["solana", "near", "hyperliquid"].contains(place):
            guard let net = EVMNetwork.named(place) else { return nil }
            let coin = net.gasCoin, coinIn = a.uppercased() == coin.uppercased(), coinOut = b.uppercased() == coin.uppercased(), n = name(place)
            return say("One swap on \(n), no USDC in the middle: the best of Uniswap v2, v3 and v4, at most 0.5–3% under its quote by the price impact, paying only this wallet.",
                       "Один обмен в \(n), без USDC посередине: лучший из Uniswap v2, v3 и v4, не хуже котировки больше чем на 0,5–3% по влиянию на цену, платит только этому кошельку.")
                + (coinIn ? say(" \(coin) goes in as itself into a v4 pool of it, else wrapped into W\(coin) first (one more transaction).", " \(coin) идёт как есть в пул v4 с ним, иначе сначала оборачивается в W\(coin) (ещё одна транзакция).")
                          : say(" An exact approve first.", " Сначала точное разрешение."))
                + (coinOut ? say(" \(coin) comes out as itself, or is unwrapped after.", " \(coin) выходит как есть или разворачивается после.") : "") + confirm
        case .crossSwap(let a, let src, let b, let dst, _):
            let from = name(src), to = name(dst), label = tokenName(b, on: dst)
            if src == "near" || dst == "near" || quotes(c)?.first?.way.hasPrefix("NEAR Intents") == true {
                return say("One deposit on \(from) to the 1Click address NEAR Intents binds to this quote — checked with 1Click before signing; that address needs Windows Hello the first time — and \(label) comes to your own address on \(to); a refund comes back to \(from).",
                           "Один депозит в \(from) на адрес 1Click, который NEAR Intents привязывает к этой котировке, — сверяется с 1Click перед подписью; этому адресу в первый раз нужен Windows Hello, — и \(label) придёт на твой же адрес в \(to); возврат — обратно в \(from).") + confirm
            }
            let coinIn = a.uppercased() == (src == "solana" ? "SOL" : EVMNetwork.named(src)?.gasCoin.uppercased() ?? "ETH")
            return say("One signature on \(from): Relay's solver swaps on the way and pays \(label) to your own address on \(to), usually within seconds. If more than 3% (or 15 cents) of the value would be lost, nothing is signed.",
                       "Одна подпись в \(from): решатель Relay обменивает по пути и платит \(label) на твой же адрес в \(to), обычно за секунды. Если потерялось бы больше 3% (или 15 центов) стоимости, ничего не подписывается.")
                + (src != "solana" && coinIn ? say(" The \(a.uppercased()) goes to Relay's depository: that address needs Windows Hello the first time.", " \(a.uppercased()) уходит в хранилище Relay: этому адресу в первый раз нужен Windows Hello.") : "")
                + (src != "solana" && !coinIn ? say(" An exact approve to Relay comes first, in the same transaction where the network allows.", " Сначала точное разрешение для Relay, в той же транзакции, где сеть это умеет.") : "")
                + confirm
        case .sendToken(let who, let token, let x?, let net):
            guard let r = try? acrossRecipient(who, token: token, network: net) else { return nil }
            let place = AddressCheck.Network(rawValue: r.entry.network)?.title ?? WalletCommand.placeName(r.entry.network), what = "\(Fmt.num(x, maxFraction: 8)) \(r.symbol)"
            return say("NEAR Intents' 1Click turns USDC into \(r.symbol) on \(place) and pays this saved address exactly \(what); what's left of the USDC comes back. Its deposit address is fresh: it needs Windows Hello before anything is sent.",
                       "1Click от NEAR Intents превращает USDC в \(r.symbol) в \(place) и платит этому сохранённому адресу ровно \(what); остаток USDC возвращается. Его адрес депозита новый: перед отправкой нужен Windows Hello.") + confirm
        case .pay:
            guard let q = payment, q.command == c else { return nil }
            let ch = q.challenge, host: String = { if case .pay(_, let url, _) = c { return URL(string: url)?.host ?? ch.realm } else { return ch.realm } }()
            let price = Fmt.num(ch.usdc, maxFraction: 6), e = AddressCheck.ends(ch.payee), need = tempoNeed(ch.usdc)
            let src = need.flatMap(tempoSource)?.net.name ?? "Solana"
            return say("One Tempo transfer of \(price) USDC to the address \(host) named (\(e.start) … \(e.end)); ", "Один перевод в Tempo на \(price) USDC по адресу, который назвал \(host) (\(e.start) … \(e.end)); ")
                + (need != nil ? say("what's short comes from \(src) over Relay first; ", "недостающее сначала придёт из \(src) через Relay; ") : "")
                + say("the fee, ≈ $0.001, comes out of the USDC. Then the request goes again with the payment's hash.", "комиссия ≈ $0,001 — из этих USDC. Потом запрос уходит снова с хешем платежа.") + confirm
        case .aave(true, let x?, let net), .morpho(true, let x?, let net):
            // The Mac's lendingRoute footer: Solana's part when the USDC or the gas there is short, and Ethereum's gas said.
            let morpho: Bool = if case .morpho = c { true } else { false }
            let n = name(net), coin = EVMNetwork.named(net)?.gasCoin ?? "ETH", short = (usdcFree(on: net) ?? 0) < x || gasLow(on: net)
            if net == "base" {
                return (short ? say("One Solana signature for the bridge, then two", "Одна подпись в Solana для моста, потом две") : say("Two", "Две"))
                    + say(" on Base: approve exactly \(Fmt.num(x)) USDC, then \(morpho ? "deposit; the shares come to this wallet only" : "supply"). \(morpho ? "The vault’s asset is" : "Pool and rate are") re-read from Base first.",
                          " транзакции в Base: разрешение ровно на \(Fmt.num(x)) USDC, потом \(morpho ? "депозит — доли приходят только на этот кошелёк" : "вклад"). \(morpho ? "Актив хранилища" : "Пул и ставка") сначала перечитываются из Base.")
                    + confirm
            }
            return (short ? say("What's missing on \(n) comes from Solana over Relay, with \(coin) for gas when there's none; then two",
                                "Недостающее на \(n) придёт с Solana через Relay, с \(coin) на газ, если его нет; потом две")
                          : say("Two", "Две"))
                + say(" \(n) transactions: approve exactly \(Fmt.num(x)) USDC, then \(morpho ? "deposit; the shares come to this wallet only" : "supply"). \(morpho ? "The vault's asset is" : "Pool and aToken are") re-read from \(n) first",
                      " транзакции в \(n): разрешение ровно на \(Fmt.num(x)) USDC, потом \(morpho ? "депозит — доли приходят только на этот кошелёк" : "вклад"). \(morpho ? "Актив хранилища" : "Пул и aToken") сначала перечитываются из \(n)")
                + (net == "ethereum" ? say("; Ethereum's gas changes by the hour (cents to dollars): ask the chat «how much is gas?» for today's",
                                           "; газ в Ethereum меняется каждый час (от центов до долларов): спроси в чате «сколько стоит газ?», чтобы узнать сегодняшний") : "")
                + "." + confirm
        case .multisigCreate:
            return say("One transaction per network from this computer to Safe's factory (Safe 1.4.1, the same contracts and the same address on each). The keys and how many must sign are set in it; ihatecrypto never changes them.",
                       "Одна транзакция на сеть с этого компьютера на фабрику Safe (Safe 1.4.1, те же контракты и тот же адрес в каждой). Ключи и сколько их нужно для подписи задаются в ней; ihatecrypto их никогда не меняет.") + confirm
        case .multisigSend:
            guard let p = multisigPayment, p.command == c, let net = EVMNetwork.named(p.net) else { return nil }
            return say("Each key signs the same move: a plain payment, money only to \(p.to.name). Then this computer sends it with the signatures and pays the gas; the multisig needs no \(net.gasCoin).",
                       "Каждый ключ подписывает одно и то же: обычный платёж, деньги только \(p.to.name). Потом этот компьютер отправляет его с подписями и платит газ; мультисигу \(net.gasCoin) не нужен.") + confirm
        case .contractCall:
            guard let r = callReview, r.command == c else { return nil }
            return say("Encoded by ihatecrypto from \(r.call.function), simulated on \(r.net.name)", "Закодировано ihatecrypto из \(r.call.function), симуляция в \(r.net.name)")
                + (r.approve == nil ? "" : say(" with its exact approve", " с точным разрешением"))
                + say("; the signer reads it again and asks for a new spender in person.", "; подписант читает его снова и о новом получателе разрешения спрашивает лично.") + confirm
        default:
            return nil
        }
    }

    /// A buy or sale done on the EVM network itself (the USDC there, or brought from another EVM network by Relay): its
    /// Uniswap footer (the Mac's tokenRoute); nil for Relay's ways from Solana, which RouteEdit.note says.
    private func hereNote(_ c: WalletCommand, _ place: String, usdc: Decimal?, from src: String?, keep: Bool) -> String? {
        let (payer, here) = evmTradeSource(place, usdc: usdc, from: src, keep: keep, command: c)
        guard here else { return nil }
        let n = WalletCommand.placeName(place)
        return (payer.map { say("First Relay moves the USDC from \(WalletCommand.placeName($0)) to \(n): an exact approve and its deposit. Then two",
                                "Сначала Relay перевезёт USDC из \(WalletCommand.placeName($0)) в \(n): точное разрешение и его депозит. Потом две") }
                ?? say("Two", "Две"))
            + say(" \(n) transactions: an exact approve and the Uniswap swap, which may only pay this wallet", " транзакции в \(n): точное разрешение и обмен в Uniswap, который может платить только этому кошельку")
            + (usdc == nil ? say("; the USDC stays on \(n).", "; USDC остаются в \(n).") : ".") + say(" Press Confirm.", " Нажми «Подтвердить».")
    }

    /// Merkl's claim or sweep (the Mac's claimRoute footer), once what's owed is read: one transaction a network.
    func claimNote(_ owed: [Merkl.Reward], _ c: WalletCommand) -> String {
        let names = Array(Set(owed.map { EVMNetwork.of(chainId: $0.chainId)?.name ?? "chain \($0.chainId)" })).sorted().joined(separator: ", ")
        let sweep: Bool = if case .sweepRewards = c { true } else { false }
        return say("One \(names) transaction a chain: Merkl's distributor pays each reward to this wallet — the signer refuses a claim that pays anyone else. ",
                   "Одна транзакция на сеть (\(names)): распределитель Merkl платит каждую награду этому кошельку — подписант откажет в получении, которое платит кому-то ещё. ")
            + (sweep ? say("Then each reward worth $1 or more is sold for USDC on Solana (Relay's swap or Uniswap and the bridge, whichever leaves more), paying only this wallet; dust stays. ",
                           "Потом каждая награда от $1 продаётся за USDC на Solana (обмен Relay или Uniswap и мост — что оставит больше), только на этот кошелёк; мелочь остаётся. ") : "")
            + say("The proofs are asked for again right before signing; gas missing there comes from Solana over Relay. Press Confirm.",
                  "Доказательства запрашиваются снова прямо перед подписью; недостающий газ придёт с Solana через Relay. Нажми «Подтвердить».")
    }
}

extension Engine {
    /// The save card's line (the Mac's PendingAddressBoard): taken from the message, or resolved from a domain just now.
    func addressNote(_ e: SignerAddress) -> String {
        let net = AddressCheck.Network(rawValue: e.network)?.title ?? WalletCommand.placeName(e.network)
        let domain = Self.savingExtras[Self.extraKey(name: e.name, network: e.network)]?.note?.components(separatedBy: " — ").first
        return domain.map { say("Resolved from \($0) on \(net) just now and checked. Save asks for Windows Hello with the whole address on the prompt.",
                                "Получено из \($0) в \(net) только что и проверено. «Сохранить» спросит Windows Hello с полным адресом в окне.") }
            ?? say("Taken from your own message and checked for \(net). Compare it with the source: Save asks for Windows Hello with the whole address on the prompt.",
                   "Взято из твоего сообщения и проверено для \(net). Сверь с источником: «Сохранить» спросит Windows Hello с полным адресом в окне.")
    }

    /// USDC to a saved address (the Mac's transferRoute footer): to one of the person's own wallets it says so; on Ethereum
    /// mainnet, that the gas is in ETH and can be dollars.
    func transferNote(_ e: SignerAddress, _ amount: Decimal) -> String {
        let net = AddressCheck.Network(rawValue: e.network)?.title ?? WalletCommand.placeName(e.network)
        return (isOwnWallet(e.address)
                ? say("To your own wallet «\(e.name)», one transfer on \(net).", "На твой собственный кошелёк «\(e.name)», один перевод в \(net).")
                : say("Only this saved address may receive the money: the guard and the signer check it, the simulation must show exactly \(Fmt.num(amount)) USDC leaving.",
                      "Получить деньги может только этот сохранённый адрес: это проверяют защита и подписант, а симуляция должна показать, что уходит ровно \(Fmt.num(amount)) USDC."))
            + (e.network == "ethereum" ? say(" Ethereum mainnet: the fee is in ETH and can be dollars; it's bought for you if the wallet has none there.",
                                             " Основная сеть Ethereum: комиссия — в ETH и может быть в долларах; если его на кошельке нет, его купят за тебя.") : "")
            + say(" Press Confirm.", " Нажми «Подтвердить».")
    }
}

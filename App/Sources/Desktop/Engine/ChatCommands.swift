import EVMChain
import Foundation
import IhatecryptoCore
import LLMCatalog
import LLMCore
import SignerAPI
import TradeExecution

/// What the Mac's app reads in the chat itself rather than through the shared parsers: exact commands read before the
/// model (Workspace.send, Wallets.walletSwitch), folders and notes (Canvases.folderCommand), the model's lookups the core
/// doesn't answer (Agent.agentLookup), answers from the portfolio and the closed trades (Activity.answer), and what the
/// chat still does when the model can't be reached.
extension Engine {
    // MARK: Exact commands, before the model

    /// «new canvas», «clear the plan», «what's new», «check for updates», «switch to wallet Main»: read before the model so
    /// a wallet's or a folder's name can't be taken for something else. True when the message was one of them.
    func appCommand(_ text: String) async -> Bool {
        let lower = text.lowercased()
        if lower.wholeMatch(of: /\s*(?:new\s+canvas|новый\s+(?:канвас|холст)|start\s+over|начн[иём]\s+заново)\s*[.!]?\s*/) != nil {
            // The command itself doesn't belong to the canvas being closed.
            let said = messages.popLast()
            do { try newCanvas() } catch {
                if let said { messages.append(said) }
                reply(error.localizedDescription)
            }
            saveChat()
            return true
        }
        if lower.wholeMatch(of: /\s*(?:clear|reset|remove|drop)\s+(?:the\s+)?plan|(?:очисти|сбрось|убери|удали)\s+план\w*\s*[.!]?\s*/) != nil {
            clearPlan()
            return true
        }
        // «what's new»: the versions by date and their stories, over the window (whatsnew.js).
        if lower.wholeMatch(of: /\s*(?:what'?s\s+new(?:\s+in\s+(?:the\s+)?(?:app|ihatecrypto))?|что\s+нового\s+в\s+(?:приложении|ihatecrypto|апп?\w*))\s*[.!?]?\s*/) != nil {
            out.event("whatsNew", [String: String]())
            reply(say("What's new is on screen; it's also in the Profile menu.", "Что нового — на экране; оно же в меню профиля."))
            return true
        }
        // «check for updates»: the window asks the signed update feed (main.rs update_check) and says what it finds.
        if lower.wholeMatch(of: /\s*(?:check\s+for\s+updates?|update\s+(?:the\s+)?app|any\s+updates\??|(?:проверь|есть|нет\s+ли)\s+обновлени\w*|обнови(?:сь|\s+(?:приложение|ihatecrypto|апп?ку?)))\s*[.!?]?\s*/) != nil {
            out.event("checkUpdates", [String: String]())
            reply(say("Checking for a new version: the window says what it finds. An update installs only when it's signed with ihatecrypto's key.",
                      "Проверяю новую версию: окно скажет, что нашлось. Обновление ставится, только если оно подписано ключом ihatecrypto."))
            return true
        }
        if let target = walletSwitch(text) {
            if let target { await switchWallet(to: target) } else {
                let yours = switchableWallets.map { "«\($0.label)»" }.joined(separator: ", ")
                reply(say("No wallet has that name. Yours: \(yours).", "Кошелька с таким именем нет. Твои: \(yours)."))
            }
            return true
        }
        return false
    }

    /// The plan goes, with its card if one waited; a move's card stays (it isn't the plan).
    func clearPlan(note: String? = nil) {
        // The price alert that waited to prepare this plan goes with it (Alerts.swift).
        let alert = planAlert
        if let alert { alerts.removeAll { $0.id == alert.id } }
        intent = TradeIntent()
        if case .trade? = pending { cancelPending() }
        // Its board goes too, and the plan a trade came from is forgotten (TradePlan.swift), as Cancel does.
        dropPlanBoard()
        reply(alert == nil ? say("The plan is cleared. Tell me what to do next.", "План очищен. Скажи, что дальше.")
                           : say("The plan is cleared, and its price alert with it. Tell me what to do next.", "План очищен, и его алерт тоже. Скажи, что дальше."), note: note)
    }

    /// The wallets a name can switch to: this PC's own (never the automation wallet) and a connected Ledger.
    var switchableWallets: [SignerWallet] {
        (status?.wallets ?? []).filter { $0.automation != true } + (ledger?.evm != nil ? [ledger!.wallet] : [])
    }

    /// «переключись на кошелёк Main», «switch to wallet Trading»: .some(nil) when the name matches none.
    func walletSwitch(_ text: String) -> SignerWallet?? {
        guard let m = text.firstMatch(of: /(?i)^\s*(?:переключ\w*|switch|use|используй)\s+(?:на\s+|to\s+)?(?:кошел[её]к\w*|wallet)\s+[«"“]?(.+?)[»"”]?\s*[.!]?\s*$/) else { return nil }
        let name = String(m.1).lowercased()
        return .some(switchableWallets.first { $0.label.lowercased() == name } ?? switchableWallets.first { $0.label.lowercased().hasPrefix(name) })
    }

    func switchWallet(to wallet: SignerWallet) async {
        let active = ledgerActive ? LedgerAccount.id : status?.activeWallet
        guard wallet.id != active else {
            reply(say("«\(wallet.label)» is already the wallet ihatecrypto signs with.", "«\(wallet.label)» и так подписывает в ihatecrypto."))
            return
        }
        do {
            if wallet.id == LedgerAccount.id { useLedger(true) } else {
                useLedger(false)
                try await signerCall(.setActiveWallet(id: wallet.id))
            }
            reply(say("Now signing with «\(wallet.label)»: \(Self.describe(wallet, say: say)).", "Теперь подписывает «\(wallet.label)»: \(Self.describe(wallet, say: say))."))
        } catch {
            reply(say("The wallet wasn't switched: ", "Кошелёк не переключён: ") + error.localizedDescription)
        }
    }

    /// «recovery phrase · Solana AN5b…e8cL, EVM 0x1523…F656»: what a wallet can sign for (the Mac's Wallets.describe).
    nonisolated static func describe(_ w: SignerWallet, say: (String, String) -> String) -> String {
        func short(_ s: String) -> String { "\(s.prefix(4))…\(s.suffix(4))" }
        let kind = w.kind == "phrase" ? say("recovery phrase", "фраза восстановления")
            : w.kind == "ledger" ? say("Ledger, confirm on the device", "Ledger, подтверждение на устройстве")
            : w.kind == "solana" ? say("Solana key only", "только ключ Solana") : say("EVM key only", "только ключ EVM")
        let parts = [w.solana.map { "Solana \(short($0))" }, w.evm.map { "EVM \(short($0))" }, w.near.map { "NEAR \(short($0))" }].compactMap { $0 }
        return "\(kind) · " + parts.joined(separator: ", ")
    }

    // MARK: Folders and notes

    /// «save this canvas to folder X» moves the open canvas there; «write to folder X: …» keeps the text there as a note:
    /// a card on a canvas of its own, titled by it. Written on a new canvas, that canvas becomes the note; otherwise the
    /// open canvas stays open and shows the card with «Open». The next note to that folder from there is one more note on
    /// that note's canvas. A typo of a folder that's there is that folder; a new name makes one (Canvases.folderCommand).
    func folderCommand(_ raw: FolderCommand, note: String? = nil) {
        let folders = canvasFolders
        let command: FolderCommand = switch raw {
        case .save(let f): .save(folder: FolderCommand.existing(f, among: folders) ?? f)
        case .note(let f, let text): .note(folder: FolderCommand.existing(f, among: folders) ?? f, text: text)
        }
        switch command {
        case .save(let folder):
            moveCanvas(canvasID, to: folder)
            reply(say("Saved this canvas to «\(folder)».", "Сохранил этот холст в папку «\(folder)»."), note: note)
        case .note(let folder, let text):
            createFolder(folder)
            let title = text.count > 48 ? String(text.prefix(48)) + "…" : text
            let shown = Set(boards.compactMap { $0.kind == "note" ? $0.opens : nil })
            if let i = canvases.firstIndex(where: { c in c.folder == folder && c.boards.contains(where: Self.isOwnNote) && (c.id == canvasID || shown.contains(c.id)) }) {
                // This canvas is a note in that folder, or shows one: the note joins it.
                if let n = canvases[i].boards.firstIndex(where: Self.isOwnNote) { canvases[i].boards[n].lines.append(text) }
                let texts = canvases[i].boards.first(where: Self.isOwnNote)?.lines ?? [text]
                let opens = canvases[i].id
                if opens != canvasID { updateNoteCard(opens) { $0.lines = texts } }
            } else if messages.filter(\.fromUser).count <= 1, boards.isEmpty {
                // A new canvas with only this message: it becomes the note, keeping the chat that wrote it.
                canvases[canvasIndex].folder = folder
                canvases[canvasIndex].title = title
                pin(Board(kind: "note", title: folder, lines: [text]))
            } else {
                // A canvas of its own in the folder; this one shows the note's card with «Open».
                var c = CanvasDoc(title: title)
                c.folder = folder
                c.boards = [Board(kind: "note", title: folder, lines: [text])]
                canvases.insert(c, at: 0)
                pin(Board(kind: "note", title: folder, lines: [text], opens: c.id))
            }
            saveCanvases()
            reply(say("Saved the note to «\(folder)» — it's on the canvas.", "Записал заметку в «\(folder)» — она на холсте."), note: note)
        }
    }

    /// A note canvas's own notes (not a card showing another canvas's).
    nonisolated static func isOwnNote(_ b: Board) -> Bool { b.kind == "note" && b.opens == nil }

    private func updateNoteCard(_ opens: String, _ change: (inout Board) -> Void) {
        guard let i = boards.firstIndex(where: { $0.kind == "note" && $0.opens == opens }) else { return }
        var list = boards
        change(&list[i])
        boards = list
    }

    // MARK: Lookups the model asks for

    /// «market ETH»: the price, the day, funding and 4h ATR; the open position there; a trade by the book now, each way,
    /// with what the rules say; the week's range (Agent.agentLookup «market»).
    func marketFacts(_ coin: String) async -> String {
        if markets[coin]?.atrPct == nil, let v = try? await Hyperliquid.atrPct(coin: coin) { markets[coin]?.atrPct = v }
        var facts = [marketLine(coin).map { $0 + (markets[coin]?.atrPct.map { ", 4h ATR \(Fmt.num($0.rounded(2, .plain)))%" } ?? "") }].compactMap { $0 }
        if let p = positions.first(where: { $0.coin == coin }) {
            facts.append("Open: \(p.size > 0 ? "long" : "short"), \(Self.summary(coin: p.coin, size: p.size, entry: p.entry, orders: hlOrders, say: { e, _ in e })), unrealized \(Fmt.usd(p.unrealizedPnl)).")
        }
        for side in [Side.long, .short] {
            guard let book = bookPlanFacts(asset: coin, side: side) else { continue }
            facts.append("By the book, a \(side == .long ? "long" : "short") now: \(book.line).")
            facts += book.advice.map { "  rule check (\($0.level == .ok ? "ok" : $0.level == .warn ? "warning" : "blocked")): \($0.text)" }
        }
        if let candles = try? await Hyperliquid.candles(coin: coin, range: .week), let first = candles.first, let last = candles.last, first.close > 0 {
            facts.append("Week: \(Fmt.price(first.close)) → \(Fmt.price(last.close)) (\(signed((last.close - first.close) / first.close * 100))), "
                         + "low \(Fmt.price(candles.map(\.low).min() ?? 0)), high \(Fmt.price(candles.map(\.high).max() ?? 0)).")
        }
        return facts.joined(separator: "\n")
    }

    /// A trade of `asset` by the book now: its risk, stop and take from the rules, and what the rules say about it
    /// (the Mac's TradeAdvice.bookPlan).
    func bookPlanFacts(asset: String, side: Side) -> (line: String, advice: [Advice])? {
        guard let m = markets[asset] else { return nil }
        var trial = TradeIntent()
        trial.asset = asset
        trial.side = side
        trial.autoRules = true
        let r = Plan.resolve(trial, market: m, equity: ruleEquity, policy: policy).intent
        guard let s = Plan.evaluate(r, market: m).sizing, let stop = r.stopPct else { return nil }
        var line = "risk \(Fmt.usd(s.lossAtStopUSD)), stop \(Fmt.pct(stop)) at \(Fmt.price(s.stopPrice))"
        if let take = r.takePct, let tp = s.takePrice, let p = s.profitAtTakeUSD { line += ", take \(Fmt.pct(take)) at \(Fmt.price(tp)) → +\(Fmt.usd(p))" }
        let advice = RiskRules.check(r, s, market: m, equity: ruleEquity, policy: policy, openRisk: HyperliquidAccount.riskAtStops(positions: positions, orders: hlOrders))
        return (line, advice)
    }

    /// «rates USDC»: where the asset earns now, best first, with each place's form, what's free and what's already in.
    func ratesFacts(_ arg: String) async -> String {
        let asset = arg.uppercased().contains("SOL") ? "SOL" : "USDC"
        let options = await loadYields(asset).sorted { $0.apy > $1.apy }
        let held = portfolio.networks.flatMap { n in n.rows.filter { $0.section == "defi" }.map { "\($0.symbol) \($0.amount)\($0.note.map { " (\($0)) " } ?? "") on \(n.name)" } }
        let solana = portfolio.networks.first { $0.key == "solana" }
        let free = ["USDC", "SOL"].compactMap { s in solana?.rows.first { $0.symbol == s && $0.section == "tokens" }.map { "\($0.amount) \(s)" } }
        return (options.isEmpty ? ["The rate sources didn't answer."] : options.prefix(16).map { o in
            "\(o.place) (\(o.asset)): \(Fmt.num(o.apy.rounded(2, .plain)))% a year" + (o.tvlUSD.map { ", \(Fmt.usd(($0 / 1_000_000).rounded(1, .plain)))M deposited" } ?? "")
                + (o.note.isEmpty ? "" : "; \(o.note)") + (o.how.map { "; form: \($0)" } ?? "; ihatecrypto can't deposit there")
        }).joined(separator: "\n")
            + "\nFree on Solana: \(free.isEmpty ? "unknown" : free.joined(separator: ", "))."
            + (held.isEmpty ? " Nothing in vaults now." : " Already in vaults: " + held.joined(separator: ", ") + ".")
    }

    /// «gas base»: today's gas on an EVM network (all of them when none is named) and what ihatecrypto's calls cost there.
    func gasFacts(_ arg: String) async -> String {
        let asked = EVMNetwork.named(arg.lowercased().trimmingCharacters(in: .whitespaces))
        if markets.isEmpty { markets = (try? await Hyperliquid.markets()) ?? [:] }
        var lines: [String] = []
        for net in asked.map({ [$0] }) ?? EVMNetwork.searched where net.feeToken == nil {
            guard let price = markets[net.gasCoin]?.markPx, let (tip, maxFee) = try? await net.rpc.fees() else {
                lines.append("\(net.name): gas couldn't be read.")
                continue
            }
            let wei = (maxFee - tip) / 2 + tip   // fees() doubles the base fee for headroom
            func usd(_ units: Decimal) -> String { Fmt.usd(units * Decimal(wei) / 1_000_000_000_000_000_000 * price) }
            lines.append("\(net.name): \(Fmt.num(Decimal(wei) / 1_000_000_000, maxFraction: 3)) gwei, \(net.gasCoin) \(Fmt.price(price)) → "
                         + "an exact approve + Relay's deposit (≈ 150k gas) \(usd(150_000)); approve + Aave supply (≈ 250k) \(usd(250_000)); "
                         + "approve + Morpho deposit (≈ 500k) \(usd(500_000)); a Uniswap swap (≈ 200k) \(usd(200_000)); a transfer (≈ 65k) \(usd(65_000)).")
        }
        return (lines.isEmpty ? ["No EVM network by «\(arg)»."] : lines).joined(separator: "\n")
            + "\nRelay's own fee for a bridge is apart: it is on the move's card. Tempo has no gas coin: its fees come out of the USDC."
    }

    /// «movers»: the day's biggest moves on Hyperliquid, up and down.
    func moversFacts() -> String {
        let list = markets.values.filter { $0.change24h != nil && $0.volume24h > 1_000_000 }.sorted { ($0.change24h ?? 0) > ($1.change24h ?? 0) }
        func line(_ m: Market) -> String { "\(IntentParser.pretty(m.name)) \(Fmt.price(m.markPx)) \(signed(m.change24h ?? 0))" }
        return list.isEmpty ? "Markets aren't loaded yet."
            : "Up the most today: " + list.prefix(6).map(line).joined(separator: ", ") + ".\nDown the most: " + list.suffix(6).reversed().map(line).joined(separator: ", ") + "."
    }

    // MARK: Questions

    /// What a question is answered from: balances, positions, the closed trades on Hyperliquid, the markets named, a trade
    /// by the book in the direction asked and what the rules say (Activity.answer).
    func answerFacts(_ question: String) async -> (facts: [String], stories: [TradeHistory.Story]) {
        let stories = await closedStories()
        var facts = portfolioFacts() + (await pnlFacts())
        for s in stories.filter({ !$0.isOpen }).prefix(6) {
            let open = s.fills.first, close = s.fills.last
            facts.append("Closed: \(s.long ? "long" : "short") \(IntentParser.pretty(s.coin)), opened \(Fmt.date(s.opened)) at \(open.map { Fmt.price($0.px) } ?? "?"), closed \(s.closed.map(Fmt.date) ?? "?") at \(close.map { Fmt.price($0.px) } ?? "?"), realized \(Fmt.usd(s.realized)) after fees.")
        }
        let named = IntentParser.namedAsset(question, markets: Set(markets.keys))
        let asked = named ?? (intent != TradeIntent() ? intent.asset : nil)
        facts += Set([asked].compactMap { $0 } + positions.map(\.coin)).compactMap(marketLine)
        let q = question.lowercased()
        let side: Side? = q.contains(/short|шорт|прода|sell/) ? .short : q.contains(/\blong|лонг|купи|buy/) ? .long : intent != TradeIntent() ? intent.side : nil
        if let coin = asked, let side {
            if markets[coin]?.atrPct == nil, let v = try? await Hyperliquid.atrPct(coin: coin) { markets[coin]?.atrPct = v }
            if let book = bookPlanFacts(asset: coin, side: side) {
                facts.append("By the book, a \(side == .long ? "long" : "short") \(IntentParser.pretty(coin)) now: \(book.line).")
                facts += book.advice.map { "Rule check (\($0.level == .ok ? "ok" : $0.level == .warn ? "warning" : "blocked")): \($0.text)" }
            }
        }
        return (facts, stories)
    }

    /// The trades on Hyperliquid as stories, newest first; none when there's no EVM address or the exchange didn't answer.
    func closedStories() async -> [TradeHistory.Story] {
        guard let me = evmAddress, let fills = try? await TradeHistory.fills(user: me) else { return [] }
        return TradeHistory.stories(fills).sorted { $0.opened > $1.opened }
    }

    /// Without a model: the questions that matter most — where is a position, what happened to it, what is open.
    func answerWithoutModel(_ question: String) async {
        let stories = await closedStories()
        reply(Self.localAnswer(question, positions: positions, stories: stories, markets: Set(markets.keys), say: say), note: say("from your portfolio and history", "по портфелю и истории"))
    }

    nonisolated static func localAnswer(_ question: String, positions: [HyperliquidAccount.Position], stories: [TradeHistory.Story], markets: Set<String>,
                                        say: (String, String) -> String) -> String {
        if let coin = IntentParser.namedAsset(question, markets: markets) {
            let name = IntentParser.pretty(coin)
            if let p = positions.first(where: { $0.coin == coin }) {
                let side = p.size > 0 ? say("long", "лонг") : say("short", "шорт")
                return say("Your \(side) \(name) is open: \(Fmt.num(abs(p.size), maxFraction: 6)) at \(Fmt.price(p.entry)), unrealized \(Fmt.usd(p.unrealizedPnl)).",
                           "Твой \(side) \(name) открыт: \(Fmt.num(abs(p.size), maxFraction: 6)) по \(Fmt.price(p.entry)), нереализованно \(Fmt.usd(p.unrealizedPnl)).")
            }
            if let s = stories.first(where: { $0.coin == coin && !$0.isOpen }), let close = s.fills.last {
                let side = s.long ? say("long", "лонг") : say("short", "шорт")
                return say("The \(side) \(name) is closed: it closed \(s.closed.map(Fmt.date) ?? "") at \(Fmt.price(close.px)), realized \(Fmt.usd(s.realized)) after fees. The whole story is in History › Positions.",
                           "\(side.capitalized) \(name) закрыт: закрылся \(s.closed.map(Fmt.date) ?? "") по \(Fmt.price(close.px)), итог \(Fmt.usd(s.realized)) после комиссий. Вся история — в «История › Позиции».")
            }
            return say("There is no \(name) position, open or closed, on Hyperliquid.", "Позиции \(name) на Hyperliquid нет — ни открытой, ни закрытой.")
        }
        if positions.isEmpty {
            let last = stories.first { !$0.isOpen }
            return say("No open positions.", "Открытых позиций нет.")
                + (last.map { say(" The last one: \($0.long ? "long" : "short") \(IntentParser.pretty($0.coin)), realized \(Fmt.usd($0.realized)).",
                                  " Последняя: \($0.long ? "лонг" : "шорт") \(IntentParser.pretty($0.coin)), итог \(Fmt.usd($0.realized)).") } ?? "")
        }
        return say("Open: ", "Открыты: ") + positions.map { "\($0.size > 0 ? say("long", "лонг") : say("short", "шорт")) \(IntentParser.pretty($0.coin)) \(Fmt.usd($0.unrealizedPnl))" }.joined(separator: ", ")
            + say(". For anything else, connect a model in Settings.", ". Для остального подключи модель в настройках.")
    }

    // MARK: When the model can't be reached

    /// The model didn't give a reading after two tries: say why in the words that help — not downloaded, refused, an
    /// answer ihatecrypto can't read — and still show what reads the same without a model (a chart, a token, P/L), never
    /// anything that moves money (Workspace.routeWithModel's nil cases).
    func modelUnreachable(_ text: String, failure: String, unreadable: Bool, typedByUser: Bool) async {
        if unreadable {
            reply(say("\(model) answered, but not in a form ihatecrypto can read, so nothing ran. Say it another way, or pick another model.",
                      "\(model) ответил, но не в том виде, который ihatecrypto может прочитать, поэтому ничего не запускал. Скажи иначе или выбери другую модель."), note: model)
            return
        }
        if let refusal = LLMCatalog.chatRefusal(model: model) {
            reply(refusal + say(" Nothing ran. Pick another model.", " Ничего не запускал. Выбери другую модель."), note: model)
            return
        }
        // A model on this PC that isn't downloaded answers 404 «model … not found»: say how to get it.
        let local = provider(providerID)?.info.kind == .local
        let why = local && failure.lowercased().contains("not found")
            ? (providerID == "ollama" ? say("it isn't downloaded — run «ollama pull \(model)» in a terminal", "она не скачана — выполни в терминале «ollama pull \(model)»")
                                      : say("it isn't downloaded", "она не скачана"))
            : String(failure.prefix(140))
        if typedByUser, await readDisplays(text) {
            reply(say("\(model) isn't answering (\(why)), so I read this one myself. Pick another model for the rest.",
                      "\(model) не отвечает (\(why)), поэтому это я прочитал сам. Для остального выбери другую модель."), note: model)
        } else {
            reply(say("I couldn’t reach \(model) (\(why)), so I didn’t act on this — nothing moved. Send it again, or pick another model.",
                      "Не достучался до \(model) (\(why)), поэтому ничего не сделал — ничего не ушло. Отправь ещё раз или выбери другую модель."), note: model)
        }
    }

    /// What shows something without moving money: a token, a chart, P/L, a Solana token by its ticker.
    func readDisplays(_ text: String) async -> Bool {
        let names = Set(markets.keys)
        if let command = TokenCommand.parse(text) { await tokenCommand(command, note: nil); return true }
        if let command = ChartCommand.parse(text, markets: names) { await chart(command, note: nil); return true }
        if let command = PnLCommand.parse(text, markets: names) { await showPnL(command, note: nil); return true }
        if let word = text.lowercased().firstMatch(of: /(?:charts?|график\w*|чарт\w*|show|покажи)\s+(?:(?:the|token|токен\w*|чарт\w*|график\w*)\s+)*\$?([a-z][a-z0-9]{1,15})\b/)?.1,
           let mint = await TokenResearch.solanaMint(symbol: String(word)) {
            await tokenCommand(.show(address: mint, chain: "solana"), note: nil)
            return true
        }
        return false
    }
}

import Foundation
import IhatecryptoCore
import SignerAPI
import TradeExecution
import WalletCore

/// What a site's request still needs the person's word for before it can be approved (the Mac's ApprovalSheet `ask`):
/// a lookalike address, programs off the list, a payee or contract that isn't theirs, the second check's concern.
nonisolated struct SiteTick: Encodable, Equatable {
    var id: String
    var title: String
    var detail: String
    var toggle: String
}

/// A site's request as the window draws it: apart from the person's own card, with its whole origin, and answered by
/// its id (`siteAnswer`), never by the move card's Confirm.
nonisolated struct SiteAskView: Encodable, Equatable {
    var id: String
    var origin: String
    var host: String
    /// «known», «lookalike» (of `real`), «unknown», «local».
    var check: String
    var real: String?
    var title: String
    var lines: [String]
    var warnings: [String]
    /// Every one must be ticked for Approve to count.
    var ticks: [SiteTick]
    /// «Connect» or «Sign».
    var approve: String
}

extension Engine {
    // MARK: A site's request (A1, A2)

    /// The person's answer to the site's request with this id, the one its card showed; any other is left alone. An
    /// approval counts only with every tick the request needs (the Mac's canApprove).
    func answerSite(_ id: String?, yes: Bool, ticks: [String]) throws {
        guard let ask = siteAsk, ask.view.id == id else { return }
        if yes, let missing = ask.view.ticks.first(where: { !ticks.contains($0.id) }) {
            throw WalletError(say("Tick «\(missing.toggle)» first, or decline.", "Сначала отметь «\(missing.toggle)» или откажи."))
        }
        siteAsk = nil
        updateBoard(ask.view.id) { b in
            b.state = yes ? "done" : "cancelled"
            if !yes { b.lines.append(say("Declined: nothing was signed.", "Отказано: ничего не подписано.")) }
        }
        ask.answer(yes)
        changed()
    }

    /// The site's request waiting is declined, with why on its block.
    func declineSite(_ why: String) {
        guard let ask = siteAsk else { return }
        siteAsk = nil
        updateBoard(ask.view.id) { $0.state = "cancelled"; $0.lines.append(why) }
        ask.answer(false)
        changed()
    }

    nonisolated static func lookalikeTick(_ real: String, say: (String, String) -> String) -> SiteTick {
        SiteTick(id: "lookalike", title: say("The address looks like \(real), but it's a different site — a common phishing trick.",
                                             "Адрес похож на \(real), но это другой сайт — частый приём фишинга."),
                 detail: say("Check the address bar letter by letter.", "Проверь адрес в строке по буквам."),
                 toggle: say("I understand this isn't \(real)", "Понимаю, что это не \(real)"))
    }

    nonisolated static func jevTick(_ text: String, say: (String, String) -> String) -> SiteTick {
        SiteTick(id: "jev", title: say("The second check has a concern", "У второй проверки есть сомнение"), detail: text,
                 toggle: say("I understand the second check's warning", "Понимаю предупреждение второй проверки"))
    }

    /// An EVM transaction's payee or contract that isn't the user's (the Mac's evmTransaction sheet): a contract it can't
    /// read when that contract is all it pays, else the addresses money or the right to spend it goes to.
    nonisolated static func payeeTick(to: String, recipients: [String], unread: Bool, say: (String, String) -> String) -> SiteTick? {
        func short(_ a: String) -> String { "\(a.prefix(10))…\(a.suffix(6))" }
        if unread, recipients == [to.lowercased()] {
            return SiteTick(id: "contract", title: say("ihatecrypto can't read what this contract does", "ihatecrypto не может прочитать, что делает этот контракт"),
                            detail: short(to) + say(" — check it on the site. The signer asks for Windows Hello once for a contract it hasn't seen.",
                                                    " — проверь его на сайте. Для контракта, которого он ещё не видел, подписант один раз спросит Windows Hello."),
                            toggle: say("Allow this contract", "Разрешить этот контракт"))
        }
        guard !recipients.isEmpty else { return nil }
        return SiteTick(id: "payee", title: say("Money, or the right to spend it, goes to an address that isn't yours", "Деньги или право их тратить уходят на чужой адрес"),
                        detail: recipients.map(short).joined(separator: ", ") + say(" — check it on the site. The signer asks for Windows Hello for a new one.",
                                                                                    " — проверь на сайте. Для нового адреса подписант спросит Windows Hello."),
                        toggle: say("Send to this address", "Отправить на этот адрес"))
    }

    nonisolated static func programsTick(_ unknown: [String], say: (String, String) -> String) -> SiteTick? {
        guard !unknown.isEmpty else { return nil }
        let names = unknown.map { "\($0.prefix(4))…\($0.suffix(4))" }.joined(separator: ", ")
        let one = unknown.count == 1
        return SiteTick(id: "programs", title: say("\(names) \(one ? "isn't" : "aren't") on ihatecrypto's list", "\(names): нет в списке ihatecrypto"),
                        detail: say("Signing calls \(one ? "it" : "them") the way the site built the transaction; the balances above are the simulated result.",
                                    "Подпись вызовет \(one ? "её" : "их") так, как сайт собрал транзакцию; балансы выше — результат симуляции."),
                        toggle: say("Allow \(one ? "it" : "them") for this one transaction", "Разрешить для этой транзакции"))
    }

    // MARK: Another wallet signs (A4)

    /// What the signer said of itself: when another wallet signs now (switched, made, imported, removed), nothing built
    /// for the last one carries over.
    func statusHeard(_ reply: SignerReply) {
        let before = status
        status = reply
        // Each wallet's own tasks and recorded costs, at launch too (the Mac's Wallets.swift).
        loadWalletState()
        if before != nil, before?.activeWallet != reply.activeWallet { walletChanged() }
    }

    /// Nothing built for the last wallet carries over (the Mac's walletChanged): the waiting card, cards parked on other
    /// canvases, a chain holding its steps, a «How much?», a «Send anyway» and a site's request go; balances are read again.
    func walletChanged() {
        let gone = say("Built for the other wallet: nothing was signed.", "Собрано для другого кошелька: ничего не подписано.")
        if card != nil { updateBoard(card?.id) { $0.state = "cancelled"; $0.lines.append(gone) } }
        if case .wallet(.multisigCreate)? = pending { multisigDraft = nil }
        pending = nil
        card = nil
        parked = [:]
        chainState = ChainState()
        awaitingAmount = nil
        costly = nil
        retry = nil
        callReview = nil
        multisigPayment = nil
        routeWay = nil
        wayQuotes = nil
        declineSite(gone)
        portfolio = PortfolioView()
        positions = []
        loadEquity()
        changed()
    }

    // MARK: A card picked (A8)

    /// A card picked in the chat (the Mac's pick): said by its title and carried out as the command it is, against the
    /// words of the message it answered — no model reads it again. A card with no command is the user's answer to the
    /// model, and goes to it as typed. Like any new request it drops a card that waited for Confirm.
    func pick(_ index: Int?) async {
        guard let i = index, options.indices.contains(i), !thinking else { return }
        markPicked(i)
        let o = options[i]
        // The words of the message these cards answered; cards offered some other way bring none of the user's words.
        let userText = offeredFor?.options == options ? offeredFor?.text ?? "" : ""
        offeredFor = nil
        options = []
        if answerNewModules(o.command) { return }
        if await answerTradingKey(o.command) { return }
        if o.command == Self.sendAnyway { await sendAnyway(); return }
        if o.command == Self.leaveCostly { costly = nil; reply(say("Left as it was: nothing was signed.", "Оставил как есть: ничего не подписано.")); return }
        guard !o.command.isEmpty else { await send(o.title); return }
        if card != nil { cancelPending() }
        messages.append(Message(fromUser: true, text: o.title))
        agentGoal = nil
        nameCanvas(from: o.title)
        saveChat()
        thinking = true
        changed()
        defer { thinking = false; changed() }
        if await openingRouteCommand(o.command) { return }
        if await !runCanonical(o.command, userText: userText) {
            reply(say("ihatecrypto can't do “\(o.command)” yet — nothing moved.", "ihatecrypto пока не умеет «\(o.command)» — ничего не ушло."))
        }
    }

    // MARK: Modules at Confirm (A9)

    /// What Settings › Modules switched off since the card was drawn: refused at Confirm too (the Mac's prepareExecution
    /// and transfer). nil when it may go.
    func confirmRefusal(_ waiting: Pending) -> String? {
        switch waiting {
        case .trade(let p):
            Modules.current.refusal(trade: p.market.dex != nil, bridges: p.bridge != nil || p.fundFrom != nil || p.openVia1Click != nil, ru: russian)
        case .transfer(let entry, _):
            ModuleNetwork(place: entry.network).flatMap { Modules.current.refusal(network: $0, ru: russian) }
        default: nil
        }
    }
}

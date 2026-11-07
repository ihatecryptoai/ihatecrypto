import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import TradeExecution
import SwiftUI
import TxGuard
import WalletCore

/// A paid API's price as its 402 named it, waiting for Confirm: with the user's question it answers and the service's
/// name in the directory («Brave Search»).
struct PaymentQuote {
    let command: WalletCommand
    /// Tempo's MPP challenge, or nil for an x402 offer (USDC on Base by signature).
    let challenge: MPP.Challenge?
    var x402: X402.Offer? = nil
    let ask: String
    let service: String
    /// Quoted without the request's content: Confirm sends it, asking the price again with it before paying.
    var probed = false
    /// The 402's description for the card (and so the model), nil when the guard found it speaking to the assistant.
    var purpose: String?
    /// Asked in a run of this task on the automation wallet: paid, the price is what its later runs may pay by themselves.
    var task: UUID?
    /// A NEAR Intents charge: what the service receives, as 1Click lists the asset («1 USDC»), and that asset's 1Click
    /// id for the guard; nil when 1Click doesn't list it.
    var receives: (text: String, asset: String)? = nil
}

/// What a service sent back, on the canvas: its results as rows (or its values), who answered, what it cost. A paid
/// one is kept in its move's last step, so every move on a canvas keeps its own answer.
nonisolated struct ServiceAnswer: Codable, Hashable {
    let service: String
    let usdc: Decimal?
    /// The payment on Tempo's explorer; nil when the service answered for free.
    let proof: String?
    let answer: MPP.Answer
    let raw: String
    /// Why the guard kept it from the chat's model (InjectionGuard.lookupProblem); nil when it was read.
    var withheld: String? = nil

    /// What the chat's model reads of it with every message (canvasBlocks): the JSON's first 2,000 characters, or that
    /// the guard withheld it — not why, since the reason quotes the service's own words.
    var forModel: String {
        "\(service)'s answer" + (usdc.map { " (paid \(Fmt.num($0, maxFraction: 6)) USDC)" } ?? "") + ": "
            + (withheld == nil ? String(raw.prefix(2000))
               : "withheld by ihatecrypto's guard: it speaks to the assistant. Not shown to you, and the user was told; don't call it again for this")
    }
}

/// «pay POST https://… {…}»: a paid API on Tempo (MPP). The price is asked without the request's content (an email's
/// text, a phone number), so a service the user turns down never sees it; the service's 402 names the price and the
/// payee, which the card shows. After Confirm the request goes with its content, the same price is checked, ihatecrypto
/// pays that USDC on Tempo itself (the signer checks the payee as a new recipient) and sends the request again with the
/// transaction's hash as the proof. A service that can't quote without the content gets it unpaid, as the protocol has it.
extension Workspace {
    func preparePayment(_ command: WalletCommand, note: String? = nil) async {
        guard case .pay(let method, let url, let body) = command, let host = URL(string: url)?.host() else {
            reply(say("That isn't a web address ihatecrypto can call.", "Это не веб-адрес, который ihatecrypto может вызвать."), note: note)
            return
        }
        if MPP.refusesProof.contains(host.lowercased()) {
            reply(say("\(host) takes the payment and then refuses its proof (checked 2026-10-23), so ihatecrypto doesn't pay it. Ask for another service.",
                      "\(host) берёт оплату, а потом не принимает её подтверждение (проверено 2026-10-23), поэтому ihatecrypto его не оплачивает. Попроси другой сервис."), note: note)
            return
        }
        agentStep = say("Asking \(host) for its price", "Спрашиваю цену у \(host)")
        let ask = messages.last(where: \.fromUser)?.text ?? ""
        // Asking a price is a request: only to a service Tempo's directory lists, or a site the user named themselves — so
        // text a lookup slipped into the chat can't have the model send the user's numbers to an address of its choosing.
        let listed = await MPP.serviceName(url)
        guard listed != nil || MPP.named(host, in: ask) else {
            reply(say("\(host) isn't in Tempo's directory of paid services and isn't in your message, so ihatecrypto doesn't call it. Write its address yourself to use it.",
                      "\(host) нет в каталоге платных сервисов Tempo и нет в твоём сообщении, поэтому ihatecrypto его не вызывает. Напиши его адрес сам, чтобы воспользоваться."), note: note)
            return
        }
        do {
            var probed = !(body ?? "").isEmpty && !Self.pricedWithContent.contains(url)
            var (status, headers, data) = try await MPP.call(method, url, probed ? nil : body)
            if probed, status != 402 {
                probed = false
                (status, headers, data) = try await MPP.call(method, url, body)
            }
            let service = listed ?? host
            guard status == 402 else {
                if (200..<300).contains(status) {
                    reply(await answered(ask, service: service, data: data, usdc: nil, proof: nil), note: note)
                } else {
                    reply(say("\(service) answered \(status): ", "\(service) ответил \(status): ") + (await Self.serviceText(MPP.shown(data, limit: 300), from: service) ?? withheldText), note: note)
                }
                return
            }
            let mpp = MPP.challenge(headers["www-authenticate"] ?? "", pays: { self.nearOrigin($0) != nil })
            // No payable MPP challenge: x402 in USDC on Base, by a signature its facilitator settles.
            if case .failure = mpp, let offer = X402.offer(header: headers["payment-required"]) {
                payment = PaymentQuote(command: command, challenge: nil, x402: offer, ask: ask, service: service, probed: probed)
                pendingAction = .wallet(command)
                pendingText = "pay \(Fmt.num(offer.usdc, maxFraction: 6)) USDC to \(service)"
                let short = (usdc(on: "base") ?? 0) < offer.usdc
                reply(say("\(service) asks \(Fmt.num(offer.usdc, maxFraction: 6)) USDC by x402 on Base. The card is on the canvas — Confirm signs it and its facilitator pays the fee"
                            + (short ? "; Base's USDC comes from Solana through NEAR Intents first." : "."),
                          "\(service) просит \(Fmt.num(offer.usdc, maxFraction: 6)) USDC по x402 в Base. Карточка на холсте — Confirm подпишет, а комиссию заплатит его посредник"
                            + (short ? "; USDC в Base сначала придут с Solana через NEAR Intents." : ".")), note: note)
                return
            }
            switch mpp {
            case .failure(let refusal):
                reply(say("Can't pay \(service): ", "Не могу оплатить \(service): ") + (await Self.serviceText(refusal.reason, from: service) ?? withheldText), note: note)
            case .success(let c) where c.near != nil:
                let n = c.near!, origin = nearOrigin(n) ?? "solana", net = WalletCommand.placeName(origin)
                let receives = await nearReceives(n)
                payment = PaymentQuote(command: command, challenge: c, ask: ask, service: service, probed: probed,
                                       purpose: await Self.serviceText(c.purpose, from: service), receives: receives)
                pendingAction = .wallet(command)
                pendingText = "pay \(Fmt.num(c.usdc, maxFraction: 6)) USDC to \(service) through NEAR Intents"
                let refund = nearRefundIsOurs(n, origin: origin) ? say("a refund comes back to you", "возврат придёт тебе")
                    : say("a refund goes to the service's address, not yours", "возврат уйдёт на адрес сервиса, а не тебе")
                reply(say("\(service) asks \(Fmt.num(c.usdc, maxFraction: 6)) USDC on \(net) through NEAR Intents and gets \(receives?.text ?? "what it asked for") at \(n.destinationRecipient). The card is on the canvas. If the payment doesn't settle, \(refund).",
                          "\(service) просит \(Fmt.num(c.usdc, maxFraction: 6)) USDC в \(net) через NEAR Intents и получит \(receives?.text ?? "то, что просит") на \(n.destinationRecipient). Карточка на холсте. Если оплата не пройдёт, \(refund).") , note: note)
            case .success(let c):
                payment = PaymentQuote(command: command, challenge: c, ask: ask, service: service, probed: probed,
                                       purpose: await Self.serviceText(c.purpose, from: service), task: automationTask(of: command)?.id)
                pendingAction = .wallet(command)
                pendingText = "pay \(Fmt.num(c.usdc, maxFraction: 6)) USDC to \(service)"
                let price = Fmt.num(c.usdc, maxFraction: 6), task = automationTask(of: command)
                if task?.approves(c.amount, for: command) == true {
                    reply(say("\(service) asks \(price) USDC — no more than you Confirmed for this task, so it goes without a card, from the automation wallet.",
                              "\(service) просит \(price) USDC — не больше, чем ты подтвердил для этой задачи, поэтому без карточки, с кошелька автоматизации."), note: note)
                    return
                }
                let short = (tempoUSDC ?? 0) < c.usdc + Decimal(string: "0.01")!
                let from = tempoSource(max(c.usdc, Decimal(Executor.tempoFloat) / 1_000_000))?.name ?? "Solana"
                reply(say("\(service) asks \(price) USDC. The card is on the canvas — Confirm pays it in USDC on Tempo"
                            + (short ? ", first bringing $1 there from \(from) so the next requests don't each need a bridge." : ".")
                            + (task == nil ? "" : " After that, this task pays up to \(price) USDC for this request by itself; a higher price waits for your Confirm again."),
                          "\(service) просит \(price) USDC. Карточка на холсте — Confirm оплатит в USDC на Tempo"
                            + (short ? ", а сначала перекинет туда $1 с \(from), чтобы следующие запросы не гоняли мост каждый раз." : ".")
                            + (task == nil ? "" : " Дальше эта задача будет сама платить за этот запрос до \(price) USDC; если цена вырастет, снова спрошу.")), note: note)
            }
        } catch {
            reply(say("Couldn't reach \(host): ", "Не достучался до \(host): ") + error.localizedDescription, note: note)
        }
    }

    /// The repeating task this is a run of on the automation wallet (TaskRun.task), with the prices Confirmed in it — when
    /// the request is one of its steps, not a card a check's model offered during the run.
    func automationTask(of command: WalletCommand) -> ScheduledTask? {
        TaskRun.task.flatMap { id in tasks.first { $0.id == id && $0.steps.contains { WalletCommand.parse($0) == command } } }
    }

    /// A chain's paid request whose price the user hasn't seen: it waits for its own card rather than going on the chain's
    /// Confirm. Only a run of a task on the automation wallet pays one by itself, up to the price Confirmed for it there.
    func paysUnseen(_ command: WalletCommand?) -> Bool {
        guard case .pay? = command else { return false }
        // x402 and NEAR Intents charges always wait for their card: a task pays only a Tempo price it was Confirmed.
        guard let q = payment, let c = q.challenge, c.near == nil else { return true }
        return automationTask(of: q.command)?.approves(c.amount, for: q.command) != true
    }

    /// After Confirm, x402: the same price asked again, USDC brought to Base through NEAR Intents if it's short, an
    /// EIP-3009 authorization the signer builds and checks itself, and the request again with it. Nothing moves unless
    /// the service's facilitator settles it; the settlement's transaction is the proof.
    func payX402(_ method: String, _ url: String, _ body: String?, quote: PaymentQuote, offer: X402.Offer, _ e: Executor) async throws -> String {
        // The signature is this wallet's own key, whose Base address pays: never a schedule's automation wallet (its
        // top-up would land there while the main key is asked) or a Ledger (it can't sign EIP-3009 here).
        guard TaskRun.automation == nil, !ledgerActive else {
            throw ExecutionError("An x402 payment is signed by this wallet's own key, so it can't run from the automation wallet or a Ledger. Nothing was paid.")
        }
        let (status, headers, _) = try await MPP.call(method, url, body)
        guard status == 402, let fresh = X402.offer(header: headers["payment-required"]), fresh.amount == offer.amount,
              fresh.payTo.lowercased() == offer.payTo.lowercased() else {
            throw ExecutionError("\(quote.service)'s price changed or it didn't quote again (\(status)); nothing was paid. Ask again to see the new card.")
        }
        guard let from = signerWallet?.evm else { throw ExecutionError("No EVM address in this wallet.") }
        if let bring = Self.x402TopUp(price: fresh.usdc, onBase: usdc(on: "base") ?? 0) {
            runningAction?.lines.append(say("Bringing USDC to Base through NEAR Intents", "Перевожу USDC в Base через NEAR Intents"))
            let arrived = try await e.usdcByIntents(to: "base", usdc: bring)
            guard (usdc(on: "base") ?? 0) + arrived >= fresh.usdc else {
                throw ExecutionError("Only \(Fmt.num(arrived)) USDC reached Base; nothing was paid. Ask again.")
            }
        }
        var nonce = [UInt8](repeating: 0, count: 32)
        guard SecRandomCopyBytes(kSecRandomDefault, 32, &nonce) == errSecSuccess else { throw ExecutionError("No randomness for the payment.") }
        // As long as the service asks and no longer than 10 minutes: an unsettled signature runs out soon.
        let validBefore = UInt64(Date().timeIntervalSince1970) + UInt64(min(max(fresh.maxTimeoutSeconds, 60), 600))
        let signature = try await XPCTradeSigner().signX402(chainId: fresh.chainId, token: fresh.asset, to: fresh.payTo, value: fresh.amount,
                                                             validBefore: validBefore, nonce: Data(nonce), service: quote.service)
        payment = nil
        answerStep(say("loading the answer…", "загружаю ответ…"), say("\(quote.service) is working · up to a minute", "\(quote.service) работает · до минуты"))
        let header = X402.payment(fresh, from: from, validBefore: validBefore, nonce: Data(nonce), signature: signature)
        // Whatever goes wrong now, the signature may still be settled until it runs out: said, so nobody pays twice.
        let pending = say("the signed payment of \(Fmt.num(fresh.usdc, maxFraction: 6)) USDC can still be settled until \(Date(timeIntervalSince1970: TimeInterval(validBefore)).formatted(date: .omitted, time: .shortened)) — check your Base USDC before asking again",
                          "подписанная оплата \(Fmt.num(fresh.usdc, maxFraction: 6)) USDC ещё может пройти до \(Date(timeIntervalSince1970: TimeInterval(validBefore)).formatted(date: .omitted, time: .shortened)) — проверь USDC в Base, прежде чем спрашивать снова")
        // Written down before it's used: a failure from here on reads «status unknown», not «Failed» with Try again,
        // since the signature may still be settled (a second Confirm would sign a second payment).
        let journal = try? Journal(url: journalURL), id = Data(nonce).hex
        try? journal?.append(.init(chain: "x402", id: id, kind: .signed, note: "\(quote.service) · \(Fmt.num(fresh.usdc, maxFraction: 6)) USDC"))
        let paidStatus: Int, paidHeaders: [String: String], data: Data
        do { (paidStatus, paidHeaders, data) = try await MPP.call(method, url, body, credential: header) }
        catch { throw ExecutionError("\(quote.service) didn't answer (\(error.localizedDescription)); \(pending).") }
        guard (200..<300).contains(paidStatus) else {
            throw ExecutionError("\(quote.service) answered \(paidStatus): \(MPP.shown(data, limit: 300)); \(pending).")
        }
        try? journal?.append(.init(chain: "x402", id: id, kind: .settled, note: quote.service))
        let proof = X402.settled(paidHeaders["payment-response"]).map { "\(EVMNetwork.base.explorer)/tx/\($0)" }
        return await answered(quote.ask, service: quote.service, data: data, usdc: fresh.usdc, proof: proof)
    }

    /// What to bring to Base for an x402 price, or nil when Base has it: what's short plus 1Click's ≈ 1% and a cent,
    /// at least a dollar (its minimum; the next requests need no bridge).
    nonisolated static func x402TopUp(price: Decimal, onBase: Decimal) -> Decimal? {
        guard onBase < price else { return nil }
        return max(1, ((price - onBase) * Decimal(string: "1.02")! + Decimal(string: "0.01")!).rounded(2, .up))
    }

    /// The x402 card: USDC on Base (brought from Solana through NEAR Intents when short) → a signature → the service.
    func x402Route(_ url: String, quote: PaymentQuote, offer: X402.Offer) -> MoveRoute {
        let host = URL(string: url)?.host() ?? quote.service, price = Fmt.num(offer.usdc, maxFraction: 6)
        let there = usdc(on: "base") ?? 0, bring = Self.x402TopUp(price: offer.usdc, onBase: there), short = bring != nil, ends = AddressCheck.ends(offer.payTo)
        return MoveRoute(title: "Pay \(price) USDC · \(quote.service)",
                         footer: "x402: a signature that lets \(host)'s facilitator move exactly \(price) USDC from your Base address to \(ends.start) … \(ends.end), valid 10 minutes at most; it pays the network fee. "
                            + (bring.map { "Base has \(Fmt.num(there)) USDC, so \(Fmt.num($0)) USDC come from Solana through NEAR Intents first. " } ?? "")
                            + "Then the request goes again with it. Press Confirm.",
                         steps: [
            short ? ActivityBubble(kind: "step", label: "FROM · SOLANA → BASE", title: "\(Fmt.num(bring ?? 1)) USDC", lines: ["through NEAR Intents · 1Click"], logo: "sol", fill: .near,
                                   money: moneyMove("USDC", on: "solana", by: -(bring ?? 1)))
                  : ActivityBubble(kind: "step", label: "FROM · BASE", title: "\(price) USDC", lines: ["your USDC on Base"], logo: "base", fill: .base,
                                   money: [MoneyMove(place: "Base", symbol: "USDC", was: there, after: max(0, there - offer.usdc))]),
            ActivityBubble(kind: "step", label: "PAY · X402", title: "\(price) USDC", lines: ["EIP-3009 signature · no gas"], logo: "base", fill: .base),
            ActivityBubble(kind: "step", label: "TO · \(quote.service.uppercased())", title: "the answer comes here", lines: [host], logo: "base", fill: .check),
        ])
    }

    /// After Confirm: the quoted price paid on Tempo, then the request again with the proof. A quote about to expire is
    /// asked for again, and only the same price to the same payee is paid.
    /// Services whose price came out different once they saw the request's content: quoted with it from then on.
    nonisolated(unsafe) static var pricedWithContent: Set<String> = []

    func pay(_ command: WalletCommand, _ e: Executor) async throws -> String {
        guard case .pay(let method, let url, let body) = command, let quote = payment, quote.command == command else {
            throw ExecutionError("The service's price is gone; ask for it again.")
        }
        if let offer = quote.x402 { return try await payX402(method, url, body, quote: quote, offer: offer, e) }
        guard var c = quote.challenge else { throw ExecutionError("The service's price is gone; ask for it again.") }
        if c.near != nil { return try await payNear(method, url, body, quote: quote, c, e) }
        if quote.probed || c.expires.map({ $0.timeIntervalSinceNow < 30 }) == true {
            let (status, headers, _) = try await MPP.call(method, url, body)
            guard status == 402, case .success(let fresh) = MPP.challenge(headers["www-authenticate"] ?? "") else {
                throw ExecutionError("The service didn't quote again (\(status)); nothing was paid.")
            }
            guard fresh.amount == c.amount, fresh.payee.lowercased() == c.payee.lowercased() else {
                // Its price depends on the request: from now on it's asked with the content, so the next card shows it.
                if quote.probed { Self.pricedWithContent.insert(url) }
                throw ExecutionError("The price changed to \(Fmt.num(fresh.usdc, maxFraction: 6)) USDC; nothing was paid. Ask again to see the new card.")
            }
            c = fresh
        }
        let hash = try await e.payTempo(c.amount, to: c.payee, memo: c.memo, label: quote.service)
        payment = nil
        if let id = quote.task, let i = tasks.firstIndex(where: { $0.id == id }) { tasks[i].approve(c.amount, for: command) }
        let proof = "\(EVMNetwork.tempo.explorer)/tx/\(hash)"
        answerStep(say("loading the answer…", "загружаю ответ…"), say("\(quote.service) is working · up to a minute", "\(quote.service) работает · до минуты"))
        runningAction?.lines.append(say("Waiting for \(quote.service)'s answer", "Жду ответ \(quote.service)"))
        let data: Data
        do {
            data = try await MPP.paid(method, url, body, credential: (c.field, MPP.credential(c, hash: hash, payer: signerWallet?.evm ?? "")),
                                      usdc: c.usdc, service: quote.service, proof: proof)
        } catch let r as MPP.Refusal {
            // Its reason carries the service's error text, which the chat shows and its model reads on later turns.
            guard await Self.serviceText(r.reason, from: quote.service) == nil else { throw r }
            throw ExecutionError("Paid \(Fmt.num(c.usdc, maxFraction: 6)) USDC, but \(quote.service) answered with an error whose text is withheld: it speaks to the assistant. The payment is on Tempo: \(proof)")
        }
        return await answered(quote.ask, service: quote.service, data: data, usdc: c.usdc, proof: proof)
    }

    /// The service's data on the canvas, and in the chat the answer to what was asked: the model reads the data
    /// (1–3 sentences, the best results as links), then one line on what it cost. Without a model: where to look.
    private func answered(_ ask: String, service: String, data: Data, usdc: Decimal?, proof: String?, on place: String = "Tempo") async -> String {
        let answer = MPP.Answer(data)
        // Written by the service, not the user: the guard's local rules read it before any model does, as a lookup's
        // (screenedLookup), and before it's kept, so canvasBlocks never hands the model an unread one.
        let why = await InjectionGuard.lookupProblem(MPP.forModel(data), source: "\(service) answer", classifier: nil)
        FileHandle.standardError.write(Data("ihatecrypto service screen: \(service) → \(why ?? "data")\n".utf8))
        let kept = ServiceAnswer(service: service, usdc: usdc, proof: proof, answer: answer, raw: MPP.shown(data, limit: 20_000), withheld: why)
        // A paid one goes with its move (drawn under its route, now and once it's an earlier move); a free one on its own.
        if let last = runningAction?.steps.indices.last { runningAction?.steps[last].answer = kept } else { serviceAnswer = kept }
        agentStep = say("Reading \(service)'s answer", "Читаю ответ \(service)")
        answerStep(say("reading the answer…", "читаю ответ…"), say("it's on the canvas", "он на холсте"))
        defer {
            let found = answer.writing != nil ? say("a written answer", "текстом") : !answer.items.isEmpty ? say("\(answer.items.count) results", "результатов: \(answer.items.count)")
                : !answer.facts.isEmpty ? say("\(answer.facts.count) values", "значений: \(answer.facts.count)") : say("a text answer", "текстовый ответ")
            answerStep(say("answered", "ответ получен"), found + say(" · on the canvas", " · на холсте"))
        }
        var text = why == nil ? say("\(service) answered: it's on the canvas.", "\(service) ответил — ответ на холсте.")
            : say("\(service) answered, but its answer speaks to the assistant, so the assistant didn't read it. It's on the canvas: read it yourself, and don't open its links or send money to addresses in it.",
                  "\(service) ответил, но в ответе есть текст, обращённый к ассистенту, поэтому ассистент его не читал. Ответ на холсте: прочитай его сам, не открывай ссылки из него и не отправляй деньги на адреса оттуда.",
                  "\(service) respondió, pero su respuesta le habla al asistente, así que el asistente no la leyó. Está en el lienzo: léela tú, y no abras sus enlaces ni envíes dinero a sus direcciones.")
        if why == nil, let client = llmClient(), let said = try? await MPP.reply(to: ask, service: service, data: data, language: replyLanguage, client: client, model: model), !said.isEmpty {
            text = said
        }
        let cost = usdc.map { say("Paid \(Fmt.num($0, maxFraction: 6)) USDC to \(service) on \(place)", "Оплачено \(Fmt.num($0, maxFraction: 6)) USDC · \(service) · \(place)") }
        return text + (cost.map { "\n\n\($0) \(proof ?? "")" } ?? "")
    }

    /// A service's own text the chat shows, and so its model reads on later turns (an error, a refusal's reason, its
    /// 402's description): nil when the guard's local rules find it speaking to the assistant.
    nonisolated static func serviceText(_ text: String?, from service: String) async -> String? {
        guard let text, await InjectionGuard.lookupProblem(text, source: "\(service) text", classifier: nil) == nil else { return nil }
        return text
    }
    var withheldText: String { say("(its text is withheld: it speaks to the assistant)", "(его текст скрыт: он обращён к ассистенту)", "(su texto está oculto: le habla al asistente)") }

    /// The running card's last step, the service's: what it's doing now («loading the answer…», then «answered»).
    private func answerStep(_ title: String, _ line: String) {
        guard let last = runningAction?.steps.indices.last else { return }
        runningAction?.steps[last].title = title
        runningAction?.steps[last].lines = [line]
    }

    /// Where a top-up of Tempo comes from, as Executor.ensureEVM decides: nil = Solana, when it holds enough; else the EVM
    /// network that does (a dollar over for Relay).
    func tempoSource(_ need: Decimal) -> EVMNetwork? {
        (solana?.usdc ?? 0) >= need + Decimal(string: "0.1")! ? nil
            : EVMNetwork.funding((need + 1).rounded(2, .up), held: { $0 == "tempo" ? nil : self.usdc(on: $0) })
    }

    /// The card: USDC on Tempo (with what comes from Solana, or the network holding it, first) → the payment → the service.
    func payRoute(_ command: WalletCommand) -> MoveRoute? {
        guard case .pay(_, let url, _) = command, let quote = payment, quote.command == command else { return nil }
        if let offer = quote.x402 { return x402Route(url, quote: quote, offer: offer) }
        guard let c = quote.challenge else { return nil }
        if let n = c.near { return nearChargeRoute(url, quote: quote, c, n) }
        let host = URL(string: url)?.host() ?? c.realm, price = Fmt.num(c.usdc, maxFraction: 6)
        let there = tempoUSDC ?? 0, short = there < c.usdc + Decimal(string: "0.01")!
        let floatUSD = Decimal(Executor.tempoFloat) / 1_000_000, requests = NSDecimalNumber(decimal: (max(c.usdc, floatUSD) / c.usdc).rounded(0, .down)).intValue
        let need = max(c.usdc, floatUSD) - there + Decimal(string: "0.01")!, src = tempoSource(need)
        return MoveRoute(title: "Pay \(price) USDC · \(quote.service)",
                         footer: "One Tempo transfer of \(price) USDC to the address \(host) named (\(AddressCheck.ends(c.payee).start) … \(AddressCheck.ends(c.payee).end)); "
                            + (short ? "what's short comes from \(src?.name ?? "Solana") over Relay first; " : "")
                            + "the fee, ≈ $0.001, comes out of the USDC. Then the request goes again with the payment's hash. Press Confirm.",
                         steps: [
            // Short on Tempo: a dollar's float comes from Solana (Executor.tempoFloat), plus a cent for fees; Relay's ≈ $0.02 on top.
            short ? ActivityBubble(kind: "step", label: "FROM · \((src?.name ?? "Solana").uppercased()) → TEMPO",
                                   title: "\(Fmt.num(src == nil ? need : (need + 1).rounded(2, .up), maxFraction: 6)) USDC",
                                   lines: [say("a float for ≈ \(requests) requests like this · Relay ≈ $0.02", "запас на ≈ \(requests) таких запросов · Relay ≈ $0.02"),
                                           src.map { "\(Fmt.num(usdc(on: $0.key) ?? 0)) USDC on \($0.name)" } ?? solana.map { "\(Fmt.num($0.usdc)) USDC on Solana" } ?? "from your Solana USDC"],
                                   logo: src.flatMap { ModuleNetwork(place: $0.key)?.logo } ?? "sol", fill: src.flatMap { RouteFill(rawValue: $0.key) } ?? .solana,
                                   money: moneyMove("USDC", on: src?.key ?? "solana", by: -(src == nil ? need : (need + 1).rounded(2, .up))))
                  : ActivityBubble(kind: "step", label: "FROM · TEMPO", title: "\(price) USDC", lines: ["your USDC on Tempo"],
                                   logo: "tempo", fill: .tempo, money: [MoneyMove(place: "Tempo", symbol: "USDC", was: there, after: max(0, there - c.usdc))]),
            ActivityBubble(kind: "step", label: "PAY · MPP", title: "\(price) USDC", lines: [quote.purpose ?? (c.purpose == nil ? "one request"
                : say("its description is withheld: it speaks to the assistant", "описание скрыто: оно обращено к ассистенту", "descripción oculta: le habla al asistente"))], logo: "tempo", fill: .tempo),
            ActivityBubble(kind: "step", label: "TO · \(quote.service.uppercased())", title: "the answer comes here", lines: [host], logo: "tempo", fill: .check),
        ])
    }
}

/// The service's answer on the canvas: who answered and what it cost (with the payment on Tempo), then its results as
/// rows — a title, the link, a line of text; pictures as pictures — or its values; «Copy» takes the whole answer.
struct ServiceAnswerBoard: View {
    @Environment(Workspace.self) private var ws
    let answer: ServiceAnswer
    /// A free answer can be closed; one kept with its move stays under it.
    var closable = true

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                TokenLogo(name: "tempo", size: 28, round: true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(answer.service).font(.ui(15, .bold)).lineLimit(1)
                    Text(answer.usdc.map { "paid \(Fmt.num($0, maxFraction: 6)) USDC \(answer.proof?.contains("near-intents") == true ? "through NEAR Intents" : answer.proof?.contains("basescan") == true ? "on Base" : "on Tempo")" } ?? "free answer").font(.ui(10.5)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                if let proof = answer.proof.flatMap(URL.init(string:)) {
                    let place = proof.host()?.contains("near-intents") == true ? "NEAR Intents" : proof.host()?.contains("basescan") == true ? "Base" : "Tempo"
                    Link(destination: proof) { Label(place, systemImage: "arrow.up.right").font(.ui(11, .semibold)) }
                        .help("The payment on \(place)'s explorer")
                }
                CopyButton(text: answer.raw, help: "Copy the whole answer")
                if closable {
                    Button { withAnimation(StudioView.spring) { ws.serviceAnswer = nil } } label: { Image(systemName: "xmark") }.buttonStyle(.hoverIcon)
                }
            }
            let a = answer.answer
            if let w = a.writing {
                Text(w).font(.ui(12.5)).lineSpacing(2).lineLimit(18).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                if !a.links.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(Array(a.links.prefix(6).enumerated()), id: \.offset) { _, link in
                            row(MPP.Answer.Item(title: URL(string: link).map { ($0.host() ?? "") + $0.path() } ?? link, url: link, detail: nil))
                        }
                    }
                }
            } else if !a.items.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(a.items.enumerated()), id: \.offset) { _, item in row(item) }
                }
            } else if !a.facts.isEmpty {
                VStack(alignment: .leading, spacing: 5) {
                    ForEach(Array(a.facts.enumerated()), id: \.offset) { _, f in
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(f.key).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1).frame(width: 150, alignment: .leading)
                            Text(f.value).font(.ui(12, .semibold)).lineLimit(2).textSelection(.enabled)
                            Spacer(minLength: 0)
                        }
                    }
                }
            } else {
                Text(a.text ?? "The service sent nothing back.").font(.ui(12)).lineLimit(14).textSelection(.enabled)
            }
        }
        .padding(16)
        .frame(width: 520, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 24).fill(Brand.card).shadow(color: .black.opacity(0.06), radius: 14, y: 8))
        .overlay(RoundedRectangle(cornerRadius: 24).strokeBorder(Brand.cardBorder))
        .transition(.opacity)
    }

    @ViewBuilder private func row(_ item: MPP.Answer.Item) -> some View {
        let url = item.url.flatMap(URL.init(string:))
        let content = VStack(alignment: .leading, spacing: 2) {
            if item.isImage, let url {
                AsyncImage(url: url) { $0.resizable().aspectRatio(contentMode: .fit) } placeholder: { Color.primary.opacity(0.05) }
                    .frame(maxWidth: 488, maxHeight: 280).clipShape(RoundedRectangle(cornerRadius: 12))
            }
            Text(item.title).font(.ui(12.5, .semibold)).lineLimit(2).multilineTextAlignment(.leading)
            if let host = url?.host() { Text(host).font(.ui(10)).foregroundStyle(RouteFill.relay.accent).lineLimit(1) }
            if let detail = item.detail { Text(detail).font(.ui(11)).foregroundStyle(.secondary).lineLimit(2).multilineTextAlignment(.leading) }
        }
        .padding(.horizontal, 10).padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.primary.opacity(0.04)))
        if let url {
            Link(destination: url) { content }.buttonStyle(.plain).modifier(HoverLift(radius: 12)).help(url.absoluteString)
        } else {
            content
        }
    }
}

/// MPP's NEAR Intents charge (paymentauth.org draft-nearintents-charge-01): the service's 402 names a 1Click deposit
/// address on the network it wants paid on and what it receives; ihatecrypto pays it in USDC on Solana, Base, Arbitrum or
/// Ethereum, checks the address with 1Click before signing (DepositGuard.charge), and proves it with the hash.
extension Workspace {
    /// The place a charge is paid from when its token is USDC there and 1Click takes it: «solana» or an EVM network's key.
    func nearOrigin(_ n: MPP.NearCharge) -> String? {
        if n.onSolana { return n.token == SpotAsset.usdc.mint ? "solana" : nil }
        guard let id = n.evmChain, let net = EVMNetwork.all.first(where: { $0.rpc.chainId == id }), net.usdc.lowercased() == n.token.lowercased(),
              OneClick.Asset.usdc(onEVM: net.key) != nil else { return nil }
        return net.key
    }

    /// Whether a refund would come back to this wallet: the service names the refund address.
    func nearRefundIsOurs(_ n: MPP.NearCharge, origin: String) -> Bool {
        let ours = origin == "solana" ? signerWallet?.solana : signerWallet?.evm
        return ours?.lowercased() == n.refundTo.lowercased()
    }

    /// What the service receives as 1Click lists it: «1 USDC», and the asset's 1Click id. nil when 1Click doesn't list it.
    func nearReceives(_ n: MPP.NearCharge) async -> (text: String, asset: String)? {
        guard let tokens = try? await OneClick.tokens() else { return nil }
        let a = n.destinationAsset, contract = a.split(separator: ":").last.map(String.init)?.lowercased() ?? ""
        let token: OneClick.Token? = if a.hasPrefix("near:mainnet/nep141:") {
            tokens.first { $0.assetId == "nep141:" + a.dropFirst("near:mainnet/nep141:".count) }
        } else if a.hasPrefix(MPP.NearCharge.solanaMainnet + "/") {
            tokens.first { $0.blockchain == "sol" && $0.contract?.lowercased() == contract }
        } else if a.hasPrefix("eip155:"), let id = UInt64(a.dropFirst(7).prefix { $0 != "/" }),
                  let net = EVMNetwork.all.first(where: { $0.rpc.chainId == id }), let chain = Executor.intentChains[net.key] {
            tokens.first { $0.blockchain == chain && $0.contract?.lowercased() == contract }
        } else { nil }
        guard let token, let units = Decimal(string: n.amountOut) else { return nil }
        return ("\(Fmt.num(units / pow(10, token.decimals), maxFraction: token.decimals)) \(token.symbol)", token.assetId)
    }

    /// The card: USDC from the network the service wants (brought there through NEAR Intents first when short) → the
    /// 1Click deposit → what the service receives, and where a refund would go.
    func nearChargeRoute(_ url: String, quote: PaymentQuote, _ c: MPP.Challenge, _ n: MPP.NearCharge) -> MoveRoute {
        let origin = nearOrigin(n) ?? "solana", net = WalletCommand.placeName(origin), price = Fmt.num(c.usdc, maxFraction: 6)
        let host = URL(string: url)?.host() ?? quote.service, there = usdc(on: origin) ?? 0
        let bring = origin == "solana" ? nil : Self.x402TopUp(price: c.usdc, onBase: there), ends = AddressCheck.ends(c.payee)
        let logo = ModuleNetwork(place: origin)?.logo ?? "sol", fill = RouteFill(rawValue: origin) ?? .check
        let refund = nearRefundIsOurs(n, origin: origin) ? "your own address" : "the service's address \(AddressCheck.ends(n.refundTo).start) … \(AddressCheck.ends(n.refundTo).end), not yours"
        // A transfer there pays gas in its coin: none, and ≈ half a dollar of USDC brings some over Relay first (ensureEVM).
        // The coin as the portfolio holds it (Base's ETH is read on its own); not read yet, nothing is claimed.
        let coin = EVMNetwork.named(origin)?.gasCoin ?? "ETH", floor = EVMNetwork.named(origin).map { Decimal($0.gasFloor) / 1_000_000_000_000_000_000 } ?? 0
        let needGas = origin != "solana" && (held(coin, on: origin).map { $0 < floor } ?? false)
        return MoveRoute(title: "Pay \(price) USDC · \(quote.service) · NEAR Intents",
                         footer: "MPP through NEAR Intents: exactly \(price) USDC on \(net) to the 1Click deposit address \(host) named (\(ends.start) … \(ends.end)). "
                            + "Before signing, 1Click itself must hold that address for \(n.destinationRecipient), \(quote.receives?.text ?? "the amount it asked") and the refund address, at no more than 1Click's own price. "
                            + (bring.map { "\(net) has \(Fmt.num(there)) USDC, so \(Fmt.num($0)) USDC come from Solana through NEAR Intents first. " } ?? "")
                            + (needGas ? "\(net) has no \(coin) for the transfer's gas, so ≈ 0.5 USDC from Solana brings some over Relay first. " : "")
                            + "If it doesn't settle, 1Click refunds the USDC to \(refund). Then the request goes again with the deposit's hash. Press Confirm.",
                         steps: [
            bring.map { b in ActivityBubble(kind: "step", label: "FROM · SOLANA → \(net.uppercased())", title: "\(Fmt.num(b)) USDC", lines: ["through NEAR Intents · 1Click"],
                                            logo: "sol", fill: .near, money: moneyMove("USDC", on: "solana", by: -b)) }
                ?? ActivityBubble(kind: "step", label: "FROM · \(net.uppercased())", title: "\(price) USDC", lines: ["your USDC on \(net)"], logo: logo, fill: fill,
                                  money: [MoneyMove(place: net, symbol: "USDC", was: there, after: max(0, there - c.usdc))]),
        ] + (needGas ? [ActivityBubble(kind: "step", label: "GAS · RELAY", title: "+ \(coin) on \(net)", lines: ["≈ 0.5 USDC from Solana · for the transfer's gas"],
                                       logo: "relay", fill: .relay, money: moneyMove("USDC", on: "solana", by: -Decimal(string: "0.5")!, approx: true))] : []) + [
            ActivityBubble(kind: "step", label: "PAY · NEAR INTENTS", title: "\(price) USDC", lines: ["1Click deposit, checked before signing"], logo: "near", fill: .near),
            ActivityBubble(kind: "step", label: "TO · \(quote.service.uppercased())", title: quote.receives?.text ?? "the answer comes here",
                           lines: [n.destinationRecipient], logo: "near", fill: .check),
        ])
    }

    /// After Confirm: asked again when the quote is about to run out (its deposit address is single-use, so a fresh one
    /// comes with it; only the same receiver, refund address and output, for at most 1% more), USDC brought to the network
    /// first when it's short, the deposit checked and sent, then the request again with its hash. The service answers
    /// once 1Click has settled it.
    func payNear(_ method: String, _ url: String, _ body: String?, quote: PaymentQuote, _ shown: MPP.Challenge, _ e: Executor) async throws -> String {
        var c = shown
        if quote.probed || c.expires.map({ $0.timeIntervalSinceNow < 60 }) == true {
            let (status, headers, _) = try await MPP.call(method, url, body)
            guard status == 402, case .success(let fresh) = MPP.challenge(headers["www-authenticate"] ?? "", pays: { self.nearOrigin($0) != nil }),
                  let n = fresh.near, let old = c.near, n.currency == old.currency, n.destinationRecipient == old.destinationRecipient,
                  n.destinationAsset == old.destinationAsset, n.amountOut == old.amountOut, n.refundTo == old.refundTo,
                  fresh.amount <= c.amount else {
                throw ExecutionError("\(quote.service)'s price or receiver changed when asked again (\(status)). Nothing was paid. Ask again to see the new card.")
            }
            c = fresh
        }
        guard let n = c.near, let origin = nearOrigin(n) else { throw ExecutionError("ihatecrypto doesn't pay this token. Nothing was paid.") }
        guard let payer = origin == "solana" ? signerWallet?.solana : signerWallet?.evm else { throw ExecutionError("No address on \(WalletCommand.placeName(origin)) in this wallet.") }
        if origin != "solana", let bring = Self.x402TopUp(price: c.usdc, onBase: usdc(on: origin) ?? 0) {
            runningAction?.lines.append(say("Bringing USDC to \(WalletCommand.placeName(origin)) through NEAR Intents", "Перевожу USDC в \(WalletCommand.placeName(origin)) через NEAR Intents"))
            let arrived = try await e.usdcByIntents(to: origin, usdc: bring)
            guard (usdc(on: origin) ?? 0) + arrived >= c.usdc else {
                throw ExecutionError("Only \(Fmt.num(arrived)) USDC reached \(WalletCommand.placeName(origin)). Nothing was paid. Ask again.")
            }
            // The top-up took its minutes: a price that ran out meanwhile isn't paid (the USDC stays where it came).
            if let expires = c.expires, expires.timeIntervalSinceNow < 60 {
                throw ExecutionError("\(quote.service)'s price ran out while the USDC came to \(WalletCommand.placeName(origin)). Nothing was paid. Ask again.")
            }
        }
        let hash = try await e.payIntentsCharge(from: origin == "solana" ? nil : origin, units: c.amount, deposit: c.payee, recipient: n.destinationRecipient,
                                                refundTo: n.refundTo, amountOut: n.amountOut, destination: quote.receives?.asset, service: quote.service)
        payment = nil
        let proof = "https://explorer.near-intents.org/transactions/\(c.payee)"
        answerStep(say("loading the answer…", "загружаю ответ…"), say("\(quote.service) waits for NEAR Intents · a minute or two", "\(quote.service) ждёт NEAR Intents · минуту-две"))
        runningAction?.lines.append(say("Waiting for \(quote.service)'s answer", "Жду ответ \(quote.service)"))
        let data = try await MPP.paid(method, url, body, credential: (c.field, MPP.credential(c, hash: hash, payer: payer)), usdc: c.usdc,
                                      service: quote.service, proof: proof, on: "NEAR Intents", timeout: 300)
        try? Journal(url: journalURL).append(.init(chain: "1click", id: c.payee, kind: .settled, note: "MPP \(quote.service)"))
        return await answered(quote.ask, service: quote.service, data: data, usdc: c.usdc, proof: proof, on: "NEAR Intents")
    }
}

import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import SwiftUI
import WalletCore

/// A paid API's price as its 402 named it, waiting for Confirm: with the user's question it answers and the service's
/// name in the directory («Brave Search»).
struct PaymentQuote {
    let command: WalletCommand
    let challenge: MPP.Challenge
    let ask: String
    let service: String
    /// Quoted without the request's content: Confirm sends it, asking the price again with it before paying.
    var probed = false
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
        guard listed != nil || ask.lowercased().contains(host.lowercased()) else {
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
                    reply(say("\(service) answered \(status): ", "\(service) ответил \(status): ") + MPP.shown(data, limit: 300), note: note)
                }
                return
            }
            switch MPP.challenge(headers["www-authenticate"] ?? "") {
            case .failure(let refusal):
                reply(say("Can't pay \(service): ", "Не могу оплатить \(service): ") + refusal.reason, note: note)
            case .success(let c):
                payment = PaymentQuote(command: command, challenge: c, ask: ask, service: service, probed: probed)
                pendingAction = .wallet(command)
                pendingText = "pay \(Fmt.num(c.usdc, maxFraction: 6)) USDC to \(service)"
                let short = (tempoUSDC ?? 0) < c.usdc + Decimal(string: "0.01")!
                let from = tempoSource(max(c.usdc, Decimal(Executor.tempoFloat) / 1_000_000))?.name ?? "Solana"
                reply(say("\(service) asks \(Fmt.num(c.usdc, maxFraction: 6)) USDC. The card is on the canvas — Confirm pays it in USDC on Tempo"
                            + (short ? ", first bringing $1 there from \(from) so the next requests don't each need a bridge." : "."),
                          "\(service) просит \(Fmt.num(c.usdc, maxFraction: 6)) USDC. Карточка на холсте — Confirm оплатит в USDC на Tempo"
                            + (short ? ", а сначала перекинет туда $1 с \(from), чтобы следующие запросы не гоняли мост каждый раз." : ".")), note: note)
            }
        } catch {
            reply(say("Couldn't reach \(host): ", "Не достучался до \(host): ") + error.localizedDescription, note: note)
        }
    }

    /// After Confirm: the quoted price paid on Tempo, then the request again with the proof. A quote about to expire is
    /// asked for again, and only the same price to the same payee is paid.
    /// Services whose price came out different once they saw the request's content: quoted with it from then on.
    nonisolated(unsafe) static var pricedWithContent: Set<String> = []

    func pay(_ command: WalletCommand, _ e: Executor) async throws -> String {
        guard case .pay(let method, let url, let body) = command, let quote = payment, quote.command == command else {
            throw ExecutionError("The service's price is gone; ask for it again.")
        }
        var c = quote.challenge
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
        let proof = "\(EVMNetwork.tempo.explorer)/tx/\(hash)"
        answerStep(say("loading the answer…", "загружаю ответ…"), say("\(quote.service) is working · up to a minute", "\(quote.service) работает · до минуты"))
        runningAction?.lines.append(say("Waiting for \(quote.service)'s answer", "Жду ответ \(quote.service)"))
        let (status, _, data) = try await MPP.call(method, url, body, credential: (c.field, MPP.credential(c, hash: hash, payer: signerWallet?.evm ?? "")))
        guard (200..<300).contains(status) else {
            throw ExecutionError("Paid \(Fmt.num(c.usdc, maxFraction: 6)) USDC, but \(quote.service) answered \(status): \(MPP.shown(data, limit: 300)). The payment is on Tempo: \(proof)")
        }
        return await answered(quote.ask, service: quote.service, data: data, usdc: c.usdc, proof: proof)
    }

    /// The service's data on the canvas, and in the chat the answer to what was asked: the model reads the data
    /// (1–3 sentences, the best results as links), then one line on what it cost. Without a model: where to look.
    private func answered(_ ask: String, service: String, data: Data, usdc: Decimal?, proof: String?) async -> String {
        let answer = MPP.Answer(data)
        let kept = ServiceAnswer(service: service, usdc: usdc, proof: proof, answer: answer, raw: MPP.shown(data, limit: 20_000))
        // A paid one goes with its move (drawn under its route, now and once it's an earlier move); a free one on its own.
        if let last = runningAction?.steps.indices.last { runningAction?.steps[last].answer = kept } else { serviceAnswer = kept }
        agentStep = say("Reading \(service)'s answer", "Читаю ответ \(service)")
        answerStep(say("reading the answer…", "читаю ответ…"), say("it's on the canvas", "он на холсте"))
        defer {
            let found = answer.writing != nil ? say("a written answer", "текстом") : !answer.items.isEmpty ? say("\(answer.items.count) results", "результатов: \(answer.items.count)")
                : !answer.facts.isEmpty ? say("\(answer.facts.count) values", "значений: \(answer.facts.count)") : say("a text answer", "текстовый ответ")
            answerStep(say("answered", "ответ получен"), found + say(" · on the canvas", " · на холсте"))
        }
        var text = say("\(service) answered: it's on the canvas.", "\(service) ответил — ответ на холсте.")
        if let client = llmClient(), let said = try? await MPP.reply(to: ask, service: service, data: data, language: replyLanguage, client: client, model: model), !said.isEmpty {
            text = said
        }
        let cost = usdc.map { say("Paid \(Fmt.num($0, maxFraction: 6)) USDC to \(service) on Tempo", "Оплачено \(Fmt.num($0, maxFraction: 6)) USDC · \(service) · Tempo") }
        return text + (cost.map { "\n\n\($0) \(proof ?? "")" } ?? "")
    }

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
        let c = quote.challenge, host = URL(string: url)?.host() ?? c.realm, price = Fmt.num(c.usdc, maxFraction: 6)
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
            ActivityBubble(kind: "step", label: "PAY · MPP", title: "\(price) USDC", lines: [c.purpose ?? "one request"], logo: "tempo", fill: .tempo),
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
                    Text(answer.usdc.map { "paid \(Fmt.num($0, maxFraction: 6)) USDC on Tempo" } ?? "free answer").font(.ui(10.5)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                if let proof = answer.proof.flatMap(URL.init(string:)) {
                    Link(destination: proof) { Label("Tempo", systemImage: "arrow.up.right").font(.ui(11, .semibold)) }
                        .help("The payment on Tempo's explorer")
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

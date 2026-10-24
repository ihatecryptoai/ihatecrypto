import EVMChain
import Foundation
import IhatecryptoCore
import TradeExecution
import WalletCore

/// «pay GET https://…»: a service that answers 402 with an MPP challenge is paid in USDC on Tempo after Confirm, then
/// asked again with the payment's hash — the Mac's Payments, over the shared MPP and Executor.payTempo.
extension Engine {
    nonisolated struct PaymentQuote {
        let command: WalletCommand
        let challenge: MPP.Challenge
        let ask: String
        let service: String
        /// Quoted without the request's content: Confirm sends it, asking the price again with it before paying.
        var probed = false
    }

    /// Services whose price came out different once they saw the request's content: quoted with it from then on.
    nonisolated(unsafe) static var pricedWithContent: Set<String> = []

    var tempoUSDC: Decimal { portfolio.networks.first { $0.key == "tempo" }?.rows.first { $0.symbol == "USDC" }?.value ?? 0 }

    func preparePayment(_ command: WalletCommand, note: String?) async {
        guard case .pay(let method, let url, let body) = command, let host = URL(string: url)?.host else {
            reply(say("That isn't a web address ihatecrypto can call.", "Это не веб-адрес, который ihatecrypto может вызвать."), note: note)
            return
        }
        if MPP.refusesProof.contains(host.lowercased()) {
            reply(say("\(host) takes the payment and then refuses its proof (checked 2026-10-23), so ihatecrypto doesn't pay it. Ask for another service.",
                      "\(host) берёт оплату, а потом не принимает её подтверждение (проверено 2026-10-23), поэтому ihatecrypto его не оплачивает. Попроси другой сервис."), note: note)
            return
        }
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
            // The price is asked without the request's content (an email's text, a phone number), so a service the user
            // turns down never sees it; one that can't quote without it gets it unpaid, as the protocol has it.
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
                let price = Fmt.num(c.usdc, maxFraction: 6), short = tempoUSDC < c.usdc + Decimal(string: "0.01")!
                show(.wallet(command), title: say("Pay \(price) USDC · \(service)", "Оплатить \(price) USDC · \(service)"), lines: [
                    say("One Tempo transfer of \(price) USDC to the address \(host) named: \(c.payee)", "Один перевод \(price) USDC на Tempo, на адрес, который назвал \(host): \(c.payee)"),
                    short ? say("What's short comes from Solana over Relay first: a dollar, so the next requests don't each need a bridge.",
                                "Сначала с Solana через Relay придёт доллар, чтобы следующие запросы не гоняли мост каждый раз.")
                          : say("From your USDC on Tempo; the fee, ≈ $0.001, comes out of the USDC.", "Из твоих USDC на Tempo; комиссия ≈ $0.001 — из тех же USDC."),
                    c.purpose.map { say("For: ", "За: ") + $0 } ?? say("For one request.", "За один запрос."),
                ])
                reply(say("\(service) asks \(price) USDC. Press Confirm to pay it in USDC on Tempo, or Cancel.",
                          "\(service) просит \(price) USDC. Нажми «Подтвердить», чтобы оплатить в USDC на Tempo, или «Отмена»."), note: note)
            }
        } catch {
            reply(say("Couldn't reach \(host): ", "Не достучался до \(host): ") + error.localizedDescription, note: note)
        }
    }

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
        running?.lines.append(say("Waiting for \(quote.service)'s answer", "Жду ответ \(quote.service)"))
        changed()
        let (status, _, data) = try await MPP.call(method, url, body, credential: (c.field, MPP.credential(c, hash: hash, payer: evmAddress ?? "")))
        guard (200..<300).contains(status) else {
            throw ExecutionError("Paid \(Fmt.num(c.usdc, maxFraction: 6)) USDC, but \(quote.service) answered \(status): \(MPP.shown(data, limit: 300)). The payment is on Tempo: \(proof)")
        }
        return await answered(quote.ask, service: quote.service, data: data, usdc: c.usdc, proof: proof)
    }

    /// The service's answer in the user's words when a model is connected, its text otherwise; what was paid under it.
    private func answered(_ ask: String, service: String, data: Data, usdc: Decimal?, proof: String?) async -> String {
        var text = say("\(service) answered:\n", "\(service) ответил:\n") + MPP.shown(data, limit: 1500)
        if let client = llmClient(), let said = try? await MPP.reply(to: ask, service: service, data: data, language: replyLanguage, client: client, model: model), !said.isEmpty {
            text = said
        }
        pin("result", service, lines: text.split(separator: "\n").prefix(20).map(String.init),
            data: BlockData(service: .init(service: service, usdc: usdc, proof: proof, answer: MPP.Answer(data), raw: MPP.shown(data, limit: 20_000))))
        let cost = usdc.map { say("Paid \(Fmt.num($0, maxFraction: 6)) USDC to \(service) on Tempo", "Оплачено \(Fmt.num($0, maxFraction: 6)) USDC · \(service) · Tempo") }
        return text + (cost.map { "\n\n\($0) \(proof ?? "")" } ?? "")
    }
}

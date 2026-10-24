import Foundation
import HyperliquidSigning
import IhatecryptoCore
import SignerAPI
import TradeExecution
import WalletCore

/// Hyperliquid's trading key, as on the Mac (Execution.swift enableAgent, resumeTradingKey, offerTradingKey; Ledger.swift
/// enableLedgerAgent; Settings › Wallet › Hyperliquid). The signer makes the key and signs its approval with the main
/// key (or the Ledger does); only once Hyperliquid accepts it does the signer sign orders, stops and closes with it,
/// without Windows Hello. It can't withdraw: anything that moves money out still takes the main key.
nonisolated struct TradingKeyView: Encodable, Equatable {
    /// The key's address once made; nil before.
    var address: String?
    var active: Bool
    /// Approved before and paused by «lock»: on again after one Windows Hello.
    var paused: Bool
    /// The Ledger's own key (its account), approved on the device.
    var ledger: Bool
    /// This Hyperliquid account approved ihatecrypto's fee; nil until Hyperliquid answered.
    var feeApproved: Bool?
    var feeRate: String
    var referral: String
    /// The wallet can have one: an EVM address and a key here.
    var possible: Bool
}

extension Engine {
    static let tradingKeyOn = "approve the hyperliquid trading key", tradingKeyLater = "leave the hyperliquid trading key off"

    /// The key of the wallet that signs: the Ledger's while it's on, else the signer's.
    var agentAddress: String? { ledgerActive ? ledgerAgent?.agentAddress : status?.agentAddress }
    var agentActive: Bool { (ledgerActive ? ledgerAgent?.agentActive : status?.agentActive) == true }

    func tradingKeyView() -> TradingKeyView {
        TradingKeyView(address: agentAddress, active: agentActive, paused: agentAddress != nil && !agentActive, ledger: ledgerActive,
                       feeApproved: feeApproved, feeRate: HLBuilder.rate, referral: HLBuilder.referralCode,
                       possible: !watching && activeWallet?.evm != nil)
    }

    /// The Ledger's key, read with the signer's status (Workspace.loadSigner).
    func readLedgerAgent() async {
        guard ledgerActive, let evm = ledger?.evm else { ledgerAgent = nil; return }
        ledgerAgent = try? await SignerLink.shared.send(.ledgerAgent(owner: evm, create: false))
    }

    /// «turn on the trading key», «верни торговый ключ»: never approved — approved now; paused — resumed.
    func tradingKeyCommand(_ text: String) async -> Bool {
        guard text.lowercased().contains(/(?:resume|unpause|turn\s+on|enable|approve|включи|верни|одобри)\s+(?:the\s+)?(?:hyperliquid\s+)?(?:trading\s+key|торгов\w*\s+ключ)/) else { return false }
        await turnOnTradingKey()
        return true
    }

    /// Settings' «Approve…» and «Resume…», the chat's words and the offer's card.
    func turnOnTradingKey() async {
        if ledgerActive, agentAddress == nil { await enableLedgerAgent() } else if agentAddress == nil { await enableAgent() } else { await resumeTradingKey() }
    }

    /// Whether Hyperliquid has this account, so its key can be approved: asked twice, since an answer that didn't come
    /// (a refused or slow request) isn't an open account. A new one gets what opens it; no answer is said, before any
    /// Windows Hello prompt for a key Hyperliquid would refuse.
    func hyperliquidOpens(_ evm: String) async -> Bool {
        for attempt in 0..<2 {
            if let new = try? await Hyperliquid.isNewAccount(evm) {
                if new { offerOpeningHyperliquid() }
                return !new
            }
            if attempt == 0 { try? await Task.sleep(for: .seconds(2)) }
        }
        reply(say("Hyperliquid didn't answer, so the trading key wasn't asked for. Try again in a minute.",
                  "Hyperliquid не ответил, поэтому торговый ключ не запрошен. Попробуй ещё раз через минуту."), note: "Hyperliquid")
        return false
    }

    /// The signer makes a key and signs its approval with the main key; it goes to Hyperliquid, and only when the exchange
    /// accepts it do orders switch to the key.
    func enableAgent() async {
        guard !watching, let evm = activeWallet?.evm else { reply(watching ? watchOnly : say("Add a wallet first.", "Сначала добавь кошелёк.")); return }
        // Hyperliquid refuses a key for an account that never had a deposit: what opens it, and the way, before any prompt.
        guard await hyperliquidOpens(evm) else { return }
        do {
            let answer = try await signerCall(.enableAgent)
            guard let action = answer.action, let signature = answer.signature, let nonce = answer.nonce, let agent = answer.agentAddress else {
                throw GuardStop(LinkTradeSigner.reason(answer))
            }
            let result = HLReply.parse(try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: true))
            guard !result.isError else { throw ExecutionError(say("Hyperliquid rejected the key: \(result).", "Hyperliquid не принял ключ: \(result).")) }
            try await signerCall(.setAgentActive(true))
            reply(say("Trading key on: \(agent.prefix(10))…. Orders, stops and closes now skip Windows Hello — new positions for 7 days, then one confirmation renews them; this key can't withdraw money, withdrawals still go through the main key.",
                      "Торговый ключ включён: \(agent.prefix(10))…. Ордера, стопы и закрытия теперь идут без Windows Hello — новые позиции 7 дней, потом одно подтверждение продлевает; вывести деньги этот ключ не может, вывод по-прежнему идёт основным ключом."), note: "Hyperliquid")
        } catch {
            reply(say("Trading key not turned on: ", "Торговый ключ не включён: ") + error.localizedDescription, note: "Hyperliquid")
        }
    }

    /// The Ledger's account gets a key of its own: the signer makes it, the Ledger approves it on its screen.
    func enableLedgerAgent() async {
        guard ledgerActive, let wallet = tradeWallet, let account = ledger else {
            reply(say("Switch to your Ledger first (“use my Ledger”): the trading key belongs to its Hyperliquid account.",
                      "Сначала переключись на Ledger («используй мой Ledger»): торговый ключ принадлежит его аккаунту Hyperliquid."))
            return
        }
        guard await hyperliquidOpens(wallet.evm) else { return }
        if ledgerConnect == nil {
            ledgerConnect = LedgerConnect(model: account.model, evm: account.evm, solana: account.solana, status: say("Making the trading key…", "Делаю торговый ключ…"), busy: true)
        }
        defer { ledgerConnect?.busy = false; changed() }
        do {
            let made = try await signerCall(.ledgerAgent(owner: wallet.evm, create: true))
            guard let agent = made.agentAddress else { throw GuardStop(LinkTradeSigner.reason(made)) }
            if made.agentActive == true {
                await readLedgerAgent()
                reply(say("Your Ledger's trading key is already on: \(agent.prefix(6))…\(agent.suffix(4)).", "Торговый ключ Ledger уже включён: \(agent.prefix(6))…\(agent.suffix(4))."))
                return
            }
            let ask = say("Approve the trading key on your Ledger. Its screen must show agentAddress \(agent.prefix(6))…\(agent.suffix(4)) and agentName ihatecrypto.",
                          "Подтверди торговый ключ на Ledger. На экране должны быть agentAddress \(agent.prefix(6))…\(agent.suffix(4)) и agentName ihatecrypto.")
            reply(ask)
            ledgerConnect?.status = ask
            let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
            let (action, signature) = try await ledgerSigner(wallet).approveAgent(agent, name: "ihatecrypto", nonce: nonce)
            let result = HLReply.parse(try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: true))
            guard !result.isError else { throw ExecutionError(say("Hyperliquid rejected the key: \(result). Its account needs a deposit first.", "Hyperliquid не принял ключ: \(result). Сначала аккаунту нужен депозит.")) }
            try await signerCall(.setLedgerAgentActive(owner: wallet.evm, active: true))
            await readLedgerAgent()
            ledgerConnect?.status = say("Trading key on: orders go without the Ledger's buttons.", "Торговый ключ включён: ордера идут без кнопок Ledger.")
            reply(say("Trading key on for your Ledger: \(agent.prefix(10))…. Orders, stops, trailing and closes now go without the device; withdrawals and transfers are still signed on the Ledger. “lock” pauses it.",
                      "Торговый ключ для Ledger включён: \(agent.prefix(10))…. Ордера, стопы, трейлинг и закрытия теперь идут без устройства; выводы и переводы по-прежнему подписываются на Ledger. «Заблокируй» ставит его на паузу."))
        } catch {
            ledgerConnect?.status = say("The trading key stayed off.", "Торговый ключ остался выключенным.")
            reply(say("Trading key not turned on: ", "Торговый ключ не включён: ") + error.localizedDescription)
        }
    }

    /// The key «lock» paused, on again; the signer asks for Windows Hello first.
    func resumeTradingKey() async {
        do {
            let r = try await signerCall(ledgerActive ? .setLedgerAgentActive(owner: ledger?.evm ?? "", active: true) : .setAgentActive(true))
            await readLedgerAgent()
            reply(r.agentActive == true || agentActive
                  ? say("Trading key on again for 7 days: orders skip Windows Hello, withdrawals still need it.", "Торговый ключ снова включён на 7 дней: ордера идут без Windows Hello, вывод по-прежнему с ним.")
                  : say("The trading key stayed off.", "Торговый ключ остался выключенным."), note: "Hyperliquid")
        } catch {
            reply(say("The trading key stayed off: ", "Торговый ключ остался выключенным: ") + error.localizedDescription, note: "Hyperliquid")
        }
    }

    /// Hyperliquid isn't open on this wallet: it opens with the first USDC that lands there.
    private func offerOpeningHyperliquid() {
        reply(say("Hyperliquid isn't open on this wallet yet. It opens with the first USDC that lands there: about $1 of it pays the opening fee, and the smallest order is $10, so bring at least $11. Then the trading key turns on with one \(ledgerActive ? "approval on the Ledger" : "Windows Hello").",
                  "Hyperliquid на этом кошельке ещё не открыт. Он открывается первыми USDC, которые туда придут: около $1 уходит на открытие, а самый маленький ордер — $10, так что нужно хотя бы $11. После этого торговый ключ включается одним подтверждением \(ledgerActive ? "на Ledger" : "Windows Hello")."), note: "Hyperliquid")
        let solanaUSDC = portfolio.networks.first { $0.key == "solana" }?.rows.first { $0.symbol == "USDC" }?.value ?? 0
        let canMove = solanaUSDC >= 11
        options = (canMove ? [ChatRouter.Option(title: say("Move 11 USDC to Hyperliquid", "Перевести 11 USDC на Hyperliquid"),
                                                comment: say("From Solana; the card waits for your Confirm", "С Solana; карточка ждёт «Подтвердить»"),
                                                command: "move 11 USDC from Solana to Hyperliquid", recommended: true)] : [])
            + [ChatRouter.Option(title: say("Top up ihatecrypto", "Пополнить ihatecrypto"),
                                 comment: say("From a browser wallet, your phone or an exchange", "Из кошелька в браузере, с телефона или с биржи"),
                                 command: WalletCommand.receive.summary, recommended: !canMove)]
    }

    /// Hyperliquid's first money on this wallet: the chat offers the key, once per address. Nothing signs until the card is
    /// picked and Windows Hello says so.
    func offerTradingKey() {
        guard agentAddress == nil, !agentActive, !watching, let evm = activeWallet?.evm, pending == nil, !thinking, signing == 0,
              (portfolio.networks.first { $0.key == "hyperliquid" }?.usd ?? 0) > 0 else { return }
        let offered = settings.tradingKeyOffered ?? []
        guard !offered.contains(evm.lowercased()) else { return }
        settings.tradingKeyOffered = offered + [evm.lowercased()]
        reply(say("Hyperliquid is open on this wallet. Turn on its trading key? Orders, stops and closes then go without \(ledgerActive ? "the Ledger's buttons" : "Windows Hello") — new positions for 7 days, then one confirmation renews them; the key can't withdraw.",
                  "Hyperliquid на этом кошельке открыт. Включить торговый ключ? Тогда ордера, стопы и закрытия идут без \(ledgerActive ? "кнопок Ledger" : "Windows Hello") — новые позиции 7 дней, потом одно подтверждение продлевает; вывести деньги ключ не может."), note: "Hyperliquid")
        options = [
            ChatRouter.Option(title: say("Turn on the trading key", "Включить торговый ключ"),
                              comment: say("One \(ledgerActive ? "approval on the Ledger" : "Windows Hello"); it can't withdraw", "Одно подтверждение \(ledgerActive ? "на Ledger" : "Windows Hello"); вывести не может"),
                              command: Self.tradingKeyOn, recommended: true),
            ChatRouter.Option(title: say("Not now", "Не сейчас"), comment: say("Settings › Wallet › Hyperliquid", "Настройки › Кошелёк › Hyperliquid"), command: Self.tradingKeyLater),
        ]
    }

    /// The offer's two cards; false for any other.
    func answerTradingKey(_ command: String) async -> Bool {
        if command == Self.tradingKeyOn { await turnOnTradingKey(); return true }
        guard command == Self.tradingKeyLater else { return false }
        reply(say("Left off. Settings › Wallet › Hyperliquid turns it on any time, or say «turn on the trading key».",
                  "Оставил выключенным. Включить можно в Настройках › Кошелёк › Hyperliquid или словами «включи торговый ключ»."))
        return true
    }

    // MARK: ihatecrypto's fee

    /// Whether this account approved ihatecrypto's fee, for Settings (Workspace.loadFeeApproval).
    func readFeeApproval() async {
        guard let user = evmAddress else { feeApproved = nil; return }
        let approved = await Executor.allowedFee(user).map { $0 >= HLBuilder.fee }
        if user == evmAddress { feeApproved = approved }
        changed()
    }

    /// Takes back the approval (the signer asks first). The next trade confirmed asks for it again.
    func revokeFee() async {
        do {
            try await executor().setFeeApproval(false)
            reply(say("ihatecrypto's fee approval is taken back. New orders go without it; stops and take-profits already on the exchange keep it until they fill or move. The next Hyperliquid trade you confirm asks for it again.",
                      "Одобрение комиссии ihatecrypto отозвано. Новые ордера идут без неё; стопы и тейки, уже стоящие на бирже, сохраняют её, пока не исполнятся или не сдвинутся. Следующая сделка на Hyperliquid, которую ты подтвердишь, спросит снова."), note: "Hyperliquid")
        } catch {
            reply(say("The fee approval stayed: ", "Одобрение комиссии осталось: ") + error.localizedDescription, note: "Hyperliquid")
        }
        await readFeeApproval()
    }
}

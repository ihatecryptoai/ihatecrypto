import EVMChain
import Foundation
import IhatecryptoCore
import SignerAPI
import TradeExecution
import WalletCore

/// A Safe multisig made in ihatecrypto: where it is, its keys and how many must sign. The chain is the truth — its owners
/// and threshold are read from it before every move. Kept for the user, not per wallet.
nonisolated struct Multisig: Codable, Equatable, Identifiable {
    nonisolated struct Key: Codable, Equatable, Hashable, Identifiable {
        var name: String
        /// Lowercase.
        var address: String
        /// «pc»: the signer's key on this computer (Windows Hello); «ledger»: the connected Ledger; «address»: a saved
        /// address (a phone's wallet, a person) that signs in Safe's own app.
        var kind: String
        var id: String { address }
    }
    var address: String
    var keys: [Key]
    var threshold: Int
    var salt: UInt64
    /// The networks it's made on (EVMNetwork keys): the same address on each.
    var networks: [String]
    var id: String { address }
}

/// The set-up card's choices, before Confirm.
nonisolated struct MultisigDraft: Codable, Equatable {
    var keys: [Multisig.Key]
    var threshold: Int
    var networks: [String]
    var salt: UInt64
    var address: String?
    /// More than half: lose fewer than half the devices and it still opens; a thief needs more than half of them.
    var majority: Int { keys.count / 2 + 1 }
}

/// A payment out of a multisig, read for its card: what, to whom, and which of its keys sign it here.
struct MultisigPayment {
    let command: WalletCommand
    let multisig: Multisig
    let net: String
    let to: SignerAddress
    let symbol: String
    let token: String?
    let amount: Decimal
    let units: Decimal
    let threshold: Int
    let signers: [Multisig.Key]
}

extension Engine {
    /// Where Safe's contracts run among ihatecrypto's networks.
    static let multisigNetworks = ["base", "arbitrum", "optimism", "ethereum", "polygon", "bnb", "hyperevm", "robinhood", "tempo"]

    func saveMultisigs() { Store.save(multisigs, "multisigs.json") }

    /// The signer's own EVM key, whichever wallet is shown: one computer is one key of a multisig.
    var pcKey: String? { evmAddress.map { $0.lowercased() } }

    /// The other EVM keys on this computer (the test or automation wallet, another phrase): they make one key, not several.
    var keysOnThisPC: Set<String> { Set((status?.wallets ?? []).compactMap { $0.evm?.lowercased() }) }

    static func command(_ d: MultisigDraft) -> WalletCommand {
        .multisigCreate(threshold: d.threshold, of: d.keys.count, keys: d.keys.filter { $0.kind != "pc" }.map(\.name), networks: d.networks)
    }

    /// Saved EVM addresses the card can add: confirmed with Windows Hello when saved, none of this computer's.
    var multisigCandidates: [Multisig.Key] {
        guard multisigDraft != nil else { return [] }
        let have = Set(multisigDraft?.keys.map(\.address) ?? []).union(keysOnThisPC)
        var out: [Multisig.Key] = []
        for e in book where (Self.evmPlaces.contains(e.network) || e.network == "hyperliquid") && isEVM(e.address)
            && !multisigs.contains(where: { $0.address == e.address.lowercased() }) && !have.contains(e.address.lowercased()) {
            if !out.contains(where: { $0.address == e.address.lowercased() }) { out.append(.init(name: e.name, address: e.address.lowercased(), kind: "address")) }
        }
        return out
    }

    /// «1 key», «3 keys»; «1 ключ», «2 ключа», «5 ключей».
    func keysWord(_ n: Int) -> String {
        if !russian { return "\(n) key" + (n == 1 ? "" : "s") }
        let tens = n % 100, ones = n % 10
        return "\(n) " + (tens / 10 == 1 ? "ключей" : ones == 1 ? "ключ" : (2...4).contains(ones) ? "ключа" : "ключей")
    }

    func isEVM(_ s: String) -> Bool { s.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil }

    /// «make a multisig 2 of 3 with Phone on base»: the keys (this computer's, the saved addresses named), how many sign and
    /// where, as a card the person adjusts before Confirm.
    func prepareMultisig(_ command: WalletCommand, note: String?) async {
        guard case .multisigCreate(let threshold, let of, let names, let networks) = command else { return }
        guard let pc = pcKey else {
            reply(say("A multisig needs this computer's EVM key: add a wallet with a phrase first.", "Для мультисига нужен EVM-ключ этого компьютера: сначала добавь кошелёк с фразой."), note: note)
            return
        }
        var keys = [Multisig.Key(name: say("This PC", "Этот ПК"), address: pc, kind: "pc")]
        for name in names {
            guard let entry = entry(name, network: "base"), isEVM(entry.address) else {
                reply(say("There's no EVM address «\(name)» in Addresses. Save it first («save 0x… as \(name) on base»), then make the multisig.",
                          "В адресной книге нет EVM-адреса «\(name)». Сначала сохрани его («сохрани 0x… как \(name) на base»), потом делай мультисиг."), note: note)
                return
            }
            guard !keysOnThisPC.contains(entry.address.lowercased()) else {
                reply(say("«\(entry.name)» is another wallet on this computer: one computer is one device, so it's one key of the multisig. Name a phone's wallet, a person or a Ledger.",
                          "«\(entry.name)» — ещё один кошелёк на этом компьютере: один компьютер — одно устройство и один ключ мультисига. Назови кошелёк телефона, человека или Ledger."), note: note)
                return
            }
            if !keys.contains(where: { $0.address == entry.address.lowercased() }) { keys.append(.init(name: entry.name, address: entry.address.lowercased(), kind: "address")) }
        }
        let nets = networks.filter(Self.multisigNetworks.contains)
        let m = min(max(threshold ?? keys.count / 2 + 1, 1), keys.count)
        var draft = MultisigDraft(keys: keys, threshold: m, networks: nets.isEmpty ? ["base"] : nets, salt: UInt64(Date().timeIntervalSince1970))
        // The same card again («Try again» after a failure): the same salt, so it finds the multisig a first try made.
        if let last = multisigDraft, last.keys == draft.keys, last.threshold == draft.threshold { draft.salt = last.salt }
        draft.address = try? Safe.address(owners: draft.keys.map(\.address), threshold: draft.threshold, salt: draft.salt)
        multisigDraft = draft
        showMultisigCard()
        let short = (of ?? 0) > keys.count
            ? say(" You asked for \(of ?? 0) keys: add the others on the card (a saved address).", " Ты просил \(of ?? 0) ключа: добавь остальные на карточке (сохранённый адрес).") : ""
        reply(say("The multisig's card is on the canvas: \(keysWord(keys.count)), any \(m) move its money, on \(draft.networks.map(WalletCommand.placeName).joined(separator: ", ")). Add keys, change how many must sign or pick the networks there, then press Confirm.",
                  "Карточка мультисига на холсте: \(keysWord(keys.count)), деньги двигают любые \(m), сеть: \(draft.networks.map(WalletCommand.placeName).joined(separator: ", ")). Там можно добавить ключи, поменять, сколько нужно подписей, выбрать сети — потом нажми «Подтвердить».") + short, note: note)
    }

    /// The card follows the draft: Confirm and the chat see what the card shows.
    private func showMultisigCard() {
        guard let d = multisigDraft else { return }
        let lines = d.keys.map { "\($0.name) · \($0.address)" } + [say("Address: ", "Адрес: ") + (d.address ?? "—"),
                     say("One transaction per network from this computer to Safe's factory (Safe 1.4.1); ihatecrypto never changes its keys.",
                         "По одной транзакции на сеть с этого компьютера в фабрику Safe (Safe 1.4.1); ihatecrypto никогда не меняет его ключи.")]
        let title = say("Multisig · any \(d.threshold) of \(keysWord(d.keys.count)) on ", "Мультисиг · любые \(d.threshold) из \(keysWord(d.keys.count)) · ")
            + d.networks.map(WalletCommand.placeName).joined(separator: ", ")
        if let id = card?.id, case .wallet(.multisigCreate)? = pending {
            pending = .wallet(Self.command(d))
            card?.title = title
            card?.lines = lines
            updateBoard(id) { $0.title = title; $0.steps = [Board.Step(label: say("MULTISIG · Safe", "МУЛЬТИСИГ · Safe"), title: title, lines: lines)] }
        } else {
            show(.wallet(Self.command(d)), title: title, lines: lines)
        }
    }

    func updateMultisigDraft(_ change: (inout MultisigDraft) -> Void) {
        guard var d = multisigDraft, case .wallet(.multisigCreate)? = pending else { return }
        change(&d)
        d.threshold = min(max(d.threshold, 1), max(d.keys.count, 1))
        d.address = try? Safe.address(owners: d.keys.map(\.address), threshold: d.threshold, salt: d.salt)
        multisigDraft = d
        showMultisigCard()
    }

    /// A key added from the card's candidates (by address) or removed; this computer's key stays.
    func multisigKey(add: String?, remove: String?) throws {
        if let a = add?.lowercased() {
            guard let c = multisigCandidates.first(where: { $0.address == a }) else { throw WalletError(say("That address can't be a key here.", "Этот адрес не может быть ключом.")) }
            updateMultisigDraft { $0.keys.append(c); $0.threshold = $0.majority }
        }
        if let r = remove?.lowercased() {
            updateMultisigDraft { d in
                guard d.keys.count > 1 else { return }
                d.keys.removeAll { $0.address == r && $0.kind != "pc" }
                d.threshold = d.majority
            }
        }
    }

    /// After Confirm: the multisig on each network the card named, kept here, and saved in Addresses so money can go to it.
    func runMultisigCreate() async throws -> String {
        guard let d = multisigDraft else { throw ExecutionError("The multisig's card is gone; ask for it again.") }
        if let problem = Safe.ownersProblem(d.keys.map(\.address), threshold: d.threshold) { throw ExecutionError(problem + " Nothing was signed.") }
        for key in d.keys where key.kind == "address" {
            for net in d.networks.compactMap(EVMNetwork.named) where try await Executor.isContract(key.address, on: net) {
                throw ExecutionError("«\(key.name)» is a contract on \(net.name), not a wallet a person signs with. Nothing was signed.")
            }
        }
        let e = try executor()
        var made: [String] = [], address = ""
        for key in d.networks {
            guard let net = EVMNetwork.named(key) else { continue }
            address = try await e.createMultisig(net, owners: d.keys.map(\.address), threshold: d.threshold, salt: d.salt)
            made.append(key)
            // Kept as soon as it exists on one network: a later one failing doesn't lose it.
            multisigs.removeAll { $0.address == address }
            multisigs.append(Multisig(address: address, keys: d.keys, threshold: d.threshold, salt: d.salt, networks: made))
            saveMultisigs()
        }
        multisigDraft = nil
        pin(Board(kind: "multisig", title: "multisig"))
        // Money goes only to saved addresses: the signer shows the whole address while Windows Hello asks.
        let entries = made.map { SignerAddress(name: "Multisig", network: $0, address: address, memo: "") }
        if let saved = (try? await signerCall(.saveAddresses(entries)))?.addresses {
            book = saved
        } else {
            reply(say("Couldn't save it in Addresses; save \(address) as «Multisig» to send money to it.", "Не сохранил его в адресах; сохрани \(address) как «Multisig», чтобы класть туда деньги."))
        }
        let nets = made.map(WalletCommand.placeName).joined(separator: ", ")
        return say("The multisig \(address) is made on \(nets): any \(d.threshold) of its \(d.keys.count) keys move its money. Put money in with «send 10 USDC on \(made.first ?? "base") to Multisig»; it's on the canvas.",
                   "Мультисиг \(address) создан в \(nets): деньги двигают любые \(d.threshold) из \(d.keys.count) ключей. Положить деньги: «отправь 10 USDC на \(made.first ?? "base") на Multisig»; он на холсте.")
    }

    /// «send 200 USDC from the multisig to Alex on base»: read from the chain for its card.
    func prepareMultisigSend(_ command: WalletCommand, note: String?) async {
        guard case .multisigSend(let token, let amount, let name, let network) = command else { return }
        guard let m = multisigs.last else { reply(say("There's no multisig yet: say «make a multisig».", "Мультисига пока нет: скажи «сделай мультисиг»."), note: note); return }
        let key = network ?? m.networks.first ?? "base"
        guard m.networks.contains(key), let net = EVMNetwork.named(key) else {
            reply(say("The multisig isn't on \(WalletCommand.placeName(key)): it's on \(m.networks.map(WalletCommand.placeName).joined(separator: ", ")).",
                      "Мультисига нет в \(WalletCommand.placeName(key)): он в \(m.networks.map(WalletCommand.placeName).joined(separator: ", "))."), note: note)
            return
        }
        let to: SignerAddress
        do {
            to = try tokenRecipient(name, network: key).entry
            guard isEVM(to.address) else { throw ExecutionError("“\(to.name)” isn't an EVM address.") }
        } catch { reply(error.localizedDescription, note: note); return }
        let coin = token.uppercased() == net.gasCoin.uppercased()
        guard let contract = coin ? "" : token.hasPrefix("0x") ? token : token.uppercased() == "USDC" ? net.usdc : Stablecoins.find(token, on: key)?.address else {
            reply(say("I don't know the token «\(token)» on \(net.name): write its contract.", "Не знаю токен «\(token)» в \(net.name): напиши его контракт."), note: note)
            return
        }
        let symbol = coin ? net.gasCoin : (try? await net.rpc.read(contract, Data([0x95, 0xd8, 0x9b, 0x41]))).flatMap(ABI.string) ?? token.uppercased()
        let decimals = coin ? 18 : Int((try? await net.rpc.read(contract, Data([0x31, 0x3c, 0xe5, 0x67]))).flatMap { ABI.uint(in: $0, word: 0) } ?? 18)
        guard let owners = (try? await net.rpc.read(m.address, Safe.ownersCall)).flatMap(Safe.owners(in:)),
              let threshold = (try? await net.rpc.read(m.address, Safe.thresholdCall)).flatMap({ ABI.uint(in: $0, word: 0) }),
              let raw = coin ? (try? await net.rpc.balance(m.address)).map(Decimal.init)
                : (try? await net.rpc.read(contract, ERC20.balanceOf(m.address))).flatMap({ ABI.decimal(in: $0, word: 0) }) else {
            reply(say("\(net.name)'s node didn't answer for the multisig; try again in a moment.", "Нода \(net.name) не ответила по мультисигу; попробуй через минуту."), note: note)
            return
        }
        let scale = pow(Decimal(10), decimals), held = raw / scale
        let units = amount.map { ($0 * scale).rounded(0, .down) } ?? raw
        guard units > 0, units <= raw else {
            reply(say("The multisig holds \(Fmt.num(held, maxFraction: 6)) \(symbol) on \(net.name)\(amount == nil ? "" : ", less than that").",
                      "В мультисиге \(Fmt.num(held, maxFraction: 6)) \(symbol) в \(net.name)\(amount == nil ? "" : " — меньше, чем нужно")."), note: note)
            return
        }
        guard !coin || units <= Decimal(UInt64.max) else { reply(say("That's more \(net.gasCoin) than one move sends.", "Это больше \(net.gasCoin), чем уходит за раз."), note: note); return }
        let keys = Set(owners.map { $0.lowercased() })
        let here = m.keys.filter { keys.contains($0.address) && $0.address == pcKey }
        guard here.count >= Int(threshold) else {
            let elsewhere = m.keys.filter { !here.contains($0) }.map(\.name).joined(separator: ", ")
            reply(say("The multisig needs \(threshold) signatures and only \(here.count) of its keys sign here. The others (\(elsewhere)) sign in Safe's own app: app.safe.global.",
                      "Мультисигу нужно \(threshold) подписи, а здесь подписывают только \(here.count) его ключа. Остальные (\(elsewhere)) подписывают в приложении Safe: app.safe.global."), note: note)
            return
        }
        multisigPayment = MultisigPayment(command: command, multisig: m, net: key, to: to, symbol: symbol, token: coin ? nil : contract,
                                          amount: units / scale, units: units, threshold: Int(threshold), signers: Array(here.prefix(Int(threshold))))
        let shown = "\(Fmt.num(units / scale, maxFraction: 6)) \(symbol)"
        show(.wallet(command), title: say("\(shown) from the multisig to \(to.name) on \(net.name)", "\(shown) из мультисига на \(to.name) в \(net.name)"),
             lines: [say("The multisig holds \(Fmt.num(held, maxFraction: 6)) \(symbol) → \(Fmt.num(held - units / scale, maxFraction: 6)) after", "В мультисиге \(Fmt.num(held, maxFraction: 6)) \(symbol) → \(Fmt.num(held - units / scale, maxFraction: 6)) после"),
                     say("Signed here by \(here.prefix(Int(threshold)).map(\.name).joined(separator: " and ")); this computer pays the gas", "Подписывает здесь \(here.prefix(Int(threshold)).map(\.name).joined(separator: " и ")); газ платит этот компьютер"),
                     to.address])
        reply(say("The card is on the canvas — press Confirm.", "Карточка на холсте — нажми «Подтвердить»."), note: note ?? "Safe · multisig")
    }

    func runMultisigSend(_ command: WalletCommand) async throws -> String {
        guard let p = multisigPayment, p.command == command, let net = EVMNetwork.named(p.net) else {
            throw ExecutionError("The multisig payment's check is gone; ask for it again.")
        }
        multisigPayment = nil
        let e = try executor()
        let cosigners: [any TradeSigner] = p.signers.map { _ in tradeSigner() }
        let amount = "\(Fmt.num(p.amount, maxFraction: 6)) \(p.symbol)"
        let data = try p.token.map { _ in try ERC20.transfer(to: p.to.address, units: p.units) } ?? Data()
        let hash = try await e.multisigPay(net, safe: p.multisig.address, to: p.token ?? p.to.address,
                                           value: p.token == nil ? NSDecimalNumber(decimal: p.units).uint64Value : 0, data: data,
                                           cosigners: cosigners, recipients: [p.to.address], note: "\(amount) from the multisig to \(p.to.name) on \(net.name)")
        return say("Sent \(amount) from the multisig to \(p.to.name) on \(net.name): \(net.explorer)/tx/\(hash)",
                   "Отправлено \(amount) из мультисига на \(p.to.name) в \(net.name): \(net.explorer)/tx/\(hash)")
    }

    func showMultisig(note: String?) {
        guard let m = multisigs.last else { reply(say("There's no multisig yet: say «make a multisig».", "Мультисига пока нет: скажи «сделай мультисиг»."), note: note); return }
        pin(Board(kind: "multisig", title: "multisig"))
        reply(say("The multisig is on the canvas: \(m.address), any \(m.threshold) of its \(m.keys.count) keys, on \(m.networks.map(WalletCommand.placeName).joined(separator: ", ")).",
                  "Мультисиг на холсте: \(m.address), любые \(m.threshold) из \(m.keys.count) ключей, сети: \(m.networks.map(WalletCommand.placeName).joined(separator: ", "))."), note: note)
    }
}

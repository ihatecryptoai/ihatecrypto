import EVMChain
import Foundation
import IhatecryptoCore
import RelayBridge
import SignerAPI
import SwiftUI
import TradeExecution
import WalletCore

/// A Safe multisig made in ihatecrypto: where it is, its keys and how many must sign. The chain is the truth — its owners
/// and threshold are read from it before every move; this is what the app shows and which of the keys it can sign with.
/// Kept for the user, not per wallet: switching to the Ledger doesn't hide it.
nonisolated struct Multisig: Codable, Equatable, Identifiable, Sendable {
    nonisolated struct Key: Codable, Equatable, Hashable, Sendable, Identifiable {
        var name: String
        /// Lowercase.
        var address: String
        /// «mac»: the signing process's key on this Mac (Touch ID); «ledger»: the connected Ledger; «address»: a saved
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
struct MultisigDraft: Equatable {
    var keys: [Multisig.Key]
    var threshold: Int
    var networks: [String]
    var salt: UInt64
    /// More than half: lose fewer than half the devices and it still opens; a thief needs more than half of them.
    var majority: Int { keys.count / 2 + 1 }
    var address: String? { try? Safe.address(owners: keys.map(\.address), threshold: threshold, salt: salt) }
}

/// A payment out of a multisig, read for its card: what, to whom, and which of its keys sign it here.
struct MultisigPayment: Equatable {
    let command: WalletCommand
    let multisig: Multisig
    let net: String
    let to: SavedAddress
    let symbol: String
    /// The token's contract; nil for the network's coin.
    let token: String?
    /// In whole tokens, and in the token's smallest units.
    let amount: Decimal
    let units: Decimal
    let held: Decimal
    let threshold: Int
    /// The keys here that sign, this Mac's first.
    let signers: [Multisig.Key]
}

extension Workspace {
    /// The networks a multisig can be made on: where Safe's contracts run among ihatecrypto's (all of them, 2026-10-09).
    static let multisigNetworks = ["base", "arbitrum", "optimism", "ethereum", "polygon", "bnb", "hyperevm", "robinhood", "tempo"]

    static func loadMultisigs() -> [Multisig] {
        UserDefaults.standard.data(forKey: "multisigs").flatMap { try? JSONDecoder().decode([Multisig].self, from: $0) } ?? []
    }

    func saveMultisigs() { UserDefaults.standard.set(try? JSONEncoder().encode(multisigs), forKey: "multisigs") }

    /// This Mac's EVM key: the signing process's own address, whichever wallet the app shows (the Ledger too).
    func macEVM() async -> String? {
        guard let a = (try? await SignerClient().send(.status))?.evmAddress, isEVMAddress(a) else { return nil }
        return a.lowercased()
    }

    /// The command the card stands for now: the confirm button and the chat read it.
    static func command(_ d: MultisigDraft) -> WalletCommand {
        .multisigCreate(threshold: d.threshold, of: d.keys.count, keys: d.keys.filter { $0.kind != "mac" }.map(\.name), networks: d.networks)
    }

    /// «make a multisig 2 of 3 with Phone on base»: the keys (this Mac's, the Ledger's, the saved addresses named), how many
    /// sign and where, as a card the user adjusts before Confirm.
    func prepareMultisig(_ command: WalletCommand, note: String?) async {
        guard case .multisigCreate(let threshold, let of, let names, let networks) = command else { return }
        guard let mac = await macEVM() else {
            reply(say("A multisig needs this Mac's EVM key in secure storage: add a wallet in Settings › Wallet.",
                      "Для мультисига нужен EVM-ключ этого Mac в защищённом хранилище: добавь кошелёк в Settings › Wallet."), note: note)
            return
        }
        var keys = [Multisig.Key(name: "This Mac", address: mac, kind: "mac")]
        // The connected Ledger: when named, when no count was said, or when the count needs it beside the names.
        let ledger = LedgerAccount.saved.flatMap { l in l.evm.map { Multisig.Key(name: l.label, address: $0.lowercased(), kind: "ledger") } }
        func isLedger(_ name: String) -> Bool { name.lowercased() == ledger?.name.lowercased() || name.lowercased().contains(/^(?:my\s+|the\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)$/) }
        let others = names.filter { !isLedger($0) }
        if let ledger, ledger.address != mac, names.contains(where: isLedger) || of == nil || (of ?? 0) > others.count + 1 { keys.append(ledger) }
        for name in others {
            guard let entry = savedEntry(name, network: "base"), isEVMAddress(entry.address) else {
                reply(say("There's no EVM address «\(name)» in Addresses. Save it first («save 0x… as \(name) on base»), then make the multisig.",
                          "В адресной книге нет EVM-адреса «\(name)». Сначала сохрани его («сохрани 0x… как \(name) на base»), потом делай мультисиг."), note: note)
                return
            }
            do { try requireConfirmed(entry) } catch { reply(error.localizedDescription, note: note); return }
            guard !keysOnThisMac.contains(entry.address.lowercased()) else {
                reply(say("«\(entry.name)» is another wallet on this Mac: one Mac is one device, so it's one key of the multisig — This Mac's. Name a phone's wallet, a person or a Ledger.",
                          "«\(entry.name)» — ещё один кошелёк на этом Mac: один Mac — одно устройство и один ключ мультисига (This Mac). Назови кошелёк телефона, человека или Ledger."), note: note)
                return
            }
            if !keys.contains(where: { $0.address == entry.address.lowercased() }) { keys.append(.init(name: entry.name, address: entry.address.lowercased(), kind: "address")) }
        }
        let nets = networks.isEmpty ? ["base"] : networks.filter(Self.multisigNetworks.contains)
        let m = min(max(threshold ?? keys.count / 2 + 1, 1), keys.count)
        var draft = MultisigDraft(keys: keys, threshold: m, networks: nets.isEmpty ? ["base"] : nets, salt: UInt64(Date().timeIntervalSince1970))
        // The same card again («Try again» after a failure): the same salt, so it finds the multisig a first try made
        // instead of making a second one.
        if let last = multisigDraft, last.keys == draft.keys, last.threshold == draft.threshold { draft.salt = last.salt }
        multisigDraft = draft
        pendingAction = .wallet(Self.command(draft))
        pendingText = Self.command(draft).summary
        let short = (of ?? 0) > keys.count
            ? say(" You asked for \(of ?? 0) keys: add the others on the card (a saved address, another Ledger).", " Ты просил \(of ?? 0) ключа: добавь остальные на карточке (сохранённый адрес, ещё один Ledger).")
            : ""
        reply(say("The multisig's card is on the canvas: \(keys.count) keys, any \(m) move its money, on \(draft.networks.map(WalletCommand.placeName).joined(separator: ", ")). Add keys, change how many must sign or pick the networks there, then press Confirm.",
                  "Карточка мультисига на холсте: \(keys.count) ключа, деньги двигают любые \(m), сеть: \(draft.networks.map(WalletCommand.placeName).joined(separator: ", ")). Там можно добавить ключи, поменять, сколько нужно подписей, выбрать сети — потом нажми Confirm.") + short, note: note)
    }

    /// The card changed: its command follows, so Confirm and the chat see what the card shows.
    func updateMultisigDraft(_ change: (inout MultisigDraft) -> Void) {
        guard var d = multisigDraft, case .wallet(.multisigCreate)? = pendingAction else { return }
        change(&d)
        d.threshold = min(max(d.threshold, 1), max(d.keys.count, 1))
        multisigDraft = d
        pendingAction = .wallet(Self.command(d))
        pendingText = Self.command(d).summary
    }

    /// The other EVM keys on this Mac (the Test or Automation wallet, another phrase): one Mac is one device, so its keys
    /// make one key of a multisig, not several.
    var keysOnThisMac: Set<String> { Set(wallets.filter { $0.kind != "ledger" }.compactMap { $0.evm?.lowercased() }) }

    /// Keys the card can add: the connected Ledger, and saved EVM addresses confirmed with Touch ID — none of this Mac's.
    var multisigCandidates: [Multisig.Key] {
        let have = Set(multisigDraft?.keys.map(\.address) ?? []).union(keysOnThisMac)
        var out: [Multisig.Key] = []
        if let l = LedgerAccount.saved, let a = l.evm?.lowercased() { out.append(.init(name: l.label, address: a, kind: "ledger")) }
        let evm: Set<AddressCheck.Network> = [.base, .arbitrum, .ethereum, .polygon, .robinhood, .tempo, .hyperevm, .optimism, .bnb]
        for e in addressBook where evm.contains(e.network) && isEVMAddress(e.address) && isConfirmed(e) && !multisigs.contains(where: { $0.address == e.address.lowercased() }) {
            if !out.contains(where: { $0.address == e.address.lowercased() }) { out.append(.init(name: e.name, address: e.address.lowercased(), kind: "address")) }
        }
        return out.filter { !have.contains($0.address) }
    }

    /// What making it costs on each network, paid by this Mac: «≈ $0.01». Read now: the gas price times the factory call's gas.
    func multisigFees(_ d: MultisigDraft) async -> [String: String] {
        var out: [String: String] = [:]
        for key in d.networks {
            guard let net = EVMNetwork.named(key), let price = try? await net.rpc.quantity("eth_gasPrice", []) else { continue }
            let gas = Decimal(260_000 + 25_000 * d.keys.count), coins = gas * Decimal(price) / 1_000_000_000_000_000_000
            if net.feeToken != nil { out[key] = say("paid in USDC", "в USDC"); continue }
            let usd = ((try? await Relay.nativePrice(chainId: net.rpc.chainId)) ?? nil).map { $0 * coins }
            out[key] = usd.map { $0 < Decimal(string: "0.01")! ? "< $0.01" : "≈ " + Fmt.usd($0) } ?? "≈ \(Fmt.num(coins, maxFraction: 6)) \(net.gasCoin)"
        }
        return out
    }

    /// After Confirm: the multisig on each network the card named, then kept here.
    func runMultisigCreate(_ e: Executor) async throws -> String {
        guard let d = multisigDraft else { throw ExecutionError("The multisig's card is gone; ask for it again.") }
        if let problem = Safe.ownersProblem(d.keys.map(\.address), threshold: d.threshold) { throw ExecutionError(problem + " Nothing was signed.") }
        for key in d.keys where key.kind == "address" {
            for net in d.networks.compactMap(EVMNetwork.named) where try await Executor.isContract(key.address, on: net) {
                throw ExecutionError("«\(key.name)» is a contract on \(net.name), not a wallet a person signs with. Nothing was signed.")
            }
        }
        var made: [String] = [], address = ""
        for key in d.networks {
            guard let net = EVMNetwork.named(key) else { continue }
            address = try await e.createMultisig(net, owners: d.keys.map(\.address), threshold: d.threshold, salt: d.salt)
            made.append(key)
            // Kept as soon as it exists on one network: a later one failing doesn't lose it.
            let m = Multisig(address: address, keys: d.keys, threshold: d.threshold, salt: d.salt, networks: made)
            multisigs.removeAll { $0.address == address }
            multisigs.append(m)
            saveMultisigs()
        }
        multisigDraft = nil
        withAnimation(StudioView.spring) { detail = "multisig" }
        let nets = made.map(WalletCommand.placeName).joined(separator: ", ")
        return say("The multisig \(address) is made on \(nets): any \(d.threshold) of its \(d.keys.count) keys move its money. Put money in with «send 10 USDC on \(made.first ?? "base") to Multisig»; it's on the canvas.",
                   "Мультисиг \(address) создан в \(nets): деньги двигают любые \(d.threshold) из \(d.keys.count) ключей. Положить деньги: «отправь 10 USDC на \(made.first ?? "base") на Multisig»; он на холсте.")
    }

    /// The multisig as an address-book entry: «send 10 USDC on base to Multisig». Only on a network it's made on; the
    /// signer asks Touch ID, with the whole address, the first time money goes there, as for any new address.
    func multisigEntry(_ name: String, network: String?) -> SavedAddress? {
        guard name.lowercased().wholeMatch(of: /(?:the\s+|my\s+)?(?:multisig|мультисиг\w*)/) != nil, let m = multisigs.last else { return nil }
        let net = network ?? m.networks.first ?? "base"
        guard m.networks.contains(net), let place = AddressCheck.Network(rawValue: net) else { return nil }
        return SavedAddress(name: "Multisig", address: m.address, network: place, category: "Your multisig")
    }

    /// «send 200 USDC from the multisig to Alex on base»: read from the chain for its card — what it holds, its keys and
    /// how many must sign, which of them sign here.
    func prepareMultisigSend(_ command: WalletCommand, note: String?) async {
        guard case .multisigSend(let token, let amount, let name, let network) = command else { return }
        guard let m = multisigs.last else {
            reply(say("There's no multisig yet: say «make a multisig».", "Мультисига пока нет: скажи «сделай мультисиг»."), note: note)
            return
        }
        let key = network ?? m.networks.first ?? "base"
        guard m.networks.contains(key), let net = EVMNetwork.named(key) else {
            reply(say("The multisig isn't on \(WalletCommand.placeName(key)): it's on \(m.networks.map(WalletCommand.placeName).joined(separator: ", ")).",
                      "Мультисига нет в \(WalletCommand.placeName(key)): он в \(m.networks.map(WalletCommand.placeName).joined(separator: ", "))."), note: note)
            return
        }
        let entry: SavedAddress
        do {
            entry = try tokenRecipient(name, network: key).entry
            try requireConfirmed(entry)
            guard isEVMAddress(entry.address) else { throw ExecutionError("“\(entry.name)” isn't an EVM address.") }
        } catch { reply(error.localizedDescription, note: note); return }
        let coin = token.uppercased() == net.gasCoin.uppercased()
        guard let contract = coin ? "" : token.hasPrefix("0x") ? token : token.uppercased() == "USDC" ? net.usdc : Stablecoins.find(token, on: key)?.address else {
            reply(say("I don't know the token «\(token)» on \(net.name): write its contract.", "Не знаю токен «\(token)» в \(net.name): напиши его контракт."), note: note)
            return
        }
        agentStep = say("Reading the multisig on \(net.name)", "Читаю мультисиг в \(net.name)")
        defer { agentStep = nil }
        let meta = coin ? (symbol: net.gasCoin, decimals: 18) : await tokenMeta(contract, net)
        guard let decimals = meta.decimals else {
            reply(say("\(meta.symbol) on \(net.name) doesn't answer decimals(), so ihatecrypto can't count the amount. Nothing was signed.",
                      "\(meta.symbol) в \(net.name) не отвечает на decimals(), поэтому ihatecrypto не может посчитать сумму. Ничего не подписано."), note: note)
            return
        }
        guard let owners = (try? await net.rpc.read(m.address, Safe.ownersCall)).flatMap(Safe.owners(in:)),
              let threshold = (try? await net.rpc.read(m.address, Safe.thresholdCall)).flatMap({ ABI.uint(in: $0, word: 0) }),
              let raw = coin ? (try? await net.rpc.balance(m.address)).map(Decimal.init) : (try? await net.rpc.read(contract, ERC20.balanceOf(m.address))).flatMap({ ABI.decimal(in: $0, word: 0) }) else {
            reply(say("\(net.name)'s node didn't answer for the multisig; try again in a moment.", "Нода \(net.name) не ответила по мультисигу; попробуй через минуту."), note: note)
            return
        }
        let scale = pow(Decimal(10), decimals), held = raw / scale
        let units = amount.map { ($0 * scale).rounded(0, .down) } ?? raw
        guard units > 0, units <= raw else {
            reply(say("The multisig holds \(Fmt.num(held, maxFraction: 6)) \(meta.symbol) on \(net.name)\(amount == nil ? "" : ", less than that").",
                      "В мультисиге \(Fmt.num(held, maxFraction: 6)) \(meta.symbol) в \(net.name)\(amount == nil ? "" : " — меньше, чем нужно")."), note: note)
            return
        }
        guard !coin || units <= Decimal(UInt64.max) else { reply(say("That's more \(net.gasCoin) than one move sends.", "Это больше \(net.gasCoin), чем уходит за раз."), note: note); return }
        let mac = await macEVM(), ledger = LedgerAccount.saved?.evm?.lowercased()
        let keys = Set(owners.map { $0.lowercased() })
        let here = m.keys.filter { keys.contains($0.address) && ($0.address == mac || $0.address == ledger) }.sorted { a, _ in a.kind == "mac" }
        guard here.count >= Int(threshold) else {
            let elsewhere = m.keys.filter { !here.contains($0) }.map(\.name).joined(separator: ", ")
            reply(say("The multisig needs \(threshold) signatures and only \(here.count) of its keys sign here. The others (\(elsewhere)) sign in Safe's own app: app.safe.global.",
                      "Мультисигу нужно \(threshold) подписи, а здесь подписывают только \(here.count) его ключа. Остальные (\(elsewhere)) подписывают в приложении Safe: app.safe.global."), note: note)
            return
        }
        multisigPayment = MultisigPayment(command: command, multisig: m, net: key, to: entry, symbol: meta.symbol, token: coin ? nil : contract,
                                          amount: units / scale, units: units, held: held, threshold: Int(threshold), signers: Array(here.prefix(Int(threshold))))
        pendingAction = .wallet(command)
        pendingText = command.summary
        reply(say("\(Fmt.num(units / scale, maxFraction: 6)) \(meta.symbol) from the multisig to \(entry.name) on \(net.name): signed by \(here.prefix(Int(threshold)).map(\.name).joined(separator: " and ")). The card is on the canvas — press Confirm.",
                  "\(Fmt.num(units / scale, maxFraction: 6)) \(meta.symbol) из мультисига на \(entry.name) в \(net.name): подпишут \(here.prefix(Int(threshold)).map(\.name).joined(separator: " и ")). Карточка на холсте — нажми Confirm."), note: note)
    }

    /// After Confirm: each key here signs the move, then this wallet sends it.
    func runMultisigSend(_ command: WalletCommand, _ e: Executor) async throws -> String {
        guard let p = multisigPayment, p.command == command, let net = EVMNetwork.named(p.net) else {
            throw ExecutionError("The multisig payment's check is gone; ask for it again.")
        }
        multisigPayment = nil
        let cosigners: [any TradeSigner] = p.signers.compactMap { k in
            guard k.kind == "ledger" else { return XPCTradeSigner() }
            guard let l = LedgerAccount.saved, let evm = l.evm else { return nil }
            return ledgerSigner(TradeWallet(solana: l.solana ?? "", evm: evm))
        }
        let amount = "\(Fmt.num(p.amount, maxFraction: 6)) \(p.symbol)"
        let data = try p.token.map { _ in try ERC20.transfer(to: p.to.address, units: p.units) } ?? Data()
        let hash = try await e.multisigPay(net, safe: p.multisig.address, to: p.token ?? p.to.address,
                                           value: p.token == nil ? NSDecimalNumber(decimal: p.units).uint64Value : 0, data: data,
                                           cosigners: cosigners, recipients: [p.to.address], note: "\(amount) from the multisig to \(p.to.name) on \(net.name)")
        markSent(p.to)
        return say("Sent \(amount) from the multisig to \(p.to.name) on \(net.name): \(net.explorer)/tx/\(hash)",
                   "Отправлено \(amount) из мультисига на \(p.to.name) в \(net.name): \(net.explorer)/tx/\(hash)")
    }

    func showMultisig(note: String?) {
        guard let m = multisigs.last else {
            reply(say("There's no multisig yet: say «make a multisig».", "Мультисига пока нет: скажи «сделай мультисиг»."), note: note)
            return
        }
        withAnimation(StudioView.spring) { detail = "multisig" }
        reply(say("The multisig is on the canvas: \(m.address), any \(m.threshold) of its \(m.keys.count) keys, on \(m.networks.map(WalletCommand.placeName).joined(separator: ", ")).",
                  "Мультисиг на холсте: \(m.address), любые \(m.threshold) из \(m.keys.count) ключей, сети: \(m.networks.map(WalletCommand.placeName).joined(separator: ", "))."), note: note)
    }

    /// The route the card and the running move show.
    func multisigRoute(_ command: WalletCommand) -> MoveRoute? {
        func short(_ a: String) -> String { "\(a.prefix(6))…\(a.suffix(4))" }
        switch command {
        case .multisigCreate:
            guard let d = multisigDraft else { return nil }
            let address = d.address.map(short) ?? "—"
            let check = ActivityBubble(kind: "step", label: "CHECK · KEYS", title: "\(d.threshold) of \(d.keys.count) keys",
                                       lines: d.keys.map { "\($0.name)\t\(short($0.address))" }, fill: .check)
            let nets = d.networks.compactMap(EVMNetwork.named).map { net in
                ActivityBubble(kind: "step", label: "MULTISIG · \(net.name.uppercased())", title: "Create on \(net.name)",
                               lines: ["address\t\(address)", "Safe 1.4.1 · fee from this Mac"], logo: ModuleNetwork(place: net.key)?.logo,
                               fill: RouteFill(rawValue: net.key) ?? .check)
            }
            return MoveRoute(title: "Multisig · any \(d.threshold) of \(d.keys.count) keys",
                             footer: "One transaction per network from this Mac to Safe's factory (Safe 1.4.1, the same contracts and the same address on each). The keys and how many must sign are set in it; ihatecrypto never changes them. Press Confirm.",
                             steps: [check] + nets)
        case .multisigSend:
            guard let p = multisigPayment, p.command == command, let net = EVMNetwork.named(p.net) else { return nil }
            let amount = "\(Fmt.num(p.amount, maxFraction: 6)) \(p.symbol)"
            var steps = p.signers.enumerated().map { i, k in
                ActivityBubble(kind: "step", label: "SIGN · \(k.kind == "ledger" ? "LEDGER" : "THIS MAC")", title: "Signature \(i + 1) of \(p.threshold)",
                               lines: ["to\t\(p.to.name)", "amount\t\(amount)", "network\t\(net.name)"], fill: .check)
            }
            steps.append(ActivityBubble(kind: "step", label: "TO · \(p.to.name.uppercased())", title: "\(amount) → \(p.to.name)",
                                        lines: ["from the multisig \(short(p.multisig.address))", "gas from this Mac"],
                                        logo: ModuleNetwork(place: net.key)?.logo, fill: RouteFill(rawValue: net.key) ?? .check,
                                        money: [MoneyMove(place: "Multisig", symbol: p.symbol, was: p.held, after: p.held - p.amount)],
                                        address: p.to.address))
            return MoveRoute(title: "\(amount) from the multisig to \(p.to.name)",
                             footer: "Each key signs the same move: a plain payment, money only to \(p.to.name). Then this Mac sends it with the signatures and pays the gas; the multisig needs no \(net.gasCoin). Press Confirm.",
                             steps: steps)
        default: return nil
        }
    }
}

// MARK: Cards

/// The picture on a multisig's card: its body with one keyhole per key, the ones a move needs lit, and its state.
struct KeyholesPicture: View {
    let total: Int
    let lit: Int
    let chip: String
    var done = false

    var body: some View {
        ZStack(alignment: .topLeading) {
            Canvas { ctx, size in
                let n = max(1, min(total, 8)), sp: CGFloat = n <= 4 ? 36 : 28
                let w = sp * CGFloat(n) + 36, h: CGFloat = 66
                let x = size.width - w - 26, y = (size.height - h) / 2 + 6
                let box = Path(roundedRect: CGRect(x: x, y: y, width: w, height: h), cornerRadius: 10)
                ctx.fill(box, with: .color(Color(hex: 0x1E211F)))
                ctx.stroke(box, with: .color(.white), lineWidth: 2.5)
                for i in 0..<n {
                    let cx = x + 18 + sp / 2 + sp * CGFloat(i)
                    var hole = Path(ellipseIn: CGRect(x: cx - 6, y: y + 16, width: 12, height: 12))
                    hole.move(to: CGPoint(x: cx - 4, y: y + 25))
                    hole.addLines([CGPoint(x: cx + 4, y: y + 25), CGPoint(x: cx + 6, y: y + 44), CGPoint(x: cx - 6, y: y + 44)])
                    hole.closeSubpath()
                    if i < lit { ctx.fill(hole, with: .color(Color(hex: 0x12FF80))) } else { ctx.stroke(hole, with: .color(.white), lineWidth: 1.8) }
                }
            }
            Text(chip).font(.ui(11, .bold)).lineLimit(1)
                .foregroundStyle(Color(hex: done ? 0x146B48 : 0x6B4F00))
                .padding(.horizontal, 10).frame(height: 22)
                .background(Capsule().fill(Color(hex: done ? 0xE2F6EC : 0xFFF1C2)))
                .padding(12)
        }
        .frame(height: 100)
        .frame(maxWidth: .infinity)
        .background(Color(hex: 0x121312))
    }
}

/// «MULTISIG · Safe · Base»: what the card is, as the route cards say it.
private struct MultisigHeading: View {
    let verb: String
    let detail: String
    var body: some View {
        HStack(spacing: 7) {
            Text(verb).font(.ui(10.5, .heavy)).tracking(0.8)
            Circle().fill(.secondary).frame(width: 3, height: 3)
            Text("S").font(.ui(9.5, .heavy)).foregroundStyle(Color(hex: 0x12FF80)).frame(width: 17, height: 17)
                .background(RoundedRectangle(cornerRadius: 5).fill(Color(hex: 0x121312)))
            Text(detail).font(.ui(12, .semibold)).lineLimit(1)
        }
    }
}

private struct SectionLabel: View {
    let text: String
    var trailing: String?
    var body: some View {
        HStack {
            Text(text)
            Spacer(minLength: 8)
            if let trailing { Text(trailing) }
        }
        .font(.ui(10, .bold)).tracking(0.6).foregroundStyle(.secondary).textCase(.uppercase)
    }
}

private struct KeyRow: View {
    let key: Multisig.Key
    let role: String
    let signs: Bool
    var remove: (() -> Void)?

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: key.kind == "mac" ? "laptopcomputer" : key.kind == "ledger" ? "externaldrive" : "iphone")
                .font(.system(size: 14, weight: .medium)).frame(width: 32, height: 32)
                .background(RoundedRectangle(cornerRadius: 9).fill(Color.primary.opacity(0.05)))
            VStack(alignment: .leading, spacing: 1) {
                Text(key.name).font(.ui(13, .semibold)).lineLimit(1)
                Text("\(key.address.prefix(6))…\(key.address.suffix(4)) · " + (key.kind == "mac" ? Presence.short : key.kind == "ledger" ? "the Ledger's buttons" : "signs in Safe's app"))
                    .font(.ui(11)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 6)
            Text(role).font(.ui(10.5, .bold)).foregroundStyle(signs ? Brand.profit : .secondary)
                .padding(.horizontal, 8).frame(height: 20)
                .background(Capsule().fill(signs ? Brand.profit.opacity(0.12) : Color.primary.opacity(0.06)))
            if let remove {
                Button(action: remove) { Image(systemName: "xmark") }.buttonStyle(.hoverIcon).help("Remove this key").accessibilityLabel("Remove \(key.name)")
            }
        }
        .padding(.horizontal, 8).padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color.primary.opacity(0.035)))
    }
}

/// The set-up card waiting for Confirm: the keys, how many must sign, where — each can change here.
struct MultisigSetupCard: View {
    @Environment(Workspace.self) private var ws
    @State private var fees: [String: String] = [:]

    var body: some View {
        if let d = ws.multisigDraft {
            let n = d.keys.count, m = d.threshold
            VStack(alignment: .leading, spacing: 0) {
                KeyholesPicture(total: n, lit: m, chip: "Ready · waits for Confirm")
                VStack(alignment: .leading, spacing: 14) {
                    VStack(alignment: .leading, spacing: 5) {
                        MultisigHeading(verb: "NEW MULTISIG", detail: "Safe · " + (d.networks.count == 1 ? WalletCommand.placeName(d.networks[0]) : "\(d.networks.count) networks"))
                        Text(m == n ? "All \(n) keys move the money" : "Any \(m) of \(n) keys move the money").font(.ui(19, .bold)).lineLimit(1).minimumScaleFactor(0.8)
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        SectionLabel(text: "Keys · one on each device", trailing: "\(n) keys")
                        ForEach(Array(d.keys.enumerated()), id: \.element.id) { i, key in
                            KeyRow(key: key, role: i < m ? "signs" : "spare", signs: i < m,
                                   remove: key.kind == "mac" || n <= 1 ? nil : { ws.updateMultisigDraft { $0.keys.removeAll { $0.address == key.address }; $0.threshold = $0.majority } })
                        }
                        let candidates = ws.multisigCandidates
                        Flow(spacing: 6) {
                            Label("Add a key:", systemImage: "plus").font(.ui(11.5, .semibold)).foregroundStyle(.secondary).frame(height: 30)
                            ForEach(candidates) { c in
                                Button { ws.updateMultisigDraft { $0.keys.append(c); $0.threshold = $0.majority } } label: {
                                    Label(c.name, systemImage: c.kind == "ledger" ? "externaldrive" : "person.crop.circle").font(.ui(11.5, .semibold))
                                        .padding(.horizontal, 10).frame(height: 30)
                                        .overlay(Capsule().strokeBorder(style: StrokeStyle(lineWidth: 1.2, dash: [4, 3])).foregroundStyle(.secondary))
                                        .contentShape(Capsule())
                                }
                                .buttonStyle(.plain).pointerStyle(.link)
                            }
                            Button { ws.suggest("save 0x… as Phone on base") } label: {
                                Label(candidates.isEmpty ? "Save a phone's or a person's address first" : "Another address…", systemImage: "square.and.pencil").font(.ui(11.5))
                                    .padding(.horizontal, 10).frame(height: 30)
                            }
                            .buttonStyle(.plain).foregroundStyle(.secondary).pointerStyle(.link)
                        }
                        Text("Checked: no two keys from one phrase, one key at most on this Mac, every address a plain wallet saved with \(Presence.name).")
                            .font(.ui(10.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        SectionLabel(text: "How many keys move money")
                        HStack(spacing: 12) {
                            Button { ws.updateMultisigDraft { $0.threshold -= 1 } } label: { Image(systemName: "minus").frame(width: 34, height: 34) }
                                .buttonStyle(.bordered).disabled(m <= 1).accessibilityLabel("One signature fewer")
                            HStack(alignment: .firstTextBaseline, spacing: 5) {
                                Text("\(m)").font(.ui(24, .heavy)).monospacedDigit()
                                Text("of \(n)").font(.ui(14, .semibold)).foregroundStyle(.secondary)
                            }
                            .frame(maxWidth: .infinity)
                            Button { ws.updateMultisigDraft { $0.threshold += 1 } } label: { Image(systemName: "plus").frame(width: 34, height: 34) }
                                .buttonStyle(.bordered).disabled(m >= n).accessibilityLabel("One signature more")
                        }
                        .padding(10)
                        .background(RoundedRectangle(cornerRadius: 14).fill(Brand.accent.opacity(0.08)))
                        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Brand.accent, lineWidth: 1.5))
                        HStack(alignment: .top, spacing: 10) {
                            Text(risk(n: n, m: m)).font(.ui(12)).foregroundStyle(m == n || m == 1 ? Brand.loss : .primary.opacity(0.8))
                                .fixedSize(horizontal: false, vertical: true)
                            Spacer(minLength: 6)
                            if m == d.majority {
                                Text("Recommended").font(.ui(10.5, .bold)).foregroundStyle(Brand.onAccent).padding(.horizontal, 8).frame(height: 20)
                                    .background(Capsule().fill(Brand.accent))
                            } else {
                                Button("Use \(d.majority) of \(n)") { ws.updateMultisigDraft { $0.threshold = $0.majority } }.buttonStyle(.bordered).font(.ui(11.5, .semibold))
                            }
                        }
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        SectionLabel(text: "Networks · pick one or several", trailing: "same address on each")
                        Flow(spacing: 6) {
                            ForEach(Workspace.multisigNetworks, id: \.self) { key in
                                let on = d.networks.contains(key)
                                Button {
                                    ws.updateMultisigDraft { x in
                                        if on { if x.networks.count > 1 { x.networks.removeAll { $0 == key } } } else { x.networks.append(key) }
                                    }
                                } label: {
                                    HStack(spacing: 6) {
                                        if let logo = ModuleNetwork(place: key)?.logo { TokenLogo(name: logo, size: 18) }
                                        Text(WalletCommand.placeName(key)).font(.ui(12, .semibold))
                                        if on, let fee = fees[key] { Text(fee).font(.ui(11)).foregroundStyle(.secondary) }
                                    }
                                    .padding(.horizontal, 9).frame(height: 32)
                                    .background(RoundedRectangle(cornerRadius: 10).fill(on ? Brand.accent.opacity(0.1) : Color.clear))
                                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(on ? Brand.accent : Color.primary.opacity(0.15), lineWidth: on ? 1.5 : 1))
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain).pointerStyle(.link).accessibilityAddTraits(on ? .isSelected : [])
                            }
                        }
                        Text("Only networks where Safe runs. Add another later, or say it in the chat.").font(.ui(10.5)).foregroundStyle(.secondary)
                    }
                    if let address = d.address {
                        HStack(spacing: 8) {
                            Text("Address").font(.ui(11.5)).foregroundStyle(.secondary)
                            Text(address).font(.system(size: 11, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                            Spacer(minLength: 4)
                            CopyButton(title: "Copy", text: address)
                        }
                    }
                    Label(foot(d), systemImage: "info.circle").font(.ui(11.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 18).padding(.vertical, 16)
            }
            .frame(width: 492)
            .background(RoundedRectangle(cornerRadius: 20).fill(Color(light: 0xFFFFFF, dark: 0x1C1C20)))
            .clipShape(RoundedRectangle(cornerRadius: 20))
            .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(Color.primary.opacity(0.1)))
            .task(id: d.networks + [String(n)]) { fees = await ws.multisigFees(d) }
        }
    }

    func risk(n: Int, m: Int) -> String {
        if m == n { return n == 1 ? "One key, one device: a multisig only matters with two keys or more." : "Every key, every time. Lose any one device and the money stays locked for good." }
        if m == 1 { return "One key moves everything: whoever gets any one device takes the money." }
        return "Lose up to \(n - m) \(n - m == 1 ? "device" : "devices") and you still get in. A thief needs \(m) of them at once."
    }

    func foot(_ d: MultisigDraft) -> String {
        func names(_ keys: ArraySlice<Multisig.Key>) -> String { keys.map { $0.kind == "mac" ? "\(Presence.short) on this Mac" : $0.kind == "ledger" ? "the Ledger" : $0.name }.joined(separator: " and ") }
        let spare = d.keys.dropFirst(d.threshold)
        return "Every send: \(names(d.keys.prefix(d.threshold)))." + (spare.isEmpty ? "" : " Spare: \(names(spare)).") + " Making it costs only the network fee; Safe takes nothing."
    }
}

/// The multisig on the canvas, opened from the wallet menu or by «show the multisig»: what it holds on each network, its
/// keys, what sends from it.
struct MultisigBoard: View {
    @Environment(Workspace.self) private var ws
    @State private var held: [String: [Holding]] = [:]
    @State private var loading = true

    var body: some View {
        if let m = ws.multisigs.last {
            let total = held.values.flatMap { $0 }.compactMap(\.usd).reduce(0, +)
            VStack(alignment: .leading, spacing: 0) {
                ZStack(alignment: .topTrailing) {
                    KeyholesPicture(total: m.keys.count, lit: m.threshold, chip: "Ready", done: true)
                    Button { withAnimation(StudioView.spring) { ws.detail = nil } } label: { Image(systemName: "xmark").foregroundStyle(.white) }
                        .buttonStyle(.hoverIcon).padding(10).help("Close").accessibilityLabel("Close the multisig")
                }
                VStack(alignment: .leading, spacing: 14) {
                    VStack(alignment: .leading, spacing: 6) {
                        MultisigHeading(verb: "MULTISIG", detail: "Safe · any \(m.threshold) of \(m.keys.count) keys")
                        HStack(spacing: 10) {
                            Text(loading && held.isEmpty ? "…" : Fmt.usd(total)).font(.ui(26, .bold)).monospacedDigit()
                            Spacer(minLength: 6)
                            Text("\(m.address.prefix(6))…\(m.address.suffix(4))").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(.secondary)
                            CopyButton(title: "Copy", text: m.address)
                        }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        SectionLabel(text: "By network · same address")
                        ForEach(m.networks, id: \.self) { key in
                            let list = (held[key] ?? []).filter { $0.looksReal || ($0.usd ?? 0) >= Decimal(string: "0.01")! }
                            HStack(spacing: 10) {
                                if let logo = ModuleNetwork(place: key)?.logo { TokenLogo(name: logo, size: 22) }
                                Text(WalletCommand.placeName(key)).font(.ui(13, .semibold))
                                Spacer(minLength: 6)
                                Text(list.isEmpty ? (loading ? "…" : "empty") : list.map { "\(Fmt.num($0.amount, maxFraction: 6)) \($0.symbol)" }.joined(separator: " · "))
                                    .font(.ui(12)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.tail)
                                Text(Fmt.usd(list.compactMap(\.usd).reduce(0, +))).font(.ui(13, .semibold)).monospacedDigit()
                            }
                            .padding(.horizontal, 10).padding(.vertical, 8)
                            .background(RoundedRectangle(cornerRadius: 12).fill(Color.primary.opacity(0.035)))
                        }
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        SectionLabel(text: "Keys · any \(m.threshold) of \(m.keys.count) move money")
                        ForEach(m.keys) { key in
                            KeyRow(key: key, role: key.kind == "address" ? "elsewhere" : "signs here", signs: key.kind != "address")
                        }
                    }
                    Flow(spacing: 8) {
                        Button { ws.suggest("send 10 USDC from the multisig to ") } label: { Label("Send", systemImage: "arrow.up") }
                            .buttonStyle(.borderedProminent).tint(Color.primary)
                        Button { ws.suggest("send 10 USDC on \(m.networks.first ?? "base") to Multisig") } label: { Label("Put money in", systemImage: "arrow.down") }
                            .buttonStyle(.bordered)
                        Button { if let url = URL(string: "https://app.safe.global/home?safe=\(Self.safePrefix(m.networks.first ?? "base")):\(m.address)") { ws.openWeb(url) } } label: {
                            Label("Open in Safe", systemImage: "arrow.up.right.square")
                        }
                        .buttonStyle(.bordered)
                        if let net = EVMNetwork.named(m.networks.first ?? "base") {
                            Button { if let url = URL(string: "\(net.explorer)/address/\(m.address)") { ws.openWeb(url) } } label: { Label("History", systemImage: "clock") }.buttonStyle(.bordered)
                        }
                    }
                    .font(.ui(12.5, .semibold))
                    Text("Its keys and how many must sign change only with that many signatures, in Safe's own app for now.")
                        .font(.ui(10.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 18).padding(.vertical, 16)
            }
            .frame(width: 492)
            .background(RoundedRectangle(cornerRadius: 20).fill(Color(light: 0xFFFFFF, dark: 0x1C1C20)))
            .clipShape(RoundedRectangle(cornerRadius: 20))
            .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(Color.primary.opacity(0.1)))
            .task(id: m.address + m.networks.joined()) {
                loading = true
                held = await Self.holdings(m)
                loading = false
                ws.multisigUSD[m.address] = held.values.flatMap { $0 }.compactMap(\.usd).reduce(0, +)
            }
        }
    }

    /// What a multisig holds on each of its networks: the coin and the network's USDC read from the chain itself (an
    /// explorer lists a new address minutes late), the other tokens from the explorer.
    static func holdings(_ m: Multisig) async -> [String: [Holding]] {
        var out: [String: [Holding]] = [:]
        for key in m.networks {
            guard let net = EVMNetwork.named(key) else { continue }
            var list = (await Holdings.evm(key, address: m.address, native: false, skip: [net.usdc.lowercased()]) ?? []).filter { !$0.native }
            if let raw = (try? await net.rpc.read(net.usdc, ERC20.balanceOf(m.address))).flatMap({ ABI.decimal(in: $0, word: 0) }), raw > 0 {
                list.insert(Holdings.dollar(key, contract: net.usdc, amount: raw / pow(10, net.usdcDecimals), icon: nil), at: 0)
            }
            if net.feeToken == nil, let wei = try? await net.rpc.balance(m.address), wei > 0 {
                let price = (try? await Relay.nativePrice(chainId: net.rpc.chainId)) ?? nil
                list.insert(Holding(chain: key, contract: "native", amount: Decimal(wei) / 1_000_000_000_000_000_000, symbol: net.gasCoin,
                                    price: price, verified: true, native: true), at: 0)
            }
            out[key] = list
        }
        return out
    }

    /// What a multisig holds on all its networks, in dollars.
    static func usd(_ m: Multisig) async -> Decimal? {
        await holdings(m).values.flatMap { $0 }.compactMap(\.usd).reduce(0, +)
    }

    /// Safe's app names networks by its own short names.
    static func safePrefix(_ key: String) -> String {
        ["base": "base", "arbitrum": "arb1", "optimism": "oeth", "ethereum": "eth", "polygon": "matic", "bnb": "bnb", "hyperevm": "hyper", "robinhood": "robinhood", "tempo": "tempo"][key] ?? key
    }
}

/// The multisig in the portfolio, under the networks: its keys, where it is, what it holds; it opens its board.
struct MultisigPortfolioCard: View {
    @Environment(Workspace.self) private var ws
    let multisig: Multisig

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "lock.square.stack").font(.system(size: 15, weight: .semibold)).foregroundStyle(Color(hex: 0x12FF80))
                .frame(width: 34, height: 34).background(RoundedRectangle(cornerRadius: 10).fill(Color(hex: 0x121312)))
            VStack(alignment: .leading, spacing: 2) {
                Text("Multisig").font(.ui(14, .semibold))
                Text("Safe · any \(multisig.threshold) of \(multisig.keys.count) keys · " + multisig.networks.map(WalletCommand.placeName).joined(separator: ", "))
                    .font(.ui(10)).foregroundStyle(.secondary).lineLimit(2).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 4)
            Text(ws.multisigUSD[multisig.address].map { Fmt.usd($0) } ?? "…").font(.ui(13, .bold)).monospacedDigit().lineLimit(1).fixedSize()
        }
        .padding(11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).strokeBorder(Brand.cardBorder))
        .modifier(Openable(place: "multisig"))
        .task(id: multisig.address + multisig.networks.joined() + String(ws.updatedAt?.timeIntervalSince1970 ?? 0)) {
            if let usd = await MultisigBoard.usd(multisig) { ws.multisigUSD[multisig.address] = usd }
        }
    }
}

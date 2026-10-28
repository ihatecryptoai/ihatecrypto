import Foundation
import IhatecryptoCore
import SignerAPI
import SwiftUI
import TradeExecution
import WalletCore

/// Several wallets in the signer, one of which signs. The signer owns that choice (switching takes Touch ID there);
/// the app follows it: its addresses, balances and positions, and what it keeps on this Mac per wallet.
extension Workspace {
    /// A per-wallet key for state kept on this Mac; wallet «1» keeps the names it had before there were several.
    static func scoped(_ base: String, _ id: String? = UserDefaults.standard.string(forKey: "activeWalletID")) -> String {
        guard let id, id != SignerWallet.firstID else { return base }
        return "\(base)-\(id)"
    }

    var activeWallet: SignerWallet? { wallets.first { $0.id == activeWalletID } }

    /// Another wallet signs now. Nothing of the last one may carry over: a card waiting for Confirm is dropped (it was
    /// built for the other wallet), its balances and positions go, and its exit rules, recurring buys and portfolio
    /// curve are put away under its id while the new wallet's come back.
    func walletChanged(to id: String?) {
        activeWalletID = id
        pendingAction = nil
        pendingText = nil
        // Cards parked on other canvases too: back on one, it would come back and sign from this wallet.
        parkedPending = [:]
        chainAhead = nil
        pendingAllocation = nil
        allocationPreview = nil
        awaitingAmount = nil
        takeChoices = nil
        positions = []
        orders = []
        recentlyClosed = nil
        hl = nil
        dexBalances = [:]
        hlp = (0, nil)
        solana = nil
        lend = []
        lendDeposits = []
        lendEarned = [:]
        lendEarnedAt = nil
        kamino = 0
        kaminoDeposits = []
        kaminoLoan = nil
        kaminoLoanRead = false
        near = 0
        base = nil
        arbitrumUSDC = nil
        robinhood = nil
        tokenBalances = [:]
        otherTokens = []
        lendingElsewhere = []
        protocolPositions = []
        protocolsReadAt = nil
        updatedAt = nil
        holdingsLoaded = false
        schedules = UserDefaults.standard.data(forKey: Self.scoped("schedules", id)).flatMap { try? JSONDecoder().decode([Schedule].self, from: $0) } ?? []
        let loaded = TaskSeal.tasks(forKey: Self.scoped("tasks", id))
        tasks = loaded.tasks
        if loaded.tampered { tasksTampered = true }
        taskQueue = nil
        exitRules = UserDefaults.standard.data(forKey: Self.scoped("exitRules", id)).flatMap { try? JSONDecoder().decode([String: ExitRules].self, from: $0) } ?? [:]
        equityHistory = LocalStore.load(Self.scoped("equity", id) + ".json") ?? []
        pnlMarks = LocalStore.load(Self.scoped("pnlmarks", id) + ".json")
        costBasis = LocalStore.load(Self.scoped("costbasis", id) + ".json") ?? [:]
        chainCost = [:]
    }

    /// A wallet removed from the signer: what this Mac kept under its id goes too. The next wallet made into an empty
    /// list gets id «1» again, and would run the old one's recurring buys and tasks with its own money and show its P/L.
    /// (flows.json is kept by address: another wallet never reads it.)
    func walletRemoved(_ id: String) {
        if id == activeWalletID {
            schedules = []
            tasks = []
            taskQueue = nil
            exitRules = [:]
            equityHistory = []
            pnlMarks = nil
            costBasis = [:]
        }
        lastBalance[id] = nil
        for key in ["schedules", "tasks", "exitRules"] { UserDefaults.standard.removeObject(forKey: Self.scoped(key, id)) }
        for file in ["equity", "pnlmarks", "costbasis"] { LocalStore.remove(Self.scoped(file, id) + ".json") }
    }

    /// Asks the signer to sign with `wallet` (Touch ID there), then reads everything for it.
    func switchWallet(_ wallet: SignerWallet, note: String? = nil) async {
        guard wallet.id != activeWalletID else {
            reply("«\(wallet.label)» is already the wallet ihatecrypto signs with.", note: note)
            return
        }
        do {
            // To the Ledger: no Touch ID, the device asks for its buttons on each move. Back from it: the signer is
            // asked only when another of its wallets is wanted than the one it had.
            if wallet.kind != "ledger", try await SignerClient().send(.status).activeWallet != wallet.id {
                let reply = try await SignerClient().send(.setActiveWallet(id: wallet.id))
                guard reply.ok else { throw WalletError(reply.error ?? "The signer refused.") }
            }
            LedgerAccount.active = wallet.kind == "ledger"
            await loadSigner()
            self.reply("Now signing with «\(wallet.label)»: \(Self.describe(wallet)).", note: note)
            await refresh()
        } catch {
            reply("The wallet wasn't switched: \(error.localizedDescription)", note: note)
        }
    }

    /// «Main · phrase: Solana AN5b…e8cL, EVM 0x1523…F656» — what a wallet can sign for.
    static func describe(_ w: SignerWallet) -> String {
        func short(_ s: String) -> String { "\(s.prefix(4))…\(s.suffix(4))" }
        let parts = [w.solana.map { "Solana \(short($0))" }, w.evm.map { "EVM \(short($0))" }, w.near.map { "NEAR \(short($0))" }].compactMap { $0 }
        return "\(kindName(w)) · " + parts.joined(separator: ", ")
    }

    /// «recovery phrase», «Ledger, confirm on the device»…: what kind of key a wallet is.
    static func kindName(_ w: SignerWallet) -> String {
        w.kind == "phrase" ? "recovery phrase" : w.kind == "ledger" ? "Ledger, confirm on the device" : w.kind == "solana" ? "Solana key only" : "EVM key only"
    }

    /// «переключись на кошелёк Main», «switch to wallet Trading»: the wallet it names, by its name. An exact app
    /// command like «new canvas», read before the model so a wallet's name can't be taken for something else.
    func walletSwitch(_ text: String) -> SignerWallet?? {
        guard let m = text.firstMatch(of: /(?i)^\s*(?:переключ\w*|switch|use|используй)\s+(?:на\s+|to\s+)?(?:кошел[её]к\w*|wallet)\s+[«"“]?(.+?)[»"”]?\s*[.!]?\s*$/) else { return nil }
        let name = String(m.1).lowercased()
        return .some(wallets.first { $0.label.lowercased() == name } ?? wallets.first { $0.label.lowercased().hasPrefix(name) })
    }

    /// The key a wallet command needs that the active wallet doesn't have, said plainly. A phrase wallet has them all;
    /// a key wallet only its own chain's.
    func missingKey(for command: WalletCommand) -> String? {
        guard let wallet = activeWallet, wallet.kind != "phrase" else { return nil }
        let chains: [String] = switch command {
        case .yields, .receive, .stopSchedules: []
        case .hlp: ["evm"]
        case .withdrawToSolana, .aave, .morpho, .baseToSolana, .arbitrumToSolana, .evmToSolana, .robinhoodToSolana, .solanaTo, .buyStock, .sellStock, .buyOn, .sellOn, .pay, .claimRewards, .sweepRewards: ["evm", "solana"]
        case .swapOn("solana", _, _, _): ["solana"]
        case .swapOn("near", _, _, _): ["near"]
        case .bridge, .cctp, .usdt, .aaveBorrow, .aaveRepay, .aaveCollateral, .swapOn, .sweepDust, .revokeApproval, .hlSpotBuy, .hlSpotSell: ["evm"]
        case .showApprovals, .multisigShow: []
        case .multisigCreate, .multisigSend: ["evm"]
        case .crossSwap(_, let from, _, let to, _): Array(Set([from, to].map { $0 == "solana" || $0 == "near" ? $0 : "evm" })).sorted()
        case .gas: ["evm", "solana"]
        case .sendNear, .closeNear: ["near"]
        case .buyNear, .sellNear: ["solana", "near"]
        default: ["solana"]   // spot, vaults, staking, sends: all on Solana
        }
        return chains.lazy.compactMap { wallet.missing($0) }.first
    }

    /// Trading on Hyperliquid (plans, positions) signs with the EVM key.
    var missingTradingKey: String? { activeWallet?.missing("evm") }
}

/// Near the balance: which wallet signs, by its name, with a lock for a Touch ID wallet and «TEST» for a test wallet
/// (no Touch ID). A click opens the wallets: the list on the left, the picked one's addresses on every network on the right.
struct WalletChip: View {
    @Environment(Workspace.self) private var ws
    var compact = false
    @State private var open = false

    var body: some View {
        if let w = ws.activeWallet {
            Button { open.toggle() } label: {
                HStack(spacing: 4) {
                    Image(systemName: w.isTest ? "flask.fill" : "lock.fill").font(.system(size: compact ? 8 : 9, weight: .bold))
                    Text(compact ? String(w.label.prefix(7)) : w.label).font(.ui(compact ? 9 : 11, .semibold)).lineLimit(1)
                    if !compact { Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold)).foregroundStyle(.secondary) }
                }
                .foregroundStyle(w.isTest ? Color.orange : Color.primary.opacity(0.75))
                .padding(.horizontal, compact ? 6 : 8).frame(height: compact ? 18 : 22)
                .background(Capsule().fill(w.isTest ? Color.orange.opacity(0.14) : Brand.glassFill))
                .overlay(Capsule().strokeBorder(open ? Brand.accent.opacity(0.8) : w.isTest ? Color.orange.opacity(0.45) : Brand.glassRim))
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .modifier(HoverLift(radius: 11))
            .help(w.isTest ? "«\(w.label)» signs without \(Presence.name) — a test wallet. Click to switch or rename." : "«\(w.label)» signs, with \(Presence.name). Click to switch or rename.")
            .popover(isPresented: $open, arrowEdge: compact ? .trailing : .bottom) { WalletMenu { open = false } }
        }
    }
}

/// The wallet menu (W04, picked 2026-10-20): wallets and multisigs on the left; a click shows one on the right — a
/// wallet's address on every network with Copy and «Switch to…», a multisig's address, networks and keys. No «New test
/// wallet» here: release users won't have one (the owner); Settings › Wallet still makes it.
private struct WalletMenu: View {
    @Environment(Workspace.self) private var ws
    let close: () -> Void
    /// What the right side shows: a wallet's id, «ms:» + a multisig's address, or «new-ms»; nil = the current wallet.
    @State private var picked: String?
    @State private var hovered: String?
    @State private var renaming: String?
    @State private var name = ""
    @State private var error: String?

    /// The automation wallet lives in the Schedule (its budget, addresses, top-up), not among the wallets to switch to.
    private var wallets: [SignerWallet] { ws.wallets.filter { !$0.isAutomation } }
    private var shown: String { picked ?? ws.activeWalletID ?? wallets.first?.id ?? "" }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            list.frame(width: 230).frame(maxHeight: .infinity, alignment: .top)
            // Wide enough for a NEAR address (64 characters) in two lines.
            detail.frame(width: 500)
        }
        .fixedSize(horizontal: false, vertical: true)
        .padding(12)
        // Solid: through the popover's glass an orange board behind tinted the grey lines orange (the owner's screenshot).
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var list: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text("Wallets").font(.ui(15, .bold))
                Spacer()
                Button { close(); ws.openSettings(.wallet) } label: { Image(systemName: "gearshape") }
                    .buttonStyle(.hoverIcon).help("Manage wallets: create, import, remove").accessibilityLabel("Manage wallets")
            }
            .padding(.leading, 8).padding(.bottom, 4)
            ForEach(wallets) { w in
                row(w.id, title: w.label, line: listLine(w), current: w.id == ws.activeWalletID) { walletIcon(w) }
            }
            if !ws.multisigs.isEmpty {
                Text("MULTISIG").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary).padding(.horizontal, 8).padding(.top, 10).padding(.bottom, 2)
                ForEach(ws.multisigs) { m in
                    row("ms:" + m.address, title: "Multisig", line: "\(m.threshold) of \(m.keys.count) keys · " + m.networks.map(networkName).joined(separator: ", "), current: false) {
                        Image(systemName: "lock.shield").font(.system(size: 13, weight: .semibold)).foregroundStyle(.secondary)
                    }
                }
            }
            if let error { Text(error).font(.ui(10)).foregroundStyle(.red).padding(.horizontal, 8).fixedSize(horizontal: false, vertical: true) }
            Spacer(minLength: 14)
            Divider().padding(.horizontal, 8).padding(.bottom, 4)
            if LedgerAccount.saved == nil {
                action("Connect Ledger…", icon: "cable.connector", help: "Keys stay on the device; you approve each move on it") {
                    close()
                    Task { await ws.connectLedger() }
                }
            }
            action("New wallet…", icon: "plus", help: "Create or import a wallet in Settings") { close(); ws.openSettings(.wallet) }
            action("New multisig…", icon: "lock.shield", id: "new-ms", help: "What a multisig is, and how to make one") { picked = "new-ms"; renaming = nil }
        }
    }

    private func row<Icon: View>(_ id: String, title: String, line: String, current: Bool, @ViewBuilder icon: () -> Icon) -> some View {
        let on = shown == id
        return Button { picked = id; renaming = nil } label: {
            HStack(spacing: 10) {
                icon().frame(width: 30, height: 30).background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.06)))
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).font(.ui(13, .semibold)).lineLimit(1)
                    Text(line).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                if current {
                    Image(systemName: "checkmark").font(.system(size: 12, weight: .bold)).foregroundStyle(Brand.accentInk).accessibilityLabel("Current wallet")
                }
            }
            .padding(8)
            .background(RoundedRectangle(cornerRadius: 11).fill(on ? Color.primary.opacity(0.07) : hovered == id ? Color.primary.opacity(0.04) : .clear))
            .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(on ? Brand.accent.opacity(0.55) : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 ? id : (hovered == id ? nil : hovered) }
        .pointerStyle(.link)
    }

    private func action(_ title: String, icon: String, id: String? = nil, help: String, _ run: @escaping () -> Void) -> some View {
        let on = id != nil && shown == id, key = id ?? title
        return Button(action: run) {
            Label(title, systemImage: icon).font(.ui(12.5, .semibold))
                .frame(maxWidth: .infinity, minHeight: 34, alignment: .leading).padding(.horizontal, 10)
                .background(RoundedRectangle(cornerRadius: 9).fill(on ? Color.primary.opacity(0.07) : hovered == key ? Color.primary.opacity(0.04) : .clear))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(on ? Brand.accent.opacity(0.55) : .clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain).help(help)
        .onHover { hovered = $0 ? key : (hovered == key ? nil : hovered) }
    }

    @ViewBuilder private func walletIcon(_ w: SignerWallet) -> some View {
        if w.kind == "ledger" { LedgerGlyph(size: 16) } else {
            Image(systemName: w.isTest ? "flask.fill" : "lock.fill").font(.system(size: 13, weight: .semibold))
                .foregroundStyle(w.isTest ? Color.orange : Color.secondary)
        }
    }

    private func listLine(_ w: SignerWallet) -> String {
        let n = networks(w).filter { $0.address != nil }.count
        let how = w.kind == "ledger" ? "On the device" : w.isTest ? "No \(Presence.short)" : Presence.short
        return "\(how) · \(n) network\(n == 1 ? "" : "s")"
    }

    private struct Net: Identifiable {
        let id, name, note, logo: String
        let address: String?
        let missing: String
    }

    /// Solana, EVM (one address on all nine), Hyperliquid (the EVM address again: its account is that key) and NEAR.
    private func networks(_ w: SignerWallet) -> [Net] {
        let place = w.kind == "ledger" ? "on a Ledger" : "in this wallet"
        return [
            Net(id: "solana", name: "Solana", note: "", logo: "sol", address: w.solana, missing: "No Solana key \(place)."),
            Net(id: "evm", name: "EVM", note: "\(EVMNetwork.all.count) networks", logo: "eth", address: w.evm, missing: "No EVM key \(place)."),
            Net(id: "hyperliquid", name: "Hyperliquid", note: "same as EVM", logo: "hype", address: w.evm, missing: "No EVM key \(place)."),
            // A phrase wallet made before NEAR learns its address on the first NEAR move (one unlock).
            Net(id: "near", name: "NEAR", note: "", logo: "near", address: w.near,
                missing: w.kind == "phrase" ? "Shown after its first NEAR move." : "No NEAR key \(place).")
        ]
    }

    private func networkName(_ key: String) -> String { EVMNetwork.all.first { $0.key == key }?.name ?? key.capitalized }

    private var detail: some View {
        Group {
            if shown == "new-ms" { newMultisig }
            else if let m = ws.multisigs.first(where: { "ms:" + $0.address == shown }) { multisig(m) }
            else if let w = wallets.first(where: { $0.id == shown }) { wallet(w) }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(RoundedRectangle(cornerRadius: 14).fill(Color.primary.opacity(0.035)))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Color.primary.opacity(0.07)))
    }

    private func wallet(_ w: SignerWallet) -> some View {
        let current = w.id == ws.activeWalletID
        let how = w.kind == "ledger" ? "on the device, you approve each move on it"
            : w.isTest ? "signs without \(Presence.name), keep only a few dollars here" : "signs each move with \(Presence.name)"
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    if renaming == w.id {
                        TextField("Name", text: $name).textFieldStyle(.roundedBorder).font(.ui(13)).frame(maxWidth: 220).onSubmit { rename(w) }
                    } else {
                        HStack(spacing: 4) {
                            Text(w.label).font(.ui(17, .bold)).lineLimit(1)
                            Button { renaming = w.id; name = w.label } label: { Image(systemName: "pencil") }
                                .buttonStyle(.hoverIcon).help("Rename").accessibilityLabel("Rename \(w.label)")
                        }
                    }
                    Text(current ? "Current wallet · \(how)" : how.prefix(1).uppercased() + how.dropFirst())
                        .font(.ui(12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 8)
                if !current {
                    Button("Switch to \(w.label)") { close(); Task { await ws.switchWallet(w) } }
                        .buttonStyle(.borderedProminent).tint(Brand.accent).lineLimit(1)
                        .help("Switching asks for \(Presence.name).")
                }
            }
            .padding(.bottom, 12)
            ForEach(networks(w)) { netRow($0) }
        }
    }

    private func netRow(_ n: Net) -> some View {
        HStack(spacing: 12) {
            HStack(spacing: 8) {
                TokenLogo(name: n.logo, size: 20, round: true)
                VStack(alignment: .leading, spacing: 1) {
                    Text(n.name).font(.ui(12.5, .semibold))
                    if !n.note.isEmpty { Text(n.note).font(.ui(10.5)).foregroundStyle(.secondary) }
                }
            }
            .frame(width: 118, alignment: .leading)
            .help(n.id == "evm" ? EVMNetwork.all.map(\.name).joined(separator: ", ") : n.name)
            if let a = n.address {
                AddressLines(address: a, size: 12, copy: false).frame(maxWidth: .infinity, alignment: .leading)
                CopyButton(title: "Copy", text: a, help: "Copy the whole \(n.name) address")
            } else {
                Text(n.missing).font(.ui(12)).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.vertical, 10)
        .opacity(n.address == nil ? 0.6 : 1)
        .overlay(alignment: .top) { Divider() }
    }

    private func multisig(_ m: Multisig) -> some View {
        let nets = m.networks.map(networkName)
        let only = nets.count < 2 ? nets.joined() : nets.dropLast().joined(separator: ", ") + " or " + (nets.last ?? "")
        return VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Multisig").font(.ui(17, .bold))
                    Text("Safe · money leaves when any \(m.threshold) of its \(m.keys.count) keys sign").font(.ui(12)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                Button("Open") { close(); withAnimation(StudioView.spring) { ws.detail = "multisig" } }
                    .buttonStyle(.borderedProminent).tint(Brand.accent).help("Its money and payments, on the canvas")
            }
            .padding(.bottom, 12)
            HStack(spacing: 12) {
                HStack(spacing: 8) {
                    HStack(spacing: -5) {
                        ForEach(m.networks.prefix(3), id: \.self) { TokenLogo(name: HoldingRow.chainLogos[$0] ?? "eth", size: 20, round: true) }
                    }
                    VStack(alignment: .leading, spacing: 1) {
                        Text("Address").font(.ui(12.5, .semibold))
                        Text(nets.joined(separator: ", ")).font(.ui(10.5)).foregroundStyle(.secondary).lineLimit(2)
                    }
                }
                .frame(width: 118, alignment: .leading)
                AddressLines(address: m.address, size: 12, copy: false).frame(maxWidth: .infinity, alignment: .leading)
                CopyButton(title: "Copy", text: m.address, help: "Copy the multisig's whole address")
            }
            .padding(.vertical, 10)
            .overlay(alignment: .top) { Divider() }
            HStack(alignment: .top, spacing: 8) {
                Image(systemName: "info.circle").foregroundStyle(Brand.accentInk)
                Text("Send to it only on \(only). It isn't a multisig on other networks.").foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .font(.ui(11.5)).padding(.horizontal, 10).padding(.vertical, 8).frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 9).fill(Brand.accent.opacity(0.08)))
            .padding(.bottom, 10)
            Text("KEYS · ANY \(m.threshold) OF \(m.keys.count) SIGN").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
                .padding(.top, 10).padding(.bottom, 4).frame(maxWidth: .infinity, alignment: .leading).overlay(alignment: .top) { Divider() }
            ForEach(m.keys) { k in
                HStack(spacing: 10) {
                    Group {
                        if k.kind == "ledger" { LedgerGlyph(size: 14) } else { Image(systemName: k.kind == "mac" ? "laptopcomputer" : "iphone").foregroundStyle(.secondary) }
                    }
                    .frame(width: 28, height: 28).background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.06)))
                    VStack(alignment: .leading, spacing: 1) {
                        Text(k.name).font(.ui(12.5, .semibold)).lineLimit(1)
                        Text(k.kind == "mac" ? "\(Presence.short) on this Mac" : k.kind == "ledger" ? "Approve on the device" : "Signs in the Safe app")
                            .font(.ui(11)).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 4)
                    if k.kind != "address" { Text("signs here").font(.ui(11)).foregroundStyle(.secondary) }
                }
                .padding(.vertical, 5)
            }
            Text("A multisig lives on EVM networks only: no Solana, Hyperliquid or NEAR.").font(.ui(11.5)).foregroundStyle(.secondary)
                .padding(.top, 10).frame(maxWidth: .infinity, alignment: .leading).overlay(alignment: .top) { Divider() }
        }
    }

    private var newMultisig: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 3) {
                Text("New multisig").font(.ui(17, .bold))
                Text("A shared safe on EVM networks. Money leaves it only when enough of its keys sign, so one lost or stolen device can't empty it.")
                    .font(.ui(12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            VStack(alignment: .leading, spacing: 12) {
                step(1, "Keys", "This Mac, your Ledger, a phone with the Safe app. As many as you want.")
                step(2, "How many must sign", "More than half is safest: 2 of 3, 3 of 5.")
                step(3, "Networks", "Base, Arbitrum or any of the \(Workspace.multisigNetworks.count) EVM networks. The address is the same on each.")
            }
            .padding(.top, 12).overlay(alignment: .top) { Divider() }
            Spacer(minLength: 0)
            HStack(spacing: 12) {
                Button { close(); ws.suggest("make a multisig") } label: { Label("Make a multisig", systemImage: "plus") }
                    .buttonStyle(.borderedProminent).tint(Brand.accent)
                Text("It goes to the chat; nothing is made until you confirm.").font(.ui(11.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(.top, 12).overlay(alignment: .top) { Divider() }
        }
    }

    private func step(_ n: Int, _ title: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text("\(n)").font(.ui(12, .bold)).frame(width: 24, height: 24).background(Circle().fill(Color.primary.opacity(0.08)))
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.ui(13, .semibold))
                Text(text).font(.ui(12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func rename(_ w: SignerWallet) {
        let label = name
        renaming = nil
        Task {
            do {
                let reply = try await SignerClient().send(.renameWallet(id: w.id, label: label))
                guard reply.ok else { throw WalletError(reply.error ?? "The signer refused.") }
                await ws.loadSigner()
            } catch { self.error = error.localizedDescription }
        }
    }
}

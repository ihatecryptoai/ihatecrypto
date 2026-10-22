import Foundation
import HyperliquidSigning
import Ledger
import IhatecryptoCore
import SignerAPI
import SwiftUI
import TradeExecution
import WalletCore

/// A Ledger the user connected: its public addresses only, on this Mac. It is one more wallet in the list, beside the
/// ones the signing process holds; while it is the active one, every move is signed on the device, where the person
/// reads it and presses the buttons. Nothing about it is secret, so it needs no Keychain and no signer.
struct LedgerAccount: Codable, Equatable {
    static let id = "ledger"
    var label = "Ledger"
    var model: String?
    var evm: String?
    var solana: String?
    var evmPath = LedgerDevice.evmPath
    var solanaPath = LedgerDevice.solanaPath
    /// Found over Bluetooth rather than USB; connecting again picks whichever answers.
    var bluetooth: Bool?

    var wallet: SignerWallet { SignerWallet(id: Self.id, label: label, kind: "ledger", solana: solana, evm: evm, near: nil) }
    var device: LedgerDevice { LedgerDevice(bluetooth == true ? LedgerBLE.shared : LedgerHID()) }

    static var saved: LedgerAccount? {
        get { UserDefaults.standard.data(forKey: "ledger").flatMap { try? JSONDecoder().decode(LedgerAccount.self, from: $0) } }
        set { UserDefaults.standard.set(newValue.flatMap { try? JSONEncoder().encode($0) }, forKey: "ledger") }
    }

    /// Whether the Ledger, not the signing process, signs now. Switching to it needs no Touch ID: the device asks for
    /// its buttons on every move anyway.
    static var active: Bool {
        get { saved != nil && UserDefaults.standard.bool(forKey: "ledgerActive") }
        set { UserDefaults.standard.set(newValue, forKey: "ledgerActive") }
    }
}

/// The connect flow's progress on the canvas.
struct LedgerConnect: Equatable {
    var model: String?
    var evm: String?
    var solana: String?
    /// What the person has to do now, or what just happened.
    var status = "Looking for a Ledger on USB…"
    var problem: String?
    /// Settings on the device the person should change (blind signing off, an app not installed).
    var notes: [String] = []
    var busy = true
}

extension Workspace {
    var ledgerActive: Bool { activeWallet?.kind == "ledger" }

    /// The signer a move uses while the Ledger is active; the canvas shows what the device waits for. With the trading
    /// key on, orders, cancels and leverage go to it in the signing process instead of the device.
    func ledgerSigner(_ wallet: TradeWallet) -> LedgerSigner {
        let account = LedgerAccount.saved ?? LedgerAccount()
        let owner = wallet.evm
        var agent: (@Sendable (MsgPack, UInt64) async throws -> EVMSignature)?
        if agentActive {
            agent = { @Sendable action, nonce in
                let r = try await SignerClient().send(.signLedgerAgentL1(owner: owner, action: action, nonce: nonce))
                guard r.ok, let signature = r.signature else { throw GuardStop(XPCTradeSigner.reason(r)) }
                return signature
            }
        }
        return LedgerSigner(device: account.device, wallet: wallet, evmPath: account.evmPath, solanaPath: account.solanaPath, agent: agent) { text in
            Task { @MainActor in SignerActivity.shared.ledger = text }
        }
    }

    /// The trading key for the Ledger's Hyperliquid account: made in the signing process, approved once on the Ledger
    /// (which shows its address), sent to Hyperliquid. Afterwards orders, stops, trailing and closes go without the
    /// device; withdrawals and transfers are still signed on it.
    func enableLedgerAgent() async {
        guard ledgerActive, let wallet = signerWallet, isEVMAddress(wallet.evm), let account = LedgerAccount.saved else {
            reply("Switch to your Ledger first (“use my Ledger”): the trading key belongs to its Hyperliquid account.")
            return
        }
        // The Ledger's card follows the approval: what the device shows and waits for.
        if ledgerConnect == nil {
            ledgerConnect = LedgerConnect(model: account.model, evm: account.evm, solana: account.solana, status: "Making the trading key…", busy: true)
        }
        defer { ledgerConnect?.busy = false }
        executing = true
        defer { executing = false }
        do {
            let made = try await SignerClient().send(.ledgerAgent(owner: wallet.evm, create: true))
            guard made.ok, let agent = made.agentAddress else { throw GuardStop(XPCTradeSigner.reason(made)) }
            if made.agentActive == true {
                reply("Your Ledger's trading key is already on: \(agent.prefix(6))…\(agent.suffix(4)).")
                return
            }
            reply("Approve the trading key on your Ledger. Its screen must show agentAddress \(agent.prefix(6))…\(agent.suffix(4)) and agentName ihatecrypto.")
            ledgerConnect?.status = "Approve the trading key on your Ledger: agentAddress \(agent.prefix(6))…\(agent.suffix(4))."
            let nonce = UInt64(Date().timeIntervalSince1970 * 1000)
            let (action, signature) = try await ledgerSigner(wallet).approveAgent(agent, name: "ihatecrypto", nonce: nonce)
            let result = HLReply.parse(try await HLExchange.send(action: action, nonce: nonce, signature: signature, mainnet: true))
            guard !result.isError else { throw ExecutionError("Hyperliquid rejected the key: \(result). Its account needs a deposit first.") }
            let on = try await SignerClient().send(.setLedgerAgentActive(owner: wallet.evm, active: true))
            guard on.ok else { throw GuardStop(XPCTradeSigner.reason(on)) }
            await loadSigner()
            ledgerConnect?.status = "Trading key on: orders go without the Ledger's buttons."
            reply("Trading key on for your Ledger: \(agent.prefix(10))…. Orders, stops, trailing and closes now go without the device; withdrawals and transfers are still signed on the Ledger. “lock” pauses it.")
        } catch {
            ledgerConnect?.status = "The trading key stayed off."
            reply("Trading key not turned on: \(error.localizedDescription)")
        }
    }

    /// «connect ledger», «подключи леджер», «use my Ledger»: true when the message was about the Ledger.
    func ledgerCommand(_ text: String) async -> Bool {
        let lower = text.lowercased()
        let aboutLedger = lower.contains(/ledger|л[еэ]дж[еэ]р/)
        // «approve trading key» while the Ledger signs: its own key, approved on the device. «resume the trading key»
        // after a lock stays with the usual command (Touch ID).
        if aboutLedger || ledgerActive, lower.contains(/trading\s+key|торгов\w*\s+ключ/), !lower.contains(/resume|unpause|верни/) {
            Task { await enableLedgerAgent() }
            return true
        }
        guard aboutLedger else { return false }
        if lower.wholeMatch(of: /\s*(?:use|switch\s+to|sign\s+with|используй|переключи\w*\s+на|подписывай\s+(?:с|на))\s+(?:my\s+|мой\s+|моём\s+|мо[йеё]м?\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)\s*[.!]?\s*/) != nil,
           let account = LedgerAccount.saved {
            await switchWallet(account.wallet)
            return true
        }
        if lower.wholeMatch(of: /\s*(?:forget|remove|disconnect|забудь|удали|отключи)\s+(?:my\s+|мой\s+)?(?:ledger|л[еэ]дж[еэ]р\w*)\s*[.!]?\s*/) != nil {
            await forgetLedger()
            return true
        }
        if lower.wholeMatch(of: /\s*(?:(?:connect|add|pair|set\s*up|link|подключи\w*|добавь|привяжи|подсоедини)\s+(?:my\s+|a\s+|мой\s+)?)?(?:ledger|л[еэ]дж[еэ]р\w*)(?:\s+(?:nano\s*\w*|flex|stax))?\s*[.!?]?\s*/) != nil {
            // The card on the canvas follows it; the chat stays free while the person is at the device.
            Task { await connectLedger() }
            return true
        }
        return false
    }

    /// Finds the Ledger, opens its Ethereum and Solana apps in turn and reads one address from each. Nothing is signed;
    /// the addresses become the «Ledger» wallet in the list.
    func connectLedger() async {
        ledgerConnect = LedgerConnect()
        reply(say("Connecting your Ledger — follow the card on the canvas.", "Подключаю Ledger — следи за карточкой на холсте."))
        // USB when it's plugged in; otherwise Bluetooth (Nano X, Stax, Flex, Apex), looked for in short rounds.
        var found = LedgerHID.connected()
        var bluetooth = false
        for _ in 0..<12 where found == nil {
            ledgerConnect?.status = "Plug your Ledger in over USB and unlock it — or, on a Nano X, Stax or Flex, unlock it with Bluetooth on and keep it close."
            if let model = await LedgerBLE.shared.connect(timeout: 4) { found = model; bluetooth = true; break }
            try? await Task.sleep(for: .seconds(1))
            guard ledgerConnect != nil else { return }
            found = LedgerHID.connected()
        }
        guard let model = found else {
            ledgerConnect?.problem = "No Ledger answered over USB or Bluetooth in a minute. Plug it in or turn its Bluetooth on, unlock it and press Try again."
            ledgerConnect?.busy = false
            return
        }
        ledgerConnect?.model = model + (bluetooth ? " · Bluetooth" : " · USB")
        var account = LedgerAccount.saved ?? LedgerAccount()
        account.model = model
        account.bluetooth = bluetooth
        let device = account.device
        do {
            ledgerConnect?.status = "Open the Ethereum app on your Ledger — or allow it when the Ledger asks."
            try await device.open("Ethereum")
            account.evm = try await device.evmAddress(account.evmPath)
            ledgerConnect?.evm = account.evm
            if let blind = try? await device.evmBlindSigning() {
                // The app doesn't report «Raw messages»; it is what lets the Ledger show a Hyperliquid approval or
                // withdrawal field by field.
                ledgerConnect?.notes.append("Ethereum \(blind.version): turn on «Raw messages» in the app's settings — then the Ledger shows Hyperliquid approvals and withdrawals field by field."
                    + (blind.on ? "" : " «Blind signing» is needed too for swaps, Aave, Morpho and bridges."))
            }
            ledgerConnect?.status = "Now the Solana app: allow it when the Ledger asks."
            do {
                try await device.open("Solana")
                account.solana = try await device.solanaAddress(account.solanaPath)
                ledgerConnect?.solana = account.solana
                if let blind = try? await device.solanaBlindSigning(), !blind.on {
                    ledgerConnect?.notes.append("Solana \(blind.version): turn on «Blind signing» in the app's settings for USDC transfers, Jupiter swaps and lending. SOL sends work without it.")
                }
            } catch let e as LedgerError where e.status == 0x6807 {
                ledgerConnect?.notes.append("No Solana app on this Ledger: add it in Ledger Live › My Ledger, then connect again. Until then it signs EVM moves only.")
            }
            LedgerAccount.saved = account
            await loadSigner()
            ledgerConnect?.status = ledgerActive ? "Connected. It signs every move now." : "Connected. Switch to it to sign with it."
        } catch {
            if account.evm != nil { LedgerAccount.saved = account; await loadSigner() }
            ledgerConnect?.problem = error.localizedDescription
        }
        ledgerConnect?.busy = false
    }

    /// Shows one of the Ledger's addresses on its own screen, for comparing with the one in ihatecrypto: what the
    /// device says is the truth even if this Mac is not.
    func showOnLedger(solana: Bool) async {
        guard let account = LedgerAccount.saved else { return }
        let device = account.device
        ledgerConnect?.problem = nil
        ledgerConnect?.status = "Compare the address on your Ledger with the one here, then approve it there."
        do {
            if solana {
                try await device.open("Solana")
                _ = try await device.solanaAddress(account.solanaPath, show: true)
            } else {
                try await device.open("Ethereum")
                _ = try await device.evmAddress(account.evmPath, show: true)
            }
            ledgerConnect?.status = "The Ledger showed the same address."
        } catch {
            ledgerConnect?.problem = error.localizedDescription
        }
    }

    /// Whether the Ledger answers now: plugged in, or linked over Bluetooth within a few seconds.
    func ledgerReachable() async -> Bool {
        guard let account = LedgerAccount.saved else { return false }
        return account.bluetooth == true ? await LedgerBLE.shared.connect(timeout: 3) != nil : LedgerHID.connected() != nil
    }

    /// Takes the Ledger off the list (its addresses only; nothing is on this Mac to wipe). The signing process's
    /// wallet signs again.
    func forgetLedger() async {
        guard let account = LedgerAccount.saved else { reply("No Ledger is connected."); return }
        // Its trading key stops signing; connecting the same Ledger again brings it back with Touch ID.
        if let evm = account.evm { _ = try? await SignerClient().send(.setLedgerAgentActive(owner: evm, active: false)) }
        LedgerAccount.active = false
        LedgerAccount.saved = nil
        ledgerConnect = nil
        await loadSigner()
        reply("Ledger removed from the wallet list. The money on it stays on it; connect it again any time.")
        await refresh()
    }
}

/// A Ledger drawn plainly: the Nano's body with its screen and swivel ring, in the app's ink.
struct LedgerGlyph: View {
    var size: CGFloat = 28
    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: size * 0.16).fill(Color.primary)
                .frame(width: size, height: size * 0.42)
            RoundedRectangle(cornerRadius: size * 0.05).fill(Brand.accent)
                .frame(width: size * 0.42, height: size * 0.2).offset(x: size * 0.1)
            Circle().strokeBorder(Color.primary.opacity(0.5), lineWidth: size * 0.05)
                .frame(width: size * 0.34).offset(x: size * 0.62)
        }
        .frame(width: size, height: size * 0.5)
        .accessibilityHidden(true)
    }
}

/// «Connect your Ledger» on the canvas: the device, each app's address as it is read, what to do on the device now.
struct LedgerBoard: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        if let c = ws.ledgerConnect {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 10) {
                    LedgerGlyph(size: 30)
                    Text("Ledger" + (c.model.map { " · \($0)" } ?? "")).font(.ui(16, .bold))
                    if c.busy { ProgressView().controlSize(.small) }
                    Spacer()
                    Button { ws.ledgerConnect = nil } label: { Image(systemName: "xmark") }.buttonStyle(.hoverIcon).help("Close")
                }
                row("USB", c.model.map { "\($0) found" }, done: c.model != nil)
                row("ETHEREUM APP · EVM", c.evm.map { "\($0.prefix(6))…\($0.suffix(4))" }, done: c.evm != nil, solana: false)
                row("SOLANA APP", c.solana.map { "\($0.prefix(4))…\($0.suffix(4))" }, done: c.solana != nil, solana: true)
                if ws.ledgerActive, c.evm != nil {
                    // Orders without the buttons once the Ledger approves a key that can't withdraw.
                    HStack(spacing: 10) {
                        Image(systemName: ws.agentActive ? "key.horizontal.fill" : "key.horizontal")
                            .foregroundStyle(ws.agentActive ? Brand.profit : .secondary).frame(width: 20)
                        VStack(alignment: .leading, spacing: 1) {
                            Text("HYPERLIQUID TRADING KEY").font(.ui(9, .heavy)).tracking(0.4).opacity(0.7)
                            Text(ws.agentActive ? "on · orders, stops and trailing without the buttons" : "off · every order asks the Ledger")
                                .font(.ui(12))
                        }
                        Spacer()
                        if !ws.agentActive, !c.busy {
                            Button("Approve on Ledger") { Task { await ws.enableLedgerAgent() } }.controlSize(.small)
                                .help("The Ledger signs one approval of a key that can trade but not withdraw; withdrawals stay on the device")
                        }
                    }
                }
                Label(SignerActivity.shared.ledger ?? c.status, systemImage: c.busy ? "hand.point.up.left.fill" : "checkmark.seal.fill")
                    .font(.ui(12, .semibold)).foregroundStyle(c.busy ? Brand.accentInk : Brand.profit)
                if let problem = c.problem {
                    HStack(alignment: .top) {
                        Label(problem, systemImage: "exclamationmark.triangle.fill").font(.ui(11)).foregroundStyle(.red)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 8)
                        Button("Try again") { Task { await ws.connectLedger() } }.controlSize(.small)
                    }
                }
                ForEach(c.notes, id: \.self) { note in
                    Label(note, systemImage: "gearshape").font(.ui(10.5)).foregroundStyle(Brand.jevInk).fixedSize(horizontal: false, vertical: true)
                }
                if !c.busy, let account = LedgerAccount.saved {
                    HStack(spacing: 8) {
                        if !ws.ledgerActive {
                            Button { Task { await ws.switchWallet(account.wallet) } } label: {
                                Label("Sign with Ledger", systemImage: "signature").font(.ui(13, .semibold)).padding(.horizontal, 6)
                            }
                            .buttonStyle(.borderedProminent).controlSize(.large)
                        }
                        Button("Forget") { Task { await ws.forgetLedger() } }.controlSize(.large)
                    }
                }
                Text("The keys never leave the Ledger. ihatecrypto checks every move first — only your own addresses, no unlimited approvals — then the Ledger shows it, and nothing is signed until you approve it on the device.")
                    .font(.ui(10)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .padding(18)
            .frame(width: 470, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 26).fill(Brand.card).shadow(color: .black.opacity(0.08), radius: 18, y: 10))
            .overlay(RoundedRectangle(cornerRadius: 26).strokeBorder(Brand.accent.opacity(0.5), lineWidth: 1.5))
            .transition(.opacity)
        }
    }

    private func row(_ label: String, _ value: String?, done: Bool, solana: Bool? = nil) -> some View {
        HStack(spacing: 10) {
            Image(systemName: done ? "checkmark.circle.fill" : "circle.dotted").foregroundStyle(done ? Brand.profit : .secondary)
            VStack(alignment: .leading, spacing: 1) {
                Text(label).font(.ui(9, .heavy)).tracking(0.4).opacity(0.7)
                Text(value ?? "—").font(value == nil ? .ui(13) : .system(size: 13, weight: .semibold, design: .monospaced))
            }
            Spacer()
            if done, let solana, ws.ledgerConnect?.busy == false {
                Button("Show on Ledger") { Task { await ws.showOnLedger(solana: solana) } }.controlSize(.small)
                    .help("The Ledger shows this address on its own screen, to compare")
            }
        }
    }
}

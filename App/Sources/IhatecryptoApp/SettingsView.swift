import AppKit
import HyperliquidSigning
import IhatecryptoCore
import LLMCatalog
import LLMCore
import ServiceManagement
import SignerAPI
import SwiftUI
import TradeExecution
import WalletCore

/// Settings occupy the window; the coin and Esc return to the canvas.
struct SettingsView: View {
    @Environment(Workspace.self) private var ws
    @State private var confirmForget = false
    @State private var alwaysRunning = KeepRunning.isOn
    @State private var alwaysRunningError: String?
    @AppStorage("chatLanguage") private var language = ChatLanguage.auto.rawValue
    @AppStorage(AppTheme.key) private var theme = AppTheme.system.rawValue
    @AppStorage(AppLanguage.key) private var appLanguage = AppLanguage.system.rawValue
    static let tabKey = "settingsTab"
    @AppStorage(SettingsView.tabKey) private var tab = Tab.general
    @AppStorage(Executor.batchKey) private var batches = false
    @AppStorage(ContractCallGuard.capKey) private var contractCap = 50.0

    /// Raw values also serve links from the chat and the networks menu.
    enum Tab: String, CaseIterable {
        case general, agent, signIns = "signins", wallet, modules, skills, debugging

        var title: String {
            switch self {
            case .general: "General"
            case .agent: "Trading agent"
            case .signIns: "AI model"
            case .wallet: "Wallet"
            case .modules: "Modules"
            case .skills: "Skills"
            case .debugging: "Debugging"
            }
        }

        var icon: String {
            switch self {
            case .general: "gearshape"
            case .agent: "chart.line.uptrend.xyaxis"
            case .signIns: "sparkles"
            case .wallet: "creditcard"
            case .modules: "square.grid.2x2"
            case .skills: "wand.and.stars"
            case .debugging: "terminal"
            }
        }
    }

    static let lights = CGPoint(x: 20, y: 21)

    var body: some View {
        VStack(spacing: 0) {
            ZStack {
                WindowDragArea()
                Text("Settings").font(.ui(13, .semibold)).allowsHitTesting(false)
                Button(action: close) { Image(systemName: "xmark").font(.system(size: 12, weight: .semibold)) }
                    .buttonStyle(.hoverIcon).help("Close Settings (Esc)")
                    .frame(maxWidth: .infinity, alignment: .trailing).padding(.trailing, 10)
            }
            .frame(height: 2 * Self.lights.y).background(Brand.card)
            Brand.cardBorder.frame(height: 1)
            HStack(spacing: 0) {
                sidebar
                Brand.cardBorder.frame(width: 1)
                page.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading).background(Brand.canvas)
            }
        }
        .ignoresSafeArea(edges: .top)
        .background(TrafficLights(center: Self.lights))
        .background(Button("Back to the canvas", action: close).keyboardShortcut(.cancelAction).opacity(0))
        .onAppear { ws.tuning = "In Settings" }
        .onDisappear { ws.tuning = nil }
    }

    private func close() { ws.settingsOpen = false }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button(action: close) {
                HStack(spacing: 10) {
                    BrandLogo(size: 28, mood: .tuning, animated: true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text("Assistant").font(.ui(13, .semibold))
                        Text(ws.tuning ?? "In Settings").font(.ui(11)).foregroundStyle(.secondary)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain).help("Back to the canvas (Esc)")
            .padding(.horizontal, 6).padding(.bottom, 18)
            ForEach(Tab.allCases.filter { $0 != .debugging }, id: \.self) { tabButton($0) }
            Spacer()
            #if DEBUG_API
            Divider().padding(.vertical, 8)
            Text("DEVELOPER").font(.ui(10, .semibold)).tracking(1).foregroundStyle(.tertiary).padding(.leading, 10)
            tabButton(.debugging)
            #endif
        }
        .padding(.horizontal, 10).padding(.vertical, 20).frame(width: 190).background(Brand.card)
    }

    private func tabButton(_ t: Tab) -> some View {
        Button { tab = t } label: {
            HStack(spacing: 10) {
                SettingsIcon(icon: t.icon, size: 24, active: tab == t)
                Text(t.title).font(.ui(13, tab == t ? .semibold : .regular))
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 8).frame(height: 38)
            .background(RoundedRectangle(cornerRadius: 8).fill(tab == t ? Brand.glassSelected : .clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain).accessibilityAddTraits(tab == t ? [.isSelected] : [])
    }

    @ViewBuilder private var page: some View {
        switch tab {
        case .modules: ModulesSettings()
        case .general: SettingsPage(title: "General", line: "How the app talks to you and keeps running.") { general }
        case .agent: SettingsPage(title: "Trading agent", line: "How the assistant trades for you. Code checks every plan against these rules.") { agent }
        case .signIns: SettingsPage(title: "AI model", line: "What reads your chat: a subscription, an API key, or a free model on this Mac.") { ModelSection() }
        case .wallet: SettingsPage(title: "Wallet", line: "Your wallets, the key that signs and \(Presence.name).") { wallet }
        case .skills: SettingsPage(title: "Skills", line: "Instructions the assistant follows for one kind of work. Switch them on and off, or add your own from a file.") { SkillsSettings() }
        #if DEBUG_API
        case .debugging: SettingsPage(title: "Debugging", line: "Developer tools on this Mac.") { debugging }
        #else
        case .debugging: EmptyView()
        #endif
        }
    }

    @ViewBuilder private var general: some View {
        SettingsSection(icon: "circle.lefthalf.filled", title: "Appearance", line: "Light, dark, or as your Mac is set.") {
            SettingsGroup {
                SettingsRow("Theme", icon: "paintpalette") {
                    // Drawn segments, like Touch ID's: the accent instead of the system blue.
                    HStack(spacing: 2) {
                        ForEach(AppTheme.allCases, id: \.self) { t in
                            let on = t.rawValue == theme
                            Button { theme = t.rawValue; AppTheme.apply() } label: {
                                Label(t.title, systemImage: t.icon).font(.ui(11.5, .semibold))
                                    .foregroundStyle(on ? Brand.onAccent : Color.primary)
                                    .frame(maxWidth: .infinity).padding(.vertical, 6)
                                    .background(RoundedRectangle(cornerRadius: 6).fill(on ? Brand.accent : .clear))
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain).accessibilityAddTraits(on ? [.isSelected] : [])
                        }
                    }
                    .padding(2).frame(width: 290)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.06)))
                }
            }
        }
        SettingsSection(icon: "globe", title: "Language", line: "The app's own texts, and the chat's replies.") {
            SettingsGroup {
                SettingsRow("App language", icon: "character.book.closed", info: "Menus, buttons and the app's texts. It takes effect when ihatecrypto opens again. Many texts are still English only; more are translated with updates.") {
                    Picker("App language", selection: Binding(get: { appLanguage }, set: { appLanguage = $0; AppLanguage.apply() })) {
                        ForEach(AppLanguage.allCases, id: \.self) { Text($0.title).tag($0.rawValue) }
                    }
                    .labelsHidden().fixedSize()
                }
                if appLanguage != AppLanguage.atLaunch {
                    SettingsRow("Restart to switch the language", icon: "arrow.clockwise") { Button("Restart now") { AppLanguage.restart() } }
                }
                SettingsRow("Reply language", icon: "text.bubble", info: "Auto answers in the language you write in. Tickers, amounts and addresses stay as they are.") {
                    Picker("Reply language", selection: $language) {
                        Text("Auto — your language").tag(ChatLanguage.auto.rawValue)
                        Text("English").tag(ChatLanguage.en.rawValue)
                        Text("Русский").tag(ChatLanguage.ru.rawValue)
                    }
                    .labelsHidden().fixedSize()
                }
            }
        }
        NoticeSettings()
        keepRunning
        NodesSection()
        OneClickKeySection()
        about
    }

    private var about: some View {
        let info = Bundle.main.infoDictionary
        return SettingsSection(icon: "info.circle", title: "About", line: "Checks for a new version once a day. It installs only when you press Install.") {
            SettingsGroup {
                SettingsRow("ihatecrypto \(info?["CFBundleShortVersionString"] as? String ?? "")", icon: "app.badge",
                            info: "Build \(info?["CFBundleVersion"] as? String ?? "?"). A new version waits for you to press Install, and What's new shows what changed; the wallet, settings and canvases stay.") {
                    if Updater.shared.available { Button("Check for updates") { Updater.shared.check() } }
                }
            }
        }
    }

    // MARK: Trading agent

    @ViewBuilder private var agent: some View {
        @Bindable var ws = ws
        style
        limits
        byTheBook
        SettingsSection(icon: "checkmark.seal", title: "What it may buy", line: "Tokens on Solana.") {
            SettingsGroup {
                SettingsRow("Safe search", icon: "checkmark.seal", info: ws.safeSearch
                            ? "Only verified coins on Solana: SOL, BTC, ETH, JUP and JitoSOL. Look-alikes of famous tokens cannot slip in."
                            : "Any Solana token can be bought by its mint through Jupiter. Before you confirm, the assistant explains risks such as a creator who can freeze or print tokens, a thin pool, or no verification. Confirm and simulation still apply.") {
                    Toggle("Safe search", isOn: $ws.safeSearch).toggleStyle(AccentSwitch())
                }
            }
        }
    }

    /// A style sets every rule at once; a number changed after it makes the policy «Custom».
    private var style: some View {
        let current = AgentStyle.allCases.first { $0.policy == ws.policy }
        return SettingsSection(icon: "dial.medium", title: "Style",
                               line: current == nil ? "Custom: your own numbers below. A style resets them." : "Sets every rule below at once. Change any number after.") {
            HStack(spacing: 10) {
                ForEach(AgentStyle.allCases) { styleCard($0, selected: $0 == current) }
            }
        }
    }

    private func styleCard(_ s: AgentStyle, selected: Bool) -> some View {
        Button { ws.policy = s.policy } label: {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top) {
                    SettingsIcon(icon: s.icon, size: 34)
                    Spacer(minLength: 4)
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle").font(.system(size: 15))
                        .foregroundStyle(selected ? Brand.accent : Color.secondary.opacity(0.5))
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(s.name).font(.ui(15, .bold))
                    Text(s.line).font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                HStack(spacing: 14) {
                    figure("Risk", Fmt.pct(s.policy.maxRiskPct))
                    figure("Leverage", "\(Fmt.num(s.policy.maxLeverage))×")
                    figure("Take", "\(Fmt.num(s.policy.defaultRewardRisk))R")
                }
            }
            .padding(14).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 12).fill(Brand.card))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(selected ? Brand.accent : Brand.cardBorder, lineWidth: selected ? 1.5 : 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain).accessibilityAddTraits(selected ? [.isSelected] : [])
        .help(selected ? "\(s.name) — in use" : "Use \(s.name): sets all the rules below")
    }

    private func figure(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(label).font(.ui(10)).foregroundStyle(.secondary)
            Text(value).font(.ui(13, .semibold)).monospacedDigit()
        }
    }

    private var limits: some View {
        let p = ws.policy
        return SettingsSection(icon: "shield.lefthalf.filled", title: "Limits", line: "A plan that breaks one is not executed. Crypto only.") {
            SettingsGroup {
                SettingsRow("Risk per trade", icon: "chart.bar", info: "The most one trade may lose at its stop, as a share of your capital.") {
                    GeometryReader { g in
                        Capsule().fill(Color.primary.opacity(0.1))
                            .overlay(alignment: .leading) {
                                Capsule().fill(Brand.accent).frame(width: g.size.width * min(max(CGFloat(NSDecimalNumber(decimal: p.maxRiskPct).doubleValue / 10), 0), 1))
                            }
                    }
                    .frame(width: 70, height: 6).accessibilityHidden(true)
                    stepper(Fmt.pct(p.maxRiskPct), "risk per trade", down: p.maxRiskPct > 0.5, up: p.maxRiskPct < 10) { $0.maxRiskPct += $1 * Self.half }
                }
                SettingsRow("All open positions together", icon: "square.stack.3d.up", info: "Their combined loss at the stops can't go over this share of capital. It's never below the risk per trade.") {
                    stepper(Fmt.pct(p.maxOpenRiskPct), "for all positions", down: p.maxOpenRiskPct > p.maxRiskPct, up: p.maxOpenRiskPct < 30) { $0.maxOpenRiskPct += $1 }
                }
                SettingsRow("Reward / risk at least", icon: "arrow.up.right", info: "The planned profit must be at least this many times the planned loss.") {
                    stepper(Fmt.num(p.minRewardRisk), "reward per risk", down: p.minRewardRisk > 1, up: p.minRewardRisk < 5) { $0.minRewardRisk += $1 * Self.tenth }
                }
                SettingsRow("Leverage up to", icon: "multiply", info: "Borrowed exposure makes gains and losses larger. The assistant never trades above this.") {
                    stepper("\(Fmt.num(p.maxLeverage))×", "leverage", down: p.maxLeverage > 1, up: p.maxLeverage < 20) { $0.maxLeverage += $1 }
                }
            }
        }
    }

    private var byTheBook: some View {
        let p = ws.policy
        return SettingsSection(icon: "book.closed", title: "By the book", line: "What «by the book» uses when you name no numbers.") {
            SettingsGroup {
                SettingsRow("Risk", icon: "chart.bar", info: "The share of capital a trade risks at its stop when you don't say how much. Never above the risk per trade.") {
                    stepper(Fmt.pct(p.defaultRiskPct), "default risk", down: p.defaultRiskPct > Self.quarter, up: p.defaultRiskPct < p.maxRiskPct) { $0.defaultRiskPct += $1 * Self.quarter }
                }
                SettingsRow("Take-profit", icon: "flag.checkered", info: "How far the take is, as a multiple of the stop's distance: 2R takes twice what the stop would lose. Never below the reward / risk limit.") {
                    stepper("\(Fmt.num(p.defaultRewardRisk))R", "take-profit", down: p.defaultRewardRisk > p.minRewardRisk, up: p.defaultRewardRisk < 10) { $0.defaultRewardRisk += $1 * Self.half }
                }
                SettingsRow("Stop", icon: "hand.raised", info: "How far the stop is, in 4-hour ATRs — the coin's usual move over four hours. Wider stops are hit less often but lose more when they are.") {
                    stepper("\(Fmt.num(p.atrStopMultiple)) ATR", "stop distance", down: p.atrStopMultiple > Self.half, up: p.atrStopMultiple < 5) { $0.atrStopMultiple += $1 * Self.half }
                }
            }
        }
    }

    private static let tenth = Decimal(string: "0.1")!, quarter = Decimal(string: "0.25")!, half = Decimal(string: "0.5")!

    /// − value +: `change` gets −1 or +1; the rules that depend on each other are kept in line after it.
    private func stepper(_ value: String, _ what: String, down: Bool, up: Bool, change: @escaping (inout RiskPolicy, Decimal) -> Void) -> some View {
        HStack(spacing: 8) {
            step("minus", "Less \(what)", enabled: down) { update { change(&$0, -1) } }
            Text(value).font(.ui(15, .bold)).monospacedDigit().frame(minWidth: 58)
            step("plus", "More \(what)", enabled: up) { update { change(&$0, 1) } }
        }
    }

    private func update(_ change: (inout RiskPolicy) -> Void) {
        var p = ws.policy
        change(&p)
        p.defaultRiskPct = min(p.defaultRiskPct, p.maxRiskPct)
        p.maxOpenRiskPct = max(p.maxOpenRiskPct, p.maxRiskPct)
        p.defaultRewardRisk = max(p.defaultRewardRisk, p.minRewardRisk)
        ws.policy = p
    }

    private func step(_ symbol: String, _ help: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 10, weight: .bold))
                .frame(width: 26, height: 26).background(RoundedRectangle(cornerRadius: 6).fill(Color.primary.opacity(0.07)))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain).disabled(!enabled).opacity(enabled ? 1 : 0.4).help(help).accessibilityLabel(help)
    }

    #if DEBUG_API
    private var debugging: some View {
        SettingsSection(icon: "terminal", title: "Local debug API", line: "For Claude Code and local developer tools.") {
            SettingsGroup {
                SettingsRow("Allow local debug access", icon: "terminal", info: "Reads state, types into the chat and captures the window. It never signs: money moves still need Confirm and \(Presence.name) in the app. Access requires a token that changes each time the API starts.") {
                    Toggle("Local debug API", isOn: Binding(
                        get: { UserDefaults.standard.bool(forKey: "debugAPI") },
                        set: { on in
                            UserDefaults.standard.set(on, forKey: "debugAPI")
                            if on { DebugAPI.shared.start(ws) } else { DebugAPI.shared.stop() }
                        }))
                    .toggleStyle(AccentSwitch())
                }
                SettingsRow("Local address", icon: "network") { Text("127.0.0.1:\(String(DebugAPI.port))").textSelection(.enabled) }
                SettingsRow("Access token", icon: "key", info: "Stored at Application Support/\(Flavor.folder)/debug-token. Use it as a Bearer token with curl.") {
                    Text("Changes each start").foregroundStyle(.secondary)
                }
            }
        }
    }
    #endif

    @ViewBuilder private var wallet: some View {
        signer
        WalletManagement()
        touchID
        PasscodeSettings()
        tradingKey
        addresses
        if ws.signerWallet != nil { forget }
    }

    private var signer: some View {
        SettingsSection(icon: "creditcard", title: "Wallet that signs", line: ws.signerWallet == nil ? "No key on this Mac yet: add a wallet below." : "The active wallet. Switch in «Your wallets» below.") {
            SettingsGroup {
                if let w = ws.activeWallet {
                    HStack(spacing: 12) {
                        Text(w.label.prefix(1).uppercased()).font(.ui(18, .bold)).foregroundStyle(Brand.accentInk)
                            .frame(width: 44, height: 44).background(RoundedRectangle(cornerRadius: 12).fill(Brand.canvas))
                        VStack(alignment: .leading, spacing: 3) {
                            Text(w.label).font(.ui(15, .semibold))
                            Text(Workspace.kindName(w)).font(.ui(12)).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 8)
                    }
                    .padding(14)
                    Divider().padding(.leading, 54)
                    ForEach([("Solana", "sol", w.solana), ("EVM", "eth", w.evm), ("NEAR", "near", w.near)], id: \.0) { pair in
                        if let address = pair.2 {
                            SettingsRow(pair.0, icon: "link", logo: pair.1) {
                                CopyButton(title: "\(address.prefix(6))…\(address.suffix(6))", text: address, help: "Copy the \(pair.0) address")
                            }
                        }
                    }
                }
                SettingsRow("Signing key", icon: "lock.shield", info: "The key stays in secure storage. A separate process without network access signs approved moves.") {
                    SettingsPill(text: ws.signerWallet == nil ? "No key on this Mac" : "Secure storage", good: ws.signerWallet != nil)
                }
                // The signer keeps only the seed made from the words (one-way), so there is nothing to show again.
                if ws.activeWallet?.kind == "phrase" {
                    SettingsRow("Recovery phrase", icon: "text.word.spacing", info: "ihatecrypto keeps only the key made from your words, sealed with \(Presence.name) in a separate process — never the words themselves, so they can't be shown again. Lost them? Create a new wallet below, write its words down and move the money there.") {
                        Text("Not kept on this Mac").foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    @ViewBuilder private var addresses: some View {
        @Bindable var ws = ws
        SettingsSection(icon: "eye", title: "Addresses for balances", line: "Read-only wallets. Never paste a recovery phrase or key.") {
            SettingsGroup {
                SettingsRow("Solana", icon: "link", logo: "sol") {
                    addressField("Solana address", text: $ws.solanaAddress)
                }
                SettingsRow("Hyperliquid", icon: "link", logo: "hype") {
                    addressField("Hyperliquid address (0x…)", text: $ws.hlAddress)
                }
            }
        }
    }

    private func addressField(_ label: String, text: Binding<String>) -> some View {
        HStack(spacing: 6) {
            TextField(label, text: text).textFieldStyle(.roundedBorder).font(.system(size: 11.5, design: .monospaced))
                .labelsHidden().frame(maxWidth: 340)
            CopyButton(text: text.wrappedValue, help: "Copy the \(label)").disabled(text.wrappedValue.isEmpty)
        }
    }

    private var touchID: some View {
        SettingsSection(icon: Presence.signIcon, title: AppLock.shared.codeSigns ? "Signing" : Presence.short,
                        line: "One \(Presence.touchID ? "touch" : AppLock.shared.codeSigns ? "code" : "password") covers one move, or a few minutes.") {
            SettingsGroup {
                SettingsRow("Stays valid for", icon: "timer", info: "One move covers that move's signatures for at most a minute and ends when it is done. With 5–30 minutes, signatures need no new \(Presence.touchID ? "touch" : "password") during that time. Each still passes the guard and second check. Locking the screen or sleep ends the session. A longer duration asks for \(Presence.name).") {
                    // Drawn segments keep the brand accent instead of macOS's system blue.
                    HStack(spacing: 2) {
                        ForEach(SignerSession.choices, id: \.self) { choice in
                            let on = choice == ws.sessionLength
                            Button {
                                Task {
                                    _ = try? await SignerClient().send(.setSessionLength(seconds: choice))
                                    await ws.loadSigner()
                                }
                            } label: {
                                Text(choice == SignerSession.standard ? "One move" : "\(choice / 60) min").font(.ui(11.5, .semibold))
                                    .foregroundStyle(on ? Brand.onAccent : Color.primary)
                                    .frame(maxWidth: .infinity).padding(.vertical, 6)
                                    .background(RoundedRectangle(cornerRadius: 6).fill(on ? Brand.accent : .clear))
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain).accessibilityAddTraits(on ? [.isSelected] : [])
                        }
                    }
                    .padding(2).frame(width: 290)
                    .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.06)))
                    .disabled(ws.signerWallet == nil)
                }
                SettingsRow("One transaction for approve and swap", icon: "square.stack.3d.up",
                            info: "On Base, Arbitrum, Ethereum, Polygon and Robinhood, an approve and the swap, deposit or bridge that spends it go out as one transaction under one \(Presence.short) (EIP-7702). For that, from the first such move on a network your wallet there runs the code of an audited account contract (eth-infinitism's Simple7702Account): it takes coins and tokens as before, only you can make it act, and every call in it is checked like a single one. Off: two transactions, two \(Presence.touchID ? "Touch IDs" : "passwords"), as before; a network already switched stays so.") {
                    Toggle("One transaction for approve and swap", isOn: $batches).toggleStyle(AccentSwitch()).labelsHidden()
                }
                SettingsRow("Cap for unknown protocols", icon: "shield.lefthalf.filled",
                            info: "A contract the chat calls outside ihatecrypto's own list (Aave, Morpho, Uniswap, Relay…) may put at most this much at risk in one call — what leaves the wallet, its approve and the coins sent with it. It must also have verified code, be a month old, hold $1M or more by DefiLlama and be on no scam list. Every check can be fooled; this caps what that costs.") {
                    Picker("Cap for unknown protocols", selection: $contractCap) {
                        ForEach([10.0, 25, 50, 100, 250, 500, 1000], id: \.self) { Text(Fmt.usd(Decimal($0))).tag($0) }
                    }
                    .labelsHidden().pickerStyle(.menu).fixedSize()
                }
            }
        }
    }

    private var tradingKey: some View {
        SettingsSection(icon: "bolt", title: "Hyperliquid", line: "A trading key places orders. It cannot withdraw money.") {
            SettingsGroup {
                SettingsRow("Trading key", icon: "key", logo: "hype", info: ws.agentActive
                            ? "Approved once for Hyperliquid orders. The key cannot withdraw money."
                            : ws.ledgerActive ? "Without a trading key, every order asks for the Ledger's buttons. Hyperliquid accepts the key after the account has had a deposit."
                            : "Without a trading key, every order asks for \(Presence.name). Hyperliquid accepts the key after the account has had a deposit.") {
                    SettingsPill(text: ws.agentActive ? "On" : "Off", good: ws.agentActive)
                    if !ws.agentActive, ws.activeWallet?.evm != nil {
                        Button(ws.ledgerActive ? "Approve on Ledger…" : "Approve…") { Task { await ws.enableAgent() } }.disabled(ws.executing)
                    }
                }
                // Paused by the chat's «lock»: the key is still approved, it only waits for a Touch ID to go on.
                if ws.agentAddress != nil, !ws.agentActive {
                    SettingsRow("Paused by «lock»", icon: "pause.circle", info: "Orders ask for \(ws.ledgerActive ? "the Ledger's buttons" : Presence.name) until it resumes. Resuming asks for \(Presence.name) once.") {
                        Button("Resume…") { Task { await ws.resumeTradingKey() } }.disabled(ws.executing)
                    }
                }
                SettingsRow("ihatecrypto's fee", icon: "percent", info: "\(HLBuilder.rate) of each Hyperliquid trade placed in ihatecrypto, on top of the exchange's own fee: perps both ways, spot only when selling. Hyperliquid takes it, and only once you approve it: the first trade you confirm asks. «Revoke» takes the approval back, and the next trade asks again. The referral code \(HLBuilder.referralCode) gives the account 4% off Hyperliquid's fees.") {
                    SettingsPill(text: ws.feeApproved == true ? "\(HLBuilder.rate) · Approved" : ws.feeApproved == false ? "\(HLBuilder.rate) · Not yet" : HLBuilder.rate,
                                 good: ws.feeApproved == true)
                    if ws.feeApproved == true {
                        Button("Revoke…") { Task { await ws.revokeFee() } }.disabled(ws.executing)
                    }
                }
                .task(id: ws.hlAddress) { await ws.loadFeeApproval() }
            }
        }
    }

    private var forget: some View {
        VStack(alignment: .center, spacing: 6) {
            Button("Forget the key on this Mac…", systemImage: "trash", role: .destructive) { confirmForget = true }
                .buttonStyle(.plain).foregroundStyle(Brand.loss)
                .confirmationDialog("Forget the wallet key on this Mac?", isPresented: $confirmForget) {
                    Button("Forget the key", role: .destructive) {
                        Task {
                            let id = (try? await SignerClient().send(.status))?.activeWallet
                            if (try? await SignerClient().send(.forget))?.ok == true, let id { ws.walletRemoved(id) }
                            await ws.loadSigner()
                        }
                    }
                } message: {
                    Text("Only if the recovery phrase is written down: without it the money on this wallet can't be reached again. Positions, stops and balances stay where they are.")
                }
            Text("Only if the recovery phrase is written down.").font(.ui(12)).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity).padding(.top, 12).padding(.bottom, 8)
    }

    private var keepRunning: some View {
        SettingsSection(icon: "moon.stars", title: "Background", line: "Stops, alerts and scheduled buys with the window closed.") {
            SettingsGroup {
                SettingsRow("Keep always running", icon: "moon.stars", info: "Starts at login and returns within a minute after Quit or a crash, in the menu bar with its window closed. Turn this off to quit for good. Nothing runs while the Mac sleeps; due tasks run when it wakes.") {
                    Toggle("Keep ihatecrypto always running", isOn: Binding(get: { alwaysRunning }, set: { on in
                        do { try KeepRunning.set(on); alwaysRunningError = nil } catch { alwaysRunningError = error.localizedDescription }
                        alwaysRunning = KeepRunning.isOn
                    })).toggleStyle(AccentSwitch())
                }
                if let alwaysRunningError {
                    SettingsRow(alwaysRunningError, icon: "exclamationmark.triangle") { EmptyView() }.foregroundStyle(Brand.loss)
                }
                SettingsRow("Open at login", icon: "power", info: "Opens the app when you log in. If you quit with Keep always running off, app-managed stops and scheduled buys stop. Stop and take-profit orders already on the exchange keep working.") {
                    Toggle("Open ihatecrypto at login", isOn: Binding(
                        get: { SMAppService.mainApp.status == .enabled },
                        set: { on in try? on ? SMAppService.mainApp.register() : SMAppService.mainApp.unregister() }))
                        .toggleStyle(AccentSwitch())
                }
            }
        }
    }
}

/// The column stays centred and readable on a wide window.
struct SettingsPage<Content: View>: View {
    let title: String
    let line: String
    @ViewBuilder let content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(.ui(24, .bold))
                    Text(line).font(.ui(13)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                content
            }
            .frame(maxWidth: 660, alignment: .leading).padding(28).frame(maxWidth: .infinity)
        }
    }
}

/// Neutral tiles keep coloured glyphs legible in both themes, including disabled Modules.
struct SettingsIcon: View {
    let icon: String
    var size: CGFloat = 26
    var active = true

    var body: some View {
        Image(systemName: icon).font(.system(size: size * 0.43, weight: .semibold))
            .foregroundStyle(active ? Brand.accentInk : Color.secondary)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: size * 0.28).fill(Color.primary.opacity(0.06)))
            .accessibilityHidden(true)
    }
}

/// Provider marks (Claude, ChatGPT, OpenRouter) are drawn for light backgrounds; the black ones vanished on dark.
/// A white tile in both themes keeps them all legible; `symbol` draws a matching tile for a choice without a logo.
struct LogoTile: View {
    var logo: String?
    var symbol: String?
    var size: CGFloat = 28

    var body: some View {
        Group {
            if let logo { TokenLogo(name: logo, size: size * 0.68) }
            else if let symbol { Image(systemName: symbol).font(.system(size: size * 0.42, weight: .semibold)).foregroundStyle(Brand.onAccent) }
        }
        .frame(width: size, height: size)
        .background(RoundedRectangle(cornerRadius: size * 0.28).fill(.white))
        .overlay(RoundedRectangle(cornerRadius: size * 0.28).strokeBorder(Color.black.opacity(0.08)))
        .accessibilityHidden(true)
    }
}

/// The app's look for every window: System follows the Mac (the default), Light and Dark fix it. NSApp.appearance
/// carries it to Brand's dynamic colors and SwiftUI's colorScheme alike.
enum AppTheme: String, CaseIterable {
    case system, light, dark
    static let key = "appTheme"

    var title: String {
        switch self {
        case .system: "System"
        case .light: "Light"
        case .dark: "Dark"
        }
    }

    var icon: String {
        switch self {
        case .system: "circle.lefthalf.filled"
        case .light: "sun.max"
        case .dark: "moon"
        }
    }

    @MainActor static func apply() {
        let theme = AppTheme(rawValue: UserDefaults.standard.string(forKey: key) ?? "") ?? .system
        NSApp.appearance = switch theme {
        case .system: nil
        case .light: NSAppearance(named: .aqua)
        case .dark: NSAppearance(named: .darkAqua)
        }
    }
}

/// The language of the app's own texts: System follows the Mac. macOS reads it from `AppleLanguages` at launch, so
/// a change takes effect after a restart; Info.plist lists en and ru, so the system's menus and buttons follow too.
enum AppLanguage: String, CaseIterable {
    case system, en, ru
    static let key = "appLanguage"
    /// What this launch runs in, to offer a restart once the choice differs.
    static let atLaunch = UserDefaults.standard.string(forKey: key) ?? system.rawValue

    var title: String {
        switch self {
        case .system: "System"
        case .en: "English"
        case .ru: "Русский"
        }
    }

    static func apply() {
        let chosen = AppLanguage(rawValue: UserDefaults.standard.string(forKey: key) ?? "") ?? .system
        if chosen == .system { UserDefaults.standard.removeObject(forKey: "AppleLanguages") }
        else { UserDefaults.standard.set([chosen.rawValue], forKey: "AppleLanguages") }
    }

    /// Quits, and opens again once this copy is gone: a second copy started while one runs exits at launch.
    @MainActor static func restart() {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/sh")
        p.arguments = ["-c", "while /bin/kill -0 \(getpid()) 2>/dev/null; do sleep 0.2; done; /usr/bin/open \"$0\"", Bundle.main.bundleURL.path]
        try? p.run()
        NSApp.terminate(nil)
    }
}

/// Three ready sets of the trading rules (RiskPolicy). Balanced is the app's default.
enum AgentStyle: CaseIterable, Identifiable {
    case careful, balanced, bold
    var id: Self { self }

    var name: String {
        switch self {
        case .careful: "Careful"
        case .balanced: "Balanced"
        case .bold: "Bold"
        }
    }

    var line: String {
        switch self {
        case .careful: "Small bets, little leverage, wide stops."
        case .balanced: "The book: 1% risk, a 2R take."
        case .bold: "Bigger bets and more leverage."
        }
    }

    var icon: String {
        switch self {
        case .careful: "gauge.with.dots.needle.0percent"
        case .balanced: "gauge.with.dots.needle.50percent"
        case .bold: "gauge.with.dots.needle.100percent"
        }
    }

    var policy: RiskPolicy {
        var p = RiskPolicy()
        func d(_ s: String) -> Decimal { Decimal(string: s)! }
        switch self {
        case .careful:
            (p.maxRiskPct, p.defaultRiskPct, p.maxOpenRiskPct) = (1, d("0.5"), 3)
            (p.minRewardRisk, p.defaultRewardRisk, p.maxLeverage, p.atrStopMultiple) = (2, d("2.5"), 2, 2)
        case .balanced: break
        case .bold:
            (p.maxRiskPct, p.defaultRiskPct, p.maxOpenRiskPct) = (3, 2, 10)
            (p.minRewardRisk, p.defaultRewardRisk, p.maxLeverage, p.atrStopMultiple) = (d("1.2"), d("1.5"), 10, 1)
        }
        return p
    }
}

struct SettingsSection<Content: View>: View {
    let icon: String
    let title: String
    let line: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                SettingsIcon(icon: icon, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.ui(17, .bold)).accessibilityAddTraits(.isHeader)
                    Text(line).font(.ui(12)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            content
        }
    }
}

struct SettingsGroup<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(spacing: 0) { content }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Brand.card)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Brand.cardBorder))
    }
}

/// Each row has one control and optional detail behind an information button.
struct SettingsRow<Control: View>: View {
    let label: String
    let icon: String
    let logo: String?
    let info: String?
    let control: Control

    init(_ label: String, icon: String, logo: String? = nil, info: String? = nil, @ViewBuilder control: () -> Control) {
        (self.label, self.icon, self.logo, self.info) = (label, icon, logo, info)
        self.control = control()
    }

    var body: some View {
        HStack(spacing: 10) {
            if let logo, logo.hasPrefix("llm-") { LogoTile(logo: logo, size: 26) }
            else if let logo { TokenLogo(name: logo, size: 26).clipShape(RoundedRectangle(cornerRadius: 7)).accessibilityHidden(true) }
            else { SettingsIcon(icon: icon) }
            Text(label).font(.ui(13)).fixedSize(horizontal: false, vertical: true)
            if let info { SettingsInfo(text: info, label: label) }
            Spacer(minLength: 8)
            control.font(.ui(12.5))
        }
        .padding(.horizontal, 14).padding(.vertical, 10).frame(minHeight: 48)
        .overlay(alignment: .bottom) { Brand.cardBorder.opacity(0.65).frame(height: 0.5).padding(.leading, 50) }
    }
}

struct SettingsInfo: View {
    let text: String
    let label: String
    @State private var showing = false

    var body: some View {
        Button { showing.toggle() } label: {
            Image(systemName: "info.circle").font(.system(size: 12)).foregroundStyle(.secondary).frame(width: 22, height: 22)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain).help("About \(label)").accessibilityLabel("About \(label)")
        .popover(isPresented: $showing) {
            Text(text).font(.ui(13)).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                .padding(16).frame(width: 330, alignment: .leading)
        }
    }
}

/// Status uses the same neutral surface; the dot carries the accent.
struct SettingsPill: View {
    let text: String
    let good: Bool

    var body: some View {
        HStack(spacing: 5) {
            Circle().fill(good ? Brand.accent : Color.secondary).frame(width: 6, height: 6)
            Text(text).font(.ui(11, .semibold))
        }
        .foregroundStyle(good ? Color.primary : Color.secondary)
        .padding(.horizontal, 9).padding(.vertical, 4).background(Capsule().fill(Color.primary.opacity(0.05)))
    }
}

struct ModelSection: View {
    @Environment(Workspace.self) private var ws
    @AppStorage("modelThinking") private var thinking = false
    @State private var key = ""
    /// The account whose sign-in shows below the list; nil = the one the chat uses. Picking a row here never switches
    /// the chat: «Switch» does (the owner, 2026-10-20: «я просто хотел подключить другой аккаунт»).
    @State private var shownID: String?
    private var shown: String { shownID ?? ws.providerID }
    private static let ways = ["claude-code", "chatgpt", "openrouter"]

    var body: some View {
        VStack(alignment: .leading, spacing: 26) {
            model
            secondCheck
            SettingsSection(icon: "desktopcomputer", title: "Free, on this Mac", line: "No subscription needed: Ollama runs a model here, with no internet.") {
                SettingsGroup { LocalModelsPanel().padding(14) }
            }
            SettingsSection(icon: "lock.shield", title: "Safety", line: "A model reads your words. It cannot approve a move.") {
                SettingsGroup {
                    SettingsRow("Money moves only after Confirm + \(Presence.short)", icon: Presence.icon, info: "You approve money moves in the app. One \(Presence.short) can cover one move or a session, as set in Wallet. A separately approved Hyperliquid trading key can place orders without a new \(Presence.touchID ? "touch" : "password"); it cannot withdraw money.") { EmptyView() }
                    SettingsRow("Keys stay on this Mac", icon: "eye.slash", info: "The chat model receives your messages, your balances and open positions, the plan and what it looks up. It never receives keys or the recovery phrase, and your own addresses reach it as «me».") { EmptyView() }
                }
            }
        }
        .onChange(of: shown) { key = ""; ws.llmStatus = nil; ws.availableModels = [] }
    }

    /// A second model (Jev on OpenRouter) reads each command and plan again; it can stop a move, never approve one.
    private var secondCheck: some View {
        SettingsSection(icon: "checkmark.shield", title: "Second check", line: "A second reader. It can only stop, never approve.") {
            SettingsGroup {
                SettingsRow("Check commands and plans twice", icon: "checkmark.shield", info: ws.jevKey == nil
                            ? "The chat's model reads each command and plan a second time. Local rules also check incoming text. OpenRouter can add a separate model; the chat keeps its own. This check can stop a move, never approve a signature."
                            : "The chat's model and a separate model on OpenRouter check your commands and plans against your words. OpenRouter receives your messages, pasted text, lookup results and each plan with its balance changes before you sign. This check can stop a move, never approve a signature.") {
                    Toggle("Second check", isOn: Binding(get: { ws.jevScreening }, set: { ws.jevScreening = $0 })).toggleStyle(AccentSwitch())
                }
                SettingsRow("OpenRouter", icon: "network", logo: "llm-openrouter") {
                    if ws.openRouterKey == nil {
                        SettingsPill(text: "Not connected", good: false)
                        Button("Connect") { Task { await ws.signInJev() } }
                    } else if ws.providerID == "openrouter" {
                        SettingsPill(text: "Your chat runs here", good: true)
                    } else {
                        // Connected for an earlier chat: texts go there only when the user turns this on.
                        Toggle("Use OpenRouter", isOn: Binding(get: { ws.jevOpenRouter }, set: { ws.jevOpenRouter = $0 })).toggleStyle(AccentSwitch())
                    }
                }
                if let status = ws.jevStatus { SettingsRow(status, icon: "info.circle") { EmptyView() } }
            }
        }
    }

    private var model: some View {
        let id = shown, info = ws.provider(id).info, connected = ws.hasModel(id)
        return SettingsSection(icon: "sparkles", title: "Chat model", line: "Lets you write freely, in any language. Connect one or more; the chat uses one at a time.") {
            SettingsGroup {
                way("claude-code", "Claude subscription", "Uses Claude Code on this Mac", logo: "llm-claude")
                way("chatgpt", "ChatGPT subscription", "Plus or Pro · sign in in the browser", logo: "llm-openai")
                way("openrouter", "OpenRouter", "One key for many models", logo: "llm-openrouter")
                other
            }
            SettingsGroup {
                SettingsRow("\(info.name)", icon: "info.circle", info: info.note.isEmpty ? nil : info.note) {
                    if let unavailable = info.unavailable { Text(unavailable).foregroundStyle(Brand.accentInk).fixedSize(horizontal: false, vertical: true) }
                    else if connected, id == ws.providerID { chatsModel }
                    else {
                        SettingsPill(text: connected ? "Connected" : "Not connected", good: connected)
                        if connected {
                            Button("Switch", systemImage: "arrow.left.arrow.right") { ws.useAccount(id) }
                                .help("The chat goes on with \(info.name)")
                        }
                    }
                }
                if info.unavailable == nil { connection(id, info) }
            }
            SettingsGroup {
                SettingsRow("Think before answering", icon: "brain",
                            info: "The model reasons before it answers: steadier on a muddled or tricky request, but each reply takes longer, often several times as long (a message can take a few model calls). Off by default. A model that can't reason answers as usual.") {
                    Toggle("Think before answering", isOn: $thinking).toggleStyle(AccentSwitch()).labelsHidden()
                }
            }
        }
    }

    /// The chat's account: a check mark and what it means, never «In use» (the owner didn't get it, 2026-10-20).
    private var chatsModel: some View {
        Label("Chat model", systemImage: "checkmark.circle.fill").font(.ui(12, .semibold)).foregroundStyle(Brand.profit)
            .help("The chat answers with this account")
    }

    /// Where an account stands, at a row's end: the chat's, connected, or nothing yet.
    @ViewBuilder private func state(_ id: String) -> some View {
        if ws.hasModel(id) {
            if id == ws.providerID { chatsModel } else { SettingsPill(text: "Connected", good: true) }
        }
    }

    /// The row picked shows its sign-in below, with a faint fill; the chat stays on its account.
    private func pickFill(_ selected: Bool) -> some View {
        Rectangle().fill(selected ? Color.primary.opacity(0.06) : .clear)
    }

    private func way(_ id: String, _ title: String, _ line: String, logo: String) -> some View {
        let selected = shown == id
        return Button { shownID = id } label: {
            HStack(spacing: 10) {
                LogoTile(logo: logo)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.ui(13, .semibold))
                    Text(line).font(.ui(11.5)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 8)
                state(id)
            }
            .padding(.horizontal, 14).padding(.vertical, 12).frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .background(pickFill(selected))
            .overlay(alignment: .bottom) { Brand.cardBorder.opacity(0.65).frame(height: 0.5).padding(.leading, 52) }
        }
        .buttonStyle(.plain).pointerStyle(selected ? nil : .link)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
        .help(selected ? "Its sign-in is below" : "Show how to connect \(title)")
    }

    /// Less common providers stay reachable from a single row.
    private var other: some View {
        let selected = !Self.ways.contains(shown)
        // The row speaks of the one picked here, or else of the chat's when it is one of these.
        let id: String? = selected ? shown : Self.ways.contains(ws.providerID) ? nil : ws.providerID
        return HStack(spacing: 10) {
            LogoTile(symbol: "key.fill")
            VStack(alignment: .leading, spacing: 2) {
                Text("API key or a model on this Mac").font(.ui(13, .semibold))
                Text(id.map { ws.provider($0).info.name } ?? "Choose a provider or local model").font(.ui(11.5)).foregroundStyle(.secondary)
            }
            Spacer(minLength: 8)
            if let id { state(id) }
            Menu("Choose…") {
                ForEach(ProviderKind.allCases, id: \.self) { kind in
                    Section(kind.title) {
                        ForEach(LLMCatalog.all.filter { $0.info.kind == kind && !Self.ways.contains($0.info.id) }, id: \.info.id) { p in
                            Button(p.info.unavailable == nil ? p.info.name : p.info.name + " — unavailable") { shownID = p.info.id }
                        }
                    }
                }
            }
            .fixedSize()
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .background(pickFill(selected))
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    @ViewBuilder private func connection(_ id: String, _ info: ProviderInfo) -> some View {
        let provider = ws.provider(id), credential = ws.credential(id)
        SettingsRow("Sign-in", icon: "person.crop.circle", info: connectionText(credential, info)) {
            if let label = info.existingLoginLabel, provider.existingLogin() != nil {
                Button(label) { ws.useExistingLogin(id) }
            }
            // Claude Code: installed from here when it's missing, signed in from a terminal once it's there.
            if info.id == "claude-code" {
                if case .external(let path)? = provider.existingLogin() {
                    Button("Sign in in Terminal") { try? ClaudeCodeInstaller.signIn(path) }
                } else {
                    Button(ws.claudeCodeInstall ?? "Install Claude Code") { Task { await ws.installClaudeCode() } }.disabled(ws.claudeCodeInstall != nil)
                }
            }
            if let label = info.signInLabel {
                Button(label) { Task { await ws.signIn(id) } }.disabled(ws.llmBusy)
            }
            if info.signInLabel == nil, info.existingLoginLabel == nil {
                Text(info.kind == .local ? "No key needed" : "Use an API key below").foregroundStyle(.secondary)
            }
        }
        if info.takesKey {
            SettingsRow("API key", icon: "key") {
                SecureField("API key", text: $key).textFieldStyle(.roundedBorder).frame(maxWidth: 320)
                Button("Save") { ws.setCredential(.apiKey(key), for: id); key = "" }.disabled(key.isEmpty)
            }
        }
        SettingsRow("Model", icon: "sparkles") {
            TextField("Model", text: Binding(get: { ws.model(id) }, set: { ws.modelChoices[id] = $0 }))
                .textFieldStyle(.roundedBorder).labelsHidden().frame(maxWidth: 300)
            Menu("List") {
                if ws.availableModels.isEmpty { Button("Load models") { Task { await ws.loadModels(id) } } }
                ForEach(ws.availableModels, id: \.self) { m in Button(m) { ws.modelChoices[id] = m } }
            }
            .fixedSize()
        }
        SettingsRow("Connection", icon: "antenna.radiowaves.left.and.right",
                    info: "Check asks the model \(ModelExam.questions.count) requests people send — buy, send, lend, a chart, a long, a question, DCA — and counts how many it reads right. 8 or more is good for the chat; fewer, and it will misread.") {
            if ws.llmBusy { ProgressView().controlSize(.small) }
            if let n = ws.examProgress {
                ProgressView().controlSize(.small)
                Text("\(n)/\(ModelExam.questions.count)").monospacedDigit().foregroundStyle(.secondary)
            } else if let s = ws.score(id, ws.model(id)) {
                SettingsPill(text: s.short, good: s.good)
            }
            Button("Check") { Task { await ws.checkModel(id) } }.disabled(ws.llmBusy || ws.examProgress != nil || !ws.hasModel(id))
            if credential != nil { Button("Disconnect") { ws.setCredential(nil, for: id) } }
        }
        if let status = ws.llmStatus {
            SettingsRow(status, icon: "info.circle") { EmptyView() }.textSelection(.enabled)
        }
    }

    private func connectionText(_ credential: Credential?, _ info: ProviderInfo) -> String {
        switch credential {
        case .apiKey: "Connected: key in Keychain."
        case .oauth: "Connected: signed in via browser."
        case .external(let path): "Connected: \(path)."
        case nil: info.kind == .local ? "No key needed; the local server must be running." : "Not connected."
        }
    }
}

extension ProviderKind {
    var title: String {
        switch self {
        case .subscription: "Subscription"
        case .apiKey: "API key or credits"
        case .local: "On this Mac"
        }
    }
}

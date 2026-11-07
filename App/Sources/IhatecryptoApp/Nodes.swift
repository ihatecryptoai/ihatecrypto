import AgentAPIKit
import TradeExecution
import EVMChain
import OneClickKit
import SolanaChain
import SwiftUI
import WalletCore

/// A network the user can give a node of their own, and how to get a free one there.
nonisolated enum NodeNetwork: String, CaseIterable, Identifiable {
    case solana, base, arbitrum, ethereum, polygon, robinhood

    var id: String { rawValue }
    var title: String { self == .robinhood ? "Robinhood" : rawValue.capitalized }
    /// The bundle's image names: sol.png, arb.png, eth.svg, pol.png.
    var logo: String { [.solana: "sol", .arbitrum: "arb", .ethereum: "eth", .polygon: "pol"][self] ?? rawValue }
    var chainId: UInt64? { [.base: 8453, .arbitrum: 42161, .ethereum: 1, .polygon: 137, .robinhood: 4663][self] }

    /// The node in use now, when it's the user's own.
    var custom: URL? { chainId.map(EVMNodes.custom) ?? SolanaNodes.custom }

    func use(_ url: URL?) {
        if let chainId { EVMNodes.set(url, for: chainId) } else { SolanaNodes.custom = url }
    }

    func problem(with url: URL) async -> String? {
        if let chainId { return await EVMNodes.problem(with: url, chainId: chainId) }
        return await SolanaNodes.problem(with: url)
    }

    /// Who gives one for free, where, and what its address looks like.
    var provider: (name: String, site: URL, example: String) {
        switch self {
        case .solana: ("Helius", URL(string: "https://www.helius.dev")!, "https://mainnet.helius-rpc.com/?api-key=…")
        case .base: ("Alchemy", URL(string: "https://www.alchemy.com")!, "https://base-mainnet.g.alchemy.com/v2/…")
        case .arbitrum: ("Alchemy", URL(string: "https://www.alchemy.com")!, "https://arb-mainnet.g.alchemy.com/v2/…")
        case .ethereum: ("Alchemy", URL(string: "https://www.alchemy.com")!, "https://eth-mainnet.g.alchemy.com/v2/…")
        case .polygon: ("Alchemy", URL(string: "https://www.alchemy.com")!, "https://polygon-mainnet.g.alchemy.com/v2/…")
        case .robinhood: ("Alchemy", URL(string: "https://www.alchemy.com")!, "https://…")
        }
    }

    var steps: [String] {
        switch self {
        case .solana:
            ["Open helius.dev and sign up — the free plan, no card.",
             "In the dashboard, open RPCs (Endpoints) and copy the Mainnet URL: \(provider.example)",
             "Paste it below and press Check and save."]
        default:
            ["Open alchemy.com and sign up — the free plan, no card.",
             "Create an app, pick \(title) Mainnet, and copy its HTTPS URL: \(provider.example)",
             "Paste it below and press Check and save."]
        }
    }

    var others: String {
        self == .solana ? "Any Solana mainnet node works the same way: QuickNode, Alchemy, Triton — paste its https address."
            : "Any \(title) node works the same way: QuickNode, Infura, drpc with a key — paste its https address. If the check says a node lacks something, try another provider."
    }
}

/// The user's own nodes: each URL carries the provider's key, so it lives in the login Keychain of this Mac only (never
/// in settings files), is put to use at launch, and only its host is ever shown.
nonisolated enum NodeStore {
    static let service = Bundle.main.bundleIdentifier.map { $0 + ".rpc" } ?? "app.ihatecrypto.rpc"

    static func query(_ net: NodeNetwork) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: net.rawValue]
    }

    static func load(_ net: NodeNetwork) -> URL? {
        var q = query(net)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return URL(string: String(decoding: data, as: UTF8.self))
    }

    /// At launch: each saved node is the one its network asks first, and 1Click's partner key goes on its requests.
    static func activate() {
        for net in NodeNetwork.allCases { net.use(load(net)) }
        OneClick.key = oneClickKey
    }

    /// The Agent API key, read from the Keychain the first time an agent wallet is used, not at launch: a key saved by
    /// another tool would make macOS ask, and the app must open without waiting on that.
    static func agentAPIReady() throws {
        if AgentAPI.key == nil { AgentAPI.key = agentKey }
        guard AgentAPI.key != nil else { throw ExecutionError("Add the Agent API key first: Settings › General › Agent wallet key.") }
    }

    /// The agent wallet's grant token (ngt_…): a bearer secret that lets this Mac act within the owner's policy.
    static var agentGrant: String? {
        get {
            let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "agentgrant",
                                    kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
            var out: CFTypeRef?
            guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
            return String(decoding: data, as: UTF8.self)
        }
        set {
            let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "agentgrant"]
            SecItemDelete(q as CFDictionary)
            if let newValue {
                var add = q
                add[kSecValueData as String] = Data(newValue.utf8)
                add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
                SecItemAdd(add as CFDictionary, nil)
            }
        }
    }

    /// NEAR Intents' Agent API partner key (naa_…), in the Keychain beside 1Click's; read at launch. Set, it goes on the
    /// Agent API's requests at once; nil removes it.
    static var agentKey: String? {
        get {
            let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "agentapi",
                                    kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
            var out: CFTypeRef?
            guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
            return String(decoding: data, as: UTF8.self)
        }
        set {
            let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "agentapi"]
            SecItemDelete(q as CFDictionary)
            if let newValue {
                var add = q
                add[kSecValueData as String] = Data(newValue.utf8)
                add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
                SecItemAdd(add as CFDictionary, nil)
            }
            AgentAPI.key = newValue
        }
    }

    /// The NEAR Intents partner key (a JWT), in the Keychain beside the nodes; nil removes it. Set, it goes on 1Click's
    /// requests at once.
    static var oneClickKey: String? {
        get {
            let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "1click",
                                    kSecReturnData as String: true, kSecMatchLimit as String: kSecMatchLimitOne]
            var out: CFTypeRef?
            guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
            return String(decoding: data, as: UTF8.self)
        }
        set {
            let q: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: "1click"]
            SecItemDelete(q as CFDictionary)
            if let newValue {
                var add = q
                add[kSecValueData as String] = Data(newValue.utf8)
                add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
                SecItemAdd(add as CFDictionary, nil)
            }
            OneClick.key = newValue
        }
    }

    /// Saves `url` for `net` (nil removes it) and puts it to use at once.
    static func save(_ url: URL?, for net: NodeNetwork) {
        SecItemDelete(query(net) as CFDictionary)
        if let url {
            var q = query(net)
            q[kSecValueData as String] = Data(url.absoluteString.utf8)
            q[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            SecItemAdd(q as CFDictionary, nil)
        }
        net.use(url)
    }
}

/// Settings › General: the node each network is read and sent through, and the way to add one's own.
struct NodesSection: View {
    /// The host of each network's own node; a network without one isn't here.
    @State private var hosts: [NodeNetwork: String] = Dictionary(uniqueKeysWithValues: NodeNetwork.allCases.compactMap { n in n.custom?.host.map { (n, $0) } })
    @State private var adding: NodeNetwork?

    var body: some View {
        SettingsSection(icon: "server.rack", title: "Your own nodes", line: "Where the app reads each network and sends its transactions.") {
            SettingsGroup {
                ForEach(NodeNetwork.allCases) { net in
                    SettingsRow(hosts[net].map { "\(net.title): \($0)" } ?? "\(net.title): \(net == .solana ? "Solana's public node" : "built-in free nodes")",
                                icon: "server.rack", logo: net.logo,
                                info: hosts[net] == nil
                                    ? "Free public nodes are shared by everyone: they limit how often anyone may ask, so at a busy moment a balance or a move can fail\(net == .solana ? ", and Solana's has no free backup" : ""). A node of your own (free at \(net.provider.name)) takes that away."
                                    : "Your node is asked first; the \(net == .solana ? "public node takes" : "built-in nodes take") over when yours doesn't answer, refuses its key or lacks a call.") {
                        HStack(spacing: 8) {
                            if hosts[net] != nil {
                                Button("Remove") { NodeStore.save(nil, for: net); hosts[net] = nil }
                            }
                            Button(hosts[net] == nil ? "Add your own…" : "Change…") { adding = net }
                        }
                    }
                }
            }
        }
        .sheet(item: $adding) { net in NodeSheet(net: net) { hosts[net] = net.custom?.host } }
    }
}

/// Why a node of one's own, how to get a free one, and the field: the address is checked against the network before
/// it's kept.
struct NodeSheet: View {
    let net: NodeNetwork
    var saved: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var address = ""
    @State private var checking = false
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                TokenLogo(name: net.logo, size: 28).clipShape(RoundedRectangle(cornerRadius: 7))
                Text("Your own \(net.title) node").font(.ui(20, .bold))
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Why").font(.ui(13, .semibold))
                Text(net == .solana
                     ? "Every balance and every move on Solana goes through a node. Solana's public node is free but shared by everyone: it limits how often you may ask and has no backup, so at a busy moment a balance doesn't load or a move fails half-way. A node of your own gives you your own limit; the public one stays behind it as a backup."
                     : "Every balance and every move on \(net.title) goes through a node. The app's built-in nodes are free and shared by everyone: each limits how often you may ask, so at a busy moment a quote, a balance or a move can stall. A node of your own gives you your own limit; the built-in ones stay behind it as backups.")
                Text("A node only reads the chain and passes on transactions already signed on this Mac: it never sees your keys and can't move money. It does see your addresses. Its address carries your key, so it's kept in this Mac's Keychain and shown only by its host.\(net == .ethereum ? " Ethereum's transactions still go out through MEV Blocker, out of the public mempool." : "")")
                    .foregroundStyle(.secondary)
            }
            .font(.ui(12.5)).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 6) {
                Text("A free node at \(net.provider.name), in two minutes").font(.ui(13, .semibold))
                ForEach(Array(net.steps.enumerated()), id: \.offset) { i, text in step(i + 1, text) }
                Link("Open \(net.provider.site.host ?? net.provider.name)", destination: net.provider.site).font(.ui(12.5))
                Text(net.others).font(.ui(11.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            SecureField(net.provider.example, text: $address)
                .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                .onSubmit { Task { await check() } }
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle").font(.ui(12)).foregroundStyle(Brand.loss)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                if checking { ProgressView().controlSize(.small) }
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Check and save") { Task { await check() } }
                    .keyboardShortcut(.defaultAction).disabled(checking || address.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(24).frame(width: 520)
    }

    private func step(_ n: Int, _ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("\(n)").font(.ui(11, .bold)).frame(width: 18, height: 18).background(Circle().fill(Brand.accent.opacity(0.18)))
            Text(text).font(.ui(12.5)).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
        }
    }

    /// Checked against the network first; kept only when it passes.
    private func check() async {
        guard let url = URL(string: address.trimmingCharacters(in: .whitespacesAndNewlines)) else { problem = "That isn't an address."; return }
        checking = true
        defer { checking = false }
        problem = await net.problem(with: url)
        guard problem == nil else { return }
        NodeStore.save(url, for: net)
        saved()
        dismiss()
    }
}

/// Settings › General: NEAR Intents' partner key. Without one 1Click adds 0.25% to every quote; with one it charges its
/// partner fees (0.20%, 0.01% on stablecoin and same-asset routes). Shown only as «set», never the key.
struct OneClickKeySection: View {
    /// Read from the Keychain when shown: as the initial value it was read (~4 ms) on every redraw of Settings.
    @State private var set = false
    @State private var adding = false

    var body: some View {
        SettingsSection(icon: "key", title: "NEAR Intents key", line: "Lower fees on NEAR and cross-chain swaps through 1Click.") {
            SettingsGroup {
                SettingsRow(set ? "NEAR Intents: your partner key" : "NEAR Intents: no key — 0.25% more per swap", icon: "key", logo: "near",
                            info: "1Click, NEAR Intents' swap service, takes an extra 0.25% from every quote asked without a key. A free partner key at partners.near-intents.org switches it to the partner fees: 0.20%, and 0.01% on stablecoins and the same coin between networks.") {
                    HStack(spacing: 8) {
                        if set { Button("Remove") { NodeStore.oneClickKey = nil; set = false } }
                        Button(set ? "Change…" : "Add your key…") { adding = true }
                    }
                }
            }
        }
        .sheet(isPresented: $adding) { OneClickKeySheet { set = true } }
        .onAppear { set = NodeStore.oneClickKey != nil }
    }
}

/// Why a partner key, how to get one, and the field; the key is checked with a dry quote before it's kept.
struct OneClickKeySheet: View {
    var saved: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var key = ""
    @State private var checking = false
    @State private var problem: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                TokenLogo(name: "near", size: 28).clipShape(RoundedRectangle(cornerRadius: 7))
                Text("NEAR Intents partner key").font(.ui(20, .bold))
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Why").font(.ui(13, .semibold))
                Text("Buying and selling NEAR, and the moves between networks that go through NEAR Intents, are quoted by its 1Click service. Without a key it adds 0.25% to every quote. With a free partner key it charges its partner fees instead: 0.20%, and 0.01% on stablecoins and the same coin between networks.")
                Text("The key only identifies you to 1Click: it can't move money or sign anything. It's kept in this Mac's Keychain.")
                    .foregroundStyle(.secondary)
            }
            .font(.ui(12.5)).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 6) {
                Text("Get one in two minutes").font(.ui(13, .semibold))
                step(1, "Open partners.near-intents.org and sign in with your email — free.")
                step(2, "In the Partner Dashboard, create an API key and copy the token (a long JWT: eyJ…).")
                step(3, "Paste it below and press Check and save.")
                Link("Open partners.near-intents.org", destination: URL(string: "https://partners.near-intents.org")!).font(.ui(12.5))
            }
            SecureField("eyJ…", text: $key)
                .textFieldStyle(.roundedBorder).font(.system(.body, design: .monospaced))
                .onSubmit { Task { await check() } }
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle").font(.ui(12)).foregroundStyle(Brand.loss)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack {
                Spacer()
                if checking { ProgressView().controlSize(.small) }
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Check and save") { Task { await check() } }
                    .keyboardShortcut(.defaultAction).disabled(checking || key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24).frame(width: 520)
    }

    private func step(_ n: Int, _ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("\(n)").font(.ui(11, .bold)).frame(width: 18, height: 18).background(Circle().fill(Brand.accent.opacity(0.18)))
            Text(text).font(.ui(12.5)).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
        }
    }

    private func check() async {
        let token = key.trimmingCharacters(in: .whitespacesAndNewlines)
        checking = true
        defer { checking = false }
        problem = await OneClick.problem(withKey: token)
        guard problem == nil else { return }
        NodeStore.oneClickKey = token
        saved()
        dismiss()
    }
}

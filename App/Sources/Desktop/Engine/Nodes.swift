import EVMChain
import Foundation
import OneClickKit
import SolanaChain
import WalletCore

/// Settings › General › Your own nodes and the NEAR Intents key, as on the Mac (Nodes.swift). A node's address carries
/// the provider's key and the 1Click key is a partner token, so both are kept encrypted to the Windows user (DPAPI, as
/// the model keys), put to use at launch, and only a node's host ever goes to the window.
nonisolated enum NodeNetwork: String, CaseIterable {
    case solana, base, arbitrum, ethereum, polygon, robinhood

    var title: String { self == .robinhood ? "Robinhood" : rawValue.capitalized }
    var logo: String { [.solana: "sol", .arbitrum: "arb", .ethereum: "eth", .polygon: "pol"][self] ?? rawValue }
    var chainId: UInt64? { [.base: 8453, .arbitrum: 42161, .ethereum: 1, .polygon: 137, .robinhood: 4663][self] }
    var custom: URL? { chainId.map(EVMNodes.custom) ?? SolanaNodes.custom }

    func use(_ url: URL?) {
        if let chainId { EVMNodes.set(url, for: chainId) } else { SolanaNodes.custom = url }
    }

    func problem(with url: URL) async -> String? {
        if let chainId { return await EVMNodes.problem(with: url, chainId: chainId) }
        return await SolanaNodes.problem(with: url)
    }

    /// Who gives one for free, and what its address looks like.
    var provider: (name: String, site: String, example: String) {
        switch self {
        case .solana: ("Helius", "https://www.helius.dev", "https://mainnet.helius-rpc.com/?api-key=…")
        case .base: ("Alchemy", "https://www.alchemy.com", "https://base-mainnet.g.alchemy.com/v2/…")
        case .arbitrum: ("Alchemy", "https://www.alchemy.com", "https://arb-mainnet.g.alchemy.com/v2/…")
        case .ethereum: ("Alchemy", "https://www.alchemy.com", "https://eth-mainnet.g.alchemy.com/v2/…")
        case .polygon: ("Alchemy", "https://www.alchemy.com", "https://polygon-mainnet.g.alchemy.com/v2/…")
        case .robinhood: ("Alchemy", "https://www.alchemy.com", "https://…")
        }
    }
}

/// What Settings draws: each network's own node by its host, and whether a 1Click key is kept.
nonisolated struct NodesView: Encodable, Equatable {
    struct Node: Encodable, Equatable { var id, title, logo, provider, site, example: String; var host: String? }
    var nodes: [Node]
    var oneClickKey: Bool
}

extension Engine {
    /// The secrets, one DPAPI blob: «solana», «base»… → a node's address; «1click» → the partner key.
    private static func nodeSecrets() -> [String: String] {
        guard let blob = try? Data(contentsOf: Store.url("nodes.bin")), let plain = Store.unprotect(blob) else { return [:] }
        return (try? JSONDecoder().decode([String: String].self, from: plain)) ?? [:]
    }

    private static func setNodeSecret(_ value: String?, for name: String) {
        var all = nodeSecrets()
        all[name] = value
        guard let plain = try? JSONEncoder().encode(all), let blob = Store.protect(plain) else { return }
        try? blob.write(to: Store.url("nodes.bin"), options: .atomic)
    }

    /// At launch: each saved node is the one its network asks first, and 1Click's partner key goes on its requests.
    static func applyNodes() {
        let all = nodeSecrets()
        for net in NodeNetwork.allCases { net.use(all[net.rawValue].flatMap(URL.init(string:))) }
        OneClick.key = all["1click"]
    }

    func nodesView() -> NodesView {
        NodesView(nodes: NodeNetwork.allCases.map {
            .init(id: $0.rawValue, title: $0.title, logo: $0.logo, provider: $0.provider.name, site: $0.provider.site, example: $0.provider.example, host: $0.custom?.host)
        }, oneClickKey: OneClick.key != nil)
    }

    /// A node of the person's own, checked first (https, the right network, the calls balances and moves need); an empty
    /// address takes it away, back to the built-in ones.
    func setNode(_ network: String?, address: String?) async throws {
        guard let net = network.flatMap(NodeNetwork.init(rawValue:)) else { throw WalletError("Unknown network «\(network ?? "")».") }
        let text = (address ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty {
            Self.setNodeSecret(nil, for: net.rawValue)
            net.use(nil)
            return
        }
        guard let url = URL(string: text), url.host != nil else { throw WalletError(say("That isn't an address.", "Это не адрес.")) }
        if let problem = await net.problem(with: url) { throw WalletError(problem) }
        Self.setNodeSecret(url.absoluteString, for: net.rawValue)
        net.use(url)
    }

    /// The NEAR Intents partner key, checked against 1Click first; empty removes it.
    func setOneClickKey(_ key: String?) async throws {
        let token = (key ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        if token.isEmpty {
            Self.setNodeSecret(nil, for: "1click")
            OneClick.key = nil
            return
        }
        if let problem = await OneClick.problem(withKey: token) { throw WalletError(problem) }
        Self.setNodeSecret(token, for: "1click")
        OneClick.key = token
    }
}

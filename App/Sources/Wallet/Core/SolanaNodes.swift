import Foundation

/// The Solana nodes ihatecrypto asks, in order: the user's own (Settings › General › Solana node; its URL, key and all,
/// lives in the Keychain) first when set, then Solana's public node — rate-limited, but it answers every call — then
/// Pocket's, for when the public one throttles. Read at each call, so a node set in Settings counts at once.
public enum SolanaNodes {
    public static let publicNode = URL(string: "https://api.mainnet-beta.solana.com")!
    /// Keyless, ~0.3 s, 30 calls at once without a 429 (2026-11-09) — a gateway to nodes that differ: none reads token
    /// accounts by program (HTTP 400 «requires a mint filter on this endpoint»), some not by mint either, and its own
    /// budget runs out now and then. So it comes after the public node, the one keyless node that reads a whole wallet,
    /// and each of those refusals goes back to it (passesOn). publicnode refuses token-account reads outright (403).
    public static let pocket = URL(string: "https://solana.api.pocket.network")!
    /// The genesis hash of Solana mainnet: a node that answers another is another network.
    public static let mainnetGenesis = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"
    private static let lock = NSLock()
    nonisolated(unsafe) private static var own: URL?

    public static var custom: URL? {
        get { lock.withLock { own } }
        set { lock.withLock { own = newValue } }
    }

    public static var urls: [URL] { [custom, publicNode, pocket].compactMap { $0 } }

    /// A node that doesn't serve this call, so the next node should: its key refused (401, 403: expired, over its plan),
    /// or the node's own limits in Pocket's words (2026-11-09): HTTP 400 «… requires a mint filter on this endpoint»,
    /// HTTP 400 «request budget exhausted», and -32010 «Tokenkeg… excluded from account secondary indexes» from a backend
    /// without the index. `body`: the HTTP body, or a JSON-RPC error's message. Any other answer is the call's own.
    public static func passesOn(status: Int, body: Data) -> Bool {
        let text = String(decoding: body.prefix(400), as: UTF8.self)
        return status == 401 || status == 403 || ["on this endpoint", "budget exhausted", "secondary indexes"].contains { text.contains($0) }
    }
}

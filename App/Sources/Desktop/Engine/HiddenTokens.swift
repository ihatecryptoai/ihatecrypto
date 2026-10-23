import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import OneClickKit
import RelayBridge
import TradeExecution

/// The tokens the person hid, and airdrops that look like scams, tucked under «N hidden · show» on their network's card
/// (the Mac's HiddenTokens and Workspace.tokenChoices): out of the card's total and the portfolio's until shown.
extension Engine {
    /// The person's say on one token, by Holding.id: shown or hidden. Not per wallet, as on the Mac: a scam token hidden
    /// once stays hidden on every wallet.
    func showToken(_ id: String, shown: Bool) {
        guard !id.isEmpty else { return }
        var choices = settings.tokenChoices ?? [:]
        choices[id] = shown
        settings.tokenChoices = choices
        var p = portfolio
        p.networks = p.networks.map { noted(Self.tucked($0, choices: choices)) }
        p.total = p.networks.reduce(0) { $0 + $1.usd }
        portfolio = withProfit(p)
    }

    /// A network's tokens split into those on show and those tucked away; its total counts the shown ones only.
    nonisolated static func tucked(_ n: PortfolioView.Network, choices: [String: Bool]) -> PortfolioView.Network {
        var n = n
        let all = n.rows + n.hidden
        let shown = { (r: PortfolioView.Row) in r.id.map { choices[$0] ?? r.real ?? true } ?? true }
        n.rows = all.filter(shown)
        // ponytail: the hundred worth the most; the rest only counted. A spammed address had 666 airdrops on Base,
        // and every state carries the list. Page through them if anyone ever needs the 101st.
        let tucked = all.filter { !shown($0) }.sorted { ($0.usd ?? 0) > ($1.usd ?? 0) }
        n.hidden = Array(tucked.prefix(100))
        n.hiddenMore += max(0, tucked.count - 100)
        n.usd = n.rows.reduce(0) { $0 + ($1.usd ?? 0) }
        return n
    }
}

/// Money between networks, as the Mac's «In transit» card: each bridge order signed and not landed yet.
nonisolated struct TransitView: Encodable, Equatable {
    struct Link: Encodable, Equatable { var title: String; var url: String }
    /// The bridge's order: Relay's request, 1Click's deposit address, «domain:burn» for CCTP, a LayerZero transaction.
    var id: String
    var chain: String
    /// «Base USDC→Solana 5», without its transaction.
    var what: String
    var bridge: String
    var logo: String
    /// A deposit address opened for money coming in from elsewhere, not a move of the app's own.
    var receiving: Bool
    var at: Date
    var tx: String?
    var links: [Link]
}

extension Engine {
    /// The orders still on their way, each older than two minutes asked of its bridge first (Executor.resolveTransit).
    func readTransit() async {
        guard let executor = try? executor() else { inTransit = []; return }
        inTransit = await executor.resolveTransit().map(Self.transit)
    }

    nonisolated static func transit(_ e: Journal.Entry) -> TransitView {
        let (bridge, logo) = switch e.chain {
        case "1click": ("NEAR Intents", "near")
        case "cctp": ("Circle's CCTP", "usdc")
        case "layerzero": ("LayerZero", "usdt")
        default: ("Relay", "relay")
        }
        let tx = e.note.components(separatedBy: ", tx ").dropFirst().first.map { String($0.prefix { !$0.isWhitespace }) }
        var links: [TransitView.Link] = []
        switch e.chain {
        case "1click": links.append(.init(title: "NEAR Intents explorer", url: "https://explorer.near-intents.org/transactions/\(e.id)"))
        case "cctp":
            if let colon = e.id.firstIndex(of: ":"), let source = UInt32(e.id[..<colon]),
               let net = EVMNetwork.all.first(where: { CCTP.domain($0.rpc.chainId) == source }) {
                links.append(.init(title: "Burn on \(net.name)", url: "\(net.explorer)/tx/\(e.id[e.id.index(after: colon)...])"))
            }
        case "layerzero": links.append(.init(title: "LayerZero Scan", url: "https://layerzeroscan.com/tx/\(e.id)"))
        default: links.append(.init(title: "Relay", url: "https://relay.link/transaction/\(e.id)"))
        }
        if e.chain != "cctp", let tx, !tx.hasPrefix("0x") { links.append(.init(title: "Solscan", url: "https://solscan.io/tx/\(tx)")) }
        return .init(id: e.id, chain: e.chain, what: e.note.components(separatedBy: ", tx ").first ?? e.note, bridge: bridge, logo: logo,
                     receiving: e.note.hasPrefix("Receive:"), at: e.at, tx: tx, links: links)
    }

    /// The bridge's own word on an order now, for its row opened in the «In transit» card.
    func transitState(chain: String, id: String) async -> String {
        switch chain {
        case "1click":
            guard let s = try? await OneClick.status(depositAddress: id) else { return say("NEAR Intents didn't answer", "NEAR Intents не ответил") }
            return Executor.oneClickWords[s.status] ?? s.status.lowercased()
        case "cctp":
            guard let colon = id.firstIndex(of: ":"), let source = UInt32(id[..<colon]) else { return say("unknown", "неизвестно") }
            return await Executor.cctpForward(source: source, burn: String(id[id.index(after: colon)...])).map { say("minted by Circle", "Circle выпустил") + " (\($0.prefix(10))…)" }
                ?? say("burned, waiting for Circle's mint", "сожжено, ждёт выпуска у Circle")
        case "layerzero":
            guard let s = await Executor.layerZeroStatus(tx: id) else { return say("LayerZero hasn't seen it yet", "LayerZero его ещё не видит") }
            return ["INFLIGHT": say("on its way: waiting for the source network's finality", "в пути: ждёт подтверждения в исходной сети"),
                    "CONFIRMING": say("verified, being delivered", "проверено, доставляется"), "DELIVERED": say("landed", "дошло"), "FAILED": say("failed", "не вышло")][s.status]
                ?? s.status.lowercased()
        default:
            guard let s = try? await Relay.status(requestId: id).status else { return say("Relay didn't answer", "Relay не ответил") }
            return ["waiting": say("waiting for the deposit", "ждёт депозита"), "pending": say("deposit seen, filling", "депозит виден, исполняется"),
                    "submitted": say("fill sent", "исполнение отправлено"), "success": say("landed", "дошло"),
                    "delayed": say("delayed — Relay is still on it", "задерживается — Relay ещё занят"), "refund": say("refunded", "возвращено"),
                    "failure": say("failed", "не вышло")][s] ?? s
        }
    }
}

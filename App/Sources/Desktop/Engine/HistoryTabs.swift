import EVMChain
import ExecutionCore
import Foundation
import IhatecryptoCore
import TradeExecution

/// The History page's three tabs, as the Mac's HistoryView: «Agent» — each request with what it was understood as, the
/// cards it had on the canvas and what was said, and the scheduled runs; earlier signature sessions under them;
/// «Positions» — each Hyperliquid position from open to close with its realized P/L and fees; «All» — every request,
/// fill, transfer and signature in one timeline (the app's journal and the terminal tool's). Read when the page asks.
nonisolated struct HistoryTabs: Encodable {
    struct Bubble: Encodable, Equatable {
        var kind: String
        var label: String
        var title: String
        var lines: [String] = []
        var status = "info"
    }
    struct Activity: Encodable, Equatable {
        /// «request», «execution» or «automatic».
        var kind: String
        var at: Date
        var request: String?
        var understood: String?
        var title: String?
        var coin: String?
        var bubbles: [Bubble] = []
        var lines: [String] = []
        var failed = false
    }
    struct Event: Encodable {
        /// «open», «close» or «activity».
        var kind: String
        var at: Date
        var label: String
        var title: String
        var lines: [String]
    }
    struct Story: Encodable {
        var coin: String
        var long: Bool
        var open: Bool
        var opened: Date
        var closed: Date?
        var realized: Decimal
        var fees: Decimal
        var peak: Decimal
        var events: [Event]
    }
    struct Row: Encodable {
        var at: Date
        /// An icon's name and its tone: «accent», «profit», «loss» or «secondary».
        var icon: String
        var tone: String
        var text: String
        var detail: String?
        var link: String?
    }
    var agent: [Activity]
    var earlier: [Activity]
    var stories: [Story]
    var rows: [Row]
    /// Hyperliquid's history didn't load: why.
    var error: String?
    var hyperliquid: Bool
}

extension Engine {
    /// What the canvases kept, as activities: each request with the cards made after it (until the next one) and the
    /// replies; each scheduled run on its own. Newest last.
    var historyActivities: [HistoryTabs.Activity] {
        var out: [HistoryTabs.Activity] = []
        let names = Set(markets.keys)
        for c in canvases {
            let asked = c.messages.filter(\.fromUser).sorted { $0.at < $1.at }
            for (i, q) in asked.enumerated() {
                let until = i + 1 < asked.count ? asked[i + 1].at : .distantFuture
                let said = c.messages.filter { !$0.fromUser && $0.at >= q.at && $0.at < until }.map(\.text)
                let made = c.boards.filter { $0.at >= q.at && $0.at < until && !Self.liveBoards.contains($0.kind) }.sorted { $0.at < $1.at }
                let bubbles = made.flatMap { b -> [HistoryTabs.Bubble] in
                    let status = b.state == "failed" ? "blocked" : b.state == "done" ? "done" : "info"
                    if let steps = b.steps, !steps.isEmpty {
                        return steps.map { HistoryTabs.Bubble(kind: "step", label: $0.label, title: $0.title, lines: Array($0.lines.prefix(2)), status: status) }
                    }
                    return [HistoryTabs.Bubble(kind: b.kind, label: b.kind == "chart" ? "Chart" : "Result", title: b.title, lines: Array(b.lines.prefix(2)), status: status)]
                }
                out.append(HistoryTabs.Activity(kind: "request", at: q.at, request: q.text, understood: q.understood, title: made.first { $0.kind == "move" }?.title,
                                                coin: IntentParser.namedAsset(q.text, markets: names), bubbles: bubbles, lines: said))
            }
        }
        for r in scheduleRuns {
            out.append(HistoryTabs.Activity(kind: "automatic", at: r.at, title: r.title, lines: r.line.map { [$0] } ?? [], failed: r.result == "failed"))
        }
        return out.sorted { $0.at < $1.at }
    }

    /// The chat's model read this message as `canonical`: kept with it for the History's «understood as».
    func noteUnderstood(_ canonical: String) {
        guard let i = messages.lastIndex(where: \.fromUser) else { return }
        messages[i].understood = canonical
    }

    func historyTabs() async -> String {
        let activities = historyActivities
        // Signatures from the app and from the terminal tool, which trades from the same wallet.
        let files = [Store.url("journal.jsonl"), FileManager.default.homeDirectoryForCurrentUser.appending(path: ".ihatecrypto/journal.jsonl")]
        let journal = await Task.detached { files.flatMap { (try? Journal(url: $0).entries()) ?? [] } }.value
        var fills: [TradeHistory.Fill] = [], transfers: [TradeHistory.Transfer] = [], problem: String?
        if let user = evmAddress {
            do {
                async let f = TradeHistory.fills(user: user)
                async let t = TradeHistory.transfers(user: user)
                fills = try await f
                transfers = try await t
            } catch {
                problem = say("Hyperliquid's history didn't load: ", "История Hyperliquid не загрузилась: ") + error.localizedDescription
            }
        }
        let tabs = Self.historyTabs(activities: activities, journal: journal, fills: fills, transfers: transfers, ru: russian)
        let out = HistoryTabs(agent: tabs.agent, earlier: tabs.earlier, stories: tabs.stories, rows: tabs.rows, error: problem, hyperliquid: evmAddress != nil)
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        return (try? encoder.encode(out)).map { String(decoding: $0, as: UTF8.self) } ?? "{}"
    }

    /// The three tabs from plain inputs, so a test can build them.
    nonisolated static func historyTabs(activities: [HistoryTabs.Activity], journal: [Journal.Entry], fills: [TradeHistory.Fill],
                                        transfers: [TradeHistory.Transfer], ru: Bool)
        -> (agent: [HistoryTabs.Activity], earlier: [HistoryTabs.Activity], stories: [HistoryTabs.Story], rows: [HistoryTabs.Row]) {
        let say = { (en: String, r: String) in ru ? r : en }
        let agent = activities.reversed().filter { !$0.lines.isEmpty || !$0.bubbles.isEmpty || $0.request != nil }
        let earlier = sessions(journal, before: activities.first?.at ?? .distantFuture, ru: ru)
        let signed = { (v: Decimal) in (v < 0 ? "−" : "+") + Fmt.usd(abs(v)) }
        let byCoin = Dictionary(grouping: activities.filter { $0.coin != nil }) { $0.coin!.uppercased() }
        let stories = TradeHistory.stories(fills).reversed().map { story -> HistoryTabs.Story in
            var events = story.fills.map { f in
                HistoryTabs.Event(kind: f.dir.hasPrefix("Open") ? "open" : "close", at: f.time, label: f.dir,
                                  title: "\(Fmt.num(f.sz, maxFraction: 6)) " + say("at", "по") + " \(Fmt.price(f.px))",
                                  lines: [f.closedPnl != 0 ? signed(f.closedPnl) + say(" · fee ", " · комиссия ") + Fmt.usd(f.fee) : say("fee ", "комиссия ") + Fmt.usd(f.fee)])
            }
            // ihatecrypto's own steps on this position: from 15 min before it opened to 2 min after it closed.
            let coin = IntentParser.pretty(story.coin).uppercased()
            events += (byCoin[coin] ?? byCoin[story.coin.uppercased()] ?? []).filter { a in
                a.at >= story.opened.addingTimeInterval(-900) && a.at <= (story.closed ?? .distantFuture).addingTimeInterval(120)
            }.map { a in
                HistoryTabs.Event(kind: "activity", at: a.at, label: a.kind == "automatic" ? say("Guardian", "Страж") : a.request != nil ? say("You asked", "Ты попросил") : "ihatecrypto",
                                  title: a.request ?? a.title ?? "", lines: Array(a.lines.suffix(1)))
            }
            return HistoryTabs.Story(coin: IntentParser.pretty(story.coin), long: story.long, open: story.isOpen, opened: story.opened, closed: story.closed,
                                     realized: story.realized, fees: story.fees, peak: story.peakSize, events: events.sorted { $0.at < $1.at })
        }
        var rows: [HistoryTabs.Row] = []
        for a in activities {
            if let q = a.request { rows.append(.init(at: a.at, icon: "person", tone: "accent", text: say("You: “\(q)”", "Ты: «\(q)»"), detail: a.lines.first)) }
            else if let t = a.title { rows.append(.init(at: a.at, icon: a.kind == "execution" ? "signature" : "gear", tone: a.failed ? "loss" : "accent", text: t, detail: a.lines.last)) }
        }
        for f in fills {
            let pnl = f.closedPnl != 0 ? " · \(signed(f.closedPnl))" : ""
            rows.append(.init(at: f.time, icon: f.dir.hasPrefix("Open") ? "arrow.up.right.circle" : "arrow.down.left.circle", tone: f.closedPnl < 0 ? "loss" : "profit",
                              text: "\(f.dir) \(IntentParser.pretty(f.coin)) \(Fmt.num(f.sz, maxFraction: 6)) " + say("at", "по") + " \(Fmt.price(f.px))\(pnl)",
                              detail: say("fee ", "комиссия ") + Fmt.usd(f.fee), link: f.hash.map { "https://app.hyperliquid.xyz/explorer/tx/\($0)" }))
        }
        for t in transfers {
            rows.append(.init(at: t.time, icon: t.amount > 0 ? "arrow.down.circle" : t.amount < 0 ? "arrow.up.circle" : "arrow.left.arrow.right", tone: "secondary", text: t.text))
        }
        for e in journal where e.kind == .signed || e.kind == .settled || e.kind == .failed {
            let link: String? = switch e.chain {
            case "solana": "https://solscan.io/tx/\(e.id)"
            case "relay": "https://relay.link/transaction/\(e.id)"
            case "near": "https://nearblocks.io/txns/\(e.id)"
            default: e.id.hasPrefix("0x") && e.id.count == 66 ? EVMNetwork.named(e.chain).map { "\($0.explorer)/tx/\(e.id)" } : nil
            }
            let what = e.kind == .settled ? say("Settled", "Завершено") : e.kind == .failed ? say("Failed", "Не вышло") : say("Signed", "Подписано")
            rows.append(.init(at: e.at, icon: e.kind == .failed ? "xmark.octagon" : "signature", tone: e.kind == .failed ? "loss" : "secondary",
                              text: "\(what) · \(e.chain.prefix(1).uppercased() + e.chain.dropFirst()): \(english(e.note))", link: link))
        }
        return (Array(agent), earlier, Array(stories), Array(rows.sorted { $0.at > $1.at }.prefix(400)))
    }

    /// Signatures grouped into sessions (a pause of 10 minutes starts a new one), newest first; each is an activity whose
    /// bubbles are its signatures with what came of them.
    nonisolated static func sessions(_ journal: [Journal.Entry], before start: Date, ru: Bool = false) -> [HistoryTabs.Activity] {
        let outcome = Dictionary(journal.filter { $0.kind != .signed }.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        var groups: [[Journal.Entry]] = []
        for e in journal.filter({ $0.kind == .signed && $0.at < start }).sorted(by: { $0.at < $1.at }) {
            if let last = groups.last?.last, e.at.timeIntervalSince(last.at) < 600 { groups[groups.count - 1].append(e) } else { groups.append([e]) }
        }
        let name = { (c: String) in c.prefix(1).uppercased() + c.dropFirst() }
        return groups.reversed().map { group in
            let bubbles = group.map { e in
                let result = outcome[e.id].map { $0.kind == .failed ? "failed: \(english($0.note))" : english($0.note) }
                return HistoryTabs.Bubble(kind: "signed", label: name(e.chain), title: english(e.note).components(separatedBy: " · valid until").first ?? e.note,
                                          lines: result.flatMap { ["", "ok", "success"].contains($0) ? nil : [$0] } ?? [],
                                          status: outcome[e.id]?.kind == .failed ? "blocked" : "info")
            }
            var titles: [String] = []
            for b in bubbles where !titles.contains(b.title) { titles.append(b.title) }
            let chains = Set(group.map { name($0.chain) }).sorted().joined(separator: ", ")
            return HistoryTabs.Activity(kind: "execution", at: group[0].at, title: titles.prefix(3).joined(separator: ", ") + (titles.count > 3 ? "…" : ""),
                                        bubbles: bubbles,
                                        lines: [ru ? "Подписей: \(group.count) · \(chains)" : "\(group.count) signature\(group.count == 1 ? "" : "s") · \(chains)"])
        }
    }

    /// Journal notes written before the app switched to English.
    nonisolated static func english(_ note: String) -> String {
        let words: [(String, String)] = [
            ("плечо", "leverage"), ("вход", "entry"), ("стоп и тейк", "stop and take-profit"), ("отмена ордеров", "cancel orders"),
            ("закрытие", "close"), ("новый стоп", "new stop"), ("снять старый стоп", "remove old stop"), ("частичный тейк", "partial take-profit"),
            ("частичное закрытие", "partial close"), ("новый тейк", "new take-profit"), ("снять старый тейк", "remove old take-profit"),
            ("стоит в стакане", "resting on the book"), ("HLP депозит", "HLP deposit"), ("лимит со стопом и тейком", "limit with stop and take-profit"),
            ("обернуть SOL", "wrap SOL"),
        ]
        if let hit = words.first(where: { note == $0.0 }) { return hit.1 }
        if let m = note.firstMatch(of: /исполнено\s+([\d.]+)\s+по\s+([\d.]+)/) { return "filled \(m.1) at \(m.2)" }
        return note
            .replacingOccurrences(of: "закрыть пустой счёт", with: "close the empty account")
            .replacingOccurrences(of: "закрыть счёт wSOL (SOL развёрнут)", with: "close the wSOL account (SOL unwrapped)")
            .replacingOccurrences(of: "закрыть счёт долей", with: "close the share account")
            .replacingOccurrences(of: "счёт долей", with: "share account")
            .replacingOccurrences(of: "; valid until block", with: " · valid until block")
            .replacingOccurrences(of: "; действует до блока", with: " · valid until block")
    }
}

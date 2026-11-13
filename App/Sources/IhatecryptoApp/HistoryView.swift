import ExecutionCore
import Foundation
import HelperAPI
import IhatecryptoCore
import SwiftUI
import TradeExecution

/// Everything ihatecrypto did and everything that happened to the money, on the canvas:
/// «Agent» — each activity with the bubbles it had; «Positions» — each position from open to close, from the exchange;
/// «All» — every fill, transfer, signature and request in one timeline.
struct HistoryView: View {
    @Environment(Workspace.self) private var ws
    @AppStorage("historyTab") private var tab = "agent"
    @State private var fills: [TradeHistory.Fill] = []
    @State private var transfers: [TradeHistory.Transfer] = []
    @State private var journal: [Journal.Entry] = []
    /// Sessions of signatures from before the agent history was kept.
    @State private var earlier: [Activity] = []
    @State private var loading = false
    @State private var loadError: String?
    /// The «All» rows and each position with its activities, built when what they come from changes (`inputs`), not on
    /// every pass: ~2,000 fills sorted with everything else.
    @State private var rows: [Row] = []
    @State private var stories: [(story: TradeHistory.Story, activities: [Activity])] = []
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            PageHeader(title: "History", close: close) {
                Picker("", selection: $tab) {
                    Text("Agent").tag("agent")
                    Text("Positions").tag("positions")
                    Text("All").tag("all")
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
                if loading { ProgressView().controlSize(.small) }
                Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.hoverIcon).help("Refresh")
            }
            if let loadError { Text(loadError).font(.ui(10)).foregroundStyle(Brand.loss).padding(.horizontal, 28) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    switch tab {
                    case "positions": positions
                    case "all": everything
                    default: agent
                    }
                }
                .padding(.horizontal, 28).padding(.bottom, 28)
                .frame(maxWidth: 760, alignment: .leading)
            }
        }
        // Full width: the chat floats above the page, as it does over the canvas.
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Brand.canvas)
        .task { await load() }
        .onChange(of: inputs, initial: true) { build() }
    }

    /// What the rows and positions come from, cheaply: activities are only added and trimmed, and only the newest one
    /// changes; the rest is replaced whole by `load` (a fresh 2,000 fills can keep the count, not the newest fill).
    private var inputs: [AnyHashable] {
        let last = ws.activities.last
        return [ws.activities.count, last?.id, last?.title, last?.coin, last?.lines.count,
                fills.count, fills.first?.time, fills.last?.time, transfers.count, journal.count]
    }

    private func build() {
        rows = allRows()
        let byCoin = Dictionary(grouping: ws.activities, by: \.coin)
        stories = TradeHistory.stories(fills).map { story in
            (story: story, activities: (byCoin[story.coin] ?? []).filter { a in
                a.at >= story.opened.addingTimeInterval(-900) && a.at <= (story.closed ?? .distantFuture).addingTimeInterval(120)
            })
        }
    }

    // MARK: Agent

    @ViewBuilder private var agent: some View {
        let items = ws.activities.reversed().filter { !$0.lines.isEmpty || !$0.bubbles.isEmpty || $0.request != nil }
        if items.isEmpty, earlier.isEmpty {
            empty("Nothing yet. Every request from now on is kept here with the bubbles it had on the canvas.")
        }
        ForEach(Array(items)) { ActivityCard(activity: $0) }
        if !earlier.isEmpty {
            Label("Earlier — from the signature journal. The chat of that time wasn’t kept; from now on every request is.", systemImage: "clock.arrow.circlepath")
                .font(.ui(10)).foregroundStyle(.secondary).padding(.top, 6)
            ForEach(earlier) { ActivityCard(activity: $0) }
        }
    }

    // MARK: Positions

    @ViewBuilder private var positions: some View {
        if stories.isEmpty, !loading {
            empty(ws.hlAddress.isEmpty ? "No Hyperliquid address yet." : "No trades on Hyperliquid yet.")
        }
        ForEach(stories, id: \.story.id) { StoryCard(story: $0.story, activities: $0.activities) }
    }

    // MARK: All

    private struct Row: Identifiable {
        let id: String
        let at: Date
        let icon: String
        let tint: Color
        let text: String
        var detail: String?
        var link: URL?
    }

    @ViewBuilder private var everything: some View {
        if rows.isEmpty, !loading { empty("Nothing yet.") }
        ForEach(rows) { r in
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                Text(Fmt.date(r.at)).font(.ui(10)).monospacedDigit().foregroundStyle(.secondary).frame(width: 118, alignment: .leading)
                Image(systemName: r.icon).foregroundStyle(r.tint).frame(width: 18)
                VStack(alignment: .leading, spacing: 2) {
                    Text(r.text).font(.ui(13))
                    if let d = r.detail { Text(ChatText.styled(d)).font(.ui(10)).foregroundStyle(.secondary) }
                }
                Spacer(minLength: 0)
                if let link = r.link { Link(destination: link) { Image(systemName: "arrow.up.right.square") }.help(link.absoluteString) }
            }
            .padding(.vertical, 6)
            Divider()
        }
    }

    private func allRows() -> [Row] {
        var rows: [Row] = []
        for a in ws.activities {
            if let q = a.request { rows.append(Row(id: "q\(a.id)", at: a.at, icon: "person.fill", tint: Brand.accent, text: "You: “\(q)”", detail: a.lines.first)) }
            else if let t = a.title { rows.append(Row(id: "a\(a.id)", at: a.at, icon: a.kind == .execution ? "signature" : "gearshape.2", tint: Brand.accent, text: t, detail: a.lines.last)) }
        }
        // One order can fill against several others in the same millisecond: same time, coin and side, so the place in
        // the list keeps each row its own.
        for (n, f) in fills.enumerated() {
            let pnl = f.closedPnl != 0 ? " · \(signed(f.closedPnl))" : ""
            rows.append(Row(id: "f\(n)·\(f.time.timeIntervalSince1970)\(f.coin)\(f.dir)", at: f.time, icon: f.dir.hasPrefix("Open") ? "arrow.up.right.circle" : "arrow.down.left.circle",
                            tint: f.closedPnl < 0 ? Brand.loss : Brand.profit,
                            text: "\(f.dir) \(IntentParser.pretty(f.coin)) \(Fmt.num(f.sz, maxFraction: 6)) at \(Fmt.price(f.px))\(pnl)",
                            detail: "fee \(Fmt.usd(f.fee))",
                            link: f.hash.flatMap { URL(string: "https://app.hyperliquid.xyz/explorer/tx/\($0)") }))
        }
        for t in transfers {
            rows.append(Row(id: "t\(t.id)", at: t.time, icon: t.amount > 0 ? "arrow.down.circle" : t.amount < 0 ? "arrow.up.circle" : "arrow.left.arrow.right.circle",
                            tint: .secondary, text: t.text))
        }
        for e in journal where e.kind == .signed || e.kind == .settled || e.kind == .failed {
            let link: URL? = switch e.chain {
            case "solana": URL(string: "https://solscan.io/tx/\(e.id)")
            case "relay": URL(string: "https://relay.link/transaction/\(e.id)")
            default: nil
            }
            rows.append(Row(id: "j\(e.at.timeIntervalSince1970)\(e.id)\(e.kind.rawValue)", at: e.at, icon: e.kind == .failed ? "xmark.octagon" : "signature",
                            tint: e.kind == .failed ? Brand.loss : .secondary,
                            text: "\(e.kind == .settled ? "Settled" : e.kind == .failed ? "Failed" : "Signed") · \(e.chain.capitalized): \(Self.english(e.note))", link: link))
        }
        return rows.sorted { $0.at > $1.at }.prefix(400).map { $0 }
    }

    private func empty(_ text: String) -> some View {
        Text(text).font(.ui(12)).foregroundStyle(.secondary)
            .padding(20).frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 18).strokeBorder(Brand.dots, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4])))
    }

    // MARK: Data

    private func load() async {
        loading = true
        defer { loading = false }
        loadError = nil
        // Signatures from the app and from the terminal tool, which trades from the same wallet.
        let appJournal = LocalStore.url("journal.jsonl")
        let cliJournal = Helper.home.appending(path: ".ihatecrypto/journal.jsonl")
        // Off the main thread: every signature ever, a JSON line each.
        let files = [appJournal, cliJournal]
        journal = await Task.detached { files.flatMap { (try? Journal(url: $0).entries()) ?? [] } }.value
        earlier = Self.sessions(journal, before: ws.activities.first?.at ?? .distantFuture)
        guard !ws.hlAddress.isEmpty else { return }
        do {
            async let f = TradeHistory.fills(user: ws.hlAddress)
            async let t = TradeHistory.transfers(user: ws.hlAddress)
            fills = try await f
            transfers = try await t
        } catch {
            loadError = "Hyperliquid history didn't load: \(error.localizedDescription)"
        }
    }

    /// Signatures grouped into sessions (a pause of 10 minutes starts a new one), newest first; each is an activity whose
    /// bubbles are its signatures with what came of them.
    static func sessions(_ journal: [Journal.Entry], before start: Date) -> [Activity] {
        let outcome = Dictionary(journal.filter { $0.kind != .signed }.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        var groups: [[Journal.Entry]] = []
        for e in journal.filter({ $0.kind == .signed && $0.at < start }).sorted(by: { $0.at < $1.at }) {
            if let last = groups.last?.last, e.at.timeIntervalSince(last.at) < 600 { groups[groups.count - 1].append(e) } else { groups.append([e]) }
        }
        return groups.reversed().map { group in
            let bubbles = group.map { e in
                let result = outcome[e.id].map { $0.kind == .failed ? "failed: \(english($0.note))" : english($0.note) }
                return ActivityBubble(kind: "signed", label: e.chain.capitalized, title: english(e.note).components(separatedBy: " · valid until").first ?? e.note,
                                      lines: result.flatMap { ["", "ok", "success"].contains($0) ? nil : [$0] } ?? [],
                                      status: outcome[e.id]?.kind == .failed ? "blocked" : "info",
                                      logo: ["solana": "sol", "hyperliquid": "hype", "relay": "relay"][e.chain])
            }
            var titles: [String] = []
            for b in bubbles where !titles.contains(b.title) { titles.append(b.title) }
            return Activity(at: group[0].at, kind: .execution, title: titles.prefix(3).joined(separator: ", ") + (titles.count > 3 ? "…" : ""),
                            bubbles: bubbles, lines: ["\(group.count) signature\(group.count == 1 ? "" : "s") · \(Set(group.map(\.chain.capitalized)).sorted().joined(separator: ", "))"])
        }
    }

    /// Journal notes written before the app switched to English.
    static func english(_ note: String) -> String {
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

/// The latest of what moved, on the canvas, to be read at a glance: what the day did to the money in a line or two, then
/// each move as one row — what it was, how much, where to — newest first, by day. A row opens to the request with the
/// bubbles it had; the chat answers «where did my money go» from the same lines; the whole history is one click away.
struct HistoryBoard: View {
    @Environment(Workspace.self) private var ws
    @AppStorage("canvasMode") private var mode = "plan"
    /// The activity whose row is open.
    @State private var open: UUID?

    private typealias Move = (activity: Activity, line: MovedLine, n: Int)

    /// Each move ihatecrypto made, newest first, with the activity it came from.
    private var moves: [Move] {
        ws.activities.reversed().flatMap { a in a.lines.compactMap(MovedLine.init).enumerated().reversed().map { (a, $0.element, $0.offset) } }
    }

    var body: some View {
        let moves = self.moves
        let shown = Array(moves.prefix(14))
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: "clock.arrow.circlepath").foregroundStyle(Brand.accentInk).font(.system(size: 16, weight: .semibold))
                Text("History · latest").font(.ui(16, .bold))
                Spacer()
                Button { withAnimation(StudioView.spring) { ws.showHistory = false } } label: { Image(systemName: "xmark") }
                    .buttonStyle(.hoverIcon).help("Close")
            }
            if let closed = ws.recentlyClosed, !closed.isEmpty {
                ClosedTrades(stories: closed)
                    .padding(16)
                    .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
                    .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Brand.cardBorder))
            }
            if let latest = moves.first {
                let day = Calendar.current.startOfDay(for: latest.activity.at)
                summary(day, moves.filter { Calendar.current.startOfDay(for: $0.activity.at) == day }.map(\.line))
            }
            if shown.isEmpty {
                // Nothing moved yet: the latest requests as they were.
                let items = ws.activities.reversed().filter { !$0.lines.isEmpty || !$0.bubbles.isEmpty || $0.request != nil }.prefix(3)
                if items.isEmpty {
                    Text("Nothing yet. Every move from now on is listed here, and every request keeps the bubbles it had on the canvas.")
                        .font(.ui(12)).foregroundStyle(.secondary)
                }
                ForEach(Array(items)) { ActivityCard(activity: $0) }
            } else {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(shown.enumerated()), id: \.offset) { i, m in
                        if i == 0 || !Calendar.current.isDate(m.activity.at, inSameDayAs: shown[i - 1].activity.at) {
                            Text(Self.day(m.activity.at).uppercased()).font(.ui(9.5, .heavy)).tracking(0.6).foregroundStyle(.secondary)
                                .padding(.top, i == 0 ? 0 : 10).padding(.bottom, 2).padding(.leading, 10)
                        }
                        row(m)
                        if open == m.activity.id, i + 1 == shown.count || shown[i + 1].activity.id != m.activity.id {
                            ActivityCard(activity: m.activity).padding(.vertical, 6)
                        }
                    }
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
                .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Brand.cardBorder))
            }
            Button { withAnimation(.snappy) { mode = "history" } } label: {
                Label("All history: requests, positions, transfers, signatures", systemImage: "list.bullet.rectangle")
            }
            .buttonStyle(.borderless).font(.ui(10, .semibold))
        }
        .frame(width: 470, alignment: .leading)
    }

    /// What the day did to the money, from its moves; and a question for the chat, which reads the same lines.
    private func summary(_ day: Date, _ lines: [MovedLine]) -> some View {
        let own = Set(lines.compactMap(\.to).filter(isOwnWallet))
        return VStack(alignment: .leading, spacing: 6) {
            Text(Self.day(day)).font(.ui(11, .bold)).foregroundStyle(.secondary)
            ForEach(MovedLine.summary(lines), id: \.self) { Text($0).font(.ui(14, .semibold)).monospacedDigit() }
            if !own.isEmpty {
                Text("\(own.sorted().joined(separator: ", ")) \(own.count == 1 ? "is" : "are") your own wallet: what went there is still yours, outside the balance.")
                    .font(.ui(10.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Button {
                let ask = ws.say("Where did my money go \(Self.day(day).lowercased())?", "Куда ушли деньги \(Calendar.current.isDateInToday(day) ? "сегодня" : Calendar.current.isDateInYesterday(day) ? "вчера" : Self.day(day))?")
                Task { await ws.send(ask) }
            } label: {
                Label("Ask the chat where the money went", systemImage: "bubble.left.and.text.bubble.right")
                    .font(.ui(11, .semibold)).padding(.horizontal, 10).padding(.vertical, 5)
                    .background(Capsule().fill(Color.primary.opacity(0.05))).contentShape(Capsule())
            }
            .buttonStyle(.plain).modifier(HoverLift(radius: 20)).padding(.top, 2)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Brand.cardBorder))
    }

    /// One move: what it was, when and where to, how much left or came in; a click opens the request it came from.
    private func row(_ m: Move) -> some View {
        let l = m.line, own = l.to.map(isOwnWallet) ?? false
        let amount: (String, Color)? = switch l.kind {
        case .sent: l.amount.map { ("−\(Fmt.num($0, maxFraction: 6)) \(l.symbol ?? "")", own ? .secondary : Brand.loss) }
        case .received: l.amount.map { ("+\(Fmt.num($0, maxFraction: 6)) \(l.symbol ?? "")", Brand.profit) }
        default: nil
        }
        return HStack(spacing: 8) {
            Button { withAnimation(.snappy) { open = open == m.activity.id ? nil : m.activity.id } } label: {
                HStack(spacing: 10) {
                    Image(systemName: Self.icon(l.kind)).font(.system(size: 12, weight: .bold)).foregroundStyle(Brand.accentInk)
                        .frame(width: 28, height: 28).background(Circle().fill(Color.primary.opacity(0.05)))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(l.text).font(.ui(12.5, .semibold)).lineLimit(1).truncationMode(.middle)
                        HStack(spacing: 5) {
                            Text(Fmt.time(m.activity.at)).monospacedDigit()
                            if own { Text("· your wallet") }
                            if let q = m.activity.request, q.count > 6 { Text("· “\(q)”").lineLimit(1).truncationMode(.tail) }
                        }
                        .font(.ui(10)).foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 6)
                    if let amount { Text(amount.0).font(.ui(12, .bold)).monospacedDigit().foregroundStyle(amount.1).lineLimit(1).fixedSize() }
                }
                .padding(.horizontal, 8).padding(.vertical, 7)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain).modifier(HoverLift(radius: 12))
            .accessibilityLabel(l.text)
            if let link = l.link {
                Button { ws.openWeb(link) } label: { Image(systemName: "arrow.up.right.square") }
                    .buttonStyle(.hoverIcon).help("See it on the explorer")
            }
        }
    }

    private func isOwnWallet(_ name: String) -> Bool { ws.addressBook.contains { $0.name == name && $0.label == "Your wallet" } }

    static func day(_ d: Date) -> String {
        Calendar.current.isDateInToday(d) ? "Today" : Calendar.current.isDateInYesterday(d) ? "Yesterday"
            : d.formatted(Date.FormatStyle(locale: Locale(identifier: "en_US")).day().month(.abbreviated))
    }

    static func icon(_ k: MovedLine.Kind) -> String {
        switch k {
        case .sent: "arrow.up.right"
        case .received: "arrow.down.left"
        case .swapped, .bought, .sold: "arrow.left.arrow.right"
        case .moved: "arrow.triangle.branch"
        case .deposited: "building.columns"
        case .withdrew: "arrow.uturn.backward"
        case .closed: "xmark"
        }
    }
}

/// One activity: the request, how it was understood, the bubbles it had, what ihatecrypto said.
struct ActivityCard: View {
    let activity: Activity
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: icon).foregroundStyle(Brand.accentInk)
                Text(activity.title ?? (activity.kind == .request ? "Request" : "ihatecrypto")).font(.ui(14, .semibold)).lineLimit(1)
                Spacer()
                Text(Fmt.date(activity.at)).font(.ui(10)).foregroundStyle(.secondary).monospacedDigit()
            }
            if let q = activity.request {
                Text(q).font(.ui(13)).foregroundStyle(Brand.onAccent)
                    .padding(.horizontal, 12).padding(.vertical, 8)
                    .background(UnevenRoundedRectangle(topLeadingRadius: 14, bottomLeadingRadius: 14, bottomTrailingRadius: 4, topTrailingRadius: 14).fill(Brand.accent))
            }
            if let u = activity.understood {
                Label("understood as “\(u)”", systemImage: "character.bubble").font(.ui(10)).foregroundStyle(.secondary)
            }
            if !activity.bubbles.isEmpty {
                Flow {
                    ForEach(activity.bubbles, id: \.self) { HistoryBubble(bubble: $0) }
                }
            }
            let lines = expanded ? activity.lines : Array(activity.lines.suffix(2))
            ForEach(lines.indices, id: \.self) { i in
                Text(ChatText.styled(lines[i])).font(.ui(10)).foregroundStyle(.secondary).lineLimit(expanded ? nil : 3).textSelection(.enabled)
            }
            if activity.lines.count > 2 {
                Button(expanded ? "Less" : "All \(activity.lines.count) lines") { withAnimation { expanded.toggle() } }
                    .buttonStyle(.borderless).font(.ui(10))
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Brand.cardBorder))
    }

    private var icon: String {
        switch activity.kind {
        case .request: "bubble.left.and.text.bubble.right"
        case .execution: "signature"
        case .automatic: "gearshape.2"
        }
    }
}

/// A bubble from the history, drawn like it was on the canvas, a bit smaller.
struct HistoryBubble: View {
    let bubble: ActivityBubble

    var body: some View {
        Group {
            if let step = bubble.routeStep {
                // As tall as the canvas card less what the narrower picture gives back: its words are the same size.
                RouteCard(step: step, state: .plain, width: 210, height: RouteCards.cardHeight - 140 * (RouteCards.cardWidth - 210) / 340)
            } else {
                generic
            }
        }
        .frame(idealWidth: 210, maxWidth: 210, alignment: .topLeading)
    }

    /// A chart, a rate or a take-profit choice: not a step of a move.
    private var generic: some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 7) {
                if let logo = bubble.logo { TokenLogo(name: logo, size: 20) }
                if !bubble.asset.isEmpty { AssetBadge(coin: bubble.asset, size: 20, dark: true) }
                Text(bubble.label.uppercased()).font(.ui(9.5, .heavy)).tracking(0.5).opacity(0.8)
            }
            Text(bubble.title).font(.ui(14, .bold)).lineLimit(3)
            ForEach(bubble.lines.prefix(2), id: \.self) { Text(ChatText.styled($0)).font(.ui(10)).opacity(0.8).lineLimit(3) }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: bubble.kind == "yield" ? 28 : 18).fill(Brand.card))
    }
}

/// A position from the exchange: how it opened, what changed, how it closed and what it made.
struct StoryCard: View {
    let story: TradeHistory.Story
    let activities: [Activity]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                AssetBadge(coin: story.coin, size: 34, dark: true)
                VStack(alignment: .leading, spacing: 2) {
                    Text("\(story.long ? "Long" : "Short") \(IntentParser.pretty(story.coin))").font(.ui(16, .heavy)).lineLimit(2)
                    Text(span).font(.ui(10)).foregroundStyle(.secondary)
                }
                Spacer()
                if story.isOpen {
                    StatusChip(text: "open", tint: Brand.accentInk)
                } else {
                    Text(signed(story.realized)).font(.ui(18, .heavy)).monospacedDigit()
                        .foregroundStyle(story.realized < 0 ? Brand.loss : Brand.profit)
                }
            }
            Flow {
                ForEach(events.indices, id: \.self) { i in
                    HStack(alignment: .top, spacing: 10) {
                        if i > 0 { Image(systemName: "chevron.right").foregroundStyle(.tertiary).padding(.top, 30) }
                        events[i]
                    }
                }
            }
            if !story.isOpen {
                Text("Realized \(signed(story.realized)) after \(Fmt.usd(story.fees)) in fees · peak size \(Fmt.num(story.peakSize, maxFraction: 6))")
                    .font(.ui(10)).foregroundStyle(.secondary)
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Brand.cardBorder))
    }

    /// «Sep 25, 11:47 PM → 11:48 PM»: the day once when both ends share it.
    private var span: String {
        guard let closed = story.closed else { return "\(Fmt.date(story.opened)) · open" }
        return "\(Fmt.date(story.opened)) → \(Calendar.current.isDate(story.opened, inSameDayAs: closed) ? Fmt.time(closed) : Fmt.date(closed))"
    }

    /// Fills and ihatecrypto's actions on this position, in time order, as bubbles.
    private var events: [AnyView] {
        var items: [(Date, AnyView)] = story.fills.map { f in
            let open = f.dir.hasPrefix("Open")
            return (f.time, AnyView(HistoryBubble(bubble: ActivityBubble(
                kind: open ? "position" : "result", label: f.dir, title: "\(Fmt.num(f.sz, maxFraction: 6)) at \(Fmt.price(f.px))",
                lines: [Fmt.date(f.time)] + (f.closedPnl != 0 ? [signed(f.closedPnl) + " · fee \(Fmt.usd(f.fee))"] : ["fee \(Fmt.usd(f.fee))"]),
                asset: open ? story.coin : "", long: story.long))))
        }
        items += activities.map { a in
            (a.at, AnyView(HistoryBubble(bubble: ActivityBubble(kind: "choice", label: a.kind == .automatic ? "Guardian" : a.request != nil ? "You asked" : "ihatecrypto",
                                                                 title: a.request ?? a.title ?? "", lines: Array(a.lines.suffix(1))))))
        }
        return items.sorted { $0.0 < $1.0 }.map(\.1)
    }
}

/// A page's top line on the canvas. The title and the way back always stay on one line; when the pane is narrow
/// the controls move under them, and scroll sideways if even that is too tight.
struct PageHeader<Controls: View>: View {
    let title: String
    let close: () -> Void
    @ViewBuilder let controls: Controls

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 12) {
                name
                controls
                Spacer(minLength: 12)
                back
            }
            VStack(alignment: .leading, spacing: 10) {
                HStack { name; Spacer(); back }
                // Room for a hovered icon button's lift, which the scroll view would clip; the row stays aligned.
                ScrollView(.horizontal, showsIndicators: false) { HStack(spacing: 10) { controls }.padding(3) }.padding(.horizontal, -3)
            }
        }
        .padding(.horizontal, 28).padding(.top, 22).padding(.bottom, 14)
    }

    private var name: some View { Text(title).font(.ui(22, .heavy)).lineLimit(1).fixedSize() }

    private var back: some View {
        Button(action: close) { Image(systemName: "xmark") }.buttonStyle(.hoverIcon).help("Back to the canvas")
    }
}

/// Views left to right, a new line when the next one does not fit.
struct Flow: Layout {
    var spacing: CGFloat = 10

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let frames = frames(subviews, width: proposal.width ?? .infinity)
        return CGSize(width: frames.map(\.maxX).max() ?? 0, height: frames.map(\.maxY).max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (view, frame) in zip(subviews, frames(subviews, width: bounds.width)) {
            view.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY), proposal: ProposedViewSize(frame.size))
        }
    }

    private func frames(_ subviews: Subviews, width: CGFloat) -> [CGRect] {
        var x: CGFloat = 0, y: CGFloat = 0, row: CGFloat = 0
        return subviews.map { view in
            var size = view.sizeThatFits(.unspecified)
            // Wider than the whole line: it gets the line and grows down instead.
            if size.width > width { size = view.sizeThatFits(ProposedViewSize(width: width, height: nil)) }
            if x > 0, x + size.width > width { x = 0; y += row + spacing; row = 0 }
            defer { x += size.width + spacing; row = max(row, size.height) }
            return CGRect(origin: CGPoint(x: x, y: y), size: size)
        }
    }
}

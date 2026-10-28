import AppKit
import IhatecryptoCore
import SwiftUI
import WalletCore

// MARK: What a step is

/// What a route step does, read from its label («BRIDGE · RELAY», «INTO · KAMINO LEND», «FROM · SOLANA»…): it picks
/// the drawing on top of its card. A trade plan's steps have their own: the rules' checklist, Jev's magnifier,
/// the price line the position rides, the shield of its stop and take, and the two ways it can end. A price alert the
/// plan waits for («WHEN · PRICE FALLS TO») draws the price reaching its level, with the bell; a task for later
/// («LATER · SCHEDULE») a calendar with its day marked; the best rates one amount branching into three vaults. The
/// steps that used to hide in a footer have theirs too: a recurring buy's loop, the wait for a bridge (an hourglass),
/// the exact approve (a key), gas that comes along (a pump), the simulation, the Touch ID signature, HLP's lockup, a
/// position closed at its flag, a top up landing in the tray, a rebalance's ring; rewards and predictions are drawn
/// for when the app has them.
enum StepKind: Equatable {
    case wallet, bridge, swap, buy, deposit, stake, send, check, jev, long, short, protect, result, schedule, split
    case recurring, wait, approve, gas, simulate, sign, lock, close, claim, receive, rebalance, predict
    case alert(up: Bool)

    init(_ step: ActivityBubble) {
        let label = step.label.uppercased()
        let place = label.components(separatedBy: "·").last?.trimmingCharacters(in: .whitespaces) ?? ""
        if label.hasPrefix("WHEN") { self = .alert(up: label.contains("RISES")) }
        else if label.hasPrefix("CHECK") { self = .check }
        else if label.hasPrefix("MULTISIG") { self = .lock }
        else if label.hasPrefix("JEV") || label.hasPrefix("SECOND") { self = .jev }
        else if label.hasPrefix("OPEN LONG") { self = .long }
        else if label.hasPrefix("OPEN SHORT") { self = .short }
        else if label.hasPrefix("PROTECT") { self = .protect }
        else if label.hasPrefix("RESULT") { self = .result }
        else if label.hasPrefix("LATER") { self = .schedule }
        else if label.contains("BEST RATES") { self = .split }
        else if label.hasPrefix("EVERY") { self = .recurring }
        else if label.hasPrefix("ARRIVES") { self = .wait }
        else if label.hasPrefix("APPROVE") { self = .approve }
        else if label.hasPrefix("GAS") { self = .gas }
        else if label.hasPrefix("SIMULATION") { self = .simulate }
        else if label.hasPrefix("SIGN ") || label.hasPrefix("SIGN·") { self = .sign }
        else if label.contains("LOCKED") { self = .lock }
        else if label.hasPrefix("CLOSE") { self = .close }
        else if label.hasPrefix("CLAIM") { self = .claim }
        else if label.hasPrefix("TOP UP") { self = .receive }
        else if label.hasPrefix("REBALANCE") { self = .rebalance }
        else if label.hasPrefix("PREDICTION") { self = .predict }
        else if label.contains("BRIDGE") || label.hasPrefix("RELAY") { self = .bridge }
        else if label.hasPrefix("SWAP") { self = .swap }
        else if label.hasPrefix("BUY") || label.hasPrefix("SELL") { self = .buy }
        else if label.contains("JITO") || label.contains("STAK") { self = .stake }
        else if label.hasPrefix("INTO") || ["AAVE", "MORPHO", "LEND", "VAULT"].contains(where: label.contains) { self = .deposit }
        else if label.hasPrefix("TO"), !Self.places.contains(where: place.hasPrefix) { self = .send }
        else { self = .wallet }
    }

    /// «TO · SOLANA» lands in one of the user's own places; «TO · TEST» is a saved address, someone else's or another wallet.
    private static let places = ["SOLANA", "BASE", "ARBITRUM", "HYPERLIQUID", "ROBINHOOD", "NEAR", "USDC", "USDG", "POLYGON", "ETHEREUM", "TEMPO", "HYPEREVM", "OPTIMISM", "BNB"]
}

/// Where a step is: waiting for Confirm, done, the one a running move is on (or waiting for Touch ID there), still
/// to come, or part of a move that failed; plain in the history, which shows a canvas as it was, with no state.
/// A plan's step can also be stopped by a rule, or want a look (a detail missing, Jev's doubt).
enum StepState: Equatable {
    case ready, done, now, touch, next, failed, plain, blocked, attention
    /// A price alert the plan waits for: watching the price, nothing to do yet.
    case watching

    /// The coin's face on the step's chip and in its drawing.
    var mood: BrandLogo.Mood {
        switch self {
        case .ready: .waiting
        case .done: .done
        case .now: .thinking
        case .touch: .tuning
        case .next: .idle
        case .failed, .blocked: .error
        case .plain: .idle
        case .attention: .tuning
        case .watching: .waiting
        }
    }
}

extension RouteFill {
    /// A place's color by its name, as the rates and the portfolio name it («Kamino Lend», «Aave on Base», «HLP»).
    static func place(_ name: String) -> RouteFill {
        let n = name.lowercased()
        let byName: [(String, RouteFill)] = [("kamino", .kamino), ("jupiter", .jupiter), ("jito", .jito), ("hlp", .hlp), ("hyperliquid", .hyperliquid),
                                             ("morpho", .morpho), ("aave", .aave), ("base", .base), ("arbitrum", .arbitrum), ("near", .near)]
        return byName.first { n.contains($0.0) }?.1 ?? .solana
    }

    /// The color on a faint card: the lines of a step still to come and the ring of the step a move is on. Solana's own
    /// tint is too pale for either; in dark mode a dark place (Hyperliquid, Relay, Kamino…) takes its light tint, or
    /// its lines and ring would vanish into the canvas.
    var accent: Color {
        switch self {
        case .solana: Color(hex: 0x8B6AE8)
        case .check: .secondary
        case .relay: Color(light: 0x4615C8, dark: 0x9C84FF)
        case .hyperliquid: Color(light: 0x072723, dark: 0x97FCE4)
        case .hlp: Color(light: 0x0D3B35, dark: 0x97FCE4)
        case .jupiter: Color(light: 0x131D27, dark: 0xC7F284)
        case .kamino: Color(light: 0x082A56, dark: 0x6FA0F0)
        case .jito: Color(light: 0x1F9D6A, dark: 0x4CD6A0)
        case .aave: Color(light: 0x2B2D3C, dark: 0xB8BBD9)
        case .morpho: Color(light: 0x1D4FC4, dark: 0x7FA2FF)
        case .base: Color(light: 0x0052FF, dark: 0x6E9BFF)
        case .arbitrum: Color(light: 0x213147, dark: 0x8FB4E3)
        case .robinhood: Color(light: 0x3A4A00, dark: 0xC9F24B)
        case .near: Color(light: 0x10231C, dark: 0x97FCE4)
        case .tempo: Color(light: 0x111111, dark: 0xE8E8E8)
        case .hyperevm: Color(light: 0x0E2B26, dark: 0x8FE3CC)
        }
    }
}

/// A card's colors, from its place and its state: a step still to come is its place's color, faint.
struct CardLook {
    let fill: RouteFill
    let state: StepState
    var kind: StepKind?

    var pale: Bool { state == .next }
    var ink: Color { pale || !fill.dark ? .primary : .white }
    var drawing: Color { pale ? fill.accent : fill.dark ? .white : .primary }
    var line: Color { ink.opacity(0.78) }
    var label: Color {
        if pale { return fill.accent }
        // A long reads green and a short red at a glance.
        if kind == .long { return profit }
        if kind == .short { return loss }
        switch fill {
        case .check: return .secondary
        case .solana: return Color(light: 0x6E5A9A, dark: 0xC9B8FF)
        case .jupiter: return Color(hex: 0xC7F284)
        case .hyperliquid, .hlp, .near: return Color(hex: 0x97FCE4)
        default: return .white.opacity(0.72)
        }
    }
    /// The ticker on a buy's price tag: Jupiter's lime on Jupiter, the ink elsewhere.
    var tag: Color { !pale && fill == .jupiter ? Color(hex: 0xC7F284) : drawing }
    /// The take's and a profit's green, the stop's and a loss's red: lighter on a dark card, which every card is in dark mode.
    var profit: Color { fill.dark && !pale ? Color(hex: 0x7EEBC0) : Color(light: 0x1F9D6A, dark: 0x7EEBC0) }
    var loss: Color { fill.dark && !pale ? Color(hex: 0xFF8A7A) : Color(light: 0xD9443A, dark: 0xFF8A7A) }
    /// Filled shapes in the drawing (a wallet, a vault, a tag) are the card's own color, so the lines read on it; on a
    /// check's grey card they are paper.
    var body: Color { pale || fill == .check ? Brand.card : fill.color }

    @ViewBuilder var background: some View {
        // Square: the card clips it to its own radius (14 small, 20 big), which the border follows.
        let shape = Rectangle()
        if pale {
            ZStack { shape.fill(Brand.card); shape.fill(fill.accent.opacity(0.16)) }
        } else {
            shape.fill(fill.color)
        }
    }
}

// MARK: The route

/// A money move's route, or a trade plan's: one card per step in its place's color, arrows between them, rows of three
/// that wrap with an arrow back to the start of the next row; a longer route is the same cards smaller, four to a row.
/// Each card says its state in words on its picture, opens its details on a click and has Why / Edit / Send to chat /
/// Copy on a right click. A new route is laid out by the assistant's coin cursor, card by card; the cards fade in
/// where they stand and never move.
struct RouteCards: View {
    @Environment(Workspace.self) private var ws
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let steps: [ActivityBubble]
    /// The step a running move is on: those before it are done, those after it wait.
    var current: Int?
    /// A move carried out (true) or failed (false); nil while it waits or runs.
    var outcome: Bool?
    /// A board shown as it was, its move gone stale: no state on its cards, only Refresh above them.
    var asItWas = false
    /// The route's own note (what's signed, how it's checked): «Why this way?» quotes it.
    var footer: String?
    /// The move waiting for Confirm: its amount, and the way of a Base or Arbitrum trade, change in a step's details.
    var pending: WalletCommand?
    /// Laid out by the coin cursor when it first shows.
    var build = false
    /// What a new layout follows; by default the titles. A plan's titles carry live numbers (the bridge's quote), so
    /// it passes the plan itself: the cards are laid out again when the plan changes, not when a price does.
    var buildKey: String?

    /// The picture on top, its words under it (the user's pick of the card designs, 2026-10-09).
    static let cardWidth: CGFloat = 236, cardHeight: CGFloat = 292, gap: CGFloat = 40, perRow = 3
    /// Four to a row: a plan of six or seven cards, the portfolio beside it, fits the window with its buttons in view.
    static let smallWidth: CGFloat = 220, smallHeight: CGFloat = 272, smallGap: CGFloat = 26, smallPerRow = 4
    /// Over three steps the cards are the small ones.
    static func compact(_ count: Int) -> Bool { count > perRow }
    /// What the route takes across: a row of cards and the arrows between them.
    static func width(_ count: Int) -> CGFloat {
        let small = compact(count)
        let n = CGFloat(min(max(count, 1), small ? smallPerRow : perRow))
        return n * (small ? smallWidth : cardWidth) + (n - 1) * (small ? smallGap : gap)
    }

    /// How many cards the cursor has put down; nil before it starts. A route it builds shows none until then (no flash
    /// of the whole route first), any other all of them.
    @State private var putDown: Int?
    private var placed: Int { putDown ?? (build && !reduceMotion ? 0 : .max) }
    @State private var frames: [Int: CGRect] = [:]
    @State private var open: Int?

    var body: some View {
        let small = Self.compact(steps.count), perRow = small ? Self.smallPerRow : Self.perRow
        let rows = stride(from: 0, to: steps.count, by: perRow).map { Array($0..<min($0 + perRow, steps.count)) }
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: small ? 36 : 46) {
                ForEach(rows, id: \.self) { row in
                    HStack(spacing: 0) {
                        ForEach(row, id: \.self) { i in
                            if i != row.first { StepArrow(solid: solid(into: i), width: small ? Self.smallGap : Self.gap).opacity(i < placed ? 1 : 0) }
                            card(i)
                        }
                    }
                }
            }
            .background(alignment: .topLeading) {
                RowTurns(rows: rows, frames: frames, placed: placed, solid: rows.map { solid(into: $0.first ?? 0) })
            }
            .overlay(alignment: .topLeading) { cursor }
            if let i = open, steps.indices.contains(i) {
                StepDetails(step: steps[i], index: i, count: steps.count, state: state(i), footer: footer, pending: editable(i) ? pending : nil,
                            wayKey: wayKey(i), hint: steps.lazy.flatMap(\.lines).first { $0.hasPrefix("free") },
                            notch: (frames[i]?.midX ?? 40), close: { withAnimation(.snappy(duration: 0.2)) { open = nil } },
                            ask: { sendToChat(i) },
                            // Round the ring: right from the last step is the first, left from the first the last.
                            go: { d in withAnimation(.snappy(duration: 0.2)) { open = (i + d + steps.count) % steps.count } })
                    // Each step its own details: an amount typed for one doesn't carry to the next.
                    .id(i)
                    .transition(.opacity)
            }
        }
        .coordinateSpace(.named("route"))
        .preference(key: RouteWidth.self, value: Self.width(steps.count))
        .task(id: build ? buildKey ?? steps.map(\.title).joined(separator: "|") : "") { await assemble() }
    }

    private func card(_ i: Int) -> some View {
        RouteCard(step: steps[i], state: state(i), isOpen: open == i, compact: Self.compact(steps.count), opens: true)
            .opacity(i < placed ? 1 : 0)
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("route")) } action: { frames[i] = $0 }
            .onTapGesture { toggle(i) }
            // The same as the click and the right-click menu, for VoiceOver and the keyboard.
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { toggle(i) }
            .accessibilityAction(named: "Why this way?") { why(i) }
            .accessibilityAction(named: "Send to chat") { sendToChat(i) }
            .accessibilityAction(named: "Copy details") { copy(i) }
            .contextMenu {
                // macOS drops menu icons unless asked; the coin marks the step going to the chat.
                Group {
                    Button { why(i) } label: { Label("Why this way?", systemImage: "questionmark.circle") }
                    Button { withAnimation(.snappy(duration: 0.2)) { open = i } } label: { Label("Edit step…", systemImage: "pencil") }
                        .disabled(!editable(i))
                    Button { sendToChat(i) } label: { Label { Text("Send to chat") } icon: { Image(nsImage: CoinIcon.image) } }
                    Divider()
                    Button { copy(i) } label: { Label("Copy details", systemImage: "doc.on.doc") }
                }
                .labelStyle(.titleAndIcon)
            }
    }

    // MARK: State

    private func state(_ i: Int) -> StepState {
        if let outcome { return outcome ? .done : .failed }
        if asItWas { return .plain }
        if let current { return i < current ? .done : i > current ? .next : SignerActivity.shared.atDevice ? .touch : .now }
        // A plan's step says its own: a rule stops it, it wants a look, or a check has passed.
        switch steps[i].status {
        case "blocked": return .blocked
        case "attention": return .attention
        case "watching": return .watching
        case "passed": return .done
        // A step of the same chain that runs after this move (Workspace.chainSteps): pale, dotted, not edited here.
        case "then": return .next
        default: return .ready
        }
    }

    /// The arrow into a step: solid along the way the money goes or went, dotted to a step still to come.
    private func solid(into i: Int) -> Bool { state(i) != .next }

    /// Only a move waiting for Confirm changes, and only where there's something to change: its amount, or the way.
    private func editable(_ i: Int) -> Bool {
        guard let pending, state(i) == .ready else { return false }
        return pending.editableAmount != nil || wayKey(i) != nil
    }

    /// The step of a Base or Arbitrum trade that is Relay's swap or Relay's bridge + Uniswap: its way can be picked.
    private func wayKey(_ i: Int) -> String? {
        guard StepKind(steps[i]) == .bridge, steps[i].label.uppercased().hasPrefix("RELAY") else { return nil }
        return pending?.wayKey
    }

    // MARK: Actions

    private func toggle(_ i: Int) { withAnimation(.snappy(duration: 0.2)) { open = open == i ? nil : i } }

    private func name(_ i: Int) -> String {
        "Step \(i + 1) · \(StepDetails.pretty(steps[i].label)) · \(ws.readable(steps[i].title))"
    }

    /// The answer from the step itself and the route's own note: nothing goes to the model, and a move waiting for
    /// Confirm keeps waiting (a message in the chat would cancel it).
    private func why(_ i: Int) {
        let s = steps[i]
        let facts = ([ws.readable(s.title)] + (s.money ?? []).map(MoneyLine.text) + s.lines.filter { !$0.isEmpty }.map(\.rowText)).joined(separator: " · ")
        ws.reply(ws.say("Why this way — \(StepDetails.pretty(s.label)): \(facts).", "Почему так — \(StepDetails.pretty(s.label)): \(facts).")
                 + (footer.map { " " + $0 } ?? ""), note: "from the route")
    }

    private func sendToChat(_ i: Int) {
        // With the step's money and rows, as «Copy details» has them: it went by its name alone before.
        let s = steps[i]
        let facts = ([ws.readable(s.title)] + (s.money ?? []).map(MoneyLine.text) + s.lines.filter { !$0.isEmpty }.map(\.rowText)).joined(separator: " · ")
        ws.attach(StepContext(title: name(i), logo: s.logo, about: "About the step «\(name(i))» of the route on the canvas (\(facts))"))
        withAnimation(.snappy(duration: 0.2)) { open = nil }
    }

    private func copy(_ i: Int) {
        let s = steps[i]
        let text = ([name(i)] + (s.money ?? []).map(MoneyLine.text) + s.lines.filter { !$0.isEmpty }.map(\.rowText) + (footer.map { [$0] } ?? [])).joined(separator: "\n")
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    // MARK: The coin cursor

    @ViewBuilder private var cursor: some View {
        if placed > 0, placed <= steps.count, let f = frames[placed - 1] {
            CoinCursor()
                .offset(x: f.maxX - 30, y: f.maxY - 26)
                .animation(.easeInOut(duration: 0.38), value: placed)
                .transition(.opacity)
        }
    }

    /// The cursor puts the cards down one by one; the chat's coin says «building» meanwhile. Without motion, for a route
    /// that is only shown again, or one of more than six cards, they are all there at once: the motion redraws the whole
    /// window every frame, and thirty cards kept it going for thirteen seconds.
    private func assemble() async {
        guard build, !reduceMotion, !steps.isEmpty, steps.count <= 6 else { putDown = .max; return }
        putDown = 0
        for i in 1...steps.count {
            try? await Task.sleep(for: .milliseconds(i == 1 ? 200 : 420))
            if Task.isCancelled { break }
            withAnimation(.easeOut(duration: 0.25)) { putDown = i }
            if ws.assembly == nil || ws.assembly?.total == steps.count { ws.assembly = (i, steps.count) }
        }
        if !Task.isCancelled { try? await Task.sleep(for: .milliseconds(550)) }
        withAnimation(.easeOut(duration: 0.25)) { putDown = .max }
        if ws.assembly?.total == steps.count { ws.assembly = nil }
    }
}

/// The assistant's cursor while it lays out a route: the arrow with the coin riding on it.
struct CoinCursor: View {
    /// The coin's face: building the route by default; the first screen's demo changes it as it goes.
    var mood: BrandLogo.Mood = .thinking

    var body: some View {
        ZStack(alignment: .topLeading) {
            Image(systemName: "cursorarrow").font(.system(size: 22, weight: .bold)).foregroundStyle(Brand.accent)
                .shadow(color: .white, radius: 0.5)
            BrandLogo(size: 30, mood: mood)
                .shadow(color: .black.opacity(0.18), radius: 3, y: 1)
                .offset(x: 14, y: 16)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// The coin as a menu icon: «Send to chat» hands the step to the assistant.
@MainActor enum CoinIcon {
    static let image: NSImage = {
        let renderer = ImageRenderer(content: BrandLogo(size: 16))
        renderer.scale = 2
        return renderer.nsImage ?? NSImage()
    }()
}

// MARK: A card

struct RouteCard: View {
    @Environment(Workspace.self) private var ws
    let step: ActivityBubble
    let state: StepState
    var isOpen = false
    /// The history draws the same card narrower.
    var width = RouteCards.cardWidth
    /// A long route's small card: the same parts a size smaller, fewer of its numbers; the rest opens on a click.
    var compact = false
    /// A route's card opens its details on a click: its last line says so («Details ›», «Close ×» while open).
    var opens = false
    /// The tooltip; the block library says what a pick does instead.
    var hint = "Details · right-click for more"
    @State private var hover = false

    var body: some View {
        let kind = StepKind(step)
        let look = CardLook(fill: step.fill ?? .solana, state: state, kind: kind)
        let w = compact ? RouteCards.smallWidth : width
        let radius: CGFloat = compact ? 16 : 20
        // The drawing's 340 × 120 grid, with a strip above it for the state.
        let k = w / 340
        VStack(alignment: .leading, spacing: 0) {
            // The picture: what the step does, the coin in it wearing the step's face, and the state in words on it —
            // the first thing the eye finds. Nothing in the other corner.
            ZStack(alignment: .topLeading) {
                look.background
                StepScene(kind: kind, look: look, state: state, ticker: ticker)
                    .frame(width: w, height: 120 * k)
                    .offset(y: 10 * k)
                    .opacity(look.pale ? 0.55 : 1)
                // What the result comes to is an estimate, not a step with a state.
                if state != .plain, kind != .result { StateChip(state: state, fill: look.fill, compact: compact).padding(compact ? 8 : 10) }
            }
            .frame(width: w, height: 140 * k, alignment: .topLeading)
            .clipped()
            info(kind)
        }
        .frame(width: w, height: compact ? RouteCards.smallHeight : RouteCards.cardHeight * width / RouteCards.cardWidth, alignment: .topLeading)
        .background(Brand.card)
        .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        .overlay { edges(look, kind: kind, radius: radius) }
        .contentShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        .onHover { hover = $0 }
        .pointerStyle(.link)
        .help(hint)
        .accessibilityElement(children: .combine)
    }

    /// Under the picture: what the step does and with what («BRIDGE · Relay»), the move itself, its numbers — the
    /// balances it changes, now → after (was → now once done), or what it checks — and one line of the rest.
    private func info(_ kind: StepKind) -> some View {
        let parts = step.label.components(separatedBy: "·").map { $0.trimmingCharacters(in: .whitespaces) }
        let place = parts.count > 1 ? Self.pretty(parts.dropFirst().joined(separator: " · ")) : nil
        let money = step.money ?? []
        let rows = step.lines.filter { $0.contains("\t") }
        // «free: 29.39 USDC» says again what the money box shows.
        let notes = step.lines.filter { line in
            !line.isEmpty && !line.contains("\t") && (money.isEmpty || !(["free:", "held:", "in the vault:"].contains(where: line.hasPrefix)
                || line.wholeMatch(of: /≈ [\d.,]+ \S+/) != nil))
        }
        // A step with no numbers says itself in the box; one with numbers keeps its first sentence for the last line.
        let said = money.isEmpty && rows.isEmpty ? notes : []
        let foot = said.isEmpty ? notes.first : nil
        let paper = CardLook(fill: .check, state: .ready, kind: kind)
        return VStack(alignment: .leading, spacing: compact ? 6 : 8) {
            HStack(spacing: 6) {
                Text(parts.first ?? step.label).font(.ui(compact ? 9.5 : 10, .heavy)).tracking(0.7).foregroundStyle(verbInk(kind))
                if let place {
                    Circle().fill(Color.secondary.opacity(0.6)).frame(width: 3, height: 3)
                    LogoBadge(step: step, size: compact ? 16 : 18)
                    Text(place).font(.ui(compact ? 11 : 11.5, .semibold)).foregroundStyle(.primary)
                }
            }
            .lineLimit(1)
            StepTitle(step: step, ink: .primary, size: compact ? 17 : 19)
            if !money.isEmpty || !rows.isEmpty || !said.isEmpty {
                // As many of its numbers as fit; the rest open with a click.
                ViewThatFits(in: .vertical) {
                    ForEach(Array(stride(from: compact ? 3 : 4, through: 1, by: -1)), id: \.self) { n in
                        box(money: money, rows: rows, said: said, upTo: n, paper: paper)
                    }
                }
            }
            Spacer(minLength: 0)
            footer(foot)
        }
        .padding(.horizontal, compact ? 12 : 14).padding(.top, compact ? 9 : 11).padding(.bottom, compact ? 8 : 10)
    }

    private func box(money: [MoneyMove], rows: [String], said: [String], upTo n: Int, paper: CardLook) -> some View {
        let shown = Array(money.prefix(n)), left = max(0, n - shown.count)
        return VStack(alignment: .leading, spacing: 4) {
            if !shown.isEmpty {
                Text(state == .done ? "MONEY · WAS → NOW" : "MONEY · NOW → AFTER").font(.ui(9, .bold)).tracking(0.6).foregroundStyle(.secondary)
                ForEach(shown) { MoneyLine(move: $0, size: compact ? 11 : 11.5) }
            }
            ForEach(Array((rows + said).prefix(left).enumerated()), id: \.offset) { i, line in
                // What stops the step comes first, in red.
                StepLine(line: line, look: paper, size: compact ? 10.5 : 11, alert: state == .blocked && i == 0 && shown.isEmpty)
            }
        }
        .padding(.horizontal, 10).padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color.primary.opacity(0.045)))
    }

    private func footer(_ note: String?) -> some View {
        VStack(spacing: 0) {
            Rectangle().fill(Color.primary.opacity(0.08)).frame(height: 1)
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                if let note {
                    Text(note).font(.ui(compact ? 10 : 10.5)).foregroundStyle(state == .blocked ? Brand.loss : .secondary).lineLimit(compact ? 1 : 2)
                }
                Spacer(minLength: 4)
                if opens {
                    HStack(spacing: 3) {
                        Text(isOpen ? "Close" : "Details").font(.ui(10.5, .semibold))
                        Image(systemName: isOpen ? "xmark" : "chevron.right").font(.system(size: 8, weight: .bold))
                    }
                    .foregroundStyle(isOpen ? Brand.accentInk : .secondary)
                    .fixedSize()
                }
            }
            .padding(.top, 6)
        }
    }

    /// The action's word: a long green and a short red at a glance; a step still to come faint.
    private func verbInk(_ kind: StepKind) -> Color {
        if state == .next { return .secondary }
        if kind == .long { return Color(light: 0x1F9D6A, dark: 0x7EEBC0) }
        if kind == .short { return Color(light: 0xD9443A, dark: 0xFF8A7A) }
        return .primary
    }

    /// «KAMINO LEND» → «Kamino Lend», «MERKL ON BASE» → «Merkl on Base», «TOUCH ID» → «Touch ID»: short words and
    /// tickers keep their capitals.
    static func pretty(_ place: String) -> String {
        let small: Set = ["ON", "OF", "TO", "OR", "AND", "IN", "AT", "FOR", "WITH", "FROM"]
        let kept = ["USDC": "USDC", "USDG": "USDG", "USDT": "USDT", "NEAR": "NEAR", "JITOSOL": "JitoSOL", "1CLICK": "1Click", "HYPEREVM": "HyperEVM", "LAYERZERO": "LayerZero", "CCTP": "CCTP", "KYBERSWAP": "KyberSwap"]
        return place.split(separator: " ").enumerated().map { i, w in
            let word = String(w)
            if let k = kept[word] { return k }
            if small.contains(word) { return i > 0 ? word.lowercased() : word.prefix(1) + word.dropFirst().lowercased() }
            if word.count <= 3 || word.contains(where: \.isNumber) || !word.contains(where: \.isLetter) { return word }
            return word.prefix(1) + word.dropFirst().lowercased()
        }
        .joined(separator: " ")
    }

    /// The step's name on its price tag, and what its title's first token is.
    private var ticker: String? {
        ws.readable(step.title).components(separatedBy: " → ").last?.split(separator: " ").last.map(String.init)
    }

    /// Only the border says more: dashed for a step still to come and for the result (an estimate), red for a step that
    /// failed or a rule stops, amber for one that wants a look, a ring for the one a move is on (breathing, the only
    /// thing on the route that moves) or whose details are open, the accent edge under the pointer.
    @ViewBuilder private func edges(_ look: CardLook, kind: StepKind, radius: CGFloat) -> some View {
        let card = RoundedRectangle(cornerRadius: radius, style: .continuous)
        ZStack {
            if look.pale { card.strokeBorder(look.fill.accent.opacity(0.7), style: StrokeStyle(lineWidth: 1.5, dash: [5, 4])) }
            else if kind == .result { card.strokeBorder(Color.secondary.opacity(0.55), style: StrokeStyle(lineWidth: 1.5, dash: [5, 4])) }
            else if look.fill == .check { card.strokeBorder(Brand.cardBorder, lineWidth: 1.5) }
            if state == .failed || state == .blocked { card.strokeBorder(Brand.loss, lineWidth: 2) }
            if state == .attention { card.strokeBorder(Color.orange, lineWidth: 2) }
            if isOpen {
                // Right on the card's edge.
                card.strokeBorder(Brand.accent, lineWidth: 3)
            } else if state == .now || state == .touch {
                RingPulse(color: state == .touch ? .primary : look.fill.accent, radius: radius)
            }
            // At once, not faded: each frame of a fade redraws the whole window.
            card.strokeBorder(Brand.accent, lineWidth: 2).opacity(hover && !isOpen ? 0.7 : 0)
        }
        .allowsHitTesting(false)
    }
}

/// The ring of the step a move is on: its opacity breathes, nothing else changes, twelve times a second and only while
/// the window is in front. A repeatForever redrew the whole window every display frame (120 a second); Core Animation's
/// BreathingBorder can't go here: an AppKit view moving with the canvas makes AppKit work out the window's drag regions
/// again on every step of a pan, asking SwiftUI's focus system each time (a quarter of a pan's main thread, measured).
/// On the card's own edge and radius: 5 pt outside it, at a fixed 25, it pulsed past the border.
private struct RingPulse: View {
    let color: Color
    let radius: CGFloat
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.appearsActive) private var active

    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 12, paused: reduceMotion || !active)) { timeline in
            // 0.9 s each way, as before: 0.35 … 1.
            let t = timeline.date.timeIntervalSinceReferenceDate
            RoundedRectangle(cornerRadius: radius, style: .continuous).strokeBorder(color, lineWidth: 2.5)
                .opacity(reduceMotion ? 1 : 0.675 + 0.325 * sin(t * .pi / 0.9))
        }
    }
}

/// The step's logo — its network, token or protocol — with the other protocol the step goes through as a small badge
/// on its bottom right (Base with Uniswap's, a token with Jupiter's), the way an app icon wears a badge.
struct LogoBadge: View {
    @Environment(Workspace.self) private var ws
    let step: ActivityBubble
    var size: CGFloat = 36

    var body: some View {
        let second = Self.second(step)
        logo.frame(width: size, height: size)
            .overlay(alignment: .bottomTrailing) {
                if let second {
                    TokenLogo(name: second, size: max(10, size * 0.62), round: true)
                        .padding(max(1, size * 0.06))
                        .background(Circle().fill(Brand.card))
                        .overlay(Circle().strokeBorder(Color.primary.opacity(0.12), lineWidth: 0.5))
                        .offset(x: size * 0.38, y: size * 0.28)
                }
            }
            // Room for the badge, so the word after it doesn't run under it.
            .padding(.trailing, second == nil ? 0 : size * 0.38)
    }

    /// The other protocol the label names («BUY · UNISWAP ON BASE», «RELAY + UNISWAP», «SELL · JUPITER»), when it isn't
    /// the step's own logo.
    static func second(_ step: ActivityBubble) -> String? {
        let label = step.label.uppercased()
        let protocols = [("UNISWAP", "uni"), ("KYBERSWAP", "kyber"), ("RELAY", "relay"), ("JUPITER", "jup"), ("NEAR INTENTS", "near"), ("1CLICK", "near"),
                         ("KAMINO", "kamino"), ("AAVE", "aave"), ("MORPHO", "morpho"), ("JITO", "jitosol")]
        return protocols.first { label.contains($0.0) && $0.1 != step.logo }?.1
    }

    @ViewBuilder private var logo: some View {
        let inner = size
        if let logo = step.logo, logo.hasPrefix("stock:") {
            let symbol = String(logo.dropFirst(6))
            StockLogo(symbol: symbol, url: ws.robinhood?.stocks.first { $0.symbol == symbol }?.logo, size: inner)
        } else if let logo = step.logo {
            TokenLogo(name: logo, size: inner)
        }
    }
}

/// One of a card's lines. «entry\t≈ 119.31» is a row, the label quiet on the left and the value bold on the right, red
/// for the stop and a loss, green for the take and a profit, so a card reads at a glance; anything else is a sentence,
/// red when it's what stops the step.
struct StepLine: View {
    let line: String
    let look: CardLook
    var size: CGFloat = 11.5
    var alert = false

    var body: some View {
        let parts = line.split(separator: "\t", maxSplits: 1).map(String.init)
        if parts.count == 2 {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(parts[0]).font(.ui(size)).foregroundStyle(look.line).lineLimit(1)
                Spacer(minLength: 4)
                Text(parts[1]).font(.ui(size, .semibold)).monospacedDigit().foregroundStyle(tint(parts[0], parts[1]) ?? look.ink)
                    .lineLimit(1).minimumScaleFactor(0.7)
            }
        } else {
            Text(line).font(.ui(size, alert ? .semibold : .regular)).foregroundStyle(alert ? look.loss : look.line)
                .lineLimit(3).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func tint(_ label: String, _ value: String) -> Color? {
        if label.contains("stop") || value.hasPrefix("−") { return look.loss }
        if label.contains("take") || value.hasPrefix("+") { return look.profit }
        return nil
    }
}

/// A balance a step changes: where, what it is now, an arrow, what it comes to (bold), the ticker.
struct MoneyLine: View {
    let move: MoneyMove
    var size: CGFloat = 11.5

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 5) {
            Text(move.place).foregroundStyle(.secondary).layoutPriority(-1)
            Spacer(minLength: 4)
            Text(Self.amount(move.was, move.symbol)).foregroundStyle(.secondary)
            Text("→").foregroundStyle(.tertiary)
            Text((move.approx ? "≈ " : "") + Self.amount(move.after, move.symbol)).fontWeight(.bold)
            Text(move.symbol).font(.ui(size - 1.5)).foregroundStyle(.secondary)
        }
        .font(.ui(size)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.75)
    }

    /// Dollars to the cent; another coin to 4 places from 1 up (2.4722 AERO), 6 under it (0.074852 SOL).
    static func amount(_ d: Decimal, _ symbol: String) -> String {
        let size = abs(d)
        return Fmt.num(d, maxFraction: ["USDC", "USDG", "USDT", "USDC.e", "pathUSD"].contains(symbol) || size >= 100 ? 2 : size >= 1 ? 4 : 6)
    }

    /// «Solana 29.39 → 27.39 USDC», for Copy details and Why this way.
    static func text(_ m: MoneyMove) -> String {
        "\(m.place) \(amount(m.was, m.symbol)) → \(m.approx ? "≈ " : "")\(amount(m.after, m.symbol)) \(m.symbol)"
    }
}

extension String {
    /// A card row «entry\t≈ 119.31» as one phrase: «entry ≈ 119.31».
    var rowText: String { replacingOccurrences(of: "\t", with: " ") }
}

/// «2 USDC → Base»: each side with its token's or place's logo when there's one.
struct StepTitle: View {
    @Environment(Workspace.self) private var ws
    let step: ActivityBubble
    let ink: Color
    var size: CGFloat = 19

    var body: some View {
        // «→ Hyperliquid» starts with its arrow: an empty first part.
        let parts = ws.readable(step.title).components(separatedBy: "→").map { $0.trimmingCharacters(in: .whitespaces) }
        HStack(spacing: size * 0.3) {
            ForEach(parts.indices, id: \.self) { i in
                if i > 0 { Text("→") }
                if !parts[i].isEmpty {
                    icon(parts[i])
                    Text(parts[i])
                }
            }
        }
        .font(.ui(size, .bold)).monospacedDigit().foregroundStyle(ink)
        .lineLimit(1).minimumScaleFactor(0.55)
    }

    @ViewBuilder private func icon(_ part: String) -> some View {
        if let contract = step.title.firstMatch(of: /0x[0-9a-fA-F]{40}/).map({ String($0.0) }), let token = ws.favoriteToken(contract) {
            TokenIcon(token: token, size: size + 1)
        } else if let name = Self.logo(part) {
            TokenLogo(name: name, size: size + 1, round: true)
        }
    }

    /// The bundled logo a title's last word names: a ticker (USDC, AERO, SOL) or a place (Base, Solana).
    static func logo(_ part: String) -> String? {
        guard let word = part.split(separator: " ").last.map({ String($0).lowercased() }), word.first?.isLetter == true else { return nil }
        let name = aliases[word] ?? word
        return TokenLogo.image(name) != nil ? name : nil
    }

    private static let aliases = ["solana": "sol", "hyperliquid": "hype", "arbitrum": "arb", "jitosol": "jitosol"]
}

/// The step's state on its chip, with the coin's face for it.
struct StateChip: View {
    let state: StepState
    let fill: RouteFill
    /// A small card's chip: the same coin and word, a size smaller (the coin alone didn't say what it meant).
    var compact = false

    var body: some View {
        let (text, ink, halo) = look
        HStack(spacing: compact ? 4 : 5) {
            ZStack {
                Circle().fill(halo)
                BrandLogo(size: compact ? 13 : 15, mood: state.mood)
            }
            .frame(width: compact ? 17 : 19, height: compact ? 17 : 19)
            Text(text).font(.ui(compact ? 10.5 : 11, .bold)).lineLimit(1).fixedSize()
        }
        .padding(.leading, 3).padding(.trailing, compact ? 8 : 9).padding(.vertical, 3)
        .foregroundStyle(ink)
        .background(Capsule().fill(state == .touch ? Color.black : Color.white))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(text)
    }

    private var look: (String, Color, Color) {
        switch state {
        case .ready: ("Ready", Color(hex: 0x7A5B00), Color(hex: 0xFFD23F))
        case .done: ("Done", Color(hex: 0x177A52), Color(hex: 0x7EEBC0))
        case .now: ("Now", fill == .solana ? Color(hex: 0x6A4FD0) : fill.color, Color(hex: 0xB7A6FF))
        case .touch: (SignerActivity.shared.ledger != nil ? "Ledger" : Presence.signShort, .white, Color(hex: 0xFFD23F))
        case .next: ("Next", Color(hex: 0x6A6A72), Color(hex: 0xD9D9DE))
        case .failed: ("Failed", Color(hex: 0xB42A20), Color(hex: 0xFFB3A8))
        case .blocked: ("Stopped", Color(hex: 0xB42A20), Color(hex: 0xFFB3A8))
        case .attention: ("Look", Color(hex: 0x8A4B00), Color(hex: 0xFFB870))
        case .watching: ("Watching", Color(hex: 0x8A4B00), Color(hex: 0xFFD9A8))
        case .plain: ("", .primary, .clear)
        }
    }
}

// MARK: Arrows

/// The arrow from one card to the next: solid where the money goes or went, dotted to a step still to come.
struct StepArrow: View {
    let solid: Bool
    var width = RouteCards.gap

    var body: some View {
        Canvas { context, size in
            let y = size.height / 2
            var shaft = Path(), head = Path()
            shaft.move(to: CGPoint(x: 7, y: y)); shaft.addLine(to: CGPoint(x: size.width - 8, y: y))
            head.move(to: CGPoint(x: size.width - 14, y: y - 6)); head.addLine(to: CGPoint(x: size.width - 8, y: y))
            head.addLine(to: CGPoint(x: size.width - 14, y: y + 6))
            let ink = GraphicsContext.Shading.color(solid ? Color.primary.opacity(0.85) : Color.secondary.opacity(0.7))
            context.stroke(shaft, with: ink, style: StrokeStyle(lineWidth: 2.2, lineCap: .round, dash: solid ? [] : [2, 5]))
            context.stroke(head, with: ink, style: StrokeStyle(lineWidth: 2.2, lineCap: .round, lineJoin: .round))
        }
        .frame(width: width, height: 18)
        .accessibilityHidden(true)
    }
}

/// A long route's turn to the next row: down from the row's last card, back under it, down into the next row's first.
private struct RowTurns: View {
    let rows: [[Int]]
    let frames: [Int: CGRect]
    let placed: Int
    let solid: [Bool]

    var body: some View {
        Canvas { context, _ in
            for r in rows.indices.dropFirst() {
                guard let last = rows[r - 1].last, let first = rows[r].first, first < placed,
                      let a = frames[last], let b = frames[first] else { continue }
                let mid = (a.maxY + b.minY) / 2
                var path = Path(), head = Path()
                path.move(to: CGPoint(x: a.midX, y: a.maxY + 6))
                path.addLine(to: CGPoint(x: a.midX, y: mid))
                path.addLine(to: CGPoint(x: b.midX, y: mid))
                path.addLine(to: CGPoint(x: b.midX, y: b.minY - 7))
                head.move(to: CGPoint(x: b.midX - 6, y: b.minY - 13)); head.addLine(to: CGPoint(x: b.midX, y: b.minY - 7))
                head.addLine(to: CGPoint(x: b.midX + 6, y: b.minY - 13))
                let ink = GraphicsContext.Shading.color(solid[r] ? Color.primary.opacity(0.85) : Color.secondary.opacity(0.7))
                context.stroke(path, with: ink, style: StrokeStyle(lineWidth: 2.2, lineCap: .round, lineJoin: .round, dash: solid[r] ? [] : [2, 5]))
                context.stroke(head, with: ink, style: StrokeStyle(lineWidth: 2.2, lineCap: .round, lineJoin: .round))
            }
        }
        .allowsHitTesting(false)
    }
}

// MARK: The drawing

/// The top of a card: what the step does, drawn in the card's ink on a 340 × 120 grid (the brand board's), with the
/// coin in it wearing the step's face.
struct StepScene: View {
    let kind: StepKind
    let look: CardLook
    let state: StepState
    let ticker: String?

    var body: some View {
        GeometryReader { geo in
            let k = geo.size.width / 340
            let (x, y, scale) = coin
            ZStack(alignment: .topLeading) {
                Canvas { context, _ in draw(&context, k: k, front: false) }
                BrandLogo(size: 34 * scale * 1.1 * k, mood: state.mood)
                    .position(x: x * k, y: y * k)
                Canvas { context, _ in draw(&context, k: k, front: true) }
            }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    /// Where the coin sits in the drawing, and how big (its radius over 17). Clear of the state on the picture's top
    /// left: none starts above y 60 left of x 150.
    private var coin: (CGFloat, CGFloat, CGFloat) {
        switch kind {
        case .wallet: (236, 50, 1.5)
        case .bridge: (state == .done ? 292 : state == .ready || state == .next ? 150 : 200, 35, 1.4)
        case .buy: (150, 62, 1.4)
        case .swap: (96, 68, 1.4)
        case .deposit: (170, 52, 1.4)
        case .stake: (170, 44, 1.4)
        case .send: (80, 72, 1.4)
        case .check: (130, 66, 1.4)
        case .jev: (120, 66, 1.4)
        case .long: (250, 35, 1.4)
        case .short: (250, 57, 1.4)
        case .protect: (150, 64, 1.4)
        case .result: (90, 68, 1.4)
        case .alert(let up): (236, up ? 58 : 74, 1.4)
        case .schedule: (130, 66, 1.4)
        case .split: (160, 36, 1.4)
        case .recurring: (170, 62, 1.2)
        case .approve: (140, 60, 1.4)
        case .simulate, .predict: (118, 66, 1.4)
        case .close: (212, 36, 1.3)
        case .rebalance: (130, 70, 1.4)
        case .wait, .gas, .sign, .lock, .claim, .receive: (130, 66, 1.4)
        }
    }

    private func draw(_ context: inout GraphicsContext, k: CGFloat, front: Bool) {
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: x * k, y: y * k) }
        let ink = GraphicsContext.Shading.color(look.drawing)
        // A small card's lines are a little heavier, as on the brand board, so they still read.
        let line = StrokeStyle(lineWidth: (k < 0.7 ? 3.5 : 3) * k, lineCap: .round, lineJoin: .round)
        let thin = StrokeStyle(lineWidth: 1.6 * k, lineCap: .round)
        let fill = GraphicsContext.Shading.color(look.body)
        func stroke(_ path: Path, _ style: StrokeStyle? = nil, opacity: Double = 1) {
            context.stroke(path, with: .color(look.drawing.opacity(opacity)), style: style ?? line)
        }
        func circle(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat) -> Path { Path(ellipseIn: CGRect(x: (x - r) * k, y: (y - r) * k, width: 2 * r * k, height: 2 * r * k)) }
        let good = look.profit, bad = look.loss
        switch (kind, front) {
        case (.wallet, true):
            var wallet = Path()
            wallet.move(to: p(150, 140)); wallet.addLine(to: p(150, 76))
            wallet.addQuadCurve(to: p(164, 62), control: p(150, 62)); wallet.addLine(to: p(360, 62)); wallet.addLine(to: p(360, 140)); wallet.closeSubpath()
            context.fill(wallet, with: fill); stroke(wallet)
            var clasp = Path()
            clasp.move(to: p(360, 82)); clasp.addLine(to: p(300, 82))
            clasp.addCurve(to: p(300, 114), control1: p(278.7, 82), control2: p(278.7, 114))
            clasp.addLine(to: p(360, 114))
            context.fill(clasp, with: .color(.white)); stroke(clasp)
            context.fill(circle(302, 98, 5), with: ink)
        case (.bridge, false):
            var deck = Path(); deck.move(to: p(-4, 60)); deck.addLine(to: p(344, 60)); stroke(deck)
            var piers = Path(); piers.move(to: p(50, 60)); piers.addLine(to: p(50, 124)); piers.move(to: p(290, 60)); piers.addLine(to: p(290, 124)); stroke(piers)
            var arch = Path(); arch.move(to: p(50, 124)); arch.addCurve(to: p(290, 124), control1: p(50, 80), control2: p(290, 80)); stroke(arch)
            var hangers = Path()
            for (x, y) in [(88.0, 99.0), (126, 93), (170, 91), (214, 93), (252, 99)] { hangers.move(to: p(x, 60)); hangers.addLine(to: p(x, y)) }
            stroke(hangers, thin)
            var water = Path(); water.move(to: p(70, 114))
            for i in 0..<10 { let x = 70 + CGFloat(i) * 20; water.addQuadCurve(to: p(x + 20, 114), control: p(x + 10, i % 2 == 0 ? 109 : 119)) }
            stroke(water, thin, opacity: 0.45)
            if state == .now || state == .touch {
                var moving = Path(); moving.move(to: p(148, 28)); moving.addLine(to: p(164, 28)); moving.move(to: p(140, 40)); moving.addLine(to: p(160, 40))
                stroke(moving, StrokeStyle(lineWidth: 2.5 * k, lineCap: .round), opacity: 0.55)
            }
        case (.buy, false):
            var string = Path(); string.move(to: p(172, 58)); string.addQuadCurve(to: p(231, 78), control: p(206, 40)); stroke(string, thin)
            var tag = Path()
            tag.move(to: p(240, 30)); tag.addLine(to: p(340, 30)); tag.addQuadCurve(to: p(350, 40), control: p(350, 30))
            tag.addLine(to: p(350, 130)); tag.addLine(to: p(240, 130)); tag.addLine(to: p(214, 80)); tag.closeSubpath()
            context.fill(tag, with: fill); stroke(tag)
            stroke(circle(236, 80, 5), StrokeStyle(lineWidth: 2.5 * k))
            if let ticker {
                context.draw(Text(ticker).font(.ui(24 * k, .heavy)).foregroundStyle(look.tag), at: p(254, 80), anchor: .leading)
            }
        case (.swap, false):
            var arrows = Path()
            arrows.move(to: p(132, 48)); arrows.addLine(to: p(196, 48)); arrows.move(to: p(188, 40)); arrows.addLine(to: p(196, 48)); arrows.addLine(to: p(188, 56))
            arrows.move(to: p(196, 74)); arrows.addLine(to: p(132, 74)); arrows.move(to: p(140, 66)); arrows.addLine(to: p(132, 74)); arrows.addLine(to: p(140, 82))
            stroke(arrows)
            context.fill(circle(236, 60, 23), with: .color(.white)); context.stroke(circle(236, 60, 23), with: .color(.black), lineWidth: 2.5 * k)
            context.stroke(circle(236, 60, 16), with: .color(.black), lineWidth: 1.2 * k)
        case (.deposit, true):
            let box = Path(roundedRect: CGRect(x: 110 * k, y: 70 * k, width: 120 * k, height: 44 * k), cornerRadius: 8 * k)
            context.fill(box, with: fill); stroke(box)
            var slot = Path(); slot.move(to: p(150, 70)); slot.addLine(to: p(190, 70)); stroke(slot, StrokeStyle(lineWidth: 5 * k, lineCap: .round))
        case (.stake, false):
            for y in [100.0, 86, 72] {
                let disc = Path(ellipseIn: CGRect(x: 110 * k, y: (y - 11) * k, width: 120 * k, height: 22 * k))
                context.fill(disc, with: fill); stroke(disc)
            }
        case (.send, false):
            var trail = Path(); trail.move(to: p(112, 56)); trail.addQuadCurve(to: p(236, 44), control: p(176, 4))
            stroke(trail, StrokeStyle(lineWidth: 2.2 * k, lineCap: .round, dash: [4 * k, 7 * k]))
            stroke(circle(266, 42, 11))
            var shoulders = Path(); shoulders.move(to: p(242, 100)); shoulders.addQuadCurve(to: p(290, 100), control: p(266, 50)); stroke(shoulders)
        case (.check, false):
            let sheet = Path(roundedRect: CGRect(x: 190 * k, y: 14 * k, width: 100 * k, height: 120 * k), cornerRadius: 10 * k)
            context.fill(sheet, with: fill); stroke(sheet)
            var ticks = Path()
            for y in [48.0, 76] { ticks.move(to: p(206, y)); ticks.addLine(to: p(211, y + 5)); ticks.addLine(to: p(220, y - 4)) }
            context.stroke(ticks, with: .color(good), style: line)
            var rows = Path(); rows.move(to: p(232, 49)); rows.addLine(to: p(276, 49)); rows.move(to: p(232, 77)); rows.addLine(to: p(276, 77))
            stroke(rows, StrokeStyle(lineWidth: 3.5 * k, lineCap: .round))
        case (.jev, false):
            let sheet = Path(roundedRect: CGRect(x: 176 * k, y: 16 * k, width: 92 * k, height: 112 * k), cornerRadius: 8 * k)
            context.fill(sheet, with: fill); stroke(sheet)
            context.fill(circle(250, 80, 26), with: fill); stroke(circle(250, 80, 26))
            var handle = Path(); handle.move(to: p(269, 99)); handle.addLine(to: p(294, 124)); stroke(handle, StrokeStyle(lineWidth: 6 * k, lineCap: .round))
            var tick = Path(); tick.move(to: p(238, 80)); tick.addLine(to: p(246, 88)); tick.addLine(to: p(260, 74))
            context.stroke(tick, with: .color(good), style: line)
        case (.long, false), (.short, false):
            // The price line the position rides: up and green for a long, down and red for a short, faintly filled under.
            let ys: [CGFloat] = kind == .long ? [112, 84, 98, 30] : [30, 58, 44, 112]
            let side = kind == .long ? good : bad
            var price = Path(); price.move(to: p(-4, ys[0])); price.addLine(to: p(90, ys[1])); price.addLine(to: p(140, ys[2])); price.addLine(to: p(344, ys[3]))
            var under = price; under.addLine(to: p(344, 130)); under.addLine(to: p(-4, 130)); under.closeSubpath()
            context.fill(under, with: .color(side.opacity(0.18)))
            context.stroke(price, with: .color(side), style: line)
        case (.schedule, false):
            // A calendar page, the day it runs marked.
            let page = Path(roundedRect: CGRect(x: 196 * k, y: 20 * k, width: 104 * k, height: 104 * k), cornerRadius: 12 * k)
            context.fill(page, with: fill); stroke(page)
            var top = Path()
            top.move(to: p(196, 46)); top.addLine(to: p(300, 46))
            top.move(to: p(224, 10)); top.addLine(to: p(224, 30)); top.move(to: p(272, 10)); top.addLine(to: p(272, 30))
            stroke(top)
            for (x, y) in [(222.0, 68.0), (248, 68), (274, 68), (222, 96), (248, 96)] { context.fill(circle(x, y, 3.2), with: .color(look.drawing.opacity(0.55))) }
            stroke(circle(274, 96, 10))
            context.fill(circle(274, 96, 3.6), with: ink)
        case (.split, false):
            // One amount branching into three vaults.
            var branches = Path()
            branches.move(to: p(-4, 62)); branches.addLine(to: p(232, 62))
            branches.move(to: p(130, 62)); branches.addCurve(to: p(232, 26), control1: p(176, 62), control2: p(186, 26))
            branches.move(to: p(130, 62)); branches.addCurve(to: p(232, 98), control1: p(176, 62), control2: p(186, 98))
            stroke(branches)
            for y in [13.0, 49, 85] {
                let vault = Path(roundedRect: CGRect(x: 236 * k, y: y * k, width: 72 * k, height: 26 * k), cornerRadius: 6 * k)
                context.fill(vault, with: fill); stroke(vault)
                var slot = Path(); slot.move(to: p(258, y)); slot.addLine(to: p(286, y))
                stroke(slot, StrokeStyle(lineWidth: 4 * k, lineCap: .round))
            }
        case (.recurring, false):
            // Three quarters of a loop round the coin, its arrow at the end, and the runs so far fading out.
            let r: CGFloat = 46, c = r * 0.5523
            var loop = Path(); loop.move(to: p(170, 16))
            loop.addCurve(to: p(216, 62), control1: p(170 + c, 16), control2: p(216, 62 - c))
            loop.addCurve(to: p(170, 108), control1: p(216, 62 + c), control2: p(170 + c, 108))
            loop.addCurve(to: p(124, 62), control1: p(170 - c, 108), control2: p(124, 62 + c))
            loop.move(to: p(116, 70)); loop.addLine(to: p(124, 60)); loop.addLine(to: p(132, 70))
            stroke(loop)
            for (x, o) in [(250.0, 1.0), (276, 0.6), (302, 0.3)] { context.fill(circle(x, 62, 6), with: .color(look.drawing.opacity(o))) }
        case (.wait, false):
            // An hourglass: the bridge is carrying the money over.
            var glass = Path(); glass.move(to: p(210, 16))
            glass.addCurve(to: p(236, 70), control1: p(210, 56), control2: p(236, 60))
            glass.addCurve(to: p(210, 124), control1: p(236, 80), control2: p(210, 84))
            glass.addLine(to: p(270, 124))
            glass.addCurve(to: p(244, 70), control1: p(270, 84), control2: p(244, 80))
            glass.addCurve(to: p(270, 16), control1: p(244, 60), control2: p(270, 56))
            glass.closeSubpath()
            context.fill(glass, with: fill); stroke(glass)
            var ends = Path(); ends.move(to: p(202, 16)); ends.addLine(to: p(278, 16)); ends.move(to: p(202, 124)); ends.addLine(to: p(278, 124)); stroke(ends)
            var sand = Path(); sand.move(to: p(226, 36)); sand.addLine(to: p(254, 36)); stroke(sand, StrokeStyle(lineWidth: 2 * k, lineCap: .round), opacity: 0.5)
            var heap = Path(); heap.move(to: p(222, 124)); heap.addQuadCurve(to: p(258, 124), control: p(240, 100)); heap.closeSubpath()
            context.fill(heap, with: .color(look.drawing.opacity(0.5)))
        case (.approve, false):
            // A key cut for one lock: this much, for this contract.
            context.fill(circle(226, 60, 20), with: fill); stroke(circle(226, 60, 20))
            stroke(circle(226, 60, 6))
            var shaft = Path(); shaft.move(to: p(246, 60)); shaft.addLine(to: p(332, 60))
            shaft.move(to: p(300, 60)); shaft.addLine(to: p(300, 76)); shaft.move(to: p(316, 60)); shaft.addLine(to: p(316, 72))
            stroke(shaft)
        case (.gas, false):
            // A fuel pump with ETH's diamond in its window.
            let pump = Path(roundedRect: CGRect(x: 200 * k, y: 22 * k, width: 64 * k, height: 104 * k), cornerRadius: 8 * k)
            context.fill(pump, with: fill); stroke(pump)
            stroke(Path(roundedRect: CGRect(x: 212 * k, y: 36 * k, width: 40 * k, height: 30 * k), cornerRadius: 4 * k))
            var diamond = Path(); diamond.move(to: p(232, 40)); diamond.addLine(to: p(241, 51)); diamond.addLine(to: p(232, 62)); diamond.addLine(to: p(223, 51)); diamond.closeSubpath()
            context.fill(diamond, with: ink)
            var hose = Path(); hose.move(to: p(264, 46)); hose.addLine(to: p(280, 46)); hose.addQuadCurve(to: p(292, 58), control: p(292, 46))
            hose.addLine(to: p(292, 92)); hose.addQuadCurve(to: p(300, 100), control: p(292, 100)); hose.addQuadCurve(to: p(308, 92), control: p(308, 100))
            hose.addLine(to: p(308, 56)); hose.addLine(to: p(296, 44))
            stroke(hose)
        case (.simulate, false):
            // A window where the move plays out first, a green tick when it comes out as planned.
            let screen = Path(roundedRect: CGRect(x: 186 * k, y: 16 * k, width: 128 * k, height: 88 * k), cornerRadius: 10 * k)
            context.fill(screen, with: fill); stroke(screen)
            var bar = Path(); bar.move(to: p(186, 36)); bar.addLine(to: p(314, 36)); stroke(bar, StrokeStyle(lineWidth: 2 * k, lineCap: .round))
            for x in [200.0, 210, 220] { context.fill(circle(x, 26, 2.6), with: ink) }
            var tick = Path(); tick.move(to: p(226, 68)); tick.addLine(to: p(240, 82)); tick.addLine(to: p(272, 50))
            context.stroke(tick, with: .color(good), style: StrokeStyle(lineWidth: 5 * k, lineCap: .round, lineJoin: .round))
            var stand = Path(); stand.move(to: p(250, 104)); stand.addLine(to: p(250, 118)); stand.move(to: p(226, 120)); stand.addLine(to: p(274, 120)); stroke(stand)
        case (.sign, false):
            // A fingerprint: Touch ID signs it.
            var outer = Path(); outer.move(to: p(200, 70)); outer.addCurve(to: p(246, 22), control1: p(204, 46), control2: p(222, 22))
            outer.addCurve(to: p(292, 66), control1: p(272, 22), control2: p(290, 42))
            stroke(outer, nil, opacity: 0.55)
            var ridges = Path()
            ridges.move(to: p(212, 104)); ridges.addCurve(to: p(244, 36), control1: p(204, 70), control2: p(220, 36)); ridges.addCurve(to: p(278, 88), control1: p(268, 36), control2: p(282, 58))
            ridges.move(to: p(224, 108)); ridges.addCurve(to: p(244, 52), control1: p(218, 82), control2: p(224, 52)); ridges.addCurve(to: p(264, 96), control1: p(262, 52), control2: p(268, 70))
            ridges.move(to: p(238, 112)); ridges.addCurve(to: p(246, 68), control1: p(234, 92), control2: p(236, 68)); ridges.addCurve(to: p(252, 102), control1: p(254, 68), control2: p(256, 82))
            stroke(ridges)
        case (.lock, false):
            // A padlock: the vault keeps it for its lockup.
            var shackle = Path(); shackle.move(to: p(212, 60)); shackle.addLine(to: p(212, 42))
            shackle.addCurve(to: p(268, 42), control1: p(212, 14), control2: p(268, 14)); shackle.addLine(to: p(268, 60))
            stroke(shackle)
            let case_ = Path(roundedRect: CGRect(x: 194 * k, y: 58 * k, width: 92 * k, height: 66 * k), cornerRadius: 12 * k)
            context.fill(case_, with: fill); stroke(case_)
            context.fill(circle(240, 84, 7), with: ink)
            var slot = Path(); slot.move(to: p(240, 88)); slot.addLine(to: p(240, 104)); stroke(slot, StrokeStyle(lineWidth: 5 * k, lineCap: .round))
        case (.close, false):
            // The price line runs to a flag: the position is closed there.
            var price = Path(); price.move(to: p(-4, 104)); price.addLine(to: p(70, 78)); price.addLine(to: p(120, 90)); price.addLine(to: p(196, 46)); stroke(price)
            var pole = Path(); pole.move(to: p(252, 124)); pole.addLine(to: p(252, 22)); stroke(pole)
            var flag = Path(); flag.move(to: p(252, 24)); flag.addLine(to: p(308, 24)); flag.addLine(to: p(296, 40)); flag.addLine(to: p(308, 56)); flag.addLine(to: p(252, 56)); flag.closeSubpath()
            context.fill(flag, with: fill); stroke(flag)
            var tick = Path(); tick.move(to: p(266, 40)); tick.addLine(to: p(273, 47)); tick.addLine(to: p(286, 34))
            context.stroke(tick, with: .color(good), style: line)
        case (.claim, false):
            // A gift box: rewards to collect.
            let box = Path(roundedRect: CGRect(x: 202 * k, y: 58 * k, width: 92 * k, height: 66 * k), cornerRadius: 8 * k)
            let lid = Path(roundedRect: CGRect(x: 194 * k, y: 42 * k, width: 108 * k, height: 20 * k), cornerRadius: 6 * k)
            context.fill(box, with: fill); stroke(box); context.fill(lid, with: fill); stroke(lid)
            var ribbon = Path(); ribbon.move(to: p(248, 42)); ribbon.addLine(to: p(248, 124))
            ribbon.move(to: p(248, 42)); ribbon.addCurve(to: p(248, 42), control1: p(232, 18), control2: p(212, 30))
            ribbon.addCurve(to: p(248, 42), control1: p(284, 30), control2: p(264, 18))
            stroke(ribbon)
            var sparks = Path(); sparks.move(to: p(170, 14)); sparks.addLine(to: p(170, 28)); sparks.move(to: p(163, 21)); sparks.addLine(to: p(177, 21))
            sparks.move(to: p(318, 22)); sparks.addLine(to: p(318, 32)); sparks.move(to: p(313, 27)); sparks.addLine(to: p(323, 27))
            stroke(sparks, StrokeStyle(lineWidth: 2.2 * k, lineCap: .round), opacity: 0.65)
        case (.receive, false):
            // An arrow into the tray: money coming in from outside.
            var arrow = Path(); arrow.move(to: p(244, 12)); arrow.addLine(to: p(244, 66))
            arrow.move(to: p(228, 50)); arrow.addLine(to: p(244, 66)); arrow.addLine(to: p(260, 50))
            stroke(arrow)
            var tray = Path(); tray.move(to: p(192, 80)); tray.addLine(to: p(220, 80)); tray.addLine(to: p(230, 96)); tray.addLine(to: p(258, 96))
            tray.addLine(to: p(268, 80)); tray.addLine(to: p(296, 80)); tray.addLine(to: p(296, 124)); tray.addLine(to: p(192, 124)); tray.closeSubpath()
            context.fill(tray, with: fill); stroke(tray)
        case (.rebalance, false):
            // The ring of where the money is, most of it on one side, and the arrow that turns it.
            let center = p(250, 62), r = 40 * k
            var most = Path(); most.addArc(center: center, radius: r, startAngle: .degrees(-90), endAngle: .degrees(125), clockwise: false)
            var rest = Path(); rest.addArc(center: center, radius: r, startAngle: .degrees(128), endAngle: .degrees(267), clockwise: false)
            context.stroke(most, with: ink, style: StrokeStyle(lineWidth: 14 * k, lineCap: .butt))
            context.stroke(rest, with: .color(look.drawing.opacity(0.4)), style: StrokeStyle(lineWidth: 14 * k, lineCap: .butt))
            var turn = Path(); turn.move(to: p(190, 30)); turn.addQuadCurve(to: p(236, 6), control: p(208, 6))
            turn.move(to: p(228, 0)); turn.addLine(to: p(237, 6)); turn.addLine(to: p(228, 13))
            stroke(turn, StrokeStyle(lineWidth: 2.5 * k, lineCap: .round, lineJoin: .round))
        case (.predict, false):
            // A ticket torn in two: YES and NO.
            let ticket = Path(roundedRect: CGRect(x: 186 * k, y: 24 * k, width: 140 * k, height: 78 * k), cornerRadius: 10 * k)
            context.fill(ticket, with: fill); stroke(ticket)
            var tear = Path(); tear.move(to: p(256, 32)); tear.addLine(to: p(256, 94))
            stroke(tear, StrokeStyle(lineWidth: 2 * k, lineCap: .round, dash: [3 * k, 5 * k]))
            context.draw(Text("YES").font(.ui(20 * k, .heavy)).foregroundStyle(good), at: p(221, 63), anchor: .center)
            context.draw(Text("NO").font(.ui(20 * k, .heavy)).foregroundStyle(bad), at: p(291, 63), anchor: .center)
        case (.protect, true):
            var shield = Path()
            shield.move(to: p(230, 10)); shield.addLine(to: p(290, 30)); shield.addLine(to: p(290, 64))
            shield.addCurve(to: p(230, 130), control1: p(290, 98), control2: p(264, 120))
            shield.addCurve(to: p(170, 64), control1: p(196, 120), control2: p(170, 98))
            shield.addLine(to: p(170, 30)); shield.closeSubpath()
            context.fill(shield, with: fill); stroke(shield)
            // The take above, dashed until it's hit; the stop below.
            var take = Path(); take.move(to: p(186, 52)); take.addLine(to: p(274, 52))
            context.stroke(take, with: .color(good), style: StrokeStyle(lineWidth: 3 * k, lineCap: .round, dash: [6 * k, 6 * k]))
            var stop = Path(); stop.move(to: p(190, 86)); stop.addLine(to: p(270, 86))
            context.stroke(stop, with: .color(bad), style: StrokeStyle(lineWidth: 3 * k, lineCap: .round))
        case (.result, false):
            var up = Path(); up.move(to: p(126, 50)); up.addCurve(to: p(244, 22), control1: p(170, 20), control2: p(206, 18))
            context.stroke(up, with: .color(good), style: StrokeStyle(lineWidth: 4 * k, lineCap: .round, dash: [7 * k, 7 * k]))
            var down = Path(); down.move(to: p(126, 72)); down.addCurve(to: p(244, 100), control1: p(170, 100), control2: p(206, 104))
            context.stroke(down, with: .color(bad), style: StrokeStyle(lineWidth: 4 * k, lineCap: .round))
        case (.alert(let up), false):
            // The level dashed across, the price running to it where the coin waits, the bell that rings there.
            func y(_ v: CGFloat) -> CGFloat { up ? 132 - v : v }
            var level = Path(); level.move(to: p(-4, y(98))); level.addLine(to: p(344, y(98)))
            context.stroke(level, with: .color(look.drawing.opacity(0.7)), style: StrokeStyle(lineWidth: 2.5 * k, lineCap: .round, dash: [8 * k, 7 * k]))
            var price = Path(); price.move(to: p(-4, y(34)))
            for (px, py) in [(46.0, 52.0), (90, 40), (138, 66), (180, 56), (212, 80)] { price.addLine(to: p(px, y(py))) }
            stroke(price)
            let cy = up ? 96.0 : 36.0
            var bell = Path()
            bell.move(to: p(284, cy + 12)); bell.addCurve(to: p(316, cy + 12), control1: p(284, cy - 22), control2: p(316, cy - 22))
            bell.move(to: p(279, cy + 12)); bell.addLine(to: p(321, cy + 12))
            context.fill(bell, with: fill); stroke(bell)
            context.fill(circle(300, cy + 18, 4.5), with: ink)
            stroke(circle(300, cy - 15, 3), thin)
        default:
            break
        }
    }
}

// MARK: The details

/// A step opened on the canvas, under its route: what it is and, while the move waits for Confirm, what can change —
/// the amount, and for a trade on Base or Arbitrum the way (whichever leaves more, Relay's swap, or Relay's bridge +
/// Uniswap). Save plans the move again with the new amount, through the same checks; the way is kept for this move.
struct StepDetails: View {
    @Environment(Workspace.self) private var ws
    let step: ActivityBubble
    let index: Int
    let count: Int
    let state: StepState
    let footer: String?
    /// Set only when the step can change.
    let pending: WalletCommand?
    let wayKey: String?
    /// What there is to spend, as the route's first step says it («free: 29.39 USDC»).
    let hint: String?
    /// Where the card it opened from is, for the notch.
    let notch: CGFloat
    let close: () -> Void
    let ask: () -> Void
    /// To the step before (−1) or after (+1), round the ring.
    let go: (Int) -> Void
    @State private var amount = ""
    @State private var way: String?
    @State private var problem: String?

    static let width: CGFloat = 520

    /// «BRIDGE · RELAY» → «Bridge · Relay».
    static func pretty(_ label: String) -> String {
        label.lowercased().capitalized.replacingOccurrences(of: " Or ", with: " or ").replacingOccurrences(of: "Usdc", with: "USDC")
            .replacingOccurrences(of: "Jitosol", with: "JitoSOL").replacingOccurrences(of: "Hlp", with: "HLP").replacingOccurrences(of: "V3", with: "v3")
            .replacingOccurrences(of: "Layerzero", with: "LayerZero").replacingOccurrences(of: "Cctp", with: "CCTP")
            .replacingOccurrences(of: "Kyberswap", with: "KyberSwap")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            header
            if let pending, pending.editableAmount != nil { amountField(pending) }
            if pending != nil, wayKey != nil { ways }
            if let pending, pending.isStockTrade { stockWays }
            if let money = step.money, !money.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text(state == .done ? "MONEY · WAS → NOW" : "MONEY · NOW → AFTER").font(.ui(10, .heavy)).tracking(0.8).foregroundStyle(.secondary)
                    ForEach(money) { MoneyLine(move: $0, size: 13) }
                }
                .padding(.horizontal, 12).padding(.vertical, 10)
                .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.primary.opacity(0.045)))
            }
            if pending == nil {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(step.lines.filter { !$0.isEmpty }, id: \.self) { Text($0.rowText).font(.ui(12)) }
                    if let footer { Text(footer).font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                }
            }
            if let problem { Label(problem, systemImage: "exclamationmark.triangle").font(.ui(11)).foregroundStyle(Brand.loss) }
            buttons
        }
        .padding(.horizontal, 20).padding(.vertical, 18)
        .frame(width: Self.width, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 20, style: .continuous).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(Brand.cardBorder, lineWidth: 1.5))
        .overlay(alignment: .topLeading) {
            // The notch points at the card the details came from.
            Rectangle().fill(Brand.card).frame(width: 14, height: 14)
                .overlay(alignment: .topLeading) { Rectangle().fill(Brand.cardBorder).frame(width: 14, height: 1.5) }
                .overlay(alignment: .topLeading) { Rectangle().fill(Brand.cardBorder).frame(width: 1.5, height: 14) }
                .rotationEffect(.degrees(45))
                .offset(x: min(max(notch - 7, 24), Self.width - 38), y: -7)
        }
        .shadow(color: .black.opacity(0.08), radius: 14, y: 6)
        .onAppear {
            amount = pending?.editableAmount.map { NSDecimalNumber(decimal: $0).stringValue } ?? ""
            way = wayKey.flatMap { ws.routeWays[$0] }
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            LogoBadge(step: step, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(Self.pretty(step.label)).font(.ui(16, .bold)).lineLimit(1)
                Text("Step \(index + 1) of \(count) · \(ws.readable(step.title))").font(.ui(12)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 8)
            if let place = step.logo.flatMap(Openable.place(logo:)) {
                Button("Open \(place.capitalized)") { ws.toggleDetail(place) }
                    .buttonStyle(.bordered).controlSize(.small)
                    .help("The place's own details")
            }
            if count > 1 {
                // The arrow keys too, unless an amount is being typed here: there they move the cursor.
                let keys = pending?.editableAmount == nil
                Button { go(-1) } label: { Image(systemName: "chevron.left").font(.system(size: 12, weight: .bold)).frame(width: 30, height: 30) }
                    .buttonStyle(.hoverIcon).help("Step \((index + count - 1) % count + 1)")
                    .keyboardShortcut(keys ? KeyboardShortcut(.leftArrow, modifiers: []) : nil)
                Button { go(1) } label: { Image(systemName: "chevron.right").font(.system(size: 12, weight: .bold)).frame(width: 30, height: 30) }
                    .buttonStyle(.hoverIcon).help("Step \((index + 1) % count + 1)")
                    .keyboardShortcut(keys ? KeyboardShortcut(.rightArrow, modifiers: []) : nil)
            }
            // Closes as the card's own «Close ×», Close below and Esc do.
            Button(action: close) { Image(systemName: "xmark").font(.system(size: 12, weight: .bold)).frame(width: 30, height: 30) }
                .buttonStyle(.hoverIcon).help("Close · Esc")
        }
    }

    private func amountField(_ pending: WalletCommand) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("YOU SEND").font(.ui(10, .heavy)).tracking(0.8).foregroundStyle(.secondary)
            HStack(spacing: 10) {
                if let logo = StepTitle.logo(unit) { TokenLogo(name: logo, size: 24) }
                TextField("Amount", text: $amount).textFieldStyle(.plain).font(.ui(20, .bold)).monospacedDigit().frame(width: 110)
                    .onSubmit(save)
                Text(unit).font(.ui(14, .semibold))
                Spacer(minLength: 6)
                if let hint = hint { Text(hint).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1) }
            }
            .padding(.horizontal, 12).frame(height: 46)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Brand.cardBorder, lineWidth: 1.5))
        }
    }

    /// What the amount is in: USDC for most moves, SOL to stake.
    private var unit: String {
        if case .stake? = pending { return "SOL" }
        if case .lendDeposit(let symbol, _)? = pending { return symbol }
        if case .kamino(_, let symbol, _)? = pending { return symbol }
        return "USDC"
    }

    private var ways: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("ROUTE").font(.ui(10, .heavy)).tracking(0.8).foregroundStyle(.secondary)
            wayRow(nil, "The best one", quotes?.first.map { "now \(Workspace.wayName($0.way)), ≈ \($0.gets) · checked again before signing" }
                   ?? "both quoted, gas counted · checked again before signing", logos: ["relay", "uni", "kyber"], recommended: true)
            wayRow("relay", "Relay swap", quoted("relay") ?? "one Solana signature, Relay swaps on the way", logos: ["relay"])
            wayRow("bridge", "Relay bridge + the best exchange", quoted("bridge") ?? "USDC over the bridge, then Uniswap or KyberSwap with an exact approve",
                   logos: ["relay", "uni", "kyber"])
        }
    }

    /// A Robinhood Chain trade's ways as quoted now (Workspace.quoteStock): the best one is what Confirm takes.
    private var stockWays: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("ROUTES · BEST IS TAKEN").font(.ui(10, .heavy)).tracking(0.8).foregroundStyle(.secondary)
            if let quotes, !quotes.isEmpty {
                ForEach(Array(quotes.enumerated()), id: \.offset) { i, q in
                    HStack(spacing: 12) {
                        TokenLogo(name: q.way == "relay" ? "relay" : "uni", size: 24, round: true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(Workspace.stockWayName(q.way)).font(.ui(13, .bold))
                            Text("≈ \(q.gets)" + (i == 0 ? "" : " · \(Fmt.usd(quotes[0].worth - q.worth)) less")).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1)
                        }
                        Spacer(minLength: 6)
                        if i == 0 {
                            Text("Best").font(.ui(10, .bold)).foregroundStyle(Brand.accentInk)
                                .padding(.horizontal, 8).padding(.vertical, 3).background(Capsule().fill(Brand.accent.opacity(0.16)))
                        }
                    }
                    .padding(.horizontal, 12).padding(.vertical, 9)
                    .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(i == 0 ? Brand.accent : Brand.cardBorder, lineWidth: i == 0 ? 2 : 1.5))
                }
                Text("Quoted again right before signing; the price must sit within 5% of the market.").font(.ui(10)).foregroundStyle(.secondary)
            } else {
                Text(quotes == nil ? "Comparing Uniswap v4, v3 and Relay’s swap…" : "No way quotes it right now.").font(.ui(11)).foregroundStyle(.secondary)
            }
        }
    }

    /// Both ways as quoted now for this trade, the better first (Workspace.quoteSwap).
    private var quotes: [(way: String, gets: String, worth: Decimal)]? {
        ws.swapQuote.flatMap { $0.command == pending ? $0.ways : nil }
    }

    private func quoted(_ way: String) -> String? {
        quotes?.first { $0.way == way || $0.way.hasPrefix(way + ":") }.map {
            "≈ \($0.gets)\($0.way.hasPrefix("bridge:") ? " on \(Workspace.venueName(String($0.way.dropFirst(7))))" : "") · \(Fmt.usd($0.worth)) after gas"
        }
    }

    private func wayRow(_ value: String?, _ title: String, _ detail: String, logos: [String], recommended: Bool = false) -> some View {
        let on = way == value
        return Button { way = value } label: {
            HStack(spacing: 12) {
                Circle().strokeBorder(on ? Brand.accent : Brand.cardBorder, lineWidth: 2).frame(width: 18, height: 18)
                    .overlay { if on { Circle().fill(Brand.accent).frame(width: 8, height: 8) } }
                HStack(spacing: -7) { ForEach(logos, id: \.self) { TokenLogo(name: $0, size: 24, round: true) } }
                    .frame(width: 42, alignment: .leading)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.ui(13, .bold))
                    Text(detail).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 6)
                if recommended {
                    Text("Recommended").font(.ui(10, .bold)).foregroundStyle(Brand.accentInk)
                        .padding(.horizontal, 8).padding(.vertical, 3).background(Capsule().fill(Brand.accent.opacity(0.16)))
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(on ? Brand.accent.opacity(0.07) : .clear))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(on ? Brand.accent : Brand.cardBorder, lineWidth: on ? 2 : 1.5))
            .contentShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
        .pointerStyle(.link)
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private var buttons: some View {
        HStack(spacing: 8) {
            Button(action: ask) {
                HStack(spacing: 7) { BrandLogo(size: 20, mood: .waiting); Text("Ask in chat").font(.ui(13, .semibold)) }
                    .padding(.leading, 6).padding(.trailing, 12).frame(height: 36)
                    .background(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Brand.cardBorder, lineWidth: 1.5))
                    .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
            .buttonStyle(.plain).pointerStyle(.link)
            .help("Attaches this step to your next message")
            Spacer()
            if pending != nil {
                // Esc too: the corner ✕ went, these close it.
                Button("Cancel", action: close).buttonStyle(.bordered).controlSize(.large).keyboardShortcut(.cancelAction)
                Button("Save", action: save).buttonStyle(.borderedProminent).controlSize(.large).disabled(ws.busy)
            } else {
                Button("Close", action: close).buttonStyle(.bordered).controlSize(.large).keyboardShortcut(.cancelAction)
            }
        }
    }

    private func save() {
        guard let pending else { close(); return }
        var changed: WalletCommand?
        if let old = pending.editableAmount {
            guard let value = Self.amount(amount), value > 0 else {
                problem = "Type the amount as one number, for example 2.5 or 1000."
                return
            }
            if value != old { changed = pending.with(amount: value) }
        }
        if let wayKey { ws.routeWays[wayKey] = way }
        close()
        // A new amount is a new plan: the same checks, Jev and the same Confirm, as if typed.
        if let changed { Task { await ws.handleWallet(changed) } }
    }

    /// The field as one amount: «2», «0,5», «1 000», «1,000.50», «1.000,50». Anything else is nil, never the number it
    /// starts with («5abc» isn't 5), and so is «1,000» alone: a thousand, or one with three decimals.
    static func amount(_ text: String) -> Decimal? {
        var s = text.trimmingCharacters(in: .whitespaces)
        // Thousands apart by spaces: «1 000», «12 500,5».
        if s.wholeMatch(of: /\d{1,3}(?:\s\d{3})+(?:[.,]\d+)?/) != nil { s.removeAll { $0.isWhitespace } }
        // Thousands apart by the other mark: with a decimal mark after them, or more than one of them.
        if s.wholeMatch(of: /\d{1,3}(?:,\d{3})+(?:\.\d+)?/) != nil, s.contains(/\.|,.*,/) { s.removeAll { $0 == "," } }
        else if s.wholeMatch(of: /\d{1,3}(?:\.\d{3})+(?:,\d+)?/) != nil, s.contains(/,|\..*\./) { s.removeAll { $0 == "." } }
        guard let m = s.wholeMatch(of: /(\d+)(?:([.,])(\d+))?/) else { return nil }
        if m.2 == ",", m.3?.count == 3, m.1.contains(where: { $0 != "0" }) { return nil }
        return Decimal(string: s.replacingOccurrences(of: ",", with: "."))
    }
}

/// A route step handed to the chat: the next message is about it (Workspace.send), and a move waiting for Confirm
/// keeps waiting while the question is answered.
nonisolated struct StepContext: Equatable, Codable {
    let title: String
    let logo: String?
    /// What the model is told the message is about: a token's, a step's or a block's facts; a block gets its own when the
    /// message goes (Workspace.blockFacts).
    var about: String? = nil

    /// A canvas block as the model reads it: its name, and what it shows when the canvas brief has its lines.
    static func block(_ title: String, shows facts: String?) -> String {
        "About the block on the canvas «\(title)»" + (facts.map { " (it shows: \($0))" } ?? "")
    }

    /// Only the block's name went: the model has none of its numbers.
    var bare: Bool { about == nil || about == Self.block(title, shows: nil) }
}

/// The widest route on the canvas: the plan column widens for three cards in a row and stays 470 otherwise.
struct RouteWidth: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

/// The step handed to the chat, above its field until the message goes (or ✕ takes it back).
struct StepContextChip: View {
    @Environment(Workspace.self) private var ws
    let step: StepContext

    var body: some View {
        HStack(spacing: 8) {
            if let logo = step.logo { TokenLogo(name: logo, size: 18) }
            Text(step.title).font(.ui(12, .semibold)).lineLimit(1).truncationMode(.middle)
            Button { withAnimation(.snappy(duration: 0.2)) { ws.attached.removeAll { $0.title == step.title } } } label: {
                Image(systemName: "xmark").font(.system(size: 9, weight: .bold)).frame(width: 20, height: 20)
            }
            .buttonStyle(.hoverIcon).help(step.about == nil ? "Don't attach this step" : "Don't attach it")
        }
        .padding(.leading, 6).padding(.trailing, 3).padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Brand.glassFill))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(Brand.glassRim))
        .transition(.opacity)
        .help(step.about == nil ? "Your next message is about this step; a move waiting for Confirm keeps waiting" : "Your next message is about \(step.title)")
    }
}

extension Workspace {
    /// Hands something to the chat: its chip over the field (the same title again replaces it), the field taking the keys.
    func attach(_ c: StepContext) {
        if let i = attached.firstIndex(where: { $0.title == c.title }) { attached[i] = c } else { attached.append(c) }
        draftSelection = NSRange(location: draft.utf16.count, length: 0)
    }
}

/// Right-click on a canvas block: ask about it in the chat or say what to do with it (a chip over the field, the block's
/// edge lit, the field taking the keys), or «What's going on here?» straight away. Each right-click adds a block, so one
/// message can be about several (the owner, 2026-10-19: «как будем работать, когда множество блоков»). The model already
/// has every block's numbers (Workspace.canvasBlocks); the chip tells it which ones the message is about.
/// `radius`: the block's own corners for the lit edge; nil for a block with no frame of its own (route cards).
struct BlockMenu: ViewModifier {
    @Environment(Workspace.self) private var ws
    let title: String
    var logo: String?
    var radius: CGFloat?

    func body(content: Content) -> some View {
        let on = ws.attached.contains { $0.title == title }
        let block = StepContext(title: title, logo: logo)
        let question = ws.say("What's going on here?", "Что здесь происходит?", "¿Qué está pasando aquí?")
        content
            .overlay {
                if on, let radius { RoundedRectangle(cornerRadius: radius).strokeBorder(Brand.accent, lineWidth: 2).allowsHitTesting(false) }
            }
            .contextMenu {
                Group {
                    if on {
                        Button { ws.attached.removeAll { $0.title == title } } label: {
                            Label(ws.say("Take out of the chat", "Убрать из чата", "Quitar del chat"), systemImage: "xmark.circle")
                        }
                    } else {
                        Button { ws.attach(block) } label: {
                            Label(ws.say("Ask or act in chat", "Спросить или поручить в чате", "Preguntar o pedir en el chat"), systemImage: "bubble.left")
                        }
                    }
                    Button { Task { await ws.send(question, about: [block]) } } label: { Label(question, systemImage: "questionmark.bubble") }
                        .disabled(ws.busy || !ws.hasModel)
                }
                .labelStyle(.titleAndIcon)
            }
    }
}

extension View {
    func blockMenu(_ title: String, logo: String? = nil, radius: CGFloat? = 24) -> some View {
        modifier(BlockMenu(title: title, logo: logo, radius: radius))
    }
}

extension Workspace {
    /// A place opened on the canvas by name: «Kamino», «Token · AI», «Chart · NEAR».
    func placeTitle(_ place: String) -> String {
        if let p = TokenBoard.parse(place) { return "Token · " + (favoriteToken(p.address)?.symbol ?? tokenLabel(p.address)) }
        if place == "chart" { return "Chart · " + (chart.map { IntentParser.pretty($0.coin) } ?? "") }
        return WalletCommand.placeName(place)
    }

    /// Which way a trade of `token` on `place` takes, as its route's step picked it: "relay", "bridge", or none.
    static func wayKey(_ place: String, _ token: String) -> String { "\(place):\(token.lowercased())" }
}

extension WalletCommand {
    /// A Robinhood Chain buy or sale, whose ways the step's details compare.
    var isStockTrade: Bool {
        switch self {
        case .buyStock, .sellStock: true
        default: false
        }
    }

    /// The key of the way a Base or Arbitrum trade takes (Workspace.routeWays); nil for any other move.
    var wayKey: String? {
        switch self {
        case .buyOn(let place, let token, _, _), .sellOn(let place, let token, _, _): Workspace.wayKey(place, token)
        default: nil
        }
    }
}

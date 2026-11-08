import SwiftUI

/// «What's new»: a panel over the window, opened from the Profile menu, the rail's logo or «what's new» in the chat. The
/// versions by date on the left, the chosen one's changes on the right as a plain list; a click on a change opens the
/// version as stories from it, switched with ← → (StoriesView). From WhatsNew/whatsnew.json, the same file release.sh turns
/// into the update window's notes. Nothing opens by itself: after an update an orange ring goes around the rail's logo
/// and the Profile menu's What's new button has a dot until the panel is seen.
@Observable
final class WhatsNew {
    static let shared = WhatsNew()

    struct Story: Decodable, Hashable {
        let title: String
        let text: String
        /// A phrase to put into the chat's field, shown as itself.
        var `try`: String?
        let symbol: String
        /// The tile's colour, hex: the brand's orange or a protocol's.
        let color: String
    }

    struct Release: Decodable {
        let version: String
        /// yyyy-MM-dd, when it came out.
        var date: String?
        let stories: [Story]

        /// «Oct 1, 2026»: Gregorian and English whatever the Mac's calendar is (this one counts Buddhist years).
        var day: String {
            guard let date, let d = try? Date(date + "T12:00:00Z", strategy: .iso8601) else { return "" }
            var style = Date.FormatStyle.dateTime.day().month(.abbreviated).year()
            style.calendar = Calendar(identifier: .gregorian)
            style.locale = Locale(identifier: "en_US")
            return d.formatted(style)
        }
    }

    /// Newest first.
    let releases: [Release] = {
        guard let url = Bundle.main.url(forResource: "whatsnew", withExtension: "json"), let data = try? Data(contentsOf: url) else { return [] }
        return (try? JSONDecoder().decode([Release].self, from: data)) ?? []
    }()
    private var seen = UserDefaults.standard.string(forKey: "whatsNewSeen")
    /// The panel is on screen.
    var open = false {
        didSet { if !open { playing = nil } }
    }
    /// The stories playing over the list: the version and the change they started from.
    var playing: (version: String, start: Int)?

    /// This version's stories, not seen yet.
    var unseen: Release? {
        let running = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String
        return releases.first { $0.version == running && $0.version != seen }
    }

    func markSeen() {
        guard let v = unseen?.version else { return }
        seen = v
        UserDefaults.standard.set(v, forKey: "whatsNewSeen")
    }
}

/// The rail's logo. With this version's stories not seen yet, an orange ring around it opens them.
struct WhatsNewLogo: View {
    var body: some View {
        let news = WhatsNew.shared.unseen
        Button { WhatsNew.shared.open = true } label: {
            BrandLogo(size: 30)
                .overlay(Circle().strokeBorder(Brand.accent, lineWidth: 2).padding(-3).opacity(news == nil ? 0 : 1))
        }
        .buttonStyle(.plain)
        // Not .disabled: a disabled button draws the logo faded.
        .allowsHitTesting(news != nil)
        .pointerStyle(news == nil ? nil : .link)
        .help(news.map { "What's new in \($0.version)" } ?? "")
    }
}

/// The panel over the dimmed window (fades in place; Esc, ✕ or a click outside closes it): the versions on the left, newest
/// first with their dates; the chosen one's changes on the right, one row each. A row opens the stories over the panel;
/// their Esc comes back to the list.
struct WhatsNewPanel: View {
    @Environment(Workspace.self) private var ws
    @FocusState private var focused: Bool

    var body: some View {
        ZStack {
            if let p = WhatsNew.shared.playing, let release = WhatsNew.shared.releases.first(where: { $0.version == p.version }) {
                StoriesView(release: release, start: p.start, tryIt: { phrase in ws.suggest(phrase); WhatsNew.shared.open = false },
                            close: { WhatsNew.shared.playing = nil })
                    .id("\(p.version)·\(p.start)").zIndex(1).transition(.opacity)
            }
            Color.black.opacity(0.35).ignoresSafeArea().onTapGesture { WhatsNew.shared.open = false }
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Text("What's new").font(.ui(22, .bold))
                    Spacer()
                    Button { WhatsNew.shared.open = false } label: { Image(systemName: "xmark").font(.system(size: 12, weight: .semibold)) }
                        .buttonStyle(.hoverIcon).help("Close · Esc")
                }
                .padding(.horizontal, 28).padding(.top, 22)
                WhatsNewPage()
            }
            .frame(maxWidth: 820, maxHeight: 560)
            .background(Brand.canvas)
            .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Brand.cardBorder))
            .shadow(color: .black.opacity(0.25), radius: 30, y: 18)
            // In a window under 870 × 610 the panel shrinks with it instead of running past its edges.
            .padding(24)
        }
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear { focused = true }
        .onKeyPress(.escape) { WhatsNew.shared.open = false; return .handled }
        .animation(.easeOut(duration: 0.2), value: WhatsNew.shared.playing?.start)
        .animation(.easeOut(duration: 0.2), value: WhatsNew.shared.playing?.version)
    }
}

struct WhatsNewPage: View {
    @Environment(Workspace.self) private var ws
    @State private var chosen = WhatsNew.shared.unseen?.version ?? WhatsNew.shared.releases.first?.version

    var body: some View {
        let releases = WhatsNew.shared.releases
        HStack(alignment: .top, spacing: 0) {
            // Its own scroll: a dozen releases are taller than the panel, and the list pushed the title out of it.
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(releases, id: \.version) { r in
                        Button { chosen = r.version } label: {
                            VStack(alignment: .leading, spacing: 1) {
                                Text("Version \(r.version)").font(.ui(13, chosen == r.version ? .semibold : .regular))
                                if !r.day.isEmpty { Text(r.day).font(.ui(11)).foregroundStyle(.secondary) }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 10).padding(.vertical, 6)
                            .background(RoundedRectangle(cornerRadius: 8).fill(chosen == r.version ? Brand.accent.opacity(0.14) : .clear))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.bottom, 24)
            }
            .frame(width: 170)
            .padding(.trailing, 16)
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    let stories = releases.first { $0.version == chosen }?.stories ?? []
                    ForEach(Array(stories.enumerated()), id: \.element) { i, s in
                        row(s, play: { if let chosen { WhatsNew.shared.playing = (chosen, i) } })
                    }
                }
                .frame(maxWidth: 560, alignment: .leading)
                .padding(.bottom, 24)
            }
        }
        .padding(.horizontal, 28).padding(.top, 16)
        .frame(maxHeight: .infinity, alignment: .top)
        .onAppear { WhatsNew.shared.markSeen() }
    }

    /// One change: its tile, its title and the first line of what it does, and «Try it» when a phrase shows it; the whole
    /// story is in the stories.
    private func row(_ s: WhatsNew.Story, play: @escaping () -> Void) -> some View {
        HStack(spacing: 12) {
            Button(action: play) {
                HStack(spacing: 12) {
                    RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color(hex: UInt32(s.color, radix: 16) ?? 0xFF7A2F))
                        .frame(width: 36, height: 36)
                        .overlay(Image(systemName: s.symbol).font(.system(size: 16, weight: .semibold)).foregroundStyle(.white))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(s.title).font(.ui(14, .semibold)).foregroundStyle(.primary)
                        Text(s.text).font(.ui(12)).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 8)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain).help("Open as stories: ← → to switch")
            if let phrase = s.try {
                TryItButton(phrase: phrase, size: 12) { ws.suggest(phrase); WhatsNew.shared.open = false }
            }
            Button(action: play) {
                Image(systemName: "play.circle").font(.system(size: 15, weight: .semibold)).foregroundStyle(Brand.accentInk)
            }
            .buttonStyle(.plain).help("Open as stories: ← → to switch")
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Brand.cardBorder))
        .modifier(HoverLift(radius: 14))
    }
}

/// «Try it»: the story's phrase into the chat's field, never sent (Workspace.suggest), shown as itself on hover.
struct TryItButton: View {
    let phrase: String
    var size: CGFloat = 13
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label("Try it", systemImage: "text.cursor").font(.ui(size, .semibold)).foregroundStyle(Brand.onAccent)
                .padding(.horizontal, size + 1).padding(.vertical, size * 0.6).background(Capsule().fill(Brand.accent))
        }
        .buttonStyle(.plain).pointerStyle(.link).help("Puts «\(phrase)» into the chat")
    }
}

/// One version's changes as stories, from the row clicked in the list: one card at a time over the dimmed panel. The arrows
/// beside the card and ← → switch; bars on top fill in 6 s and move on; Space pauses; Esc, ✕ or a click outside goes back
/// to the list, and so does the end of the last card. Cards fade in place (no slides); with Reduce Motion they just switch.
struct StoriesView: View {
    let release: WhatsNew.Release
    var start = 0
    /// The story's phrase into the chat's field.
    let tryIt: (String) -> Void
    /// Back to the list.
    let close: () -> Void
    @State private var index = 0
    /// Time watched on this card before the last pause; the clock runs from `resumed`.
    @State private var watched: TimeInterval = 0
    @State private var resumed = Date()
    @State private var paused = false
    @FocusState private var focused: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    static let duration: TimeInterval = 6

    private var story: WhatsNew.Story { release.stories[index] }

    var body: some View {
        ZStack {
            Color.black.opacity(0.35).ignoresSafeArea().onTapGesture(perform: close)
            HStack(spacing: 18) {
                arrow("chevron.left", enabled: index > 0) { go(-1) }.help("Previous · ←")
                card
                arrow("chevron.right", enabled: true) { go(1) }.help(index + 1 < release.stories.count ? "Next · →" : "Back to the list · →")
            }
        }
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear { index = min(start, release.stories.count - 1); focused = true }
        .onKeyPress(.leftArrow) { go(-1); return .handled }
        .onKeyPress(.rightArrow) { go(1); return .handled }
        .onKeyPress(.space) { pause(!paused); return .handled }
        .onKeyPress(.escape) { close(); return .handled }
        // Moves on when the card's time is up; a pause or a new card starts the wait again.
        .task(id: "\(index) \(paused)") {
            guard !paused else { return }
            try? await Task.sleep(for: .seconds(max(0, Self.duration - watched)))
            if !Task.isCancelled { go(1) }
        }
    }

    private func arrow(_ name: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: name).font(.system(size: 16, weight: .bold)).foregroundStyle(.white)
                .frame(width: 44, height: 44).background(Circle().fill(.black.opacity(0.35)))
                .overlay(Circle().strokeBorder(.white.opacity(0.25)))
        }
        .buttonStyle(.plain).pointerStyle(enabled ? .link : nil).opacity(enabled ? 1 : 0.3).disabled(!enabled)
    }

    private var card: some View {
        VStack(spacing: 0) {
            ZStack(alignment: .top) {
                Color(hex: UInt32(story.color, radix: 16) ?? 0xFF7A2F)
                Image(systemName: story.symbol).font(.system(size: 76, weight: .semibold)).foregroundStyle(.white)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .id(index).transition(.opacity)
                VStack(spacing: 10) {
                    bars
                    HStack {
                        Text("What's new in \(release.version)").font(.ui(12, .semibold)).foregroundStyle(.white.opacity(0.85))
                        Spacer()
                        Button(action: close) {
                            Image(systemName: "xmark").font(.system(size: 12, weight: .bold)).foregroundStyle(.white)
                                .frame(width: 28, height: 28).background(Circle().fill(.black.opacity(0.2)))
                        }
                        .buttonStyle(.plain).pointerStyle(.link).help("Back to the list · Esc")
                    }
                }
                .padding(14)
            }
            .frame(height: 300)
            VStack(alignment: .leading, spacing: 8) {
                Text(story.title).font(.ui(22, .semibold)).fixedSize(horizontal: false, vertical: true)
                Text(story.text).font(.ui(14)).foregroundStyle(.secondary).lineSpacing(2).fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 12)
                HStack {
                    if let phrase = story.try { TryItButton(phrase: phrase, size: 13) { tryIt(phrase) } }
                    Spacer()
                    Text("\(index + 1) of \(release.stories.count)").font(.ui(11)).foregroundStyle(.tertiary).monospacedDigit()
                }
            }
            .padding(20)
            .frame(maxHeight: .infinity, alignment: .top)
            .id(index).transition(.opacity)
        }
        .frame(width: 380, height: 560)
        .background(Brand.card)
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Brand.cardBorder))
        .shadow(color: .black.opacity(0.25), radius: 30, y: 18)
    }

    /// One bar per card: past ones full, this one filling (a paused one holds), the rest empty.
    private var bars: some View {
        TimelineView(.animation(minimumInterval: 1 / 30, paused: paused)) { context in
            let now = paused ? 0 : context.date.timeIntervalSince(resumed)
            let part = min(1, (watched + now) / Self.duration)
            HStack(spacing: 4) {
                ForEach(release.stories.indices, id: \.self) { i in
                    GeometryReader { g in
                        Capsule().fill(.white.opacity(0.35))
                            .overlay(alignment: .leading) {
                                Capsule().fill(.white).frame(width: g.size.width * (i < index ? 1 : i == index ? part : 0))
                            }
                    }
                    .frame(height: 3)
                }
            }
        }
    }

    private func go(_ step: Int) {
        let next = index + step
        guard release.stories.indices.contains(next) else {
            if step > 0 { close() } else { watched = 0; resumed = Date() }
            return
        }
        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { index = next }
        watched = 0
        resumed = Date()
        paused = false
    }

    private func pause(_ on: Bool) {
        guard on != paused else { return }
        if on { watched += Date().timeIntervalSince(resumed) } else { resumed = Date() }
        paused = on
    }
}

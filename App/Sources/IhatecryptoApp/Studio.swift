import AppKit
import DeFiYields
import HyperliquidSigning
import IhatecryptoCore
import JupiterSwap
import LLMCatalog
import LLMCore
import SwiftUI
import TradeExecution
import WalletCore

// «Студия» (R4 on the design canvas), ihatecrypto's only layout: the wallet rail on the left, the portfolio and the plan on
// one canvas, the chat floating on top of it.

extension Color {
    /// One color for light and one for dark appearance.
    init(light: UInt32, dark: UInt32, lightAlpha: CGFloat = 1, darkAlpha: CGFloat = 1) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let hex = isDark ? dark : light
            return NSColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
                           blue: CGFloat(hex & 0xFF) / 255, alpha: isDark ? darkAlpha : lightAlpha)
        })
    }
    init(hex: UInt32) { self.init(light: hex, dark: hex) }
}

extension Font {
    /// The interface face (Onest, in Fonts/); the system font under `swift run`, whose bundle has no fonts.
    static func ui(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        BrandFonts.ready ? .custom("Onest", size: size).weight(weight) : .system(size: size, weight: weight)
    }

    /// The wordmark's face (DynaPuff): only the logo is cartoonish, the interface stays business-like.
    static func brand(_ size: CGFloat) -> Font {
        BrandFonts.ready ? .custom("DynaPuff", size: size).weight(.bold) : .system(size: size, weight: .heavy, design: .rounded)
    }
}

/// Registers the bundled faces with this process the first time a view asks for one.
enum BrandFonts {
    static let ready: Bool = {
        var ok = true
        for name in ["Onest", "DynaPuff"] {
            guard let url = Bundle.main.url(forResource: name, withExtension: "ttf") else { ok = false; continue }
            ok = CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil) && ok
        }
        return ok
    }()
}

/// The chains' and protocols' own colors, so a block reads as «Solana» or «Hyperliquid» before its text does.
/// The brand's one color, orange, is for the chrome: the canvas keeps each chain's and protocol's own colors.
enum Brand {
    /// Fills: the primary button, the send button, the logo coin. Text on it is `onAccent`.
    static let accent = Color(hex: 0xFF7A2F)
    /// The accent as text or an icon on the window's background, dark enough to read in light mode.
    static let accentInk = Color(light: 0xB8480C, dark: 0xFF9A5C)
    /// Text and glyphs on an accent fill.
    static let onAccent = Color(hex: 0x111111)
    /// A control sitting on glass: a light pill in light mode, a faint one in dark mode.
    static let glassFill = Color(light: 0xFFFFFF, dark: 0xFFFFFF, lightAlpha: 0.72, darkAlpha: 0.08)
    static let glassRim = Color(light: 0xFFFFFF, dark: 0xFFFFFF, lightAlpha: 0.95, darkAlpha: 0.12)
    /// The selected row on glass.
    static let glassSelected = Color(light: 0xFFFFFF, dark: 0xFFFFFF, lightAlpha: 0.9, darkAlpha: 0.14)
    /// A tint for the big glass panels (rail, chat): text over colored canvas cards stays readable.
    static let glassTint = Color(light: 0xFFFFFF, dark: 0x15171C, lightAlpha: 0.42, darkAlpha: 0.55)
    static let canvas = Color(light: 0xEEF0F3, dark: 0x0E0F12)
    static let dots = Color(light: 0xD3D7DC, dark: 0x26282D)
    static let card = Color(light: 0xFFFFFF, dark: 0x1B1E24)
    static let cardBorder = Color(light: 0xE1E4EC, dark: 0x2C3038)
    static let solanaTint = Color(light: 0xF4ECFF, dark: 0x2A2140)
    static let solanaInk = Color(light: 0x5B3AA8, dark: 0xC9B3FF)
    static let relay = Color(hex: 0x4615C8)
    static let relayInk = Color(hex: 0xDCD0FF)
    static let hl = Color(hex: 0x072723)
    static let hlChip = Color(hex: 0x0D3B35)
    static let hlMint = Color(hex: 0x97FCE4)
    static let hlSoft = Color(hex: 0xB6E8DC)
    static let jupiterTint = Color(light: 0xEEF8E0, dark: 0x1E2A16)
    static let jupiterInk = Color(light: 0x3D6B12, dark: 0xC7F284)
    static let riskTint = Color(light: 0xF3EDFF, dark: 0x241F36)
    static let riskInk = Color(light: 0x5B3AA8, dark: 0xC9B3FF)
    static let blockedTint = Color(light: 0xFDECEA, dark: 0x3A1D1A)
    static let blockedInk = Color(light: 0xB42318, dark: 0xFF9C8F)
    static let jevTint = Color(light: 0xFFF7E6, dark: 0x33290F)
    static let jevInk = Color(light: 0x7A4A0C, dark: 0xF5C451)
    static let outcome = Color(light: 0x0F1222, dark: 0x2A2E3A)
    static let profit = Color(light: 0x0B7A55, dark: 0x6FF0B4)
    static let loss = Color(light: 0xB42318, dark: 0xFF9C8F)
    static let secondCursor = Color(hex: 0x7C3AED)
    static let jevCursor = Color(hex: 0xF59E0B)
}

/// A chain or token logo from the app bundle (Logos/); a lettered circle when the file is missing (`swift run`).
struct TokenLogo: View {
    let name: String
    var size: CGFloat = 28
    /// A disc edge to edge on white: a square logo (Solana's) is cut to the disc, so no white shows beside it and no
    /// corner sticks out; a transparent one sits on the white.
    var round = false

    var body: some View {
        if name == "hype" {
            (round ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: size * 0.3))).fill(Brand.hl).frame(width: size, height: size)
                .overlay(logo(size: size * 0.72, round: false))
        } else if name == "uni" {
            // Uniswap's pink unicorn has no background of its own: on the dark theme it floated; a white disc holds it.
            Circle().fill(.white).frame(width: size, height: size).overlay(logo(size: size * 0.74, round: false))
        } else {
            logo(size: size, round: round)
        }
    }

    @ViewBuilder private func logo(size: CGFloat, round: Bool) -> some View {
        if let image = Self.image(name) {
            let picture = Image(nsImage: image).resizable().interpolation(.high).aspectRatio(contentMode: .fit).frame(width: size, height: size)
            if round {
                // The white half a point inside: under the whole disc its edge showed as a hairline rim.
                picture.clipShape(Circle()).background(Circle().fill(.white).padding(0.5))
            } else {
                picture.clipShape(RoundedRectangle(cornerRadius: size * 0.28))
            }
        } else {
            Circle().fill(.quaternary).frame(width: size, height: size)
                .overlay(Text(name.prefix(1).uppercased()).font(.ui(size * 0.45, .bold)))
        }
    }

    /// Misses too: most traded assets (oil, stocks, many coins) have no file, and every badge asked the bundle twice
    /// on every pass.
    private static var cache: [String: NSImage?] = [:]
    static func image(_ name: String) -> NSImage? {
        if let hit = cache[name] { return hit }
        let image = ["png", "svg"].lazy.compactMap { Bundle.main.url(forResource: name, withExtension: $0).flatMap(NSImage.init(contentsOf:)) }.first
        cache[name] = .some(image)
        return image
    }

    /// The logo for a traded asset; nil for assets that have none (oil, stocks).
    /// By ticker: «xrp.png», «tsla.png», «eth.svg» (HIP-3 «xyz:TSLA» too) when bundled; nil when there's none.
    static func name(forAsset coin: String) -> String? {
        let ticker = (coin.split(separator: ":").last.map(String.init) ?? coin).lowercased()
        let name = aliases[ticker] ?? ticker
        return image(name) != nil ? name : nil
    }

    /// Bridged coins shown with their own coin's logo: Unit's BTC, ETH and SOL on Hyperliquid spot, Tether's USDT0.
    static let aliases = ["ubtc": "btc", "ueth": "eth", "usol": "sol", "usdt0": "usdt"]
}

struct StudioView: View {
    @Environment(Workspace.self) private var ws
    /// Full-height chat; by default it sits compact on the lower half and grows on demand.
    @AppStorage("chatFull") private var chatOpen = false
    /// «plan» — the canvas; «history» — everything that happened.
    @AppStorage("canvasMode") private var mode = "plan"
    /// The chat's width, set by dragging its left edge: from a column up to half the window.
    @AppStorage("chatWidth") private var chatWidth = 350.0
    /// The width while the edge is being dragged; nil when resting.
    @State private var resizing: CGFloat?
    /// Where the chat sits, as an offset from its docked corner (bottom right); both ≤ 0.
    @AppStorage("chatX") private var chatX = 0.0
    @AppStorage("chatY") private var chatY = 0.0
    /// The header's pull while the chat is being moved.
    @State private var moving: CGSize = .zero
    /// Where the web panel was moved by its header, from its place beside the chat (both ≤ 0), and its pull.
    @AppStorage("webX") private var webX = 0.0
    @AppStorage("webY") private var webY = 0.0
    @State private var webMoving: CGSize = .zero
    /// The web panel's width set by its left edge; 0 is its own (a share of the canvas).
    @AppStorage("webWidth") private var webWidth = 0.0
    @State private var webResizing: CGFloat?
    @AppStorage("railCollapsed") private var railCollapsed = true
    /// The canvas in its own hosting view, panned by the scroll view (CanvasHost.swift); off = the SwiftUI offset.
    @AppStorage("canvasHost") private var canvasHost = true
    @State private var railFoldedForRoom = false
    /// Counts the orange waves around the window's edge (EdgePulse); each new value runs one.
    @State private var edgePulse = 0
    /// The block library under «Add block» (BlocksPanel.swift).
    @State private var showBlocks = false
    /// Where the block library was last left: beside the sidebar, or under «Add block» at the right.
    @AppStorage("blocksOnLeft") private var blocksOnLeft = false
    static let spring = Animation.spring(response: 0.42, dampingFraction: 0.82)
    static let minChat: CGFloat = 320
    /// Room the canvas keeps for the chat when it fits its boards: the default chat width and its margins.
    static let chatColumn: CGFloat = 350 + 32
    /// Below this window width the wide rail (280) and the chat (350) would leave the canvas too little room.
    static let crampedWidth: CGFloat = 1140

    /// Keeps the chat inside the canvas: offsets from the docked corner, never past the left or top edge.
    static func clamp(_ s: CGSize, room: CGSize) -> CGSize {
        CGSize(width: min(0, max(-room.width, s.width)), height: min(0, max(-room.height, s.height)))
    }

    var body: some View {
        // Where the rail ends: the canvas runs under it, its boards start past it.
        let railEdge = 10 + (railCollapsed ? StudioRail.narrowWidth : StudioRail.wideWidth)
        ZStack(alignment: .topLeading) {
            // The canvas fills the window: the boards show around and through the glass rail, not cut at its edge.
            if mode == "history" {
                HistoryView { withAnimation(.snappy) { mode = "plan" } }.padding(.leading, railEdge)
            } else {
                // The canvas's scale follows the window alone: moving or resizing the chat, which floats
                // above everything, never shifts or rescales the boards.
                if canvasHost {
                    CanvasScroller(inset: Self.chatColumn, leading: railEdge, contentWidth: StudioCanvas.contentWidth) { StudioCanvas() }
                } else {
                    CanvasViewport(inset: Self.chatColumn, leading: railEdge, contentWidth: StudioCanvas.contentWidth) { StudioCanvas() }
                }
            }
            HStack(spacing: 0) {
                // The rail floats over the canvas as a glass panel; the window's traffic lights sit inside it.
                StudioRail().padding([.leading, .vertical], 10)
                    .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { ws.railFrame = $0 }
                GeometryReader { geo in
                    let maxWidth = max(Self.minChat, geo.size.width / 2)
                    let width = min(resizing ?? CGFloat(chatWidth), maxWidth)
                    // Full height, or compact (half the window); the header moves the chat anywhere inside the canvas.
                    let full = max(200, geo.size.height - 32)
                    let small = min(full, max(260, geo.size.height * 0.5))
                    let height = chatOpen ? full : small
                    let room = CGSize(width: max(0, geo.size.width - width - 32), height: max(0, geo.size.height - height - 32))
                    let place = Self.clamp(CGSize(width: chatX + moving.width, height: chatY + moving.height), room: room)
                    // Docked, the canvas keeps clear of the chat; moved away, the chat floats over it.
                    let docked = place.width > -40 && place.height > -40
                    // «Add block» keeps to the canvas's top right; a chat reaching the top there pushes it to the chat's left.
                    let chatLeft = geo.size.width - 16 - width + place.width
                    let chatTop = geo.size.height - 16 - height + place.height
                    let blocksEdge = chatTop < 70 && chatLeft + width > geo.size.width - 200 ? geo.size.width - chatLeft + 12 : 16
                    ZStack(alignment: .topTrailing) {
                        // A page opened from a board or the chat: beside the docked chat, under the chat when it's moved.
                        if ws.showWeb {
                            let beside = docked ? width + 32 : 16
                            let widest = max(380, geo.size.width - beside - 16)
                            let restWidth = min(widest, webWidth > 0 ? CGFloat(webWidth) : min(760, max(380, (geo.size.width - beside) * 0.62)))
                            let panelWidth = webResizing ?? restWidth
                            // Moved by its header anywhere left of its place, never out of the canvas; home again within 40 pt.
                            let webRoom = CGSize(width: max(0, geo.size.width - beside - panelWidth - 16), height: 0)
                            WebPanel(drag: $webMoving, endDrag: { pull in
                                let end = Self.clamp(CGSize(width: webX + pull.width, height: webY + pull.height), room: webRoom)
                                webX = end.width > -40 ? 0 : end.width
                                webY = 0
                                webMoving = .zero
                            }, dock: { webX = 0; webY = 0 })
                                .frame(width: panelWidth)
                                .frame(maxHeight: max(300, geo.size.height - 32))
                                .overlay(alignment: .leading) {
                                    ResizeEdge(active: webResizing != nil, help: "Drag to resize the page · double-click: its own width",
                                               changed: { webResizing = min(widest, max(380, restWidth + $0)) },
                                               ended: { if let r = webResizing { webWidth = Double(r) }; webResizing = nil },
                                               double: { withAnimation(Self.spring) { webWidth = 0 } })
                                }
                                .offset(Self.clamp(CGSize(width: webX + webMoving.width, height: 0), room: webRoom))
                                .padding(.top, 16).padding(.trailing, beside)
                                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                                .transition(.opacity)
                        }
                        FloatingChat(
                            compact: !chatOpen,
                            wide: width >= maxWidth - 1,
                            toggleCompact: { withAnimation(Self.spring) { chatOpen.toggle() } },
                            toggleWide: { withAnimation(Self.spring) { chatWidth = width >= maxWidth - 1 ? 350 : Double(maxWidth) } },
                            drag: $moving,
                            endDrag: { pull in
                                let end = Self.clamp(CGSize(width: chatX + pull.width, height: chatY + pull.height), room: room)
                                // Close to its corner it snaps home.
                                let home = end.width > -40 && end.height > -40
                                chatX = home ? 0 : end.width
                                chatY = home ? 0 : end.height
                                moving = .zero
                            },
                            dock: { chatX = 0; chatY = 0 }
                        )
                        .frame(width: width, height: height)
                        .overlay(alignment: .leading) { edge(width: width, maxWidth: maxWidth) }
                        .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { ws.chatFrame = $0 }
                        .shadow(color: .black.opacity(docked ? 0 : 0.22), radius: 26, y: 16)
                        .offset(place)
                        .padding(16)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomTrailing)
                        // Over the chat while open: a pick closes it and the chat shows the block's words.
                        if mode == "plan" {
                            // «Add block» / «Close blocks» stays at the right; the library itself sits under it or,
                            // moved by its corner button, beside the sidebar, and glides between the two.
                            AddBlockButton(open: $showBlocks)
                                .padding(.top, 16).padding(.trailing, blocksEdge)
                            if showBlocks {
                                BlocksPanel(left: blocksOnLeft, move: { blocksOnLeft.toggle() }) { withAnimation(.snappy) { showBlocks = false } }
                                    .frame(maxHeight: max(200, geo.size.height - (blocksOnLeft ? 32 : 78)), alignment: .top)
                                    .padding(.top, blocksOnLeft ? 16 : 62)
                                    .padding(.leading, blocksOnLeft ? 16 : 0)
                                    .padding(.trailing, blocksOnLeft ? 0 : blocksEdge)
                                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: blocksOnLeft ? .topLeading : .topTrailing)
                            }
                        }
                    }
                    .animation(.snappy, value: blocksOnLeft)
                    // @AppStorage changes skip withAnimation, so the spring is tied to the values themselves.
                    .animation(Self.spring, value: chatOpen)
                    .animation(Self.spring, value: chatX)
                    .animation(Self.spring, value: chatY)
                }
            }
        }
        // No title bar: the rail and the canvas go up to the window's top edge.
        .ignoresSafeArea(edges: .top)
        // Web links on the canvas and in the chat (a chart, an explorer, a transaction) open beside the canvas; the
        // rest (mail, apps) go to the system. Settings isn't under this: signing in goes through the browser.
        .environment(\.openURL, OpenURLAction { url in
            guard ["http", "https"].contains(url.scheme?.lowercased() ?? "") else { return .systemAction }
            MainActor.assumeIsolated { ws.openWeb(url) }
            return .handled
        })
        .overlay { EdgePulse(trigger: edgePulse) }
        // «What's new» over everything, the chat too; it fades in place.
        .overlay { if WhatsNew.shared.open { WhatsNewPanel().transition(.opacity) } }
        .animation(.easeOut(duration: 0.2), value: WhatsNew.shared.open)
        .onReceive(NotificationCenter.default.publisher(for: .canvasEdgePulse)) { _ in edgePulse += 1 }
        // One ground under the rail and the canvas, so the glass has the same dots behind it on both sides.
        .background { ZStack { Brand.canvas; DotGrid() }.ignoresSafeArea() }
        // Folded, the lights sit in the middle of the strip; open, at the rail's left beside its first row.
        .background(TrafficLights(middle: railCollapsed ? 10 + StudioRail.narrowWidth / 2 : nil))
        // A window getting narrow folds the rail, and widening again brings back what it folded; by hand it still opens.
        .onGeometryChange(for: Bool.self) { $0.size.width < Self.crampedWidth } action: { cramped in
            withAnimation(Self.spring) {
                if cramped, !railCollapsed { railCollapsed = true; railFoldedForRoom = true }
                else if !cramped, railFoldedForRoom { railCollapsed = false; railFoldedForRoom = false }
            }
        }
        .sheet(isPresented: Binding(get: { ws.preparing || ws.prepared != nil || (ws.executionError != nil && !ws.executing) },
                                    set: { if !$0 { ws.prepared = nil; ws.executionError = nil } })) {
            ConfirmSheet().environment(ws)
        }
    }

    /// The chat's left edge: drag to widen up to half the window, double-click to switch between narrow and half.
    private func edge(width: CGFloat, maxWidth: CGFloat) -> some View {
        ResizeEdge(active: resizing != nil, help: "Drag to resize the chat · double-click: half the window",
                   changed: { resizing = min(maxWidth, max(Self.minChat, CGFloat(chatWidth) + $0)) },
                   ended: { if let r = resizing { chatWidth = Double(r) }; resizing = nil },
                   double: { withAnimation(Self.spring) { chatWidth = width >= maxWidth - 1 ? 350 : Double(maxWidth) } })
    }
}

/// A panel's left edge, the chat's or the web page's: drag to resize (the right edge stays put), double-click for the
/// other width. `changed` gets the pull, positive to the left.
struct ResizeEdge: View {
    let active: Bool
    let help: String
    let changed: (CGFloat) -> Void
    let ended: () -> Void
    let double: () -> Void
    @State private var hover = false

    var body: some View {
        Capsule().fill(hover || active ? Brand.accent.opacity(0.7) : .clear)
            .frame(width: 4, height: 56)
            .frame(width: 14)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .offset(x: -7)
            .onHover { inside in
                hover = inside
                if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
            }
            .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                .onChanged { changed(-$0.translation.width) }
                .onEnded { _ in ended() })
            .onTapGesture(count: 2, perform: double)
            .help(help)
    }
}

extension Notification.Name {
    /// The canvas was already where the Portfolio button would put it: the window's edge pulses instead.
    static let canvasEdgePulse = Notification.Name("canvasEdgePulse")
}

/// One quiet orange beat around the whole window's edge: the answer to «Portfolio» when the portfolio is
/// already in place. Only the border moves; the boards stay still.
struct EdgePulse: View {
    let trigger: Int

    private struct Wave {
        var opacity = 0.0
        /// How far the glow reaches into the window.
        var spread = 4.0
    }

    var body: some View {
        Color.clear
            .keyframeAnimator(initialValue: Wave(), trigger: trigger) { _, wave in
                // The window's own corners; the outer half of each stroke falls outside it.
                ZStack {
                    ConcentricRectangle().stroke(Brand.accent, lineWidth: 3)
                    ConcentricRectangle().stroke(Brand.accent.opacity(0.4), lineWidth: wave.spread * 2).blur(radius: wave.spread / 2)
                }
                .opacity(wave.opacity)
            } keyframes: { _ in
                KeyframeTrack(\.opacity) {
                    CubicKeyframe(0.8, duration: 0.25)
                    CubicKeyframe(0, duration: 0.75)
                }
                KeyframeTrack(\.spread) {
                    CubicKeyframe(10, duration: 1)
                }
            }
            .ignoresSafeArea()
            .allowsHitTesting(false)
    }
}

/// Moves the window's traffic lights into the rail's glass corner: with the title bar hidden they'd sit on its edge.
/// The title bar's container grows to hold them (a button outside it can't be clicked). AppKit lays the title bar out
/// again on resize and after full screen, so they're put back each time.
struct TrafficLights: NSViewRepresentable {
    /// The close button's middle, from the window's top-left.
    static let center = CGPoint(x: 30, y: 30)
    /// The folded rail's middle (window x): the three lights center on it. nil: the close button at `center`.
    var middle: CGFloat?
    /// The close button's middle when `middle` is nil; its double height is the title bar's.
    var center = TrafficLights.center

    func makeNSView(context: Context) -> NSView {
        let probe = Probe()
        probe.center = center
        return probe
    }
    func updateNSView(_ view: NSView, context: Context) {
        guard let probe = view as? Probe, probe.middle != middle else { return }
        probe.middle = middle
        probe.place(animated: true)
    }

    final class Probe: NSView {
        var middle: CGFloat?
        var center = TrafficLights.center
        private var observers: [NSObjectProtocol] = []
        /// The lights are sliding with the rail, or being put in place right now: their frame changes then are ours.
        /// (Without `placing` the close button's own change re-entered place() before the other two had moved, with a
        /// step read from half-moved buttons, until the stack ran out — a crash at launch, 2026-10-11.)
        private var sliding = false
        private var placing = false

        override func viewDidMoveToWindow() {
            observers.forEach(NotificationCenter.default.removeObserver)
            observers = []
            guard let window else { return }
            for name in [NSWindow.didResizeNotification, NSWindow.didExitFullScreenNotification] {
                observers.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.place() }
                })
            }
            // macOS 27 lays the title bar out again by itself, not only on resize: it shrank the container back to its
            // own 32 pt while the lights kept their place, so they stood half above the window. Its frame changing is
            // the one sign of that.
            if let container = window.standardWindowButton(.closeButton)?.superview?.superview {
                container.postsFrameChangedNotifications = true
                observers.append(NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: container, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.place() }
                })
            }
            // AppKit also lays the buttons out again without touching the container (the window coming back on screen
            // after a relaunch put them at their default x of 8, 31, 54, beside the rail's edge, 2026-10-11): whatever moves
            // the close button, they go back.
            if let close = window.standardWindowButton(.closeButton) {
                close.postsFrameChangedNotifications = true
                observers.append(NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: close, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { if self?.sliding == false { self?.place() } }
                })
            }
            DispatchQueue.main.async { self.place() }
        }

        func place(animated: Bool = false) {
            guard !placing else { return }
            placing = true
            defer { placing = false }
            guard let window, !window.styleMask.contains(.fullScreen),
                  let close = window.standardWindowButton(.closeButton),
                  let mini = window.standardWindowButton(.miniaturizeButton),
                  let zoom = window.standardWindowButton(.zoomButton),
                  let container = close.superview?.superview else { return }
            let height = 2 * center.y
            let frame = CGRect(x: container.frame.minX, y: window.frame.height - height, width: container.frame.width, height: height)
            // Only a change: setting it posts the frame change place() listens to.
            if container.frame != frame { container.frame = frame }
            let step = mini.frame.minX - close.frame.minX
            let first = middle.map { $0 - (2 * step + close.frame.width) / 2 } ?? center.x - close.frame.width / 2
            let buttons = [close, mini, zoom]
            let origins = buttons.enumerated().map { i, b in CGPoint(x: first + CGFloat(i) * step, y: (height - b.frame.height) / 2) }
            // Already there: moving them again would only post the frame change place() listens to.
            guard zip(buttons, origins).contains(where: { $0.frame.origin != $1 }) else { return }
            // Folding the rail slides them along with it.
            sliding = animated
            NSAnimationContext.runAnimationGroup { context in
                context.duration = animated ? 0.35 : 0
                for (button, origin) in zip(buttons, origins) {
                    if animated { button.animator().setFrameOrigin(origin) } else { button.setFrameOrigin(origin) }
                }
            } completionHandler: { [weak self] in
                MainActor.assumeIsolated {
                    self?.sliding = false
                    if animated { self?.place() }
                }
            }
        }
    }
}

/// An empty stretch that moves the window when dragged: the hidden title bar's job.
struct WindowDragArea: View {
    var body: some View {
        Color.clear.frame(maxWidth: .infinity, maxHeight: .infinity).contentShape(Rectangle()).gesture(WindowDragGesture())
    }
}

// MARK: Rail
/// A new version is out (Workspace.updateReady): by the logo until it's installed; a click opens Sparkle's window with
/// its notes and Install.
struct UpdateTag: View {
    let version: String
    var compact = false
    /// The folded rail's full width, at its foot by the profile (the owner, 2026-10-26: «лучше расположить вот… и на всю
    /// ширину»).
    var wide = false

    var body: some View {
        Button { Updater.shared.check() } label: {
            HStack(spacing: 4) {
                Image(systemName: "arrow.down.circle.fill")
                if !compact { Text(version).lineLimit(1) }
            }
            .fixedSize()
            .font(.ui(wide ? 11.5 : 10, .bold)).foregroundStyle(Brand.accentInk)
            .padding(.horizontal, compact ? 4 : 6)
            .frame(maxWidth: wide ? .infinity : nil).frame(height: wide ? 30 : 18)
            .background(Capsule().fill(Brand.accent.opacity(0.16)))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain).pointerStyle(.link)
        .help("ihatecrypto \(version) is out: see what's new and install it")
        .accessibilityLabel("Update to ihatecrypto \(version)")
    }
}

/// «DEV» by the logo of the development copy (`Flavor`), beside the working one: nobody types into the wrong window.
struct DevTag: View {
    var body: some View {
        Text(Flavor.tag).font(.ui(10, .bold)).foregroundStyle(Brand.accentInk)
            .padding(.horizontal, 6).frame(height: 18)
            .background(Capsule().fill(Brand.accent.opacity(0.16)))
            .help("The development copy: its own wallets, settings and data. The working ihatecrypto is a separate app.")
    }
}

struct StudioRail: View {
    @Environment(Workspace.self) private var ws
    @Environment(\.colorScheme) private var scheme
    @AppStorage("railCollapsed") private var collapsed = true
    /// The width while the edge is being dragged; nil when resting.
    @State private var dragWidth: CGFloat?
    @State private var edgeHover = false
    @State private var canvasesOpen = false
    @State private var balanceHover = false
    private var balanceHidden: Bool { BalanceEye.shared.hidden }
    /// The wallet button sits in the balance block but isn't the balance: over it «Updated … ago» stays hidden.
    @State private var walletHover = false
    /// The folded rail's coin was clicked: the balances are being read again.
    @State private var refreshing = false
    /// In full screen the traffic lights are up in the menu bar's strip: the rail's first row is free for the logo.
    @State private var fullScreen = false
    /// The folded strip leaves room on both sides of the traffic lights.
    static let narrowWidth: CGFloat = 92, wideWidth: CGFloat = 280

    private var width: CGFloat { dragWidth ?? (collapsed ? Self.narrowWidth : Self.wideWidth) }

    var body: some View {
        ZStack(alignment: .topLeading) {
            // Past the middle the wide rail shows; below it, the strip. The wide rail stands still: the widening edge
            // uncovers it and it fades in a beat later; folding, it fades out quickly. Nothing slides.
            if width < 170 {
                narrow.frame(width: Self.narrowWidth)
                    .transition(.opacity.combined(with: .scale(scale: 0.9, anchor: .leading)))
            } else {
                wide.frame(width: max(width, 220))
                    .transition(.asymmetric(insertion: .opacity.animation(.easeOut(duration: 0.25).delay(0.08)),
                                            removal: .opacity.animation(.easeOut(duration: 0.12))))
            }
        }
        .frame(width: width, alignment: .leading)
        .frame(maxHeight: .infinity, alignment: .top)
        .clipShape(RoundedRectangle(cornerRadius: Self.radius, style: .continuous))
        .animation(StudioView.spring, value: collapsed)
        // The rail is built again when Settings closes, possibly already in full screen: ask the window, then follow it.
        .onAppear { fullScreen = NSApp.windows.contains { $0.styleMask.contains(.fullScreen) } }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willEnterFullScreenNotification)) { _ in fullScreen = true }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willExitFullScreenNotification)) { _ in fullScreen = false }
        // Glass picks its own light or dark look from what's behind it; the rail's text follows the window instead.
        .environment(\.colorScheme, scheme)
        // Liquid Glass: the rail floats over the canvas's ground.
        .measuredGlass(.regular.tint(Brand.glassTint), in: RoundedRectangle(cornerRadius: Self.radius, style: .continuous))
        .overlay(alignment: .trailing) {
            // The edge: drag to collapse or expand; a spring settles it on the nearer width. It shows under the pointer.
            Capsule().fill(edgeHover || dragWidth != nil ? Brand.accent.opacity(0.8) : .clear)
                .frame(width: 3, height: 56)
                .frame(maxHeight: .infinity)
                .frame(width: 12)
                .offset(x: 6)
                .contentShape(Rectangle())
                .onHover { inside in
                    edgeHover = inside
                    if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() }
                }
                .gesture(DragGesture(minimumDistance: 2, coordinateSpace: .global)
                    .onChanged { v in
                        let base = collapsed ? Self.narrowWidth : Self.wideWidth
                        withAnimation(.interactiveSpring) { dragWidth = min(340, max(Self.narrowWidth, base + v.translation.width)) }
                    }
                    .onEnded { v in
                        let base = collapsed ? Self.narrowWidth : Self.wideWidth
                        let landing = base + v.predictedEndTranslation.width
                        withAnimation(StudioView.spring) {
                            collapsed = landing < (Self.narrowWidth + Self.wideWidth) / 2
                            dragWidth = nil
                        }
                    })
                .help("Drag to collapse or expand the wallet")
        }
    }

    static let radius: CGFloat = 24

    /// The first row's top: its middle lines up with the traffic lights (the rail starts 10 below the window's top).
    private let lightsTop = TrafficLights.center.y - 10 - Self.lightsRow / 2
    static let lightsRow: CGFloat = 28

    private var collapseButton: some View {
        Button { withAnimation(StudioView.spring) { collapsed = true } } label: { Image(systemName: "sidebar.left") }
            .buttonStyle(.hoverIcon).help("Collapse the sidebar")
    }

    /// Top padding: under the traffic lights' row in a window, a plain margin in full screen.
    private var top: CGFloat { fullScreen ? 14 : lightsTop }

    private var wide: some View {
        VStack(alignment: .leading, spacing: 16) {
            // The traffic lights are on this row; its empty part drags the window, as a title bar would.
            if !fullScreen {
                HStack {
                    WindowDragArea()
                    collapseButton
                }
                .frame(height: Self.lightsRow)
            }
            HStack(spacing: 8) {
                WhatsNewLogo()
                Text("ihatecrypto").font(.brand(19)).lineLimit(1).fixedSize()
                if Flavor.dev { DevTag() }
                // Beside «DEV» only the arrow fits.
                if let version = ws.updateReady { UpdateTag(version: version, compact: Flavor.dev) }
                Spacer(minLength: 0)
                if fullScreen { collapseButton }
            }
            // The balance and its buttons stay put; only the canvases scroll, in the room left above the profile.
            // Places, positions, closed trades and favorites are on the canvas.
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text("Balance").font(.ui(12)).foregroundStyle(.secondary)
                    // Before the refresh: the freshness line that opens after it would push the eye along.
                    Button { BalanceEye.shared.hidden.toggle() } label: { Image(systemName: balanceHidden ? "eye.slash" : "eye") }
                        .buttonStyle(HoverIconStyle(size: 18))
                        .help(balanceHidden ? "Show the balance" : "Hide the balance")
                        .accessibilityLabel(balanceHidden ? "Show the balance" : "Hide the balance")
                    BalanceFreshness(hovering: balanceHover && !walletHover)
                    Spacer(minLength: 4)
                    // Keeps its whole name: the freshness line gives way, not «Led…».
                    WalletChip().layoutPriority(1).onHover { walletHover = $0 }
                }
                // Both wallets: what went to Automation is still the user's (AutomationViews.swift).
                BalanceAmount(font: .ui(32, .semibold), mask: "$•••••", skeleton: CGSize(width: 150, height: 34)).tracking(-0.6)
                if ws.balanceReady, let auto = ws.automationUSD, auto >= Decimal(string: "0.01")!, let main = ws.total {
                    AutomationSplit(main: main, automation: auto)
                }
                if !ws.positions.isEmpty, !balanceHidden {
                    Text("\(signed(ws.positionsPnl)) in positions").font(.ui(12, .semibold))
                        .foregroundStyle(ws.positionsPnl < 0 ? Brand.loss : Brand.profit)
                }
            }
            .padding(.horizontal, 4)
            .onHover { balanceHover = $0 }
            // The loaded balance fades in instead of popping up.
            .animation(.easeOut(duration: 0.35), value: ws.balanceReady)
            RailActions()
            CanvasList()
            ProfileMenu(compact: false)
        }
        .padding(.horizontal, 14).padding(.top, top).padding(.bottom, 12)
    }

    /// Folded: a strip of round glass buttons, the logo on top and the profile at the bottom.
    private var narrow: some View {
        VStack(spacing: 8) {
            // The traffic lights' row, which also drags the window.
            if !fullScreen { WindowDragArea().frame(height: Self.lightsRow) }
            // The coin and the balance, above the orange Top up button: a click reads the balances again. The coin works
            // on it and the number fades until it's done; nothing else moves.
            Button {
                refreshing = true
                Task { await ws.refresh(); refreshing = false }
            } label: {
                VStack(spacing: 8) {
                    BrandLogo(size: 34, mood: refreshing ? .thinking : .idle, animated: true)
                    if Flavor.dev { DevTag() }
                    VStack(spacing: 1) {
                        BalanceAmount(font: .ui(11, .semibold), mask: "$•••", skeleton: CGSize(width: 44, height: 11)).lineLimit(1).minimumScaleFactor(0.7)
                        if !ws.positions.isEmpty, !balanceHidden {
                            Text(signed(ws.positionsPnl)).font(.ui(10, .semibold))
                                .foregroundStyle(ws.positionsPnl < 0 ? Brand.loss : Brand.profit)
                        }
                    }
                    .opacity(refreshing ? 0.4 : 1)
                    .animation(.easeOut(duration: 0.2), value: refreshing)
                }
                .padding(.vertical, 6).frame(width: 76)
            }
            .buttonStyle(LitBlock())
            .disabled(refreshing)
            .help(refreshing ? "Updating the balance…" : "Balance — click to update it")
            WalletChip(compact: true)
            Divider().frame(width: 32)
            RailActions(compact: true)
            Divider().frame(width: 32)
            Button("Canvases") { canvasesOpen.toggle() }
                .buttonStyle(RailRound(icon: "rectangle.stack", title: "Canvases", on: canvasesOpen)).help("Canvases")
                .popover(isPresented: $canvasesOpen, arrowEdge: .trailing) {
                    CanvasList { canvasesOpen = false }.frame(width: 280, height: 380).padding(14)
                }
            Button("New canvas") { withAnimation(StudioView.spring) { ws.newCanvas() } }
                .buttonStyle(RailRound(icon: "plus", title: "New", dashed: true)).help("New canvas").disabled(ws.busy)
            Spacer(minLength: 0)
            if let version = ws.updateReady { UpdateTag(version: version, wide: true).frame(width: 76) }
            Button("Expand the sidebar") { withAnimation(StudioView.spring) { collapsed = false } }
                .buttonStyle(RailRound(icon: "sidebar.left", plain: true)).help("Expand the sidebar")
            ProfileMenu(compact: true)
        }
        .padding(.top, top).padding(.bottom, 12)
    }

    /// Every section of the rail but the canvases shows this many rows, then «Show N more».
    static let railLimit = 4
}

/// «Show 3 more» under a rail list cut to its first few rows; «Show less» folds it back.
struct RailMore: View {
    let total: Int
    @Binding var expanded: Bool

    var body: some View {
        if total > StudioRail.railLimit {
            Button { withAnimation(.snappy) { expanded.toggle() } } label: {
                Label(expanded ? "Show less" : "Show \(total - StudioRail.railLimit) more", systemImage: expanded ? "chevron.up" : "chevron.down")
                    .font(.ui(10, .semibold)).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 8).padding(.vertical, 4)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain).pointerStyle(.link)
        }
    }
}

/// A grey block with a slow light sweep: where a number is about to appear, so nothing pops up from nowhere.
struct Skeleton: View {
    var width: CGFloat? = nil
    var height: CGFloat = 12
    var radius: CGFloat = 6

    var body: some View {
        RoundedRectangle(cornerRadius: radius)
            .fill(Color.primary.opacity(0.07))
            .overlay { LightSweep(radius: radius).allowsHitTesting(false) }
            .frame(width: width, height: height)
            .accessibilityElement().accessibilityLabel("Loading")
    }
}

/// The balance wherever it shows (sidebar, portfolio, menu bar): both wallets once every read is in (`balanceReady`).
/// Until then this wallet's last full balance, faded under the skeleton's light; the skeleton when it never had one.
struct BalanceAmount: View {
    @Environment(Workspace.self) private var ws
    let font: Font
    /// What stands for the amount while the balance is hidden; nil = its digits as dots.
    var mask: String? = nil
    var skeleton = CGSize(width: 110, height: 22)

    private func shown(_ usd: Decimal?) -> String {
        guard BalanceEye.shared.hidden else { return usd.map(Fmt.usd) ?? "—" }
        return mask ?? Fmt.dots(usd.map(Fmt.usd) ?? "—")
    }

    var body: some View {
        if ws.balanceReady {
            Text(shown(ws.balance)).font(font).monospacedDigit()
        } else if let last = ws.lastBalance[ws.activeWalletID ?? ""] {
            Text(shown(last)).font(font).monospacedDigit().opacity(0.4)
                .overlay { LightSweep(radius: 6).allowsHitTesting(false) }
                .help("Updating: the last full balance until every network answers")
                .accessibilityLabel("Updating, last balance \(shown(last))")
        } else {
            Skeleton(width: skeleton.width, height: skeleton.height, radius: skeleton.height / 3)
        }
    }
}

// Motion drawn by Core Animation. A SwiftUI repeatForever or TimelineView lays out and draws the whole window again on
// every frame (≈ 13 ms of main thread with the canvas, the chat and the rail in one window, measured), so a pulse at
// 120 Hz kept the main thread full and everything else stuttered. A layer animation runs in the render server: the
// app does nothing per frame. Each view lets clicks through and sets its colors again when the window turns light or dark.
// Not on the panned canvas for long: an AppKit view that moves with a pan makes AppKit work out the window's drag regions
// on every step, asking SwiftUI's focus system each time (RingPulse, RouteCards.swift). The skeletons there last seconds.

/// A rounded border whose opacity breathes, 0.9 s each way.
struct BreathingBorder: NSViewRepresentable {
    let color: Color
    let radius: CGFloat
    var lineWidth: CGFloat = 1.5
    var low: Float = 0.35
    var high: Float = 1
    /// How many breaths before it holds still at `high` (a half one ends there); endless by default.
    var breaths: Float = .infinity

    func makeNSView(context: Context) -> Layered { Layered() }
    func updateNSView(_ view: Layered, context: Context) {
        view.breaths = breaths
        view.color = NSColor(color)
        view.radius = radius
        view.lineWidth = lineWidth
        view.range = (low, high)
        view.needsLayout = true
    }

    final class Layered: NSView {
        var color = NSColor.clear, radius: CGFloat = 0, lineWidth: CGFloat = 1, range: (Float, Float) = (0, 1)
        var breaths: Float = .infinity
        private let edge = CALayer()
        private var animated: (Float, Float)?

        override init(frame: NSRect) {
            super.init(frame: frame)
            wantsLayer = true
            layer?.addSublayer(edge)
        }
        required init?(coder: NSCoder) { nil }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidChangeEffectiveAppearance() { needsLayout = true }
        override func viewDidMoveToWindow() { animated = nil; needsLayout = true }

        override func layout() {
            super.layout()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            edge.frame = bounds
            edge.cornerRadius = radius
            edge.cornerCurve = .continuous
            edge.borderWidth = lineWidth
            effectiveAppearance.performAsCurrentDrawingAppearance { edge.borderColor = color.cgColor }
            edge.opacity = range.1
            CATransaction.commit()
            // A layer leaving its window loses its animations: it gets them again when it's back.
            guard window != nil, animated.map({ $0 != range }) ?? true else { return }
            let breath = CABasicAnimation(keyPath: "opacity")
            breath.fromValue = range.0
            breath.toValue = range.1
            breath.duration = 0.9
            breath.autoreverses = true
            breath.repeatCount = breaths
            breath.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            edge.add(breath, forKey: "breath")
            animated = range
        }
    }
}

/// The skeleton's slow light sweep, left to right every 1.3 s.
private struct LightSweep: NSViewRepresentable {
    let radius: CGFloat

    func makeNSView(context: Context) -> Layered { Layered() }
    func updateNSView(_ view: Layered, context: Context) { view.radius = radius; view.needsLayout = true }

    final class Layered: NSView {
        var radius: CGFloat = 0
        private let light = CAGradientLayer()
        private var animatedWidth: CGFloat?

        override init(frame: NSRect) {
            super.init(frame: frame)
            wantsLayer = true
            layer?.masksToBounds = true
            light.startPoint = CGPoint(x: 0, y: 0.5)
            light.endPoint = CGPoint(x: 1, y: 0.5)
            layer?.addSublayer(light)
        }
        required init?(coder: NSCoder) { nil }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidChangeEffectiveAppearance() { needsLayout = true }
        override func viewDidMoveToWindow() { animatedWidth = nil; needsLayout = true }

        override func layout() {
            super.layout()
            let w = bounds.width
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            layer?.cornerRadius = radius
            light.bounds = CGRect(x: 0, y: 0, width: w * 0.6, height: bounds.height)
            light.position = CGPoint(x: -w * 0.3, y: bounds.midY)
            effectiveAppearance.performAsCurrentDrawingAppearance {
                let tint = NSColor.labelColor.withAlphaComponent(0.09).cgColor
                light.colors = [NSColor.clear.cgColor, tint, NSColor.clear.cgColor]
            }
            CATransaction.commit()
            guard window != nil, w > 0, animatedWidth != w else { return }
            let sweep = CABasicAnimation(keyPath: "position.x")
            sweep.fromValue = -w * 0.3
            sweep.toValue = w * 1.3
            sweep.duration = 1.3
            sweep.repeatCount = .infinity
            light.add(sweep, forKey: "sweep")
            animatedWidth = w
        }
    }
}

/// Three dots breathing in turn, 7 pt with 5 pt between them.
struct BreathingDots: NSViewRepresentable {
    static let size = CGSize(width: 31, height: 7)

    func makeNSView(context: Context) -> Layered { Layered() }
    func updateNSView(_ view: Layered, context: Context) {}

    final class Layered: NSView {
        private let row = CAReplicatorLayer(), dot = CALayer()
        private var animating = false

        override init(frame: NSRect) {
            super.init(frame: frame)
            wantsLayer = true
            dot.frame = CGRect(x: 0, y: 0, width: 7, height: 7)
            dot.cornerRadius = 3.5
            dot.opacity = 0.3
            row.instanceCount = 3
            row.instanceTransform = CATransform3DMakeTranslation(12, 0, 0)
            row.instanceDelay = 0.18
            row.addSublayer(dot)
            layer?.addSublayer(row)
        }
        required init?(coder: NSCoder) { nil }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }
        override func viewDidChangeEffectiveAppearance() { needsLayout = true }
        override func viewDidMoveToWindow() { animating = false; needsLayout = true }

        override func layout() {
            super.layout()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            row.frame = bounds
            effectiveAppearance.performAsCurrentDrawingAppearance { dot.backgroundColor = NSColor.secondaryLabelColor.cgColor }
            CATransaction.commit()
            guard window != nil, !animating else { return }
            let breath = CABasicAnimation(keyPath: "opacity")
            breath.fromValue = 0.3
            breath.toValue = 1
            breath.duration = 0.63
            breath.autoreverses = true
            breath.repeatCount = .infinity
            breath.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
            dot.add(breath, forKey: "breath")
            animating = true
        }
    }
}

/// The mascot's moving face: frames Core Animation plays once (`BrandLogo.film`), with no SwiftUI update a frame.
struct MascotFilm: NSViewRepresentable {
    let frames: [CGImage]

    func makeNSView(context: Context) -> Layered { Layered(frames: frames) }
    func updateNSView(_ view: Layered, context: Context) {}

    final class Layered: NSView {
        private let film = CALayer()
        private let frames: [CGImage]

        init(frames: [CGImage]) {
            self.frames = frames
            super.init(frame: .zero)
            wantsLayer = true
            film.contents = frames.first
            film.contentsGravity = .resize
            layer?.addSublayer(film)
        }
        required init?(coder: NSCoder) { nil }
        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            guard window != nil, film.animation(forKey: "film") == nil, frames.count > 1 else { return }
            let play = CAKeyframeAnimation(keyPath: "contents")
            play.values = frames
            play.calculationMode = .discrete
            play.duration = Double(frames.count) / BrandLogo.filmFPS
            play.fillMode = .forwards
            play.isRemovedOnCompletion = false
            film.add(play, forKey: "film")
        }

        override func layout() {
            super.layout()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            film.frame = bounds
            CATransaction.commit()
        }
    }
}

/// «+$0,48» / «−$0,25».
func signed(_ d: Decimal) -> String { d < 0 ? Fmt.usd(d) : "+" + Fmt.usd(d) }

/// The eye by «Balance» (the owner, 2026-10-15): closed, the wallet's amounts show as dots on the rail, in the portfolio
/// and on the canvas's boards. A move's own card keeps its numbers: what gets signed is always seen.
@Observable final class BalanceEye {
    static let shared = BalanceEye()
    var hidden = UserDefaults.standard.bool(forKey: "balanceHidden") {
        didSet { UserDefaults.standard.set(hidden, forKey: "balanceHidden") }
    }
}

/// An amount as shown: while the eye is closed every number in it but a percentage is dots («worth ••• · +4.2%»).
/// Read in a body, it redraws the view when the eye turns. Only for what's drawn: commands and the model's context
/// keep their numbers.
func masked(_ text: String) -> String {
    BalanceEye.shared.hidden ? Fmt.dots(text) : text
}

/// ihatecrypto's logo: an orange coin with a ridged edge that rolls its eyes («even crypto hates crypto»). The same
/// coin is the assistant's face in the chat, and its expression says what it's doing.
struct BrandLogo: View {
    /// idle: the brand's eye-roll, still. The rest are the assistant's states from the brand board: waiting for Confirm,
    /// building the route, done, error; and tuning (brows up, looking down at an open menu or Settings), listening;
    /// asleep, eyes shut, on the lock screen.
    enum Mood { case idle, waiting, thinking, done, error, tuning, listening, asleep }
    var size: CGFloat = 34
    var mood: Mood = .idle
    /// The chat's avatar moves: the eyes roll while it waits for Confirm, the ridge turns and the eyes look around while
    /// it builds the route, a hop when it's done, a shake on an error. The rail's logo stays still.
    var animated = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    /// While the window is in the background nobody sees it move, so it holds still.
    @Environment(\.appearsActive) private var windowActive
    @Environment(\.displayScale) private var displayScale
    /// Bumped on every change of mood: it plays the hop or the shake.
    @State private var kick = 0
    /// A new mood moves for a few seconds, then the coin rests: a card can wait minutes for Confirm, and a model minutes
    /// for its answer (the dots go on showing that), with every frame redrawing the window.
    @State private var moving = true

    private struct Pose { var scale = 1.0, x = 0.0, y = 0.0 }

    var body: some View {
        let live = animated && !reduceMotion && moving && windowActive && [.waiting, .thinking, .listening].contains(mood)
        // The moving face is a film Core Animation plays (`MascotFilm`): through a TimelineView each of its 30 frames a
        // second redrew the whole window, ≈ 20 % of the main thread while a model answered (measured 2026-10-16).
        Group {
            if Render.classic {
                TimelineView(.animation(minimumInterval: 1.0 / 30, paused: !live)) { timeline in
                    let t = live ? timeline.date.timeIntervalSinceReferenceDate : 0
                    Canvas { context, s in draw(&context, s, t: t) }
                }
            } else {
                Canvas { context, s in draw(&context, s, t: 0) }
                    .opacity(live ? 0 : 1)
                    .overlay { if live { MascotFilm(frames: Self.film(mood: mood, size: size, scale: displayScale)).id(mood) } }
            }
        }
        .frame(width: size, height: size)
        .keyframeAnimator(initialValue: Pose(), trigger: kick) { content, pose in
            content.scaleEffect(pose.scale).offset(x: pose.x * size / 30, y: pose.y * size / 30)
        } keyframes: { _ in
            KeyframeTrack(\.scale) {
                CubicKeyframe(mood == .done ? 1.2 : mood == .error ? 1 : 1.1, duration: 0.14)
                SpringKeyframe(1, duration: 0.5, spring: .bouncy)
            }
            KeyframeTrack(\.y) {
                CubicKeyframe(mood == .done ? -7 : 0, duration: 0.16)
                SpringKeyframe(0, duration: 0.5, spring: .bouncy)
            }
            KeyframeTrack(\.x) {
                let a = mood == .error ? 4.0 : 0
                for side in [-1.0, 1, -1, 1, -0.5] { CubicKeyframe(a * side, duration: 0.06) }
                CubicKeyframe(0, duration: 0.06)
            }
        }
        .onChange(of: mood) { if animated && !reduceMotion { kick += 1 } }
        .task(id: mood) {
            moving = true
            try? await Task.sleep(for: .seconds(4))
            if !Task.isCancelled { moving = false }
        }
        .accessibilityLabel("ihatecrypto")
    }

    /// The few seconds a mood moves, 30 frames a second, drawn once (one sheet, cut into frames) and kept.
    private static var films: [String: [CGImage]] = [:]
    static let filmFPS = 30.0

    static func film(mood: Mood, size: CGFloat, scale: CGFloat) -> [CGImage] {
        let key = "\(mood) \(size) \(scale)"
        if let film = films[key] { return film }
        let count = 120, columns = 12
        let logo = BrandLogo(size: size, mood: mood)
        let sheet = Canvas { context, _ in
            for i in 0..<count {
                var cell = context
                cell.translateBy(x: CGFloat(i % columns) * size, y: CGFloat(i / columns) * size)
                logo.draw(&cell, CGSize(width: size, height: size), t: Double(i) / filmFPS)
            }
        }
        .frame(width: size * CGFloat(columns), height: size * CGFloat((count + columns - 1) / columns))
        let renderer = ImageRenderer(content: sheet)
        renderer.scale = scale
        guard let image = renderer.cgImage else { return [] }
        let px = size * scale
        let film = (0..<count).compactMap { i in
            image.cropping(to: CGRect(x: CGFloat(i % columns) * px, y: CGFloat(i / columns) * px, width: px, height: px))
        }
        films[key] = film
        return film
    }

    /// Drawn on a 140-point grid, the same as the brand boards; `t` (seconds) moves the eyes, mouth and ridge.
    private func draw(_ context: inout GraphicsContext, _ s: CGSize, t: Double) {
        let k = s.width / 140
        func p(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: x * k, y: y * k) }
        func circle(_ x: CGFloat, _ y: CGFloat, _ r: CGFloat) -> Path { Path(ellipseIn: CGRect(x: (x - r) * k, y: (y - r) * k, width: 2 * r * k, height: 2 * r * k)) }
        let ink = GraphicsContext.Shading.color(Brand.onAccent)
        let line = StrokeStyle(lineWidth: 8 * k, lineCap: .round, lineJoin: .round)
        context.fill(circle(70, 70, 64), with: .color(Brand.accent))
        // The ridged edge, a dashed ring over the rim that turns while the route is being built, and the inner rim.
        let spin = mood == .thinking ? CGFloat(t * 18) : 0
        context.stroke(circle(70, 70, 62), with: ink, style: StrokeStyle(lineWidth: 9 * k, dash: [7 * k, 7 * k], dashPhase: spin * k))
        context.stroke(circle(70, 70, 52), with: ink, lineWidth: 3.5 * k)
        var face = Path()
        switch mood {
        case .idle, .waiting, .thinking:
            // Where the pupils look, as an angle over the eye: up and to the right is the eye-roll. Waiting, they
            // roll back and forth over the top; building the route, they look around up and to the left.
            let a = switch mood {
            case .waiting: 1.1 + 0.55 * sin(t * 2.2)
            case .thinking: 2.25 + 0.5 * sin(t * 3.1)
            default: 1.1
            }
            for x in [50.0, 90.0] {
                context.fill(circle(x, 62, 17), with: .color(.white))
                context.stroke(circle(x, 62, 17), with: ink, lineWidth: 5 * k)
                context.fill(circle(x + 9 * cos(a), 62 - 9 * sin(a), 8), with: ink)
            }
            if mood == .thinking {
                let w = 3 * sin(t * 6)
                face.move(to: p(52, 98))
                face.addQuadCurve(to: p(64, 98), control: p(58, 92 - w))
                face.addQuadCurve(to: p(76, 98), control: p(70, 104 + w))
                face.addQuadCurve(to: p(88, 98), control: p(82, 92 - w))
            } else {
                face.move(to: p(54, 98)); face.addQuadCurve(to: p(86, 96), control: p(70, 92))
            }
        case .tuning, .listening:
            // Tuning: brows up, the eyes down and to the left at the menu, a small «o». Listening: eyes ahead, the «o»
            // opening and closing as if taking the words in.
            let (dx, dy): (CGFloat, CGFloat) = mood == .tuning ? (-3, 6) : (0, 0)
            for x in [50.0, 90.0] {
                context.fill(circle(x, 62, 17), with: .color(.white))
                context.stroke(circle(x, 62, 17), with: ink, lineWidth: 5 * k)
                context.fill(circle(x + dx, 62 + dy, 8), with: ink)
            }
            if mood == .tuning {
                face.move(to: p(34, 36)); face.addQuadCurve(to: p(62, 36), control: p(48, 28))
                face.move(to: p(78, 36)); face.addQuadCurve(to: p(106, 36), control: p(92, 28))
            }
            let r = mood == .tuning ? 6 : 6 + 2.5 * abs(sin(t * 5))
            context.stroke(circle(70, 97, r), with: ink, lineWidth: 7 * k)
        case .done:
            face.move(to: p(36, 64)); face.addQuadCurve(to: p(64, 64), control: p(50, 48))
            face.move(to: p(76, 64)); face.addQuadCurve(to: p(104, 64), control: p(90, 48))
            face.move(to: p(50, 86)); face.addQuadCurve(to: p(90, 86), control: p(70, 106))
        case .asleep:
            face.move(to: p(37, 62)); face.addQuadCurve(to: p(63, 62), control: p(50, 75))
            face.move(to: p(77, 62)); face.addQuadCurve(to: p(103, 62), control: p(90, 75))
            face.move(to: p(60, 98)); face.addLine(to: p(80, 98))
        case .error:
            for (a, b) in [(p(38, 50), p(60, 72)), (p(60, 50), p(38, 72)), (p(80, 50), p(102, 72)), (p(102, 50), p(80, 72))] {
                face.move(to: a); face.addLine(to: b)
            }
            face.move(to: p(54, 98)); face.addLine(to: p(86, 98))
        }
        context.stroke(face, with: ink, style: line)
    }
}

/// The asset of a position: its logo, or a drawn sign for oil and other HIP-3 markets.
struct AssetBadge: View {
    let coin: String
    var size: CGFloat = 30
    var dark = false

    var body: some View {
        if let logo = TokenLogo.name(forAsset: coin) {
            TokenLogo(name: logo, size: size)
        } else {
            RoundedRectangle(cornerRadius: size * 0.3).fill(dark ? Brand.hl : Brand.hlChip).frame(width: size, height: size)
                .overlay(Image(systemName: Self.sign(coin)).font(.system(size: size * 0.45, weight: .semibold)).foregroundStyle(Self.tint(coin)))
        }
    }
}

extension AssetBadge {
    /// Markets without a brand: oil a drop, metals a bar, indices bars, anything else a chart line.
    static func sign(_ coin: String) -> String {
        let t = coin.split(separator: ":").last.map(String.init)?.uppercased() ?? coin
        if ["CL", "BRENTOIL", "NATGAS"].contains(t) { return t == "NATGAS" ? "flame.fill" : "drop.fill" }
        if ["GOLD", "SILVER", "PLATINUM", "PALLADIUM", "COPPER"].contains(t) { return "rectangle.stack.fill" }
        if t.contains("500") || t.contains("100") || t.hasPrefix("SOX") { return "chart.bar.fill" }
        return "chart.line.uptrend.xyaxis"
    }

    static func tint(_ coin: String) -> Color {
        switch coin.split(separator: ":").last.map(String.init)?.uppercased() ?? coin {
        case "GOLD": Color(hex: 0xF5C542)
        case "SILVER", "PLATINUM", "PALLADIUM": Color(hex: 0xD9DEE7)
        case "COPPER": Color(hex: 0xE08A5A)
        default: Brand.hlMint
        }
    }
}

struct PositionChip: View {
    @Environment(Workspace.self) private var ws
    let position: HyperliquidAccount.Position

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                AssetBadge(coin: position.coin, size: 22)
                Text("\(position.size > 0 ? "Long" : "Short") \(IntentParser.pretty(position.coin))").font(.ui(13, .semibold)).lineLimit(1)
                Spacer()
                Text(masked(signed(position.unrealizedPnl))).font(.ui(13, .bold)).monospacedDigit()
                    .foregroundStyle(position.unrealizedPnl < 0 ? Color(hex: 0xFF9C8F) : Brand.hlMint)
            }
            Text(ws.positionSummary(position)).font(.ui(10)).foregroundStyle(Brand.hlSoft).lineLimit(2)
        }
        .foregroundStyle(.white)
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 14).fill(Brand.hl))
    }
}

/// Positions closed in the last week with their result after fees, on the History board; a click opens them in the
/// full history.
struct ClosedTrades: View {
    let stories: [TradeHistory.Story]
    @State private var all = false

    var body: some View {
        let total = stories.reduce(Decimal(0)) { $0 + $1.realized }
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Closed · 7 days").font(.ui(10)).foregroundStyle(.secondary)
                Spacer()
                Text(masked(signed(total))).font(.ui(10, .semibold)).monospacedDigit().foregroundStyle(total < 0 ? Brand.loss : Brand.profit)
            }
            ForEach(all ? stories : Array(stories.prefix(StudioRail.railLimit))) { s in
                Button {
                    UserDefaults.standard.set("positions", forKey: "historyTab")
                    withAnimation(.snappy) { UserDefaults.standard.set("history", forKey: "canvasMode") }
                } label: {
                    HStack(spacing: 8) {
                        AssetBadge(coin: s.coin, size: 20)
                        VStack(alignment: .leading, spacing: 0) {
                            Text("\(s.long ? "Long" : "Short") \(IntentParser.pretty(s.coin))").font(.ui(12, .semibold)).lineLimit(1)
                            Text(s.closed.map(Fmt.date) ?? "").font(.ui(10)).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(masked(signed(s.realized))).font(.ui(12, .bold)).monospacedDigit()
                            .foregroundStyle(s.realized < 0 ? Brand.loss : Brand.profit)
                    }
                    .padding(.vertical, 3).contentShape(Rectangle())
                }
                .buttonStyle(.plain).pointerStyle(.link).help("Open in History → Positions")
            }
            RailMore(total: stories.count, expanded: $all)
        }
    }
}

// MARK: Canvas

/// What the canvas shows: the portfolio and the plan beside it, a trade plan as route cards the coin cursor lays out.
/// The pan and zoom around it are CanvasViewport's, and this view takes nothing from it, so what it reads from the
/// workspace redraws the canvas and nothing else does.
struct StudioCanvas: View {
    @Environment(Workspace.self) private var ws
    /// The widest route on the canvas (RouteWidth): the plan column widens for a row of cards.
    @State private var routeWidth: CGFloat = 0

    /// Portfolio frame + gap + plan + leading padding.
    static let contentWidth: CGFloat = 28 + 270 + 36 + 470 + 24

    var body: some View {
        // The plan once per pass: the blocks, the status and the lines all read it.
        let nodes = nodes
        let chartAt = ws.chartPlace(tradePlan: !nodes.isEmpty)
        HStack(alignment: .top, spacing: 36) {
            // Right-click on any block: the chat with it attached (BlockMenu); several blocks can go into one message.
            StudioPortfolio().blockMenu("Portfolio", radius: 26)
            // The rail's Schedule button puts it here, beside the portfolio; a row opens its page.
            if ws.showSchedule { ScheduleFrame().blockMenu("Schedule", radius: 26).transition(.opacity) }
            plan(nodes, chartAt: chartAt).frame(width: max(470, routeWidth), alignment: .topLeading)
                .onPreferenceChange(RouteWidth.self) { width in MainActor.assumeIsolated { routeWidth = width } }
            if chartAt == .beside, let chart = ws.chart {
                ChartBoard(state: chart).blockMenu("Chart · \(IntentParser.pretty(chart.coin))")
            }
        }
        .padding(.leading, 28).padding(.top, 24)
        // Full height even when the canvas is shorter: blocks never squeeze their text; drag to see the rest.
        .fixedSize(horizontal: false, vertical: true)
        // Inside the zoom: a pan moves none of the frames measured in it.
        .coordinateSpace(.named("studio"))
    }

    // MARK: Plan

    private var hasRoute: Bool { ws.intent != TradeIntent() || ws.managesPosition }
    /// A reopened canvas, or this one after its money move: its board as it was.
    private var board: CanvasSession? { ws.savedBoard ?? ws.finishedCanvas }
    /// Once the plan is on the exchange, the canvas shows the position itself instead of the old route.
    private var nodes: [StudioNode] { hasRoute && !ws.managesPosition ? StudioNode.build(ws) : [] }

    private func plan(_ nodes: [StudioNode], chartAt: ChartPlace?) -> some View {
        // A watched token opened from Favorites sits to the right of the list, not above it (the owner, 2026-10-16:
        // «открывай справа сбоку, а не снизу»).
        let beside = ws.showFavorites ? ws.detail.flatMap { place in
            TokenBoard.parse(place).flatMap { p in ws.favoriteTokens.contains { $0.chain == p.chain && $0.address.lowercased() == p.address.lowercased() } ? place : nil }
        } : nil
        return VStack(alignment: .leading, spacing: 14) {
            // A place opened from the rail is its own block above the plan or the position: outside their frame,
            // covering nothing.
            // A coin's chart opened from its row goes where any chart goes (chartPlace).
            if let place = ws.detail, place != beside, place != "chart" || chartAt == .column {
                PlaceDetailBoard(place: place)
                    .blockMenu(ws.placeTitle(place))
                    .padding(.bottom, 10)
                    .transition(.opacity)
            }
            // A scheduled item's own canvas: its card first; the chat on it changes it.
            if let key = ws.currentScheduleKey {
                ScheduleItemBoard(key: key).blockMenu(ws.scheduleEntries.first { $0.id == key }?.title ?? "Scheduled task").padding(.bottom, 10)
            }
            if let note = ws.noteCard { NoteBoard(note: note).blockMenu(note.folder.map { "Notes · \($0)" } ?? "Notes", radius: 26).padding(.bottom, 10) }
            // The rail's buttons put their boards here too.
            if ws.showReceive { ReceiveBoard().blockMenu("Top up", radius: 26) }
            LedgerBoard().blockMenu("Ledger", radius: 26)
            if ws.showFavorites {
                HStack(alignment: .top, spacing: 36) {
                    FavoritesBoard().blockMenu("Favorites", radius: nil)
                    if let beside { PlaceDetailBoard(place: beside).blockMenu(ws.placeTitle(beside)).transition(.opacity) }
                }
                .transition(.opacity)
            }
            if ws.showHistory { HistoryBoard().blockMenu("History", radius: 22).transition(.opacity) }
            // A move waiting for its signature or running has its own title and status: no «Plan · waiting» over it.
            if chartAt == .plan, let chart = ws.chart {
                // A trade plan of the chart's coin is drawn on its chart, its Cancel and Confirm on top (the owner's pick C).
                PlanChartBoard(state: chart) { planHeader(nodes) }.blockMenu("Chart · \(IntentParser.pretty(chart.coin))")
            } else if hasRoute || ws.allocationPreview != nil || (ws.pendingAction == nil && !ws.moveRunning && ws.currentScheduleKey == nil) {
                planHeader(nodes)
            }
            // Only this canvas's own position, or one the user tapped (a tap opens its chart), not every open one:
            // its board with the controls first, the chart beside it (chartPlace).
            let positions = nodes.isEmpty ? ws.positions.filter { $0.coin == ws.chart?.coin || (ws.managesPosition && $0.coin == ws.intent.asset) } : []
            ForEach(positions, id: \.coin) { PositionBoard(position: $0).blockMenu("Position · \(IntentParser.pretty($0.coin))", radius: 22) }
            if chartAt == .column, let chart = ws.chart, ws.detail != "chart" { ChartBoard(state: chart).blockMenu("Chart · \(IntentParser.pretty(chart.coin))") }
            PnLBoard().blockMenu("Profit and loss", radius: 20)
            if ws.showMovers { MoversBoard().blockMenu("Movers") }
            if ws.showAlerts, !ws.alerts.isEmpty { AlertsBoard().blockMenu("Price alerts") }
            if let yields = ws.yieldOptions ?? ws.heldAnswer?.yields, !yields.isEmpty {
                YieldBubbles(options: yields).allowsHitTesting(ws.yieldOptions != nil).blockMenu("Rates", radius: nil)
            }
            if let allocation = ws.allocationPreview {
                AllocationBoard(preview: allocation).blockMenu("Spread plan", radius: nil)
            }
            RebalanceBoard().blockMenu("Rebalance", radius: nil)
            WithdrawBoard().blockMenu("Withdrawal", radius: nil)
            PositionMoveBoard().blockMenu("Position move", radius: nil)
            RunningBoard().blockMenu("Move under way", radius: nil)
            PendingAddressBoard().blockMenu("New address", radius: nil)
            if let answer = ws.serviceAnswer { ServiceAnswerBoard(answer: answer).blockMenu("\(answer.service)'s answer") }
            if nodes.isEmpty {
                if (ws.yieldOptions ?? ws.heldAnswer?.yields) == nil, ws.allocationPreview == nil, ws.rebalancePlan == nil {
                    ForEach(ws.schedules, id: \.symbol) { ScheduleBoard(schedule: $0).blockMenu("Recurring buy · \($0.symbol)") }
                    if let dca = ws.jupiterDCAs, !dca.isEmpty { JupiterDCABoard(orders: dca).blockMenu("Jupiter DCA") }
                    // Under a running board (a chain's move done, its next step coming) the saved one would draw it twice.
                    if let board, !ws.moveRunning { SavedBoard(canvas: board).blockMenu(ws.readable(board.title), radius: nil) } else if board == nil, positions.isEmpty, ws.schedules.isEmpty, ws.jupiterDCAs?.isEmpty ?? true, ws.pendingAction == nil, !ws.moveRunning, ws.chart == nil, ws.serviceAnswer == nil, ws.ledgerConnect == nil,
                              (ws.canvases.first { $0.id == ws.canvasID }?.earlier ?? []).isEmpty {
                        // Never blank while something is under way: whatever path left nothing live, the canvas's last
                        // board stays until the answer or the outcome takes its place. The empty canvas is for when nothing runs.
                        // A traded plan whose position isn't there (a limit entry still waiting, or closed since) keeps it too.
                        if ws.thinking || ws.busy || ws.executing || ws.preparing || ws.managesPosition,
                           let last = ws.canvases.first(where: { $0.id == ws.canvasID }), !last.bubbles.isEmpty {
                            SavedBoard(canvas: last).blockMenu(ws.readable(last.title), radius: nil)
                        } else if ws.currentScheduleKey == nil {
                            placeholder
                        }
                    }
                }
            } else {
                let route = ws.planRoute(nodes)
                RouteCards(steps: route.steps, footer: route.footer, build: true,
                           buildKey: String(describing: ws.intent) + route.steps.map(\.label).joined())
                    .blockMenu("Plan · \(ws.intent.side == .long ? "long" : "short") \(IntentParser.pretty(ws.intent.asset))", radius: nil)
                Label(route.footer, systemImage: "signature").font(.ui(12)).foregroundStyle(.primary.opacity(0.78)).lineSpacing(2)
                    .frame(maxWidth: 560, alignment: .leading)
                // A plan the rules stop: what fits (checked by the code), raising the limit. The model's ideas are in the chat.
                if nodes.contains(where: { $0.status == .blocked }), !ws.assembling {
                    let fit = ws.planFix.map { ChatRouter.Option(title: $0.title, comment: $0.detail, command: $0.command, recommended: true) }
                    let overLimit = ws.evaluation.sizing.map { s in (ws.equity ?? 0) > 0 && s.lossAtStopUSD / ws.equity! * 100 > ws.policy.maxRiskPct } ?? false
                    if fit != nil || overLimit { NextStepsBoard(fit: fit, overLimit: overLimit) }
                }
                if let choices = ws.takeChoices ?? ws.heldAnswer?.takes, !ws.assembling {
                    TakeChoices(choices: choices, risk: ws.evaluation.sizing?.lossAtStopUSD)
                        .allowsHitTesting(ws.takeChoices != nil)
                        .blockMenu("Take-profit choices", radius: 20)
                        .transition(.opacity)
                }
            }
            // Moves already done on this canvas stay under the current one, the newest first: the newest block is on top.
            EarlierBoards()
        }
    }

    /// An empty canvas: the coin, «Pick a starting block» and six square block cards (V02 «Cards» on the design board):
    /// a picture, the name and what it does. A click puts the block's phrase into the chat.
    private var placeholder: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                BrandLogo(size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Pick a starting block").font(.ui(18, .bold))
                    if ws.hasModel {
                        Text("or type anything in the chat").font(.ui(12)).foregroundStyle(.secondary)
                    } else {
                        // The chat needs a model: the bar under it says how to connect one.
                        Text("Connect a model below to start").font(.ui(12)).foregroundStyle(.secondary)
                    }
                }
            }
            LazyVGrid(columns: Array(repeating: GridItem(.fixed(148), spacing: 12), count: 3), alignment: .leading, spacing: 12) {
                ForEach(TemplatesPanel.templates.prefix(6), id: \.title) { t in
                    let look = TemplatesPanel.looks[t.title]
                    Button { ws.suggest(t.phrase) } label: {
                        VStack(alignment: .leading, spacing: 0) {
                            RoundedRectangle(cornerRadius: 10, style: .continuous).fill(Color.primary.opacity(0.06)).frame(width: 34, height: 34)
                                .overlay(Image(systemName: look?.icon ?? "square.grid.2x2").font(.system(size: 15, weight: .semibold))
                                    .foregroundStyle(look.map { Color(light: $0.light, dark: $0.dark) } ?? .secondary))
                            // Top-aligned: every card's name on the same line, whether it wraps or not.
                            Text(t.title).font(.ui(13.5, .bold)).lineLimit(2).fixedSize(horizontal: false, vertical: true).padding(.top, 14)
                            if let line = look?.line {
                                Text(line).font(.ui(11)).foregroundStyle(.secondary).lineLimit(2).padding(.top, 3)
                            }
                            Spacer(minLength: 0)
                        }
                        .padding(14)
                        .frame(width: 148, height: 140, alignment: .topLeading)
                        .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Brand.card))
                        .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).strokeBorder(Brand.cardBorder))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .modifier(HoverLift(radius: 18))
                    .pointerStyle(.link)
                    .help("Puts “\(t.phrase)” into the chat")
                }
            }
        }
        .frame(width: 470, alignment: .leading)
    }

    /// The plan's title, its status and its buttons: over the route cards, or on top of the chart it is drawn on.
    private func planHeader(_ nodes: [StudioNode]) -> some View {
        HStack(spacing: 8) {
            Text(ws.allocationPreview.map { "Plan · spread \($0.asset) across the best rates" }
                 ?? (hasRoute ? "\(ws.managesPosition ? "Position" : "Plan") · \(ws.intent.side == .long ? "long" : "short") \(IntentParser.pretty(ws.intent.asset))" : board.map { ws.readable($0.title) } ?? "Plan"))
                .font(.ui(14, .semibold))
            if ws.assembling { AssemblyCount(chip: true) } else { StatusChip(text: statusText(nodes), tint: statusTint(nodes)) }
            // An open position is on its market for good: nothing to pick until it closes.
            if hasRoute, !ws.managesPosition { MarketPicker() }
            Spacer(minLength: 12)
            // Top right: a live plan's Cancel and Confirm; a board shown as it was (its move gone stale) only Refresh.
            if hasRoute, !ws.managesPosition {
                PlanButtons()
            } else if ws.allocationPreview != nil, ws.pendingAllocation != nil {
                // A spread waiting to be signed: its Confirm on the canvas too, not only under the chat.
                PendingButtons()
            } else if !hasRoute, let b = board, b.outcome == nil,
                      // The request that drew it, not a «cancel» or a «да» said after (Refresh asked «cancel» again).
                      let request = b.messages.last(where: { $0.fromUser && !ChatWords.confirms($0.text) && !ChatWords.declines($0.text) })?.text {
                Button { Task { await ws.send(request) } } label: {
                    Label("Refresh", systemImage: "arrow.clockwise").font(.ui(13, .semibold)).padding(.horizontal, 6)
                }
                .buttonStyle(.bordered).controlSize(.large).disabled(ws.busy)
                .help("Asks “\(request)” again for fresh numbers")
            }
        }
    }

    private func statusText(_ nodes: [StudioNode]) -> String {
        if ws.allocationPreview != nil { return ws.pendingAllocation != nil ? "ready to sign" : "nothing to place" }
        guard hasRoute else { return board.map { $0.outcome.map { $0.ok ? "executed" : "failed" } ?? "saved canvas" } ?? "waiting for a task" }
        if ws.managesPosition { return ws.positions.contains { $0.coin == ws.intent.asset } ? "position open" : "done · position closed" }
        if nodes.contains(where: { $0.status == .blocked }) { return ws.planBlocker.map { ws.say("stopped: ", "стоп: ") + $0.short } ?? "stopped by the rules" }
        return ws.planIsReady ? "ready to sign" : "needs details"
    }

    private func statusTint(_ nodes: [StudioNode]) -> Color {
        if ws.allocationPreview != nil { return ws.pendingAllocation != nil ? Brand.profit : .secondary }
        if !hasRoute, let o = board?.outcome { return o.ok ? Brand.profit : Brand.loss }
        guard hasRoute, !ws.assembling else { return .secondary }
        if nodes.contains(where: { $0.status == .blocked }) { return Brand.blockedInk }
        return ws.planIsReady || ws.managesPosition ? Brand.profit : Brand.jevInk
    }
}

/// The canvas's pan and zoom. The state lives here, apart from what the canvas shows: a scroll, a drag or a pinch
/// moves the content already built instead of rebuilding the portfolio and the plan on every frame of the gesture.
struct CanvasViewport<Content: View>: View {
    @Environment(Workspace.self) private var ws
    @Environment(\.colorScheme) private var scheme
    /// Width covered by the floating chat on the right; the content is scaled to fit left of it.
    let inset: CGFloat
    /// Width covered by the rail on the left; the content starts past it.
    let leading: CGFloat
    /// The content's own width: at 100 % it fits left of the chat.
    let contentWidth: CGFloat
    @ViewBuilder let content: Content
    @State private var zoom: CGFloat = 1
    @State private var pan: CGSize = .zero
    @GestureState private var drag: CGSize = .zero
    /// A trackpad pinch in progress, and the point between the fingers that stays put.
    @GestureState private var pinch: CGFloat = 1
    @State private var pinchAt: CGPoint = .zero
    /// Two-finger scrolling pans the canvas while the pointer is over it (not over the chat); ⌘-scroll zooms about the
    /// pointer. Kept in a plain class, not @State: the pointer moves with every mouse event, and as state each move
    /// rebuilt the viewport and redrew the window.
    @State private var mouse = Mouse()
    final class Mouse { var over = false, at: CGPoint = .zero }
    @State private var scrollMonitor: Any?
    /// In full screen there's no window to move: the top strip pans like the rest.
    @State private var fullScreen = false
    @Environment(\.displayScale) private var displayScale
    static var zoomRange: ClosedRange<CGFloat> { 0.4...2.5 }

    var body: some View {
        GeometryReader { geo in
            let fit = min(1, max(0.55, (geo.size.width - inset - leading) / contentWidth))
            canvas(scale: fit * zoom)
        }
    }

    private func canvas(scale base: CGFloat) -> some View {
        // Zooming about a point P keeps it still: the offset becomes P − (P − pan)·k.
        let k = Self.zoomRange.clamped(zoom * pinch) / zoom
        let scale = base * k
        let live = CGSize(width: leading + pan.width + drag.width, height: pan.height + drag.height)
        let moved = CanvasZoom.pan(keeping: (pinchAt.x, pinchAt.y), from: (live.width, live.height), by: k)
        // On whole pixels: a drag moves by fractions of a point, and under a scale that isn't 100 % each board, line
        // and letter then snapped to the screen's pixels its own way every frame — the boards wobbled against each other.
        let snapped = CGSize(width: (moved.x * displayScale).rounded() / displayScale, height: (moved.y * displayScale).rounded() / displayScale)
        // The ground (color and dots) is StudioView's, under the rail too; this only takes the gestures.
        // The content hangs in an overlay so the viewport keeps the window's size: a board taller than the window
        // (Favorites, History) no longer stretches it and pushes the zoom bar below the bottom edge.
        return Color.clear
            // Where a title bar would be, the canvas moves the window, as its first 30 pt already did by themselves:
            // below them a grab panned the boards instead, so the same «top of the window» did one thing or the other.
            // The boards over the strip still take their clicks and drags.
            .overlay(alignment: .top) { if !fullScreen { WindowDragArea().frame(height: 2 * TrafficLights.center.y) } }
            .overlay(alignment: .topLeading) {
                content
                    .scaleEffect(scale, anchor: .topLeading)
                    .offset(snapped)
            }
        .clipped()
        .contentShape(Rectangle())
        .gesture(DragGesture(minimumDistance: 6)
            .updating($drag) { value, state, _ in state = value.translation }
            .onEnded { value in pan.width += value.translation.width; pan.height += value.translation.height })
        // Pinch with two fingers: in and out around the point between them.
        .simultaneousGesture(MagnifyGesture()
            .updating($pinch) { value, state, _ in state = value.magnification }
            .onChanged { value in pinchAt = value.startLocation }
            .onEnded { value in zoomAbout(value.startLocation, by: value.magnification) })
        .onContinuousHover { phase in
            switch phase {
            case .active(let p): mouse.over = true; mouse.at = p
            case .ended: mouse.over = false
            }
        }
        .onReceive(NotificationCenter.default.publisher(for: .canvasZoom)) { note in
            if let f = note.userInfo?["factor"] as? CGFloat { zoomAbout(mouse.at, by: f) }
        }
        .onReceive(NotificationCenter.default.publisher(for: .canvasPan)) { note in
            pan.width += note.userInfo?["dx"] as? CGFloat ?? 0
            pan.height += note.userInfo?["dy"] as? CGFloat ?? 0
        }
        .onReceive(NotificationCenter.default.publisher(for: .canvasFit)) { note in
            // Another canvas: its start in view, the zoom left as the user set it («нажимаю закрыть и оно приближает»).
            if note.object as? String == "home" { pan = .zero; return }
            // The Portfolio button with the portfolio already in place: nothing to move, so the window's edge answers.
            if note.object as? String == "portfolio", zoom == 1, pan == .zero {
                NotificationCenter.default.post(name: .canvasEdgePulse, object: nil)
            }
            zoom = 1; pan = .zero
        }
        .onAppear(perform: watchScrolling)
        .onAppear { fullScreen = NSApp.windows.contains { $0.styleMask.contains(.fullScreen) } }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willEnterFullScreenNotification)) { _ in fullScreen = true }
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.willExitFullScreenNotification)) { _ in fullScreen = false }
        .onDisappear { if let m = scrollMonitor { NSEvent.removeMonitor(m); scrollMonitor = nil }; CanvasSharpness.shared.track(false) }
        .onChange(of: scale >= CanvasSharpness.from, initial: true) { _, on in CanvasSharpness.shared.track(on) }
        // Any motion of the content (a drag, a scroll, a pinch, a zoom step, a resize): sharpen once it stops.
        .onChange(of: [scale, snapped.width, snapped.height]) { CanvasSharpness.shared.hold() }
        .overlay(alignment: .bottomLeading) { zoomBar(scale).padding(18).padding(.leading, leading) }
    }

    /// Changes the zoom by `factor`, keeping `point` (in the canvas) where it is.
    private func zoomAbout(_ point: CGPoint, by factor: CGFloat) {
        let next = Self.zoomRange.clamped(zoom * factor)
        let k = next / zoom
        let moved = CanvasZoom.pan(keeping: (point.x - leading, point.y), from: (pan.width, pan.height), by: k)
        pan = CGSize(width: moved.x, height: moved.y)
        zoom = next
        pinchAt = .zero
    }

    /// Trackpad two-finger scrolling pans; ⌘ + scroll zooms (for a mouse wheel). Over the chat, scrolling stays the chat's.
    private func watchScrolling() {
        guard scrollMonitor == nil else { return }
        let mouse = mouse, pan = $pan
        let ws = ws
        scrollMonitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { event in
            guard mouse.over, let window = event.window else { return event }
            let height = window.contentView?.bounds.height ?? 0
            let point = CGPoint(x: event.locationInWindow.x, y: height - event.locationInWindow.y)
            if ws.chatFrame.contains(point) || ws.railFrame.contains(point) { return event }
            let step: CGFloat = event.hasPreciseScrollingDeltas ? 1 : 10
            if event.modifierFlags.contains(.command) {
                NotificationCenter.default.post(name: .canvasZoom, object: nil, userInfo: ["factor": 1 + event.scrollingDeltaY * step * 0.01])
            } else {
                pan.wrappedValue.width += event.scrollingDeltaX * step
                pan.wrappedValue.height += event.scrollingDeltaY * step
            }
            return nil
        }
    }

    private func zoomGlyph(_ name: String) -> some View {
        Image(systemName: name).frame(width: 26, height: 26).contentShape(Rectangle())
    }

    private func zoomBar(_ scale: CGFloat) -> some View {
        HStack(spacing: 4) {
            // A borderless button is clickable only where its label is: the «minus» glyph alone is 14×4 pt.
            // No animation: an animated zoom re-renders and re-lays out the whole canvas on every frame (≈ 60 a click
            // at 120 Hz) and saturated the main thread; a step is one redraw.
            Button { zoom = Self.zoomRange.clamped(zoom - 0.1) } label: { zoomGlyph("minus") }.help("Zoom out · or pinch")
            Text("\(Int((scale * 100).rounded()))%").font(.ui(12, .semibold)).monospacedDigit().frame(width: 44)
            Button { zoom = Self.zoomRange.clamped(zoom + 0.1) } label: { zoomGlyph("plus") }.help("Zoom in · or pinch")
            Button("Fit") { zoom = 1; pan = .zero }
            #if DEBUG_API
            DebugBadge()
            #endif
        }
        .buttonStyle(.borderless)
        .padding(.leading, 6).padding(.trailing, 12).padding(.vertical, 4)
        .environment(\.colorScheme, scheme)
        .measuredGlass(in: Capsule())
    }
}

/// A zoomed-in canvas stays sharp. SwiftUI draws what a scaleEffect enlarges at the window's scale (2× on Retina) and
/// Core Animation stretches that bitmap, so at 250 % the text was soft. Each drawing layer in sight under the zoom
/// gets 2 × zoom instead (at most 4). SwiftUI puts the window's scale back on every layer it updates (a changed number,
/// a hover), so this runs after Core Animation's commits while the canvas is zoomed in, at most 10 times a second and
/// not while the content moves; the fixed layer is drawn again at once. SF Symbols come as ready bitmaps and stay as
/// they are.
@MainActor
final class CanvasSharpness {
    static let shared = CanvasSharpness()
    /// Below it the stretch doesn't show, and hover lifts (1.02–1.08) aren't worth a redraw.
    static let from: CGFloat = 1.25
    /// At most 2 × the window's scale (4 pixels a point on Retina): at 250 % text is a touch soft, but a board's bitmap
    /// is 16 bytes a point instead of 25, and there's one redraw less to wait for.
    static let most: CGFloat = 2
    /// A walk of the layers at most this often: a spinner or the thinking dots no longer mean a walk every frame.
    static let every: CFTimeInterval = 0.1
    private var observer: CFRunLoopObserver?
    private var holdUntil: CFTimeInterval = 0
    private var lastWalk: CFTimeInterval = 0
    private var wakePending = false
    private var drawing: [ObjectIdentifier: Bool] = [:]

    func track(_ zoomedIn: Bool) {
        if zoomedIn, observer == nil {
            // Right after Core Animation's own commit observer (order 2000000).
            let o = CFRunLoopObserverCreateWithHandler(nil, CFRunLoopActivity.beforeWaiting.rawValue, true, 2_000_001) { _, _ in
                MainActor.assumeIsolated { CanvasSharpness.shared.sharpen() }
            }
            CFRunLoopAddObserver(CFRunLoopGetMain(), o, .commonModes)
            observer = o
        } else if !zoomedIn, let o = observer {
            CFRunLoopObserverInvalidate(o)
            observer = nil
        }
    }

    /// A pinch changes the scale every frame and SwiftUI resets every layer each time, and a pan changes nothing a walk
    /// would fix: sharpen once the content stops, not 120 times a second.
    func hold() {
        guard observer != nil else { return }
        holdUntil = CACurrentMediaTime() + 0.2
        wake(after: 0.25)
    }

    /// Wakes the run loop once, so the observer runs even if nothing else happens; one wake at a time.
    private func wake(after delay: CFTimeInterval) {
        guard !wakePending else { return }
        wakePending = true
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { MainActor.assumeIsolated { CanvasSharpness.shared.wakePending = false } }
    }

    private func sharpen() {
        let now = CACurrentMediaTime()
        if now < holdUntil { return wake(after: holdUntil - now + 0.02) }
        if now - lastWalk < Self.every { return wake(after: Self.every - (now - lastWalk) + 0.01) }
        lastWalk = now
        for window in NSApp.windows where window.isVisible && window.occlusionState.contains(.visible) {
            if let layer = window.contentView?.layer { sharpen(layer, root: layer, scale: 1, backing: window.backingScaleFactor) }
        }
    }

    private func sharpen(_ layer: CALayer, root: CALayer, scale: CGFloat, backing: CGFloat) {
        let s = scale * hypot(layer.transform.m11, layer.transform.m12)
        // Only what's in the window: a board panned out of sight keeps its bitmap, and gets sharpened when it's back.
        if s >= Self.from, draws(layer), root.bounds.intersects(layer.convert(layer.bounds, to: root)) {
            // In quarter steps: a pixel of the screen or finer, without redrawing on every tiny change.
            let target = backing * min(Self.most, (s * 4).rounded(.up) / 4)
            if layer.contentsScale != target { layer.contentsScale = target; layer.setNeedsDisplay() }
        }
        layer.sublayers?.forEach { sharpen($0, root: root, scale: s, backing: backing) }
    }

    /// Only layers that draw themselves: SwiftUI's text and shapes, Canvas, AppKit controls. A layer holding a
    /// ready image (a logo, a symbol) would lose it on a redraw.
    private func draws(_ layer: CALayer) -> Bool {
        let type = ObjectIdentifier(Swift.type(of: layer))
        if let known = drawing[type] { return known }
        let name = NSStringFromClass(Swift.type(of: layer))
        let draws = name.hasSuffix("DrawingLayer") || name == "NSViewBackingLayer"
        drawing[type] = draws
        return draws
    }
}

/// The dotted paper under the canvas: its own view, so it is drawn again only when the window's size changes.
struct DotGrid: View {
    var body: some View {
        Canvas { context, size in
            for x in stride(from: CGFloat(11), through: size.width, by: 22) {
                for y in stride(from: CGFloat(11), through: size.height, by: 22) {
                    context.fill(Path(ellipseIn: CGRect(x: x, y: y, width: 2, height: 2)), with: .color(Brand.dots))
                }
            }
        }
        .accessibilityHidden(true)
    }
}

/// «building · 3 of 7» while the cursor puts a route's cards down: the plan's status chip, or the bar over the chat's
/// actions. Its own view, so the count redraws only itself, not the canvas or the chat.
struct AssemblyCount: View {
    @Environment(Workspace.self) private var ws
    let chip: Bool

    var body: some View {
        if let a = ws.assembly {
            if chip {
                StatusChip(text: "building · \(a.shown) of \(a.total)", tint: .secondary)
            } else {
                HStack(spacing: 8) {
                    ProgressView(value: Double(a.shown), total: Double(a.total)).progressViewStyle(.linear).frame(width: 70)
                    Text("Building on the canvas · \(a.shown) of \(a.total)").font(.ui(10)).foregroundStyle(.secondary).monospacedDigit()
                }
            }
        }
    }
}

struct StatusChip: View {
    let text: String
    let tint: Color

    var body: some View {
        Text(text).font(.ui(10)).foregroundStyle(tint)
            .padding(.horizontal, 9).padding(.vertical, 4)
            .background(Capsule().fill(tint.opacity(0.12)))
    }
}

// MARK: Portfolio frame

/// The tokens on a network's card the user hid, and airdrops that look like scams: tucked under «N hidden» at the end
/// of the card's coins (the shown ones are among them, NetworkCard).
struct HiddenTokens: View {
    @Environment(Workspace.self) private var ws
    /// The network's Holding.chain: «solana», «base», «robinhood»…
    let chain: String
    @State private var showTucked = false

    var body: some View {
        let tucked = ws.tuckedTokens.filter { $0.chain == chain }
        if !tucked.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Button(showTucked ? "Collapse" : "\(tucked.count) hidden · show") { withAnimation(.snappy) { showTucked.toggle() } }
                    .buttonStyle(.plain).font(.ui(10, .semibold)).foregroundStyle(.secondary)
                    .help("Tokens you hid, and airdrops that look like scams: unverified, with no real pool")
                if showTucked { ForEach(tucked) { HoldingRow(token: $0, shown: false) } }
            }
        }
    }
}

/// One token: its icon, symbol and amount under it; its value and the day's move on the right. The eye hides it (on
/// hover) or shows a tucked one.
/// A held token's icon: the market list's picture; without one, the bundled logo by ticker (Logos/ holds the top coins
/// by market cap, Tools/top-logos.py); else its first letter.
struct HoldingIcon: View {
    let token: Holding
    var size: CGFloat = 20

    var body: some View {
        // An unverified token's icon URL is its creator's pick: fetching it would tell a scammer the user's IP.
        RemoteIcon(url: token.verified ? token.icon : nil, size: size) {
            // A ticker names the coin only where it can't be copied: the network's own coin, a market list's check, or
            // Hyperliquid's spot book (a name there belongs to one token). An airdropped «USDC» look-alike keeps its letter.
            if token.verified || token.native || token.chain == "hyperliquid", let name = TokenLogo.name(forAsset: token.symbol) {
                TokenLogo(name: name, size: size, round: true)
            } else {
                Circle().fill(Brand.solanaInk.opacity(0.15))
                    .overlay(Text(token.symbol.prefix(1)).font(.ui(size * 0.45, .bold)).foregroundStyle(Brand.solanaInk))
            }
        }
        .frame(width: size, height: size).clipShape(Circle())
    }
}

struct HoldingRow: View {
    @Environment(Workspace.self) private var ws
    let token: Holding
    let shown: Bool
    @State private var hover = false
    /// A network's logo by Holding.chain (the P/L board's rows).
    static let chainLogos = ["base": "base", "arbitrum": "arb", "ethereum": "eth", "polygon": "pol", "robinhood": "robinhood", "hyperliquid": "hype", "near": "near", "tempo": "tempo", "hyperevm": "hyperevm", "optimism": "op", "bnb": "bnb"]

    var body: some View {
        HStack(spacing: 8) {
            HoldingIcon(token: token, size: 20)
            // What it made or lost since bought here, after the amount: in the hint alone it looked as if there was none.
            // One of the big five stablecoins reads like idle USDC: no day's move or P/L to show.
            let dollar = Stablecoins.isGenuine(token.contract, on: token.chain)
            CoinName(symbol: token.symbol, amount: Fmt.num(token.amount, maxFraction: 4), verified: token.verified ? true : token.unverified ? false : nil,
                     pnl: dollar ? nil : ws.purchasePnL(token))
            Spacer(minLength: 4)
            // 20 pt, under the row's own height: at the usual 28 the row grew while the pointer was on it, and the card's
            // new height redrew the whole canvas on every row the pointer crossed. Kept out of the row until hovered: one
            // button per row would be one more thing to move on every step of a pan.
            if hover || !shown {
                Button { withAnimation(.snappy) { ws.tokenChoices[token.id] = !shown } } label: { Image(systemName: shown ? "eye.slash" : "eye") }
                    .buttonStyle(HoverIconStyle(size: 20)).help(shown ? "Hide \(token.symbol): it leaves the portfolio and its total" : "Show \(token.symbol) in the portfolio")
            }
            // A network's own coin (ETH on Arbitrum) without a pool's word: Hyperliquid's market of it.
            CoinValue(value: token.usd.map(Fmt.usd) ?? "no price",
                      change: dollar ? nil : token.change24h ?? (token.native ? ws.markets[token.symbol.uppercased()]?.change24h : nil))
        }
        .opacity(shown ? 1 : 0.6)
        .contentShape(Rectangle())
        .onHover { hover = $0 }
        // A click opens the token (its board, or the chart of a network's own coin); right-click asks the chat about it or
        // trades it.
        .modifier(RowLink(action: ws.opener(chain: token.chain, contract: token.contract, symbol: token.symbol),
                          help: "\(token.symbol): details and chart · right-click to ask the chat, buy or sell",
                          selected: ws.isOpen(chain: token.chain, contract: token.contract, symbol: token.symbol)))
        .modifier(TokenMenu(chain: token.chain, address: token.native || token.chain == "hyperliquid" ? nil : token.contract,
                            symbol: token.symbol, verified: token.verified))
    }
}

struct StudioPortfolio: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        Group {
            if !ws.balanceReady { loading } else { loaded.transition(.opacity) }
        }
        .padding(14)
        .frame(width: 270)
        .background(RoundedRectangle(cornerRadius: 26).fill(Brand.card).shadow(color: .black.opacity(0.08), radius: 18, y: 10))
        .overlay(RoundedRectangle(cornerRadius: 26).strokeBorder(Brand.cardBorder))
        .animation(.easeOut(duration: 0.35), value: ws.balanceReady)
    }

    /// Until every read is in: the card's shape, shimmering, under the last full balance.
    private var loading: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("PORTFOLIO").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            BalanceAmount(font: .ui(20, .heavy))
            Skeleton(height: 30, radius: 8)
            HStack(spacing: 12) {
                Skeleton(width: 70, height: 70, radius: 35)
                VStack(alignment: .leading, spacing: 7) { ForEach(0..<4, id: \.self) { _ in Skeleton(height: 8) } }
            }
            ForEach(0..<3, id: \.self) { _ in Skeleton(height: 56, radius: 16) }
        }
    }

    @ViewBuilder private var loaded: some View {
        // Once per pass: it filters the whole equity history. P/L, not value: a top-up isn't a gain.
        let series = ws.pnlSeries()
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                VStack(alignment: .leading, spacing: 1) {
                    Text("PORTFOLIO").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
                    BalanceAmount(font: .ui(20, .heavy))
                    if series.count > 2, let first = series.first, let last = series.last {
                        // Hours until a day is counted: the line starts fresh, and «1 d» over twenty minutes said too much.
                        let hours = last.at.timeIntervalSince(first.at) / 3_600
                        Text("\(masked(signed(last.usd - first.usd))) · \(hours < 24 ? "\(max(1, Int(hours.rounded(.up)))) h" : "\(Int((hours / 24).rounded(.up))) d")")
                            .font(.ui(10, .semibold)).monospacedDigit()
                            .foregroundStyle(last.usd >= first.usd ? Brand.profit : Brand.loss)
                    }
                }
                Spacer()
                if !ws.positions.isEmpty {
                    Text(signed(ws.positionsPnl)).font(.ui(10, .bold))
                        .foregroundStyle(ws.positionsPnl < 0 ? Brand.loss : Brand.profit)
                        .padding(.horizontal, 9).padding(.vertical, 4)
                        .background(Capsule().fill((ws.positionsPnl < 0 ? Brand.loss : Brand.profit).opacity(0.12)))
                }
            }
            if series.count > 2 {
                EquitySparkline(points: series, height: 30)
            }
            PortfolioRing()
            ScheduleTop3()
            NetworkCards()
            AlertsCard()

            if !ws.schedules.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Label("Recurring buys", systemImage: "arrow.triangle.2.circlepath").font(.ui(10, .semibold))
                    ForEach(ws.schedules, id: \.symbol) { s in
                        Text("\(s.symbol) for \(Fmt.usd(s.usdc)) · \(Fmt.date(s.next))").font(.ui(10)).foregroundStyle(.secondary).monospacedDigit()
                    }
                }
                .padding(11)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 16).strokeBorder(Brand.cardBorder))
            }
        }
    }
}

extension ExitRules {
    /// What ihatecrypto's guardian does for this position, one line each.
    var duties: [String] {
        var out: [String] = []
        if partialOrderPending { out.append("After entry, places half at 1R.") }
        if breakevenAfterPartial {
            out.append(movedToBreakeven ? "Stop is already at breakeven (\(Fmt.price(entry)))." : "After the first half, moves the stop to breakeven (\(Fmt.price(entry))).")
        }
        if let t = trailingPct { out.append("Trailing \(Fmt.pct(t)): the stop follows the best price, now \(Fmt.price(peak)).") }
        return out.isEmpty ? ["Stop and take-profit sit on the exchange; ihatecrypto doesn't move anything."] : out
    }
}

// MARK: Plan nodes

struct StudioNode: Identifiable {
    enum Kind: String { case source, bridge, position, protection, risk, jev, outcome, other }
    let id: String
    let kind: Kind
    let label: String
    let title: String
    let lines: [String]
    let status: Block.Status
    var asset: String = ""
    var long = true
    /// Which signature this step is (1, 2, …); nil for checks and for steps that move nothing.
    var signature: Int?

    /// 0 — left column, 1 — right, 2 — full width under both.
    var column: Int {
        switch kind {
        case .source, .bridge, .risk, .other: 0
        case .position, .protection, .jev: 1
        case .outcome: 2
        }
    }

    @MainActor static func build(_ ws: Workspace) -> [StudioNode] {
        var out = ws.blocks.map { b in
            let kind: Kind = switch b.id {
            case "source": .source
            case "bridge": .bridge
            case "position": .position
            case "protection": .protection
            case "risk": .risk
            default: .other
            }
            return StudioNode(id: b.id, kind: kind, label: b.label, title: b.title, lines: Array(b.lines.prefix(4)), status: b.status,
                              asset: ws.intent.asset, long: ws.intent.side == .long)
        }
        // What the user signs: the bridge (when there is one), the entry, and the stop and take on the exchange.
        var n = 0
        for i in out.indices where [.bridge, .position, .protection].contains(out[i].kind) && out[i].status != .info {
            n += 1
            out[i].signature = n
        }
        if let r = ws.planReview {
            // What was checked, each line a fact the code or a second model confirmed: your words, then what the money
            // touches — the token, the bridge's program, the destination, the market. The transactions themselves are
            // checked the same way at Execute, before Touch ID.
            var lines = [r.concern ? r.text : "Matches your request."]
            let bridging = ws.quote != nil || ws.resolved.intent.amountUSDC != nil
                || ws.blocks.contains { $0.id == "bridge" && $0.status != .info && $0.status != .needsInput }
            if bridging {
                lines.append(ws.fundingNet.map { "\($0.key == "robinhood" ? "USDG" : "USDC") on \($0.name): \($0.usdc.prefix(6))…\($0.usdc.suffix(4))." } ?? "USDC: Circle's token on Solana, EPjF…Dt1v.")
                lines.append("Bridge: \(ws.quote?.provider ?? "Relay"), its program on the allowed list; each signature is checked again at Execute.")
            }
            if isEVMAddress(ws.hlAddress) { lines.append("To: your own Hyperliquid account \(ws.hlAddress.prefix(6))…\(ws.hlAddress.suffix(4)).") }
            if ws.market != nil { lines.append("Market: \(IntentParser.pretty(ws.intent.asset)) on Hyperliquid.") }
            if let t = ws.txReview { lines.append("Signatures: " + t.text) }
            let doubt = r.concern || ws.txReview?.concern == true
            out.append(StudioNode(id: "jev", kind: .jev, label: "Second check", title: doubt ? "Has doubts" : "All correct",
                                  lines: lines, status: doubt ? .warning : .ready))
        } else if let note = ws.jevNote, ws.evaluation.sizing != nil {
            // Unreachable: the card says so, rather than the check silently missing from the route.
            out.append(StudioNode(id: "jev", kind: .jev, label: "Second check", title: "Not checking", lines: [note], status: .info))
        }
        if let s = ws.evaluation.sizing {
            var line = "If stopped: −\(Fmt.usd(s.lossAtStopUSD))"
            if let profit = s.profitAtTakeUSD { line += "   ·   if take: +\(Fmt.usd(profit))" }
            out.append(StudioNode(id: "outcome", kind: .outcome, label: "Result", title: line, lines: [], status: .info))
        }
        return out
    }
}

/// A trade plan's Cancel and Confirm, at the top right of its route. Confirm opens the simulation and Touch ID signs;
/// while a rule stops the plan or a detail is missing it is grey and says why.
struct PlanButtons: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        let ready = ws.planIsReady
        HStack(spacing: 8) {
            Button("Cancel") { withAnimation(StudioView.spring) { ws.clearPlan() } }
                .buttonStyle(.bordered).controlSize(.large)
            // Stopped, with a change that passes every rule: the button makes it, and Confirm comes back.
            if !ready, let fix = ws.planFix {
                Button { Task { await ws.send(fix.command) } } label: {
                    Label(fix.title, systemImage: "wand.and.stars").font(.ui(13, .semibold)).padding(.horizontal, 6)
                }
                .buttonStyle(.borderedProminent).controlSize(.large).tint(Brand.accent)
                .help((ws.planBlocker?.sentence ?? "") + " " + fix.detail)
            } else {
            Button { Task { await ws.prepareExecution() } } label: {
                Label(ws.signerWallet == nil ? "Preview" : "Confirm", systemImage: ready ? Presence.icon : "lock.fill").font(.ui(13, .semibold)).padding(.horizontal, 6)
            }
            // A disabled prominent button still reads blue on macOS 26: grey and locked says it can't go yet.
            .buttonStyle(.borderedProminent).controlSize(.large).tint(ready ? nil : .gray)
            .disabled(!ready || ws.previewWallet == nil || ws.executing || ws.preparing)
            .help(ready ? "Shows the simulation, then signs after \(Presence.name)" : ws.missingDetails.map { "Needs \($0) first" } ?? ws.planBlocker?.sentence ?? "A rule on the canvas stops this plan")
            }
        }
        .disabled(ws.busy)
        // The SIGN card counts the fee approval the first trade asks for.
        .task(id: ws.hlAddress) { if ws.feeApproved != true { await ws.loadFeeApproval() } }
    }
}

extension Workspace {
    /// The trade plan as route cards, in the order it happens: the money from Solana (or already on Hyperliquid), the
    /// bridge, the rules and Jev's check, the position, its stop and take, and what it comes to. A step a rule stops,
    /// or that waits for a detail, says so on its card; its lines open with a click.
    func planRoute(_ nodes: [StudioNode]) -> (steps: [ActivityBubble], footer: String) {
        // The plan as the rules filled it: the leverage, stop and limit by the book are on the cards too.
        let intent = resolved.intent
        func node(_ kind: StudioNode.Kind) -> StudioNode? { nodes.first { $0.kind == kind } }
        func state(_ s: Block.Status, check: Bool = false) -> String {
            switch s {
            case .blocked: "blocked"
            case .needsInput: "attention"
            case .ready, .warning, .info: check ? "passed" : "info"
            case .noData: "info"
            }
        }
        func card(_ label: String, _ title: String, _ lines: [String], _ status: String, logo: String? = nil, _ fill: RouteFill, money: [MoneyMove] = []) -> ActivityBubble {
            ActivityBubble(kind: "step", label: label, title: title, lines: lines, status: status, asset: intent.asset, logo: logo, fill: fill,
                           money: money.isEmpty ? nil : money)
        }
        // A price alert tied to the plan comes first: the plan waits for that price.
        var steps: [ActivityBubble] = planAlertStep.map { [$0] } ?? []
        let sizing = evaluation.sizing
        let name = IntentParser.pretty(intent.asset).components(separatedBy: " (").first ?? intent.asset
        // Rows are «label\tvalue» (StepLine): the eye finds the number without reading a sentence.
        func row(_ label: String, _ value: String) -> String { "\(label)\t\(value)" }
        // «Not needed»: the margin is on Hyperliquid already, and the route starts there.
        let bridge = node(.bridge).flatMap { $0.status == .info ? nil : $0 }
        if let source = node(.source) {
            if let bridge {
                // What goes over: the quote's, the amount asked for, or — while a rule stops the plan and nothing is
                // quoted — what the margin and fees come to, so the card always says how much.
                let free = hl?.withdrawable ?? 0
                let need = sizing.map { max(0, $0.margin + $0.feesUSD - free).rounded(2, .up) }
                let asked = resolved.intent.amountUSDC
                let send = quote?.amountIn ?? asked ?? need
                let approx = quote == nil && asked == nil ? "≈ " : ""
                // Solana short: the USDC leaves straight from the EVM network that has it (fundingNet).
                let from = fundingNet, have = from.map { usdc(on: $0.key) ?? 0 } ?? solana?.usdc
                // The wallet's USDC now and after: the card's money box.
                let wallet = have.map { h in [MoneyMove(place: from?.name ?? "Solana", symbol: "USDC", was: h, after: max(0, h - (send ?? 0)), approx: !approx.isEmpty)] } ?? []
                var lines: [String] = wallet.isEmpty || send == nil ? have.map { [row("wallet", Fmt.num($0))] } ?? [] : []
                if source.status == .blocked, let short = source.lines.last { lines.insert(short, at: 0) }
                steps.append(card("FROM · \((from?.name ?? "Solana").uppercased())", send.map { "\(approx)\(Fmt.num($0)) USDC" } ?? "USDC",
                                  lines.isEmpty && wallet.isEmpty ? source.lines : lines, state(source.status),
                                  logo: from.flatMap { ModuleNetwork(place: $0.key)?.logo } ?? "sol", from.flatMap { RouteFill(rawValue: $0.key) } ?? .solana,
                                  money: send == nil ? [] : wallet))
                // Its amount waits for the position's checks: nothing for the user to do here.
                let oneClick = bridge.label.contains("1Click") && !bridge.label.contains("Relay")
                var lines2: [String]
                if let q = quote {
                    lines2 = [row("sends", "\(Fmt.num(q.amountIn)) USDC"), row("arrives", "\(Fmt.num(q.amountOut)) USDC"),
                              row("fee", Fmt.usd(q.amountIn - q.reserve - q.amountOut) + (q.time.firstMatch(of: /\d+\s*s\b/).map { " · \($0.0)" } ?? ""))]
                    if q.opening { lines2.append(q.provider == "Relay" ? "incl. Hyperliquid's one-time account fee" : "opens the account, no opening fee") }
                } else {
                    lines2 = (send.map { [row("sends", "≈ \(Fmt.num($0)) USDC")] } ?? []) + ["quoted once the position passes the checks"]
                }
                // What lands on Hyperliquid: the quote's, else about what goes over.
                let lands = quote?.amountOut ?? send
                steps.append(card(oneClick ? "BRIDGE · 1CLICK" : "BRIDGE · RELAY", "→ Hyperliquid", lines2,
                                  bridge.status == .needsInput ? "info" : state(bridge.status), logo: oneClick ? "usdc" : "relay", .relay,
                                  money: lands.map { [MoneyMove(place: "Hyperliquid", symbol: "USDC", was: free, after: free + $0, approx: quote == nil)] } ?? []))
            } else {
                steps.append(card("FROM · HYPERLIQUID", hl.map { "\(Fmt.num($0.withdrawable)) USDC" } ?? "USDC",
                                  ["the margin is on Hyperliquid already"], "info", logo: "hype", .hyperliquid))
            }
        }
        if let r = node(.risk) {
            var lines: [String] = []
            if let s = sizing {
                let pct = equity.flatMap { $0 > 0 ? (s.lossAtStopUSD / $0 * 100).rounded(1, .plain) : nil }
                lines.append(row("risk", Fmt.usd(s.lossAtStopUSD) + (pct.map { " · \(Fmt.num($0))%" } ?? "")))
                lines.append(row("limit", "\(Fmt.num(policy.maxRiskPct))% a trade"))
                if let profit = s.profitAtTakeUSD, s.lossAtStopUSD > 0 { lines.append(row("reward/risk", "1:\(Fmt.num((profit / s.lossAtStopUSD).rounded(1, .plain)))")) }
            }
            // A rule it breaks, in its own words, first.
            lines.insert(contentsOf: r.lines.filter { $0.hasPrefix("✗") }, at: 0)
            steps.append(card("CHECK RULES", r.title, lines.isEmpty ? r.lines : lines, state(r.status, check: true), .check))
        }
        if let j = node(.jev) {
            steps.append(card("SECOND CHECK", j.title, j.lines,
                              j.status == .info ? "info" : j.status == .warning ? "attention" : "passed", .check))
        }
        if let p = node(.position) {
            let leverage = "\(Fmt.num(intent.leverage ?? 1))×"
            var lines = p.lines
            if let s = sizing, let m = market {
                lines = [row("amount", "\(Fmt.num(s.size, maxFraction: m.szDecimals)) \(name)"),
                         row(intent.entryPx == nil ? "entry" : "limit", "\(intent.entryPx == nil ? "≈ " : "")\(Fmt.price(s.entry))"),
                         row("margin", Fmt.usd(s.margin)),
                         row("liquidation", "≈ \(Fmt.price(s.liquidationPrice))")]
            }
            // What stops it goes first, in red.
            if p.status == .blocked, let problem = evaluation.problem ?? p.lines.last { lines.insert(problem, at: 0) }
            // The size and the leverage are the title: the first thing read.
            steps.append(card("OPEN \(p.long ? "LONG" : "SHORT") · \(name.uppercased())", sizing.map { "\(Fmt.usd($0.notional)) · \(leverage)" } ?? "\(name) \(leverage)",
                              lines, state(p.status), logo: TokenLogo.name(forAsset: intent.asset) ?? "hype", .hyperliquid))
        }
        if let s = node(.protection) {
            var lines = s.lines
            if let z = sizing, let stop = intent.stopPct {
                lines = [row("stop", "\(Fmt.price(z.stopPrice)) · −\(Fmt.pct(stop))")]
                if let tp = z.takePrice {
                    lines.append(row("take", "\(Fmt.price(tp)) · +\(Fmt.pct((abs(tp - z.entry) / z.entry * 100).rounded(1, .plain)))"))
                }
                lines.append("both wait on the exchange")
            }
            steps.append(card("PROTECT", sizing?.takePrice != nil ? "Stop + take" : "Stop", lines, state(s.status), logo: "hype", .hyperliquid))
        }
        // What Confirm does: the simulation first, then Touch ID for every signature — ihatecrypto's fee approval on the
        // first trade is one of them.
        let signing = nodes.filter { $0.signature != nil }.count + ((prepared?.asksFee ?? (feeApproved == false)) ? 1 : 0)
        if sizing != nil, signing > 0 {
            steps.append(card(prepared == nil ? "SIMULATION · ON CONFIRM" : "SIMULATION · PASSED", prepared == nil ? "Shows first" : "As planned",
                              prepared.map { Array($0.changes.prefix(3)) } ?? ["every balance, before and after", "nothing is signed before it"],
                              prepared == nil ? "info" : "passed", .check))
            steps.append(card(ledgerActive ? "SIGN · LEDGER" : "SIGN · \(Presence.signShort.uppercased())", "\(signing) signature\(signing == 1 ? "" : "s")",
                              prepared?.steps ?? [ledgerActive ? "each one approved on the device" : "one \(Presence.signShort) covers them all"], "info", .check))
        }
        if let s = sizing {
            var lines = [row("at the stop", "−\(Fmt.usd(s.lossAtStopUSD))")]
            if let profit = s.profitAtTakeUSD { lines.append(row("at the take", "+\(Fmt.usd(profit))")) }
            // One row: a compact card shows three. ihatecrypto's part named beside the total.
            lines.append(row("fees", "≈ \(Fmt.usd(s.feesUSD)) · ihatecrypto \(Fmt.usd(s.builderFeesUSD))"))
            steps.append(card("RESULT · ESTIMATE", "−\(Fmt.usd(s.lossAtStopUSD))" + (s.profitAtTakeUSD.map { " / +\(Fmt.usd($0))" } ?? ""), lines, "info", .check))
        }
        let footer = "Execute shows the simulation first, then \(signing) signature\(signing == 1 ? "" : "s") — nothing is signed before "
            + (ledgerActive ? "you approve it on your Ledger." : "\(Presence.signName).")
        return (steps, footer)
    }
}

struct StudioNodeView: View {
    let node: StudioNode

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if let action { tag(action.icon, action.verb) }
                Spacer(minLength: 0)
                if let n = node.signature {
                    HStack(spacing: 3) { Image(systemName: "signature"); Text("\(n)") }
                        .font(.ui(10, .bold))
                        .padding(.horizontal, 6).padding(.vertical, 3)
                        .background(Capsule().strokeBorder(labelColor.opacity(0.6)))
                        .foregroundStyle(labelColor)
                        .help("This step is signed — signature \(n)")
                }
                if node.status == .warning || node.status == .needsInput || node.status == .blocked {
                    Circle().fill(dotColor).frame(width: 7, height: 7)
                }
            }
            if node.kind != .outcome {
                HStack(spacing: 7) {
                    header
                    if node.label != "Source" { Text(node.label).font(.ui(10)).foregroundStyle(labelColor).lineLimit(1) }
                }
            }
            Text(node.title).font(.ui(node.kind == .outcome ? 15 : 15, .bold)).lineLimit(3)
            ForEach(node.lines.indices, id: \.self) { i in
                Text(node.lines[i]).font(.ui(10)).foregroundStyle(lineColor).lineLimit(3).monospacedDigit()
            }
        }
        .foregroundStyle(textColor)
        .padding(.leading, 14).padding(.vertical, 14).padding(.trailing, node.kind == .bridge ? 28 : 14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(shape.fill(background))
        .overlay(shape.stroke(border, style: StrokeStyle(lineWidth: node.status == .blocked ? 2 : 1, dash: isCheck ? [5, 4] : [])))
    }

    /// Money that moves is an arrow, checks are soft capsules, the rest are cards.
    private var shape: AnyShape {
        switch node.kind {
        case .bridge: AnyShape(ArrowCard())
        case .risk, .jev: AnyShape(RoundedRectangle(cornerRadius: 30))
        default: AnyShape(RoundedRectangle(cornerRadius: 20))
        }
    }

    private var isCheck: Bool { node.kind == .risk || node.kind == .jev }

    /// The verb on top of each block: what this step does, readable before the numbers.
    private var action: (icon: String, verb: String)? {
        switch node.kind {
        case .source: ("tray.and.arrow.up.fill", "TAKE FROM")
        case .bridge: ("arrow.left.arrow.right", "BRIDGE")
        case .position: (node.long ? "arrow.up.right" : "arrow.down.right", node.long ? "OPEN LONG" : "OPEN SHORT")
        case .protection: ("shield.fill", "PROTECT")
        case .risk: ("checklist", "CHECK RULES")
        case .jev: ("checkmark.shield.fill", "SECOND CHECK")
        case .outcome: ("equal.circle.fill", "RESULT")
        case .other: nil
        }
    }

    private func tag(_ icon: String, _ verb: String) -> some View {
        let dark = [.bridge, .position, .outcome].contains(node.kind) && node.status != .blocked
        return HStack(spacing: 4) {
            Image(systemName: icon).font(.system(size: 9, weight: .bold))
            Text(verb).font(.ui(9.5, .heavy)).tracking(0.5)
        }
        .padding(.horizontal, 7).padding(.vertical, 4)
        .foregroundStyle(dark ? .white : labelColor)
        .background(Capsule().fill(dark ? Color.white.opacity(0.16) : labelColor.opacity(0.13)))
    }

    @ViewBuilder private var header: some View {
        switch node.kind {
        case .source:
            HStack(spacing: -6) { TokenLogo(name: node.title.contains("Hyperliquid") ? "hype" : "sol", size: 20); TokenLogo(name: "usdc", size: 20) }
        case .bridge: TokenLogo(name: node.title.contains("1Click") ? "usdc" : "relay", size: 20)
        case .position: HStack(spacing: 4) { TokenLogo(name: "hype", size: 20); AssetBadge(coin: node.asset, size: 20) }
        case .jev: Image(systemName: "checkmark.shield").foregroundStyle(Brand.jevInk)
        case .risk: Image(systemName: "gauge.with.dots.needle.50percent").foregroundStyle(labelColor)
        case .protection: Image(systemName: "shield.lefthalf.filled").foregroundStyle(.secondary)
        case .outcome, .other: EmptyView()
        }
    }

    private var background: Color {
        if node.status == .blocked { return Brand.blockedTint }
        switch node.kind {
        case .source: return Brand.solanaTint
        case .bridge: return Brand.relay
        case .position: return Brand.hl
        case .risk: return Brand.riskTint
        case .jev: return Brand.jevTint
        case .outcome: return Brand.outcome
        case .protection, .other: return Brand.card
        }
    }

    private var border: Color {
        if node.status == .blocked { return Brand.blockedInk }
        switch node.kind {
        case .protection, .other, .source: return Brand.cardBorder
        default: return .clear
        }
    }

    private var textColor: Color {
        switch node.kind {
        case .bridge, .position, .outcome: node.status == .blocked ? Brand.blockedInk : .white
        default: .primary
        }
    }

    private var labelColor: Color {
        if node.status == .blocked { return Brand.blockedInk }
        switch node.kind {
        case .source: return Brand.solanaInk
        case .bridge: return Brand.relayInk
        case .position: return Brand.hlMint
        case .risk: return Brand.riskInk
        case .jev: return Brand.jevInk
        case .outcome: return Color(hex: 0xB9C1DC)
        case .protection, .other: return .secondary
        }
    }

    private var lineColor: Color {
        switch node.kind {
        case .bridge: Brand.relayInk
        case .position: Brand.hlSoft
        case .outcome: Color(hex: 0xB9C1DC)
        default: .secondary
        }
    }

    private var dotColor: Color {
        switch node.status {
        case .blocked: .red
        case .warning: .yellow
        default: .orange
        }
    }
}

// MARK: Floating chat

/// Where the chat is scrolled, for following its end.
private struct ChatScroll: Equatable {
    var y: CGFloat
    var height: CGFloat
    var atEnd: Bool
}

struct FloatingChat: View {
    @Environment(Workspace.self) private var ws
    @Environment(\.colorScheme) private var scheme
    var compact = false
    var wide = false
    let toggleCompact: () -> Void
    var toggleWide: () -> Void = {}
    /// The header moves the chat anywhere; a double-click brings it home. The header buttons still click.
    @Binding var drag: CGSize
    var endDrag: (CGSize) -> Void = { _ in }
    var dock: () -> Void = {}
    /// The first of the four starting points shown: a different four each launch.
    @State private var ideasStart = Int.random(in: 0..<TemplatesPanel.templates.count)
    /// The chat follows what's written to its end, the answer as it streams too — unless the person scrolled up to read:
    /// then it stays where they are until they're back at the end or send something (the owner, 2026-10-19).
    @State private var follow = true

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                BrandLogo(size: 30, mood: mood, animated: true)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Assistant").font(.ui(14, .semibold))
                    Text(status).font(.ui(10)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                Button { withAnimation(StudioView.spring) { ws.newCanvas() } } label: { headerIcon("square.and.pencil") }
                    .buttonStyle(.hoverIcon).help("New canvas").disabled(ws.busy)
                Button(action: toggleWide) { headerIcon(wide ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right") }
                    .buttonStyle(.hoverIcon).help(wide ? "Narrow chat" : "Chat on half the window")
                Button(action: toggleCompact) { headerIcon(compact ? "chevron.up" : "minus") }
                    .buttonStyle(.hoverIcon).help(compact ? "Full-height chat" : "Compact chat")
            }
            .padding(.horizontal, 14).padding(.top, 12).padding(.bottom, 8)
            .background(Color.clear.contentShape(Rectangle()).pointerStyle(.grabIdle).gesture(DragGesture(minimumDistance: 4, coordinateSpace: .global)
                .onChanged { drag = $0.translation }
                .onEnded { endDrag($0.translation) })
                .simultaneousGesture(TapGesture(count: 2).onEnded { dock() }))

            ScrollViewReader { proxy in
                ScrollView {
                    // Not lazy: a lazy stack anchored at the bottom dropped the first message when it replaced the intro.
                    VStack(alignment: .leading, spacing: 14) {
                        ChatHistory(messages: Array(ws.messages.dropFirst()), running: ws.executing)
                        DraftReply()
                        Color.clear.frame(height: 0).id(ChatHistory.end)
                        if ws.messages.count <= 1 {
                            Text("Your portfolio is on the canvas. Tell me what to do with your money — in any language — and I’ll build the plan next to it. Nothing moves without your signature.")
                                .font(.ui(14)).foregroundStyle(.secondary)
                            // Full-width cards with an icon, not capsules: the model and network chips under them are capsules.
                            // Four of the plan templates at a time; the arrows bring the next four.
                            HStack {
                                Spacer()
                                Button { withAnimation(.snappy) { ideasStart += 4 } } label: { headerIcon("arrow.clockwise") }
                                    .buttonStyle(.hoverIcon).help("Other starting points")
                            }
                            .padding(.top, 4)
                            VStack(spacing: 6) {
                                let all = TemplatesPanel.templates
                                ForEach((0..<4).map { all[(ideasStart + $0) % all.count] }, id: \.title) { t in
                                    Button { ws.suggest(t.phrase) } label: {
                                        HStack(spacing: 10) {
                                            Image(systemName: TemplatesPanel.looks[t.title]?.icon ?? "square.grid.2x2")
                                                .font(.system(size: 13, weight: .semibold)).foregroundStyle(Brand.accentInk)
                                                .frame(width: 28, height: 28)
                                                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Brand.accent.opacity(0.12)))
                                            // What it does in plain words; the phrase itself types out in the field on a click.
                                            VStack(alignment: .leading, spacing: 1) {
                                                Text(t.title).font(.ui(13, .semibold)).foregroundStyle(.primary)
                                                Text(TemplatesPanel.looks[t.title]?.line ?? t.phrase).font(.ui(11)).foregroundStyle(.secondary)
                                            }
                                            .lineLimit(1).truncationMode(.tail)
                                            Spacer(minLength: 6)
                                            Image(systemName: "arrow.up.left").font(.system(size: 11, weight: .semibold)).foregroundStyle(.tertiary)
                                        }
                                        .padding(7).padding(.trailing, 5)
                                        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Brand.card))
                                        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(Brand.cardBorder))
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain).modifier(HoverLift(radius: 12))
                                    // The scheme it builds, step by step, before anything is typed.
                                    .help("Puts “\(t.phrase)” in the field to edit and send\n\n"
                                          + (TemplatesPanel.steps[t.title] ?? []).map { "\($0.0): \($0.1)" }.joined(separator: "\n"))
                                }
                            }
                        }
                    }
                    .padding(.horizontal, 14).padding(.bottom, 8)
                }
                .defaultScrollAnchor(.bottom)
                // Up by the person (the content the same height): stop following; at the end again: follow. The chat
                // growing or shrinking under a follower (a streamed answer, cards, a step line) keeps the end in view.
                .onScrollGeometryChange(for: ChatScroll.self) { g in
                    ChatScroll(y: g.contentOffset.y, height: g.contentSize.height, atEnd: g.visibleRect.maxY >= g.contentSize.height - 24)
                } action: { old, new in
                    if new.atEnd { follow = true }
                    else if new.height == old.height, new.y < old.y - 1 { follow = false }
                    else if follow, new.height != old.height { proxy.scrollTo(ChatHistory.end, anchor: .bottom) }
                }
                // Not animated: a scroll animation redraws the whole window every frame, and execution adds a line at a time.
                // Their own message always brings the end back.
                .onChange(of: ws.messages.count) {
                    if ws.messages.last?.fromUser == true { follow = true }
                    if follow { proxy.scrollTo(ChatHistory.end, anchor: .bottom) }
                }
                // Cards come under a reply already shown: in view with it, and when they don't all fit, the question
                // they answer stays in view from its top (a reply grouped into steps has no id here: the end it is).
                .onChange(of: ws.nextSteps?.message) {
                    guard follow, let id = ws.nextSteps?.message else { return }
                    proxy.scrollTo(ChatHistory.end, anchor: .bottom)
                    // After the cards are laid out: in this turn the bottom anchor pulled it back to the end as they grew.
                    DispatchQueue.main.async { proxy.scrollTo(id, anchor: .top) }
                }
                // The card ↑↓ moved to, in view.
                .onChange(of: ws.cardKey) { if let k = ws.cardKey, let id = ws.nextSteps?.message { proxy.scrollTo(Answers.cardID(id, k)) } }
            }

            if ws.assembling {
                AssemblyCount(chip: false).padding(.horizontal, 14).padding(.bottom, 4).transition(.opacity)
            }
            ChatActions().padding(.horizontal, 14)

            ChatComposer()
        }
        .frame(maxHeight: .infinity)
        .environment(\.colorScheme, scheme)
        // Liquid Glass, floating over the canvas like the rail.
        .measuredGlass(.regular.tint(Brand.glassTint), in: RoundedRectangle(cornerRadius: 26, style: .continuous))
    }

    private func headerIcon(_ name: String) -> some View {
        Image(systemName: name).font(.system(size: 13, weight: .semibold))
    }

    /// The coin's face in the header: working, waiting for the user, done, or something went wrong; listening, or
    /// looking at a menu or Settings the person has open.
    private var mood: BrandLogo.Mood {
        if ws.executionError != nil { return .error }
        if ws.thinking || ws.executing || ws.preparing || ws.assembling { return .thinking }
        if ws.voice.listening { return .listening }
        if ws.tuning != nil { return .tuning }
        if ws.pendingText != nil || ws.pendingAllocation != nil { return .waiting }
        if let outcome = (ws.savedBoard ?? ws.finishedCanvas)?.outcome { return outcome.ok ? .done : .error }
        return .idle
    }

    /// The line under «Assistant», in step with the face.
    private var status: String {
        switch mood {
        case .error: "Something went wrong"
        case .thinking: ws.executing ? "Carrying it out" : "Working on it"
        case .listening: "Listening…"
        case .tuning: ws.tuning ?? ""
        case .waiting: "Waiting for your Confirm"
        case .done: "Done"
        case .idle, .asleep: "Waiting for you"
        }
    }
}

/// The one thing to do next under the chat: sign the allocation, execute the plan, or manage the open position.
/// Its own view: it reads the plan and prices, so a balance refresh redraws these buttons, not the whole chat.
private struct ChatActions: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        if ws.pendingText != nil || ws.pendingAllocation != nil {
            // A move waiting for Confirm: its Cancel and Confirm are in the field (ChatComposer.waitingBar).
            EmptyView()
        } else if ws.managesPosition, ws.positions.contains(where: { $0.coin == ws.intent.asset }) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach([("move stop to breakeven", "shield.lefthalf.filled"), ("trail 2%", "arrow.up.forward"),
                             ("close half", "circle.lefthalf.filled"), ("close", "xmark.circle")], id: \.0) { phrase, icon in
                        Button { Task { await ws.send(phrase) } } label: { Label(phrase, systemImage: icon) }
                            .buttonStyle(.plain).font(.ui(12))
                            .padding(.horizontal, 10).padding(.vertical, 6)
                            .background(Capsule().fill(Brand.glassFill))
                            .overlay(Capsule().strokeBorder(Brand.glassRim))
                            .foregroundStyle(phrase == "close" ? Brand.loss : Color.primary)
                    }
                }
            }
            .padding(.bottom, 4)
        } else if ws.intent != TradeIntent(), !ws.managesPosition {
            // Once: each read builds the whole plan again.
            let ready = ws.planIsReady
            VStack(alignment: .leading, spacing: 6) {
                if let r = ws.planReview {
                    Label(r.text, systemImage: r.concern ? "exclamationmark.triangle.fill" : "checkmark.seal")
                        .font(.ui(10)).foregroundStyle(r.concern ? AnyShapeStyle(.orange) : AnyShapeStyle(.secondary)).lineLimit(2)
                }
                HStack {
                    if ws.executing || ws.preparing { ProgressView().controlSize(.small) }
                    if !ready, !ws.preparing, !ws.executing, let fix = ws.planFix {
                        Button { Task { await ws.send(fix.command) } } label: { primary(fix.title, enabled: true) }
                            .buttonStyle(.plain)
                            .help(fix.detail)
                    } else {
                        Button { Task { await ws.prepareExecution() } } label: {
                            primary(ws.signerWallet == nil ? "Preview" : "Execute", enabled: ready)
                        }
                        .buttonStyle(.plain)
                        .disabled(!ready || ws.previewWallet == nil || ws.executing || ws.preparing)
                    }
                }
                // A grey Execute says why, and the missing details are one tap away.
                if !ready, !ws.preparing, !ws.executing {
                    HStack(spacing: 8) {
                        Label(ws.missingDetails.map { "Execute needs \($0)." }
                              ?? ws.planBlocker.map { $0.sentence + (ws.planFix == nil ? "" : ws.say(" The button above changes that; then Execute.", " Кнопка выше это исправит, потом Execute.")) }
                              ?? "A rule on the canvas stops this plan.", systemImage: "lock.fill")
                            .font(.ui(10, .semibold)).foregroundStyle(.orange)
                        Spacer(minLength: 4)
                        if ws.missingDetails != nil {
                            Button { Task { await ws.send("by the book") } } label: {
                                Text("By the book").font(.ui(12, .semibold)).foregroundStyle(Brand.onAccent)
                                    .padding(.horizontal, 12).padding(.vertical, 6)
                                    .background(Capsule().fill(Brand.accent))
                            }
                                .buttonStyle(.plain).pointerStyle(.link)
                                .help("Stop from 4h volatility, take at 2R, 1% of capital at risk")
                        }
                    }
                }
                Text("Edit it in words: “take like the pros”, “risk $0.20”, “entry on a 1% pullback”.")
                    .font(.ui(10)).foregroundStyle(.secondary)
            }
            .padding(.bottom, 4)
        }
    }

    /// The one orange capsule: dark text on orange reads in both themes; grey when it can't be pressed yet.
    private func primary(_ title: String, enabled: Bool = true) -> some View {
        Text(title).font(.ui(14, .semibold)).foregroundStyle(enabled ? Brand.onAccent : Color.secondary)
            .frame(maxWidth: .infinity, minHeight: 42)
            .background(Capsule().fill(enabled ? Brand.accent : Color.primary.opacity(0.08)))
            .overlay(Capsule().strokeBorder(enabled ? Color.white.opacity(0.35) : .clear))
    }
}

/// The voice as bars, the newest on the left: taller the louder, a dot in silence. Heights flow into each other as
/// the row moves right instead of jumping, and the older half fades out.
private struct VoiceBars: View {
    /// Its levels (oldest first, 0…1) are read here, not in the composer: ten readings a second redraw the wave, not
    /// the field and its buttons.
    let voice: VoiceInput

    var body: some View {
        let levels = voice.levels
        let bars = Array(levels.reversed())
        HStack(spacing: 3) {
            ForEach(bars.indices, id: \.self) { i in
                Capsule().fill(Brand.accent)
                    .frame(width: 3, height: 3 + 23 * bars[i])
                    .opacity(i < bars.count / 2 ? 1 : 1 - 0.85 * Double(i - bars.count / 2) / Double(bars.count - bars.count / 2))
            }
        }
        // Not animated: ten readings a second kept a 0.2 s animation running, redrawing the window, for the whole dictation.
        .frame(maxWidth: .infinity, maxHeight: 30, alignment: .leading)
        .clipped()
        .accessibilityHidden(true)
    }
}

/// No model reads the chat yet: instead of a field whose words would only be guessed at, how to get one — a subscription,
/// a key, or a model on this Mac — or, for the model on this Mac, what it still needs: Ollama running, the model downloaded.
private struct ConnectModelBar: View {
    @Environment(Workspace.self) private var ws

    var body: some View {
        let info = ws.provider.info, model = ws.model, name = LLMCatalog.displayName(model: model)
        let local = info.kind == .local && info.unavailable == nil && LLMCatalog.chatRefusal(model: model) == nil
        let down = local && ws.localServers[info.id] == false
        let outdated = local && info.id == "ollama" && ws.ollamaOutdated
        let size = ws.modelSizes[model]
        VStack(alignment: .leading, spacing: 8) {
            if outdated {
                title("Ollama \(ws.ollamaVersion ?? "") is too old for \(name)",
                      "Update it to \(ws.ollamaLatest.map { "the newest, \($0)" } ?? "the newest") — free, right here — then download the model.")
            } else if down {
                title(info.id == "ollama" ? "Ollama isn't running" : "\(info.name) isn't running",
                      "Open it to chat with a model on this Mac — or connect a subscription instead.")
            } else if local {
                title("\(name) isn't on this Mac yet", size.map { gb in
                    "≈ \(Fmt.num(Decimal(gb), maxFraction: 1)) GB to download; it runs in ≈ \(Fmt.num(Decimal(gb * 1.2), maxFraction: 0)) GB of this Mac's \(Int(OllamaModels.memoryGB.rounded())) GB"
                        + (OllamaModels.fits(sizeGB: gb) ? "." : " — too big for it: pick a smaller one or a subscription.")
                } ?? "Download it, or use one this Mac already has.")
            } else if LLMCatalog.chatRefusal(model: model) != nil {
                title("\(name) is too small for the chat", "Pick a bigger model above, or connect a subscription.")
            } else {
                title("Connect a model to chat", "The chat needs a model to understand you: a subscription you already pay for, an API key, or a free model on this Mac.")
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    if outdated {
                        chip(ws.ollamaInstall ?? ws.modelDownloads[model].map { "Downloading · \(Int($0 * 100))%" } ?? "Update Ollama" + (ws.ollamaLatest.map { " to \($0)" } ?? "") + " and download",
                             symbol: "arrow.down.circle", accent: true) {
                            Task { await ws.setUpLocal() }
                        }
                        .disabled(ws.ollamaInstall != nil)
                        chip("Check again", symbol: "arrow.clockwise") { Task { await ws.refreshLocalModels() } }
                    } else if down {
                        if OllamaModels.installed, info.id == "ollama" {
                            chip("Open Ollama", symbol: "play.circle") { OllamaModels.start() }
                        } else if info.id == "ollama" {
                            chip(ws.ollamaInstall ?? "Get Ollama" + (ws.ollamaLatest.map { " \($0)" } ?? "") + " and a model", symbol: "arrow.down.circle", accent: true) {
                                Task { await ws.setUpLocal() }
                            }
                            .disabled(ws.ollamaInstall != nil)
                        }
                        chip("Check again", symbol: "arrow.clockwise") { Task { await ws.refreshLocalModels() } }
                    } else if local, info.id == "ollama" {
                        if let p = ws.modelDownloads[model] {
                            chip("Downloading · \(Int(p * 100))%", symbol: "arrow.down.circle") {}.disabled(true)
                        } else if size.map({ OllamaModels.fits(sizeGB: $0) }) != false {
                            chip(size.map { "Download · ≈ \(Fmt.num(Decimal($0), maxFraction: 1)) GB" } ?? "Download", symbol: "arrow.down.circle", accent: true) {
                                Task { await ws.downloadModel(model) }
                            }
                        }
                        // Models this Mac already has: one click and the chat opens.
                        ForEach((ws.liveModels[info.id] ?? []).filter { LLMCatalog.chatRefusal(model: $0) == nil }.prefix(2), id: \.self) { have in
                            chip("Use \(LLMCatalog.displayName(model: have))", symbol: "desktopcomputer") {
                                ws.model = have
                                OllamaModels.preload(have)
                            }
                        }
                    }
                    way("ChatGPT", logo: "llm-openai", id: "chatgpt")
                    way("Claude", logo: "llm-claude", id: "claude-code")
                    way("OpenRouter", logo: "llm-openrouter", id: "openrouter")
                    // Everything for a free model here in one go: the newest Ollama, then a model that fits this Mac.
                    if !local { chip("On this Mac", symbol: "desktopcomputer") { Task { await ws.setUpLocal() } } }
                }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 22, style: .continuous).fill(Brand.glassFill))
        .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Brand.glassRim))
    }

    private func title(_ text: String, _ line: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(text).font(.ui(13, .semibold))
            Text(line).font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }

    /// A subscription or a key: chosen, then signed in where Settings › AI model shows how.
    private func way(_ title: String, logo: String, id: String) -> some View {
        Button { ws.providerID = id; ws.openSettings(.signIns) } label: {
            HStack(spacing: 6) { LogoTile(logo: logo, size: 20); Text(title).font(.ui(12, .semibold)) }
                .padding(.leading, 5).padding(.trailing, 10).frame(height: 30)
                .background(Capsule().fill(Brand.glassFill)).overlay(Capsule().strokeBorder(Brand.glassRim))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain).pointerStyle(.link)
    }

    private func chip(_ title: String, symbol: String, accent: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: symbol).font(.ui(12, .semibold))
                .foregroundStyle(accent ? Brand.onAccent : Color.primary)
                .padding(.horizontal, 10).frame(height: 30)
                .background(Capsule().fill(accent ? Brand.accent : Brand.glassFill)).overlay(Capsule().strokeBorder(accent ? .clear : Brand.glassRim))
                .contentShape(Capsule())
        }
        .buttonStyle(.plain).pointerStyle(.link)
    }
}

/// What a rounded rectangle leaves bare of its frame at the top: the two corners past its curves.
private nonisolated struct TopCorners: Shape {
    let radius: CGFloat
    func path(in r: CGRect) -> Path {
        Path(CGRect(x: r.minX, y: r.minY, width: r.width, height: radius))
            .subtracting(Path(roundedRect: r, cornerRadius: radius, style: .continuous))
    }
}

/// The chat's input: the model and the networks above it, the field with the microphone. Its own view: a keystroke or
/// the voice's wave redraws the field and its buttons, not the conversation above it.
private struct ChatComposer: View {
    @Environment(Workspace.self) private var ws
    @State private var templatesOpen = false
    /// The highlighted row of the «/», «@» or «$» list.
    @State private var pick = 0
    /// The row ↑↓ moved to: only the keys scroll the list to it. A row the pointer is over is highlighted, never scrolled
    /// to: as the list moved under a resting pointer, the row under it took the highlight and the list ran to the bottom.
    @State private var keyedRow: Int?
    /// Where the pointer was when ↑↓ last moved: a row it rests on doesn't take the highlight back until it moves.
    @State private var pointerAtKey: CGPoint?
    /// The field's text when Esc closed the list: it stays closed until the text changes.
    @State private var dismissed: String?
    /// The sent message ↑ brought back into the field (SentHistory), while the field still holds it.
    @State private var recalled: Int?

    /// Where the caret is in the field (UTF-16 offset in the draft): «/», «@» and «$» lists work on the word it's in,
    /// anywhere in the text.
    @State private var caretAt: Int?
    /// A pick from a list puts the caret right after what went in.
    @State private var caretRequest: Int?
    private var caret: Int? { caretAt.map { min($0, ws.draft.utf16.count) } }
    private func walk(up: Bool) -> Bool {
        let history = SentHistory.list(ws.messages.filter(\.fromUser).map(\.text))
        guard let next = SentHistory.step(history, at: recalled, draft: ws.draft, up: up) else { return false }
        (recalled, ws.draft) = next
        return true
    }

    /// Return, ↑↓, Tab and Esc from the field.
    private func key(_ key: ComposerField.Key) -> Bool {
        switch key {
        case .submit:
            if let k = ws.cardKey, let options = liveCards, options.indices.contains(k) {
                Task { await ws.pick(options[k]) }
                return true
            }
            send()
            return true
        // ↑↓ walk the «/», «@» or «$» list; then the cards under the last reply, from an empty field: they sit above it,
        // so ↑ goes into them from the bottom and ↓ past the last one comes back to the field. Otherwise what was sent,
        // as a terminal's history: ↑ in an empty field or on a message brought back goes one further back, ↓ forward
        // and out to an empty field (SentHistory).
        case .up:
            if !suggestions.isEmpty { keyed(max(0, pick - 1)); return true }
            if nudge(up: true) { return true }
            if let options = liveCards { ws.cardKey = max(0, (ws.cardKey ?? options.count) - 1); return true }
            return walk(up: true)
        case .down:
            if !suggestions.isEmpty { keyed(min(suggestions.count - 1, pick + 1)); return true }
            if nudge(up: false) { return true }
            if let options = liveCards, let k = ws.cardKey { ws.cardKey = k + 1 < options.count ? k + 1 : nil; return true }
            return walk(up: false)
        case .tab:
            guard let row = picked else { return false }
            choose(row)
            return true
        case .escape:
            // Esc stops the model, as the Stop button does.
            if ws.modelRunning, suggestions.isEmpty { ws.stopModel(); return true }
            if ws.cardKey != nil, suggestions.isEmpty { ws.cardKey = nil; return true }
            guard !suggestions.isEmpty else { return false }
            dismissed = ws.draft
            return true
        }
    }

    private func keyed(_ row: Int) {
        pick = row
        keyedRow = row
        pointerAtKey = NSEvent.mouseLocation
    }

    /// The move waiting for Confirm, said in the field's own head; nil when nothing waits.
    private var waiting: String? {
        guard let text = ws.pendingText ?? ws.pendingAllocation.map({ ws.readable($0.summary) }) else { return nil }
        // «All» with its number, as the card on the canvas says it («Withdraw all 0.034429 SPCX from Kamino»): the wallet
        // alone can't tell what a protocol holds or what's owed.
        let drawn: String? = if case .wallet(let c)? = ws.pendingAction, text.contains("all "), let t = ws.route(c)?.title,
                                t.contains("all "), t.contains(where: \.isNumber) { t } else { nil }
        let shown = drawn ?? ws.withAmounts(text)
        return shown.prefix(1).uppercased() + shown.dropFirst()
    }

    /// The cards under the last reply while the keys may walk them: the field empty and nothing under way.
    private var liveCards: [ChatRouter.Option]? {
        guard ws.draft.isEmpty, !ws.busy, let steps = ws.liveSteps, ws.cardFold[steps.message] != true, !steps.options.isEmpty else { return nil }
        let options = steps.options
        return options
    }

    /// The draft's amount of a token the wallet holds, for the slider over the field and ↑↓ on the number.
    private var draftAmount: (amount: DraftAmount, held: Decimal, place: String)? {
        guard !ws.busy, waiting == nil else { return nil }
        let place = DraftAmount.place(ws.draft)
        for a in DraftAmount.all(ws.draft) { if let h = ws.draftHeld(a.ticker, place: place) { return (a, h.amount, h.place) } }
        return nil
    }

    /// ↑↓ with the caret on the draft's amount: a step up or down, between none and all that's held.
    private func nudge(up: Bool) -> Bool {
        guard let a = draftAmount, let caret, caret >= a.amount.range.location, caret <= NSMaxRange(a.amount.range) else { return false }
        let scale = DraftAmount.scale(a.held), step = pow(Decimal(10), -scale)
        let value = min(a.held, max(0, (a.amount.value + (up ? step : -step)).rounded(scale, up ? .down : .up)))
        // All that's held keeps every place: rounded, «max» left dust behind.
        let places = value == a.held ? 18 : scale
        ws.draft = a.amount.replacing(in: ws.draft, with: value, scale: places)
        caretRequest = a.amount.range.location + DraftAmount.written(value, scale: places).utf16.count
        return true
    }

    private var suggestions: [ComposerSuggestion] { ws.draft == dismissed ? [] : ComposerSuggestion.for(ws.draft, caret: caret, ws: ws) }
    private var picked: ComposerSuggestion? { let rows = suggestions; return rows.indices.contains(pick) ? rows[pick] : nil }

    var body: some View {
        let voice = ws.voice
        VStack(alignment: .leading, spacing: 8) {
            if voice.listening {
                HStack(spacing: 8) {
                    Circle().fill(Brand.accent).frame(width: 8, height: 8)
                        .background(Circle().fill(Brand.accent.opacity(0.25)).frame(width: 16, height: 16))
                    Text("Listening").font(.ui(12, .semibold))
                    Text("· \(voice.language) · \(voice.onDevice ? "recognized on this Mac" : "recognized by Apple")").font(.ui(10)).foregroundStyle(.secondary)
                }
                .frame(height: 30).padding(.horizontal, 6)
                .help("It listens in the keyboard's language: switch the layout before the microphone to speak another one.")
            } else {
                HStack(spacing: 6) {
                    ChatModelMenu()
                    NetworksChip()
                    Spacer(minLength: 0)
                    // What the chat fills of the model's window, or what a key paid for its answers; after the first answer.
                    ContextChip()
                }
            }
            if case .failed(let why) = voice.state {
                Label(why, systemImage: "mic.slash").font(.ui(10)).foregroundStyle(.orange).lineLimit(3)
            }
            if !ws.attached.isEmpty, !voice.listening { Flow(spacing: 6) { ForEach(ws.attached, id: \.title) { StepContextChip(step: $0) } } }
            if !voice.listening, case let rows = suggestions, !rows.isEmpty { suggestionList(rows) }
            if !voice.listening, let amount = ws.topUpDraft { TopUpBalances(amount: amount) }
            else if !voice.listening, let a = draftAmount { AmountSlider(amount: a.amount, held: a.held, place: a.place) }
            if voice.listening { listening(voice) } else if ws.hasModel { field } else { ConnectModelBar() }
        }
        .padding(12)
        .onChange(of: ws.draft) {
            pick = 0
            keyedRow = nil
            if ws.cardKey != nil { ws.cardKey = nil }
        }
        // A model on this Mac: whether its server answers and has it, before a field is offered.
        .task(id: ws.providerID) { if ws.provider.info.kind == .local { await ws.refreshLocalModels() } }
        // A contract or mint typed after «@» or «$»: asked once, for the list above the field.
        .task(id: ComposerSuggestion.mentionedAddress(ws.draft, caret: caret)) {
            if let address = ComposerSuggestion.mentionedAddress(ws.draft, caret: caret) { await ws.lookUpContract(address) }
        }
        // A «$0x…» chip anywhere in the text (pasted whole): its symbol and picture.
        .task(id: TokenMentions.addresses(ws.draft)) {
            for address in TokenMentions.addresses(ws.draft) { await ws.lookUpContract(address) }
        }
        // Whatever was heard goes into the field when listening ends (Done, or the recognizer stopping by itself);
        // the person reads it and sends it.
        .onChange(of: voice.listening) { was, now in
            if was, !now, !voice.transcript.isEmpty {
                ws.draft = [ws.draft, voice.transcript].filter { !$0.isEmpty }.joined(separator: " ")
            }
        }
    }

    private func listening(_ voice: VoiceInput) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            // What stopped changing in the ink, the recognizer's current guess in grey.
            let heard = voice.transcript, settled = heard.index(heard.startIndex, offsetBy: min(voice.settled, heard.count))
            Group {
                if heard.isEmpty { Text("Say what to do…").foregroundStyle(.secondary) }
                else { Text("\(Text(heard[..<settled]))\(Text(heard[settled...] + "…").foregroundStyle(.secondary))") }
            }
            .font(.ui(14)).lineLimit(5).frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 8) {
                VoiceBars(voice: voice)
                TimelineView(.periodic(from: voice.startedAt, by: 1)) { context in
                    let s = max(0, Int(context.date.timeIntervalSince(voice.startedAt)))
                    Text(String(format: "%d:%02d", s / 60, s % 60)).font(.ui(12, .semibold)).monospacedDigit().foregroundStyle(.secondary)
                }
                Button { voice.cancel() } label: { circleIcon("xmark", fill: Color.primary.opacity(0.08), ink: .primary) }
                    .buttonStyle(.plain).keyboardShortcut(.cancelAction).help("Cancel · Esc")
                Button { Task { await voice.finish() } } label: { circleIcon("checkmark", fill: Brand.accent, ink: Brand.onAccent) }
                    .buttonStyle(.plain).keyboardShortcut(.return, modifiers: []).help("Done: put the words in the field · Return")
            }
        }
        .padding(.top, 10).padding(.bottom, 5).padding(.leading, 14).padding(.trailing, 5)
        .background(RoundedRectangle(cornerRadius: 22, style: .continuous).fill(Brand.glassFill))
        .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Brand.accent, lineWidth: 1.5))
    }

    private func circleIcon(_ name: String, fill: Color, ink: Color) -> some View {
        Image(systemName: name).font(.system(size: 13, weight: .bold)).foregroundStyle(ink)
            .frame(width: 34, height: 34).background(Circle().fill(fill)).contentShape(Circle())
    }

    private var field: some View {
        let waiting = waiting
        return VStack(spacing: 0) {
            if let waiting { waitingBar(waiting) }
            // A signature waiting for the passcode takes the field's place (Passcode.swift).
            if let ask = CodePrompt.shared.pending { CodeEntryRow(ask: ask) } else { entry(waiting: waiting != nil) }
        }
        .background(RoundedRectangle(cornerRadius: 22, style: .continuous).fill(Brand.glassFill))
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        // A glass pill on the glass chat, its rim orange while a move waits for Confirm.
        .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(waiting == nil ? Brand.glassRim : Brand.accent, lineWidth: waiting == nil ? 1 : 1.5))
    }

    /// Variant E: while a move waits, the field carries it — what it is, Cancel and Confirm in one place, the orange bar
    /// above the field gone. The steps stay on the canvas; the field below still asks about it or changes it.
    private func waitingBar(_ title: String) -> some View {
        // Once a second while a priced swap's Confirm counts down its 15 s after the quote, then Refresh asks again
        // (quoteState). Otherwise nothing here changes with the time, and each tick redraws the window.
        Group {
            if case .fresh = ws.quoteState(at: Date()) {
                TimelineView(.periodic(from: .now, by: 1)) { context in waitingRow(title, state: ws.quoteState(at: context.date)) }
            } else {
                waitingRow(title, state: ws.quoteState(at: Date()))
            }
        }
        .padding(.vertical, 10).padding(.leading, 14).padding(.trailing, 9)
        .background(Brand.accent.opacity(0.10))
    }

    private func waitingRow(_ title: String, state: Workspace.QuoteState) -> some View {
        let sign = ws.pendingText == nil
        // A swap: what it gets and at what rate, as quoted now; else where its steps are.
        let line: String = {
            if ws.executing { return "Signing…" }
            guard case .wallet(let c)? = ws.pendingAction, state != .none else { return "Its steps are on the canvas · signs after \(Presence.signName)" }
            switch state {
            case .quoting: return "Getting the price…"
            case .stale: return (ws.swapLine(c).map { $0 + " · " } ?? "") + "the price is over 15 s old"
            case .fresh(let left): return (ws.swapLine(c) ?? "Quoted") + " · \(left) s"
            case .none: return ""
            }
        }()
        let what = HStack(alignment: .top, spacing: 9) {
            Circle().fill(Brand.accent).frame(width: 8, height: 8).padding(.top, 5)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.ui(13, .semibold)).lineLimit(3).fixedSize(horizontal: false, vertical: true)
                Text(line).font(.ui(11)).foregroundStyle(.secondary).monospacedDigit().lineLimit(2).fixedSize(horizontal: false, vertical: true)
            }
        }
        let buttons = HStack(spacing: 6) {
            Button { Task { await ws.send("cancel") } } label: {
                Text("Cancel").font(.ui(12.5, .medium)).padding(.horizontal, 13).frame(height: 32)
                    .overlay(Capsule().strokeBorder(Brand.cardBorder)).contentShape(Capsule())
            }
            .buttonStyle(.plain).pointerStyle(.link)
            switch state {
            case .stale:
                Button { ws.requoteSwap() } label: {
                    Label("Refresh", systemImage: "arrow.clockwise").font(.ui(12.5, .bold)).foregroundStyle(Brand.onAccent)
                        .padding(.horizontal, 14).frame(height: 32).background(Capsule().fill(Brand.accent)).contentShape(Capsule())
                }
                .buttonStyle(.plain).pointerStyle(.link).help("Ask for the price again; Confirm comes back for 15 s")
            case .quoting:
                Label("Quoting…", systemImage: "hourglass").font(.ui(12.5, .bold)).foregroundStyle(.secondary)
                    .padding(.horizontal, 14).frame(height: 32).background(Capsule().fill(Color.primary.opacity(0.08)))
            default:
                Button { Task { await ws.pressConfirm() } } label: {
                    Label(sign ? "Sign" : "Confirm", systemImage: Presence.signIcon).font(.ui(12.5, .bold)).foregroundStyle(Brand.onAccent)
                        .padding(.horizontal, 14).frame(height: 32).background(Capsule().fill(Brand.accent)).contentShape(Capsule())
                }
                .buttonStyle(.plain).pointerStyle(.link).help(title)
            }
        }
        .disabled(ws.busy || ws.executing)
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: 10) { what; Spacer(minLength: 6); buttons }
            VStack(alignment: .leading, spacing: 8) { what; HStack { Spacer(); buttons } }
        }
    }

    private func entry(waiting: Bool) -> some View {
        HStack(alignment: .bottom, spacing: 8) {
            Button { templatesOpen.toggle() } label: {
                Image(systemName: "square.grid.2x2").font(.system(size: 14, weight: .semibold))
                    .frame(width: 34, height: 34)
                    .background(Circle().fill(Brand.glassFill))
                    .overlay(Circle().strokeBorder(Brand.glassRim))
            }
            .buttonStyle(.plain)
            .pointerStyle(.link)
            .foregroundStyle(.secondary)
            .help("Plan templates")
            .popover(isPresented: $templatesOpen, arrowEdge: .top) {
                TemplatesPanel { phrase in ws.suggest(phrase); templatesOpen = false }
            }
            // «$SOL» shows as the token's chip (ComposerField); Shift-Return breaks the line, Return sends.
            ZStack(alignment: .topLeading) {
                if ws.draft.isEmpty {
                    Text(waiting ? "Ask about it, change it («make it 0.02»), or something else"
                         : ws.attached.isEmpty ? "What should we do? / blocks, @ names, $ tokens"
                         : ws.attached.count == 1 ? "Ask about it, or say what to do with it…" : "Ask about these \(ws.attached.count), or say what to do with them…")
                        .font(.ui(14)).foregroundStyle(.tertiary).lineLimit(1)
                        .padding(.vertical, ComposerField.inset.height).padding(.leading, 5)
                        .allowsHitTesting(false)
                }
                ComposerField(text: Bindable(ws).draft, caret: $caretAt, moveCaret: $caretRequest, select: Bindable(ws).draftSelection, onKey: key,
                              known: TokenMentions.addresses(ws.draft).filter { ws.contractLookups[$0] != nil }.count,
                              face: ws.tokenFace)
            }
            Button { Task { await ws.voice.start() } } label: {
                Image(systemName: "mic").font(.system(size: 14, weight: .semibold))
                    .frame(width: 34, height: 34).contentShape(Circle())
            }
            .buttonStyle(.hoverIcon)
            .help("Speak instead of typing")
            if ws.modelRunning || ws.executing {
                // Stop while the model answers; a signed move can't be called back, so then it waits, saying why.
                Button { ws.stopModel() } label: {
                    Image(systemName: "stop.fill").font(.system(size: 11, weight: .bold))
                        .foregroundStyle(ws.executing ? AnyShapeStyle(.tertiary) : AnyShapeStyle(Brand.onAccent))
                        .frame(width: 34, height: 34)
                        .background(Circle().fill(ws.executing ? Color.primary.opacity(0.08) : Brand.accent))
                }
                .buttonStyle(.plain)
                .pointerStyle(ws.executing ? nil : .link)
                .disabled(ws.executing)
                .help(ws.executing ? "A signed move is going through: it can't be called back. The chat says when it lands."
                                   : "Stop — the model stops, nothing runs (Esc)")
                .accessibilityLabel("Stop")
            } else {
                Button(action: send) {
                    Image(systemName: "arrow.up").font(.system(size: 13, weight: .bold))
                        .foregroundStyle(ws.draft.isEmpty ? AnyShapeStyle(.tertiary) : AnyShapeStyle(Brand.onAccent))
                        .frame(width: 34, height: 34)
                        .background(Circle().fill(ws.draft.isEmpty ? Color.primary.opacity(0.08) : Brand.accent))
                }
                .buttonStyle(.plain)
                .pointerStyle(ws.draft.isEmpty || ws.busy ? nil : .link)
                .disabled(ws.draft.isEmpty || ws.busy)
                .keyboardShortcut(.return, modifiers: .command)
            }
        }
        .padding(.vertical, 5).padding(.leading, 5).padding(.trailing, 5)
        // A glass pill on the glass chat: white in light mode, faint in dark.
        .background(RoundedRectangle(cornerRadius: 22, style: .continuous).fill(Brand.glassFill))
        // Under a waiting move, the corners past its rounded top take the move's tint: the frame's own colour showed
        // there as a third one, left and right of the field.
        .background { if waiting { TopCorners(radius: 22).fill(Brand.accent.opacity(0.10)) } }
        .overlay(RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(Brand.glassRim))
    }

    private func send() {
        // Return with the list open puts the highlighted row in; the next Return sends.
        if let row = picked { choose(row); return }
        // The model and the parsers read «SOL»: the «$» only made the chip. The chat shows it as typed, chips and all.
        let typed = ws.draft.trimmingCharacters(in: .whitespacesAndNewlines), text = TokenMentions.plain(typed)
        guard !text.isEmpty, !ws.busy else { return }
        ws.draft = ""
        Task { await ws.send(text, shown: typed == text ? nil : typed) }
    }

    private func choose(_ row: ComposerSuggestion) {
        ws.draft = row.draft
        // The caret right after what went in, so the rest of the text stays where it was.
        caretRequest = min(row.caret, row.draft.utf16.count)
        // A block's example is finished and sent by the person: the list stays closed on it.
        dismissed = row.draft
    }

    private func suggestionList(_ rows: [ComposerSuggestion]) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            // Every row: past eight it scrolls, and the one ↑↓ picked stays in view.
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                            Button { choose(row) } label: {
                                HStack(spacing: 8) {
                                    rowIcon(row, lit: i == pick).frame(width: 18, height: 18)
                                    Text(row.title).font(.ui(12.5, .semibold)).lineLimit(1).layoutPriority(1)
                                    Text(row.detail).font(.ui(11.5)).foregroundStyle(.secondary).lineLimit(1)
                                    Spacer(minLength: 0)
                                }
                                .padding(.horizontal, 9).frame(height: 30)
                                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(i == pick ? Brand.accent.opacity(0.14) : .clear))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain).pointerStyle(.link)
                            .onHover { if $0, NSEvent.mouseLocation != pointerAtKey { pick = i } }
                            .id(i)
                        }
                    }
                }
                .frame(height: CGFloat(min(rows.count, 8)) * 32 - 2)
                .onChange(of: keyedRow) { if let keyedRow { proxy.scrollTo(keyedRow) } }
            }
            Text(rows.count > 8 ? "\(rows.count) · scroll or ↑↓ · Return puts it in · Esc closes" : "↑↓ choose · Return puts it in · Esc closes")
                .font(.ui(10)).foregroundStyle(.tertiary).padding(.horizontal, 9).padding(.top, 2)
        }
        .padding(5)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Brand.glassFill))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Brand.glassRim))
        .transition(.opacity)
    }

    @ViewBuilder private func rowIcon(_ row: ComposerSuggestion, lit: Bool) -> some View {
        if let url = row.image {
            AsyncImage(url: url) { $0.resizable().scaledToFill() } placeholder: { Circle().fill(Color.primary.opacity(0.08)) }
                .frame(width: 16, height: 16).clipShape(Circle())
        } else if let logo = row.logo {
            TokenLogo(name: logo, size: 16)
        } else {
            Image(systemName: row.icon).font(.system(size: 12, weight: .semibold)).foregroundStyle(lit ? Brand.accentInk : Color.secondary)
        }
    }
}

/// A row over the chat's field. «/» lists the blocks (BlockCatalog): a pick puts its example in the field to finish.
/// «@» at the start of a word lists the address book and the favorite tokens: a pick puts the name in its place. A contract
/// or mint after «@» is looked up (DexScreener) and a pick puts the address in, now known by its ticker.
struct ComposerSuggestion: Identifiable, Equatable {
    var id: String { icon + title + detail }
    let icon: String
    let title: String
    let detail: String
    /// The field's text once it's picked, and where the caret goes then (UTF-16 offset).
    let draft: String
    var caret = 0
    /// A small logo in place of the symbol: TokenLogo's name, or a token's picture.
    var logo: String?
    var image: URL?

    /// The word the caret is in, with the text before and after it.
    static func word(_ draft: String, caret: Int?) -> (text: Substring, before: String, after: String) {
        let at = String.Index(utf16Offset: min(max(0, caret ?? draft.utf16.count), draft.utf16.count), in: draft)
        let start = draft[..<at].lastIndex(where: \.isWhitespace).map { draft.index(after: $0) } ?? draft.startIndex
        let end = draft[at...].firstIndex(where: \.isWhitespace) ?? draft.endIndex
        return (draft[start..<end], String(draft[..<start]), String(draft[end...]))
    }

    /// The contract or mint typed after «@» or «$» in the word at the caret, as typed (a mint's case matters).
    static func mentionedAddress(_ draft: String, caret: Int?) -> String? {
        let w = word(draft, caret: caret).text
        guard w.hasPrefix("@") || w.hasPrefix("$") else { return nil }
        let address = String(w.dropFirst())
        return address.wholeMatch(of: /0x[0-9a-fA-F]{40}|[1-9A-HJ-NP-Za-km-z]{32,44}/) != nil ? address : nil
    }

    static func `for`(_ draft: String, caret: Int?, ws: Workspace) -> [ComposerSuggestion] {
        let w = word(draft, caret: caret)
        // A pick replaces the word; the rest of the text stays, the caret after what went in.
        // `space`: a space after it even alone in the field — «$SOL » becomes its chip only once something closes it.
        func row(_ icon: String, _ title: String, _ detail: String, put text: String, space: Bool = false, logo: String? = nil, image: URL? = nil) -> ComposerSuggestion {
            let alone = w.before.isEmpty && w.after.isEmpty
            let head = w.before + text + (alone && !space ? "" : " ")
            let tail = w.after.hasPrefix(" ") ? String(w.after.dropFirst()) : w.after
            return ComposerSuggestion(icon: icon, title: title, detail: detail, draft: head + tail, caret: head.utf16.count, logo: logo, image: image)
        }
        if w.text.hasPrefix("/") {
            let q = w.text.dropFirst().lowercased()
            return BlockCatalog.all.filter { !$0.automatic && !$0.example.isEmpty }
                .filter { q.isEmpty || "\($0.title) \($0.group) \($0.example)".lowercased().contains(q) }
                .map { row("square.grid.2x2", $0.title, $0.example, put: $0.example, logo: $0.logo) }
        }
        // A contract or mint after «@» or «$»: one row per network it trades on, the deepest pool first. «$» puts
        // «$0x… » in, drawn as the token's chip with its market's picture; «@» the bare address.
        if let address = mentionedAddress(draft, caret: caret) {
            let dollar = w.text.hasPrefix("$"), put = (dollar ? "$" : "") + address
            let short = "\(address.prefix(6))…\(address.suffix(4))"
            guard let found = ws.contractLookups[address] else { return [row("magnifyingglass", short, "looking it up…", put: put, space: dollar)] }
            guard !found.isEmpty else { return [row("questionmark.circle", short, "no market knows it", put: put, space: dollar)] }
            var chains = Set<String>()
            return found.filter { chains.insert($0.chain).inserted }.map { t in
                let net = ModuleNetwork(place: t.chain)
                let detail = [t.name, net?.name ?? t.chain.capitalized, t.priceUSD.map { Fmt.usd($0) }, t.liquidityUSD.map { "liquidity \(Fmt.usd($0))" }]
                    .compactMap { $0 }.joined(separator: " · ")
                return row("circle.dotted", t.symbol, detail, put: (dollar ? "$" : "") + t.address, space: dollar,
                           logo: t.imageURL == nil ? net?.logo : nil, image: t.imageURL)
            }
        }
        // «$» lists the tokens: what the wallet holds first, then the usual ones and the favorites. «$5» is dollars.
        if w.text.hasPrefix("$"), !(w.text.dropFirst().first?.isNumber ?? false) {
            let q = w.text.dropFirst().lowercased()
            return tokens(ws).filter { q.isEmpty || $0.symbol.lowercased().hasPrefix(q) || $0.detail.lowercased().contains(q) }
                .prefix(40).map { t in
                    // The app's own logo first, the token's picture from the network when it has none.
                    let own = TokenLogo.name(forAsset: t.symbol)
                    return row("dollarsign.circle", t.symbol, t.detail, put: "$" + t.symbol, space: true, logo: own, image: own == nil ? t.image : nil)
                }
        }
        guard w.text.hasPrefix("@") else { return [] }
        let q = w.text.dropFirst().lowercased()
        let names = ws.addressBook.map { row("person.crop.circle", $0.name, $0.network.title, put: $0.name, logo: $0.network.logo) }
            + ws.favoriteTokens.map { row("star", $0.symbol, "\($0.name) · \($0.chainTitle)", put: $0.symbol, logo: $0.image == nil ? $0.chainLogo : nil, image: $0.image) }
        return names.filter { q.isEmpty || $0.title.lowercased().contains(q) }
    }

    /// The «$» list's tokens, each once: the wallet's (SOL and USDC on Solana, then the tokens on show by value), the
    /// usual ones, the favorites.
    static func tokens(_ ws: Workspace) -> [(symbol: String, detail: String, image: URL?)] {
        var seen = Set<String>(), out: [(symbol: String, detail: String, image: URL?)] = []
        func add(_ symbol: String, _ detail: String, image: URL? = nil) {
            guard !symbol.isEmpty, symbol.count <= 12, seen.insert(symbol.uppercased()).inserted else { return }
            out.append((symbol, detail, image))
        }
        if let s = ws.solana {
            add("SOL", "Solana · \(Fmt.num(s.sol, maxFraction: 4)) SOL")
            add("USDC", "Solana · \(Fmt.num(s.usdc, maxFraction: 2)) USDC")
        }
        for t in ws.shownTokens.sorted(by: { ($0.usd ?? 0) > ($1.usd ?? 0) }) {
            add(t.symbol, "\(ModuleNetwork(place: t.chain)?.name ?? t.chain.capitalized) · \(t.usd.map { Fmt.usd($0) } ?? Fmt.num(t.amount))", image: t.icon)
        }
        for (symbol, name) in [("SOL", "Solana"), ("USDC", "USD Coin"), ("ETH", "Ether"), ("BTC", "Bitcoin"), ("HYPE", "Hyperliquid"),
                               ("NEAR", "NEAR"), ("USDT", "Tether"), ("JUP", "Jupiter")] {
            add(symbol, name)
        }
        for f in ws.favoriteTokens { add(f.symbol, "\(f.name) · \(f.chainTitle)", image: f.image) }
        return out
    }
}

/// «[logo] GPT 6 SOL ▾»: the model for the chat, a chip above its field. The choice is kept per canvas.
/// The menu lists companies; one with several ways in (subscription, API key) always stands open, so the menu keeps
/// its height: a popover that resizes moves itself, and the row under the pointer with it.
struct ChatModelMenu: View {
    @Environment(Workspace.self) private var ws
    @State private var open = false
    /// The company whose ways in and models the right column shows: the one in use when the menu opens.
    @State private var shown: String?
    /// The companies' column height; the right column scrolls within it, so the menu never changes size.
    @State private var listHeight: CGFloat = 420
    /// When the popover last closed: a click on the chip closes it first, and the same click must not reopen it.
    @State private var closedAt = Date.distantPast
    /// Providers with a saved login, read once per opening: each Keychain read is a round trip to securityd.
    @State private var linked: Set<String> = []

    var body: some View {
        Button {
            if open { open = false; return }
            guard Date.now.timeIntervalSince(closedAt) > 0.3 else { return }
            linked = Set(LLMCatalog.all.map(\.info.id).filter { Keychain.load($0) != nil })
            shown = (ModelCompany.of(ws.providerID) ?? ModelCompany.all.first)?.name
            open = true
        } label: {
            // A chip above the chat's field, next to the networks.
            HStack(spacing: 6) {
                if let c = ModelCompany.of(ws.providerID) { ModelCompanyLogo(company: c, size: 20).clipShape(Circle()) }
                Text(ws.model.isEmpty ? ws.provider.info.name : LLMCatalog.displayName(model: ws.model)).lineLimit(1)
                    .font(.ui(12, .semibold))
                Image(systemName: "chevron.down").font(.system(size: 8, weight: .bold)).foregroundStyle(.secondary)
            }
            .padding(.leading, 5).padding(.trailing, 9).frame(height: 30)
            .background(Capsule().fill(open ? Brand.accent.opacity(0.14) : Brand.glassFill))
            .overlay(Capsule().strokeBorder(Brand.glassRim))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .fixedSize()
        .help("\(ws.provider.info.name) · \(ws.model). The model for this canvas; it never sees your wallet key.")
        .popover(isPresented: $open, arrowEdge: .top) { menu }
        // The live list is read when the chip appears and after the menu closes — never while it's open, where a new list
        // swapped the rows under the pointer («модели прыгают»).
        .task { await ws.refreshLiveModels() }
        .onChange(of: open) {
            if !open { closedAt = .now; Task { await ws.refreshLiveModels() } }
            // Closing doesn't clear what Settings, opened from this menu, has just set.
            if open { ws.tuning = "Picking a model" } else if ws.tuning == "Picking a model" { ws.tuning = nil }
        }
    }

    private var menu: some View {
        VStack(alignment: .leading, spacing: 2) {
            // Two columns, so nothing opens between the rows and nothing moves under the pointer: the companies on the
            // left, the chosen one's ways in and models on the right.
            HStack(alignment: .top, spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    caption("MODEL FOR THIS CANVAS")
                    ForEach(ModelCompany.all.filter { !$0.local }) { companyRow($0) }
                    caption("ON THIS MAC · NO INTERNET").padding(.top, 6)
                    ForEach(ModelCompany.all.filter(\.local)) { companyRow($0) }
                }
                .frame(width: 230)
                .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { listHeight = $0 }
                Divider()
                ScrollView {
                    if let c = ModelCompany.all.first(where: { $0.name == shown }) {
                        ways(c).id(c.name).transition(.opacity)
                    }
                }
                .frame(width: 260, height: listHeight)
            }
            Divider().padding(.vertical, 4)
            HStack {
                Label("The model never sees your wallet key.", systemImage: "lock.shield").font(.ui(10)).foregroundStyle(.secondary)
                Spacer()
                Button("Sign-ins…") {
                    open = false
                    ws.openSettings(.signIns)
                }
                .buttonStyle(.borderless).font(.ui(10))
            }
            .padding(.horizontal, 10).padding(.bottom, 4)
        }
        .padding(8)
        // What Ollama and LM Studio have downloaded, read as the menu opens.
        .task { await ws.refreshLocalModels() }
    }

    private func caption(_ text: String) -> some View {
        Text(text).font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            .padding(.horizontal, 10).padding(.top, 6).padding(.bottom, 4)
    }

    /// A company on the left: a tap shows its ways in on the right; nothing is picked until a model is.
    private func companyRow(_ c: ModelCompany) -> some View {
        let ways = c.ids.compactMap { LLMCatalog.provider(id: $0)?.info }
        let current = ways.first { $0.id == ws.providerID }
        return CompanyRow(company: c, detail: summary(ways, current: current), current: current != nil, selected: shown == c.name) {
            withAnimation(.easeOut(duration: 0.12)) { shown = c.name }
        }
    }

    /// The company's ways in, each with its models. Only the model in use is checked.
    private func ways(_ c: ModelCompany) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            caption(c.name.uppercased())
            ForEach(c.ids.compactMap { LLMCatalog.provider(id: $0)?.info }) { way($0) }
        }
    }

    /// A subscription stands out in the menu: its models come with what's already paid for, no key and no per-request bill.
    static let subscriptionInk = Color(light: 0x1F7A4A, dark: 0x6FD39C)

    @ViewBuilder private func way(_ info: ProviderInfo) -> some View {
        let icon = info.kind == .subscription ? "person.crop.circle.fill" : info.kind == .local ? "desktopcomputer" : "key"
        let iconTint = info.kind == .subscription ? Self.subscriptionInk : nil
        let list = connected(info) && info.unavailable == nil ? models(info) : []
        if list.isEmpty {
            // Nothing to choose inside it: the way in is the choice, or its sign-in.
            MenuRow(title: info.name, detail: info.unavailable == nil ? detail(info) : "not open to apps yet — use the API key",
                    icon: icon, iconTint: iconTint, checked: info.id == ws.providerID) { pick(info) }
                .disabled(info.unavailable != nil)
                .opacity(info.unavailable != nil ? 0.5 : 1)
                .help(info.unavailable ?? info.note)
        } else {
            HStack(spacing: 10) {
                Image(systemName: icon).frame(width: 18).foregroundStyle(iconTint ?? .secondary)
                Text(info.name).font(.ui(11, .semibold))
                Spacer(minLength: 8)
                Text(info.kind == .local ? "this Mac · \(Int(OllamaModels.memoryGB.rounded())) GB memory" : "connected").font(.ui(10))
            }
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10).padding(.top, 8).padding(.bottom, 2)
            .help(info.note)
            // Ollama that didn't answer: open it, or get it — models on this Mac need it running.
            if info.id == "ollama", ws.localServers["ollama"] == false {
                if OllamaModels.installed {
                    MenuRow(title: "Ollama isn't running", detail: "start it, then open this menu again", icon: "exclamationmark.triangle") { OllamaModels.start() }
                    .padding(.leading, 28)
                } else {
                    MenuRow(title: ws.ollamaInstall ?? "Get Ollama", detail: "free · the newest release, then a model that fits this Mac", icon: "arrow.down.circle") {
                        Task { await ws.setUpLocal() }
                    }
                    .disabled(ws.ollamaInstall != nil)
                    .padding(.leading, 28)
                }
            }
            ForEach(list, id: \.self) { id in
                // A model too small for the chat's brief stays listed, faded, with the reason.
                let off = LLMCatalog.chatRefusal(model: id), absent = missing(info, id)
                // Not downloaded in Ollama: a click downloads it here, with its progress; one too big for this Mac stays faded.
                let loading = ws.modelDownloads[id], size = ws.modelSizes[id]
                let pullable = absent && info.id == "ollama" && ws.localServers["ollama"] == true
                let tooBig = pullable && size.map { !OllamaModels.fits(sizeGB: $0) } == true
                MenuRow(title: LLMCatalog.displayName(model: id),
                        detail: loading.map { "downloading · \(Int($0 * 100))%" }
                            ?? (tooBig ? "≈ \(Fmt.num(Decimal(size ?? 0), maxFraction: 1)) GB · too big for this Mac"
                                : pullable ? "Download · \(size.map { "≈ \(Fmt.num(Decimal($0), maxFraction: 1)) GB" } ?? "not on this Mac yet")"
                                : absent ? "not downloaded" : off != nil ? "too small for the chat"
                                : details(info, id)),
                        icon: pullable && loading == nil && !tooBig ? "arrow.down.circle" : nil, iconTint: Brand.accentInk,
                        // Not on this Mac: greyed like a disabled row — it can't be picked, only downloaded.
                        checked: info.id == ws.providerID && id == chosen(info), tint: absent ? .secondary : .primary) {
                    if pullable { Task { await ws.downloadModel(id) } } else { pick(info, model: id) }
                }
                    .padding(.leading, 28)
                    .disabled(off != nil || (absent && (!pullable || tooBig || loading != nil)))
                    .opacity(off != nil || (absent && (!pullable || tooBig)) ? 0.5 : 1)
                    .help(tooBig ? "Needs about \(Fmt.num(Decimal((size ?? 0) * 1.2), maxFraction: 0)) GB of memory; this Mac has \(Int(OllamaModels.memoryGB.rounded())) GB."
                          : pullable ? "Downloads \(id) into Ollama on this Mac; then pick it here." : absent ? "Not downloaded in \(info.name) yet." : off ?? id)
            }
        }
    }

    /// A way's models: the live list for a subscription, else the checked one. A model typed in Settings goes last,
    /// so the listed ones keep their places.
    private func models(_ info: ProviderInfo) -> [String] {
        let chosen = chosen(info)
        var listed = ws.liveModels[info.id] ?? info.models
        // On this Mac the downloaded models come first; the suggested ones not downloaded stay, marked (`missing`).
        if info.kind == .local, ws.liveModels[info.id] != nil { listed += info.models.filter { !listed.contains($0) } }
        return listed.contains(chosen) || chosen.isEmpty ? listed : listed + [chosen]
    }

    /// What a model row says: recommended, the memory a model on this Mac takes, and how it did in the chat's exam.
    private func details(_ info: ProviderInfo, _ id: String) -> String? {
        // On this Mac: the biggest model that fits its memory, not one name for every Mac.
        let parts = [id == (info.kind == .local ? ws.localPick ?? info.defaultModel : info.defaultModel) ? "recommended" : nil,
                     info.kind == .local ? ws.modelSizes[id].map { "≈ \(Fmt.num(Decimal($0 * 1.2), maxFraction: 0)) GB memory" } : nil,
                     ws.score(info.id, id).map { ($0.good ? "✓ " : "⚠︎ ") + $0.short }].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// Listed for a server on this Mac that answered, but not downloaded there.
    private func missing(_ info: ProviderInfo, _ id: String) -> Bool {
        info.kind == .local && ws.liveModels[info.id].map { !$0.contains(id) } == true
    }

    private func chosen(_ info: ProviderInfo) -> String { ws.modelChoices[info.id] ?? info.defaultModel }

    /// Under the company: the model in use, or else how to get in.
    private func summary(_ ways: [ProviderInfo], current: ProviderInfo?) -> String {
        if let current {
            let model = chosen(current)
            return (model.isEmpty ? current.name : LLMCatalog.displayName(model: model)) + " · in use"
        }
        if ways.count == 1, let only = ways.first { return detail(only) }
        return ways.filter { $0.unavailable == nil }.map { $0.kind == .subscription ? "Subscription" : "API key" }
            .joined(separator: " or ")
    }

    private func pick(_ info: ProviderInfo, model: String? = nil) {
        ws.providerID = info.id
        if let model { ws.model = model }
        // Loaded into memory now, so the first answer doesn't wait for it.
        if info.id == "ollama", let model { OllamaModels.preload(model) }
        ws.saveCanvas()
        open = false
        if !connected(info) { ws.openSettings(.signIns) }
    }

    private func connected(_ info: ProviderInfo) -> Bool { info.kind == .local || linked.contains(info.id) }

    private func detail(_ info: ProviderInfo) -> String {
        let model = chosen(info)
        let state = info.kind == .local ? "on this Mac" : connected(info) ? "connected" : "sign-in needed"
        let shown = info.takesKey && !connected(info) ? "any model by key" : model.isEmpty ? "" : LLMCatalog.displayName(model: model)
        return [shown, state].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// A model maker in the chat's model menu: its logo on a brand tile and its ways in (catalog ids).
struct ModelCompany: Identifiable {
    let name: String
    /// Logos/<logo>.png
    let logo: String
    let tile: UInt32
    /// The color for a one-color logo; nil keeps the logo's own colors.
    var ink: UInt32?
    let ids: [String]
    var id: String { name }
    var local: Bool { LLMCatalog.provider(id: ids[0])?.info.kind == .local }

    static let known: [ModelCompany] = [
        .init(name: "OpenAI", logo: "llm-openai", tile: 0x000000, ink: 0xFFFFFF, ids: ["chatgpt", "openai"]),
        .init(name: "Anthropic", logo: "llm-claude", tile: 0xF4F3EE, ids: ["claude-code", "anthropic"]),
        .init(name: "Google", logo: "llm-gemini", tile: 0xFFFFFF, ids: ["gemini-subscription", "gemini"]),
        .init(name: "xAI", logo: "llm-grok", tile: 0x000000, ink: 0xFFFFFF, ids: ["grok-subscription", "xai"]),
        .init(name: "DeepSeek", logo: "llm-deepseek", tile: 0xFFFFFF, ids: ["deepseek"]),
        .init(name: "OpenRouter", logo: "llm-openrouter", tile: 0x6566F1, ink: 0xFFFFFF, ids: ["openrouter"]),
        .init(name: "Ollama", logo: "llm-ollama", tile: 0xFFFFFF, ink: 0x000000, ids: ["ollama"]),
        .init(name: "LM Studio", logo: "llm-lmstudio", tile: 0x4338CA, ink: 0xFFFFFF, ids: ["lmstudio"]),
        // Apple's logo is a system symbol, not a file: «sf:» names it.
        .init(name: "Apple", logo: "sf:apple.logo", tile: 0x000000, ink: 0xFFFFFF, ids: ["apple"]),
    ]
    /// A provider added to the catalog but not to a company above still shows, as its own company with a lettered tile.
    static let all = known + LLMCatalog.all.map(\.info).filter { info in !known.contains { $0.ids.contains(info.id) } }
        .map { ModelCompany(name: $0.name, logo: "", tile: 0x8E8E93, ids: [$0.id]) }

    static func of(_ providerID: String) -> ModelCompany? { all.first { $0.ids.contains(providerID) } }
}

/// The company's logo on its brand tile, like an app icon.
struct ModelCompanyLogo: View {
    let company: ModelCompany
    var size: CGFloat = 28

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: size * 0.26, style: .continuous)
        shape.fill(Color(hex: company.tile))
            .overlay { glyph.frame(width: size * 0.64, height: size * 0.64) }
            .overlay(shape.strokeBorder(Color.primary.opacity(0.12), lineWidth: 0.5))
            .frame(width: size, height: size)
    }

    @ViewBuilder private var glyph: some View {
        if company.logo.hasPrefix("sf:") {
            Image(systemName: String(company.logo.dropFirst(3))).resizable().aspectRatio(contentMode: .fit)
                .foregroundStyle(Color(hex: company.ink ?? 0xFFFFFF))
        } else if !company.logo.isEmpty, let image = TokenLogo.image(company.logo) {   // "" found a token's logo (a Shiba Inu)
            if let ink = company.ink {
                Image(nsImage: image).renderingMode(.template).resizable().interpolation(.high).aspectRatio(contentMode: .fit)
                    .foregroundStyle(Color(hex: ink))
            } else {
                Image(nsImage: image).resizable().interpolation(.high).aspectRatio(contentMode: .fit)
            }
        } else {
            Text(company.name.prefix(1)).font(.ui(size * 0.5, .bold)).foregroundStyle(.white)
        }
    }
}

private struct CompanyRow: View {
    let company: ModelCompany
    let detail: String
    /// The model in use is this company's.
    let current: Bool
    /// Its ways in are the ones on the right.
    let selected: Bool
    let action: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                ModelCompanyLogo(company: company)
                VStack(alignment: .leading, spacing: 2) {
                    Text(company.name).font(.ui(13, .semibold))
                    Text(detail).font(.ui(10)).foregroundStyle(current ? AnyShapeStyle(Brand.accentInk) : AnyShapeStyle(.secondary)).lineLimit(1)
                }
                Spacer(minLength: 8)
                Image(systemName: "chevron.right").font(.system(size: 10, weight: .bold))
                    .foregroundStyle(selected ? .secondary : .tertiary)
            }
            .padding(.horizontal, 8).padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.primary.opacity(selected ? 0.08 : hover ? 0.05 : 0)))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}

/// Ready phrases for the schemes ihatecrypto builds; a tap puts the phrase into the field.
struct TemplatesPanel: View {
    let pick: (String) -> Void

    static let templates: [(title: String, phrase: String, colors: [UInt32])] = [
        ("Trade by the book", "long ETH by the book", [0x9945FF, 0x4615C8, 0x072723]),
        ("Short on a pullback", "short oil on a 2% pullback by the book", [0x072723, 0x97FCE4]),
        ("Best rates", "spread USDC across the best rates", [0x2775CA, 0x131D27, 0x082A56]),
        ("Where to keep it", "where to keep USDC", [0x2775CA, 0xC7F284]),
        ("Every week", "buy SOL for 2 USDC every week", [0x2775CA, 0x9945FF]),
        ("Back to Solana", "withdraw 1 USDC to Solana", [0x072723, 0x4615C8, 0x9945FF]),
        ("Protect a position", "move stop to breakeven", [0x072723, 0xF59E0B]),
        ("Stake SOL", "stake 0.01 SOL", [0x9945FF, 0x1F9D6A]),
        ("Aave on Base", "lend 2 USDC on Aave", [0x2775CA, 0x0052FF, 0xB6509E]),
        ("Price alert", "alert me when ETH is below 2,500", [0x627EEA, 0xF59E0B]),
        ("Buy a stock token", "buy SpaceX stock for 2 USDC", [0x2775CA, 0x4615C8, 0xCCFF00]),
        ("Profit and loss", "profit and loss week", [0x0B7A55, 0xB42318]),
        ("What's moving", "what's moving", [0xF59E0B, 0x072723]),
        ("Chart a coin", "chart SOL week", [0x9945FF, 0x072723]),
        ("Buy a memecoin", "buy BONK on solana for 1 USDC", [0x2775CA, 0xF7931A]),
        ("Swap on Base", "swap 2 USDC for ETH on base", [0x2775CA, 0x0052FF, 0x627EEA]),
        ("Move to Base", "move 2 USDC from Solana to Base", [0x9945FF, 0x2775CA, 0x0052FF]),
        ("Sweep dust", "sweep dust", [0xF59E0B, 0x2775CA]),
        ("Claim rewards", "claim rewards", [0x1F9D6A, 0x0052FF]),
        ("Hyperliquid spot", "buy HYPE on hyperliquid spot for 10 USDC", [0x072723, 0x97FCE4]),
        ("Trade gold", "long gold by the book", [0xD4AF37, 0x072723]),
        ("Make a multisig", "make a multisig", [0x0052FF, 0x072723])
    ]

    /// The start screen's block cards: a picture, its colour and one line on what the block does.
    static let looks: [String: (icon: String, light: UInt32, dark: UInt32, line: String)] = [
        "Trade by the book": ("chart.line.uptrend.xyaxis", 0x0B7A55, 0x6FF0B4, "long ETH with a stop"),
        "Short on a pullback": ("arrow.down.right", 0xB42318, 0xFF9C8F, "short oil on a bounce"),
        "Best rates": ("percent", 0x1D4ED8, 0x7FB2FF, "free USDC where it earns most"),
        "Where to keep it": ("building.columns", 0x4D7C0F, 0xC7F284, "compare where USDC earns"),
        "Every week": ("calendar", 0x6D28D9, 0xC9A4FF, "a small SOL buy every week"),
        "Back to Solana": ("arrow.uturn.left", 0xB45309, 0xF2C14E, "1 USDC home from Hyperliquid"),
        "Protect a position": ("shield.lefthalf.filled", 0xB45309, 0xF2C14E, "move your stop to the entry"),
        "Stake SOL": ("leaf", 0x0B7A55, 0x6FF0B4, "earn the staking yield on SOL"),
        "Aave on Base": ("banknote", 0x1D4ED8, 0x7FB2FF, "lend 2 USDC on Aave"),
        "Price alert": ("bell", 0xB45309, 0xF2C14E, "a ping when ETH drops below 2,500"),
        "Buy a stock token": ("building.2", 0x6D28D9, 0xC9A4FF, "SpaceX stock for 2 USDC"),
        "Profit and loss": ("chart.bar.xaxis", 0x0B7A55, 0x6FF0B4, "what you made or lost this week"),
        "What's moving": ("flame", 0xB45309, 0xF2C14E, "the biggest movers right now"),
        "Chart a coin": ("chart.xyaxis.line", 0x6D28D9, 0xC9A4FF, "SOL's last week on a chart"),
        "Buy a memecoin": ("pawprint", 0xB45309, 0xF2C14E, "BONK for 1 USDC on Solana"),
        "Swap on Base": ("arrow.left.arrow.right", 0x1D4ED8, 0x7FB2FF, "USDC for ETH on Uniswap"),
        "Move to Base": ("arrow.right.circle", 0x1D4ED8, 0x7FB2FF, "2 USDC over Relay in seconds"),
        "Sweep dust": ("sparkles", 0xB45309, 0xF2C14E, "small leftovers back to USDC"),
        "Claim rewards": ("gift", 0x0B7A55, 0x6FF0B4, "what Merkl owes your wallet"),
        "Hyperliquid spot": ("drop", 0x0B7A55, 0x6FF0B4, "HYPE itself, no leverage"),
        "Trade gold": ("medal", 0xB45309, 0xF2C14E, "gold with stop and take set"),
        "Make a multisig": ("key", 0x1D4ED8, 0x7FB2FF, "a vault that needs several keys"),
    ]

    /// What each template's scheme looks like, for the preview beside the list (S15).
    static let steps: [String: [(String, String)]] = [
        "Trade by the book": [("From", "USDC on Solana"), ("Bridge", "Relay or 1Click — the cheaper"), ("Position", "long or short, up to 5×"),
                              ("Stop and take", "stop 1.5 ATR · take in parts"), ("Rules", "risk 1% of capital"), ("Second check", "checks the plan")],
        "Short on a pullback": [("Entry", "limit 2% above the market"), ("Position", "short oil · xyz exchange"), ("Stop and take", "by the book"),
                                ("Guardian", "breakeven after ½")],
        "Best rates": [("Free", "USDC on Solana"), ("Split", "best rates · ≤ 50% per protocol"), ("Deposit", "Jupiter Lend, Kamino"), ("Result", "a year’s yield in USDC")],
        "Where to keep it": [("Rates", "Jupiter Lend, Kamino, HLP, JitoSOL"), ("Conditions", "minimums, rent, lockups"), ("Advice", "at most half in one place")],
        "Every week": [("Buy", "SOL for 2 USDC via Jupiter"), ("Every", "week, while ihatecrypto runs"), ("Checks", "address, simulation, signature"), ("Warning", "when Solana runs short")],
        "Back to Solana": [("From", "Hyperliquid"), ("Bridge", "Relay"), ("To", "your Solana address"), ("Signature", "main key — the trading key can’t withdraw")],
        "Protect a position": [("Position", "open on Hyperliquid"), ("Stop", "to the entry — breakeven"), ("Result", "worst case ≈ $0 and fees")],
        "Stake SOL": [("From", "SOL on Solana"), ("Swap", "SOL → JitoSOL"), ("Earns", "the staking yield"), ("Exit", "swap back any time")],
        "Aave on Base": [("From", "USDC on Solana"), ("Bridge", "Relay to Base + a little gas"), ("Approve", "exactly the amount"), ("Supply", "Aave v3 · ≈ 4% a year")],
        "Price alert": [("Watch", "the mark price every 10–30 s"), ("Fire", "one notification at the level"), ("Then", "the alert is done")],
        "Buy a stock token": [("From", "USDC on Solana"), ("Bridge", "Relay → USDG on Robinhood Chain"), ("Swap", "Uniswap · Robinhood's registered token"), ("Hold", "SPCX, TSLA… in your wallet")],
        "Profit and loss": [("Trading", "Hyperliquid fills and funding"), ("Holdings", "every coin, cost from its buys"), ("Result", "dollars made or lost this week")],
        "What's moving": [("Markets", "Hyperliquid's coins and stocks"), ("Sorted", "by today's change"), ("Then", "pick one to chart or trade")],
        "Chart a coin": [("Price", "a week of SOL"), ("Facts", "high, low, change"), ("Then", "trade it or set an alert")],
        "Buy a memecoin": [("From", "USDC on Solana"), ("Swap", "Jupiter · a verified token"), ("Hold", "BONK in your wallet")],
        "Swap on Base": [("From", "USDC on Base"), ("Swap", "Uniswap · the best of v2, v3, v4"), ("Get", "ETH itself, not WETH")],
        "Move to Base": [("From", "USDC on Solana"), ("Bridge", "Relay · a few seconds"), ("To", "your own address on Base")],
        "Sweep dust": [("Find", "tokens under $5 on Base, Arbitrum, Polygon"), ("Sell", "each for USDC on its own network"), ("Result", "USDC instead of crumbs")],
        "Claim rewards": [("Check", "Merkl on Base, Arbitrum, Ethereum, Polygon"), ("Claim", "Morpho's, Aave's and others' rewards"), ("Keep", "they land in your wallet")],
        "Hyperliquid spot": [("Pay", "10 USDC — the book's minimum"), ("Buy", "HYPE on Hyperliquid's spot book"), ("Hold", "the coin itself, no leverage")],
        "Trade gold": [("Market", "gold · Hyperliquid's xyz exchange"), ("Position", "long, up to 5×"), ("Stop and take", "by the book")],
        "Make a multisig": [("Keys", "this Mac + a Ledger, a phone or a partner"), ("Rule", "a majority must sign each move"), ("Network", "Base unless you name others")],
    ]
    @State private var hovered: String?

    var body: some View {
        HStack(alignment: .top, spacing: 16) {
            list
            preview(hovered ?? Self.templates[0].title)
        }
        .padding(16)
    }

    private func preview(_ title: String) -> some View {
        let t = Self.templates.first { $0.title == title } ?? Self.templates[0]
        let steps = Self.steps[title] ?? []
        return VStack(alignment: .leading, spacing: 6) {
            Text("PREVIEW · \(title.uppercased())").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            Text("this is how the scheme will look").font(.ui(10)).foregroundStyle(.tertiary)
            ForEach(steps.indices, id: \.self) { i in
                if i > 0 { Image(systemName: "arrow.down").font(.system(size: 9, weight: .bold)).foregroundStyle(.tertiary).padding(.leading, 14) }
                HStack(spacing: 8) {
                    RoundedRectangle(cornerRadius: 3).fill(Color(hex: t.colors[i % t.colors.count])).frame(width: 6, height: 30)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(steps[i].0.uppercased()).font(.ui(9, .heavy)).tracking(0.4).foregroundStyle(.secondary)
                        Text(steps[i].1).font(.ui(12, .semibold)).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 12).fill(.fill.quaternary))
            }
            Text("Say just the asset and direction — ihatecrypto fills the rest from the rules.").font(.ui(10)).foregroundStyle(.secondary).padding(.top, 4)
        }
        .frame(width: 230, alignment: .topLeading)
        .animation(.snappy, value: hovered)
    }

    private var list: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Plan templates").font(.ui(13, .bold))
            ScrollView {
                LazyVGrid(columns: [GridItem(.fixed(150)), GridItem(.fixed(150))], spacing: 8) {
                    ForEach(Self.templates, id: \.title) { t in
                        Button { pick(t.phrase) } label: {
                            VStack(alignment: .leading, spacing: 8) {
                                HStack(spacing: 3) {
                                    ForEach(t.colors.indices, id: \.self) { i in
                                        if i > 0 { Rectangle().fill(.tertiary).frame(width: 6, height: 2) }
                                        RoundedRectangle(cornerRadius: 3).fill(Color(hex: t.colors[i])).frame(width: 16, height: 11)
                                    }
                                }
                                Text(t.title).font(.ui(13, .semibold))
                                Text("«\(t.phrase)»").font(.ui(10)).foregroundStyle(.secondary).lineLimit(2)
                            }
                            .frame(maxWidth: .infinity, minHeight: 92, alignment: .topLeading)
                            .padding(10)
                            .background(RoundedRectangle(cornerRadius: 14).fill(.fill.quaternary))
                            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(hovered == t.title ? Brand.accent : .clear, lineWidth: 1.5))
                        }
                        .buttonStyle(.plain)
                        .onHover { if $0 { hovered = t.title } }
                    }
                }
            }
            .frame(maxHeight: 440)
        }
    }
}

extension Workspace {
    /// Unrealized PnL of all open positions.
    var positionsPnl: Decimal { positions.reduce(0) { $0 + $1.unrealizedPnl } }
    /// Hyperliquid main account plus HIP-3 margin.
    /// Trading margin: the main perp balance and the HIP-3 exchanges.
    var hlMargin: Decimal? { hl.map { $0.accountValue + dexBalances.values.reduce(0) { $0 + $1.accountValue } } }
    /// Everything on Hyperliquid: margin and the HLP vault.
    var hlTotal: Decimal? { hlMargin.map { $0 + hlp.equity } }
    var lendTotal: Decimal { lend.reduce(0) { $0 + $1.usdc } }
}

/// A card whose right side is an arrow: this step moves money on to the next one.
nonisolated struct ArrowCard: Shape {
    var radius: CGFloat = 18
    var tip: CGFloat = 16

    func path(in r: CGRect) -> Path {
        let body = r.maxX - tip
        let points = [CGPoint(x: r.minX, y: r.minY), CGPoint(x: body, y: r.minY), CGPoint(x: r.maxX, y: r.midY),
                      CGPoint(x: body, y: r.maxY), CGPoint(x: r.minX, y: r.maxY)]
        var p = Path()
        p.move(to: CGPoint(x: r.minX, y: r.midY))
        for i in points.indices {
            p.addArc(tangent1End: points[i], tangent2End: points[(i + 1) % points.count], radius: i == 2 ? 6 : radius)
        }
        p.closeSubpath()
        return p
    }
}

/// «Take profit like the best investors»: the three styles as cards with what each pays; a click puts it in the plan.
struct TakeChoices: View {
    @Environment(Workspace.self) private var ws
    let choices: [TakeStyle]
    let risk: Decimal?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("PICK HOW TO TAKE PROFIT").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            ForEach(choices, id: \.self) { style in
                Button { Task { await ws.chooseTake(style) } } label: {
                    HStack(spacing: 14) {
                        TakeSketch(style: style).frame(width: 96, height: 64)
                            .background(RoundedRectangle(cornerRadius: 12).fill(.fill.quaternary))
                        VStack(alignment: .leading, spacing: 3) {
                            HStack(spacing: 6) {
                                Text(style.title).font(.ui(15, .heavy))
                                if style == .partial {
                                    Text("Recommended").font(.ui(10, .bold)).foregroundStyle(Brand.onAccent)
                                        .padding(.horizontal, 7).padding(.vertical, 2)
                                        .background(Capsule().fill(Brand.accent))
                                }
                            }
                            Text(style.detail).font(.ui(10)).foregroundStyle(.secondary)
                            Text(style.note).font(.ui(10)).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 0)
                        Text(risk.map(style.payoff) ?? "").font(.ui(16, .heavy)).foregroundStyle(Brand.profit)
                    }
                    .padding(12)
                    .background(RoundedRectangle(cornerRadius: 20).fill(Brand.card))
                    .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(style == .partial ? Brand.accent : Brand.cardBorder, lineWidth: style == .partial ? 2 : 1))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .modifier(HoverLift(radius: 20))
            }
            Text("The stop is the same in all three; only how the profit is taken changes.").font(.ui(10)).foregroundStyle(.secondary)
        }
    }
}

/// A tiny price path with the take lines of a style.
struct TakeSketch: View {
    let style: TakeStyle

    var body: some View {
        Canvas { context, size in
            let w = size.width, h = size.height
            func y(_ v: CGFloat) -> CGFloat { h * v }
            var price = Path()
            let points: [CGPoint] = [CGPoint(x: 0, y: y(0.72)), CGPoint(x: w * 0.18, y: y(0.66)), CGPoint(x: w * 0.3, y: y(0.72)),
                                     CGPoint(x: w * 0.46, y: y(0.52)), CGPoint(x: w * 0.62, y: y(0.45)), CGPoint(x: w * 0.8, y: y(0.28)),
                                     CGPoint(x: w, y: style == .trend ? y(0.08) : y(0.2))]
            price.addLines(points)
            let dash = StrokeStyle(lineWidth: 1.5, dash: [3, 3])
            func line(_ v: CGFloat, _ color: Color, from x0: CGFloat = 0) {
                var p = Path(); p.move(to: CGPoint(x: x0, y: y(v))); p.addLine(to: CGPoint(x: w, y: y(v)))
                context.stroke(p, with: .color(color), style: dash)
            }
            line(0.9, Brand.loss)
            switch style {
            case .classic: line(0.2, Brand.profit)
            case .partial: line(0.52, Brand.profit.opacity(0.6)); line(0.2, Brand.profit)
            case .trend:
                line(0.52, Brand.profit.opacity(0.6))
                var trail = Path()
                trail.addLines([CGPoint(x: w * 0.46, y: y(0.8)), CGPoint(x: w * 0.62, y: y(0.8)), CGPoint(x: w * 0.62, y: y(0.62)),
                                CGPoint(x: w * 0.8, y: y(0.62)), CGPoint(x: w * 0.8, y: y(0.45)), CGPoint(x: w, y: y(0.45))])
                context.stroke(trail, with: .color(Brand.jevCursor), style: dash)
            }
            context.stroke(price, with: .color(Brand.accent), style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
        }
        .padding(6)
        .accessibilityHidden(true)
    }
}

/// An open position on the canvas: the price between its stop and takes, and what can be done with it.
struct PositionBoard: View {
    @Environment(Workspace.self) private var ws
    let position: HyperliquidAccount.Position
    @State private var confirmClose: String?

    var body: some View {
        let long = position.size > 0
        let triggers = ws.orders.filter { $0.coin == position.coin && $0.isTrigger }
        let stop = triggers.first { ($0.orderType ?? "").contains("Stop") }?.triggerPx
        let takes = triggers.filter { ($0.orderType ?? "").contains("Take") }.compactMap(\.triggerPx)
        let mark = ws.markets[position.coin]?.markPx
        VStack(alignment: .leading, spacing: 12) {
            Flow(spacing: 6) {
                ForEach([("breakeven", "Stop to breakeven", "move stop to breakeven", "shield.lefthalf.filled"), ("trail", "Trailing 2%", "trail 2%", "arrow.up.forward"),
                         ("half", "Close ½", "close half", "circle.lefthalf.filled"), ("close", "Close", "close", "xmark.circle")], id: \.0) { item in
                    Button {
                        switch item.0 {
                        case "breakeven": Task { await ws.manage(.stopToBreakeven, coin: position.coin) }
                        case "trail": Task { await ws.manage(.trail(2), coin: position.coin) }
                        default: confirmClose = item.2
                        }
                    } label: { Label(item.1, systemImage: item.3) }
                    .buttonStyle(.plain).font(.ui(12, .semibold))
                    .padding(.horizontal, 10).padding(.vertical, 7)
                    .background(RoundedRectangle(cornerRadius: 9).fill(item.0 == "breakeven" ? Brand.accent : Color.white.opacity(0.12)))
                    .foregroundStyle(item.0 == "close" ? Color(hex: 0xFF9C8F) : item.0 == "breakeven" ? Brand.onAccent : .white)
                }
            }
            .padding(5)
            .background(RoundedRectangle(cornerRadius: 13).fill(Color(hex: 0x0F1222)))
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 12) {
                    AssetBadge(coin: position.coin, size: 40, dark: true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(long ? "Long" : "Short") \(IntentParser.pretty(position.coin))").font(.ui(17, .heavy))
                        Text("\(masked(Fmt.num(abs(position.size), maxFraction: 6))) at \(Fmt.price(position.entry))\(mark.map { " · now \(Fmt.price($0))" } ?? "")")
                            .font(.ui(10)).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(masked(signed(position.unrealizedPnl))).font(.ui(22, .heavy)).monospacedDigit()
                        .foregroundStyle(position.unrealizedPnl < 0 ? Brand.loss : Brand.profit)
                }
                PriceRuler(entry: position.entry, mark: mark, stop: stop, takes: takes)
                if let rules = ws.exitRules[position.coin] {
                    Label(rules.movedToBreakeven ? "Guardian: stop already at breakeven" : rules.breakevenAfterPartial ? "Guardian: stop to breakeven after the first half" : "Guardian is watching",
                          systemImage: "shield.lefthalf.filled").font(.ui(10)).foregroundStyle(.secondary)
                }
            }
            .padding(16)
            .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
            .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Color(hex: 0x0F6B5D), lineWidth: 2))
        }
        .confirmationDialog("\(confirmClose == "close" ? "Close" : "Close half of") \(IntentParser.pretty(position.coin)) at market?",
                            isPresented: Binding(get: { confirmClose != nil }, set: { if !$0 { confirmClose = nil } })) {
            Button(confirmClose == "close" ? "Close position" : "Close half", role: .destructive) {
                // This card's own position, never «the first one».
                let share: Decimal = confirmClose == "close" ? 1 : Decimal(string: "0.5")!
                Task { await ws.manage(.close(share), coin: position.coin) }
                confirmClose = nil
            }
        }
    }
}

/// Take, entry, now and stop on one line; green where the trade makes money, red where it loses.
struct PriceRuler: View {
    let entry: Decimal
    let mark: Decimal?
    let stop: Decimal?
    let takes: [Decimal]

    var body: some View {
        let values = ([entry, mark, stop].compactMap { $0 } + takes).map { NSDecimalNumber(decimal: $0).doubleValue }
        let low = (values.min() ?? 0), high = (values.max() ?? 1)
        let span = max(high - low, 0.0001)
        func x(_ d: Decimal, _ width: CGFloat) -> CGFloat { CGFloat((NSDecimalNumber(decimal: d).doubleValue - low) / span) * (width - 24) + 12 }
        return GeometryReader { geo in
            let w = geo.size.width
            ZStack(alignment: .topLeading) {
                Capsule().fill(.fill.tertiary).frame(height: 8).offset(y: 14)
                if let stop {
                    let a = min(x(entry, w), x(stop, w)), b = max(x(entry, w), x(stop, w))
                    Capsule().fill(Brand.loss.opacity(0.35)).frame(width: b - a, height: 8).offset(x: a, y: 14)
                }
                if let far = takes.max(by: { abs($0 - entry) < abs($1 - entry) }) {
                    let a = min(x(entry, w), x(far, w)), b = max(x(entry, w), x(far, w))
                    Capsule().fill(Brand.profit.opacity(0.35)).frame(width: b - a, height: 8).offset(x: a, y: 14)
                }
                if let mark { RoundedRectangle(cornerRadius: 2).fill(.primary).frame(width: 4, height: 26).offset(x: x(mark, w) - 2, y: 5) }
                let labels = placed([("entry \(Fmt.price(entry))", x(entry, w), Color.secondary)]
                                    + (stop.map { [("stop \(Fmt.price($0))", x($0, w), Brand.loss)] } ?? [])
                                    + takes.map { ("take \(Fmt.price($0))", x($0, w), Brand.profit) }, width: w)
                ForEach(labels.indices, id: \.self) { i in
                    Text(labels[i].text).font(.ui(10, .semibold)).foregroundStyle(labels[i].color).fixedSize()
                        .offset(x: labels[i].x, y: 30 + CGFloat(labels[i].row) * 14)
                }
            }
        }
        .frame(height: 76)
    }

    /// Labels sorted along the bar, each in the first row where it doesn't overlap the previous one, and never past
    /// either end of the bar: a label near an edge is pushed inside, so nothing leaves the card.
    /// The labels' own face (`Font.ui(10, .semibold)`), so the measured width is the drawn one; looked up once, not on
    /// every layout pass.
    private static let labelFont = BrandFonts.ready
        ? NSFont(descriptor: NSFontDescriptor(fontAttributes: [.family: "Onest", .traits: [NSFontDescriptor.TraitKey.weight: NSFont.Weight.semibold]]), size: 10) ?? .systemFont(ofSize: 10, weight: .semibold)
        : .systemFont(ofSize: 10, weight: .semibold)

    private func placed(_ items: [(text: String, x: CGFloat, color: Color)], width w: CGFloat) -> [(text: String, x: CGFloat, color: Color, row: Int)] {
        var ends: [CGFloat] = []
        return items.sorted { $0.x < $1.x }.map { item in
            let width = ceil((item.text as NSString).size(withAttributes: [.font: Self.labelFont]).width)
            let start = min(max(0, item.x - width / 2), max(0, w - width))
            let row = ends.firstIndex { $0 + 6 < start } ?? ends.count
            if row == ends.count { ends.append(start + width) } else { ends[row] = start + width }
            return (item.text, start, item.color, row)
        }
    }
}

/// Where money earns most: one bubble per place, bigger for a higher rate, in the protocol's colors.
struct YieldBubbles: View {
    @Environment(Workspace.self) private var ws
    let options: [Yields.Option]

    var body: some View {
        let top = options.map(\.apy).max() ?? 1
        VStack(alignment: .leading, spacing: 10) {
            Text("RATES NOW · TAP TO PUT THE PHRASE IN THE CHAT").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 140), spacing: 10)], alignment: .leading, spacing: 10) {
                ForEach(Array(options.prefix(8).enumerated()), id: \.offset) { _, o in
                    let size = 96 + 70 * CGFloat(NSDecimalNumber(decimal: o.apy / max(top, 0.0001)).doubleValue)
                    Button { if let how = o.how { ws.suggest(how) } } label: {
                        VStack(spacing: 3) {
                            TokenLogo(name: logo(o.place), size: 26)
                            Text("\(Fmt.num(o.apy.rounded(2, .plain)))%").font(.ui(size > 140 ? 24 : 19, .heavy)).monospacedDigit()
                            Text(o.place).font(.ui(11, .semibold)).lineLimit(1)
                            Text(o.asset).font(.ui(10)).opacity(0.8)
                        }
                        .foregroundStyle(ink(o.place))
                        .frame(width: size, height: size)
                        .background(Circle().fill(fill(o.place)))
                        .overlay(Circle().strokeBorder(o.how == nil ? Brand.dots : .clear, style: StrokeStyle(lineWidth: 2, dash: [5, 4])))
                        .modifier(HoverLift(radius: size / 2, enabled: o.how != nil))
                        .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.plain)
                    .help(o.note)
                }
            }
            Text("Tip: don’t keep everything in one protocol — say “spread USDC across the best rates”.").font(.ui(10)).foregroundStyle(.secondary)
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 22).fill(Brand.card))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Brand.cardBorder))
    }

    private func logo(_ place: String) -> String { Workspace.logo(for: place) }
    private func fill(_ place: String) -> Color {
        if place.contains("Jupiter") { return Color(hex: 0x131D27) }
        if place.contains("Kamino") { return Color(hex: 0x082A56) }
        if place.contains("Jito") { return Color(hex: 0x1F9D6A) }
        if place.contains("HLP") { return Brand.hl }
        return Brand.solanaTint
    }
    private func ink(_ place: String) -> Color {
        if place.contains("Jupiter") { return Color(hex: 0xC7F284) }
        if place.contains("HLP") { return Brand.hlMint }
        return place.contains("Kamino") || place.contains("Jito") ? .white : .primary
    }
}

/// «Spread USDC across the best rates»: the free money, where each slice goes, what was skipped and why.
struct AllocationBoard: View {
    @Environment(Workspace.self) private var ws
    let preview: AllocationPreview

    private var unit: String { preview.asset }
    private func amount(_ d: Decimal) -> String { Fmt.num(d, maxFraction: unit == "SOL" ? 6 : 2) + " " + unit }

    /// The spread as route cards: what it takes from each network, the straight moves between them, then into each place
    /// with its rate; what's skipped and why under them.
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            RouteCards(steps: steps, build: true)
            ForEach(preview.skipped, id: \.place) { s in
                Label("\(s.place) · \(Fmt.num(s.apy.rounded(2, .plain)))% — skipped: \(s.reason)", systemImage: "minus.circle")
                    .font(.ui(10)).foregroundStyle(.secondary)
            }
        }
    }

    private var steps: [ActivityBubble] { ws.allocationBubbles(preview) }
}

/// Under the balance: five ways onto the canvas, each with its own icon. Top up, in the accent colour, spans the first
/// row; the others light up while their board is on the canvas. The collapsed rail has the same buttons in a column.
struct RailActions: View {
    @Environment(Workspace.self) private var ws
    @AppStorage("canvasMode") private var mode = "plan"
    var compact = false

    var body: some View {
        if compact {
            VStack(spacing: 6) { topUp; portfolio; history; favorites; schedule }
        } else {
            Grid(horizontalSpacing: 6, verticalSpacing: 6) {
                GridRow { topUp.gridCellColumns(2) }
                GridRow { portfolio; history }
                GridRow { favorites; schedule }
            }
        }
    }

    private var topUp: some View {
        tile("Top up", "arrow.down.circle.fill", on: ws.showReceive, accent: true, help: "Your addresses: where to send money into ihatecrypto") {
            ws.showReceive.toggle()
        }
    }

    private var portfolio: some View {
        tile("Portfolio", "chart.pie.fill", on: false, help: "The portfolio on the canvas") {
            NotificationCenter.default.post(name: .canvasFit, object: "portfolio")
        }
    }

    private var history: some View {
        tile("History", "clock.arrow.circlepath", on: ws.showHistory, help: "What ihatecrypto did lately") { ws.showHistory.toggle() }
    }

    private var favorites: some View {
        tile("Favorites", "star.fill", on: ws.showFavorites, help: "Favorite tokens, saved addresses, your wallets") { ws.showFavorites.toggle() }
    }

    private var schedule: some View {
        let on = ws.showSchedule
        return tile("Schedule", "calendar", on: on, help: "Beside the portfolio: what runs on its own, its budget and when it ran") { ws.showSchedule = !on }
    }

    @ViewBuilder
    private func tile(_ title: String, _ icon: String, on: Bool, accent: Bool = false, help: String, action: @escaping () -> Void) -> some View {
        let tap = {
            withAnimation(StudioView.spring) { action() }
            mode = "plan"
        }
        if compact {
            Button(title, action: tap).buttonStyle(RailRound(icon: icon, title: title, on: on, accent: accent)).help("\(title): \(help)")
        } else {
            Button(action: tap) {
                // A glass capsule; Top up is the orange one.
                HStack(spacing: 6) {
                    Image(systemName: icon).font(.system(size: 13, weight: .semibold))
                    Text(title).font(.ui(13, .semibold)).lineLimit(1).minimumScaleFactor(0.8)
                }
                .padding(.horizontal, 10)
                .frame(maxWidth: .infinity, minHeight: 38)
                .foregroundStyle(accent ? Brand.onAccent : on ? Brand.accentInk : Color.primary)
                .background(Capsule().fill(accent ? Brand.accent : on ? Brand.accent.opacity(0.16) : Brand.glassFill))
                .overlay(Capsule().strokeBorder(accent ? Color.white.opacity(on ? 0.8 : 0.35) : on ? Brand.accent.opacity(0.6) : Brand.glassRim, lineWidth: 1))
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .modifier(HoverLift(radius: 19))
            .help(help)
        }
    }
}

/// A block that can be clicked: under the pointer an accent edge and the hand cursor (HoverLift), pressed a fill.
struct LitBlock: ButtonStyle {
    var radius: CGFloat = 14

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(RoundedRectangle(cornerRadius: radius).fill(Brand.accent.opacity(configuration.isPressed ? 0.2 : 0)))
            .contentShape(RoundedRectangle(cornerRadius: radius))
            .modifier(HoverLift(radius: radius))
    }
}

/// A button of the folded sidebar: a glass circle with its name under it, orange for Top up, dashed for a new canvas.
/// Under the pointer its edge lights up in the accent and pressed it fills more; `on` marks a board that's open.
/// Nothing moves: only the edge and the fill change (the user's rule).
struct RailRound: ButtonStyle {
    let icon: String
    var title: String?
    var on = false
    var accent = false
    var dashed = false
    /// No circle until the pointer is over it (the expand button).
    var plain = false

    func makeBody(configuration: Configuration) -> some View { Face(style: self, pressed: configuration.isPressed) }

    private struct Face: View {
        let style: RailRound
        let pressed: Bool
        @State private var hover = false
        @Environment(\.isEnabled) private var enabled

        var body: some View {
            let s = style, lit = hover && enabled
            VStack(spacing: 3) {
                Image(systemName: s.icon).font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(s.accent ? Brand.onAccent : s.on || lit ? Brand.accentInk : Color.primary.opacity(0.8))
                    .frame(width: 42, height: 42)
                    .background {
                        if s.accent { Circle().fill(Brand.accent.opacity(pressed ? 0.75 : 1)) }
                        else if pressed || s.on { Circle().fill(Brand.accent.opacity(pressed ? 0.26 : 0.16)) }
                        else if !s.plain || lit { Circle().fill(lit ? Brand.glassSelected : Brand.glassFill) }
                    }
                    .overlay {
                        if s.accent {
                            Circle().strokeBorder(Color.white.opacity(lit || s.on ? 0.9 : 0.35), lineWidth: lit || s.on ? 2 : 1)
                        } else if s.dashed {
                            Circle().strokeBorder(lit ? Brand.accent : Color.secondary.opacity(0.55), style: StrokeStyle(lineWidth: 1.2, dash: [3, 3]))
                        } else if lit || s.on {
                            Circle().strokeBorder(Brand.accent.opacity(lit ? 0.9 : 0.6), lineWidth: 1.5)
                        } else if !s.plain {
                            Circle().strokeBorder(Brand.glassRim, lineWidth: 1)
                        }
                    }
                if let title = s.title {
                    Text(title).font(.ui(10, .medium)).lineLimit(1)
                        .foregroundStyle(s.on || lit ? AnyShapeStyle(Brand.accentInk) : AnyShapeStyle(.secondary))
                }
            }
            .frame(minWidth: 60)
            .contentShape(Rectangle())
            .opacity(enabled ? 1 : 0.5)
            // Hover changes at once (a fade redraws the whole window every frame); a press fades for a tenth of a second.
            .animation(.snappy(duration: 0.1), value: pressed)
            .onHover { hover = $0 }
            .pointerStyle(enabled ? .link : nil)
        }
    }
}


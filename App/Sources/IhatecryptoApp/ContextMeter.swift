import SwiftUI
import IhatecryptoCore
import LLMCatalog
import LLMCore

/// What the model was given with this canvas's last message and what its answers cost: kept with the canvas.
/// A field added later must be optional, or canvases saved before it stop loading.
nonisolated struct ContextMeter: Codable, Equatable {
    /// Tokens the model read with the last message, as its service counted them; nil before the first answer.
    var used: Int?
    /// The model's context window; nil when its service doesn't say.
    var window: Int?
    /// Of `used`: the instructions and the chat's lines. The rest is the canvas, the balances and the message.
    var instructions = 0, chat = 0
    /// Lines of the chat sent with the last message, of how many there were.
    var sent = 0, lines = 0
    /// Dollars the answers on this canvas cost, counted only when a key pays for them; and the last answer's.
    var spent = 0.0, last: Double?
    /// The dollars are the service's own bill (OpenRouter), not tokens at the list price.
    var billed = false
    /// The dollars are Fable's on a Claude subscription: usage credits, at Claude Code's price (`credits`).
    var credits: Bool?

    /// The part of the window in use; nil when the window isn't known.
    var share: Double? {
        guard let used, let window, window > 0 else { return nil }
        return min(1, Double(used) / Double(window))
    }

    /// «950», «9.6k», «18k», «272k», «1M», «1.05M».
    static func short(_ tokens: Int) -> String {
        func trimmed(_ value: Double, _ digits: Int) -> String {
            var text = String(format: "%.\(digits)f", value)
            if text.contains(".") { while text.hasSuffix("0") { text.removeLast() }; if text.hasSuffix(".") { text.removeLast() } }
            return text
        }
        if tokens >= 995_000 { return trimmed(Double(tokens) / 1_000_000, 2) + "M" }
        if tokens >= 9_950 { return "\(Int((Double(tokens) / 1000).rounded()))k" }
        if tokens >= 1_000 { return trimmed(Double(tokens) / 1000, 1) + "k" }
        return "\(tokens)"
    }

    /// Fable on a Claude subscription: a Pro plan's limits don't cover it, Claude bills it as usage credits (a Max plan
    /// spends up to half its weekly limit on it first). Its answers are counted in dollars, Claude Code's price for them.
    /// support.claude.com/en/articles/15424964, 2026-11-05.
    static func credits(provider: String, model: String) -> Bool { provider == "claude-code" && model.contains("fable") }

    /// «$1.24», «$0.04», «$0.004»; under a tenth of a cent «<$0.001».
    static func dollars(_ value: Double) -> String {
        if value >= 0.0095 { return String(format: "$%.2f", value) }
        if value >= 0.00095 { return String(format: "$%.3f", value) }
        return value > 0 ? "<$0.001" : "$0"
    }
}

extension Workspace {
    /// An answer's count, told by the model's client (llmClient). The chat's own reading of a message is what fills the
    /// window; every answer a key pays for adds to the canvas's dollars.
    // ponytail: counted on the canvas open when the answer ends. A scheduled check answering in the background adds
    // its cents there too; give the client its canvas if that starts to matter.
    func count(_ usage: LLMUsage, for request: LLMRequest, provider id: String) async {
        let paid = LLMCatalog.provider(id: id)?.info.kind == .apiKey
        if paid, modelFacts.isEmpty { await loadModelFacts() }
        let facts = paid ? modelFacts[ModelFacts.key(request.model)] : nil
        if ChatRouter.reads(request) {
            let bytes = max(1, request.messages.reduce(0) { $0 + $1.text.utf8.count }), brief = request.system.utf8.count
            let all = chatLines(), sent = sentChat(all)
            meter.used = usage.input
            meter.window = usage.window ?? facts?.window
            meter.instructions = usage.input * min(brief, bytes) / bytes
            meter.chat = usage.input * min(max(0, bytes - brief), sent.reduce(0) { $0 + $1.utf8.count }) / bytes
            meter.sent = sent.count
            meter.lines = all.count
        }
        let credits = ContextMeter.credits(provider: id, model: request.model)
        if paid || credits, let cost = usage.cost ?? facts?.cost(usage) {
            meter.spent += cost
            meter.last = cost
            meter.billed = usage.cost != nil
            meter.credits = credits
        }
    }

    /// Windows and prices of the models a key pays for, read once a launch (asked again after a minute when it failed).
    func loadModelFacts() async {
        guard Date().timeIntervalSince(modelFactsAsked) > 60 else { return }
        modelFactsAsked = Date()
        if let data = try? await HTTP.get(ModelFacts.source) { modelFacts = ModelFacts.table(data) }
    }

    /// The lines of a chat that go to the open canvas's model with a message (ChatRecall): all of them to a wide
    /// window, the last 12 at most to a model on this Mac.
    func sentChat(_ lines: [String]) -> [String] {
        ChatRecall.sent(lines, window: LLMCatalog.window(provider: providerID) ?? meter.window, small: provider.info.kind == .local,
                        instructions: ChatRouter.instructionTokens)
    }
}

/// The chip at the right of the model and the networks, over the chat's field: how much of the model's window the chat
/// fills, or what its answers cost when a key pays for them. A click opens what it's made of.
struct ContextChip: View {
    @Environment(Workspace.self) private var ws
    @State private var open = false
    /// When the popover last closed: the click on the chip that closes it must not reopen it.
    @State private var closedAt = Date.distantPast

    var body: some View {
        let m = ws.meter
        if let used = m.used {
            // Under a fifth of the window left: the rim and the number light up; the chip itself stays still.
            // Dollars while the chat's model is paid for (a key, or Fable on a subscription), said as what they were spent on.
            let tight = (m.share ?? 0) >= 0.8, credits = m.credits == true
            let paid = (ws.provider.info.kind == .apiKey || ContextMeter.credits(provider: ws.providerID, model: ws.model)) && m.spent > 0
            Button {
                if open { open = false; return }
                guard Date.now.timeIntervalSince(closedAt) > 0.3 else { return }
                open = true
            } label: {
                HStack(spacing: 6) {
                    if let share = m.share { ContextRing(share: share, ink: tight ? Brand.accent : Color.primary.opacity(0.82)) }
                    if paid {
                        Text(ContextMeter.dollars(m.spent)).foregroundStyle(tight ? Brand.accentInk : .primary)
                        if credits { Text(ws.say("credits", "за кредиты", "con créditos")).font(.ui(12, .medium)).foregroundStyle(.secondary) }
                    } else {
                        Text(ContextMeter.short(used)).foregroundStyle(tight ? Brand.accentInk : .primary)
                        if let window = m.window { Text("/ " + ContextMeter.short(window)).font(.ui(12, .medium)).foregroundStyle(.secondary) }
                    }
                }
                .font(.ui(12, .semibold)).lineLimit(1)
                .padding(.leading, 8).padding(.trailing, 10).frame(height: 30)
                .background(Capsule().fill(open ? Brand.accent.opacity(0.14) : Brand.glassFill))
                .overlay(Capsule().strokeBorder(tight ? Brand.accent : Brand.glassRim))
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .fixedSize()
            .accessibilityLabel(m.window.map { "Context: \(ContextMeter.short(used)) of \(ContextMeter.short($0)) tokens" } ?? "Context: \(ContextMeter.short(used)) tokens")
            .help(paid ? (credits ? "Fable is paid for with usage credits. " : "") + "This chat's answers cost \(ContextMeter.dollars(m.spent)) so far. Click for the context they read."
                       : "What the model read with your last message" + (m.window == nil ? "." : ", of what its window holds."))
            .popover(isPresented: $open, arrowEdge: .top) { details(m, used: used, credits: credits) }
            .onChange(of: open) { if !open { closedAt = .now } }
            .transition(.opacity)
        }
    }

    private func details(_ m: ContextMeter, used: Int, credits: Bool) -> some View {
        let rest = max(0, used - m.instructions - m.chat)
        return VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("CONTEXT").font(.ui(10, .semibold)).tracking(1.2)
                Spacer()
                if let share = m.share { Text(share < 0.01 ? "under 1%" : "\(Int((share * 100).rounded()))%").font(.ui(11, .semibold)) }
            }
            .foregroundStyle(.secondary)
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(ContextMeter.short(used)).font(.ui(24, .bold))
                Text(m.window.map { "of \(ContextMeter.short($0)) tokens" } ?? "tokens").font(.ui(12)).foregroundStyle(.secondary)
            }
            if let window = m.window {
                // What the window holds, each part as wide as its share; a part too thin to see keeps a sliver.
                GeometryReader { box in
                    HStack(spacing: 1) {
                        ForEach(Array(zip([m.instructions, m.chat, rest], [Color.secondary, Brand.accent, Brand.profit]).enumerated()), id: \.offset) { _, part in
                            if part.0 > 0 { part.1.frame(width: max(2, box.size.width * CGFloat(part.0) / CGFloat(window))) }
                        }
                        Spacer(minLength: 0)
                    }
                }
                .frame(height: 6)
                .background(Color.primary.opacity(0.1))
                .clipShape(Capsule())
            }
            VStack(alignment: .leading, spacing: 6) {
                row(.secondary, "Instructions", ContextMeter.short(m.instructions))
                row(Brand.accent, m.sent < m.lines ? "This chat · the last \(m.sent) of \(m.lines) messages" : "This chat · \(m.lines) messages", ContextMeter.short(m.chat))
                row(Brand.profit, "Canvas, balances, your message", ContextMeter.short(rest))
            }
            if m.spent > 0 {
                Divider()
                VStack(alignment: .leading, spacing: 6) {
                    row(nil, "This chat’s answers", ContextMeter.dollars(m.spent))
                    if let last = m.last { row(nil, "The last one", ContextMeter.dollars(last)) }
                }
                Text(credits ? "Claude Code’s price for these tokens. A Pro plan doesn’t cover Fable: Claude takes it from usage credits. A Max plan spends up to half its weekly limit on Fable first."
                     : m.billed ? "As OpenRouter charged it." : "Tokens at the service’s list price. Its own invoice is the final word.")
                    .font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            Divider()
            Text(m.sent < m.lines
                 ? "This chat is long: only its newest messages go with each request, so it never runs out of room."
                 : "Each message goes with the chat so far. A long chat sends only its newest messages, so it never runs out of room.")
                .font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .frame(width: 300)
    }

    private func row(_ dot: Color?, _ name: String, _ value: String) -> some View {
        HStack(spacing: 8) {
            if let dot { Circle().fill(dot).frame(width: 8, height: 8) }
            Text(name).font(.ui(12)).lineLimit(1)
            Spacer(minLength: 8)
            Text(value).font(.ui(12, .semibold))
        }
    }
}

/// How full the window is, as a ring that fills clockwise from the top.
private struct ContextRing: View {
    let share: Double
    let ink: Color

    var body: some View {
        Circle().stroke(Color.primary.opacity(0.2), lineWidth: 3)
            .overlay(Circle().trim(from: 0, to: max(0.03, share)).stroke(ink, lineWidth: 3).rotationEffect(.degrees(-90)))
            .frame(width: 11, height: 11)
            .padding(1.5)
    }
}

import SignerAPI
import SwiftUI
import WalletCore

/// Settings › Wallet › Limits: what signs without the person, in their own numbers. The signer keeps them: Save sends
/// them, and any number raised asks for Touch ID there (lowering one is immediate).
struct LimitsSettings: View {
    @Environment(Workspace.self) private var ws
    @State private var draft = SignerLimits.standard
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        SettingsSection(icon: "gauge.with.dots.needle.50percent", title: "Limits",
                        line: "What signs without \(Presence.short). A higher number asks for \(Presence.name) when you save it.") {
            SettingsGroup {
                SettingsRow("Trading key: one order up to", icon: "chart.line.uptrend.xyaxis",
                            info: "The Hyperliquid trading key opens a position on its own only as a buy, priced by the order itself. Bigger orders, shorts, selling and TWAPs ask for \(Presence.name). Stops, takes and closes always go.") {
                    DollarField(label: "Trading key: one order up to", value: $draft.agentOrder)
                }
                SettingsRow("Trading key: a day up to", icon: "calendar", info: "What the trading key opens in any 24 hours, all orders together.") {
                    DollarField(label: "Trading key: a day up to", value: $draft.agentDay)
                }
                SettingsRow("Trading key works after \(Presence.short)", icon: "clock",
                            info: "Days the trading key opens positions after you last turned it on or signed an order in person. Then only stops, takes and closes go until you sign again.") {
                    Picker("Trading key works after \(Presence.short)", selection: $draft.agentDays) {
                        ForEach(1...SignerLimits.maxAgentDays, id: \.self) { Text($0 == 1 ? "1 day" : "\($0) days").tag($0) }
                    }
                    .labelsHidden().pickerStyle(.menu).fixedSize()
                }
                SettingsRow("Automation wallet: a day up to", icon: "clock.arrow.circlepath",
                            info: "Dollars scheduled tasks spend without you in any 24 hours. Money moved to your own wallets doesn't count. Past it, or for anything that isn't dollars, a task waits for \(Presence.name).") {
                    DollarField(label: "Automation wallet: a day up to", value: $draft.automationDay)
                }
                SettingsRow("Automation wallet: a week up to", icon: "calendar.badge.clock", info: "The same, in any 7 days.") {
                    DollarField(label: "Automation wallet: a week up to", value: $draft.automationWeek)
                }
                SettingsRow("Swap slippage without a new \(Presence.short)", icon: "arrow.left.arrow.right",
                            info: "A Jupiter swap that allows more slippage than this asks for \(Presence.name) again, whatever session is open. Its window says what goes in and the least that comes out.") {
                    Picker("Swap slippage without a new \(Presence.short)", selection: $draft.slippageBps) {
                        ForEach(Array(Set([50, 100, 200, 300, 500, 1000, draft.slippageBps])).sorted(), id: \.self) { Text(SignerLimits.percent($0)).tag($0) }
                    }
                    .labelsHidden().pickerStyle(.menu).fixedSize()
                }
                if draft != ws.limits || problem != nil {
                    HStack(spacing: 10) {
                        if let problem = problem ?? draft.problem { Text(problem).font(.ui(12)).foregroundStyle(.secondary) }
                        Spacer()
                        Button("Undo") { draft = ws.limits; problem = nil }.disabled(saving)
                        Button(draft.looser(than: ws.limits).isEmpty ? "Save" : "Save with \(Presence.short)") { save() }
                            .buttonStyle(.borderedProminent).tint(Brand.accent).disabled(saving || draft.problem != nil)
                    }
                    .padding(.horizontal, 14).padding(.vertical, 10)
                }
            }
            .disabled(ws.signerWallet == nil)
        }
        .onAppear { draft = ws.limits }
        .onChange(of: ws.limits) { _, now in if !saving { draft = now } }
    }

    private func save() {
        saving = true
        let wanted = draft
        Task {
            do {
                let reply = try await SignerClient().send(.setLimits(limits: wanted))
                problem = reply.ok ? nil : reply.error
            } catch { problem = error.localizedDescription }
            await ws.loadSigner()
            if problem != nil { draft = ws.limits }
            saving = false
        }
    }
}

/// A whole-dollar amount typed in place.
private struct DollarField: View {
    let label: String
    @Binding var value: Int

    var body: some View {
        HStack(spacing: 2) {
            Text("$").foregroundStyle(.secondary)
            TextField(label, value: $value, format: .number.grouping(.automatic))
                .textFieldStyle(.roundedBorder).labelsHidden().multilineTextAlignment(.trailing).frame(width: 96)
        }
    }
}

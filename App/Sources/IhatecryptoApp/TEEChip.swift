import LLMNearAI
import SwiftUI

/// Beside the model menu while the chat runs on NEAR AI Cloud: the enclave check that let the last message go
/// (NearAI.ensureVerified, before every message, at most ten minutes old). A click shows what was checked.
struct TEEChip: View {
    @Environment(Workspace.self) private var ws
    @State private var open = false

    var body: some View {
        // NearAI's check isn't observed: the meter, which every answer updates, brings the redraw that reads it.
        let _ = ws.meter
        if ws.providerID == NearAI().info.id, let v = NearAI.verified(ws.model) {
            // A check mark only when the last reply's own bytes were signed by the attested model key.
            let signed = NearAI.lastReply(ws.model) == .model
            Button { open.toggle() } label: {
                HStack(spacing: 5) {
                    Image(systemName: signed ? "checkmark.shield.fill" : "shield").font(.system(size: 11, weight: .semibold))
                    Text("TEE")
                }
                .font(.ui(12, .semibold)).lineLimit(1)
                .padding(.leading, 8).padding(.trailing, 10).frame(height: 30)
                .background(Capsule().fill(open ? Brand.accent.opacity(0.14) : Brand.glassFill))
                .overlay(Capsule().strokeBorder(Brand.glassRim))
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .fixedSize()
            .accessibilityLabel("Enclave attested")
            .help("NEAR AI's hardware enclaves for this model were attested before your message went. Click for what was checked.")
            .popover(isPresented: $open, arrowEdge: .top) { details(v) }
        }
    }

    private func details(_ v: NearAttestation.Verified) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("PRIVATE MODEL").font(.ui(10, .semibold)).tracking(1.2).foregroundStyle(.secondary)
            Text("Enclave attested").font(.ui(20, .bold))
            Text(v.model).font(.ui(11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            VStack(alignment: .leading, spacing: 8) {
                row("Intel TDX", "The enclave's quote, signed down to Intel's root certificate")
                row("NVIDIA GPU", "NVIDIA's attestation service passed the GPUs")
                row("Fresh", "Answers a random number from this Mac, checked \(v.at.formatted(date: .omitted, time: .shortened))")
                row("Enclave key", String(v.gatewayKey.prefix(8)) + "…" + String(v.gatewayKey.suffix(8)))
                switch NearAI.lastReply(v.model) {
                case .model?: row("Reply", "Its exact bytes signed by this model's enclave key")
                case .gateway?: row("Reply", "Signed by NEAR AI's attested gateway, not by the model's own key", ok: false)
                case .unverified(let why)?: row("Reply", "Not verified: \(why)", ok: false)
                case nil: EmptyView()
                }
            }
            Divider()
            Text("NEAR AI runs this model inside hardware enclaves. ihatecrypto checks their evidence itself before every message, at most ten minutes old; when a check fails, nothing is sent and this chip goes away. Each reply's signature is checked against the enclave's key.")
                .font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .frame(width: 300)
    }

    private func row(_ name: String, _ detail: String, ok: Bool = true) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: ok ? "checkmark" : "exclamationmark").font(.system(size: 10, weight: .bold))
            VStack(alignment: .leading, spacing: 2) {
                Text(name).font(.ui(12, .semibold))
                Text(detail).font(.ui(11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

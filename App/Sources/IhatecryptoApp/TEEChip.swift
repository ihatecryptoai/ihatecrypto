import LLMNearAI
import SwiftUI

extension Brand {
    /// The private model's green (T4): the sealed hardware the chat runs in.
    static let tee = Color(hex: 0x7FD6A0)
    static let teeInk = Color(light: 0x1C7A48, dark: 0x7FD6A0)
}

extension Workspace {
    /// The last enclave check that passed while the chat runs on NEAR AI Cloud (NearAI.ensureVerified, before every
    /// message, at most ten minutes old when it goes); nil on any other model, before a check passed or after one failed.
    var teeVerified: NearAttestation.Verified? {
        // NearAI's check isn't observed: the meter, which every answer updates, brings the redraw that reads it.
        _ = meter
        return providerID == NearAI().info.id ? NearAI.lastVerified(model) : nil
    }
}

/// T4 «Private assistant» (owner's pick, 2026-11-06): the coin's lock in the chat's header while the model runs in an
/// attested enclave. Green, the color of the sealed hardware.
struct TEELockBadge: View {
    var size: CGFloat = 17

    var body: some View {
        Image(systemName: "lock.fill").font(.system(size: size * 0.5, weight: .bold)).foregroundStyle(Color(hex: 0x1D1B19))
            .frame(width: size, height: size)
            .background(Circle().fill(Brand.tee))
            .overlay(Circle().strokeBorder(Brand.card, lineWidth: 1.5))
            .accessibilityHidden(true)
    }
}

/// «Proof» in the private assistant's header: what was checked before the last message went, and whether the reply's own
/// bytes were signed by the attested model key.
struct TEEProofButton: View {
    let verified: NearAttestation.Verified
    /// The chat's model as the app names it: the reply's signature is kept under this name, not the attestation's.
    let model: String
    @State private var open = false

    var body: some View {
        // A warning only for a reply that came back unproven: none yet (the first answer is on its way) is no warning.
        let proof = NearAI.lastReply(model), signed = proof == nil || proof == .model
        Button { open.toggle() } label: {
            HStack(spacing: 4) {
                if !signed { Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 9, weight: .bold)) }
                Text("Proof")
            }
            .font(.ui(12, .semibold)).foregroundStyle(signed ? Brand.teeInk : Color.orange)
            .padding(.horizontal, 11).frame(height: 28)
            .background(Capsule().fill(open ? Brand.tee.opacity(0.18) : Color.clear))
            .overlay(Capsule().strokeBorder(Brand.tee.opacity(0.55)))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .pointerStyle(.link)
        .fixedSize()
        .accessibilityLabel("Proof that the model runs in a sealed enclave")
        .help(signed ? "NEAR AI's hardware enclaves for this model were attested before your message went, and the reply came signed by the model's own key. Click for what was checked."
                     : "The enclaves were attested, but the last reply wasn't signed by the model's own key. Click for what was checked.")
        .popover(isPresented: $open, arrowEdge: .bottom) { details(verified) }
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
                switch NearAI.lastReply(model) {
                case .model?: row("Reply", "Its exact bytes signed by this model's enclave key")
                case .gateway?: row("Reply", "Signed by NEAR AI's attested gateway, not by the model's own key", ok: false)
                case .unverified(let why)?: row("Reply", "Not verified: \(why)", ok: false)
                case nil: EmptyView()
                }
            }
            Divider()
            Text("NEAR AI runs this model inside hardware enclaves. ihatecrypto checks their evidence itself before every message, at most ten minutes old. When a check fails, nothing is sent and the header turns back to the plain assistant. Each reply's signature is checked against the enclave's key.")
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

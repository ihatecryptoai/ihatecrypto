import Foundation
@testable import LLMNearAI
import Testing

/// A real report NEAR AI Cloud answered on 2026-11-01 (Qwen/Qwen3.8-27B, ed25519) with the nonce it was asked with.
/// The GPU verdict is NVIDIA's network service, so it's stubbed here; everything else is checked offline.
private func fixture() throws -> (report: Data, nonce: String) {
    let url = try #require(Bundle.module.url(forResource: "near-attestation-qwen", withExtension: "json", subdirectory: "Fixtures"))
    let nonce = try String(contentsOf: url.deletingPathExtension().appendingPathExtension("nonce"), encoding: .utf8)
    return (try Data(contentsOf: url), nonce.trimmingCharacters(in: .whitespacesAndNewlines))
}

@Test func nearAttestationChecksTheEnclavesDownToIntel() async throws {
    let (report, nonce) = try fixture()
    let v = try await NearAttestation.verify(report, nonce: nonce, model: "Qwen/Qwen3.8-27B") { _ in true }
    #expect(v.gatewayKey.count == 64 && v.modelKeys.count >= 1 && v.modelKeys.allSatisfy { $0.count == 64 })
}

@Test func nearAttestationRefusesWhatDoesntBind() async throws {
    let (report, nonce) = try fixture()
    // Another nonce: evidence made for someone else's check, or replayed.
    let other = String(nonce.reversed())
    await #expect(throws: NearAttestation.Failure.self) { try await NearAttestation.verify(report, nonce: other, model: "Qwen/Qwen3.8-27B") { _ in true } }
    // A model it gave no evidence for, and GPUs NVIDIA doesn't pass.
    await #expect(throws: NearAttestation.Failure.self) { try await NearAttestation.verify(report, nonce: nonce, model: "z-ai/glm-5.3") { _ in true } }
    await #expect(throws: NearAttestation.Failure.self) { try await NearAttestation.verify(report, nonce: nonce, model: "Qwen/Qwen3.8-27B") { _ in false } }
    // One byte of the model's quote changed: in its measurements, then in Intel's QE report.
    var root = try #require(try JSONSerialization.jsonObject(with: report) as? [String: Any])
    var models = try #require(root["model_attestations"] as? [[String: Any]])
    let quote = try #require(models[0]["intel_quote"] as? String)
    for at in [48 + 136, 48 + 584 + 4 + 128 + 6 + 10] {
        var bytes = try NearAttestation.bytes(quote)
        bytes[at] ^= 1
        models[0]["intel_quote"] = NearAttestation.hex(bytes)
        root["model_attestations"] = models
        let tampered = try JSONSerialization.data(withJSONObject: root)
        await #expect(throws: NearAttestation.Failure.self) { try await NearAttestation.verify(tampered, nonce: nonce, model: "Qwen/Qwen3.8-27B") { _ in true } }
    }
}

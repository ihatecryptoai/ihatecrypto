import CryptoKit
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

@Test func aReplyCountsOnlyWhenItsOwnBytesAreSignedByTheAttestedKey() throws {
    func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }
    let model = Curve25519.Signing.PrivateKey(), gateway = Curve25519.Signing.PrivateKey(), stranger = Curve25519.Signing.PrivateKey()
    let v = NearAttestation.Verified(model: "m/x", gatewayKey: hex(gateway.publicKey.rawRepresentation),
                                     modelKeys: [hex(model.publicKey.rawRepresentation)], at: Date())
    let request = Data(#"{"model":"m/x"}"#.utf8), response = Data(#"{"id":"chatcmpl-1"}"#.utf8)
    let hashes = hex(Data(SHA256.hash(data: request))) + ":" + hex(Data(SHA256.hash(data: response)))
    func signed(_ key: Curve25519.Signing.PrivateKey, _ text: String, _ kind: String) throws -> [String: Any] {
        ["text": text, "signature": hex(try key.signature(for: Data(text.utf8))), "signing_address": hex(key.publicKey.rawRepresentation),
         "signing_algo": "ed25519", "signature_kind": kind]
    }
    #expect(NearAI.check(try signed(model, "m/x:" + hashes, "provider_tee"), model: "m/x", request: request, response: response, verified: v) == .model)
    #expect(NearAI.check(try signed(gateway, hashes, "gateway"), model: "m/x", request: request, response: response, verified: v) == .gateway)
    // Other bytes, a key the attestation never saw, a forged signature: not verified.
    if case .model = NearAI.check(try signed(model, "m/x:" + hashes, "provider_tee"), model: "m/x", request: request, response: Data("{}".utf8), verified: v) { Issue.record("other bytes") }
    if case .model = NearAI.check(try signed(stranger, "m/x:" + hashes, "provider_tee"), model: "m/x", request: request, response: response, verified: v) { Issue.record("stranger") }
    var forged = try signed(model, "m/x:" + hashes, "provider_tee")
    forged["text"] = "m/x:" + hashes.replacingOccurrences(of: "a", with: "b")
    if case .model = NearAI.check(forged, model: "m/x", request: request, response: response, verified: v) { Issue.record("forged") }
}

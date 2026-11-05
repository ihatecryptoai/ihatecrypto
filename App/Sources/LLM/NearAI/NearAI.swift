import CryptoKit
import Foundation
import LLMAPIKey
import LLMCore

/// NEAR AI Cloud: open models that run inside Intel TDX enclaves on attested NVIDIA GPUs, behind an OpenAI-compatible API
/// (cloud-api.near.ai/v1). Before a chat goes out the enclaves' evidence is checked with a fresh nonce
/// (NearAttestation), and again every ten minutes; nothing is sent while it fails. Only the models NEAR itself runs
/// attest (provider=near, 2026-11-01): Claude, Gemini, GLM and the others there are proxied, not private.
public struct NearAI: LLMProvider {
    public let info = ProviderInfo(
        id: "nearai", name: "NEAR AI Cloud (private)", kind: .apiKey, defaultModel: "deepseek-ai/DeepSeek-V4.1-Flash-1017",
        note: "Key from cloud.near.ai. The model runs in a hardware enclave; ihatecrypto checks the enclave before every chat, so the provider can't read your balances.",
        takesKey: true, models: ["deepseek-ai/DeepSeek-V4.1-Flash-1017", "Qwen/Qwen3.8-27B", "Qwen/Qwen3.6-35B-A3B-FP8"])
    static let base = "https://cloud-api.near.ai/v1"

    public init() {}

    public func client(_ credential: Credential?, save: @escaping SaveCredential) throws -> any LLMClient {
        guard case .apiKey(let key) = credential else { throw LLMError("\(info.name): add a key in Settings.") }
        return Client(key: key)
    }

    /// The check that passed for `model` within the last ten minutes: what the chat shows as «TEE». Older, or after a
    /// check that failed, nil.
    public static func verified(_ model: String) -> NearAttestation.Verified? {
        lock.withLock { checks[model] }.flatMap { Date().timeIntervalSince($0.at) < 600 ? $0 : nil }
    }

    /// The enclaves behind `model` checked now, or within the last ten minutes.
    public static func ensureVerified(_ model: String) async throws -> NearAttestation.Verified {
        if let v = verified(model) { return v }
        let nonce = (0..<32).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
        var url = URLComponents(string: base + "/attestation/report")!
        url.queryItems = [.init(name: "model", value: model), .init(name: "provider", value: "near"),
                          .init(name: "signing_algo", value: "ed25519"), .init(name: "nonce", value: nonce)]
        var r = URLRequest(url: url.url!, timeoutInterval: 60)
        r.setValue("true", forHTTPHeaderField: "x-no-aliasing")
        // Whatever goes wrong from here, the old check no longer stands for this model.
        lock.withLock { checks[model] = nil }
        let (data, response) = try await URLSession.shared.data(for: r)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw LLMError("NEAR AI gave no enclave evidence for \(model), so nothing was sent. Pick another of its private models.")
        }
        do {
            let v = try await NearAttestation.verify(data, nonce: nonce, model: model, gpu: NearAttestation.nvidiaVerdict)
            lock.withLock { checks[model] = v }
            return v
        } catch {
            throw LLMError("NEAR AI's enclave didn't check out, so nothing was sent: \(error.localizedDescription)")
        }
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var checks: [String: NearAttestation.Verified] = [:]

    /// What the last reply of a model proved, by model: what the chat's TEE chip shows beside the enclave check.
    public enum ReplyProof: Sendable, Equatable {
        /// Its exact bytes signed by the model's own enclave key, the one the attestation verified.
        case model
        /// Signed by the attested gateway only: it passed through NEAR AI's enclave, not proven from the model's.
        case gateway
        case unverified(String)
    }

    public static func lastReply(_ model: String) -> ReplyProof? { lock.withLock { replies[model] } }
    nonisolated(unsafe) private static var replies: [String: ReplyProof] = [:]

    /// The chat, each request only after its model's enclaves check out, and each reply checked against them after:
    /// the exact request and response bytes, signed by the model's enclave key (GET /v1/signature). Answers come whole,
    /// not streamed: a streamed one is re-signed by the gateway, which proves less.
    struct Client: LLMClient {
        let key: String

        func complete(_ r: LLMRequest) async throws -> String {
            let verified = try await NearAI.ensureVerified(r.model)
            var body: [String: Any] = ["model": r.model, "messages": r.messages.map { ["role": $0.role.rawValue, "content": $0.text] }]
            if r.wantsJSON { body["response_format"] = ["type": "json_object"] }
            let request = try JSONSerialization.data(withJSONObject: body)
            var u = URLRequest(url: URL(string: NearAI.base + "/chat/completions")!, timeoutInterval: r.thinking ? 180 : 90)
            u.httpMethod = "POST"
            u.httpBody = request
            for (k, v) in ["Authorization": "Bearer \(key)", "Content-Type": "application/json", "x-no-aliasing": "true", "Accept-Encoding": "identity"] {
                u.setValue(v, forHTTPHeaderField: k)
            }
            let (data, response) = try await URLSession.shared.data(for: u)
            guard (response as? HTTPURLResponse)?.statusCode == 200,
                  let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any], let id = o["id"] as? String else {
                throw LLMError("NEAR AI: " + String(decoding: data.prefix(300), as: UTF8.self))
            }
            LLMUsage.report(LLMUsage(reported: o["usage"] as? [String: Any]))
            let text = ((o["choices"] as? [[String: Any]])?.first?["message"] as? [String: Any])?["content"] as? String ?? ""
            let proof = await NearAI.proof(id: id, model: r.model, request: request, response: data, verified: verified, key: key)
            NearAI.lock.withLock { NearAI.replies[r.model] = proof }
            return text
        }
    }

    /// The reply's signature, fetched (it can lag a moment) and checked: the text must be the hashes of these exact
    /// bytes, the signer the attested model key (or the gateway's), the ed25519 signature valid.
    static func proof(id: String, model: String, request: Data, response: Data, verified v: NearAttestation.Verified, key: String) async -> ReplyProof {
        var c = URLComponents(string: base + "/signature/" + id)!
        c.queryItems = [.init(name: "model", value: model), .init(name: "signing_algo", value: "ed25519")]
        var u = URLRequest(url: c.url!, timeoutInterval: 20)
        u.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        for attempt in 0..<4 {
            if attempt > 0 { try? await Task.sleep(for: .seconds(1)) }
            guard let (data, http) = try? await URLSession.shared.data(for: u), (http as? HTTPURLResponse)?.statusCode == 200,
                  let o = try? JSONSerialization.jsonObject(with: data) as? [String: Any], o["error_code"] == nil else { continue }
            return check(o, model: model, request: request, response: response, verified: v)
        }
        return .unverified("no signature for this reply")
    }

    static func check(_ o: [String: Any], model: String, request: Data, response: Data, verified v: NearAttestation.Verified) -> ReplyProof {
        func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }
        let hashes = hex(Data(SHA256.hash(data: request))) + ":" + hex(Data(SHA256.hash(data: response)))
        guard o["signing_algo"] as? String == "ed25519", let text = o["text"] as? String, let signer = (o["signing_address"] as? String)?.lowercased(),
              let signature = (o["signature"] as? String).flatMap({ try? NearAttestation.bytes($0.replacingOccurrences(of: "0x", with: "")) }), signature.count == 64,
              let signerBytes = try? NearAttestation.bytes(signer.replacingOccurrences(of: "0x", with: "")), signerBytes.count == 32,
              let publicKey = try? Curve25519.Signing.PublicKey(rawRepresentation: Data(signerBytes)),
              publicKey.isValidSignature(Data(signature), for: Data(text.utf8)) else {
            return .unverified("the reply's signature doesn't check out")
        }
        switch o["signature_kind"] as? String {
        case "provider_tee":
            guard text == model + ":" + hashes else { return .unverified("the signature is for other bytes") }
            return v.modelKeys.map { $0.lowercased() }.contains(signer) ? .model : .unverified("signed by a key the attestation didn't verify")
        case "gateway":
            guard text == hashes else { return .unverified("the signature is for other bytes") }
            return signer == v.gatewayKey.lowercased() ? .gateway : .unverified("signed by a key the attestation didn't verify")
        default: return .unverified("an unknown kind of signature")
        }
    }
}

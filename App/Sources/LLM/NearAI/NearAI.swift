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
        return Client(chat: OpenAIChatClient(baseURL: Self.base, key: key, jsonMode: true))
    }

    /// The last check that passed, by model: what the chat shows as «Verified · TEE».
    public static func verified(_ model: String) -> NearAttestation.Verified? { lock.withLock { checks[model] } }

    /// The enclaves behind `model` checked now, or within the last ten minutes.
    public static func ensureVerified(_ model: String) async throws -> NearAttestation.Verified {
        if let v = verified(model), Date().timeIntervalSince(v.at) < 600 { return v }
        let nonce = (0..<32).map { _ in String(format: "%02x", UInt8.random(in: 0...255)) }.joined()
        var url = URLComponents(string: base + "/attestation/report")!
        url.queryItems = [.init(name: "model", value: model), .init(name: "provider", value: "near"),
                          .init(name: "signing_algo", value: "ed25519"), .init(name: "nonce", value: nonce)]
        var r = URLRequest(url: url.url!, timeoutInterval: 60)
        r.setValue("true", forHTTPHeaderField: "x-no-aliasing")
        let (data, response) = try await URLSession.shared.data(for: r)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else {
            throw LLMError("NEAR AI gave no enclave evidence for \(model), so nothing was sent. Pick another of its private models.")
        }
        do {
            let v = try await NearAttestation.verify(data, nonce: nonce, model: model, gpu: NearAttestation.nvidiaVerdict)
            lock.withLock { checks[model] = v }
            return v
        } catch {
            lock.withLock { checks[model] = nil }
            throw LLMError("NEAR AI's enclave didn't check out, so nothing was sent: \(error.localizedDescription)")
        }
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var checks: [String: NearAttestation.Verified] = [:]

    /// The chat, each request only after its model's enclaves check out.
    struct Client: LLMClient {
        let chat: OpenAIChatClient

        func complete(_ r: LLMRequest) async throws -> String {
            _ = try await NearAI.ensureVerified(r.model)
            return try await chat.complete(r)
        }

        func stream(_ r: LLMRequest, partial: @escaping @Sendable (String) -> Void) async throws -> String {
            _ = try await NearAI.ensureVerified(r.model)
            return try await chat.stream(r, partial: partial)
        }
    }
}

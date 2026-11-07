import CryptoKit
import Foundation
import Security

/// NEAR AI Cloud's evidence that its gateway and the model run inside Intel TDX enclaves (the model on attested NVIDIA
/// GPUs), checked here before a chat goes out (docs.near.ai/cloud/verification, 2026-11-01). For every report:
/// - the quote is signed by its attestation key; Intel's Quoting Enclave vouches for that key in a report signed by the
///   platform's PCK certificate, and that certificate chains to Intel's SGX root, pinned here by its SHA-256;
/// - the quote's report data is the enclave's signing key followed by our fresh nonce: that key lives in that enclave,
///   and the evidence was made for this check;
/// - not a debug enclave; its configuration (MRCONFIGID) is the hash of the app_compose it shows, and RTMR3 is the
///   replay of its event log;
/// - the model's GPUs: NVIDIA's attestation service passes their evidence, bound to the same nonce.
/// ponytail: Intel's TCB level (collateral from Intel PCS) and the image's provenance aren't checked, so a genuine but
/// out-of-date platform still passes; add the dcap collateral when that matters.
public enum NearAttestation {
    /// Intel SGX Provisioning Certification Root CA, as certificates.trustedservices.intel.com serves it.
    public static let intelRootSHA256 = "44a0196b2b99f889b8e149e95b807a350e7424964399e885a7cbb8ccfab674d3"

    public struct Verified: Sendable, Equatable {
        public let model: String
        /// The ed25519 keys that sign inside the enclaves, hex.
        public let gatewayKey: String
        public let modelKeys: [String]
        public let at: Date
    }

    public struct Failure: LocalizedError, Equatable {
        public let message: String
        public init(_ message: String) { self.message = message }
        public var errorDescription: String? { message }
    }

    /// `report`: GET /v1/attestation/report with `model`, provider=near, signing_algo=ed25519 and `nonce` (32 random
    /// bytes, hex). `gpu`: whether NVIDIA passes one GPU evidence payload (`nvidiaVerdict`).
    public static func verify(_ report: Data, nonce: String, model: String, gpu: (String) async throws -> Bool) async throws -> Verified {
        guard let root = try? JSONSerialization.jsonObject(with: report) as? [String: Any],
              let gateway = root["gateway_attestation"] as? [String: Any] else { throw Failure("NEAR AI sent no gateway attestation.") }
        let ours = try bytes(nonce)
        guard ours.count == 32 else { throw Failure("The nonce must be 32 bytes.") }
        let gatewayKey = try check(gateway, nonce: ours, what: "NEAR AI's gateway")
        let reports = (root["model_attestations"] as? [[String: Any]] ?? []).filter { $0["model_name"] as? String == model }
        guard !reports.isEmpty else { throw Failure("NEAR AI sent no enclave evidence for \(model).") }
        var keys: [String] = []
        for r in reports {
            keys.append(try check(r, nonce: ours, what: model))
            guard let payload = r["nvidia_payload"] as? String,
                  let p = try? JSONSerialization.jsonObject(with: Data(payload.utf8)) as? [String: Any],
                  (p["nonce"] as? String)?.lowercased() == nonce.lowercased() else {
                throw Failure("\(model)'s GPU evidence isn't bound to this check.")
            }
            guard try await gpu(payload) else { throw Failure("NVIDIA didn't pass \(model)'s GPUs.") }
        }
        return Verified(model: model, gatewayKey: gatewayKey, modelKeys: keys, at: Date())
    }

    /// One report: its quote checked down to Intel's root and bound to its key and `nonce`. Returns the key.
    static func check(_ r: [String: Any], nonce: Data, what: String) throws -> String {
        guard r["signing_algo"] as? String == "ed25519", let address = r["signing_address"] as? String,
              let quote = r["intel_quote"] as? String else { throw Failure("\(what): the report is incomplete.") }
        let key = try bytes(address.hasPrefix("0x") ? String(address.dropFirst(2)) : address)
        guard key.count == 32 else { throw Failure("\(what): the signing key isn't an ed25519 key.") }
        guard (r["request_nonce"] as? String)?.lowercased() == hex(nonce) else { throw Failure("\(what): the report answers another check.") }
        let tcb = json(r["info"].flatMap { ($0 as? [String: Any])?["tcb_info"] }) as? [String: Any]
        guard let compose = tcb?["app_compose"] as? String else { throw Failure("\(what): the report shows no configuration.") }
        let events = json(r["event_log"]) as? [[String: Any]] ?? []
        try verifyQuote(try bytes(quote), reportData: key + nonce, appCompose: compose, events: events, what: what)
        return hex(key)
    }

    /// An Intel TDX v4 quote: its report data, configuration and RTMR3 as expected, and its signatures down to Intel.
    static func verifyQuote(_ quote: Data, reportData: Data, appCompose: String, events: [[String: Any]], what: String) throws {
        let q = [UInt8](quote), body = 48, signed = 48 + 584
        func data(_ from: Int, _ n: Int) throws -> Data {
            guard from >= 0, n >= 0, from + n <= q.count else { throw Failure("\(what): the quote is cut short.") }
            return Data(q[from..<(from + n)])
        }
        func u16(_ o: Int) throws -> Int { let d = try data(o, 2); return Int(d[0]) | Int(d[1]) << 8 }
        func u32(_ o: Int) throws -> Int {
            let low = try u16(o), high = try u16(o + 2)
            return low | high << 16
        }
        guard try u16(0) == 4, try u32(4) == 0x81 else { throw Failure("\(what): not a TDX quote.") }
        guard try data(body + 520, 64) == reportData else { throw Failure("\(what): the quote doesn't bind its key and our nonce.") }
        guard try data(body + 120, 1)[0] & 1 == 0 else { throw Failure("\(what): a debug enclave, which its host can read.") }
        guard try data(body + 184, 48) == Data([1]) + Data(SHA256.hash(data: Data(appCompose.utf8))) + Data(count: 15) else {
            throw Failure("\(what): the enclave runs another configuration than the one shown.")
        }
        var rtmr3 = Data(count: 48)
        for e in events where (e["imr"] as? NSNumber)?.intValue == 3 {
            guard let type = (e["event_type"] as? NSNumber)?.uint32Value else { throw Failure("\(what): an unreadable event.") }
            let event = Data((e["event"] as? String ?? "").utf8), payload = try bytes(e["event_payload"] as? String ?? "")
            let digest = SHA384.hash(data: withUnsafeBytes(of: type.littleEndian) { Data($0) } + Data(":".utf8) + event + Data(":".utf8) + payload)
            rtmr3 = Data(SHA384.hash(data: rtmr3 + Data(digest)))
        }
        guard try data(body + 472, 48) == rtmr3 else { throw Failure("\(what): the event log doesn't add up to RTMR3.") }

        // The signature data: the quote's ECDSA signature and attestation key, then Intel's QE report certifying the key.
        var o = signed + 4
        let signature = try data(o, 64), attestationKey = try data(o + 64, 64)
        o += 128
        guard try u16(o) == 6 else { throw Failure("\(what): the quote carries no QE report.") }
        o += 6
        let qe = try data(o, 384), qeSignature = try data(o + 384, 64)
        o += 448
        let authLength = try u16(o), auth = try data(o + 2, authLength)
        o += 2 + authLength
        guard try u16(o) == 5 else { throw Failure("\(what): the quote carries no PCK certificates.") }
        let pem = String(decoding: try data(o + 6, try u32(o + 2)), as: UTF8.self)
        guard (try? P256.Signing.PublicKey(rawRepresentation: attestationKey))?
            .isValidSignature(try P256.Signing.ECDSASignature(rawRepresentation: signature), for: try data(0, signed)) == true else {
            throw Failure("\(what): the quote's signature doesn't verify.")
        }
        guard qe[320..<352] == Data(SHA256.hash(data: attestationKey + auth)) else {
            throw Failure("\(what): Intel's Quoting Enclave didn't vouch for this quote's key.")
        }
        guard try pckKey(pem, what: what).isValidSignature(try P256.Signing.ECDSASignature(rawRepresentation: qeSignature), for: qe) else {
            throw Failure("\(what): the QE report isn't signed by the platform's Intel certificate.")
        }
    }

    /// The platform's PCK key, once its certificates chain to Intel's pinned root (dates checked).
    static func pckKey(_ pem: String, what: String) throws -> P256.Signing.PublicKey {
        let ders = pem.components(separatedBy: "-----END CERTIFICATE-----").compactMap { part -> Data? in
            guard let r = part.range(of: "-----BEGIN CERTIFICATE-----") else { return nil }
            return Data(base64Encoded: part[r.upperBound...].filter { !$0.isWhitespace })
        }
        guard ders.count >= 2, let root = ders.last, hex(Data(SHA256.hash(data: root))) == intelRootSHA256 else {
            throw Failure("\(what): the certificates don't end at Intel's root.")
        }
        let certs = ders.compactMap { SecCertificateCreateWithData(nil, $0 as CFData) }
        var trust: SecTrust?
        guard certs.count == ders.count,
              SecTrustCreateWithCertificates(Array(certs.dropLast()) as CFArray, SecPolicyCreateBasicX509(), &trust) == errSecSuccess,
              let trust, SecTrustSetAnchorCertificates(trust, [certs[certs.count - 1]] as CFArray) == errSecSuccess,
              SecTrustSetAnchorCertificatesOnly(trust, true) == errSecSuccess, SecTrustEvaluateWithError(trust, nil),
              let key = SecCertificateCopyKey(certs[0]), let raw = SecKeyCopyExternalRepresentation(key, nil) as Data?,
              let pck = try? P256.Signing.PublicKey(x963Representation: raw) else {
            throw Failure("\(what): the Intel certificate chain doesn't verify.")
        }
        return pck
    }

    /// NVIDIA's remote attestation service on one payload: its token's overall result.
    /// ponytail: the token's signature isn't checked against NVIDIA's keys; it's trusted for coming over TLS from
    /// nras.attestation.nvidia.com. Check it with NVIDIA's JWKS if the service is ever reached through anyone else.
    public static func nvidiaVerdict(_ payload: String) async throws -> Bool {
        var r = URLRequest(url: URL(string: "https://nras.attestation.nvidia.com/v3/attest/gpu")!, timeoutInterval: 60)
        r.httpMethod = "POST"
        r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        r.httpBody = Data(payload.utf8)
        let (data, _) = try await URLSession.shared.data(for: r)
        guard let list = try? JSONSerialization.jsonObject(with: data) as? [Any], let first = list.first as? [Any],
              first.first as? String == "JWT", let token = first.last as? String else { return false }
        return claims(token)?["x-nvidia-overall-att-result"] as? Bool == true
    }

    static func claims(_ jwt: String) -> [String: Any]? {
        let parts = jwt.split(separator: ".")
        guard parts.count >= 2 else { return nil }
        var b = parts[1].replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        b += String(repeating: "=", count: (4 - b.count % 4) % 4)
        return Data(base64Encoded: b).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }

    /// A field that is JSON or JSON written as a string.
    static func json(_ v: Any?) -> Any? {
        if let s = v as? String { return try? JSONSerialization.jsonObject(with: Data(s.utf8)) }
        return v
    }

    static func bytes(_ hex: String) throws -> Data {
        guard hex.count % 2 == 0 else { throw Failure("Odd hex.") }
        var out = Data(capacity: hex.count / 2), i = hex.startIndex
        while i < hex.endIndex {
            let j = hex.index(i, offsetBy: 2)
            guard let b = UInt8(hex[i..<j], radix: 16) else { throw Failure("Not hex.") }
            out.append(b)
            i = j
        }
        return out
    }

    static func hex(_ d: Data) -> String { d.map { String(format: "%02x", $0) }.joined() }
}

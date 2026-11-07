import Foundation
import WalletCore

/// x402 (github.com/coinbase/x402), version 2: an API answers 402 with a PAYMENT-REQUIRED header — base64 JSON whose
/// `accepts` lists what it takes — and the request is sent again with PAYMENT-SIGNATURE, the chosen entry echoed and an
/// EIP-3009 authorization the service's facilitator settles. ihatecrypto takes the `exact` scheme in USDC on Base only.
public enum X402 {
    public static let baseUSDC = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"
    /// The signer's cap per payment (X402Authorization.cap): a dearer offer gets no card.
    public static let cap: UInt64 = 5_000_000

    public struct Offer: Equatable, Sendable {
        /// The `accepts` entry and the `resource` exactly as they came: the payment echoes them unchanged.
        public let accepted: Data
        public let resource: Data?
        /// The 402's `extensions`, echoed back as the spec asks.
        public let extensions: Data?
        public let chainId: UInt64
        public let asset: String
        public let payTo: String
        /// USDC units.
        public let amount: UInt64
        public let maxTimeoutSeconds: Int
        public var usdc: Decimal { Decimal(amount) / 1_000_000 }
    }

    /// The offer in a 402's PAYMENT-REQUIRED header that ihatecrypto can pay, or nil.
    public static func offer(header: String?) -> Offer? {
        guard let header, let json = Data(base64Encoded: header.trimmingCharacters(in: .whitespaces)),
              let o = try? JSONSerialization.jsonObject(with: json) as? [String: Any], o["x402Version"] as? Int == 2,
              let list = o["accepts"] as? [[String: Any]], let raws = try? NearOwnerSigning.rawElements(NearOwnerSigning.rawValue("accepts", in: json)),
              raws.count == list.count else { return nil }
        for (a, raw) in zip(list, raws) {
            guard a["scheme"] as? String == "exact", a["network"] as? String == "eip155:8453",
                  let asset = (a["asset"] as? String)?.lowercased(), asset == baseUSDC,
                  let payTo = a["payTo"] as? String, payTo.wholeMatch(of: /0x[0-9a-fA-F]{40}/) != nil,
                  let amount = UInt64(a["amount"] as? String ?? ""), amount > 0, amount <= cap,
                  ((a["extra"] as? [String: Any])?["assetTransferMethod"] as? String).map({ $0 == "eip3009" }) ?? true else { continue }
            return Offer(accepted: raw, resource: try? NearOwnerSigning.rawValue("resource", in: json),
                         extensions: try? NearOwnerSigning.rawValue("extensions", in: json), chainId: 8453, asset: asset,
                         payTo: payTo, amount: amount, maxTimeoutSeconds: a["maxTimeoutSeconds"] as? Int ?? 300)
        }
        return nil
    }

    /// The PAYMENT-SIGNATURE header for `offer`, signed by `from`.
    public static func payment(_ offer: Offer, from: String, validBefore: UInt64, nonce: Data, signature: Data) -> (field: String, value: String) {
        func hex(_ d: Data) -> String { "0x" + d.map { String(format: "%02x", $0) }.joined() }
        let authorization = ["from": from, "to": offer.payTo, "value": "\(offer.amount)", "validAfter": "0", "validBefore": "\(validBefore)", "nonce": hex(nonce)]
        let payload = (try? JSONSerialization.data(withJSONObject: ["signature": hex(signature), "authorization": authorization])) ?? Data()
        var json = Data(#"{"x402Version":2,"#.utf8)
        if let resource = offer.resource { json += Data(#""resource":"#.utf8) + resource + Data(",".utf8) }
        json += Data(#""accepted":"#.utf8) + offer.accepted + Data(#","payload":"#.utf8) + payload
        if let extensions = offer.extensions { json += Data(#","extensions":"#.utf8) + extensions }
        json += Data("}".utf8)
        return ("PAYMENT-SIGNATURE", json.base64EncodedString())
    }

    /// The settlement's transaction from a PAYMENT-RESPONSE header, when it says it succeeded.
    public static func settled(_ header: String?) -> String? {
        guard let header, let json = Data(base64Encoded: header.trimmingCharacters(in: .whitespaces)),
              let o = try? JSONSerialization.jsonObject(with: json) as? [String: Any], o["success"] as? Bool == true else { return nil }
        return (o["transaction"] as? String).flatMap { $0.wholeMatch(of: /0x[0-9a-fA-F]{64}/) != nil ? $0 : nil }
    }
}

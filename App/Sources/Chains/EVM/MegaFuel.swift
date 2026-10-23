import Foundation
import WalletCore

/// A dollar transfer on BNB Chain at gas price 0, which BNB Chain's own paymaster, MegaFuel (BEP-414, run by NodeReal),
/// pays the gas for: the way out for USDC there with no BNB, whose Binance-Peg token has no payment by signature. MegaFuel
/// sponsors plain stablecoin transfers (checked 2026-10-23 with pm_isSponsorable: «sponsorName: BNB Chain»), not approvals
/// or contract calls, so the dollar goes to Relay's deposit address for the order. The signer builds the transfer itself:
/// only a dollar it knows, only `transfer`, the fee zero; the recipient is approved like any other.
public struct SponsoredTransfer: Codable, Sendable, Equatable {
    public let chainId: UInt64
    public let nonce: UInt64
    public let gasLimit: UInt64
    public let token: String
    public let to: String
    /// In the token's own units (18 decimals on BNB Chain), as a decimal string: 20 USDC there is past 64 bits.
    public let amount: String

    public init(chainId: UInt64, nonce: UInt64, gasLimit: UInt64, token: String, to: String, amount: String) {
        (self.chainId, self.nonce, self.gasLimit, self.token, self.to, self.amount) = (chainId, nonce, gasLimit, token, to, amount)
    }

    /// The dollars MegaFuel sponsors a transfer of, by "chain:token", with their symbol and decimals.
    public static let tokens: [String: (symbol: String, decimals: Int)] = [
        "56:0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d": ("USDC", 18),
    ]

    public static func supports(chainId: UInt64, token: String) -> Bool { tokens["\(chainId):\(token.lowercased())"] != nil }

    var known: (symbol: String, decimals: Int)? { Self.tokens["\(chainId):\(token.lowercased())"] }

    /// Why the signer won't sign it, or nil.
    public var refusal: String? {
        guard known != nil else { return "\(token) on network \(chainId) isn't a dollar BNB Chain pays the gas for." }
        guard let units = Decimal(string: amount), units > 0, units == units.whole else { return "The transfer's amount isn't a whole number of units." }
        guard (try? ABI.address(to)) != nil, to.lowercased() != "0x0000000000000000000000000000000000000000" else { return "The transfer has no recipient." }
        guard gasLimit > 0, gasLimit <= 100_000 else { return "A token transfer takes under 100,000 gas." }
        return nil
    }

    public func calldata() throws -> Data {
        guard let units = Decimal(string: amount) else { throw WalletError("Invalid amount \(amount).") }
        return try ERC20.transfer(to: to, units: units)
    }

    /// rlp([nonce, gasPrice 0, gasLimit, token, value 0, data, chainId, 0, 0]): EIP-155's signing payload.
    func fields() throws -> [RLP] {
        guard let token = Data(hex: token), token.count == 20 else { throw WalletError("Invalid token address: \(self.token)") }
        return [.uint(nonce), .uint(0), .uint(gasLimit), .bytes(token), .uint(0), .bytes(try calldata())]
    }

    public func signingHash() throws -> Data {
        Keccak.hash256(RLP.list(try fields() + [.uint(chainId), .uint(0), .uint(0)]).encoded)
    }

    /// The raw legacy transaction, v = recovery id + chainId·2 + 35.
    public func signed(privateKey: Data) throws -> (raw: Data, hash: String) {
        let s = try EVMSigner.sign(digest: try signingHash(), privateKey: privateKey)
        let raw = RLP.list(try fields() + [.uint(UInt64(s.v - 27) + chainId * 2 + 35), .uint(bigEndian: s.r), .uint(bigEndian: s.s)]).encoded
        return (raw, "0x" + Keccak.hash256(raw).hex)
    }

    /// What the prompt says.
    public var summary: String {
        let amount = (Decimal(string: amount) ?? 0) / pow(10, known?.decimals ?? 0)
        return "send \(amount) \(known?.symbol ?? "tokens") on \(EVMRPC.names[chainId] ?? "network \(chainId)") to \(to) (BNB Chain pays the gas)"
    }
}

/// BNB Chain's paymaster, MegaFuel: asks whether a zero-fee transaction is sponsored, gives the nonce and takes it.
/// Public, no key (docs.nodereal.io/reference/pm-issponsorable).
public enum MegaFuel {
    public static let endpoint = URL(string: "https://bsc-megafuel.nodereal.io/")!

    static func call(_ method: String, _ params: [Any]) async throws -> Any {
        var request = URLRequest(url: endpoint, timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["jsonrpc": "2.0", "id": 1, "method": method, "params": params])
        let ask = request
        return try await RPCRetry.run({ try await URLSession.shared.data(for: ask) }) { data, status in
            guard let j = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
                throw RPCRetry.Again(status == 429 || status >= 500, WalletError("MegaFuel: \(RPCRetry.said(data, status: status))"))
            }
            if let e = j["error"] as? [String: Any] { throw WalletError("MegaFuel: \(e["message"] as? String ?? "\(e)")") }
            guard let result = j["result"] else { throw WalletError("MegaFuel answered nothing.") }
            return result
        }
    }

    /// Whether BNB Chain pays the gas for this transfer from `from`.
    public static func sponsors(_ t: SponsoredTransfer, from: String) async throws -> Bool {
        let tx: [String: Any] = ["from": from, "to": t.token, "value": "0x0", "data": "0x" + (try t.calldata()).hex,
                                 "gas": "0x" + String(t.gasLimit, radix: 16)]
        return ((try await call("pm_isSponsorable", [tx])) as? [String: Any])?["sponsorable"] as? Bool ?? false
    }

    /// The next nonce, pending transactions counted (MegaFuel's own view).
    public static func nonce(_ address: String) async throws -> UInt64 {
        guard let hex = try await call("eth_getTransactionCount", [address, "pending"]) as? String, let n = UInt64(hex.dropFirst(2), radix: 16) else {
            throw WalletError("MegaFuel gave no nonce.")
        }
        return n
    }

    /// Sends the signed zero-fee transaction; MegaFuel bundles it with its own that pays the gas. Returns the hash.
    public static func send(_ raw: Data) async throws -> String {
        guard let hash = try await call("eth_sendRawTransaction", ["0x" + raw.hex]) as? String else { throw WalletError("MegaFuel gave no transaction hash.") }
        return hash
    }
}

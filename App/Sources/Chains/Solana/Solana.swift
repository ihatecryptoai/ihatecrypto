import CryptoKit
import Foundation
import WalletCore

/// Solana JSON-RPC: only what sending and confirming a transaction needs.
public struct SolanaRPC: Sendable {
    /// The node asked first.
    public let url: URL
    /// Made for one node (a check, a test): no other is asked.
    let pinned: Bool

    /// SolanaNodes, read at each call: the user's own node first when set, the public one after it.
    public init() { (url, pinned) = (SolanaNodes.urls[0], false) }
    public init(url: URL) { (self.url, pinned) = (url, true) }

    public func call(_ method: String, _ params: [Any]) async throws -> Any {
        let nodes = pinned ? [url] : SolanaNodes.urls
        var attempt = 0
        var request = URLRequest(url: nodes[0], timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["jsonrpc": "2.0", "id": 1, "method": method, "params": params])
        // The public RPC answers 429 under bursts and drops a connection now and then: backed-off retries (RPCRetry),
        // each to the next node, then a clear error. A resent transaction is the same one (its signature), so sends
        // retry too. The user's node refusing its key (401, 403: expired, over its plan) passes the call to the next.
        return try await RPCRetry.run {
            request.url = nodes[attempt % nodes.count]
            attempt += 1
            return try await URLSession.shared.data(for: request)
        } answer: { data, status in
            guard (200..<300).contains(status) else {
                let next = (status == 401 || status == 403) && nodes.count > 1
                throw RPCRetry.Again(status == 429 || status >= 500 || next, WalletError("Solana RPC \(method): HTTP \(status) \(String(decoding: data.prefix(120), as: UTF8.self))"))
            }
            guard let reply = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
                throw WalletError("Solana RPC \(method): response isn't JSON: \(String(decoding: data.prefix(120), as: UTF8.self))")
            }
            if let error = reply["error"] as? [String: Any] {
                let message = error["message"] as? String ?? "\(error)"
                throw RPCRetry.Again(RPCRetry.transient(message), WalletError("Solana RPC \(method): \(message)"))
            }
            return reply["result"] ?? NSNull()
        }
    }

    public func balance(_ address: String) async throws -> UInt64 {
        let r = try await call("getBalance", [address, ["commitment": "confirmed"]]) as? [String: Any]
        return (r?["value"] as? NSNumber)?.uint64Value ?? 0
    }

    /// What `owner`'s own token account for `mint` holds, in whole tokens; 0 without one. Two light reads (the mint's
    /// program, the account's balance), never getTokenAccountsByOwner, which the public node throttles right after a swap.
    public func ownedAmount(owner: String, mint: String) async throws -> Decimal {
        let info = try await call("getAccountInfo", [mint, ["encoding": "base64", "commitment": "confirmed"]]) as? [String: Any]
        guard let program = (info?["value"] as? [String: Any])?["owner"] as? String, [SPL.tokenProgram, SPL.token2022Program].contains(program) else {
            throw WalletError("\(mint) isn't a token mint on Solana.")
        }
        let account = try SPL.associatedAccount(owner: owner, mint: mint, program: program)
        guard let value = (try? await call("getTokenAccountBalance", [account, ["commitment": "confirmed"]]) as? [String: Any])?["value"] as? [String: Any],
              let units = (value["amount"] as? String).flatMap({ Decimal(string: $0) }), let decimals = (value["decimals"] as? NSNumber)?.intValue else { return 0 }
        return units / pow(Decimal(10), decimals)
    }

    /// Raw units of an SPL token held by `owner` across its accounts.
    public func tokenBalance(owner: String, mint: String) async throws -> UInt64 {
        let r = try await call("getTokenAccountsByOwner", [owner, ["mint": mint], ["encoding": "jsonParsed", "commitment": "confirmed"]]) as? [String: Any]
        let accounts = r?["value"] as? [[String: Any]] ?? []
        return accounts.reduce(0) { sum, account in
            let amount = (((account["account"] as? [String: Any])?["data"] as? [String: Any])?["parsed"] as? [String: Any])
                .flatMap { (($0["info"] as? [String: Any])?["tokenAmount"] as? [String: Any])?["amount"] as? String }
            return sum + (amount.flatMap { UInt64($0) } ?? 0)
        }
    }

    public func latestBlockhash() async throws -> (blockhash: String, lastValidBlockHeight: UInt64) {
        let r = try await call("getLatestBlockhash", [["commitment": "confirmed"]]) as? [String: Any]
        guard let value = r?["value"] as? [String: Any], let hash = value["blockhash"] as? String,
              let height = (value["lastValidBlockHeight"] as? NSNumber)?.uint64Value else { throw WalletError("No blockhash") }
        return (hash, height)
    }

    /// Simulates first (preflight), then broadcasts. Returns the signature, which is the transaction id.
    public func send(_ transaction: Data) async throws -> String {
        do {
            let r = try await call("sendTransaction", [transaction.base64EncodedString(), ["encoding": "base64", "preflightCommitment": "confirmed", "maxRetries": 5]])
            guard let signature = r as? String else { throw WalletError("sendTransaction returned no signature") }
            return signature
        } catch let error as WalletError where error.message.contains("already been processed") && transaction.count > 65 {
            // A retry after the first send got through: the same transaction, named by its first signature (after the count byte).
            return Base58.encode(transaction[transaction.startIndex + 1 ..< transaction.startIndex + 65])
        }
    }

    /// Runs the transaction without sending it, with signature checks on. nil error = it would succeed.
    public func simulate(_ transaction: Data) async throws -> (error: String?, logs: [String]) {
        let r = try await call("simulateTransaction", [transaction.base64EncodedString(), ["encoding": "base64", "sigVerify": true, "commitment": "confirmed"]]) as? [String: Any]
        let value = r?["value"] as? [String: Any]
        let err = value?["err"]
        return (err == nil || err is NSNull ? nil : "\(err!)", value?["logs"] as? [String] ?? [])
    }

    public enum Outcome: Equatable, Sendable { case confirmed, failed(String), expired }

    /// Waits for "confirmed". A transaction whose blockhash is past `lastValidBlockHeight` can never land:
    /// that is a definite «didn't land», not a guess, so it is safe to build a new one.
    public func waitForConfirmation(_ signature: String, lastValidBlockHeight: UInt64, timeout: TimeInterval = 120) async throws -> Outcome {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            let r = try await call("getSignatureStatuses", [[signature], ["searchTransactionHistory": true]]) as? [String: Any]
            if let status = (r?["value"] as? [Any])?.first as? [String: Any] {
                if let err = status["err"], !(err is NSNull) { return .failed("\(err)") }
                let level = status["confirmationStatus"] as? String
                if level == "confirmed" || level == "finalized" { return .confirmed }
            } else if let height = (try await call("getBlockHeight", [["commitment": "confirmed"]]) as? NSNumber)?.uint64Value,
                      height > lastValidBlockHeight {
                return .expired
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw WalletError("Transaction status still unknown after \(Int(timeout)) s: \(signature)")
    }
}

public enum SolanaTransaction {
    static let systemProgram = Data(count: 32) // 11111111111111111111111111111111
    static let computeBudgetProgram = try! Base58.decode("ComputeBudget111111111111111111111111111111")

    /// Native SOL transfer with a small priority fee, as a signed legacy transaction.
    public static func transfer(from privateKey: Data, to recipient: String, lamports: UInt64, blockhash: String,
                                microLamportsPerCU: UInt64 = 20_000) throws -> (signature: String, wire: Data) {
        let signer = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let from = signer.publicKey.rawRepresentation
        let to = try Base58.decode(recipient)
        guard to.count == 32 else { throw WalletError("Invalid Solana address: \(recipient)") }
        guard to != from else { throw WalletError("Transfer to yourself") }

        var message = Data([1, 0, 2])                         // 1 signer, 0 readonly signed, 2 readonly unsigned
        message += compact(4) + from + to + systemProgram + computeBudgetProgram
        message += try Base58.decode(blockhash)
        message += compact(2)
        // ComputeBudget.SetComputeUnitPrice
        message += Data([3]) + compact(0) + compact(9) + Data([3]) + le(microLamportsPerCU)
        // System.Transfer: u32 index 2, u64 lamports
        message += Data([2]) + compact(2) + Data([0, 1]) + compact(12) + le(UInt32(2)) + le(lamports)

        let signature = try signer.signature(for: message)
        return (Base58.encode(signature), compact(1) + signature + message)
    }

    /// Signs a transaction someone else built (Jupiter). Refuses unless our key is the fee payer, account 0 — or, with a
    /// `sponsor` (a 32-byte public key) paying, unless it is account 0 and our key the second and last signer.
    public static func sign(_ wire: Data, privateKey: Data, sponsor: Data? = nil) throws -> (signature: String, wire: Data) {
        let signer = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let (message, slot) = try unsigned(wire, payer: signer.publicKey.rawRepresentation, sponsor: sponsor)
        let signature = try signer.signature(for: message)
        var signed = wire
        signed.replaceSubrange(slot, with: signature)
        return (Base58.encode(signature), signed)
    }

    /// The message our key signs and where in `wire` its 64-byte signature goes. Refuses unless `payer` (a 32-byte
    /// public key) is the fee payer, account 0, and the only signer; with a `sponsor`, unless the sponsor is account 0 and
    /// `payer` account 1 of exactly two signers.
    public static func unsigned(_ wire: Data, payer: Data, sponsor: Data? = nil) throws -> (message: Data, slot: Range<Data.Index>) {
        var offset = 0
        let signatureCount = try readCompact(wire, &offset)
        let signaturesStart = offset
        let messageStart = offset + signatureCount * 64
        guard signatureCount >= 1, wire.count > messageStart else { throw WalletError("Incomplete transaction") }
        let message = wire.subdata(in: messageStart..<wire.count)

        var m = message.startIndex
        if message[m] & 0x80 != 0 { m += 1 }                    // versioned (v0) prefix
        let signers = sponsor == nil ? 1 : 2
        guard message[m] == signers, signatureCount == signers else {
            throw WalletError("The transaction needs \(message[m]) signatures; expected \(sponsor == nil ? "only ours" : "the fee payer's and ours")")
        }
        m += 3
        var keyOffset = m - message.startIndex
        guard try readCompact(message, &keyOffset) >= signers, message.startIndex + keyOffset + 32 * signers <= message.endIndex else {
            throw WalletError("Incomplete transaction")
        }
        func key(_ i: Int) -> Data { message.subdata(in: (message.startIndex + keyOffset + 32 * i)..<(message.startIndex + keyOffset + 32 * (i + 1))) }
        if let sponsor {
            guard key(0) == sponsor, sponsor != payer, key(1) == payer else { throw WalletError("The fee payer isn't the sponsor, or our wallet isn't the second signer") }
        } else {
            guard key(0) == payer else { throw WalletError("The fee payer isn't our wallet") }
        }
        let start = wire.startIndex + signaturesStart + 64 * (signers - 1)
        return (message, start..<(start + 64))
    }

    static func compact(_ n: Int) -> Data {
        var value = n, out = Data()
        repeat {
            var byte = UInt8(value & 0x7f)
            value >>= 7
            if value > 0 { byte |= 0x80 }
            out.append(byte)
        } while value > 0
        return out
    }

    static func readCompact(_ data: Data, _ offset: inout Int) throws -> Int {
        var value = 0, shift = 0
        while true {
            guard data.startIndex + offset < data.endIndex else { throw WalletError("Truncated length") }
            let byte = data[data.startIndex + offset]
            offset += 1
            value |= Int(byte & 0x7f) << shift
            if byte & 0x80 == 0 { return value }
            shift += 7
        }
    }

    static func le<T: FixedWidthInteger>(_ v: T) -> Data { withUnsafeBytes(of: v.littleEndian) { Data($0) } }
}

/// An instruction as protocols hand it over (Relay's quote steps, for example).
public struct SolanaInstruction: Sendable {
    public struct Account: Sendable {
        public let pubkey: String
        public let isSigner: Bool
        public let isWritable: Bool
        public init(pubkey: String, isSigner: Bool, isWritable: Bool) {
            self.pubkey = pubkey
            self.isSigner = isSigner
            self.isWritable = isWritable
        }
    }

    public let program: String
    public let accounts: [Account]
    public let data: Data

    public init(program: String, accounts: [Account], data: Data) {
        self.program = program
        self.accounts = accounts
        self.data = data
    }
}

extension SolanaTransaction {
    /// Compiles instructions into a signed legacy transaction, fee payer first, with a priority fee.
    public static func compile(_ instructions: [SolanaInstruction], privateKey: Data, blockhash: String,
                               microLamportsPerCU: UInt64 = 20_000) throws -> (signature: String, wire: Data) {
        let signer = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let message = try compileMessage(instructions, payer: Base58.encode(signer.publicKey.rawRepresentation),
                                         blockhash: blockhash, microLamportsPerCU: microLamportsPerCU)
        let signature = try signer.signature(for: message)
        return (Base58.encode(signature), compact(1) + signature + message)
    }

    /// A one-signer transaction with an empty signature slot, for a signer in another process.
    public static func unsigned(_ message: Data) -> Data { compact(1) + Data(count: 64) + message }

    /// The legacy message for `instructions`, fee payer first, with a priority fee.
    /// ponytail: no lookup tables, so every account is static; fine up to ~30 accounts (1232-byte limit).
    public static func compileMessage(_ instructions: [SolanaInstruction], payer: String, blockhash: String,
                                      microLamportsPerCU: UInt64 = 20_000) throws -> Data {
        let budget = SolanaInstruction(program: Base58.encode(computeBudgetProgram), accounts: [],
                                       data: Data([3]) + le(microLamportsPerCU))
        let all = [budget] + instructions

        // Merge every account's flags; programs are readonly non-signers.
        var flags: [String: (signer: Bool, writable: Bool)] = [payer: (true, true)]
        var order = [payer]
        func add(_ key: String, signer: Bool, writable: Bool) {
            if let f = flags[key] { flags[key] = (f.signer || signer, f.writable || writable) }
            else { flags[key] = (signer, writable); order.append(key) }
        }
        for ix in all {
            for a in ix.accounts { add(a.pubkey, signer: a.isSigner, writable: a.isWritable) }
            add(ix.program, signer: false, writable: false)
        }
        let signers = order.filter { flags[$0]!.signer }
        guard signers == [payer] else { throw WalletError("The instructions need other signers: \(signers.filter { $0 != payer })") }
        let keys = [payer]
            + order.filter { $0 != payer && flags[$0]!.writable }
            + order.filter { $0 != payer && !flags[$0]!.writable }
        let readonlyUnsigned = keys.filter { !flags[$0]!.writable }.count
        let index = Dictionary(uniqueKeysWithValues: keys.enumerated().map { ($1, $0) })

        var message = Data([1, 0, UInt8(readonlyUnsigned)]) + compact(keys.count)
        for key in keys { message += try Base58.decode(key) }
        message += try Base58.decode(blockhash)
        message += compact(all.count)
        for ix in all {
            message += Data([UInt8(index[ix.program]!)]) + compact(ix.accounts.count) + Data(ix.accounts.map { UInt8(index[$0.pubkey]!) })
            message += compact(ix.data.count) + ix.data
        }
        guard message.count + 65 <= 1232 else { throw WalletError("The transaction is over 1232 bytes") }
        return message
    }
}

extension SolanaNodes {
    /// Why `url` can't be the wallet's Solana node, or nil: https only; Solana mainnet (its genesis hash); and token
    /// accounts by owner answered — the call free public nodes turn away (publicnode: 403), which balances and moves need.
    public static func problem(with url: URL) async -> String? {
        guard url.scheme == "https", url.host != nil else { return "A node's address starts with https://." }
        let node = SolanaRPC(url: url)
        do {
            guard let genesis = try await node.call("getGenesisHash", []) as? String else { return "The node didn't answer like a Solana node." }
            guard genesis == mainnetGenesis else { return "That node is on another Solana network (devnet or testnet), not mainnet." }
            _ = try await node.call("getTokenAccountsByOwner", ["11111111111111111111111111111111",
                                                                ["programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"], ["encoding": "jsonParsed"]])
            _ = try await node.call("getLatestBlockhash", [["commitment": "confirmed"]])
            return nil
        } catch {
            let why = error.localizedDescription
            if why.contains("HTTP 401") || why.contains("HTTP 403") {
                return "The node turned the key away (\(why.contains("HTTP 401") ? "401" : "403")): copy the whole address again, api-key included — or this plan doesn't serve these calls."
            }
            return "The node didn't answer: \(why)"
        }
    }
}

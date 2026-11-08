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
        let r = try await call("getBalance", [address, ["commitment": "finalized"]]) as? [String: Any]
        return (r?["value"] as? NSNumber)?.uint64Value ?? 0
    }

    /// The lamports an account of `bytes` must hold to open: its rent minimum as the network says it now. Never written
    /// down: it fell from 890,880 for an empty account to 650,240 in Sep 2026 and keeps stepping down (SIMD-0437).
    public func rentMinimum(bytes: Int = 0) async throws -> UInt64 {
        // Bounded, so no sum with it can overflow: a node's -1 or 1e30 is no rent (2^40 lamports ≈ 1,100 SOL).
        guard let lamports = (try await call("getMinimumBalanceForRentExemption", [bytes]) as? NSNumber)?.int64Value,
              lamports > 0, lamports < 1 << 40 else {
            throw WalletError("Solana RPC getMinimumBalanceForRentExemption: no rent minimum in the answer")
        }
        return UInt64(lamports)
    }

    /// What `owner`'s own token account for `mint` holds, in whole tokens; 0 without one. Two light reads (the mint's
    /// program, the account's balance), never getTokenAccountsByOwner, which the public node throttles right after a swap.
    public func ownedAmount(owner: String, mint: String) async throws -> Decimal {
        let info = try await call("getAccountInfo", [mint, ["encoding": "base64", "commitment": "finalized"]]) as? [String: Any]
        guard let program = (info?["value"] as? [String: Any])?["owner"] as? String, [SPL.tokenProgram, SPL.token2022Program].contains(program) else {
            throw WalletError("\(mint) isn't a token mint on Solana.")
        }
        let account = try SPL.associatedAccount(owner: owner, mint: mint, program: program)
        guard let value = (try? await call("getTokenAccountBalance", [account, ["commitment": "finalized"]]) as? [String: Any])?["value"] as? [String: Any],
              let units = (value["amount"] as? String).flatMap({ Decimal(string: $0) }), let decimals = (value["decimals"] as? NSNumber)?.intValue else { return 0 }
        return units / pow(Decimal(10), decimals)
    }

    /// Raw units of an SPL token held by `owner` across its accounts.
    public func tokenBalance(owner: String, mint: String) async throws -> UInt64 {
        let r = try await call("getTokenAccountsByOwner", [owner, ["mint": mint], ["encoding": "jsonParsed", "commitment": "finalized"]]) as? [String: Any]
        let accounts = r?["value"] as? [[String: Any]] ?? []
        return accounts.reduce(0) { sum, account in
            let amount = (((account["account"] as? [String: Any])?["data"] as? [String: Any])?["parsed"] as? [String: Any])
                .flatMap { (($0["info"] as? [String: Any])?["tokenAmount"] as? [String: Any])?["amount"] as? String }
            return sum + (amount.flatMap { UInt64($0) } ?? 0)
        }
    }

    public func latestBlockhash() async throws -> (blockhash: String, lastValidBlockHeight: UInt64) {
        let r = try await call("getLatestBlockhash", [["commitment": "finalized"]]) as? [String: Any]
        guard let value = r?["value"] as? [String: Any], let hash = value["blockhash"] as? String,
              let height = (value["lastValidBlockHeight"] as? NSNumber)?.uint64Value else { throw WalletError("No blockhash") }
        return (hash, height)
    }

    /// Simulates first (preflight), then broadcasts. Returns the signature, which is the transaction id.
    public func send(_ transaction: Data) async throws -> String {
        do {
            let r = try await call("sendTransaction", [transaction.base64EncodedString(), ["encoding": "base64", "preflightCommitment": "finalized", "maxRetries": 5]])
            guard let signature = r as? String else { throw WalletError("sendTransaction returned no signature") }
            return signature
        } catch let error as WalletError where error.message.contains("already been processed") && transaction.count > 65 {
            // A retry after the first send got through: the same transaction, named by its first signature (after the count byte).
            return Base58.encode(transaction[transaction.startIndex + 1 ..< transaction.startIndex + 65])
        }
    }

    /// Runs the transaction without sending it, with signature checks on. nil error = it would succeed.
    public func simulate(_ transaction: Data) async throws -> (error: String?, logs: [String]) {
        let r = try await call("simulateTransaction", [transaction.base64EncodedString(), ["encoding": "base64", "sigVerify": true, "commitment": "finalized"]]) as? [String: Any]
        let value = r?["value"] as? [String: Any]
        let err = value?["err"]
        return (err == nil || err is NSNull ? nil : "\(err!)", value?["logs"] as? [String] ?? [])
    }

    public enum Outcome: Equatable, Sendable { case confirmed, failed(String), expired }

    /// Waits for "confirmed". Past `lastValidBlockHeight` a transaction can't land any more, so «expired» is a definite
    /// «didn't land», not a guess, and it is safe to build a new one — once the status, read again from a node that has
    /// reached a finalized block past that height, still finds nothing. Read before that block, or from a node behind it
    /// (another node of the public pool, the public node after the user's own), a missing status may only not have reached
    /// that node yet; a confirmed block wouldn't do, as a node can be ahead of it on another fork. That costs the expiry at
    /// most the gap between confirmed and finalized.
    public func waitForConfirmation(_ signature: String, lastValidBlockHeight: UInt64, timeout: TimeInterval = 120) async throws -> Outcome {
        let deadline = Date().addingTimeInterval(timeout)
        var pastSlot: UInt64?   // a finalized slot whose block height is past lastValidBlockHeight
        while Date() < deadline {
            let r = try await call("getSignatureStatuses", [[signature], ["searchTransactionHistory": true]]) as? [String: Any]
            if let status = (r?["value"] as? [Any])?.first as? [String: Any] {
                if let err = status["err"], !(err is NSNull) { return .failed("\(err)") }
                let level = status["confirmationStatus"] as? String
                if level == "confirmed" || level == "finalized" { return .confirmed }
            } else if let pastSlot {
                let seen = ((r?["context"] as? [String: Any])?["slot"] as? NSNumber)?.uint64Value ?? 0
                if seen >= pastSlot { return .expired }
            } else if let epoch = try await call("getEpochInfo", [["commitment": "finalized"]]) as? [String: Any],
                      let height = (epoch["blockHeight"] as? NSNumber)?.uint64Value, height > lastValidBlockHeight,
                      let slot = (epoch["absoluteSlot"] as? NSNumber)?.uint64Value {
                pastSlot = slot
                continue   // the status once more, now: it may have landed between the two reads
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw WalletError("Transaction status still unknown after \(Int(timeout)) s: \(signature)")
    }

    /// A transaction we signed second (Jupiter's gasless swap, a JupiterZ fill): the network knows it only by the first
    /// signature, someone else's, so it's looked for among `signer`'s own transactions, with its id. `slot`: where a node
    /// knew `blockhash` before we signed (the simulation's), so the transaction can only be in a later one.
    /// «expired» only once a node finalized past `slot` finds the blockhash past use, at finalized slot D — nothing lands
    /// after D — and the wallet's transactions, read finalized at or past D, back to `slot`, hold none with ours. A node
    /// behind either slot refuses (`minContextSlot`): one that doesn't know a blockhash yet says «not valid» too. Finalized,
    /// since the index of a wallet's transactions is written after a block is replayed: a confirmed one may not be in it yet.
    public func waitForCosigned(_ signature: String, signer: String, blockhash: String, after slot: UInt64,
                                timeout: TimeInterval = 120) async throws -> (outcome: Outcome, transaction: String?) {
        let deadline = Date().addingTimeInterval(timeout)
        var others: Set<String> = []   // the wallet's transactions already read, without ours
        while Date() < deadline {
            do {
                let valid = try? await call("isBlockhashValid", [blockhash, ["commitment": "finalized", "minContextSlot": slot]]) as? [String: Any]
                let dead = slot > 0 && (valid?["value"] as? Bool) == false ? ((valid?["context"] as? [String: Any])?["slot"] as? NSNumber)?.uint64Value : nil
                guard let list = try await call("getSignaturesForAddress", [signer, ["limit": 100, "commitment": "finalized",
                                                                                    "minContextSlot": dead ?? slot]]) as? [[String: Any]] else {
                    throw WalletError("Solana RPC getSignaturesForAddress: no list")
                }
                var reached = list.count < 100, unread = false   // a short page is the wallet's whole history
                for s in list {
                    guard let id = s["signature"] as? String, let at = (s["slot"] as? NSNumber)?.uint64Value else { unread = true; continue }
                    if at <= slot { reached = true; break }
                    guard !others.contains(id) else { continue }
                    // Read whole or not at all: a reply without its signatures or its outcome may be ours.
                    guard let tx = try await call("getTransaction", [id, ["encoding": "json", "commitment": "finalized",
                                                                          "maxSupportedTransactionVersion": 1]]) as? [String: Any],
                          let meta = tx["meta"] as? [String: Any],
                          let signatures = (tx["transaction"] as? [String: Any])?["signatures"] as? [String] else { unread = true; continue }
                    guard signatures.contains(signature) else { others.insert(id); continue }
                    let err = meta["err"]
                    return (err == nil || err is NSNull ? .confirmed : .failed("\(err!)"), id)
                }
                if dead != nil, reached, !unread { return (.expired, nil) }
            } catch {
                // A node that fails or is behind proves nothing either way: asked again in a moment, until the deadline.
            }
            try await Task.sleep(for: .seconds(2))
        }
        throw WalletError("Transaction status still unknown after \(Int(timeout)) s: signed \(signature)")
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
        let message = wire.subdata(in: (wire.startIndex + messageStart)..<wire.endIndex)

        var m = message.startIndex
        if message[m] & 0x80 != 0 { m += 1 }                    // versioned (v0) prefix
        guard m < message.endIndex else { throw WalletError("Incomplete transaction") }
        let signers = sponsor == nil ? 1 : 2
        guard message[m] == signers, signatureCount == signers else {
            throw WalletError("The transaction needs \(message[m]) signatures, but \(sponsor == nil ? "only ours was" : "the fee payer's and ours were") expected")
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

    /// Solana's compact-u16, read as the network reads it: at most three bytes, at most 65,535, no padded form. Anything
    /// else is refused, so every count is 0…65,535 and no length math after it can stop the app (a site's bytes once did).
    static func readCompact(_ data: Data, _ offset: inout Int) throws -> Int {
        var value = 0
        for shift in [0, 7, 14] {
            guard data.startIndex + offset < data.endIndex else { throw WalletError("Truncated length") }
            let byte = data[data.startIndex + offset]
            offset += 1
            guard byte != 0 || shift == 0 else { throw WalletError("Bad length") }
            value |= Int(byte & 0x7f) << shift
            if byte & 0x80 == 0 {
                guard value <= 0xFFFF else { throw WalletError("Bad length") }
                return value
            }
        }
        throw WalletError("Bad length")
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

extension SolanaTransaction {
    /// The v0 message for `instructions` as they come (their own compute budget included), fee payer first, with accounts
    /// that `tables` (lookup table → its addresses) hold loaded through them — except signers, programs and `keepStatic`
    /// (the wallet's own accounts: the signer lets money land only in an account the message names itself).
    public static func compileV0(_ instructions: [SolanaInstruction], payer: String, blockhash: String,
                                 tables: [(address: String, entries: [String])], keepStatic: Set<String> = []) throws -> Data {
        var flags: [String: (signer: Bool, writable: Bool)] = [payer: (true, true)]
        var order = [payer], programs = Set<String>()
        func add(_ key: String, signer: Bool, writable: Bool) {
            if let f = flags[key] { flags[key] = (f.signer || signer, f.writable || writable) }
            else { flags[key] = (signer, writable); order.append(key) }
        }
        for ix in instructions {
            for a in ix.accounts { add(a.pubkey, signer: a.isSigner, writable: a.isWritable) }
            add(ix.program, signer: false, writable: false)
            programs.insert(ix.program)
        }
        let signers = order.filter { flags[$0]!.signer }
        guard signers == [payer] else { throw WalletError("The instructions need other signers: \(signers.filter { $0 != payer })") }
        // Each loadable account from the first table that has it.
        var loaded: [String: (table: Int, index: Int)] = [:]
        for key in order where key != payer && !programs.contains(key) && !keepStatic.contains(key) {
            for (t, table) in tables.enumerated() {
                if let i = table.entries.firstIndex(of: key), i < 256 { loaded[key] = (t, i); break }
            }
        }
        let statics = order.filter { loaded[$0] == nil }
        let keys = [payer] + statics.filter { $0 != payer && flags[$0]!.writable } + statics.filter { $0 != payer && !flags[$0]!.writable }
        let readonlyUnsigned = keys.filter { !flags[$0]!.writable }.count
        // The full list the instructions index: static keys, then every table's writable entries, then their readonly ones.
        var lookups: [(table: String, writable: [UInt8], readonly: [UInt8], w: [String], r: [String])] = []
        for (t, table) in tables.enumerated() {
            let mine = order.filter { loaded[$0]?.table == t }
            let w = mine.filter { flags[$0]!.writable }, r = mine.filter { !flags[$0]!.writable }
            if !mine.isEmpty { lookups.append((table.address, w.map { UInt8(loaded[$0]!.index) }, r.map { UInt8(loaded[$0]!.index) }, w, r)) }
        }
        let full = keys + lookups.flatMap(\.w) + lookups.flatMap(\.r)
        guard full.count <= 256 else { throw WalletError("The transaction names more than 256 accounts.") }
        let index = Dictionary(uniqueKeysWithValues: full.enumerated().map { ($1, $0) })

        var message = Data([0x80, 1, 0, UInt8(readonlyUnsigned)]) + compact(keys.count)
        for key in keys { message += try Base58.decode(key) }
        message += try Base58.decode(blockhash)
        message += compact(instructions.count)
        for ix in instructions {
            message += Data([UInt8(index[ix.program]!)]) + compact(ix.accounts.count) + Data(ix.accounts.map { UInt8(index[$0.pubkey]!) })
            message += compact(ix.data.count) + ix.data
        }
        message += compact(lookups.count)
        for l in lookups {
            message += try Base58.decode(l.table) + compact(l.writable.count) + Data(l.writable) + compact(l.readonly.count) + Data(l.readonly)
        }
        guard message.count + 65 <= 1232 else { throw WalletError("The transaction is \(message.count + 65) bytes, over Solana's limit of 1232") }
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
            _ = try await node.call("getLatestBlockhash", [["commitment": "finalized"]])
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

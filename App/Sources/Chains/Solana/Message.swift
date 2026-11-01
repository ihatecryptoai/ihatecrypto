import Foundation
import WalletCore

/// A decoded Solana transaction message (legacy or v0), for checking what is about to be signed.
public struct SolanaMessage: Sendable {
    public struct Instruction: Sendable {
        public let programIndex: Int
        public let accounts: [Int]
        public let data: Data
    }

    public struct Lookup: Sendable {
        public let table: String
        public let writable: [Int]
        public let readonly: [Int]
    }

    public let version: Int?
    public let requiredSignatures: Int
    public let staticKeys: [String]
    /// The recent blockhash the message was built on: once past use, the transaction can never land.
    public let blockhash: String
    public let instructions: [Instruction]
    public let lookups: [Lookup]

    /// Parses a full wire transaction (signatures + message).
    public static func parse(transaction wire: Data) throws -> SolanaMessage {
        var o = 0
        let signatures = try SolanaTransaction.readCompact(wire, &o)
        return try parse(message: wire.subdata(in: (wire.startIndex + o + signatures * 64)..<wire.endIndex))
    }

    public static func parse(message m: Data) throws -> SolanaMessage {
        let bytes = [UInt8](m)
        var p = 0
        func need(_ n: Int) throws { guard p + n <= bytes.count else { throw WalletError("Message truncated") } }
        func compact() throws -> Int {
            let data = Data(bytes)
            let value = try SolanaTransaction.readCompact(data, &p)
            return value
        }
        try need(1)
        var version: Int?
        if bytes[0] & 0x80 != 0 { version = Int(bytes[0] & 0x7f); p = 1 }
        try need(3)
        let required = Int(bytes[p]); p += 3
        let keyCount = try compact()
        try need(keyCount * 32 + 32)
        let keys = (0..<keyCount).map { Base58.encode(Data(bytes[(p + $0 * 32)..<(p + $0 * 32 + 32)])) }
        let blockhash = Base58.encode(Data(bytes[(p + keyCount * 32)..<(p + keyCount * 32 + 32)]))
        p += keyCount * 32 + 32 // keys, recent blockhash
        var instructions: [Instruction] = []
        for _ in 0..<(try compact()) {
            try need(1)
            let program = Int(bytes[p]); p += 1
            let n = try compact(); try need(n)
            let accounts = bytes[p..<(p + n)].map(Int.init); p += n
            let len = try compact(); try need(len)
            instructions.append(Instruction(programIndex: program, accounts: accounts, data: Data(bytes[p..<(p + len)])))
            p += len
        }
        var lookups: [Lookup] = []
        if version != nil {
            for _ in 0..<(try compact()) {
                try need(32)
                let table = Base58.encode(Data(bytes[p..<(p + 32)])); p += 32
                let w = try compact(); try need(w); let writable = bytes[p..<(p + w)].map(Int.init); p += w
                let r = try compact(); try need(r); let readonly = bytes[p..<(p + r)].map(Int.init); p += r
                lookups.append(Lookup(table: table, writable: writable, readonly: readonly))
            }
        }
        // Trailing bytes would be signed without being checked.
        guard p == bytes.count else { throw WalletError("Extra bytes after the message") }
        return SolanaMessage(version: version, requiredSignatures: required, staticKeys: keys, blockhash: blockhash, instructions: instructions, lookups: lookups)
    }

    /// Every account the message can touch, in Solana's order: static keys, then all writable
    /// lookup entries, then all readonly ones. `tables` maps a lookup table to its addresses.
    public func accounts(tables: [String: [String]]) throws -> [String] {
        var out = staticKeys
        for pick in [\Lookup.writable, \Lookup.readonly] {
            for lookup in lookups {
                guard let addresses = tables[lookup.table] else { throw WalletError("No address lookup table \(lookup.table)") }
                for i in lookup[keyPath: pick] {
                    guard i < addresses.count else { throw WalletError("Index outside lookup table \(lookup.table)") }
                    out.append(addresses[i])
                }
            }
        }
        return out
    }
}

extension SolanaRPC {
    /// Addresses stored in an address lookup table account (56-byte header, then 32-byte keys).
    public func lookupTable(_ address: String) async throws -> [String] {
        let r = try await call("getAccountInfo", [address, ["encoding": "base64", "commitment": "confirmed"]]) as? [String: Any]
        guard let value = r?["value"] as? [String: Any], let data = (value["data"] as? [Any])?.first as? String,
              let raw = Data(base64Encoded: data), raw.count >= 56 else { throw WalletError("Address lookup table not found: \(address)") }
        return stride(from: 56, to: raw.count - 31, by: 32).map { Base58.encode(raw.subdata(in: $0..<($0 + 32))) }
    }

    /// Full account list of a message, loading its lookup tables from the chain.
    public func resolveAccounts(_ message: SolanaMessage) async throws -> [String] {
        var tables: [String: [String]] = [:]
        for lookup in message.lookups { tables[lookup.table] = try await lookupTable(lookup.table) }
        return try message.accounts(tables: tables)
    }

    /// Simulates and returns the watched accounts' state afterwards: lamports and, for token accounts, raw amount.
    /// `sigVerify: false` simulates a transaction before it is signed.
    /// `slot`: the slot it ran at. It ran on the transaction's own blockhash, so that node knew the blockhash then.
    public func simulate(_ transaction: Data, watch: [String], sigVerify: Bool = true) async throws
        -> (error: String?, after: [String: (lamports: UInt64, token: UInt64?)], slot: UInt64) {
        let r = try await call("simulateTransaction", [transaction.base64EncodedString(), [
            "encoding": "base64", "sigVerify": sigVerify, "commitment": "confirmed",
            "accounts": ["encoding": "jsonParsed", "addresses": watch],
        ]]) as? [String: Any]
        let value = r?["value"] as? [String: Any], slot = ((r?["context"] as? [String: Any])?["slot"] as? NSNumber)?.uint64Value ?? 0
        var after: [String: (lamports: UInt64, token: UInt64?)] = [:]
        for (address, account) in zip(watch, value?["accounts"] as? [Any] ?? []) { after[address] = Self.state(account) ?? (0, nil) }
        guard let err = value?["err"], !(err is NSNull) else { return (nil, after, slot) }
        let text = JSONSerialization.isValidJSONObject(err)
            ? (try? JSONSerialization.data(withJSONObject: err)).map { String(decoding: $0, as: UTF8.self) } ?? "\(err)" : "\(err)"
        let logs = value?["logs"] as? [String] ?? []
        // Short of SOL: said with what to do, not as Solana's code («{"InsufficientFundsForRent":{"account_index":0}}»).
        if Self.shortOfSOL(text, logs: logs), let payer = try? SolanaMessage.parse(transaction: transaction).staticKeys.first {
            let have = (try? await balance(payer)).map { " has \(Decimal($0) / 1_000_000_000) SOL and" } ?? ""
            return ("Not enough SOL: the wallet \(payer.prefix(4))…\(payer.suffix(4))\(have) can't pay Solana's fee and the rent of the account "
                    + "this move opens. Send it ≈ 0.01 SOL, then try again.", after, slot)
        }
        // The program's own words when it logged any, e.g. «Error: insufficient funds».
        let reason = logs.first { $0.contains("Error") }?.replacingOccurrences(of: "Program log: ", with: "")
        return (reason.map { "\($0) \(text)" } ?? text, after, slot)
    }

    /// The watched accounts' state now, in one read and one slot: what a move's balance rules compare the simulation with.
    public func accounts(_ addresses: [String]) async throws -> [String: (lamports: UInt64, token: UInt64?)] {
        let r = try await call("getMultipleAccounts", [addresses, ["encoding": "jsonParsed", "commitment": "confirmed"]]) as? [String: Any]
        return try Self.accounts(addresses, r?["value"])
    }

    /// The node answers null for an account that isn't there yet (a coin's first buy opens it): that holds nothing. Anything
    /// else that isn't the list asked for throws: a made-up 0 before would let a route take everything or pay someone else.
    static func accounts(_ addresses: [String], _ value: Any?) throws -> [String: (lamports: UInt64, token: UInt64?)] {
        guard let list = value as? [Any], list.count == addresses.count else { throw WalletError("Solana RPC getMultipleAccounts: no account list") }
        return try Dictionary(zip(addresses, list).map { address, account in
            if account is NSNull { return (address, (0, 0)) }
            guard let state = state(account) else { throw WalletError("Solana RPC getMultipleAccounts: \(address) isn't an account") }
            // A plain system account holds no tokens: a wallet, or a coin account's address someone sent SOL to before it
            // was opened (the Associated Token program opens a funded address). A mint or another program's account stays nil.
            let system = (account as? [String: Any])?["owner"] as? String == "11111111111111111111111111111111"
            return (address, (state.lamports, state.token ?? (system ? 0 : nil)))
        }, uniquingKeysWith: { first, _ in first })
    }

    /// What each watched balance held before a move: token units for a token rule, lamports for the others. A token rule
    /// on an account with no token amount (a mint, another program's account) throws: its balance can't be checked.
    public static func before(_ watch: [(address: String, label: String, token: Bool)],
                              _ now: [String: (lamports: UInt64, token: UInt64?)]) throws -> [String: Int64] {
        try Dictionary(watch.map { w in
            guard let held = (w.token ? now[w.address]?.token : now[w.address]?.lamports).flatMap(Int64.init(exactly:)) else {
                throw WalletError("\(w.label) isn't a token account, so its balance can't be checked.")
            }
            return (w.address, held)
        }, uniquingKeysWith: { _, last in last })
    }

    /// One account in jsonParsed: its lamports and, for a token account, the raw amount; nil when it isn't an account.
    static func state(_ account: Any) -> (lamports: UInt64, token: UInt64?)? {
        guard let a = account as? [String: Any], let lamports = (a["lamports"] as? NSNumber)?.uint64Value else { return nil }
        let amount = (((a["data"] as? [String: Any])?["parsed"] as? [String: Any])?["info"] as? [String: Any])
            .flatMap { ($0["tokenAmount"] as? [String: Any])?["amount"] as? String }.flatMap(UInt64.init)
        return (lamports, amount)
    }

    /// The fee payer can't cover the fee or a new account's rent: it holds no SOL at all (AccountNotFound), too little for
    /// the fee, would drop under its own rent, or the System Program couldn't fund an account from it.
    static func shortOfSOL(_ error: String, logs: [String]) -> Bool {
        error.contains("AccountNotFound") || error.contains("InsufficientFundsForFee")
            || error.contains(#"{"InsufficientFundsForRent":{"account_index":0}}"#)
            || logs.contains { $0.contains("Transfer: insufficient lamports") }
    }
}

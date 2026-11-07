import CryptoKit
import Foundation
@testable import JupiterSwap
@testable import SolanaChain
import Testing
import WalletCore

@Test func solTransferIsSignedLegacyMessageWithPriorityFee() throws {
    let key = Data(repeating: 7, count: 32)
    let recipient = Base58.encode(Data(repeating: 9, count: 32))
    let blockhash = Base58.encode(Data(repeating: 1, count: 32))
    let (signature, wire) = try SolanaTransaction.transfer(from: key, to: recipient, lamports: 1_000_000, blockhash: blockhash)

    #expect(wire[0] == 1)                                          // one signature
    let sig = wire.subdata(in: 1..<65), message = wire.subdata(in: 65..<wire.count)
    #expect(Base58.encode(sig) == signature)
    let pub = try Curve25519.Signing.PrivateKey(rawRepresentation: key).publicKey
    #expect(pub.isValidSignature(sig, for: message))
    #expect(Array(message.prefix(4)) == [1, 0, 2, 4])               // header, then 4 account keys
    #expect(message.subdata(in: 4..<36) == pub.rawRepresentation)   // fee payer first
    #expect(message.suffix(8) == Data([0x40, 0x42, 0x0f, 0, 0, 0, 0, 0])) // 1 000 000 lamports, little-endian
}

@Test func signingSomeoneElsesTransactionRequiresUsAsFeePayer() throws {
    let key = Data(repeating: 7, count: 32)
    let (_, ours) = try SolanaTransaction.transfer(from: key, to: Base58.encode(Data(repeating: 9, count: 32)),
                                                   lamports: 1, blockhash: Base58.encode(Data(repeating: 1, count: 32)))
    let unsigned = Data([1]) + Data(count: 64) + ours.subdata(in: 65..<ours.count)
    let (_, signed) = try SolanaTransaction.sign(unsigned, privateKey: key)
    // CryptoKit's ed25519 signatures are randomized, so compare meaning, not bytes.
    let message = ours.subdata(in: 65..<ours.count)
    #expect(signed.subdata(in: 65..<signed.count) == message)
    #expect(try Curve25519.Signing.PrivateKey(rawRepresentation: key).publicKey.isValidSignature(signed.subdata(in: 1..<65), for: message))
    #expect(throws: WalletError.self) { try SolanaTransaction.sign(unsigned, privateKey: Data(repeating: 8, count: 32)) }
}

@Test func base58RoundTrips() throws {
    for bytes in [Data(), Data([0, 0, 1]), Data(repeating: 255, count: 32)] {
        #expect(try Base58.decode(Base58.encode(bytes)) == bytes)
    }
    #expect(try Base58.decode("ComputeBudget111111111111111111111111111111").count == 32)
}

// Real accounts read from mainnet RPC: a wallet (curve point) and its associated USDC account (off curve).
// The wallet also has 13 other, non-associated USDC accounts; only this one is derived.
private let binanceWallet = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM"
private let binanceUSDCAccount = "FGETo8T8wMcN2wCjav8VK6eh3dLk63evNDPxzLSJra8B" // its associated USDC account (holds the balance)
private let usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

@Test func curveCheckSeparatesWalletsFromProgramAddresses() throws {
    #expect(Ed25519.isOnCurve(try Base58.decode(binanceWallet)))
    #expect(Ed25519.isOnCurve(try Base58.decode("2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj")))
    #expect(!Ed25519.isOnCurve(try Base58.decode(binanceUSDCAccount)))
    // Every freshly generated public key is a curve point.
    for _ in 0..<20 {
        #expect(Ed25519.isOnCurve(Curve25519.Signing.PrivateKey().publicKey.rawRepresentation))
    }
}

@Test func fieldArithmeticIdentities() {
    let a = Ed25519.Field(hex: "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
    let b = Ed25519.Field(hex: "0fedcba09876543210fedcba09876543210fedcba09876543210fedcba098765")
    #expect((a + b) - b == a)
    #expect(a * Ed25519.Field.one == a)
    #expect(a * b == b * a)
    // Fermat: a^(p−1) = 1
    #expect(a.power(Ed25519.Field.pMinus2) * a == Ed25519.Field.one)
}

@Test func associatedTokenAccountMatchesMainnet() throws {
    #expect(try SPL.associatedAccount(owner: binanceWallet, mint: usdcMint) == binanceUSDCAccount)
}

@Test func compiledInstructionsPutThePayerFirstAndMergeFlags() throws {
    let key = Data(repeating: 7, count: 32)
    let me = Base58.encode(try Curve25519.Signing.PrivateKey(rawRepresentation: key).publicKey.rawRepresentation)
    let vault = Base58.encode(Data(repeating: 3, count: 32)), config = Base58.encode(Data(repeating: 4, count: 32))
    let program = Base58.encode(Data(repeating: 5, count: 32))
    // Relay-style: we appear twice (signer+writable, then readonly); flags must merge into one account.
    let ix = SolanaInstruction(program: program, accounts: [
        .init(pubkey: config, isSigner: false, isWritable: false), .init(pubkey: me, isSigner: true, isWritable: true),
        .init(pubkey: me, isSigner: false, isWritable: false), .init(pubkey: vault, isSigner: false, isWritable: true),
    ], data: Data([1, 2, 3]))
    let (signature, wire) = try SolanaTransaction.compile([ix], privateKey: key, blockhash: Base58.encode(Data(repeating: 1, count: 32)))
    let m = try SolanaMessage.parse(transaction: wire)
    #expect(m.staticKeys.first == me && m.requiredSignatures == 1)
    #expect(Set(m.staticKeys).count == m.staticKeys.count)            // no duplicates
    #expect(m.staticKeys[1] == vault)                                // writable before readonly
    #expect(m.instructions.count == 2)                                // priority fee + ours
    #expect(m.instructions[1].accounts.map { m.staticKeys[$0] } == [config, me, me, vault])
    #expect(signature == Base58.encode(wire.subdata(in: 1..<65)))
    // Someone else's signature requirement is refused.
    let foreign = SolanaInstruction(program: program, accounts: [.init(pubkey: vault, isSigner: true, isWritable: false)], data: Data())
    #expect(throws: WalletError.self) { try SolanaTransaction.compile([foreign], privateKey: key, blockhash: Base58.encode(Data(repeating: 1, count: 32))) }
}

@Test func parseRejectsBytesAfterTheMessage() throws {
    let wire = try SolanaTransaction.transfer(from: Data(repeating: 7, count: 32), to: Base58.encode(Data(repeating: 9, count: 32)),
                                              lamports: 1, blockhash: Base58.encode(Data(repeating: 1, count: 32))).wire
    _ = try SolanaMessage.parse(transaction: wire)
    #expect(throws: WalletError.self) { try SolanaMessage.parse(transaction: wire + Data([0])) }
}

/// A v1 transaction (SIMD-0385, on mainnet since Oct 2026): the message first — 0x81, the header, the config mask, the
/// blockhash, the counts, the addresses — and the signatures last. Read as legacy, 0x81 0x01 was 129 signatures and the
/// range past them ran backwards: a site's signTransaction stopped the app (2026-11-05).
private let v1Message = Data([0x81, 1, 0, 1]) + Data(count: 4) + Data(repeating: 1, count: 32) + Data([0, 2])
    + Data(repeating: 7, count: 32) + Data(repeating: 9, count: 32)

@Test func parseRefusesV1() throws {
    let refused = #expect(throws: WalletError.self) { try SolanaMessage.parse(transaction: v1Message + Data(count: 64)) }
    #expect(refused?.message.contains("v1") == true && refused?.message.contains("Nothing was signed") == true, "\(String(describing: refused))")
    // The same message inside the old envelope (a count, a signature, the message) is v1 too, never read as v0.
    #expect(#expect(throws: WalletError.self) { try SolanaMessage.parse(message: v1Message) }?.message.contains("v1") == true)
    #expect(#expect(throws: WalletError.self) { try SolanaMessage.parse(transaction: Data([1]) + Data(count: 64) + v1Message) }?.message.contains("v1") == true)
    #expect(throws: WalletError.self) { try SolanaTransaction.unsigned(v1Message + Data(count: 64), payer: Data(repeating: 7, count: 32)) }
    // Any other first byte of 128 or more is no transaction at all: never called «v0», which is read.
    #expect(#expect(throws: WalletError.self) { try SolanaMessage.parse(transaction: Data([0x80, 1, 0, 0])) }?.message.contains("can read") == true)
}

/// Whatever bytes a site sends, the parsers answer or throw: the app and the signer never stop on them. Random buffers,
/// a real transfer with bytes changed or cut, and lengths made to overflow.
@Test func parseNeverTraps() throws {
    struct SplitMix: RandomNumberGenerator {   // seeded, so a failure repeats
        var state: UInt64
        mutating func next() -> UInt64 {
            state &+= 0x9E37_79B9_7F4A_7C15
            var z = state
            z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
            z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
            return z ^ (z >> 31)
        }
    }
    var random = SplitMix(state: 2026_11_05)
    let payer = Data(repeating: 7, count: 32)
    let transfer = try SolanaTransaction.transfer(from: payer, to: Base58.encode(Data(repeating: 9, count: 32)),
                                                  lamports: 1, blockhash: Base58.encode(Data(repeating: 1, count: 32))).wire
    var buffers: [Data] = [Data(repeating: 0x80, count: 9) + Data([0x01]), Data([0xff, 0xff, 0xff, 0x7f]),
                           Data([0x01]) + Data(count: 64) + Data([0x80]), Data([0x80]), Data(), v1Message]
    for _ in 0..<2_000 {
        buffers.append(Data((0..<Int.random(in: 0...600, using: &random)).map { _ in UInt8.random(in: 0...255, using: &random) }))
        var changed = transfer
        for _ in 0..<Int.random(in: 1...3, using: &random) {
            changed[Int.random(in: 0..<changed.count, using: &random)] = UInt8.random(in: 0...255, using: &random)
        }
        buffers.append(changed.prefix(Int.random(in: 0...changed.count, using: &random)))
    }
    for whole in buffers {
      for wire in [whole, (Data([0]) + whole).dropFirst()] {   // a slice too: its indices start at 1
        _ = try? SolanaMessage.parse(transaction: wire)
        _ = try? SolanaMessage.parse(message: wire)
        _ = try? SolanaTransaction.unsigned(wire, payer: payer)
      }
    }
    // A transaction read from a slice is the same transaction.
    #expect(try SolanaMessage.parse(transaction: (Data([0]) + transfer).dropFirst()).staticKeys == SolanaMessage.parse(transaction: transfer).staticKeys)
    let us = try Curve25519.Signing.PrivateKey(rawRepresentation: payer).publicKey.rawRepresentation
    let whole = try SolanaTransaction.unsigned(transfer, payer: us), sliced = try SolanaTransaction.unsigned((Data([0]) + transfer).dropFirst(), payer: us)
    #expect(sliced.message == whole.message && sliced.slot.lowerBound == whole.slot.lowerBound + 1)
}

/// Solana's compact-u16, as the network reads it: at most three bytes, at most 65,535, no padded form.
@Test func compactIsU16() throws {
    func read(_ bytes: [UInt8]) throws -> Int { var o = 0; return try SolanaTransaction.readCompact(Data(bytes), &o) }
    #expect(try read([0x7f]) == 127)
    #expect(try read([0x80, 0x01]) == 128)
    #expect(try read([0xff, 0xff, 0x03]) == 65_535)
    for bad: [UInt8] in [[0xff, 0xff, 0x04], [0x80, 0x80, 0x80, 0x01], [0x80, 0x00], [0xff, 0xff, 0xff, 0x7f]] {
        #expect(throws: WalletError.self, "\(bad)") { try read(bad) }
    }
}

/// Token-2022 accounts derive from that program: real holders of POKÉMON CARDS (a Token-2022 mint) from a mainnet
/// transaction, 2026-09-29.
@Test func token2022AccountsDeriveFromTheirProgram() throws {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6"
    let holders = [("6LXutJvKUw8Q5ue2gCgKHQdAN4suWW8awzFVC6XCguFx", "AKfyuSUkLjnHb4KadX6SxfAi6eGePBJqQSZCsCBk2jnc"),
                   ("GNqZFdcipNFo5nr8LnK7YVsMy84458BWHwnE1RX5dNpC", "G3zG4N9MfWTfrCBxBYNcTXsKmsYLNp4GfcUmxagndT9t"),
                   ("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL", "7QnWse8CQFg6hP5uBKii1gndpMxastHFUKn4TjaFwfcM")]
    let matches = try holders.filter { owner, account in try SPL.associatedAccount(owner: owner, mint: mint, program: SPL.token2022Program) == account }
    #expect(!matches.isEmpty)
    for (owner, account) in matches { #expect(try SPL.associatedAccount(owner: owner, mint: mint) != account) }
}

/// The automation wallet's daily spread failed as «{"InsufficientFundsForRent":{"account_index":0}}» with 0.00198 SOL
/// (2026-10-06): read as short of SOL, so the error says to send some. Another account's rent or a slippage error isn't.
@Test func shortOfSOLReadsTheFeePayersErrors() {
    #expect(SolanaRPC.shortOfSOL(#"{"InsufficientFundsForRent":{"account_index":0}}"#, logs: []))
    #expect(SolanaRPC.shortOfSOL("AccountNotFound", logs: []))
    #expect(SolanaRPC.shortOfSOL("InsufficientFundsForFee", logs: []))
    #expect(SolanaRPC.shortOfSOL(#"{"InstructionError":[2,{"Custom":1}]}"#, logs: ["Transfer: insufficient lamports 1977880, need 2039280"]))
    #expect(!SolanaRPC.shortOfSOL(#"{"InsufficientFundsForRent":{"account_index":3}}"#, logs: []))
    #expect(!SolanaRPC.shortOfSOL(#"{"InstructionError":[3,{"Custom":6001}]}"#, logs: ["Program log: Error: slippage tolerance exceeded"]))
}

/// Jupiter's price impact and the swap's USD worth, as each API says them: v1's quote in strings, v2's order in numbers
/// with the impact negative (2026-10-28, 2M WIF for USDC).
@Test func jupiterSaysImpactAndWorthEitherWay() {
    let v1 = Jupiter.impactAndWorth(["priceImpactPct": "0.1235947568", "swapUsdValue": "320500.59"])
    #expect(v1.impact == Decimal(string: "0.1235947568") && v1.usd == Decimal(string: "320500.59"))
    let v2 = Jupiter.impactAndWorth(["priceImpactPct": "-0.0103491778", "swapUsdValue": NSNumber(value: 361914.39)])
    #expect(v2.impact == Decimal(string: "0.0103491778") && v2.usd > 361_914 && v2.usd < 361_915)
    #expect(Jupiter.impactAndWorth([:]) == (0, 0))
}

/// Balances before a move are read in one getMultipleAccounts (2026-11-01). The node answers null for an account that
/// isn't there yet (a coin's first buy opens it): that holds nothing. Real shapes from mainnet, USDC and an Ondo stock.
private func tokenAccount(_ program: String, amount: String, lamports: UInt64) -> [String: Any] {
    ["lamports": NSNumber(value: lamports), "owner": SPL.tokenProgram,
     "data": ["program": program, "space": 165, "parsed": ["type": "account", "info": ["tokenAmount": ["amount": amount, "decimals": 6]]]]]
}

@Test func aMissingAccountHoldsNothingBeforeAMoveOpensIt() throws {
    let now = try SolanaRPC.accounts(["A"], [NSNull()])
    #expect(now["A"]?.lamports == 0 && now["A"]?.token == 0)
}

@Test func aTokenAccountsAmountIsReadFromTheParsedData() throws {
    for program in ["spl-token", "spl-token-2022"] {
        let now = try SolanaRPC.accounts(["A"], [tokenAccount(program, amount: "2335542", lamports: 2_039_280)])
        #expect(now["A"]?.lamports == 2_039_280 && now["A"]?.token == 2_335_542)
    }
}

/// A plain system account comes back as base64 even when jsonParsed is asked, and holds no tokens: a wallet, or a coin
/// account's address someone sent SOL to before the buy opens it (that buy isn't stopped). A mint has no token amount,
/// so a token rule on it stops the move.
@Test func aSystemAccountHoldsNoTokensAndAMintIsNotATokenAccount() throws {
    let funded: [String: Any] = ["lamports": NSNumber(value: 890_880), "owner": "11111111111111111111111111111111", "data": ["", "base64"]]
    let mint: [String: Any] = ["lamports": NSNumber(value: 1_461_600), "owner": SPL.tokenProgram,
                               "data": ["program": "spl-token", "parsed": ["type": "mint", "info": ["decimals": 6, "supply": "1000"]]]]
    let now = try SolanaRPC.accounts(["F", "M"], [funded, mint])
    #expect(now["F"]?.lamports == 890_880 && now["F"]?.token == 0)
    #expect(now["M"]?.lamports == 1_461_600 && now["M"]?.token == nil)
}

/// Each rule takes its own kind of balance before; one the read didn't give is a stop, never a 0 (2026-11-01).
@Test func eachRuleReadsItsOwnBalanceBefore() throws {
    let now: [String: (lamports: UInt64, token: UInt64?)] = ["new": (0, 0), "usdc": (2_039_280, 5_000_000), "wallet": (41_000_000, 0), "mint": (1_461_600, nil)]
    let before = try SolanaRPC.before([("new", "JUP", true), ("usdc", "USDC", true), ("wallet", "lamports", false)], now)
    #expect(before == ["new": 0, "usdc": 5_000_000, "wallet": 41_000_000])
    #expect(throws: WalletError.self) { try SolanaRPC.before([("mint", "USDC", true)], now) }
    #expect(throws: WalletError.self) { try SolanaRPC.before([("absent", "USDC", true)], now) }
    #expect(throws: WalletError.self) { try SolanaRPC.before([("absent", "lamports", false)], now) }
    #expect(throws: WalletError.self) { try SolanaRPC.before([("big", "X", true)], ["big": (0, UInt64.max)]) }
}

/// An answer that isn't the list asked for is an error, never «0 everywhere», which would switch the balance rules off.
@Test func anAnswerWithoutTheAccountListThrows() {
    #expect(throws: WalletError.self) { try SolanaRPC.accounts(["A"], nil) }
    #expect(throws: WalletError.self) { try SolanaRPC.accounts(["A"], ["value": [NSNull()]]) }
    #expect(throws: WalletError.self) { try SolanaRPC.accounts(["A", "B"], [NSNull()]) }
    #expect(throws: WalletError.self) { try SolanaRPC.accounts(["A"], ["not an account"]) }
    #expect(throws: WalletError.self) { try SolanaRPC.accounts(["A"], [["data": ["", "base64"]]]) }
}

/// Before and after are read by the same code: the simulation's accounts give what its old loop gave.
@Test func beforeAndAfterUseOneReader() {
    let usdc = SolanaRPC.state(tokenAccount("spl-token", amount: "1000000", lamports: 2_039_280))
    #expect(usdc?.lamports == 2_039_280 && usdc?.token == 1_000_000)
    let wallet = SolanaRPC.state(["lamports": NSNumber(value: 5000), "data": ["", "base64"]] as [String: Any])
    #expect(wallet?.lamports == 5000 && wallet?.token == nil)
    #expect(SolanaRPC.state(NSNull()) == nil)
}

/// A Solana node at https://<name>.solana.stub for the waits below: block height 1001 at slot 110, one past lastValid 1000, and
/// the signature's statuses in turn, the last one repeated. Each test has its own name, so they run side by side.
private final class ScriptedRPC: URLProtocol {
    static let lock = NSLock()
    nonisolated(unsafe) static var statuses: [String: [String]] = [:]
    static let installed = URLProtocol.registerClass(ScriptedRPC.self)

    static func node(_ name: String, statuses: [String]) -> SolanaRPC {
        _ = installed
        lock.withLock { self.statuses[name] = statuses }
        return SolanaRPC(url: URL(string: "https://\(name).solana.stub")!)
    }

    static func nextStatus(_ name: String) -> String {
        lock.withLock {
            guard let queue = statuses[name], let first = queue.first else { return "null" }
            if queue.count > 1 { statuses[name]!.removeFirst() }
            return first
        }
    }

    override class func canInit(with request: URLRequest) -> Bool { request.url?.host?.hasSuffix(".solana.stub") == true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}
    override func startLoading() {
        var body = Data()
        if let stream = request.httpBodyStream {   // URLProtocol gets the body as a stream, not httpBody
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 { body.append(buffer, count: n) }
            stream.close()
        }
        let name = String(request.url!.host!.dropLast(".solana.stub".count))
        let result = switch ((try? JSONSerialization.jsonObject(with: body)) as? [String: Any])?["method"] as? String {
        case "getSignatureStatuses": Self.nextStatus(name)
        case "getEpochInfo": #"{"absoluteSlot":110,"blockHeight":1001,"epoch":1,"slotIndex":110,"slotsInEpoch":432000}"#
        case "getBlockHeight": "1001"
        default: "null"
        }
        client!.urlProtocol(self, didReceive: HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!,
                            cacheStoragePolicy: .notAllowed)
        client!.urlProtocol(self, didLoad: Data(#"{"jsonrpc":"2.0","id":1,"result":\#(result)}"#.utf8))
        client!.urlProtocolDidFinishLoading(self)
    }
}

/// getSignatureStatuses as a node at `slot` answers it: nothing, or the transaction at `level`.
private func status(at slot: Int, _ level: String? = nil) -> String {
    let value = level.map { #"{"slot":105,"confirmations":null,"err":null,"confirmationStatus":"\#($0)"}"# } ?? "null"
    return #"{"context":{"slot":\#(slot)},"value":[\#(value)]}"#
}

/// The status is read before the block height: a transaction that lands between the two reads is found by reading the
/// status again, never called «expired» (which the chat says is safe to retry: a second swap, send or bridge).
@Test func expiryRereadsTheStatusBeforeSayingExpired() async throws {
    let rpc = ScriptedRPC.node("reread", statuses: [status(at: 100), status(at: 111, "confirmed")])
    #expect(try await rpc.waitForConfirmation("sig", lastValidBlockHeight: 1000) == .confirmed)
}

/// A node behind the block that proved the expiry (another in the public pool, the public node after the user's own) may
/// not have the transaction yet: its «nothing» isn't believed, the wait goes on.
@Test func aStatusNodeBehindTheExpiryIsNotBelieved() async throws {
    let rpc = ScriptedRPC.node("behind", statuses: [status(at: 100), status(at: 105), status(at: 112, "confirmed")])
    #expect(try await rpc.waitForConfirmation("sig", lastValidBlockHeight: 1000) == .confirmed)
}

/// A node that never catches up ends «status unknown», which the chat doesn't offer to retry — never «expired».
@Test func aNodeStuckBehindEndsUnknownNotExpired() async {
    let rpc = ScriptedRPC.node("stuck", statuses: [status(at: 100), status(at: 105)])
    let error = await #expect(throws: WalletError.self) { try await rpc.waitForConfirmation("sig", lastValidBlockHeight: 1000, timeout: 1) }
    #expect(error?.message.contains("still unknown") == true)
}

/// A node past the proving block that still has nothing: then it's sure, «expired» as before.
@Test func expiredWhenANodePastTheExpiryStillHasNothing() async throws {
    let rpc = ScriptedRPC.node("gone", statuses: [status(at: 100), status(at: 111)])
    #expect(try await rpc.waitForConfirmation("sig", lastValidBlockHeight: 1000) == .expired)
}

/// Live (IHATECRYPTO_NET=1, reads only): a signature that never was, past its last valid block on mainnet, ends «expired»
/// — the real getEpochInfo and getSignatureStatuses' context.slot read as the stub above answers them.
@Test(.enabled(if: ProcessInfo.processInfo.environment["IHATECRYPTO_NET"] == "1"))
func waitSaysExpiredOnMainnetForASignatureThatNeverWas() async throws {
    let rpc = SolanaRPC(url: SolanaNodes.publicNode)
    let epoch = try await rpc.call("getEpochInfo", [["commitment": "finalized"]]) as? [String: Any]
    let height = try #require((epoch?["blockHeight"] as? NSNumber)?.uint64Value)
    let signature = Base58.encode(Data((0..<64).map { _ in UInt8.random(in: 0...255) }))
    let started = Date()
    #expect(try await rpc.waitForConfirmation(signature, lastValidBlockHeight: height - 1, timeout: 30) == .expired)
    #expect(Date().timeIntervalSince(started) < 10)
}

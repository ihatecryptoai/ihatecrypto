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

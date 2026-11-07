import CryptoKit
import Foundation
import WalletCore

/// SPL tokens: the address of a token account (ATA) and a checked transfer.
public enum SPL {
    public static let tokenProgram = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
    public static let associatedTokenProgram = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
    public static let token2022Program = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"

    /// The associated token account of `owner` for `mint`: a program-derived address, off the ed25519 curve. A Token-2022
    /// mint's accounts derive from that program.
    public static func associatedAccount(owner: String, mint: String, program: String = tokenProgram) throws -> String {
        let seeds = [try Base58.decode(owner), try Base58.decode(program), try Base58.decode(mint)]
        return Base58.encode(try programAddress(seeds: seeds, program: try Base58.decode(associatedTokenProgram)))
    }

    /// Solana's find_program_address: the first bump from 255 down whose hash is not a curve point.
    public static func programAddress(seeds: [Data], program: Data) throws -> Data {
        for bump in (0...255).reversed() {
            var input = Data()
            for seed in seeds { input += seed }
            input += Data([UInt8(bump)]) + program + Data("ProgramDerivedAddress".utf8)
            let hash = Data(SHA256.hash(data: input))
            if !Ed25519.isOnCurve(hash) { return hash }
        }
        throw WalletError("No program address found")
    }

    /// Creates our own token account for `mint` if it does not exist yet (idempotent).
    public static func createOwnAccount(privateKey: Data, mint: String, blockhash: String,
                                        microLamportsPerCU: UInt64 = 20_000) throws -> (signature: String, wire: Data) {
        let signer = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let payer = signer.publicKey.rawRepresentation
        let keys = [
            payer,                                                                                   // 0 signer, writable
            try Base58.decode(try associatedAccount(owner: Base58.encode(payer), mint: mint)),       // 1 new account
            try Base58.decode(mint),                                                                 // 2 readonly
            SolanaTransaction.systemProgram,                                                         // 3
            try Base58.decode(tokenProgram),                                                         // 4
            try Base58.decode(associatedTokenProgram),                                               // 5
            SolanaTransaction.computeBudgetProgram,                                                  // 6
        ]
        typealias T = SolanaTransaction
        var message = Data([1, 0, 5]) + T.compact(keys.count)
        for key in keys { message += key }
        message += try Base58.decode(blockhash)
        message += T.compact(2)
        message += Data([6]) + T.compact(0) + T.compact(9) + Data([3]) + T.le(microLamportsPerCU)
        message += Data([5]) + T.compact(6) + Data([0, 1, 0, 2, 3, 4]) + T.compact(1) + Data([1])
        let signature = try signer.signature(for: message)
        return (Base58.encode(signature), T.compact(1) + signature + message)
    }

    /// Creates the recipient's token account if missing (idempotent), then TransferChecked, with a priority fee.
    public static func transfer(from privateKey: Data, to owner: String, mint: String, amount: UInt64, decimals: UInt8,
                                blockhash: String, microLamportsPerCU: UInt64 = 20_000) throws -> (signature: String, wire: Data) {
        let signer = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey)
        let message = try transferMessage(payer: Base58.encode(signer.publicKey.rawRepresentation), to: owner, mint: mint, amount: amount,
                                          decimals: decimals, blockhash: blockhash, microLamportsPerCU: microLamportsPerCU)
        let signature = try signer.signature(for: message)
        return (Base58.encode(signature), SolanaTransaction.compact(1) + signature + message)
    }

    /// The same transfer as an unsigned message, for a signer that holds the key elsewhere. `program`: the mint's own,
    /// SPL Token or Token-2022 — both accounts and the TransferChecked derive from it.
    /// `close`: our token account closes after the transfer (all of it went), its rent back to the wallet.
    public static func transferMessage(payer payerAddress: String, to owner: String, mint: String, amount: UInt64, decimals: UInt8,
                                       blockhash: String, microLamportsPerCU: UInt64 = 20_000, program: String = tokenProgram,
                                       close: Bool = false) throws -> Data {
        let payer = try Base58.decode(payerAddress)
        guard owner != payerAddress else { throw WalletError("Transfer to yourself") }
        let keys = [
            payer,                                                                 // 0 signer, writable
            try Base58.decode(try associatedAccount(owner: payerAddress, mint: mint, program: program)), // 1 our token account
            try Base58.decode(try associatedAccount(owner: owner, mint: mint, program: program)),   // 2 their token account
            try Base58.decode(owner),                                              // 3 readonly from here on
            try Base58.decode(mint),                                               // 4
            SolanaTransaction.systemProgram,                                       // 5
            try Base58.decode(program),                                            // 6
            try Base58.decode(associatedTokenProgram),                             // 7
            SolanaTransaction.computeBudgetProgram,                                // 8
        ]
        typealias T = SolanaTransaction
        var message = Data([1, 0, 6]) + T.compact(keys.count)
        for key in keys { message += key }
        message += try Base58.decode(blockhash)
        message += T.compact(close ? 4 : 3)
        message += Data([8]) + T.compact(0) + T.compact(9) + Data([3]) + T.le(microLamportsPerCU)
        message += Data([7]) + T.compact(6) + Data([0, 2, 3, 4, 5, 6]) + T.compact(1) + Data([1])        // CreateIdempotent
        message += Data([6]) + T.compact(4) + Data([1, 4, 2, 0]) + T.compact(10) + Data([12]) + T.le(amount) + Data([decimals])
        if close { message += Data([6]) + T.compact(3) + Data([1, 0, 0]) + T.compact(1) + Data([9]) }   // CloseAccount(ours, → wallet, owner)
        return message
    }
}

/// Whether 32 bytes decode to a point on edwards25519, the way curve25519-dalek's decompress decides:
/// x² = (y² − 1)/(d·y² + 1) must be a square mod p = 2²⁵⁵ − 19.
public enum Ed25519 {
    public static func isOnCurve(_ bytes: Data) -> Bool {
        var le = [UInt8](bytes)
        le[31] &= 0x7f                                   // the top bit is x's sign, not part of y
        let y = Field(littleEndian: le)
        let y2 = y * y
        let u = y2 - Field.one
        let v = Field.d * y2 + Field.one
        let w = u * v.power(Field.pMinus2)               // u / v
        let legendre = w.power(Field.halfPMinus1)        // 0 or 1 for squares, p − 1 otherwise
        return legendre == Field.zero || legendre == Field.one
    }

    /// Field element mod p = 2²⁵⁵ − 19 in four little-endian 64-bit limbs, always fully reduced.
    struct Field: Equatable {
        var l: [UInt64]

        static let p: [UInt64] = [0xffff_ffff_ffff_ffed, 0xffff_ffff_ffff_ffff, 0xffff_ffff_ffff_ffff, 0x7fff_ffff_ffff_ffff]
        static let zero = Field(limbs: [0, 0, 0, 0])
        static let one = Field(limbs: [1, 0, 0, 0])
        /// d = −121665 / 121666
        static let d = Field(hex: "52036cee2b6ffe738cc740797779e89800700a4d4141d8ab75eb4dca135978a3")
        static let pMinus2: [UInt64] = [0xffff_ffff_ffff_ffeb, 0xffff_ffff_ffff_ffff, 0xffff_ffff_ffff_ffff, 0x7fff_ffff_ffff_ffff]
        static let halfPMinus1: [UInt64] = [0xffff_ffff_ffff_fff6, 0xffff_ffff_ffff_ffff, 0xffff_ffff_ffff_ffff, 0x3fff_ffff_ffff_ffff]

        init(limbs: [UInt64]) { l = limbs; reduceOnce(); reduceOnce() }

        init(littleEndian b: [UInt8]) {
            var limbs = [UInt64](repeating: 0, count: 4)
            for i in 0..<32 { limbs[i / 8] |= UInt64(b[i]) << (8 * UInt64(i % 8)) }
            self.init(limbs: limbs)
        }

        init(hex: String) {
            self.init(littleEndian: Array(Data(hex: hex)!.reversed()))
        }

        /// Subtract p while ≥ p (value < 2²⁵⁶ needs at most two).
        mutating func reduceOnce() {
            var geq = true
            for i in (0..<4).reversed() where l[i] != Field.p[i] { geq = l[i] > Field.p[i]; break }
            guard geq else { return }
            var borrow: UInt64 = 0
            for i in 0..<4 {
                let (a, o1) = l[i].subtractingReportingOverflow(Field.p[i])
                let (b, o2) = a.subtractingReportingOverflow(borrow)
                l[i] = b
                borrow = (o1 || o2) ? 1 : 0
            }
        }

        static func + (a: Field, b: Field) -> Field {
            var out = [UInt64](repeating: 0, count: 4)
            var carry: UInt64 = 0
            for i in 0..<4 {
                let (s1, o1) = a.l[i].addingReportingOverflow(b.l[i])
                let (s2, o2) = s1.addingReportingOverflow(carry)
                out[i] = s2
                carry = (o1 || o2) ? 1 : 0
            }
            // a, b < p < 2²⁵⁵, so the sum fits in 256 bits.
            return Field(limbs: out)
        }

        static func - (a: Field, b: Field) -> Field {
            // a − b = a + (p − b)
            var negB = [UInt64](repeating: 0, count: 4)
            var borrow: UInt64 = 0
            for i in 0..<4 {
                let (s1, o1) = p[i].subtractingReportingOverflow(b.l[i])
                let (s2, o2) = s1.subtractingReportingOverflow(borrow)
                negB[i] = s2
                borrow = (o1 || o2) ? 1 : 0
            }
            return a + Field(limbs: negB)
        }

        static func * (a: Field, b: Field) -> Field {
            // 512-bit schoolbook product in eight limbs.
            var t = [UInt64](repeating: 0, count: 8)
            for i in 0..<4 {
                var carry: UInt64 = 0
                for j in 0..<4 {
                    let (hi, lo) = a.l[i].multipliedFullWidth(by: b.l[j])
                    let (s1, o1) = t[i + j].addingReportingOverflow(lo)
                    let (s2, o2) = s1.addingReportingOverflow(carry)
                    t[i + j] = s2
                    carry = hi &+ (o1 ? 1 : 0) &+ (o2 ? 1 : 0)
                }
                t[i + 4] = carry
            }
            // 2²⁵⁶ ≡ 38 (mod p): fold the high half in, twice.
            var r = [UInt64](repeating: 0, count: 5)
            var carry: UInt64 = 0
            for i in 0..<4 {
                let (hi, lo) = t[i + 4].multipliedFullWidth(by: 38)
                let (s1, o1) = t[i].addingReportingOverflow(lo)
                let (s2, o2) = s1.addingReportingOverflow(carry)
                r[i] = s2
                carry = hi &+ (o1 ? 1 : 0) &+ (o2 ? 1 : 0)
            }
            r[4] = carry
            let (hi, lo) = r[4].multipliedFullWidth(by: 38)
            precondition(hi == 0)
            var out = Array(r[0..<4])
            var c = lo
            for i in 0..<4 where c != 0 {
                let (s, o) = out[i].addingReportingOverflow(c)
                out[i] = s
                c = o ? 1 : 0
            }
            if c != 0 { // wrapped past 2²⁵⁶ once more: add 38 again
                let (s, _) = out[0].addingReportingOverflow(38)
                out[0] = s
            }
            return Field(limbs: out)
        }

        func power(_ e: [UInt64]) -> Field {
            var result = Field.one
            for limb in e.reversed() {
                for bit in (0..<64).reversed() {
                    result = result * result
                    if limb >> UInt64(bit) & 1 == 1 { result = result * self }
                }
            }
            return result
        }
    }
}

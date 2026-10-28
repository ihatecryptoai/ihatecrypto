import Foundation
import HyperliquidSigning
import SolanaChain
import WalletCore

/// The last check before signing. Plain code, not a model: whatever a web page, token metadata or a
/// prompt injection made the planner or an API return, only programs on the list run, and money goes only
/// to the user's own accounts or to addresses bound to a quote the user approved.
public struct Verdict: Sendable {
    /// What was verified, in words for the confirmation screen.
    public var checks: [String] = []
    /// Any entry means: do not sign.
    public var problems: [String] = []
    public var ok: Bool { problems.isEmpty }

    public init() {}

    public mutating func merge(_ other: Verdict) {
        checks += other.checks
        problems += other.problems
    }
}

// MARK: - Solana

public struct SolanaPolicy: Sendable {
    public let owner: String
    /// Addresses money may go to besides the owner: a quote's deposit address and nothing else by default.
    public var recipients: Set<String>
    public var programs: [String: String]
    /// Most the transaction may pay in priority fees, lamports.
    public var maxPriorityFee: UInt64
    /// False where lookup-table contents come from someone the check cannot verify (the signer has no network):
    /// then money may not go to an account that only a lookup table names.
    public var trustLookupTables: Bool
    /// Someone else who pays the fee and signs first (Jupiter's gasless swap: its wallet pays for a wallet with no SOL).
    /// Our wallet is then the second and last signer; what it signs for is checked the same. SOL may go to someone else
    /// only out of the sponsor's own account — or back to the sponsor, up to `sponsorRefund`: the rent it fronted for the
    /// swap's temporary account, returned to us when that closes. The simulation checks what we get in the end.
    public var sponsor: String?
    public static let sponsorRefund: UInt64 = 5_000_000

    public init(owner: String, recipients: Set<String> = [], programs: [String: String] = SolanaGuard.knownPrograms,
                maxPriorityFee: UInt64 = 5_000_000, trustLookupTables: Bool = true, sponsor: String? = nil) {
        self.owner = owner
        self.recipients = recipients
        self.programs = programs
        self.maxPriorityFee = maxPriorityFee
        self.trustLookupTables = trustLookupTables
        self.sponsor = sponsor
    }
}

public enum SolanaGuard {
    public static let system = "11111111111111111111111111111111"
    public static let computeBudget = "ComputeBudget111111111111111111111111111111"
    public static let jupiterSwap = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"
    public static let jupiterLend = "jup3YeL8QhtSx1e253b2FDvsMNC87fDrgQZivbrndc9"
    /// Jupiter's on-chain DCA (Recurring v1): open_dca_v2 and close_dca on the user's own order account.
    public static let jupiterDCA = "DCA265Vj8a9CEuX1eb1LWRnDT7uK6q1xMipnNyatn23M"
    /// JupiterZ, Jupiter's market makers (its order engine, jup-ag/rfq-webhook-toolkit): one instruction, `fill`, where the
    /// maker pays the fee, signs first and swaps with the taker at its quoted price.
    public static let jupiterZ = "61DFfeTKM7trxYcPQCM78bJ794ddZprZpAwAnLiwTpYH"
    /// fill(input_amount, output_amount, expire_at): sha256("global:fill")[0..<8]; its accounts taker 0, maker 1, the taker's
    /// input 2 and output 4 accounts (the program's own id for none: SOL then comes as lamports), input mint 6, output mint 8.
    static let jupiterZFill = "a860b7a35c0a28a0"
    static let wrappedSOL = "So11111111111111111111111111111111111111112"
    /// Relay Depository (github.com/relayprotocol/relay-depository), Anchor program.
    public static let relayDepository = "99vQwtBwYtrqqD9YSXbdum3KBdxPAVxYTaQ3cfnJSrN2"
    public static let kaminoLend = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD"
    /// Kamino's vaults (Earn) and the farm their shares are staked in: a vault withdrawal unstakes, then redeems.
    public static let kaminoVault = "KvauGMspG5k6rtzrqqn7WNn3oZdyKqLKwK2XWQ8FLjd"
    public static let kaminoFarms = "FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr"
    /// Kamino's main lending market, the only one ihatecrypto lends on (Kamino.mainMarket).
    public static let kaminoMainMarket = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF"

    /// The Kamino instructions ihatecrypto's deposits and withdrawals use whose programs pay out only to the signer
    /// (token::authority = owner, has_one = owner; read in the verified source, klend e921c64, kvault 2dadf94, kfarms
    /// bfa1860, 2026-10-23), by 8-byte discriminator. Any other — a deposit minting to someone else's account, repaying
    /// someone else's debt, a flash loan, handing a farm stake over (transfer_ownership), topping up a vault's rewards —
    /// is refused, whatever the app says. KLend's value is the index of the obligation the instruction acts on (nil: none).
    static let kaminoLendAllowed: [String: Int?] = [
        "75a9b045c5170fa2": nil,                                                  // init_user_metadata (no referrer, below)
        "fb0ae74c1b0b9f60": 2, "883f0fbad398a8a4": 2,                             // init_obligation, init_obligation_farms_for_reserve
        "02da8aeb4fc91966": nil, "218493e497c04859": 1, "8c90fd150a4af803": 1,    // refresh_reserve, refresh_obligation, refresh_obligation_farms_for_reserve
        "2477fb8122f00793": 1,                                                    // request_elevation_group
        "81c70402de271a2e": 1, "d8e0bf1bcc9766af": 1,                             // deposit_reserve_liquidity_and_obligation_collateral(_v2)
        "4b5d5ddc2296dac4": 1, "eb34779895c51407": 1,                             // withdraw_obligation_collateral_and_redeem_reserve_collateral(_v2)
        "2574cd67f3c05cc6": 1, "caf97572e7c02f8a": 1,                             // withdraw_obligation_collateral(_v2)
        "6cd1044815167685": 1, "8991975ea7710491": 1,                             // deposit_obligation_collateral(_v2)
        "797f12cc49f5e141": 1, "a1808ff5abc7c206": 1,                             // borrow_obligation_liquidity(_v2)
        "ea75b57db98edc1d": nil,                                                  // redeem_reserve_collateral
    ]
    /// Vault withdrawals (withdraw, sell, withdraw_from_available, redeem_in_kind): to the signer's own accounts. No deposits.
    static let kaminoVaultAllowed: Set<String> = ["b712469c946da122", "33e685a4017f83ad", "1383709baadc2239", "663abdfcc0db8c59"]
    /// Farms: unstake, withdraw_unstaked_deposits, harvest_reward (to the owner), refresh_farm, refresh_user_state, initialize_user.
    static let kaminoFarmsAllowed: Set<String> = ["5a5f6b2acd7c32e1", "2466bb31dc248443", "44c8e4e9b820e2bc", "d6838ab790c2ac2a", "01870c3ef38c4d6c",
                                                  "6f11b9fa3c7a26fe"]

    /// Whether the transaction sends this wallet's SOL to the fee payer (rent it fronted, coming back): only then is the
    /// payer someone the wallet pays, to be allowed as such. A JupiterZ maker isn't: the swap's prompt names it.
    public static func refundsSponsor(_ m: SolanaMessage, accounts: [String], owner: String, sponsor: String) -> Bool {
        m.instructions.contains { ix in
            ix.programIndex < accounts.count && accounts[ix.programIndex] == system && ix.data.count >= 12 && le32(ix.data, 0) == 2
                && ix.accounts.count > 1 && ix.accounts[0] < accounts.count && ix.accounts[1] < accounts.count
                && accounts[ix.accounts[0]] == owner && accounts[ix.accounts[1]] == sponsor
        }
    }

    /// The Relay order ids the transaction's depository deposits name (bytes 16..<48 of each one's data).
    public static func relayDepositIDs(_ m: SolanaMessage) -> [Data] {
        m.instructions.compactMap { ix in
            guard ix.programIndex < m.staticKeys.count, m.staticKeys[ix.programIndex] == relayDepository, ix.data.count >= 48 else { return nil }
            return Data(ix.data.dropFirst(16).prefix(32))
        }
    }

    /// Jupiter's swap forms (its IDL, 2026-10-24) by discriminator: the account the output lands in, the one that replaces
    /// it when set, and the output mint. Plain and token-ledger routes, exact-out, their shared-accounts kinds and the v2
    /// forms the gasless /order builds.
    static let jupiterRoutes: [String: (to: Int, instead: Int?, mint: Int)] = [
        "e517cb977ae3ad2a": (3, 4, 5), "96564774a75d0e68": (3, 4, 5), "d033ef977b2bed5c": (3, 4, 6),   // route, route_with_token_ledger, exact_out_route
        "c1209b3341d69c81": (6, nil, 8), "e6798f50779f6aaa": (6, nil, 8), "b0d169a89a7d453e": (6, nil, 8), // shared_accounts_ the same three
        "bb64facc31c4af14": (2, 7, 4), "9d8ab85215f4f324": (2, 7, 4),                                   // route_v2, exact_out_route_v2
        "d19853937cfed8e9": (5, nil, 7), "3560e5cad8bbfa18": (5, nil, 7),                               // shared_accounts_route_v2, …exact_out_route_v2
    ]

    /// Jupiter Lend Earn (Fluid's lending program): its deposits mint fTokens to account 2, the signer's own account for
    /// the fToken mint (account 6); its withdrawals and redemptions pay account 2, the signer's own account for the token
    /// (account 5). The program requires both today (token::authority = signer, Instadapp/fluid-solana-programs
    /// context.rs); the signer checks them itself, since Jupiter can upgrade it. By discriminator: deposit,
    /// deposit_with_min_amount_out, mint, mint_with_max_assets / withdraw, redeem, withdraw_with_max_shares_burn,
    /// redeem_with_min_amount_out.
    static let jupiterLendDeposits: Set<String> = ["f223c68952e1f2b6", "74901061766d2877", "3339e12fb69289a6", "065e457a1eb392ab"]
    static let jupiterLendWithdrawals: Set<String> = ["b712469c946da122", "b80c569546c461e1", "2fc5b7abef12f5ab", "ebbded38a6b4b895"]
    /// Jupiter's recurring orders (DCA): open_dca_v2 and close_dca only, read by their accounts (its IDL; no source is
    /// public). Measured on mainnet 2026-10-23: the program refuses another payee, and the signer checks it as well.
    static let jupiterDCAOpen = "8e772b6da2340bb1", jupiterDCAClose = "16072162a8b722f3"

    /// The owner's plain obligation on Kamino's main market: seeds [tag 0, id 0, owner, market, default, default] (klend
    /// handler_init_obligation.rs). An instruction on it acts on that market (has_one = lending_market), whatever market
    /// a lookup table names: the signer can't read those, so this pins the market from a static account instead.
    /// `account` is `owner`'s associated token account for `mint`, under SPL Token or Token-2022.
    static func ownTokenAccount(_ account: String?, owner: String, mint: String?) -> Bool {
        guard let account, let mint else { return false }
        return [SPL.tokenProgram, SPL.token2022Program].contains { (try? SPL.associatedAccount(owner: owner, mint: mint, program: $0)) == account }
    }

    public static func kaminoObligation(owner: String) -> String? {
        guard let o = try? Base58.decode(owner), let m = try? Base58.decode(kaminoMainMarket), let p = try? Base58.decode(kaminoLend),
              let pda = try? SPL.programAddress(seeds: [Data([0]), Data([0]), o, m, Data(count: 32), Data(count: 32)], program: p) else { return nil }
        return Base58.encode(pda)
    }
    /// Kamino's first deposit creates the user's own lookup table; only create and extend with the owner as authority pass.
    public static let lookupTableProgram = "AddressLookupTab1e1111111111111111111111111"
    public static let knownPrograms: [String: String] = [
        system: "System", computeBudget: "Compute Budget", SPL.tokenProgram: "SPL Token", SPL.token2022Program: "SPL Token-2022",
        SPL.associatedTokenProgram: "Associated Token", jupiterSwap: "Jupiter Swap", jupiterLend: "Jupiter Lend Earn", jupiterDCA: "Jupiter DCA", jupiterZ: "JupiterZ",
        relayDepository: "Relay Depository", kaminoLend: "Kamino Lend", kaminoVault: "Kamino Vault", kaminoFarms: "Kamino Farms",
        lookupTableProgram: "Address Lookup Table",
    ]
    /// Anchor discriminators: sha256("global:deposit_token")[0..<8], sha256("global:deposit_native")[0..<8].
    static let relayDepositToken = Data([0x0b, 0x9c, 0x60, 0xda, 0x27, 0xa3, 0xb4, 0x13])
    static let relayDepositNative = Data([0x0d, 0x9e, 0x0d, 0xdf, 0x5f, 0xd5, 0x1c, 0x06])
    /// Jupiter's route and shared_accounts_route, the two its swap API builds for an exact-in swap (read 2026-10-21 from
    /// lite-api's /swap): sha256("global:route")[0..<8], sha256("global:shared_accounts_route")[0..<8].
    /// What the Touch ID prompt names a swap's output in; another mint shows its raw units.
    static let mintNames: [String: (symbol: String, decimals: Int)] = [
        "So11111111111111111111111111111111111111112": ("SOL", 9),
        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v": ("USDC", 6),
        "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB": ("USDT", 6),
    ]

    /// `accounts`: the message's full account list (static keys plus lookup tables).
    /// `tokenOwners`: token account → its owner wallet, for the owner's and the allowed recipients' accounts.
    public static func check(_ m: SolanaMessage, accounts: [String], policy: SolanaPolicy, tokenOwners: [String: String]) -> Verdict {
        var v = Verdict()
        // An entry a lookup table names is whatever the app says it is: the signer can't read tables. Unless it trusts the
        // app with them, such an entry is never taken for any address — a lying app could name this wallet there while
        // the table on chain holds someone else's, and a payee checked by its text would pass (the 2026-10-24 review).
        let accounts = policy.trustLookupTables || accounts.count <= m.staticKeys.count ? accounts
            : m.staticKeys + (m.staticKeys.count..<accounts.count).map { "lookup-table entry \($0 - m.staticKeys.count + 1)" }
        if let sponsor = policy.sponsor {
            guard sponsor != policy.owner, m.requiredSignatures == 2, accounts.count > 1, accounts[0] == sponsor, accounts[1] == policy.owner else {
                v.problems.append("Only \(sponsor.prefix(6))… may pay the fee, and your wallet must be the only other signer.")
                return v
            }
            v.checks.append("\(sponsor.prefix(6))… pays the network fee; your wallet is the only other signer.")
        } else {
            guard m.requiredSignatures == 1, accounts.first == policy.owner else {
                v.problems.append("Only your wallet may sign and pay for the transaction.")
                return v
            }
            v.checks.append("Only your wallet signs and pays the fee.")
        }

        func mine(_ address: String) -> Bool {
            address == policy.owner || tokenOwners[address] == policy.owner
        }
        let staticKeys = Set(m.staticKeys)
        func allowed(_ address: String) -> Bool {
            guard policy.trustLookupTables || staticKeys.contains(address) else { return false }
            return mine(address) || policy.recipients.contains(address) || tokenOwners[address].map(policy.recipients.contains) == true
        }
        func name(_ address: String) -> String {
            if address == policy.owner { return "your wallet" }
            if tokenOwners[address] == policy.owner { return "your token account" }
            if policy.recipients.contains(address) || tokenOwners[address].map(policy.recipients.contains) == true {
                return "approved address \(address.prefix(6))…"
            }
            return address
        }

        var unitLimit: UInt64 = 200_000
        var unitPrice: UInt64 = 0
        var refunded: UInt64 = 0
        var fills = 0
        for (n, ix) in m.instructions.enumerated() {
            guard ix.programIndex < m.staticKeys.count else {
                v.problems.append("Instruction \(n + 1): the program is loaded from a lookup table.")
                continue
            }
            let program = accounts[ix.programIndex]
            guard let programName = policy.programs[program] else {
                v.problems.append("Instruction \(n + 1): program \(program) is not on the allowed list.")
                continue
            }
            func account(_ i: Int) -> String? { i < ix.accounts.count && ix.accounts[i] < accounts.count ? accounts[ix.accounts[i]] : nil }
            let tag = ix.data.first

            switch program {
            case computeBudget:
                if tag == 2, ix.data.count >= 5 { unitLimit = UInt64(le32(ix.data, 1)) }
                if tag == 3, ix.data.count >= 9 { unitPrice = le64(ix.data, 1) }
            case system:
                guard ix.data.count >= 12, le32(ix.data, 0) == 2, let to = account(1) else {
                    v.problems.append("Instruction \(n + 1): System is allowed only for SOL transfers.")
                    continue
                }
                let lamports = le64(ix.data, 4)
                if let sponsor = policy.sponsor, account(0) == sponsor {
                    v.checks.append("\(Double(lamports) / 1e9) SOL from the fee payer's own account.")
                } else if let sponsor = policy.sponsor, to == sponsor, account(0) == policy.owner, policy.trustLookupTables || staticKeys.contains(to) {
                    refunded += lamports
                    if refunded <= SolanaPolicy.sponsorRefund {
                        v.checks.append("\(Double(lamports) / 1e9) SOL back to the fee payer for the account it opened; the simulation checks what you get.")
                    } else {
                        v.problems.append("\(Double(refunded) / 1e9) SOL back to the fee payer is more than the rent it fronted.")
                    }
                } else if allowed(to) {
                    v.checks.append("\(Double(lamports) / 1e9) SOL → \(name(to)).")
                } else {
                    v.problems.append("SOL goes to someone else's address \(to).")
                }
            // Token-2022 shares the classic program's instructions and their layout; its extensions' own instructions
            // (a transfer with a fee, confidential transfers…) are refused below like approvals.
            case SPL.tokenProgram, SPL.token2022Program:
                switch tag {
                case 3, 12: // Transfer(source, dest, authority), TransferChecked(source, mint, dest, authority)
                    guard let to = account(tag == 3 ? 1 : 2), ix.data.count >= (tag == 3 ? 9 : 10) else { v.problems.append("Instruction \(n + 1): incomplete token transfer."); continue }
                    // The amount the instruction moves, for the prompt: by its mint where the signer knows it, else raw units.
                    let units = le64(ix.data, 1)
                    let known = tag == 12 ? account(1).flatMap { mintNames[$0] } : nil
                    let amount = known.map { "\(Decimal(units) / pow(10, $0.decimals)) \($0.symbol)" } ?? "\(units) units of a token"
                    if allowed(to) { v.checks.append("\(amount) → \(name(to)).") } else { v.problems.append("\(amount) go to someone else's account \(to).") }
                case 9: // CloseAccount(account, destination, owner): the rent must come back to us
                    if let to = account(1), mine(to), policy.trustLookupTables || staticKeys.contains(to) { v.checks.append("Temporary account closes, remainder → \(name(to)).") }
                    else { v.problems.append("Closing the account sends the remainder to someone else's address.") }
                case 17: v.checks.append("wSOL sync.")
                default:
                    // Approve, SetAuthority and the rest hand control of tokens to someone else.
                    v.problems.append("Instruction \(n + 1): token operation \(tag.map(String.init) ?? "?") is forbidden (approvals, owner changes and the like).")
                }
            case SPL.associatedTokenProgram:
                guard let owner = account(2) else { v.problems.append("Instruction \(n + 1): incomplete account creation."); continue }
                if owner == policy.owner || (policy.recipients.contains(owner) && (policy.trustLookupTables || staticKeys.contains(owner))) { v.checks.append("Creates a token account owned by \(name(owner)).") }
                else { v.problems.append("Creates a token account for someone else's wallet \(owner).") }
            case lookupTableProgram:
                // 0 Create(table, authority, payer, system), 2 Extend(table, authority, payer, system): our table only.
                guard tag == 0 || tag == 2, let authority = account(1), authority == policy.owner else {
                    v.problems.append("Instruction \(n + 1): the lookup table is not yours, or the operation is not create/extend.")
                    continue
                }
                v.checks.append(tag == 0 ? "Creates your lookup table (rent in SOL)." : "Extends your lookup table.")
            case relayDepository:
                // Only deposits, with our wallet as the depositor; the amount goes into the envelope check too.
                // deposit_token/deposit_native(amount, id): data 8 + 8 + 32, accounts depository, sender, depositor, …
                // The depositor (account 2, a static key) is this wallet: a recovered deposit is refunded to it. Whom the
                // order pays is checked by the signer from the order itself (RelayOrder, relayDepositIDs).
                let kind = ix.data.prefix(8)
                guard kind == relayDepositToken || kind == relayDepositNative, ix.data.count >= 48 else {
                    v.problems.append("Instruction \(n + 1): only deposits are allowed on Relay.")
                    continue
                }
                if account(2) == policy.owner, account(1) == policy.owner {
                    v.checks.append("Relay deposit: \(le64(ix.data, 8)) units from your wallet, for the order the signer checked.")
                } else {
                    v.problems.append("Relay deposit is not from your wallet, or names someone else as the depositor.")
                }
            case jupiterLend:
                let kind = ix.data.prefix(8).hex, deposit = jupiterLendDeposits.contains(kind)
                guard deposit || jupiterLendWithdrawals.contains(kind) else {
                    v.problems.append("Instruction \(n + 1): a Jupiter Lend instruction ihatecrypto doesn't use (\(kind)).")
                    continue
                }
                guard account(0) == policy.owner, let to = account(2), policy.trustLookupTables || staticKeys.contains(to),
                      ownTokenAccount(to, owner: policy.owner, mint: account(deposit ? 6 : 5)) else {
                    v.problems.append("Instruction \(n + 1): Jupiter Lend would \(deposit ? "credit the deposit to" : "pay") someone else's account.")
                    continue
                }
                v.checks.append("Jupiter Lend \(deposit ? "deposit: the shares" : "withdrawal: the money") → your account. The balance simulation checks the amount.")
            case jupiterDCA:
                let kind = ix.data.prefix(8).hex
                let mine = kind == jupiterDCAOpen
                    // dca 0, user 1, payer 2, inputMint 3, outputMint 4, userAta 5, inAta 6, outAta 7: the order's own ATAs.
                    ? account(1) == policy.owner && account(2) == policy.owner && ownTokenAccount(account(5), owner: policy.owner, mint: account(3))
                        && account(0).map { ownTokenAccount(account(6), owner: $0, mint: account(3)) && ownTokenAccount(account(7), owner: $0, mint: account(4)) } == true
                    // user 0, dca 1, inputMint 2, outputMint 3, …, userInAta 6, userOutAta 7: what's left comes back here.
                    : kind == jupiterDCAClose && account(0) == policy.owner && ownTokenAccount(account(6), owner: policy.owner, mint: account(2))
                        && ownTokenAccount(account(7), owner: policy.owner, mint: account(3))
                        && [6, 7].allSatisfy { i in policy.trustLookupTables || account(i).map(staticKeys.contains) == true }
                guard mine else {
                    v.problems.append("Instruction \(n + 1): a Jupiter DCA instruction that isn't your own order's opening or closing (\(kind)).")
                    continue
                }
                v.checks.append(kind == jupiterDCAOpen ? "Jupiter DCA order of yours: what it buys is yours." : "Closes your Jupiter DCA order: what's left comes back to you.")
            case kaminoLend, kaminoVault, kaminoFarms:
                let kind = ix.data.prefix(8).hex
                if program == kaminoVault ? !kaminoVaultAllowed.contains(kind) : program == kaminoFarms ? !kaminoFarmsAllowed.contains(kind)
                    : kaminoLendAllowed[kind] == nil {
                    v.problems.append("Instruction \(n + 1): a \(programName) instruction ihatecrypto doesn't use (\(kind)); it could pay someone else.")
                    continue
                }
                if program == kaminoLend, case .some(.some(let at)) = kaminoLendAllowed[kind] {
                    guard let obligation = account(at), staticKeys.contains(obligation), obligation == kaminoObligation(owner: policy.owner) else {
                        v.problems.append("Instruction \(n + 1): the Kamino position isn't your own on Kamino's main market.")
                        continue
                    }
                }
                // init_user_metadata's referrer (account 3) would take a share of the fees: none, which Anchor writes as the program.
                if kind == "75a9b045c5170fa2", account(3) != kaminoLend {
                    v.problems.append("Instruction \(n + 1): Kamino's account setup names a referrer.")
                    continue
                }
                v.checks.append("Calls \(programName): an instruction that pays only you. The balance simulation checks the amount.")
            case jupiterZ:
                // One fill, this wallet the taker: what it gives comes from its own account for the input mint (or, with none
                // named — the program's id — as SOL), and what it gets lands in its own account for the output mint (or as
                // SOL). Another fill, or one drawing on a different account of the wallet, could hand the maker a token the
                // app's simulation doesn't watch (the 2026-10-28 review). The amounts go on the prompt; the simulation checks them.
                fills += 1
                guard fills == 1, ix.data.prefix(8).hex == jupiterZFill, ix.data.count >= 24, account(0) == policy.owner,
                      let source = account(2), let inMint = account(6), let out = account(4), let mint = account(8) else {
                    v.problems.append("Instruction \(n + 1): a JupiterZ instruction that isn't the one fill with your wallet as the taker.")
                    continue
                }
                func own(_ a: String, _ m: String) -> Bool {
                    (a == jupiterZ && m == wrappedSOL) || (mine(a) || ownTokenAccount(a, owner: policy.owner, mint: m)) && (policy.trustLookupTables || staticKeys.contains(a))
                }
                guard own(source, inMint) else { v.problems.append("The JupiterZ fill takes from \(source), not your account for what you swap."); continue }
                guard own(out, mint) else { v.problems.append("The JupiterZ fill pays \(out), not your wallet."); continue }
                func units(_ n: UInt64, _ m: String) -> String { mintNames[m].map { "\(Decimal(n) / pow(10, $0.decimals)) \($0.symbol)" } ?? "\(n) units of \(m.prefix(6))…" }
                v.checks.append("JupiterZ fill: \(units(le64(ix.data, 8), inMint)) for \(units(le64(ix.data, 16), mint)) → \(out == jupiterZ ? "your wallet, as SOL" : "your account"). The balance simulation checks it.")
            case jupiterSwap:
                // Every swap form's output account (its «instead» one when set: the program's own id when not) must be this
                // wallet's own; any other Jupiter instruction is refused (the 2026-10-24 review: the gasless path's route_v2
                // went unchecked). The amounts aren't read: they end the data, and the program ignores bytes after them,
                // so a site could append a made-up quote and slippage (the 2026-10-22 review). The simulation checks what arrives.
                guard let layout = jupiterRoutes[ix.data.prefix(8).hex], var to = account(layout.to), let mint = account(layout.mint) else {
                    v.problems.append("Instruction \(n + 1): a Jupiter instruction that isn't one of its swaps (\(ix.data.prefix(8).hex)).")
                    continue
                }
                if let instead = layout.instead.flatMap(account), instead != jupiterSwap { to = instead }
                // This wallet's own token account for the mint, by the claim the signer proved or derived here.
                let ours = mine(to) || [SPL.tokenProgram, SPL.token2022Program].contains { (try? SPL.associatedAccount(owner: policy.owner, mint: mint, program: $0)) == to }
                guard ours, policy.trustLookupTables || staticKeys.contains(to) else {
                    v.problems.append("The Jupiter swap pays \(to), not your wallet.")
                    continue
                }
                v.checks.append("Jupiter swap → your account (\(mintNames[mint]?.symbol ?? "\(mint.prefix(6))…")). The balance simulation checks the amount.")
            default:
                v.checks.append("Calls \(programName). The balance simulation checks the result.")
            }
        }
        // Both numbers are the transaction's: a product past 64 bits is refused, not trapped on.
        let (product, overflow) = unitLimit.multipliedReportingOverflow(by: unitPrice)
        let fee = product / 1_000_000
        if overflow || fee > policy.maxPriorityFee { v.problems.append(overflow ? "Priority fee out of range." : "Priority fee up to \(Double(fee) / 1e9) SOL is too high.") }
        return v
    }

    public struct BalanceRule: Sendable {
        public let label: String
        public let before: Int64
        public let after: Int64
        /// The change must be at least this (negative: at most this much may leave).
        public let atLeast: Int64

        public init(label: String, before: Int64, after: Int64, atLeast: Int64) {
            self.label = label
            self.before = before
            self.after = after
            self.atLeast = atLeast
        }
    }

    /// Compares simulated balances with what the user approved: this catches a route that pays someone else.
    public static func balances(_ rules: [BalanceRule]) -> Verdict {
        var v = Verdict()
        for r in rules {
            let delta = r.after - r.before
            if delta >= r.atLeast { v.checks.append("Simulation: \(r.label) \(delta >= 0 ? "+" : "")\(delta).") }
            else { v.problems.append("Simulation: \(r.label) changes by \(delta); the minimum allowed is \(r.atLeast).") }
        }
        return v
    }

    static func le16(_ d: Data, _ at: Int) -> UInt16 { UInt16(d[d.startIndex + at]) | UInt16(d[d.startIndex + at + 1]) << 8 }
    static func le32(_ d: Data, _ at: Int) -> UInt32 { (0..<4).reduce(0) { $0 | UInt32(d[d.startIndex + at + $1]) << (8 * $1) } }
    static func le64(_ d: Data, _ at: Int) -> UInt64 { (0..<8).reduce(0) { $0 | UInt64(d[d.startIndex + at + $1]) << (8 * $1) } }
}

// MARK: - Hyperliquid

public enum HLGuard {
    /// Hyperliquid's own market-making vault; the only vault money may go to without the user naming it.
    public static let hlpVault = "0xdfc24b077bc1425ad1dea75bcb6f8158e10df303"
    /// The top of Settings › Trading's leverage stepper (SettingsView, `p.maxLeverage < 20`).
    public static let maxLeverage: UInt64 = 20

    /// Orders and cancels trade inside the account. Anything that moves money must go to an approved address.
    public static func check(_ action: MsgPack, owner: String, recipients: Set<String>, agent: String? = nil) -> Verdict {
        var v = Verdict()
        guard case .map(let pairs) = action else { v.problems.append("Unrecognized action."); return v }
        func text(_ key: String) -> String? {
            if case .string(let s)? = pairs.first(where: { $0.0 == key })?.1 { return s }
            return nil
        }
        let allowed = Set(recipients.map { $0.lowercased() } + [owner.lowercased()])
        switch text("type") {
        case "updateLeverage":
            // The trading key signs this without Touch ID: the signer holds it to the most the risk rules can be set to.
            guard case .uint(let leverage)? = pairs.first(where: { $0.0 == "leverage" })?.1, leverage >= 1, leverage <= maxLeverage else {
                v.problems.append("Leverage above \(maxLeverage)×: the most ihatecrypto's risk rules allow.")
                return v
            }
            v.checks.append("Leverage \(leverage)× inside your account.")
        case "order":
            // What each order does, for a prompt when the main key signs it: side, size, limit price, market (by index).
            if case .array(let orders)? = pairs.first(where: { $0.0 == "orders" })?.1 {
                for case .map(let o) in orders {
                    func field(_ k: String) -> MsgPack? { o.first(where: { $0.0 == k })?.1 }
                    let side = field("b") == .bool(true) ? "buy" : "sell"
                    let size = if case .string(let s)? = field("s") { s } else { "?" }, price = if case .string(let p)? = field("p") { p } else { "?" }
                    let market = if case .uint(let a)? = field("a") { "market #\(a)" } else { "a market" }
                    v.checks.append("\(side) \(size) on \(market) at \(price)\(field("r") == .bool(true) ? ", only reducing a position" : "") → your account")
                }
            }
            // The trading key signs orders without Touch ID: an order's fee may go to ihatecrypto, at its rate, and nowhere else.
            guard let builder = pairs.first(where: { $0.0 == "builder" })?.1 else { v.checks.append("Trading action inside your account."); break }
            guard case .map(let b) = builder, b.count == 2, b.first(where: { $0.0 == "b" })?.1 == .string(HLBuilder.address),
                  case .uint(let f)? = b.first(where: { $0.0 == "f" })?.1, f <= HLBuilder.fee else {
                v.problems.append("The order pays a fee to someone other than ihatecrypto, or more than \(HLBuilder.rate).")
                return v
            }
            v.checks.append("Trading action inside your account; ihatecrypto's fee \(HLBuilder.rate).")
        case "cancel":
            v.checks.append("Trading action inside your account.")
        case "setReferrer":
            if text("code") == HLBuilder.referralCode { v.checks.append("Referral code \(HLBuilder.referralCode): 4% off your Hyperliquid fees.") }
            else { v.problems.append("Referral code \(text("code") ?? "?") is not ihatecrypto's.") }
        case "approveBuilderFee":
            let ours = text("builder")?.lowercased() == HLBuilder.address
            switch text("maxFeeRate") {
            case HLBuilder.rate? where ours: v.checks.append("ihatecrypto's fee \(HLBuilder.rate) of each Hyperliquid trade → \(HLBuilder.address.prefix(6))…\(HLBuilder.address.suffix(4)).")
            case HLBuilder.revoke? where ours: v.checks.append("Takes back the approval of ihatecrypto's fee.")
            default: v.problems.append("Approves a fee of \(text("maxFeeRate") ?? "?") for \(text("builder") ?? "?"): only ihatecrypto's \(HLBuilder.rate) is allowed.")
            }
        case "usdClassTransfer":
            v.checks.append("Transfer between your spot and perp balances.")
        case "spotSend", "withdraw3", "usdSend", "sendAsset":
            let to = text("destination")?.lowercased() ?? ""
            // What moves, for the prompt: the amount and, where it says, the token (a spot token is «NAME:0x…»).
            let token = text("token").map { String($0.split(separator: ":").first ?? "") } ?? "USDC"
            let amount = "\(text("amount") ?? "?") \(token)"
            if allowed.contains(to) { v.checks.append("\(amount) → \(to.prefix(6))…\(to.suffix(4)).") }
            else { v.problems.append("\(amount) go to someone else's address \(to).") }
        case "vaultTransfer":
            let vault = text("vaultAddress")?.lowercased() ?? ""
            // usd: micro-dollars (6 decimals); isDeposit: into the vault, else out of it.
            let deposit = pairs.first(where: { $0.0 == "isDeposit" })?.1 == .bool(true)
            let usd: Decimal = if case .uint(let u)? = pairs.first(where: { $0.0 == "usd" })?.1 { Decimal(u) / 1_000_000 } else { 0 }
            let named = vault == hlpVault ? "the HLP vault" : "the vault \(vault.prefix(6))…\(vault.suffix(4))"
            if vault == hlpVault || allowed.contains(vault) {
                v.checks.append(deposit ? "$\(usd) → \(named) (locked for days)." : "$\(usd) out of \(named) → your account.")
            } else { v.problems.append("Transfer to an unknown vault \(vault).") }
        case "approveAgent":
            if let agent, text("agentAddress")?.lowercased() == agent.lowercased() { v.checks.append("Approves your agent key.") }
            else { v.problems.append("Approves an unknown agent \(text("agentAddress") ?? "?").") }
        case let other:
            v.problems.append("Action \(other ?? "?") is not allowed.")
        }
        return v
    }
}

// MARK: - Quote bindings (1Click, Relay)

/// Every field a quote binds (recipient, refund address, assets, amount) must be what the user approved.
public enum BindingGuard {
    public static func check(_ bound: [String: String], expected: [String: String], subject: String) -> Verdict {
        var v = Verdict()
        for (field, value) in expected.sorted(by: { $0.key < $1.key }) where bound[field]?.lowercased() != value.lowercased() {
            v.problems.append("\(subject): \(field) = \(bound[field] ?? "none") instead of \(value).")
        }
        if v.ok { v.checks.append("\(subject): recipient, sender and amount are yours.") }
        return v
    }
}

// MARK: - 1Click

public enum DepositGuard {
    /// A deposit address is safe to pay only if 1Click has it bound to a quote with our recipient and refund
    /// address, the assets we asked for and our amount. A substituted address fails this.
    public static func check(bound: [String: String], recipient: String, refundTo: String, origin: String, destination: String, amount: String) -> Verdict {
        var v = Verdict()
        let expected = ["recipient": recipient, "refundTo": refundTo, "originAsset": origin, "destinationAsset": destination, "amount": amount]
        for (field, value) in expected.sorted(by: { $0.key < $1.key }) {
            if bound[field]?.lowercased() == value.lowercased() { continue }
            v.problems.append("The deposit address is bound to a different \(field): \(bound[field] ?? "none") instead of \(value).")
        }
        if v.ok { v.checks.append("The deposit address is bound to a quote with your recipient, refund address and amount.") }
        return v
    }
}

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
    /// Signable, but only after a fresh in-person prompt that names these, whatever session is open (a swap whose
    /// slippage is past the person's limit).
    public var askAfresh: [String] = []
    /// What leaves the wallet, as read: a transfer (with whom it goes to: an address, or its owner for a token account the
    /// signer could tie to one), or what a swap gives (no `to`). The automation wallet's dollars are counted from these.
    public var outflows: [Outflow] = []
    /// A swap is in it: its rate is the person's to judge (the quote it's held to comes with the request), so a person
    /// signs it with a prompt of its own, never inside an open session (the 2026-11-08 review).
    public var swaps = false
    public var ok: Bool { problems.isEmpty }

    public struct Outflow: Sendable, Equatable {
        /// A mint, «SOL», or an EVM token contract / «native».
        public let asset: String
        public let units: UInt64
        public let to: String?
        public init(asset: String, units: UInt64, to: String?) { (self.asset, self.units, self.to) = (asset, units, to) }
    }

    public init() {}

    public mutating func merge(_ other: Verdict) {
        checks += other.checks
        problems += other.problems
        askAfresh += other.askAfresh
        outflows += other.outflows
        swaps = swaps || other.swaps
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
    /// The slippage a swap may allow before it asks in person afresh (the person's limit, Settings), basis points.
    public var maxSlippageBps: Int

    public init(owner: String, recipients: Set<String> = [], programs: [String: String] = SolanaGuard.knownPrograms,
                maxPriorityFee: UInt64 = 5_000_000, trustLookupTables: Bool = true, sponsor: String? = nil, maxSlippageBps: Int = 300) {
        self.owner = owner
        self.recipients = recipients
        self.programs = programs
        self.maxPriorityFee = maxPriorityFee
        self.trustLookupTables = trustLookupTables
        self.sponsor = sponsor
        self.maxSlippageBps = maxSlippageBps
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
    /// What a JupiterZ market maker runs beside its fill to source the coin, seen in its orders for Ondo's tokenized stocks
    /// (2026-10-29): Ondo Global Markets mints or redeems the stock for the maker against a price it signed (the secp256k1
    /// precompile checks that signature), between two calls of a Jupiter program (jupgf…) on the maker's accounts. Allowed
    /// only beside the one fill, with a maker paying, and never given this wallet or one of its accounts.
    static let jupiterZMakerPrograms = [
        "XzTT4XB8m7sLD2xi6snefSasaswsKCxx5Tifjondogm": "Ondo Global Markets", "KeccakSecp256k11111111111111111111111111111": "the secp256k1 check",
        "jupgfSgfuAXv4B6R2Uxu85Z1qdzgju79s6MfZekN6XS": "Jupiter's maker program",
    ]
    static let wrappedSOL = "So11111111111111111111111111111111111111112"
    /// Relay Depository (github.com/relayprotocol/relay-depository), Anchor program.
    public static let relayDepository = "99vQwtBwYtrqqD9YSXbdum3KBdxPAVxYTaQ3cfnJSrN2"
    public static let kaminoLend = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD"
    /// Kamino's vaults (Earn) and the farm their shares are staked in: a vault withdrawal unstakes, then redeems.
    public static let kaminoVault = "KvauGMspG5k6rtzrqqn7WNn3oZdyKqLKwK2XWQ8FLjd"
    public static let kaminoFarms = "FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr"
    /// Kamino's main lending market (Kamino.mainMarket).
    public static let kaminoMainMarket = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF"
    /// Kamino's Backpack market (Kamino.backpackMarket): Backpack's tokenized stocks as collateral, USDC lent against them.
    public static let kaminoBackpackMarket = "4iRHKGsTq3e4uut6e4PfyV9AEbNuXaMu9JaSP378p9qy"
    /// The markets ihatecrypto lends and borrows on; a position on any other is refused.
    public static let kaminoMarkets = [kaminoMainMarket, kaminoBackpackMarket]

    /// The Kamino instructions ihatecrypto's deposits and withdrawals use whose programs pay out only to the signer
    /// (token::authority = owner, has_one = owner; read in the verified source, klend e921c64, kvault 2dadf94, kfarms
    /// bfa1860, 2026-10-23; the repays klend 77e8185, 2026-10-28), by 8-byte discriminator. Any other — a deposit
    /// minting to someone else's account, a flash loan, handing a farm stake over (transfer_ownership), topping up a
    /// vault's rewards — is refused, whatever the app says. KLend's value is the index of the obligation the instruction
    /// acts on (nil: none), which must be the owner's own on one of `kaminoMarkets`: a repay of someone else's debt fails it.
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
        // repay_obligation_liquidity(_v2): the program lets anyone repay any obligation; pinned to the owner's own, it
        // pays back only this wallet's debt. The money comes from an account the signer is the authority of (the
        // transfer runs under `owner`'s signature), and the balance simulation checks how much leaves.
        "91b20de14cf09348": 1, "74aed54cb435d290": 1,
        "ea75b57db98edc1d": nil,                                                  // redeem_reserve_collateral
    ]
    /// KLend's borrows and withdrawals by discriminator: the index of the account they pay into (user_destination_liquidity,
    /// klend's IDL; v2 adds its farm accounts after it). Its amount is the u64 after the discriminator: a borrow's in the
    /// coin, a withdrawal's in Kamino's shares of it (checked on transactions ktx built, 2026-11-05).
    static let kaminoPayouts: [String: (to: Int, borrow: Bool)] = [
        "797f12cc49f5e141": (8, true), "a1808ff5abc7c206": (8, true),             // borrow_obligation_liquidity(_v2)
        "4b5d5ddc2296dac4": (9, false), "eb34779895c51407": (9, false),           // withdraw_obligation_collateral_and_redeem_reserve_collateral(_v2)
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

    /// Jupiter's exact-in v2 swaps by discriminator (route_v2, shared_accounts_route_v2): where the arguments start
    /// (in_amount u64, quoted_out_amount u64, slippage_bps u16, platform_fee_bps u16, positive_slippage_bps u16, then the
    /// route), the account and mint that go in, the account the output lands in, the one that replaces it when set, and
    /// the output mint. Checked 2026-11-08 on mainnet swaps: in_amount is what left the source account.
    static let jupiterRoutes: [String: (args: Int, source: Int, inMint: Int, to: Int, instead: Int?, mint: Int)] = [
        "bb64facc31c4af14": (8, 1, 3, 2, 7, 4),
        "d19853937cfed8e9": (9, 2, 6, 5, nil, 7),
    ]
    /// Jupiter's other swaps: v1 (route, shared_accounts_route, their token-ledger and exact-out kinds) put the amounts
    /// after the route, and reading that takes Jupiter's whole list of pools, which grows every few weeks; exact-out v2
    /// isn't one ihatecrypto builds. Refused by name.
    static let jupiterUnread: Set<String> = ["e517cb977ae3ad2a", "96564774a75d0e68", "d033ef977b2bed5c", "c1209b3341d69c81",
                                             "e6798f50779f6aaa", "b0d169a89a7d453e", "9d8ab85215f4f324", "3560e5cad8bbfa18"]

    /// `n` units of `mint`, by its name where the signer knows it: «3 USDC», else «3000000 units of EPjFWd…».
    static func units(_ n: UInt64, _ mint: String) -> String {
        mintNames[mint].map { "\(Decimal(n) / pow(10, $0.decimals)) \($0.symbol)" } ?? "\(n) units of \(mint.prefix(6))…"
    }

    /// 300 → «3%».
    static func percent(_ bps: Int) -> String { "\(NSDecimalNumber(decimal: Decimal(bps) / 100).stringValue)%" }

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

    /// The owner's plain obligation on a Kamino market: seeds [tag 0, id 0, owner, market, default, default] (klend
    /// handler_init_obligation.rs). An instruction on it acts on that market (has_one = lending_market), whatever market
    /// a lookup table names: the signer can't read those, so this pins the market from a static account instead.
    /// `account` is `owner`'s associated token account for `mint`, under SPL Token or Token-2022.
    static func ownTokenAccount(_ account: String?, owner: String, mint: String?) -> Bool {
        guard let account, let mint else { return false }
        return [SPL.tokenProgram, SPL.token2022Program].contains { (try? SPL.associatedAccount(owner: owner, mint: mint, program: $0)) == account }
    }

    public static func kaminoObligation(owner: String, market: String = kaminoMainMarket) -> String? {
        guard let o = try? Base58.decode(owner), let m = try? Base58.decode(market), let p = try? Base58.decode(kaminoLend),
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
        // Backpack's stocks, Token-2022 (Kamino's Backpack market): a withdrawal of one names it.
        "SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb": ("SPCX", 6), "MUxEsUKSMACyw5fZf68wxf5FLnZVhtU9CwH8uNNGay1": ("MU", 6),
        "SKHYhSjuRWHgikq8eRKbtBbpABgJSkd7ytQV14i9EQ3": ("SKHY", 6), "SNDKbwMUQvZhnLnxLduradgLHG5KrPuKwpnrkkGRhfH": ("SNDK", 6),
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
        var fills = 0, makerSteps = 0
        // The fill's taker side — the taker (0), what it gives from (2) and what it gets into (4) — found before the loop:
        // a maker's step never touches them, whatever the app claimed about whose they are.
        let takerSide = Set(m.instructions.first { $0.programIndex < m.staticKeys.count && m.staticKeys[$0.programIndex] == jupiterZ }
            .map { ix in [0, 2, 4].compactMap { $0 < ix.accounts.count ? ix.accounts[$0] : nil } } ?? [])
        for (n, ix) in m.instructions.enumerated() {
            guard ix.programIndex < m.staticKeys.count else {
                v.problems.append("Instruction \(n + 1): the program is loaded from a lookup table.")
                continue
            }
            let program = accounts[ix.programIndex]
            // The wallet signs as account 1 and is a static key, so a step not given it can't act with its signature.
            if policy.sponsor != nil, let maker = jupiterZMakerPrograms[program] {
                guard !ix.accounts.contains(where: { takerSide.contains($0) || $0 < accounts.count && (accounts[$0] == policy.owner || mine(accounts[$0])) }) else {
                    v.problems.append("Instruction \(n + 1): \(maker) is given your wallet or one of its accounts.")
                    continue
                }
                makerSteps += 1
                if makerSteps == 1 { v.checks.append("The market maker sources the coin itself (\(maker) and the like), given none of your swap's accounts.") }
                continue
            }
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
                    if account(0) == policy.owner, !mine(to) { v.outflows.append(.init(asset: "SOL", units: lamports, to: to)) }
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
                    // The mint: named by TransferChecked, else the one this wallet's own source account is for.
                    // Counted when this wallet signs for it (its authority) or its own account pays, whatever the app claims.
                    let source = account(0)
                    let mint = tag == 12 ? account(1) : accounts.first { ownTokenAccount(source, owner: policy.owner, mint: $0) }
                    if account(tag == 3 ? 2 : 3) == policy.owner || source.map(mine) == true, !mine(to) {
                        v.outflows.append(.init(asset: mint ?? "?", units: units, to: tokenOwners[to] ?? to))
                    }
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
                    guard let obligation = account(at), staticKeys.contains(obligation),
                          kaminoMarkets.contains(where: { kaminoObligation(owner: policy.owner, market: $0) == obligation }) else {
                        v.problems.append("Instruction \(n + 1): the Kamino position isn't your own on Kamino's main or Backpack market.")
                        continue
                    }
                }
                // init_user_metadata's referrer (account 3) would take a share of the fees: none, which Anchor writes as the program.
                if kind == "75a9b045c5170fa2", account(3) != kaminoLend {
                    v.problems.append("Instruction \(n + 1): Kamino's account setup names a referrer.")
                    continue
                }
                // A borrow or a withdrawal pays this wallet's own account, and the prompt says how much, read here: a borrow
                // ten times the card's would otherwise sign as «Calls Kamino Lend» (the 2026-11-05 review).
                if program == kaminoLend, let payout = kaminoPayouts[kind] {
                    guard ix.data.count >= 16, let to = account(payout.to), staticKeys.contains(to), mine(to) else {
                        v.problems.append("Instruction \(n + 1): Kamino would pay an account that isn't yours.")
                        continue
                    }
                    let raw = le64(ix.data, 8)
                    let what = mintNames.first { ownTokenAccount(to, owner: policy.owner, mint: $0.key) }
                        .map { "\(Decimal(raw) / pow(10, $0.value.decimals)) \($0.value.symbol)" } ?? "\(raw) units"
                    v.checks.append(payout.borrow ? "Kamino lends \(what) → your account, owed back to Kamino."
                                                  : "Kamino withdrawal: up to \(what) in Kamino's shares, each worth one or more → your account.")
                }
                v.checks.append("Calls \(programName) on your own position: it pays only you, and a repay pays only your debt. The balance simulation checks the amount.")
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
                // A mint a lookup table names is unread here: the wallet's own account for it says which it is (its address
                // derives from the mint), so the prompt reads «3 USDC», not «3000000 units of lookup-table entry 21».
                func named(_ a: String, _ m: String) -> String { mintNames.keys.first { ownTokenAccount(a, owner: policy.owner, mint: $0) } ?? m }
                v.outflows.append(.init(asset: named(source, inMint), units: le64(ix.data, 8), to: nil))
                v.swaps = true
                v.checks.append("JupiterZ fill: \(units(le64(ix.data, 8), named(source, inMint))) for \(units(le64(ix.data, 16), named(out, mint))) → \(out == jupiterZ ? "your wallet, as SOL" : "your account"). The balance simulation checks it.")
            case jupiterSwap:
                // Exact-in v2 swaps only: in_amount, quoted_out_amount and slippage_bps open their data, so what the signer
                // reads is what the program reads, and the prompt says what goes in and the least that comes out (the
                // 2026-11-08 review: a program that took over the app could swap at a bad rate inside a Touch ID session).
                // v1 swaps put the amounts after the route, where a site could also append a made-up quote (the
                // 2026-10-22 review): refused, and ihatecrypto asks Jupiter for v2. The output account (its «instead» one
                // when set: the program's own id when not) must be this wallet's own (the 2026-10-24 review).
                let disc = ix.data.prefix(8).hex
                guard let layout = jupiterRoutes[disc], ix.data.count >= layout.args + 22, var to = account(layout.to),
                      let mint = account(layout.mint), let inMint = account(layout.inMint) else {
                    v.problems.append(jupiterUnread.contains(disc)
                        ? "Instruction \(n + 1): an older Jupiter swap whose amounts the signer can't read: it signs only Jupiter's v2 swaps."
                        : "Instruction \(n + 1): a Jupiter instruction that isn't one of its swaps (\(disc)).")
                    continue
                }
                if let instead = layout.instead.flatMap(account), instead != jupiterSwap { to = instead }
                // This wallet's own token account for the mint, by the claim the signer proved or derived here.
                let ours = mine(to) || [SPL.tokenProgram, SPL.token2022Program].contains { (try? SPL.associatedAccount(owner: policy.owner, mint: mint, program: $0)) == to }
                guard ours, policy.trustLookupTables || staticKeys.contains(to) else {
                    v.problems.append("The Jupiter swap pays \(to), not your wallet.")
                    continue
                }
                let inAmount = le64(ix.data, layout.args), quoted = le64(ix.data, layout.args + 8)
                let slippage = Int(le16(ix.data, layout.args + 16)), fee = Int(le16(ix.data, layout.args + 18))
                guard slippage <= 10_000, fee <= 1_000 else { v.problems.append("Instruction \(n + 1): a Jupiter swap with slippage over 100% or a fee over 10%."); continue }
                let least = UInt64((UInt128(quoted) * UInt128(10_000 - slippage)) / 10_000)
                // Each mint by this wallet's own account for it where the message's mint is one a lookup table names
                // (the signer can't read tables): «3 USDC», not «3000000 units of lookup-table entry 21».
                func named(_ a: String?, _ m: String) -> String {
                    guard let a else { return m }
                    return ownTokenAccount(a, owner: policy.owner, mint: m) ? m : mintNames.keys.first { ownTokenAccount(a, owner: policy.owner, mint: $0) } ?? m
                }
                let given = named(account(layout.source), inMint), got = named(to, mint)
                v.checks.append("Jupiter swap: \(units(inAmount, given)) → at least \(units(least, got)), your account (slippage \(percent(slippage))"
                                + (fee > 0 ? ", a \(percent(fee)) fee to the site" : "") + ")")
                if slippage > policy.maxSlippageBps { v.askAfresh.append("slippage \(percent(slippage)) is over your \(percent(policy.maxSlippageBps))") }
                if fee > 0 { v.askAfresh.append("the site takes a \(percent(fee)) fee") }
                // Counted as that coin only when it leaves this wallet's own account for it.
                let sourceOwn = account(layout.source).map { mine($0) || ownTokenAccount($0, owner: policy.owner, mint: given) } ?? false
                v.outflows.append(.init(asset: sourceOwn ? given : "?", units: inAmount, to: nil))
                v.swaps = true
            default:
                v.checks.append("Calls \(programName). The balance simulation checks the result.")
            }
        }
        if makerSteps > 0, fills != 1 { v.problems.append("A market maker's steps come without its one JupiterZ fill.") }
        // A JupiterZ maker fronts no rent: the wallet's own new account is paid by its own create step. SOL «back» to the
        // fee payer beside a fill would be the maker skimming the wallet (the 2026-10-29 review).
        if fills > 0, refunded > 0 { v.problems.append("\(Double(refunded) / 1e9) SOL to the market maker: a JupiterZ fill takes no SOL from your wallet.") }
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
        /// And at most this, when set: a borrow or a withdrawal brings no more than the card said.
        public let atMost: Int64?

        public init(label: String, before: Int64, after: Int64, atLeast: Int64, atMost: Int64? = nil) {
            self.label = label
            self.before = before
            self.after = after
            self.atLeast = atLeast
            self.atMost = atMost
        }
    }

    /// Compares simulated balances with what the user approved: this catches a route that pays someone else.
    public static func balances(_ rules: [BalanceRule]) -> Verdict {
        var v = Verdict()
        for r in rules {
            let delta = r.after - r.before
            if delta < r.atLeast { v.problems.append("Simulation: \(r.label) changes by \(delta); the minimum allowed is \(r.atLeast).") }
            else if let most = r.atMost, delta > most { v.problems.append("Simulation: \(r.label) changes by \(delta), over the most allowed, \(most).") }
            else { v.checks.append("Simulation: \(r.label) \(delta >= 0 ? "+" : "")\(delta).") }
        }
        return v
    }

    /// Rights a transaction hands out that no balance shows (research E-1; SlowMist, December 2025: a program outside the
    /// list Assigned wallets to itself while the signing window's balances didn't move). Read from the simulation: the
    /// wallet still belongs to the System program and holds no data, and each of our token accounts in `before` keeps its
    /// owner and gets no new close authority, no new delegate and no more for its delegate to spend. A change is refused,
    /// never a field already set: spam accounts others open for a wallet carry a stranger's close authority from the start.
    /// A token account of the wallet's in `after` that isn't in `before` was opened by this transaction: it starts with no
    /// one else's rights (a delegate on it would drain every later deposit). An account missing from `after` was closed:
    /// it has no rights left. `names`: a token account's coin, «USDC».
    /// ponytail: the wallet and its token accounts only (variant A); stake, nonce and lookup-table authorities and Token-2022's
    /// extensions aren't compared — every writable account's address fields (variant B) once wallets hold those.
    public static func rightsProblems(owner: String, before: [String: SolanaAccountRights], after: [String: SolanaAccountRights],
                                      names: [String: String] = [:]) -> [String] {
        var out: [String] = []
        if let w = after[owner] {
            if w.program != system {
                out.append("Your wallet would be handed to \(w.program.isEmpty ? "a program the simulation doesn't name" : "the program \(w.program)"): "
                           + "from then on that program, not your key, decides what leaves it.")
            }
            if w.space > 0 { out.append("Your wallet would get \(w.space) bytes of data and could no longer pay Solana's fee.") }
        }
        let opened = after.filter { before[$0.key] == nil && $0.value.owner == owner }.mapValues { _ in SolanaAccountRights() }
        for (address, was) in before.merging(opened, uniquingKeysWith: { a, _ in a }).sorted(by: { $0.key < $1.key }) {
            guard let now = after[address], let holder = now.owner else { continue }
            let coin = names[address] ?? "tokens in \(address.prefix(4))…\(address.suffix(4))"
            let account = names[address].map { "\($0) account" } ?? "token account \(address.prefix(4))…\(address.suffix(4))"
            if holder != owner { out.append("Your \(account) would belong to \(holder).") }
            if let c = now.closeAuthority, c != owner, c != was.closeAuthority { out.append("\(c) could close your \(account) and take its rent.") }
            if let d = now.delegate, d != owner, d != was.delegate || now.delegatedAmount > was.delegatedAmount { out.append("\(d) could spend your \(coin).") }
        }
        return out
    }

    static func le16(_ d: Data, _ at: Int) -> UInt16 { UInt16(d[d.startIndex + at]) | UInt16(d[d.startIndex + at + 1]) << 8 }
    static func le32(_ d: Data, _ at: Int) -> UInt32 { (0..<4).reduce(0) { $0 | UInt32(d[d.startIndex + at + $1]) << (8 * $1) } }
    static func le64(_ d: Data, _ at: Int) -> UInt64 { (0..<8).reduce(0) { $0 | UInt64(d[d.startIndex + at + $1]) << (8 * $1) } }
}

// MARK: - A second simulation

/// What a second simulation, run after the user pressed Sign, may differ in from the one the window showed (research E-4;
/// ScamSniffer: a site changes the state in the seconds between the look and the signature).
public enum SimulationDrift {
    /// Why what leaves now isn't what was shown: an asset leaves that didn't, or more than 1 % more of one leaves. Keys are
    /// assets, values their signed change. Arrivals aren't compared: the transaction's own minimum holds them, and a price
    /// moves in half a minute. `names`: how an asset reads in the reason.
    public static func outflowsGrew(shown: [String: Decimal], now: [String: Decimal], names: [String: String] = [:]) -> String? {
        for (asset, change) in now.sorted(by: { $0.key < $1.key }) where change < 0 {
            let name = names[asset] ?? asset
            guard let was = shown[asset], was < 0 else { return "\(name) would leave too, which the window didn't show" }
            if -change > -was * Decimal(string: "1.01")! { return "more \(name) would leave than the window showed" }
        }
        return nil
    }
}

// MARK: - Hyperliquid

public enum HLGuard {
    /// Hyperliquid's own market-making vault; the only vault money may go to without the user naming it.
    public static let hlpVault = "0xdfc24b077bc1425ad1dea75bcb6f8158e10df303"
    /// The top of Settings › Trading's leverage stepper (RiskPolicy.leverageSteps): 50×, the most any Hyperliquid market gives.
    /// Past the person's own limit (SignerLimits.leverageCap) the trading key doesn't sign it: the main key does, in person.
    public static let maxLeverage: UInt64 = 50

    /// «leverage 30× is above your 10× limit» for an updateLeverage past `cap`, else nil.
    public static func leverage(_ action: MsgPack, over cap: Int) -> String? {
        guard case .map(let pairs) = action, case .string("updateLeverage")? = pairs.first(where: { $0.0 == "type" })?.1,
              case .uint(let n)? = pairs.first(where: { $0.0 == "leverage" })?.1, n > UInt64(max(cap, 1)) else { return nil }
        return "leverage \(n)× is above your \(cap)× limit"
    }

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
        case "trailingStop":
            // The trading key signs it without a prompt: only one that can close, never open or add to, a position. The
            // exchange holds a reduce-only order to the position's own coin and size.
            guard pairs.first(where: { $0.0 == "reduceOnly" })?.1 == .bool(true) else {
                v.problems.append("A trailing stop that could open or add to a position is not allowed.")
                return v
            }
            let side = pairs.first(where: { $0.0 == "isBuy" })?.1 == .bool(true) ? "buy" : "sell"
            let market = if case .uint(let a)? = pairs.first(where: { $0.0 == "asset" })?.1 { "market #\(a)" } else { "a market" }
            let pct = if case .map(let r)? = pairs.first(where: { $0.0 == "retracement" })?.1, case .string(let p)? = r.first?.1 { p } else { "?" }
            v.checks.append("Trailing stop: \(side) \(text("sz") ?? "?") on \(market) once the price falls back \(pct) from its best, only reducing a position → your account")
        case "twapOrder":
            // Parts at the market inside the account, like an order; it takes no builder field, so no fee can ride on it.
            guard case .map(let t)? = pairs.first(where: { $0.0 == "twap" })?.1, t.count == 6,
                  // Up to 4 hours: a stop that fires leaves a TWAP buying, and only the app watching stops it (Plan.twapMaxMinutes).
                  case .uint(let minutes)? = t.first(where: { $0.0 == "m" })?.1, (5...240).contains(minutes) else {
                v.problems.append("A TWAP that isn't 5 minutes to 4 hours of parts inside your account is not allowed.")
                return v
            }
            func field(_ k: String) -> MsgPack? { t.first(where: { $0.0 == k })?.1 }
            let side = field("b") == .bool(true) ? "buy" : "sell"
            let size = if case .string(let s)? = field("s") { s } else { "?" }
            let market = if case .uint(let a)? = field("a") { "market #\(a)" } else { "a market" }
            v.checks.append("TWAP: \(side) \(size) on \(market) over \(minutes) min at the market\(field("r") == .bool(true) ? ", only reducing a position" : "") → your account")
        case "twapCancel":
            v.checks.append("Stops a TWAP inside your account.")
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

    /// A private move goes only where 1Click itself holds the deposit as confidential (Confidential Intents): else it
    /// would go in the open.
    public static func confidential(bound: [String: String]) -> Verdict {
        var v = Verdict()
        if bound["confidentiality"]?.lowercased() == "basic" { v.checks.append("1Click holds the deposit as confidential.") }
        else { v.problems.append("1Click didn't take it as private (Confidential Intents): it says «\(bound["confidentiality"] ?? "nothing")».") }
        return v
    }

    /// A deposit an MPP service asks for through NEAR Intents (its quote, not ours): 1Click itself must hold the address
    /// for an exact-output quote paying the service's recipient exactly `amountOut`, refunding to the address the service
    /// named, taking USDC from the network paid on and, when it's known, paying the asset the card shows. And the deposit
    /// can't be more than 1Click's quote takes plus its slippage: the rest would only come back as a refund — to the
    /// service. `bound` as OneClick.boundRequest reads it.
    public static func charge(bound: [String: String], recipient: String, refundTo: String, origin: String, destination: String?,
                              amountOut: String, deposit: UInt64, now: Date = Date()) -> Verdict {
        var v = Verdict()
        if bound["swapType"] != "EXACT_OUTPUT" { v.problems.append("1Click holds it as \(bound["swapType"] ?? "an unknown") quote, not one paying an exact amount.") }
        var expected = ["recipient": recipient, "refundTo": refundTo, "originAsset": origin, "amount": amountOut]
        if let destination { expected["destinationAsset"] = destination }
        for (field, value) in expected.sorted(by: { $0.key < $1.key }) where bound[field]?.lowercased() != value.lowercased() {
            v.problems.append("The deposit address is bound to a different \(field): \(bound[field] ?? "none") instead of \(value).")
        }
        if let asked = bound["quoteAmountIn"].flatMap({ UInt64($0) }) {
            // 1Click's own price for this deposit, plus 1% for its slippage and rounding.
            if deposit > asked + asked / 100 + 1 { v.problems.append("The service asks \(deposit) units, more than the \(asked) 1Click takes for it.") }
        } else {
            v.problems.append("1Click didn't say what this deposit costs.")
        }
        // Under 1Click's minimum, or after its quote runs out, the deposit only comes back — to the service's refund address.
        if let least = bound["quoteMinAmountIn"].flatMap({ UInt64($0) }), deposit < least {
            v.problems.append("The deposit is under the \(least) units 1Click needs for it.")
        }
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let deadline = bound["deadline"].flatMap { f.date(from: $0) ?? ISO8601DateFormatter().date(from: $0) }
        if let deadline, deadline < now.addingTimeInterval(120) { v.problems.append("1Click's quote for this deposit runs out at \(bound["deadline"] ?? "")") }
        if v.ok { v.checks.append("1Click holds the deposit for the service's recipient, its exact amount and its refund address, at 1Click's own price.") }
        return v
    }
}

/// A limit order's deposit goes only to an order 1Click itself holds as asked: read back from 1Click by its id, never taken
/// from the answer that created it alone. Money then rests with 1Click and pays out later without another signature, so
/// whatever this check lets through is the only check the order gets.
public enum OrderGuard {
    /// `bound`: the order as OneClick.order reads it. `deposit`: the units the wallet is about to send; `most`: the units
    /// the user approved (a sale's coins, a buy's USDC). `fee`: ihatecrypto's own fee, the only one an order may carry.
    public static func check(bound: [String: String], sell: Bool, base: String, quote: String, quantity: String, price: Decimal,
                             depositAddress: String, recipient: String, refundTo: String, deposit: String, most: Decimal,
                             fee: (recipient: String, bps: Int), deadline: Date, now: Date = Date()) -> Verdict {
        var v = Verdict()
        let expected = ["orderType": "LIMIT", "side": sell ? "SELL" : "BUY", "baseAsset": base, "quoteAsset": quote, "quantity": quantity,
                        "depositAddress": depositAddress, "depositType": "ORIGIN_CHAIN", "recipient": recipient, "recipientType": "DESTINATION_CHAIN",
                        "refundTo": refundTo, "refundType": "ORIGIN_CHAIN"]
        for (field, value) in expected.sorted(by: { $0.key < $1.key }) where bound[field]?.lowercased() != value.lowercased() {
            v.problems.append("1Click holds the order with a different \(field): \(bound[field] ?? "none") instead of \(value).")
        }
        if bound["price"].flatMap({ Decimal(string: $0) }) != price { v.problems.append("1Click holds the order at \(bound["price"] ?? "no") price, not \(price).") }
        if !["basic", "advanced"].contains(bound["confidentiality"]?.lowercased() ?? "") {
            v.problems.append("1Click didn't take the order as private (Confidential Intents): it says «\(bound["confidentiality"] ?? "nothing")».")
        }
        // A memo the transfer can't carry would leave the deposit unmatched.
        if let memo = bound["depositMemo"], !memo.isEmpty { v.problems.append("1Click asks for a deposit memo this wallet can't send.") }
        // 1Click lists its own platform fee among the order's fees (20 basis points to its account, seen 2026-11-11): that
        // much is allowed besides ihatecrypto's, nothing more.
        var others = 0
        for f in (bound["appFees"] ?? "").split(separator: ",") {
            let parts = f.split(separator: ":"), bps = parts.count == 2 ? Int(parts[1]) ?? -1 : -1
            guard parts.count == 2, bps >= 0 else { v.problems.append("The order carries a fee 1Click doesn't spell out: \(f)."); continue }
            if String(parts[0]) == fee.recipient {
                if bps > fee.bps { v.problems.append("The order gives ihatecrypto \(bps) basis points, not \(fee.bps).") }
            } else {
                others += bps
            }
        }
        if others > protocolBps { v.problems.append("The order carries \(others) basis points of fees besides ihatecrypto's, more than 1Click's own \(protocolBps).") }
        // What 1Click's swap view takes is what the wallet sends, and never more than was approved.
        let takes = bound[sell ? "amountIn" : "maxAmountIn"]
        if takes != deposit { v.problems.append("1Click's order takes \(takes ?? "an unknown amount"), not the \(deposit) units about to be sent.") }
        if sell, deposit != quantity { v.problems.append("A sale deposits its own quantity, \(quantity) units, not \(deposit).") }
        if let d = Decimal(string: deposit), d > most || d <= 0 { v.problems.append("The deposit, \(deposit) units, is more than the \(most) approved.") }
        if Decimal(string: deposit) == nil { v.problems.append("The deposit isn't a number of units: \(deposit).") }
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let ends = bound["deadline"].flatMap { f.date(from: $0) ?? ISO8601DateFormatter().date(from: $0) }
        if let ends {
            if ends > deadline.addingTimeInterval(3600) { v.problems.append("1Click keeps the order until \(bound["deadline"] ?? ""), later than asked.") }
            if ends < now.addingTimeInterval(600) { v.problems.append("The order runs out at \(bound["deadline"] ?? ""), before it could rest.") }
        } else {
            v.problems.append("1Click didn't say until when the order rests.")
        }
        if v.ok { v.checks.append("1Click holds the order as asked: the pair, side, amount and price, your own addresses, private, with only ihatecrypto's fee.") }
        return v
    }

    /// 1Click's own fee on an order, as it lists it: 0.20%.
    public static let protocolBps = 20

    /// Whether the price is already through the market, so the order would fill at once — a swap at market, not a limit:
    /// a sale at or under what one coin sells for now, a buy at or over what it costs.
    public static func fillsAtOnce(sell: Bool, price: Decimal, market: Decimal) -> Bool {
        market > 0 && (sell ? price <= market : price >= market)
    }
}

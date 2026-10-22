import Foundation
import HyperliquidSigning
import SolanaChain

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

    public init(owner: String, recipients: Set<String> = [], programs: [String: String] = SolanaGuard.knownPrograms,
                maxPriorityFee: UInt64 = 5_000_000, trustLookupTables: Bool = true) {
        self.owner = owner
        self.recipients = recipients
        self.programs = programs
        self.maxPriorityFee = maxPriorityFee
        self.trustLookupTables = trustLookupTables
    }
}

public enum SolanaGuard {
    public static let system = "11111111111111111111111111111111"
    public static let computeBudget = "ComputeBudget111111111111111111111111111111"
    public static let jupiterSwap = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"
    public static let jupiterLend = "jup3YeL8QhtSx1e253b2FDvsMNC87fDrgQZivbrndc9"
    /// Jupiter's on-chain DCA (Recurring v1): open_dca_v2 and close_dca on the user's own order account.
    public static let jupiterDCA = "DCA265Vj8a9CEuX1eb1LWRnDT7uK6q1xMipnNyatn23M"
    /// Relay Depository (github.com/relayprotocol/relay-depository), Anchor program.
    public static let relayDepository = "99vQwtBwYtrqqD9YSXbdum3KBdxPAVxYTaQ3cfnJSrN2"
    public static let kaminoLend = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD"
    /// Kamino's vaults (Earn) and the farm their shares are staked in: a vault withdrawal unstakes, then redeems.
    public static let kaminoVault = "KvauGMspG5k6rtzrqqn7WNn3oZdyKqLKwK2XWQ8FLjd"
    public static let kaminoFarms = "FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr"
    /// Kamino's first deposit creates the user's own lookup table; only create and extend with the owner as authority pass.
    public static let lookupTableProgram = "AddressLookupTab1e1111111111111111111111111"
    public static let knownPrograms: [String: String] = [
        system: "System", computeBudget: "Compute Budget", SPL.tokenProgram: "SPL Token", SPL.token2022Program: "SPL Token-2022",
        SPL.associatedTokenProgram: "Associated Token", jupiterSwap: "Jupiter Swap", jupiterLend: "Jupiter Lend Earn", jupiterDCA: "Jupiter DCA",
        relayDepository: "Relay Depository", kaminoLend: "Kamino Lend", kaminoVault: "Kamino Vault", kaminoFarms: "Kamino Farms",
        lookupTableProgram: "Address Lookup Table",
    ]
    /// Anchor discriminators: sha256("global:deposit_token")[0..<8], sha256("global:deposit_native")[0..<8].
    static let relayDepositToken = Data([0x0b, 0x9c, 0x60, 0xda, 0x27, 0xa3, 0xb4, 0x13])
    static let relayDepositNative = Data([0x0d, 0x9e, 0x0d, 0xdf, 0x5f, 0xd5, 0x1c, 0x06])
    /// Jupiter's route and shared_accounts_route, the two its swap API builds for an exact-in swap (read 2026-10-21 from
    /// lite-api's /swap): sha256("global:route")[0..<8], sha256("global:shared_accounts_route")[0..<8].
    static let jupiterRoute = Data([0xe5, 0x17, 0xcb, 0x97, 0x7a, 0xe3, 0xad, 0x2a])
    static let jupiterSharedRoute = Data([0xc1, 0x20, 0x9b, 0x33, 0x41, 0xd6, 0x9c, 0x81])
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
        guard m.requiredSignatures == 1, accounts.first == policy.owner else {
            v.problems.append("Only your wallet may sign and pay for the transaction.")
            return v
        }
        v.checks.append("Only your wallet signs and pays the fee.")

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
                if allowed(to) {
                    v.checks.append("\(Double(lamports) / 1e9) SOL → \(name(to)).")
                } else {
                    v.problems.append("SOL goes to someone else's address \(to).")
                }
            // Token-2022 shares the classic program's instructions and their layout; its extensions' own instructions
            // (a transfer with a fee, confidential transfers…) are refused below like approvals.
            case SPL.tokenProgram, SPL.token2022Program:
                switch tag {
                case 3, 12: // Transfer(source, dest, authority), TransferChecked(source, mint, dest, authority)
                    guard let to = account(tag == 3 ? 1 : 2) else { v.problems.append("Instruction \(n + 1): incomplete token transfer."); continue }
                    if allowed(to) { v.checks.append("Tokens → \(name(to)).") } else { v.problems.append("Tokens go to someone else's account \(to).") }
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
                let kind = ix.data.prefix(8)
                guard kind == relayDepositToken || kind == relayDepositNative, ix.data.count >= 16 else {
                    v.problems.append("Instruction \(n + 1): only deposits are allowed on Relay.")
                    continue
                }
                if ix.accounts.contains(where: { $0 < accounts.count && accounts[$0] == policy.owner }) {
                    v.checks.append("Relay deposit: \(le64(ix.data, 8)) units from your wallet. The quote sets the recipient.")
                } else {
                    v.problems.append("Relay deposit is not from your wallet.")
                }
            case jupiterSwap:
                // route: the output to account 3, or to account 4 when it's set (the program's own id when not), mint 5;
                // shared_accounts_route: to account 6, mint 8. Its other forms are left to the simulation, as before.
                // The amounts aren't read: they end the data, and the program ignores bytes after them, so a site could
                // append a made-up quote and slippage (the 2026-10-22 review). The simulation checks what arrives.
                let kind = ix.data.prefix(8)
                let layout: (to: Int, instead: Int?, mint: Int)? = kind == jupiterRoute ? (3, 4, 5) : kind == jupiterSharedRoute ? (6, nil, 8) : nil
                guard let layout, var to = account(layout.to), let mint = account(layout.mint) else {
                    v.checks.append("Calls Jupiter Swap. The balance simulation checks the result.")
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
        case "order", "cancel":
            v.checks.append("Trading action inside your account.")
        case "usdClassTransfer":
            v.checks.append("Transfer between your spot and perp balances.")
        case "spotSend", "withdraw3", "usdSend", "sendAsset":
            let to = text("destination")?.lowercased() ?? ""
            if allowed.contains(to) { v.checks.append("Funds → approved address \(to.prefix(8))….") }
            else { v.problems.append("Funds go to someone else's address \(to).") }
        case "vaultTransfer":
            let vault = text("vaultAddress")?.lowercased() ?? ""
            if vault == hlpVault || allowed.contains(vault) { v.checks.append("Transfer between your account and the HLP vault.") }
            else { v.problems.append("Transfer to an unknown vault \(vault).") }
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

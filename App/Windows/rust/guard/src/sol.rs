//! The Solana guard (`SolanaGuard`, `SolanaPolicy` in Guard.swift).

use std::collections::{BTreeMap, HashMap, HashSet};

use solana::message::{AccountRights, Message};
use solana::spl::{self, ASSOCIATED_TOKEN_PROGRAM, TOKEN_2022_PROGRAM, TOKEN_PROGRAM};
use wallet::keys::base58;

use crate::{Outflow, Verdict, prefix, sol_text, suffix, units};

pub const SYSTEM: &str = "11111111111111111111111111111111";
pub const COMPUTE_BUDGET: &str = "ComputeBudget111111111111111111111111111111";
pub const JUPITER_SWAP: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
pub const JUPITER_LEND: &str = "jup3YeL8QhtSx1e253b2FDvsMNC87fDrgQZivbrndc9";
/// Jupiter's on-chain DCA (Recurring v1): open_dca_v2 and close_dca on the person's own order account.
pub const JUPITER_DCA: &str = "DCA265Vj8a9CEuX1eb1LWRnDT7uK6q1xMipnNyatn23M";
/// JupiterZ, Jupiter's market makers: one instruction, `fill`, where the maker pays the fee, signs first and swaps
/// with the taker at its quoted price.
pub const JUPITER_Z: &str = "61DFfeTKM7trxYcPQCM78bJ794ddZprZpAwAnLiwTpYH";
/// fill(input_amount, output_amount, expire_at): its accounts taker 0, maker 1, the taker's input 2 and output 4
/// accounts (the program's own id for none: SOL then comes as lamports), input mint 6, output mint 8.
const JUPITER_Z_FILL: &str = "a860b7a35c0a28a0";
/// What a JupiterZ market maker runs beside its fill to source the coin (Ondo's tokenized stocks, 2026-10-29). Allowed
/// only beside the one fill, with a maker paying, and never given this wallet or one of its accounts.
fn jupiter_z_maker_program(program: &str) -> Option<&'static str> {
    match program {
        "XzTT4XB8m7sLD2xi6snefSasaswsKCxx5Tifjondogm" => Some("Ondo Global Markets"),
        "KeccakSecp256k11111111111111111111111111111" => Some("the secp256k1 check"),
        "jupgfSgfuAXv4B6R2Uxu85Z1qdzgju79s6MfZekN6XS" => Some("Jupiter's maker program"),
        _ => None,
    }
}
const WRAPPED_SOL: &str = "So11111111111111111111111111111111111111112";
/// Relay Depository (github.com/relayprotocol/relay-depository), Anchor program.
pub const RELAY_DEPOSITORY: &str = "99vQwtBwYtrqqD9YSXbdum3KBdxPAVxYTaQ3cfnJSrN2";
pub const KAMINO_LEND: &str = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";
/// Kamino's vaults (Earn) and the farm their shares are staked in.
pub const KAMINO_VAULT: &str = "KvauGMspG5k6rtzrqqn7WNn3oZdyKqLKwK2XWQ8FLjd";
pub const KAMINO_FARMS: &str = "FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr";
pub const KAMINO_MAIN_MARKET: &str = "7u3HeHxYDLhnCoErrtycNokbQYbWGzLs6JSDqGAv5PfF";
/// Kamino's Backpack market: Backpack's tokenized stocks as collateral, USDC lent against them.
pub const KAMINO_BACKPACK_MARKET: &str = "4iRHKGsTq3e4uut6e4PfyV9AEbNuXaMu9JaSP378p9qy";
/// The markets ihatecrypto lends and borrows on; a position on any other is refused.
pub const KAMINO_MARKETS: [&str; 2] = [KAMINO_MAIN_MARKET, KAMINO_BACKPACK_MARKET];
/// Kamino's first deposit creates the person's own lookup table; only create and extend with the owner as authority pass.
pub const LOOKUP_TABLE_PROGRAM: &str = "AddressLookupTab1e1111111111111111111111111";

/// The Kamino Lend instructions ihatecrypto uses whose program pays out only to the signer (klend e921c64, the repays
/// 77e8185), by 8-byte discriminator, with the index of the obligation it acts on (None: none), which must be the
/// owner's own on one of `KAMINO_MARKETS`. Absent: refused.
fn kamino_lend_allowed(kind: &str) -> Option<Option<usize>> {
    Some(match kind {
        "75a9b045c5170fa2" => None,                                   // init_user_metadata (no referrer, below)
        "fb0ae74c1b0b9f60" | "883f0fbad398a8a4" => Some(2),           // init_obligation, init_obligation_farms_for_reserve
        "02da8aeb4fc91966" => None,                                   // refresh_reserve
        "218493e497c04859" | "8c90fd150a4af803" => Some(1),           // refresh_obligation, refresh_obligation_farms_for_reserve
        "2477fb8122f00793" => Some(1),                                // request_elevation_group
        "81c70402de271a2e" | "d8e0bf1bcc9766af" => Some(1),           // deposit_reserve_liquidity_and_obligation_collateral(_v2)
        "4b5d5ddc2296dac4" | "eb34779895c51407" => Some(1),           // withdraw_obligation_collateral_and_redeem_reserve_collateral(_v2)
        "2574cd67f3c05cc6" | "caf97572e7c02f8a" => Some(1),           // withdraw_obligation_collateral(_v2)
        "6cd1044815167685" | "8991975ea7710491" => Some(1),           // deposit_obligation_collateral(_v2)
        "797f12cc49f5e141" | "a1808ff5abc7c206" => Some(1),           // borrow_obligation_liquidity(_v2)
        // repay_obligation_liquidity(_v2): pinned to the owner's own obligation, it pays back only this wallet's debt.
        "91b20de14cf09348" | "74aed54cb435d290" => Some(1),
        "ea75b57db98edc1d" => None,                                   // redeem_reserve_collateral
        _ => return None,
    })
}

/// KLend's borrows and withdrawals: the index of the account they pay into and whether it's a borrow. Its amount is
/// the u64 after the discriminator: a borrow's in the coin, a withdrawal's in Kamino's shares of it.
fn kamino_payout(kind: &str) -> Option<(usize, bool)> {
    match kind {
        "797f12cc49f5e141" | "a1808ff5abc7c206" => Some((8, true)),
        "4b5d5ddc2296dac4" | "eb34779895c51407" => Some((9, false)),
        _ => None,
    }
}

/// Vault withdrawals (withdraw, sell, withdraw_from_available, redeem_in_kind): to the signer's own accounts. No deposits.
const KAMINO_VAULT_ALLOWED: [&str; 4] = ["b712469c946da122", "33e685a4017f83ad", "1383709baadc2239", "663abdfcc0db8c59"];
/// Farms: unstake, withdraw_unstaked_deposits, harvest_reward (to the owner), refresh_farm, refresh_user_state, initialize_user.
const KAMINO_FARMS_ALLOWED: [&str; 6] = ["5a5f6b2acd7c32e1", "2466bb31dc248443", "44c8e4e9b820e2bc", "d6838ab790c2ac2a", "01870c3ef38c4d6c", "6f11b9fa3c7a26fe"];

/// Jupiter's exact-in v2 swaps by discriminator (route_v2, shared_accounts_route_v2): where the arguments start
/// (in_amount u64, quoted_out_amount u64, slippage_bps u16, platform_fee_bps u16, positive_slippage_bps u16, then the
/// route), the account and mint that go in, the account the output lands in, the one that replaces it when set, and
/// the output mint. Checked 2026-11-08 on mainnet swaps: in_amount is what left the source account.
struct JupiterRoute { args: usize, source: usize, in_mint: usize, to: usize, instead: Option<usize>, mint: usize }

fn jupiter_route(kind: &str) -> Option<JupiterRoute> {
    Some(match kind {
        "bb64facc31c4af14" => JupiterRoute { args: 8, source: 1, in_mint: 3, to: 2, instead: Some(7), mint: 4 },
        "d19853937cfed8e9" => JupiterRoute { args: 9, source: 2, in_mint: 6, to: 5, instead: None, mint: 7 },
        _ => return None,
    })
}

/// Jupiter's other swaps: v1 (route, shared_accounts_route, their token-ledger and exact-out kinds) put the amounts
/// after the route, and reading that takes Jupiter's whole list of pools, which grows every few weeks; exact-out v2
/// isn't one ihatecrypto builds. Refused by name.
const JUPITER_UNREAD: [&str; 8] = ["e517cb977ae3ad2a", "96564774a75d0e68", "d033ef977b2bed5c", "c1209b3341d69c81",
                                   "e6798f50779f6aaa", "b0d169a89a7d453e", "9d8ab85215f4f324", "3560e5cad8bbfa18"];

/// `n` units of `mint`, by its name where the signer knows it: «3 USDC», else «3000000 units of EPjFWd…».
fn mint_units(n: u64, mint: &str) -> String {
    mint_name(mint).map_or(format!("{n} units of {}…", prefix(mint, 6)), |(s, d)| format!("{} {s}", units(n, d)))
}

/// 300 → «3%».
pub fn percent(bps: i64) -> String { format!("{}%", rust_decimal::Decimal::new(bps, 2).normalize()) }

/// Jupiter Lend Earn: deposits mint fTokens to account 2, the signer's own account for the fToken mint (account 6);
/// withdrawals and redemptions pay account 2, the signer's own account for the token (account 5).
const JUPITER_LEND_DEPOSITS: [&str; 4] = ["f223c68952e1f2b6", "74901061766d2877", "3339e12fb69289a6", "065e457a1eb392ab"];
const JUPITER_LEND_WITHDRAWALS: [&str; 4] = ["b712469c946da122", "b80c569546c461e1", "2fc5b7abef12f5ab", "ebbded38a6b4b895"];
/// Jupiter's recurring orders (DCA): open_dca_v2 and close_dca only, read by their accounts.
const JUPITER_DCA_OPEN: &str = "8e772b6da2340bb1";
const JUPITER_DCA_CLOSE: &str = "16072162a8b722f3";

/// Anchor discriminators of Relay's deposit_token and deposit_native.
const RELAY_DEPOSIT_TOKEN: [u8; 8] = [0x0b, 0x9c, 0x60, 0xda, 0x27, 0xa3, 0xb4, 0x13];
const RELAY_DEPOSIT_NATIVE: [u8; 8] = [0x0d, 0x9e, 0x0d, 0xdf, 0x5f, 0xd5, 0x1c, 0x06];

/// What the prompt names a mint in; another mint shows its raw units.
const MINT_NAMES: [(&str, &str, u32); 7] = [
    ("So11111111111111111111111111111111111111112", "SOL", 9),
    ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "USDC", 6),
    ("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", "USDT", 6),
    // Backpack's stocks, Token-2022 (Kamino's Backpack market): a withdrawal of one names it.
    ("SPCXxcqXj6e5dJDVNovHN8744zkbhM2bYudU45BimGb", "SPCX", 6), ("MUxEsUKSMACyw5fZf68wxf5FLnZVhtU9CwH8uNNGay1", "MU", 6),
    ("SKHYhSjuRWHgikq8eRKbtBbpABgJSkd7ytQV14i9EQ3", "SKHY", 6), ("SNDKbwMUQvZhnLnxLduradgLHG5KrPuKwpnrkkGRhfH", "SNDK", 6),
];

fn mint_name(mint: &str) -> Option<(&'static str, u32)> {
    MINT_NAMES.iter().find(|m| m.0 == mint).map(|m| (m.1, m.2))
}

pub fn known_programs() -> HashMap<String, String> {
    [(SYSTEM, "System"), (COMPUTE_BUDGET, "Compute Budget"), (TOKEN_PROGRAM, "SPL Token"), (TOKEN_2022_PROGRAM, "SPL Token-2022"),
     (ASSOCIATED_TOKEN_PROGRAM, "Associated Token"), (JUPITER_SWAP, "Jupiter Swap"), (JUPITER_LEND, "Jupiter Lend Earn"),
     (JUPITER_DCA, "Jupiter DCA"), (JUPITER_Z, "JupiterZ"), (RELAY_DEPOSITORY, "Relay Depository"), (KAMINO_LEND, "Kamino Lend"),
     (KAMINO_VAULT, "Kamino Vault"), (KAMINO_FARMS, "Kamino Farms"), (LOOKUP_TABLE_PROGRAM, "Address Lookup Table")]
        .into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub owner: String,
    /// Addresses money may go to besides the owner: a quote's deposit address and nothing else by default.
    pub recipients: HashSet<String>,
    pub programs: HashMap<String, String>,
    /// Most the transaction may pay in priority fees, lamports.
    pub max_priority_fee: u64,
    /// False where lookup-table contents come from someone the check cannot verify (the signer has no network): then
    /// money may not go to an account that only a lookup table names.
    pub trust_lookup_tables: bool,
    /// Someone else who pays the fee and signs first (Jupiter's gasless swap). Our wallet is then the second and last
    /// signer; SOL may go to someone else only out of the sponsor's own account — or back to the sponsor, up to
    /// `SPONSOR_REFUND`: the rent it fronted for the swap's temporary account.
    pub sponsor: Option<String>,
    /// The slippage a swap may allow before it asks in person afresh (the person's limit, Settings), basis points.
    pub max_slippage_bps: i64,
}

pub const SPONSOR_REFUND: u64 = 5_000_000;

impl Policy {
    /// The Swift defaults: no recipients, the known programs, 0.005 SOL of priority fee, lookup tables trusted, no sponsor,
    /// 3% slippage.
    pub fn new(owner: &str) -> Self {
        Policy { owner: owner.into(), recipients: HashSet::new(), programs: known_programs(), max_priority_fee: 5_000_000,
                 trust_lookup_tables: true, sponsor: None, max_slippage_bps: 300 }
    }
}

fn le16(d: &[u8], at: usize) -> u16 { u16::from_le_bytes(d[at..at + 2].try_into().expect("2 bytes")) }
fn le32(d: &[u8], at: usize) -> u32 { u32::from_le_bytes(d[at..at + 4].try_into().expect("4 bytes")) }
fn le64(d: &[u8], at: usize) -> u64 { u64::from_le_bytes(d[at..at + 8].try_into().expect("8 bytes")) }
fn kind(data: &[u8]) -> String { wallet::to_hex(&data[..data.len().min(8)]) }

/// Whether the transaction sends this wallet's SOL to the fee payer (rent it fronted, coming back): only then is the
/// payer someone the wallet pays. A JupiterZ maker isn't: the swap's prompt names it.
pub fn refunds_sponsor(m: &Message, accounts: &[String], owner: &str, sponsor: &str) -> bool {
    m.instructions.iter().any(|ix| {
        accounts.get(ix.program_index).is_some_and(|p| p == SYSTEM) && ix.data.len() >= 12 && le32(&ix.data, 0) == 2 && ix.accounts.len() > 1
            && ix.accounts[0] < accounts.len() && ix.accounts[1] < accounts.len()
            && accounts[ix.accounts[0]] == owner && accounts[ix.accounts[1]] == sponsor
    })
}

/// The Relay order ids the transaction's depository deposits name (bytes 16..<48 of each one's data).
pub fn relay_deposit_ids(m: &Message) -> Vec<Vec<u8>> {
    m.instructions.iter().filter_map(|ix| {
        (m.static_keys.get(ix.program_index).is_some_and(|p| p == RELAY_DEPOSITORY) && ix.data.len() >= 48).then(|| ix.data[16..48].to_vec())
    }).collect()
}

/// `account` is `owner`'s associated token account for `mint`, under SPL Token or Token-2022.
fn own_token_account(account: Option<&str>, owner: &str, mint: Option<&str>) -> bool {
    let (Some(account), Some(mint)) = (account, mint) else { return false };
    [TOKEN_PROGRAM, TOKEN_2022_PROGRAM].iter().any(|p| spl::associated_account(owner, mint, p).is_ok_and(|a| a == account))
}

/// The owner's plain obligation on a Kamino market: seeds [tag 0, id 0, owner, market, default, default]. An
/// instruction on it acts on that market, whatever market a lookup table names.
pub fn kamino_obligation(owner: &str, market: &str) -> Option<String> {
    let (o, m, p) = (base58::decode(owner).ok()?, base58::decode(market).ok()?, base58::decode(KAMINO_LEND).ok()?);
    let pda = spl::program_address(&[&[0], &[0], &o, &m, &[0; 32], &[0; 32]], &p).ok()?;
    Some(base58::encode(&pda))
}

/// `accounts`: the message's full account list (static keys plus lookup tables). `token_owners`: token account → its
/// owner wallet, for the owner's and the allowed recipients' accounts.
pub fn check(m: &Message, accounts: &[String], policy: &Policy, token_owners: &HashMap<String, String>) -> Verdict {
    let mut v = Verdict::default();
    let owner = policy.owner.as_str();
    let trust = policy.trust_lookup_tables;
    // An entry a lookup table names is whatever the app says it is: the signer can't read tables. Unless it trusts the
    // app with them, such an entry is never taken for any address (the 2026-10-24 review).
    let accounts: Vec<String> = if trust || accounts.len() <= m.static_keys.len() { accounts.to_vec() } else {
        let n = m.static_keys.len();
        m.static_keys.iter().cloned().chain((n..accounts.len()).map(|i| format!("lookup-table entry {}", i - n + 1))).collect()
    };
    if let Some(sponsor) = &policy.sponsor {
        if !(sponsor != owner && m.required_signatures == 2 && accounts.len() > 1 && accounts[0] == *sponsor && accounts[1] == owner) {
            v.problems.push(format!("Only {}… may pay the fee, and your wallet must be the only other signer.", prefix(sponsor, 6)));
            return v;
        }
        v.checks.push(format!("{}… pays the network fee, and your wallet is the only other signer.", prefix(sponsor, 6)));
    } else {
        if !(m.required_signatures == 1 && accounts.first().is_some_and(|a| a == owner)) {
            v.problems.push("Only your wallet may sign and pay for the transaction.".into());
            return v;
        }
        v.checks.push("Only your wallet signs and pays the fee.".into());
    }

    let mine = |a: &str| a == owner || token_owners.get(a).is_some_and(|o| o == owner);
    let static_keys: HashSet<&str> = m.static_keys.iter().map(String::as_str).collect();
    let seen = |a: &str| trust || static_keys.contains(a);
    let recipient = |a: &str| policy.recipients.contains(a) || token_owners.get(a).is_some_and(|o| policy.recipients.contains(o));
    let allowed = |a: &str| seen(a) && (mine(a) || recipient(a));
    let name = |a: &str| -> String {
        if a == owner { "your wallet".into() }
        else if token_owners.get(a).is_some_and(|o| o == owner) { "your token account".into() }
        else if recipient(a) { format!("approved address {}…", prefix(a, 6)) }
        else { a.into() }
    };

    let (mut unit_limit, mut unit_price, mut refunded): (u64, u64, u64) = (200_000, 0, 0);
    let (mut fills, mut maker_steps) = (0, 0);
    // The fill's taker side — the taker (0), what it gives from (2) and what it gets into (4) — found before the loop: a
    // maker's step never touches them, whatever the app claimed about whose they are.
    let taker_side: HashSet<usize> = m.instructions.iter()
        .find(|ix| m.static_keys.get(ix.program_index).is_some_and(|p| p == JUPITER_Z))
        .map(|ix| [0, 2, 4].iter().filter_map(|&i| ix.accounts.get(i).copied()).collect())
        .unwrap_or_default();

    for (n, ix) in m.instructions.iter().enumerate() {
        let n = n + 1;
        if ix.program_index >= m.static_keys.len() {
            v.problems.push(format!("Instruction {n}: the program is loaded from a lookup table."));
            continue;
        }
        // ponytail: Swift traps on an account list shorter than the static keys; refused here instead.
        let Some(program) = accounts.get(ix.program_index).map(String::as_str) else {
            v.problems.push(format!("Instruction {n}: the program isn't among the transaction's accounts."));
            continue;
        };
        // The wallet signs as account 1 and is a static key, so a step not given it can't act with its signature.
        if policy.sponsor.is_some() {
            if let Some(maker) = jupiter_z_maker_program(program) {
                if ix.accounts.iter().any(|&i| taker_side.contains(&i) || accounts.get(i).is_some_and(|a| a == owner || mine(a))) {
                    v.problems.push(format!("Instruction {n}: {maker} is given your wallet or one of its accounts."));
                    continue;
                }
                maker_steps += 1;
                if maker_steps == 1 {
                    v.checks.push(format!("The market maker sources the coin itself ({maker} and the like), given none of your swap's accounts."));
                }
                continue;
            }
        }
        let Some(program_name) = policy.programs.get(program) else {
            v.problems.push(format!("Instruction {n}: program {program} is not on the allowed list."));
            continue;
        };
        let account = |i: usize| ix.accounts.get(i).and_then(|&j| accounts.get(j)).map(String::as_str);
        let data = ix.data.as_slice();
        let tag = data.first().copied();

        match program {
            COMPUTE_BUDGET => {
                if tag == Some(2) && data.len() >= 5 { unit_limit = le32(data, 1) as u64 }
                if tag == Some(3) && data.len() >= 9 { unit_price = le64(data, 1) }
            }
            SYSTEM => {
                let to = account(1);
                let (Some(to), true) = (to, data.len() >= 12 && le32(data, 0) == 2) else {
                    v.problems.push(format!("Instruction {n}: System is allowed only for SOL transfers."));
                    continue;
                };
                let lamports = le64(data, 4);
                let sponsor = policy.sponsor.as_deref();
                if sponsor.is_some() && account(0) == sponsor {
                    v.checks.push(format!("{} SOL from the fee payer's own account.", sol_text(lamports)));
                } else if sponsor == Some(to) && account(0) == Some(owner) && seen(to) {
                    // ponytail: saturating where Swift's += would trap on two amounts past 2⁶⁴; either way it's refused.
                    refunded = refunded.saturating_add(lamports);
                    if refunded <= SPONSOR_REFUND {
                        v.checks.push(format!("{} SOL back to the fee payer for the account it opened. The simulation checks what you get.", sol_text(lamports)));
                    } else {
                        v.problems.push(format!("{} SOL back to the fee payer is more than the rent it fronted.", sol_text(refunded)));
                    }
                } else if allowed(to) {
                    v.checks.push(format!("{} SOL → {}.", sol_text(lamports), name(to)));
                    if account(0) == Some(owner) && !mine(to) { v.outflows.push(Outflow { asset: "SOL".into(), units: lamports, to: Some(to.into()) }) }
                } else {
                    v.problems.push(format!("SOL goes to someone else's address {to}."));
                }
            }
            // Token-2022 shares the classic program's instructions and their layout; its extensions' own instructions are
            // refused below like approvals.
            TOKEN_PROGRAM | TOKEN_2022_PROGRAM => match tag {
                Some(t @ (3 | 12)) => {
                    // Transfer(source, dest, authority), TransferChecked(source, mint, dest, authority)
                    let to = account(if t == 3 { 1 } else { 2 });
                    let (Some(to), true) = (to, data.len() >= if t == 3 { 9 } else { 10 }) else {
                        v.problems.push(format!("Instruction {n}: incomplete token transfer."));
                        continue;
                    };
                    let raw = le64(data, 1);
                    let known = if t == 12 { account(1).and_then(mint_name) } else { None };
                    let amount = known.map_or(format!("{raw} units of a token"), |(symbol, d)| format!("{} {symbol}", units(raw, d)));
                    if allowed(to) { v.checks.push(format!("{amount} → {}.", name(to))) }
                    else { v.problems.push(format!("{amount} go to someone else's account {to}.")) }
                    // The mint: named by TransferChecked, else the one this wallet's own source account is for.
                    let source = account(0);
                    let mint = if t == 12 { account(1) } else { accounts.iter().map(String::as_str).find(|m| own_token_account(source, owner, Some(m))) };
                    // Counted when this wallet signs for it (its authority) or its own account pays, whatever the engine claims.
                    if (account(if t == 3 { 2 } else { 3 }) == Some(owner) || source.is_some_and(|s| mine(s))) && !mine(to) {
                        v.outflows.push(Outflow { asset: mint.unwrap_or("?").into(), units: raw, to: Some(token_owners.get(to).cloned().unwrap_or_else(|| to.into())) });
                    }
                }
                Some(9) => {
                    // CloseAccount(account, destination, owner): the rent must come back to us.
                    match account(1) {
                        Some(to) if mine(to) && seen(to) => v.checks.push(format!("Temporary account closes, remainder → {}.", name(to))),
                        _ => v.problems.push("Closing the account sends the remainder to someone else's address.".into()),
                    }
                }
                Some(17) => v.checks.push("wSOL sync.".into()),
                // Approve, SetAuthority and the rest hand control of tokens to someone else.
                other => v.problems.push(format!("Instruction {n}: token operation {} is forbidden (approvals, owner changes and the like).",
                                                 other.map_or("?".to_string(), |t| t.to_string()))),
            },
            ASSOCIATED_TOKEN_PROGRAM => {
                let Some(holder) = account(2) else {
                    v.problems.push(format!("Instruction {n}: incomplete account creation."));
                    continue;
                };
                if holder == owner || (policy.recipients.contains(holder) && seen(holder)) {
                    v.checks.push(format!("Creates a token account owned by {}.", name(holder)));
                } else {
                    v.problems.push(format!("Creates a token account for someone else's wallet {holder}."));
                }
            }
            LOOKUP_TABLE_PROGRAM => {
                // 0 Create(table, authority, payer, system), 2 Extend(table, authority, payer, system): our table only.
                if !matches!(tag, Some(0 | 2)) || account(1) != Some(owner) {
                    v.problems.push(format!("Instruction {n}: the lookup table is not yours, or the operation is not create/extend."));
                    continue;
                }
                v.checks.push(if tag == Some(0) { "Creates your lookup table (rent in SOL).".into() } else { "Extends your lookup table.".into() });
            }
            RELAY_DEPOSITORY => {
                // Only deposits, with our wallet as the depositor and the sender; whom the order pays the signer checks
                // from the order itself.
                let k = &data[..data.len().min(8)];
                if !((k == RELAY_DEPOSIT_TOKEN || k == RELAY_DEPOSIT_NATIVE) && data.len() >= 48) {
                    v.problems.push(format!("Instruction {n}: only deposits are allowed on Relay."));
                    continue;
                }
                if account(2) == Some(owner) && account(1) == Some(owner) {
                    v.checks.push(format!("Relay deposit: {} units from your wallet, for the order the signer checked.", le64(data, 8)));
                } else {
                    v.problems.push("Relay deposit is not from your wallet, or names someone else as the depositor.".into());
                }
            }
            JUPITER_LEND => {
                let k = kind(data);
                let deposit = JUPITER_LEND_DEPOSITS.contains(&k.as_str());
                if !deposit && !JUPITER_LEND_WITHDRAWALS.contains(&k.as_str()) {
                    v.problems.push(format!("Instruction {n}: a Jupiter Lend instruction ihatecrypto doesn't use ({k})."));
                    continue;
                }
                let to = account(2);
                if !(account(0) == Some(owner) && to.is_some_and(seen) && own_token_account(to, owner, account(if deposit { 6 } else { 5 }))) {
                    v.problems.push(format!("Instruction {n}: Jupiter Lend would {} someone else's account.", if deposit { "credit the deposit to" } else { "pay" }));
                    continue;
                }
                v.checks.push(format!("Jupiter Lend {} → your account. The balance simulation checks the amount.",
                                      if deposit { "deposit: the shares" } else { "withdrawal: the money" }));
            }
            JUPITER_DCA => {
                let k = kind(data);
                let ours = if k == JUPITER_DCA_OPEN {
                    // dca 0, user 1, payer 2, inputMint 3, outputMint 4, userAta 5, inAta 6, outAta 7: the order's own ATAs.
                    account(1) == Some(owner) && account(2) == Some(owner) && own_token_account(account(5), owner, account(3))
                        && account(0).is_some_and(|dca| own_token_account(account(6), dca, account(3)) && own_token_account(account(7), dca, account(4)))
                } else {
                    // user 0, dca 1, inputMint 2, outputMint 3, …, userInAta 6, userOutAta 7: what's left comes back here.
                    k == JUPITER_DCA_CLOSE && account(0) == Some(owner) && own_token_account(account(6), owner, account(2))
                        && own_token_account(account(7), owner, account(3))
                        && [6, 7].iter().all(|&i| trust || account(i).is_some_and(|a| static_keys.contains(a)))
                };
                if !ours {
                    v.problems.push(format!("Instruction {n}: a Jupiter DCA instruction that isn't your own order's opening or closing ({k})."));
                    continue;
                }
                v.checks.push(if k == JUPITER_DCA_OPEN { "Jupiter DCA order of yours: what it buys is yours.".into() }
                              else { "Closes your Jupiter DCA order: what's left comes back to you.".into() });
            }
            KAMINO_LEND | KAMINO_VAULT | KAMINO_FARMS => {
                let k = kind(data);
                let lend = kamino_lend_allowed(&k);
                let refused = match program {
                    KAMINO_VAULT => !KAMINO_VAULT_ALLOWED.contains(&k.as_str()),
                    KAMINO_FARMS => !KAMINO_FARMS_ALLOWED.contains(&k.as_str()),
                    _ => lend.is_none(),
                };
                if refused {
                    v.problems.push(format!("Instruction {n}: a {program_name} instruction ihatecrypto doesn't use ({k}). It could pay someone else."));
                    continue;
                }
                if program == KAMINO_LEND {
                    if let Some(Some(at)) = lend {
                        let own = account(at).is_some_and(|o| static_keys.contains(o)
                            && KAMINO_MARKETS.iter().any(|market| kamino_obligation(owner, market).as_deref() == Some(o)));
                        if !own {
                            v.problems.push(format!("Instruction {n}: the Kamino position isn't your own on Kamino's main or Backpack market."));
                            continue;
                        }
                    }
                }
                // init_user_metadata's referrer (account 3) would take a share of the fees: none, which Anchor writes as the program.
                if k == "75a9b045c5170fa2" && account(3) != Some(KAMINO_LEND) {
                    v.problems.push(format!("Instruction {n}: Kamino's account setup names a referrer."));
                    continue;
                }
                // A borrow or a withdrawal pays this wallet's own account, and the prompt says how much, read here.
                if program == KAMINO_LEND {
                    if let Some((to, borrow)) = kamino_payout(&k) {
                        let Some(to) = account(to).filter(|t| data.len() >= 16 && static_keys.contains(t) && mine(t)) else {
                            v.problems.push(format!("Instruction {n}: Kamino would pay an account that isn't yours."));
                            continue;
                        };
                        let raw = le64(data, 8);
                        let what = MINT_NAMES.iter().find(|m| own_token_account(Some(to), owner, Some(m.0)))
                            .map_or(format!("{raw} units"), |m| format!("{} {}", units(raw, m.2), m.1));
                        v.checks.push(if borrow { format!("Kamino lends {what} → your account, owed back to Kamino.") }
                                      else { format!("Kamino withdrawal: up to {what} in Kamino's shares, each worth one or more → your account.") });
                    }
                }
                v.checks.push(format!("Calls {program_name} on your own position: it pays only you, and a repay pays only your debt. The balance simulation checks the amount."));
            }
            JUPITER_Z => {
                // One fill, this wallet the taker: what it gives comes from its own account for the input mint (or, with
                // none named — the program's id — as SOL), and what it gets lands in its own account for the output mint
                // (or as SOL). The amounts go on the prompt; the simulation checks them.
                fills += 1;
                let parts = (account(2), account(6), account(4), account(8));
                let (Some(source), Some(in_mint), Some(out), Some(mint)) = parts else {
                    v.problems.push(format!("Instruction {n}: a JupiterZ instruction that isn't the one fill with your wallet as the taker."));
                    continue;
                };
                if !(fills == 1 && kind(data) == JUPITER_Z_FILL && data.len() >= 24 && account(0) == Some(owner)) {
                    v.problems.push(format!("Instruction {n}: a JupiterZ instruction that isn't the one fill with your wallet as the taker."));
                    continue;
                }
                let own = |a: &str, m: &str| (a == JUPITER_Z && m == WRAPPED_SOL) || ((mine(a) || own_token_account(Some(a), owner, Some(m))) && seen(a));
                if !own(source, in_mint) {
                    v.problems.push(format!("The JupiterZ fill takes from {source}, not your account for what you swap."));
                    continue;
                }
                if !own(out, mint) {
                    v.problems.push(format!("The JupiterZ fill pays {out}, not your wallet."));
                    continue;
                }
                // A mint a lookup table names is unread here: the wallet's own account for it says which it is.
                let named = |a: &str, m: &str| -> String {
                    MINT_NAMES.iter().find(|x| own_token_account(Some(a), owner, Some(x.0))).map_or(m.to_string(), |x| x.0.to_string())
                };
                v.outflows.push(Outflow { asset: named(source, in_mint), units: le64(data, 8), to: None });
                v.swaps = true;
                v.checks.push(format!("JupiterZ fill: {} for {} → {}. The balance simulation checks it.",
                                      mint_units(le64(data, 8), &named(source, in_mint)), mint_units(le64(data, 16), &named(out, mint)),
                                      if out == JUPITER_Z { "your wallet, as SOL" } else { "your account" }));
            }
            JUPITER_SWAP => {
                // Exact-in v2 swaps only: in_amount, quoted_out_amount and slippage_bps open their data, so what the signer
                // reads is what the program reads, and the prompt says what goes in and the least that comes out (the
                // 2026-11-08 review: a program that took over the app could swap at a bad rate inside a session).
                // v1 swaps put the amounts after the route, where a site could also append a made-up quote (the
                // 2026-10-22 review): refused, and ihatecrypto asks Jupiter for v2. The output account (its «instead» one
                // when set: the program's own id when not) must be this wallet's own (the 2026-10-24 review).
                let k = kind(data);
                let found = jupiter_route(&k).filter(|l| data.len() >= l.args + 22)
                    .and_then(|l| Some((account(l.to)?, account(l.mint)?, account(l.in_mint)?, l)));
                let Some((mut to, mint, in_mint, layout)) = found else {
                    v.problems.push(if JUPITER_UNREAD.contains(&k.as_str()) {
                        format!("Instruction {n}: an older Jupiter swap whose amounts the signer can't read: it signs only Jupiter's v2 swaps.")
                    } else {
                        format!("Instruction {n}: a Jupiter instruction that isn't one of its swaps ({k}).")
                    });
                    continue;
                };
                if let Some(instead) = layout.instead.and_then(account).filter(|i| *i != JUPITER_SWAP) { to = instead }
                // This wallet's own token account for the mint, by the claim the signer proved or derived here.
                let ours = mine(to) || own_token_account(Some(to), owner, Some(mint));
                if !(ours && seen(to)) {
                    v.problems.push(format!("The Jupiter swap pays {to}, not your wallet."));
                    continue;
                }
                let (in_amount, quoted) = (le64(data, layout.args), le64(data, layout.args + 8));
                let (slippage, fee) = (le16(data, layout.args + 16) as i64, le16(data, layout.args + 18) as i64);
                if slippage > 10_000 || fee > 1_000 {
                    v.problems.push(format!("Instruction {n}: a Jupiter swap with slippage over 100% or a fee over 10%."));
                    continue;
                }
                let least = (quoted as u128 * (10_000 - slippage) as u128 / 10_000) as u64;
                // Each mint by this wallet's own account for it where the message's mint is one a lookup table names (the
                // signer can't read tables): «3 USDC», not «3000000 units of lookup-table entry 21».
                let named = |a: Option<&str>, m: &str| -> String {
                    let Some(a) = a else { return m.into() };
                    if own_token_account(Some(a), owner, Some(m)) { return m.into() }
                    MINT_NAMES.iter().find(|x| own_token_account(Some(a), owner, Some(x.0))).map_or(m.to_string(), |x| x.0.to_string())
                };
                let (given, got) = (named(account(layout.source), in_mint), named(Some(to), mint));
                v.checks.push(format!("Jupiter swap: {} → at least {}, your account (slippage {}{})", mint_units(in_amount, &given), mint_units(least, &got),
                                      percent(slippage), if fee > 0 { format!(", a {} fee to the site", percent(fee)) } else { String::new() }));
                if slippage > policy.max_slippage_bps {
                    v.ask_afresh.push(format!("slippage {} is over your {}", percent(slippage), percent(policy.max_slippage_bps)));
                }
                if fee > 0 { v.ask_afresh.push(format!("the site takes a {} fee", percent(fee))) }
                // Counted as that coin only when it leaves this wallet's own account for it; else unknown (the automation
                // wallet then asks), where a coin named only by the message could be taken for dollars.
                let counted = account(layout.source).is_some_and(|a| own_token_account(Some(a), owner, Some(&given)));
                v.outflows.push(Outflow { asset: if counted { given } else { "?".into() }, units: in_amount, to: None });
                v.swaps = true;
            }
            _ => v.checks.push(format!("Calls {program_name}. The balance simulation checks the result.")),
        }
    }
    if maker_steps > 0 && fills != 1 { v.problems.push("A market maker's steps come without its one JupiterZ fill.".into()) }
    // A JupiterZ maker fronts no rent: SOL «back» to the fee payer beside a fill would be the maker skimming the wallet.
    if fills > 0 && refunded > 0 {
        v.problems.push(format!("{} SOL to the market maker: a JupiterZ fill takes no SOL from your wallet.", sol_text(refunded)));
    }
    // Both numbers are the transaction's: a product past 64 bits is refused, not trapped on.
    match unit_limit.checked_mul(unit_price) {
        None => v.problems.push("Priority fee out of range.".into()),
        Some(p) if p / 1_000_000 > policy.max_priority_fee => {
            v.problems.push(format!("Priority fee up to {} SOL is too high.", sol_text(p / 1_000_000)));
        }
        _ => {}
    }
    v
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRule {
    pub label: String,
    pub before: i64,
    pub after: i64,
    /// The change must be at least this (negative: at most this much may leave).
    pub at_least: i64,
    /// And at most this, when set: a borrow or a withdrawal brings no more than the card said.
    pub at_most: Option<i64>,
}

/// Compares simulated balances with what the person approved: this catches a route that pays someone else.
pub fn balances(rules: &[BalanceRule]) -> Verdict {
    let mut v = Verdict::default();
    for r in rules {
        // ponytail: i128, where Swift's Int64 subtraction would trap on balances near ±2⁶³; a real balance never is.
        let delta = r.after as i128 - r.before as i128;
        if delta < r.at_least as i128 {
            v.problems.push(format!("Simulation: {} changes by {delta}, under the minimum allowed, {}.", r.label, r.at_least));
        } else if let Some(most) = r.at_most.filter(|m| delta > *m as i128) {
            v.problems.push(format!("Simulation: {} changes by {delta}, over the most allowed, {most}.", r.label));
        } else {
            v.checks.push(format!("Simulation: {} {}{delta}.", r.label, if delta >= 0 { "+" } else { "" }));
        }
    }
    v
}

/// Rights a transaction hands out that no balance shows (research E-1): read from the simulation, the wallet still
/// belongs to the System program and holds no data, and each of our token accounts in `before` keeps its owner and gets
/// no new close authority, no new delegate and no more for its delegate to spend. A change is refused, never a field
/// already set. A token account of the wallet's in `after` that isn't in `before` was opened by this transaction: it
/// starts with no one else's rights. An account missing from `after` was closed. `names`: a token account's coin.
/// ponytail: the wallet and its token accounts only (variant A); stake, nonce and lookup-table authorities and
/// Token-2022's extensions aren't compared.
pub fn rights_problems(owner: &str, before: &HashMap<String, AccountRights>, after: &HashMap<String, AccountRights>,
                       names: &HashMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(w) = after.get(owner) {
        if w.program != SYSTEM {
            let to = if w.program.is_empty() { "a program the simulation doesn't name".to_string() } else { format!("the program {}", w.program) };
            out.push(format!("Your wallet would be handed to {to}: from then on that program, not your key, decides what leaves it."));
        }
        if w.space > 0 { out.push(format!("Your wallet would get {} bytes of data and could no longer pay Solana's fee.", w.space)) }
    }
    let mut all: BTreeMap<&String, AccountRights> = after.iter()
        .filter(|(k, a)| !before.contains_key(*k) && a.owner.as_deref() == Some(owner))
        .map(|(k, _)| (k, AccountRights::default())).collect();
    all.extend(before.iter().map(|(k, r)| (k, r.clone())));
    for (address, was) in all {
        let Some(now) = after.get(address) else { continue };
        let Some(holder) = &now.owner else { continue };
        let short = format!("{}…{}", prefix(address, 4), suffix(address, 4));
        let coin = names.get(address).cloned().unwrap_or_else(|| format!("tokens in {short}"));
        let account = names.get(address).map_or(format!("token account {short}"), |n| format!("{n} account"));
        if holder != owner { out.push(format!("Your {account} would belong to {holder}.")) }
        if let Some(c) = &now.close_authority {
            if c != owner && was.close_authority.as_ref() != Some(c) { out.push(format!("{c} could close your {account} and take its rent.")) }
        }
        if let Some(d) = &now.delegate {
            if d != owner && (was.delegate.as_ref() != Some(d) || now.delegated_amount > was.delegated_amount) {
                out.push(format!("{d} could spend your {coin}."));
            }
        }
    }
    out
}

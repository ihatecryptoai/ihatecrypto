//! SPL tokens (`SPL.swift`): the address of a token account, program addresses, a checked transfer; .sol names
//! (`SNS.swift`) as far as they are read offline.

use curve25519_dalek::edwards::CompressedEdwardsY;
use sha2::{Digest, Sha256};
use wallet::Result;
use wallet::keys::base58;

use crate::{COMPUTE_BUDGET_PROGRAM, SYSTEM_PROGRAM, compact, decode, fail, key};

pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const TOKEN_2022_PROGRAM: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// Whether 32 bytes decode to a point on edwards25519, as curve25519-dalek's decompress decides (Swift copies it).
pub fn is_on_curve(bytes: &[u8]) -> bool {
    <[u8; 32]>::try_from(bytes).is_ok_and(|b| CompressedEdwardsY(b).decompress().is_some())
}

/// Solana's find_program_address: the first bump from 255 down whose hash is not a curve point.
pub fn program_address(seeds: &[&[u8]], program: &[u8]) -> Result<[u8; 32]> {
    for bump in (0..=255u8).rev() {
        let mut h = Sha256::new();
        for seed in seeds { h.update(seed) }
        h.update([bump]);
        h.update(program);
        h.update(b"ProgramDerivedAddress");
        let hash: [u8; 32] = h.finalize().into();
        if !is_on_curve(&hash) { return Ok(hash) }
    }
    fail("No program address found")
}

/// The associated token account of `owner` for `mint`: a program-derived address, off the ed25519 curve. A Token-2022
/// mint's accounts derive from that program.
pub fn associated_account(owner: &str, mint: &str, program: &str) -> Result<String> {
    let (o, p, m) = (decode(owner)?, decode(program)?, decode(mint)?);
    Ok(base58::encode(&program_address(&[&o, &p, &m], &decode(ASSOCIATED_TOKEN_PROGRAM)?)?))
}

fn budget(m: &mut Vec<u8>, index: u8, micro_lamports_per_cu: u64) {
    m.push(index);
    m.extend(compact(0));
    m.extend(compact(9));
    m.push(3);
    m.extend(micro_lamports_per_cu.to_le_bytes());
}

fn instruction(m: &mut Vec<u8>, program: u8, accounts: &[u8], data: &[u8]) {
    m.push(program);
    m.extend(compact(accounts.len()));
    m.extend(accounts);
    m.extend(compact(data.len()));
    m.extend(data);
}

fn message(header: [u8; 3], keys: &[Vec<u8>], blockhash: &str, count: usize) -> Result<Vec<u8>> {
    let mut m = header.to_vec();
    m.extend(compact(keys.len()));
    for k in keys { m.extend(k) }
    m.extend(decode(blockhash)?);
    m.extend(compact(count));
    Ok(m)
}

/// Creates our own token account for `mint` if it does not exist yet (idempotent).
pub fn create_own_account(private_key: &[u8], mint: &str, blockhash: &str, micro_lamports_per_cu: u64) -> Result<(String, Vec<u8>)> {
    let signer = key(private_key)?;
    let payer = signer.verifying_key().to_bytes();
    let keys = vec![
        payer.to_vec(),                                                                                       // 0 signer, writable
        decode(&associated_account(&base58::encode(&payer), mint, TOKEN_PROGRAM)?)?,                          // 1 new account
        decode(mint)?,                                                                                        // 2 readonly
        SYSTEM_PROGRAM.to_vec(),                                                                              // 3
        decode(TOKEN_PROGRAM)?,                                                                               // 4
        decode(ASSOCIATED_TOKEN_PROGRAM)?,                                                                    // 5
        decode(COMPUTE_BUDGET_PROGRAM)?,                                                                      // 6
    ];
    let mut m = message([1, 0, 5], &keys, blockhash, 2)?;
    budget(&mut m, 6, micro_lamports_per_cu);
    instruction(&mut m, 5, &[0, 1, 0, 2, 3, 4], &[1]);
    Ok(crate::signed(&signer, m))
}

/// Creates the recipient's token account if missing (idempotent), then TransferChecked, with a priority fee.
pub fn transfer(private_key: &[u8], owner: &str, mint: &str, amount: u64, decimals: u8, blockhash: &str, micro_lamports_per_cu: u64) -> Result<(String, Vec<u8>)> {
    let signer = key(private_key)?;
    let payer = base58::encode(&signer.verifying_key().to_bytes());
    let m = transfer_message(&payer, owner, mint, amount, decimals, blockhash, micro_lamports_per_cu, TOKEN_PROGRAM, false)?;
    Ok(crate::signed(&signer, m))
}

/// The same transfer as an unsigned message, for a signer that holds the key elsewhere. `program`: the mint's own,
/// SPL Token or Token-2022 — both accounts and the TransferChecked derive from it. `close`: our token account closes
/// after the transfer (all of it went), its rent back to the wallet.
#[allow(clippy::too_many_arguments)]
pub fn transfer_message(payer: &str, owner: &str, mint: &str, amount: u64, decimals: u8, blockhash: &str,
                        micro_lamports_per_cu: u64, program: &str, close: bool) -> Result<Vec<u8>> {
    let payer_key = decode(payer)?;
    if owner == payer { return fail("Transfer to yourself") }
    let keys = vec![
        payer_key,                                                         // 0 signer, writable
        decode(&associated_account(payer, mint, program)?)?,               // 1 our token account
        decode(&associated_account(owner, mint, program)?)?,               // 2 their token account
        decode(owner)?,                                                    // 3 readonly from here on
        decode(mint)?,                                                     // 4
        SYSTEM_PROGRAM.to_vec(),                                           // 5
        decode(program)?,                                                  // 6
        decode(ASSOCIATED_TOKEN_PROGRAM)?,                                 // 7
        decode(COMPUTE_BUDGET_PROGRAM)?,                                   // 8
    ];
    let mut m = message([1, 0, 6], &keys, blockhash, if close { 4 } else { 3 })?;
    budget(&mut m, 8, micro_lamports_per_cu);
    instruction(&mut m, 7, &[0, 2, 3, 4, 5, 6], &[1]); // CreateIdempotent
    let mut data = vec![12];
    data.extend(amount.to_le_bytes());
    data.push(decimals);
    instruction(&mut m, 6, &[1, 4, 2, 0], &data); // TransferChecked
    if close { instruction(&mut m, 6, &[1, 0, 0], &[9]) } // CloseAccount(ours, → wallet, owner)
    Ok(m)
}

/// Solana Name Service names (toly.sol): the name's account is a program address of the Name Service from
/// sha256("SPL Name Service" + name) under the .sol root, and its data holds the parent and the owner.
pub mod sns {
    use super::*;

    pub const PROGRAM: &str = "namesLPneVptA9Z5rqUDD9tMTWEJwofgaYwp8cawRkX";
    pub const ROOT: &str = "58PwtjSDuFHuUkYjH9BYnnQKHfwo9reZhC2zMJv9JPkx";

    /// Plain ASCII names only, as for ENS: `[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.sol`.
    pub fn is_name(name: &str) -> bool {
        let Some(label) = name.strip_suffix(".sol") else { return false };
        let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
        let bytes = label.as_bytes();
        !bytes.is_empty() && ok(bytes[0]) && ok(bytes[bytes.len() - 1]) && bytes.iter().all(|&b| ok(b) || b == b'-')
    }

    /// The name's account: find_program_address([sha256(prefix + name), 32 zero bytes (no class), .sol root], program).
    pub fn account(name: &str) -> Result<String> {
        let label = name.strip_suffix(".sol").unwrap_or(name);
        let hashed: [u8; 32] = Sha256::digest(format!("SPL Name Service{label}")).into();
        let root = decode(ROOT)?;
        Ok(base58::encode(&program_address(&[&hashed, &[0; 32], &root], &decode(PROGRAM)?)?))
    }

    /// The owner, from the name account's program and data as getAccountInfo returns them. An owner off the curve is
    /// a program (a tokenized domain's escrow), never a wallet to pay: refused.
    pub fn owner(name: &str, program: Option<&str>, data: Option<&[u8]>) -> Result<String> {
        let raw = match (program, data) {
            (Some(PROGRAM), Some(raw)) if raw.len() >= 96 => raw,
            _ => return fail(format!("{name} isn't registered.")),
        };
        if base58::encode(&raw[..32]) != ROOT { return fail(format!("{name}'s account isn't under .sol.")) }
        let owner = &raw[32..64];
        if !is_on_curve(owner) {
            return fail(format!("{name} is held by a program (a tokenized domain), not a wallet. Paste the address itself."));
        }
        Ok(base58::encode(owner))
    }

    pub fn not_a_name(name: &str) -> String {
        format!("{name} isn't a .sol name ihatecrypto reads: plain letters, digits and hyphens.")
    }
}

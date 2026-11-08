//! Solana read and built offline (`Chains/Solana`): messages (legacy and v0; v1 refused), transactions, SPL token
//! accounts and transfers, program addresses, .sol names, and the pure halves of the RPC answers the checks read.
//! The RPC client itself belongs to the engine, so the signer links no network code.

pub mod message;
pub mod spl;

use ed25519_dalek::{Signer, SigningKey};
use wallet::keys::base58;
use wallet::{Result, WalletError};

pub(crate) fn fail<T>(m: impl Into<String>) -> Result<T> { Err(WalletError(m.into())) }

pub const SYSTEM_PROGRAM: [u8; 32] = [0; 32]; // 11111111111111111111111111111111
pub const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";
/// The genesis hash of Solana mainnet: a node that answers another is another network.
pub const MAINNET_GENESIS: &str = "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d";
pub const PUBLIC_NODE: &str = "https://api.mainnet-beta.solana.com";

pub(crate) fn decode(s: &str) -> Result<Vec<u8>> { base58::decode(s) }

pub(crate) fn key(private_key: &[u8]) -> Result<SigningKey> {
    let Ok(secret) = <[u8; 32]>::try_from(private_key) else { return fail("An ed25519 private key is 32 bytes.") };
    Ok(SigningKey::from_bytes(&secret))
}

/// Solana's compact-u16.
pub fn compact(n: usize) -> Vec<u8> {
    let (mut value, mut out) = (n, Vec::new());
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value > 0 { byte |= 0x80 }
        out.push(byte);
        if value == 0 { return out }
    }
}

/// Solana's compact-u16, read as the network reads it: at most three bytes, at most 65,535, no padded form. Anything
/// else is refused, so every count is 0…65,535 and no length math after it can overflow.
pub fn read_compact(data: &[u8], offset: &mut usize) -> Result<usize> {
    let mut value = 0usize;
    for shift in [0, 7, 14] {
        let Some(&byte) = data.get(*offset) else { return fail("Truncated length") };
        *offset += 1;
        if byte == 0 && shift != 0 { return fail("Bad length") }
        value |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return if value <= 0xFFFF { Ok(value) } else { fail("Bad length") };
        }
    }
    fail("Bad length")
}

fn signed(key: &SigningKey, message: Vec<u8>) -> (String, Vec<u8>) {
    let signature = key.sign(&message).to_bytes();
    let mut wire = compact(1);
    wire.extend(signature);
    wire.extend(message);
    (base58::encode(&signature), wire)
}

/// Native SOL transfer with a small priority fee, as a signed legacy transaction: (signature, wire).
pub fn transfer(private_key: &[u8], recipient: &str, lamports: u64, blockhash: &str, micro_lamports_per_cu: u64) -> Result<(String, Vec<u8>)> {
    let signer = key(private_key)?;
    let from = signer.verifying_key().to_bytes();
    let to = decode(recipient)?;
    if to.len() != 32 { return fail(format!("Invalid Solana address: {recipient}")) }
    if to == from { return fail("Transfer to yourself") }
    let mut m = vec![1, 0, 2]; // 1 signer, 0 readonly signed, 2 readonly unsigned
    m.extend(compact(4));
    m.extend(from);
    m.extend(&to);
    m.extend(SYSTEM_PROGRAM);
    m.extend(decode(COMPUTE_BUDGET_PROGRAM)?);
    m.extend(decode(blockhash)?);
    m.extend(compact(2));
    // ComputeBudget.SetComputeUnitPrice
    m.extend([3]);
    m.extend(compact(0));
    m.extend(compact(9));
    m.push(3);
    m.extend(micro_lamports_per_cu.to_le_bytes());
    // System.Transfer: u32 index 2, u64 lamports
    m.extend([2]);
    m.extend(compact(2));
    m.extend([0, 1]);
    m.extend(compact(12));
    m.extend(2u32.to_le_bytes());
    m.extend(lamports.to_le_bytes());
    Ok(signed(&signer, m))
}

/// Signs a transaction someone else built (Jupiter). Refuses unless our key is the fee payer, account 0 — or, with a
/// `sponsor` (a 32-byte public key) paying, unless it is account 0 and our key the second and last signer.
pub fn sign(wire: &[u8], private_key: &[u8], sponsor: Option<&[u8]>) -> Result<(String, Vec<u8>)> {
    let signer = key(private_key)?;
    let (message, slot) = unsigned(wire, &signer.verifying_key().to_bytes(), sponsor)?;
    let signature = signer.sign(&message).to_bytes();
    let mut out = wire.to_vec();
    out[slot].copy_from_slice(&signature);
    Ok((base58::encode(&signature), out))
}

/// The message our key signs and where in `wire` its 64-byte signature goes. Refuses unless `payer` (a 32-byte public
/// key) is the fee payer, account 0, and the only signer; with a `sponsor`, unless the sponsor is account 0 and
/// `payer` account 1 of exactly two signers.
pub fn unsigned(wire: &[u8], payer: &[u8], sponsor: Option<&[u8]>) -> Result<(Vec<u8>, std::ops::Range<usize>)> {
    let incomplete = || fail("Incomplete transaction");
    let mut offset = 0;
    let signature_count = read_compact(wire, &mut offset)?;
    let signatures_start = offset;
    let message_start = offset + signature_count * 64;
    if signature_count < 1 || wire.len() <= message_start { return incomplete() }
    let message = &wire[message_start..];
    let mut m = 0;
    if message[m] & 0x80 != 0 { m += 1 } // versioned (v0) prefix
    if m >= message.len() { return incomplete() }
    let signers = if sponsor.is_none() { 1 } else { 2 };
    if message[m] as usize != signers || signature_count != signers {
        return fail(format!("The transaction needs {} signatures, but {} expected", message[m],
                            if sponsor.is_none() { "only ours was" } else { "the fee payer's and ours were" }));
    }
    let mut key_offset = m + 3;
    if read_compact(message, &mut key_offset)? < signers || key_offset + 32 * signers > message.len() { return incomplete() }
    let key = |i: usize| &message[key_offset + 32 * i..key_offset + 32 * (i + 1)];
    match sponsor {
        Some(sponsor) => if key(0) != sponsor || sponsor == payer || key(1) != payer {
            return fail("The fee payer isn't the sponsor, or our wallet isn't the second signer");
        },
        None => if key(0) != payer { return fail("The fee payer isn't our wallet") },
    }
    let start = signatures_start + 64 * (signers - 1);
    Ok((message.to_vec(), start..start + 64))
}

/// An instruction as protocols hand it over (Relay's quote steps, for example).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub program: String,
    /// (pubkey, is_signer, is_writable)
    pub accounts: Vec<(String, bool, bool)>,
    pub data: Vec<u8>,
}

/// A one-signer transaction with an empty signature slot, for a signer in another process.
pub fn unsigned_transaction(message: &[u8]) -> Vec<u8> {
    let mut wire = compact(1);
    wire.extend([0; 64]);
    wire.extend(message);
    wire
}

/// Compiles instructions into a signed legacy transaction, fee payer first, with a priority fee.
pub fn compile(instructions: &[Instruction], private_key: &[u8], blockhash: &str, micro_lamports_per_cu: u64) -> Result<(String, Vec<u8>)> {
    let signer = key(private_key)?;
    let message = compile_message(instructions, &base58::encode(&signer.verifying_key().to_bytes()), blockhash, micro_lamports_per_cu)?;
    Ok(signed(&signer, message))
}

/// The legacy message for `instructions`, fee payer first, with a priority fee.
/// ponytail: no lookup tables, so every account is static; fine up to ~30 accounts (1232-byte limit).
pub fn compile_message(instructions: &[Instruction], payer: &str, blockhash: &str, micro_lamports_per_cu: u64) -> Result<Vec<u8>> {
    let mut budget_data = vec![3];
    budget_data.extend(micro_lamports_per_cu.to_le_bytes());
    let budget = Instruction { program: COMPUTE_BUDGET_PROGRAM.into(), accounts: vec![], data: budget_data };
    let all: Vec<&Instruction> = std::iter::once(&budget).chain(instructions).collect();

    // Merge every account's flags; programs are readonly non-signers.
    let mut order: Vec<String> = vec![payer.into()];
    let mut flags: std::collections::HashMap<String, (bool, bool)> = [(payer.to_string(), (true, true))].into();
    let mut add = |k: &str, signer: bool, writable: bool| {
        match flags.get_mut(k) {
            Some(f) => *f = (f.0 || signer, f.1 || writable),
            None => { flags.insert(k.into(), (signer, writable)); order.push(k.into()) }
        }
    };
    for ix in &all {
        for (k, s, w) in &ix.accounts { add(k, *s, *w) }
        add(&ix.program, false, false);
    }
    let signers: Vec<&String> = order.iter().filter(|k| flags[*k].0).collect();
    if signers != [payer] {
        let others: Vec<String> = signers.iter().filter(|k| **k != payer).map(|k| format!("\"{k}\"")).collect();
        return fail(format!("The instructions need other signers: [{}]", others.join(", ")));
    }
    let keys: Vec<&String> = std::iter::once(&order[0])
        .chain(order.iter().filter(|k| *k != payer && flags[*k].1))
        .chain(order.iter().filter(|k| *k != payer && !flags[*k].1))
        .collect();
    let readonly_unsigned = keys.iter().filter(|k| !flags[**k].1).count();
    let index: std::collections::HashMap<&String, u8> = keys.iter().enumerate().map(|(i, k)| (*k, i as u8)).collect();

    let mut m = vec![1, 0, readonly_unsigned as u8];
    m.extend(compact(keys.len()));
    for k in &keys { m.extend(decode(k)?) }
    m.extend(decode(blockhash)?);
    m.extend(compact(all.len()));
    for ix in &all {
        m.push(index[&ix.program]);
        m.extend(compact(ix.accounts.len()));
        m.extend(ix.accounts.iter().map(|(k, _, _)| index[k]));
        m.extend(compact(ix.data.len()));
        m.extend(&ix.data);
    }
    if m.len() + 65 > 1232 { return fail("The transaction is over 1232 bytes") }
    Ok(m)
}

/// The v0 message for `instructions` as they come (their own compute budget included), fee payer first, with accounts
/// that `tables` (lookup table → its addresses) hold loaded through them — except signers, programs and `keep_static`
/// (the wallet's own accounts: the signer lets money land only in an account the message names itself).
pub fn compile_v0(instructions: &[Instruction], payer: &str, blockhash: &str, tables: &[(String, Vec<String>)],
                  keep_static: &std::collections::HashSet<String>) -> Result<Vec<u8>> {
    use std::collections::{HashMap, HashSet};
    let mut flags: HashMap<String, (bool, bool)> = [(payer.to_string(), (true, true))].into();
    let (mut order, mut programs) = (vec![payer.to_string()], HashSet::new());
    let mut add = |k: &str, signer: bool, writable: bool| match flags.get_mut(k) {
        Some(f) => *f = (f.0 || signer, f.1 || writable),
        None => { flags.insert(k.into(), (signer, writable)); order.push(k.into()) }
    };
    for ix in instructions {
        for (k, s, w) in &ix.accounts { add(k, *s, *w) }
        add(&ix.program, false, false);
        programs.insert(ix.program.clone());
    }
    let signers: Vec<&String> = order.iter().filter(|k| flags[*k].0).collect();
    if signers != [payer] {
        let others: Vec<String> = signers.iter().filter(|k| **k != payer).map(|k| format!("\"{k}\"")).collect();
        return fail(format!("The instructions need other signers: [{}]", others.join(", ")));
    }
    // Each loadable account from the first table that has it.
    let mut loaded: HashMap<&String, (usize, u8)> = HashMap::new();
    for key in order.iter().filter(|k| *k != payer && !programs.contains(*k) && !keep_static.contains(*k)) {
        if let Some(at) = tables.iter().enumerate().find_map(|(t, (_, entries))| entries.iter().position(|e| e == key).filter(|&i| i < 256).map(|i| (t, i as u8))) {
            loaded.insert(key, at);
        }
    }
    let writable = |k: &String| flags[k].1;
    let statics: Vec<&String> = order.iter().filter(|k| !loaded.contains_key(k)).collect();
    let keys: Vec<&String> = std::iter::once(&order[0])
        .chain(statics.iter().copied().filter(|k| *k != payer && writable(k)))
        .chain(statics.iter().copied().filter(|k| *k != payer && !writable(k)))
        .collect();
    let readonly_unsigned = keys.iter().filter(|k| !writable(k)).count();
    // The full list the instructions index: static keys, then every table's writable entries, then their readonly ones.
    let lookups: Vec<(&String, Vec<&String>, Vec<&String>)> = tables.iter().enumerate().filter_map(|(t, (address, _))| {
        let mine: Vec<&String> = order.iter().filter(|k| loaded.get(k).is_some_and(|l| l.0 == t)).collect();
        let (w, r): (Vec<&String>, Vec<&String>) = mine.into_iter().partition(|k| writable(k));
        (!w.is_empty() || !r.is_empty()).then_some((address, w, r))
    }).collect();
    let full: Vec<&String> = keys.iter().copied().chain(lookups.iter().flat_map(|l| l.1.clone())).chain(lookups.iter().flat_map(|l| l.2.clone())).collect();
    if full.len() > 256 { return fail("The transaction names more than 256 accounts.") }
    let index: HashMap<&String, u8> = full.iter().enumerate().map(|(i, k)| (*k, i as u8)).collect();

    let mut m = vec![0x80, 1, 0, readonly_unsigned as u8];
    m.extend(compact(keys.len()));
    for k in &keys { m.extend(decode(k)?) }
    m.extend(decode(blockhash)?);
    m.extend(compact(instructions.len()));
    for ix in instructions {
        m.push(index[&ix.program]);
        m.extend(compact(ix.accounts.len()));
        m.extend(ix.accounts.iter().map(|(k, _, _)| index[k]));
        m.extend(compact(ix.data.len()));
        m.extend(&ix.data);
    }
    m.extend(compact(lookups.len()));
    for (table, w, r) in &lookups {
        m.extend(decode(table)?);
        m.extend(compact(w.len()));
        m.extend(w.iter().map(|k| loaded[k].1));
        m.extend(compact(r.len()));
        m.extend(r.iter().map(|k| loaded[k].1));
    }
    if m.len() + 65 > 1232 { return fail(format!("The transaction is {} bytes, over Solana's limit of 1232", m.len() + 65)) }
    Ok(m)
}

#[cfg(test)]
mod tests;

//! A decoded Solana transaction message (legacy or v0), for checking what is about to be signed (`Message.swift`),
//! and the pure halves of the RPC answers about accounts.

use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value as Json;
use wallet::keys::base58;
use wallet::{Result, WalletError};

use crate::{fail, read_compact};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub program_index: usize,
    pub accounts: Vec<usize>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub table: String,
    pub writable: Vec<usize>,
    pub readonly: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub version: Option<u8>,
    pub required_signatures: usize,
    /// The header's counts of signers and of other accounts that are only read.
    pub readonly_signed: usize,
    pub readonly_unsigned: usize,
    pub static_keys: Vec<String>,
    /// The recent blockhash the message was built on: once past use, the transaction can never land.
    pub blockhash: String,
    pub instructions: Vec<Instruction>,
    pub lookups: Vec<Lookup>,
}

/// Only legacy and v0 are read. v1 moves the signatures and puts the priority fee in its header, so every check here
/// would read it wrong: refused until it's read in full.
fn unsupported(version: u8) -> WalletError {
    WalletError(if version == 1 {
        "This transaction is in Solana's new v1 format, which ihatecrypto doesn't sign yet. Nothing was signed.".into()
    } else {
        "This isn't a Solana transaction ihatecrypto can read. Nothing was signed.".into()
    })
}

impl Message {
    /// Parses a full wire transaction (signatures + message).
    pub fn parse_transaction(wire: &[u8]) -> Result<Self> {
        // Legacy and v0 start with the count of signatures, always under 128. A first byte of 128 or more is a version:
        // v1's 0x81 puts the message first and the signatures last (SIMD-0385).
        if let Some(&first) = wire.first() {
            if first & 0x80 != 0 { return Err(unsupported(first & 0x7f)) }
        }
        let mut o = 0;
        let signatures = read_compact(wire, &mut o)?;
        if o + signatures * 64 > wire.len() { return fail("Transaction truncated") }
        Self::parse(&wire[o + signatures * 64..])
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut p = 0usize;
        let need = |p: usize, n: usize| if p + n <= bytes.len() { Ok(()) } else { fail("Message truncated") };
        need(p, 1)?;
        let mut version = None;
        if bytes[0] & 0x80 != 0 {
            version = Some(bytes[0] & 0x7f);
            p = 1;
        }
        if let Some(v) = version.filter(|&v| v != 0) { return Err(unsupported(v)) }
        need(p, 3)?;
        let (required, readonly_signed, readonly_unsigned) = (bytes[p] as usize, bytes[p + 1] as usize, bytes[p + 2] as usize);
        p += 3;
        let key_count = read_compact(bytes, &mut p)?;
        need(p, key_count * 32 + 32)?;
        let static_keys = (0..key_count).map(|i| base58::encode(&bytes[p + i * 32..p + i * 32 + 32])).collect();
        let blockhash = base58::encode(&bytes[p + key_count * 32..p + key_count * 32 + 32]);
        p += key_count * 32 + 32; // keys, recent blockhash
        let mut instructions = Vec::new();
        for _ in 0..read_compact(bytes, &mut p)? {
            need(p, 1)?;
            let program_index = bytes[p] as usize;
            p += 1;
            let n = read_compact(bytes, &mut p)?;
            need(p, n)?;
            let accounts = bytes[p..p + n].iter().map(|&b| b as usize).collect();
            p += n;
            let len = read_compact(bytes, &mut p)?;
            need(p, len)?;
            instructions.push(Instruction { program_index, accounts, data: bytes[p..p + len].to_vec() });
            p += len;
        }
        let mut lookups = Vec::new();
        if version.is_some() {
            for _ in 0..read_compact(bytes, &mut p)? {
                need(p, 32)?;
                let table = base58::encode(&bytes[p..p + 32]);
                p += 32;
                let mut list = || -> Result<Vec<usize>> {
                    let n = read_compact(bytes, &mut p)?;
                    need(p, n)?;
                    let out = bytes[p..p + n].iter().map(|&b| b as usize).collect();
                    p += n;
                    Ok(out)
                };
                let (writable, readonly) = (list()?, list()?);
                lookups.push(Lookup { table, writable, readonly });
            }
        }
        // Trailing bytes would be signed without being checked.
        if p != bytes.len() { return fail("Extra bytes after the message") }
        Ok(Message { version, required_signatures: required, readonly_signed, readonly_unsigned, static_keys, blockhash, instructions, lookups })
    }

    /// The accounts the message may write, from `accounts`'s list: signers but the last `readonly_signed`, other static
    /// keys but the last `readonly_unsigned`, then every lookup table's writable entries.
    pub fn writable(&self, accounts: &[String]) -> Vec<String> {
        let signed = 0..self.required_signatures.saturating_sub(self.readonly_signed);
        let unsigned = self.required_signatures..self.required_signatures.max(self.static_keys.len().saturating_sub(self.readonly_unsigned));
        let n = self.static_keys.len();
        let looked = n..n + self.lookups.iter().map(|l| l.writable.len()).sum::<usize>();
        accounts.iter().enumerate()
            .filter(|(i, _)| signed.contains(i) || unsigned.contains(i) || looked.contains(i))
            .map(|(_, a)| a.clone()).collect()
    }

    /// Every account the message can touch, in Solana's order: static keys, then all writable lookup entries, then all
    /// readonly ones. `tables` maps a lookup table to its addresses.
    pub fn accounts(&self, tables: &HashMap<String, Vec<String>>) -> Result<Vec<String>> {
        let mut out = self.static_keys.clone();
        for writable in [true, false] {
            for lookup in &self.lookups {
                let Some(addresses) = tables.get(&lookup.table) else { return fail(format!("No address lookup table {}", lookup.table)) };
                for &i in if writable { &lookup.writable } else { &lookup.readonly } {
                    let Some(a) = addresses.get(i) else { return fail(format!("Index outside lookup table {}", lookup.table)) };
                    out.push(a.clone());
                }
            }
        }
        Ok(out)
    }
}

/// Who has a say over an account, read from jsonParsed: what a balance can't show (research E-1, 2026-11-05).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountRights {
    /// The program that owns it: the System program for a wallet.
    pub program: String,
    /// Bytes of data it holds: none for a wallet.
    pub space: u64,
    /// A token account's owner, its delegate and how much that one may still spend, and who may close it.
    pub owner: Option<String>,
    pub delegate: Option<String>,
    pub delegated_amount: u64,
    pub close_authority: Option<String>,
}

/// Lamports and, for a token account, the raw amount.
pub type State = (u64, Option<u64>);

/// Addresses stored in an address lookup table account's data (56-byte header, then 32-byte keys).
pub fn lookup_table(raw: &[u8]) -> Vec<String> {
    (56..raw.len().saturating_sub(31)).step_by(32).map(|o| base58::encode(&raw[o..o + 32])).collect()
}

fn info(account: &Json) -> Option<&Json> { account.get("data")?.get("parsed")?.get("info") }

/// One account in jsonParsed: its lamports and, for a token account, the raw amount; None when it isn't an account.
pub fn state(account: &Json) -> Option<State> {
    let lamports = account.as_object()?.get("lamports")?.as_u64()?;
    let amount = info(account).and_then(|i| i.get("tokenAmount")?.get("amount")?.as_str()?.parse().ok());
    Some((lamports, amount))
}

/// One account in jsonParsed: its program, its size (the node's «space», else the length of its raw data) and, for a
/// token account, its owner, delegate and close authority; None when it isn't an account (null: closed or never opened).
pub fn rights(account: &Json) -> Option<AccountRights> {
    let a = account.as_object()?;
    let text = |v: Option<&Json>| v.and_then(Json::as_str).map(str::to_string);
    let raw = a.get("data").and_then(|d| d.get(0)).and_then(Json::as_str).and_then(|s| STANDARD.decode(s).ok()).map(|d| d.len() as u64);
    let space = a.get("space").and_then(Json::as_u64)
        .or_else(|| a.get("data").and_then(|d| d.get("space")).and_then(Json::as_u64))
        .or(raw).unwrap_or(0);
    let i = info(account);
    Some(AccountRights {
        program: text(a.get("owner")).unwrap_or_default(),
        space,
        owner: text(i.and_then(|i| i.get("owner"))),
        delegate: text(i.and_then(|i| i.get("delegate"))),
        delegated_amount: i.and_then(|i| i.get("delegatedAmount")?.get("amount")?.as_str()?.parse().ok()).unwrap_or(0),
        close_authority: text(i.and_then(|i| i.get("closeAuthority"))),
    })
}

/// getMultipleAccounts' value. The node answers null for an account that isn't there yet (a coin's first buy opens
/// it): that holds nothing. Anything else that isn't the list asked for is an error: a made-up 0 would let a route take
/// everything or pay someone else.
pub fn accounts(addresses: &[String], value: Option<&Json>) -> Result<HashMap<String, State>> {
    let Some(list) = value.and_then(Json::as_array).filter(|l| l.len() == addresses.len()) else {
        return fail("Solana RPC getMultipleAccounts: no account list");
    };
    let mut out = HashMap::new();
    for (address, account) in addresses.iter().zip(list) {
        let entry = if account.is_null() { (0, Some(0)) } else {
            let Some((lamports, token)) = state(account) else {
                return fail(format!("Solana RPC getMultipleAccounts: {address} isn't an account"));
            };
            // A plain system account holds no tokens: a wallet, or a coin account's address someone sent SOL to before
            // it was opened. A mint or another program's account stays None.
            let system = account.get("owner").and_then(Json::as_str) == Some("11111111111111111111111111111111");
            (lamports, token.or(if system { Some(0) } else { None }))
        };
        out.entry(address.clone()).or_insert(entry);
    }
    Ok(out)
}

/// What each watched balance held before a move: token units for a token rule, lamports for the others. A token rule
/// on an account with no token amount (a mint, another program's account) is an error: its balance can't be checked.
pub fn before(watch: &[(String, String, bool)], now: &HashMap<String, State>) -> Result<HashMap<String, i64>> {
    watch.iter().map(|(address, label, token)| {
        let held = now.get(address).and_then(|s| if *token { s.1 } else { Some(s.0) }).and_then(|n| i64::try_from(n).ok());
        match held {
            Some(h) => Ok((address.clone(), h)),
            None => fail(format!("{label} isn't a token account, so its balance can't be checked.")),
        }
    }).collect()
}

/// The fee payer can't cover the fee or a new account's rent: it holds no SOL at all (AccountNotFound), too little for
/// the fee, would drop under its own rent, or the System Program couldn't fund an account from it.
pub fn short_of_sol(error: &str, logs: &[String]) -> bool {
    error.contains("AccountNotFound") || error.contains("InsufficientFundsForFee")
        || error.contains(r#"{"InsufficientFundsForRent":{"account_index":0}}"#)
        || logs.iter().any(|l| l.contains("Transfer: insufficient lamports"))
}

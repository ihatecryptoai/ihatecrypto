//! The cases of SolanaTests.swift, with its mainnet accounts.

use ed25519_dalek::{Signature, Verifier};
use serde_json::json;

use super::*;
use crate::message::{self, Message};
use crate::spl::{self, sns};

fn b58(bytes: &[u8]) -> String { base58::encode(bytes) }
const KEY: [u8; 32] = [7; 32];

fn transfer_wire() -> Vec<u8> { transfer(&KEY, &b58(&[9; 32]), 1, &b58(&[1; 32]), 20_000).unwrap().1 }

#[test]
fn sol_transfer_is_signed_legacy_message_with_priority_fee() {
    let (signature, wire) = transfer(&KEY, &b58(&[9; 32]), 1_000_000, &b58(&[1; 32]), 20_000).unwrap();
    assert_eq!(wire[0], 1);
    let (sig, message) = (&wire[1..65], &wire[65..]);
    assert_eq!(b58(sig), signature);
    let public = key(&KEY).unwrap().verifying_key();
    assert!(public.verify(message, &Signature::from_slice(sig).unwrap()).is_ok());
    assert_eq!(message[..4], [1, 0, 2, 4]);
    assert_eq!(message[4..36], public.to_bytes());
    assert_eq!(message[message.len() - 8..], [0x40, 0x42, 0x0f, 0, 0, 0, 0, 0]);
    assert_eq!(transfer(&KEY, &b58(&public.to_bytes()), 1, &b58(&[1; 32]), 1).unwrap_err().0, "Transfer to yourself");
    assert_eq!(transfer(&KEY, &b58(&[9; 31]), 1, &b58(&[1; 32]), 1).unwrap_err().0, format!("Invalid Solana address: {}", b58(&[9; 31])));
}

#[test]
fn signing_someone_elses_transaction_requires_us_as_fee_payer() {
    let ours = transfer_wire();
    let mut blank = vec![1];
    blank.extend([0; 64]);
    blank.extend(&ours[65..]);
    let (_, signed) = sign(&blank, &KEY, None).unwrap();
    assert_eq!(signed, ours); // ed25519-dalek is deterministic, so the very bytes
    assert_eq!(sign(&blank, &[8; 32], None).unwrap_err().0, "The fee payer isn't our wallet");
    let sponsor = [5u8; 32];
    assert!(sign(&blank, &KEY, Some(&sponsor)).unwrap_err().0.starts_with("The transaction needs 1 signatures, but the fee payer's and ours were"));
}

#[test]
fn associated_token_accounts_match_mainnet() {
    let (wallet, account, usdc) = ("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", "FGETo8T8wMcN2wCjav8VK6eh3dLk63evNDPxzLSJra8B", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    assert!(spl::is_on_curve(&decode(wallet).unwrap()));
    assert!(spl::is_on_curve(&decode("2EUrWmf5xMmWER9BtDbXbGbZjoL7R3eTDMXYR6H6cKPj").unwrap()));
    assert!(!spl::is_on_curve(&decode(account).unwrap()));
    assert_eq!(spl::associated_account(wallet, usdc, spl::TOKEN_PROGRAM).unwrap(), account);
    // Token-2022 accounts derive from that program: real holders of a Token-2022 mint, 2026-09-29.
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6";
    let holders = [("6LXutJvKUw8Q5ue2gCgKHQdAN4suWW8awzFVC6XCguFx", "AKfyuSUkLjnHb4KadX6SxfAi6eGePBJqQSZCsCBk2jnc"),
                   ("GNqZFdcipNFo5nr8LnK7YVsMy84458BWHwnE1RX5dNpC", "G3zG4N9MfWTfrCBxBYNcTXsKmsYLNp4GfcUmxagndT9t"),
                   ("GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL", "7QnWse8CQFg6hP5uBKii1gndpMxastHFUKn4TjaFwfcM")];
    let matches: Vec<_> = holders.iter().filter(|(o, a)| spl::associated_account(o, mint, spl::TOKEN_2022_PROGRAM).unwrap() == *a).collect();
    assert!(!matches.is_empty());
    for (o, a) in matches { assert_ne!(spl::associated_account(o, mint, spl::TOKEN_PROGRAM).unwrap(), *a) }
}

#[test]
fn compiled_instructions_put_the_payer_first_and_merge_flags() {
    let me = b58(&key(&KEY).unwrap().verifying_key().to_bytes());
    let (vault, config, program) = (b58(&[3; 32]), b58(&[4; 32]), b58(&[5; 32]));
    let ix = Instruction { program: program.clone(), accounts: vec![
        (config.clone(), false, false), (me.clone(), true, true), (me.clone(), false, false), (vault.clone(), false, true),
    ], data: vec![1, 2, 3] };
    let (signature, wire) = compile(&[ix], &KEY, &b58(&[1; 32]), 20_000).unwrap();
    let m = Message::parse_transaction(&wire).unwrap();
    assert_eq!((m.static_keys[0].clone(), m.required_signatures), (me.clone(), 1));
    assert_eq!(m.static_keys.iter().collect::<std::collections::HashSet<_>>().len(), m.static_keys.len());
    assert_eq!(m.static_keys[1], vault);
    assert_eq!(m.instructions.len(), 2);
    assert_eq!(m.instructions[1].accounts.iter().map(|&i| m.static_keys[i].clone()).collect::<Vec<_>>(), [config, me.clone(), me, vault.clone()]);
    assert_eq!(signature, b58(&wire[1..65]));
    let foreign = Instruction { program, accounts: vec![(vault.clone(), true, false)], data: vec![] };
    assert_eq!(compile(&[foreign], &KEY, &b58(&[1; 32]), 1).unwrap_err().0, format!("The instructions need other signers: [\"{vault}\"]"));
}

#[test]
fn parse_rejects_bytes_after_the_message() {
    let mut wire = transfer_wire();
    let m = Message::parse_transaction(&wire).unwrap();
    assert_eq!(m.blockhash, b58(&[1; 32]));
    assert_eq!((m.readonly_signed, m.readonly_unsigned), (0, 2));
    assert_eq!(m.writable(&m.static_keys), m.static_keys[..2]);
    wire.push(0);
    assert_eq!(Message::parse_transaction(&wire).unwrap_err().0, "Extra bytes after the message");
}

fn v1_message() -> Vec<u8> {
    let mut m = vec![0x81, 1, 0, 1, 0, 0, 0, 0];
    m.extend([1; 32]);
    m.extend([0, 2]);
    m.extend([7; 32]);
    m.extend([9; 32]);
    m
}

#[test]
fn parse_refuses_v1() {
    let mut wire = v1_message();
    wire.extend([0; 64]);
    let e = Message::parse_transaction(&wire).unwrap_err().0;
    assert!(e.contains("v1") && e.contains("Nothing was signed"));
    assert!(Message::parse(&v1_message()).unwrap_err().0.contains("v1"));
    let mut enveloped = vec![1];
    enveloped.extend([0; 64]);
    enveloped.extend(v1_message());
    assert!(Message::parse_transaction(&enveloped).unwrap_err().0.contains("v1"));
    assert!(unsigned(&wire, &[7; 32], None).is_err());
    assert!(Message::parse_transaction(&[0x80, 1, 0, 0]).unwrap_err().0.contains("can read"));
}

/// Whatever bytes a site sends, the parsers answer or fail: never a panic. Random buffers, a real transfer with bytes
/// changed or cut, and lengths made to overflow.
#[test]
fn parse_never_panics() {
    let mut state: u64 = 2026_11_05;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let transfer = transfer_wire();
    let mut buffers: Vec<Vec<u8>> = vec![[vec![0x80; 9], vec![1]].concat(), vec![0xff, 0xff, 0xff, 0x7f],
                                         [vec![1], vec![0; 64], vec![0x80]].concat(), vec![0x80], vec![], v1_message()];
    for _ in 0..2000 {
        let n = (next() % 601) as usize;
        buffers.push((0..n).map(|_| next() as u8).collect());
        let mut changed = transfer.clone();
        for _ in 0..1 + next() % 3 {
            let i = (next() as usize) % changed.len();
            changed[i] = next() as u8;
        }
        changed.truncate((next() as usize) % (changed.len() + 1));
        buffers.push(changed);
    }
    for wire in &buffers {
        let _ = Message::parse_transaction(wire);
        let _ = Message::parse(wire);
        let _ = unsigned(wire, &[7; 32], None);
        let _ = unsigned(wire, &[7; 32], Some(&[5; 32]));
    }
}

#[test]
fn compact_is_u16() {
    let read = |b: &[u8]| { let mut o = 0; read_compact(b, &mut o) };
    assert_eq!(read(&[0x7f]).unwrap(), 127);
    assert_eq!(read(&[0x80, 0x01]).unwrap(), 128);
    assert_eq!(read(&[0xff, 0xff, 0x03]).unwrap(), 65_535);
    for bad in [&[0xff, 0xff, 0x04][..], &[0x80, 0x80, 0x80, 0x01], &[0x80, 0x00], &[0xff, 0xff, 0xff, 0x7f]] {
        assert!(read(bad).is_err(), "{bad:?}");
    }
    for n in [0, 1, 127, 128, 16383, 16384, 65535] {
        assert_eq!(read(&compact(n)).unwrap(), n);
    }
}

#[test]
fn short_of_sol_reads_the_fee_payers_errors() {
    let none: Vec<String> = vec![];
    assert!(message::short_of_sol(r#"{"InsufficientFundsForRent":{"account_index":0}}"#, &none));
    assert!(message::short_of_sol("AccountNotFound", &none));
    assert!(message::short_of_sol("InsufficientFundsForFee", &none));
    assert!(message::short_of_sol(r#"{"InstructionError":[2,{"Custom":1}]}"#, &["Transfer: insufficient lamports 1977880, need 2039280".into()]));
    assert!(!message::short_of_sol(r#"{"InsufficientFundsForRent":{"account_index":3}}"#, &none));
    assert!(!message::short_of_sol(r#"{"InstructionError":[3,{"Custom":6001}]}"#, &["Program log: Error: slippage tolerance exceeded".into()]));
}

#[test]
fn spl_transfer_message_layout() {
    let payer = b58(&key(&KEY).unwrap().verifying_key().to_bytes());
    let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let m = spl::transfer_message(&payer, "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", usdc, 5, 6, &b58(&[1; 32]), 1, spl::TOKEN_PROGRAM, true).unwrap();
    let parsed = Message::parse(&m).unwrap();
    assert_eq!(parsed.static_keys[2], "FGETo8T8wMcN2wCjav8VK6eh3dLk63evNDPxzLSJra8B");
    assert_eq!(parsed.instructions.len(), 4);
    assert_eq!(parsed.instructions[2].data, [vec![12], 5u64.to_le_bytes().to_vec(), vec![6]].concat());
    assert_eq!(parsed.instructions[3].data, [9]);
    assert_eq!(spl::transfer_message(&payer, &payer, usdc, 5, 6, &b58(&[1; 32]), 1, spl::TOKEN_PROGRAM, false).unwrap_err().0, "Transfer to yourself");
    let (_, wire) = spl::create_own_account(&KEY, usdc, &b58(&[1; 32]), 1).unwrap();
    assert_eq!(Message::parse_transaction(&wire).unwrap().instructions[1].accounts, [0, 1, 0, 2, 3, 4]);
}

#[test]
fn sol_names() {
    assert!(sns::is_name("toly.sol") && sns::is_name("a-b.sol") && sns::is_name("x.sol"));
    assert!(!sns::is_name("-a.sol") && !sns::is_name("a-.sol") && !sns::is_name(".sol") && !sns::is_name("Toly.sol") && !sns::is_name("toly"));
    // bonfida.sol's name account, as the Name Service derives it.
    assert_eq!(sns::account("bonfida.sol").unwrap(), "Crf8hzfthWGbGbLTVCiqRqV5MVnbpHB1L9KQMd6gsinb");
    let mut data = decode(sns::ROOT).unwrap();
    data.extend(decode("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM").unwrap());
    data.extend([0; 32]);
    assert_eq!(sns::owner("x.sol", Some(sns::PROGRAM), Some(&data)).unwrap(), "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM");
    data[32..64].copy_from_slice(&decode("FGETo8T8wMcN2wCjav8VK6eh3dLk63evNDPxzLSJra8B").unwrap());
    assert!(sns::owner("x.sol", Some(sns::PROGRAM), Some(&data)).unwrap_err().0.contains("held by a program"));
    assert_eq!(sns::owner("x.sol", None, None).unwrap_err().0, "x.sol isn't registered.");
}

#[test]
fn account_answers() {
    let token = json!({"lamports": 2039280, "owner": spl::TOKEN_PROGRAM, "space": 165,
        "data": {"parsed": {"info": {"owner": "W", "delegate": "D", "delegatedAmount": {"amount": "7"}, "tokenAmount": {"amount": "42"}}}}});
    let wallet = json!({"lamports": 5, "owner": "11111111111111111111111111111111", "data": ["", "base64"]});
    let mint = json!({"lamports": 9, "owner": spl::TOKEN_PROGRAM, "data": ["AAAA", "base64"]});
    let addresses: Vec<String> = ["t", "w", "m", "n"].map(String::from).to_vec();
    let got = message::accounts(&addresses, Some(&json!([token, wallet, mint, null]))).unwrap();
    assert_eq!(got["t"], (2039280, Some(42)));
    assert_eq!(got["w"], (5, Some(0)));
    assert_eq!(got["m"], (9, None));
    assert_eq!(got["n"], (0, Some(0)));
    assert!(message::accounts(&addresses, Some(&json!([1, 2]))).is_err());
    assert!(message::accounts(&addresses, Some(&json!(["x", null, null, null]))).unwrap_err().0.contains("t isn't an account"));
    let r = message::rights(&token).unwrap();
    assert_eq!((r.space, r.delegate.as_deref(), r.delegated_amount), (165, Some("D"), 7));
    assert_eq!(message::rights(&mint).unwrap().space, 3);
    assert!(message::rights(&json!(null)).is_none());
    let before = message::before(&[("t".into(), "USDC".into(), true), ("w".into(), "SOL".into(), false)], &got).unwrap();
    assert_eq!((before["t"], before["w"]), (42, 5));
    assert_eq!(message::before(&[("m".into(), "The mint".into(), true)], &got).unwrap_err().0, "The mint isn't a token account, so its balance can't be checked.");
    let mut table = vec![0u8; 56];
    table.extend([1; 32]);
    table.extend([2; 32]);
    assert_eq!(message::lookup_table(&table), [b58(&[1; 32]), b58(&[2; 32])]);
}

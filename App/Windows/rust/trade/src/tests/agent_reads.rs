//! AgentReadsTests: a contract's view call encoded and its answer read, hostile answers, JSON shaped for the model and
//! the user's address hidden from it.

use serde_json::json;
use wallet::{from_hex, to_hex};

use crate::agent_reads::{AgentReads, Me};

#[test]
fn agent_reads_encode_and_decode_contract_calls() {
    let owner = "0x1111111111111111111111111111111111111111";
    assert_eq!(to_hex(&AgentReads::encode("balanceOf", &["address"], &[owner]).unwrap()), format!("70a08231{}{}", "0".repeat(24), &owner[2..]));
    assert!(to_hex(&AgentReads::encode("convertToAssets", &["uint256"], &["1000000"]).unwrap()).ends_with("0f4240"));
    assert!(AgentReads::encode("f", &["string"], &["x"]).is_err());
    assert_eq!(AgentReads::encode("withdraw", &["uint256"], &["max"]).unwrap()[4..], [0xff; 32]);

    let max = [0xffu8; 32];
    let abc = [&[0u8; 31][..], &[0x20], &[0; 31], &[3], b"abc", &[0; 29]].concat();
    assert_eq!(AgentReads::decode(&["uint256", "int256"], &[max, max].concat()), ["max (2^256−1, unlimited)", "-1"]);
    assert_eq!(AgentReads::decode(&["string"], &abc), ["«abc»"]);
    let pair = [&[0u8; 12][..], &from_hex(owner).unwrap(), &[0; 31], &[1]].concat();
    assert_eq!(AgentReads::decode(&["address", "bool"], &pair), [owner, "true"]);
}

/// A contract's answer is someone else's data: an offset or a length near 2⁶⁴ reads as «?», never stops the app.
#[test]
fn hostile_answers_read_as_nothing() {
    let words = [0u8; 64];
    for i in [2, usize::MAX / 32, usize::MAX] {
        assert!(evm::abi::uint_in(&words, i).is_none() && evm::abi::address_in(&words, i).is_none() && evm::abi::u256_in(&words, i).is_none());
    }
    let far_offset = [&[0u8; 24][..], &[0x80], &[0; 7]].concat();   // 2⁶³: word 2⁵⁸, whose end overflowed Int
    let long_length = [&[0u8; 31][..], &[0x20], &[0; 24], &[0xff; 8]].concat();   // 2⁶⁴ − 1 bytes
    assert_eq!(AgentReads::decode(&["string"], &[&far_offset[..], &words].concat()), ["?"]);
    assert_eq!(AgentReads::decode(&["bytes"], &long_length), ["?"]);
}

#[test]
fn agent_reads_keep_only_matching_items_and_hide_the_users_address() {
    let pools = r#"{"status":"ok","data":[{"project":"fluid-lending","chain":"Arbitrum","symbol":"USDC"},{"project":"aave-v3","chain":"Base","symbol":"USDC"}]}"#;
    let out = AgentReads::shaped(pools.as_bytes(), &["fluid".into(), "arbitrum".into()]);
    assert!(out.starts_with("1 of 2 items hold «fluid arbitrum»:"), "{out}");
    assert!(out.contains("fluid-lending") && !out.contains("aave-v3"));
    assert_eq!(AgentReads::compact(&json!({"a": null, "b": "", "c": [], "d": 1})), r#"{"d":1}"#);
    assert_eq!(AgentReads::split("https://x.io/a find Morpho base"), ("https://x.io/a".to_string(), vec!["morpho".to_string(), "base".to_string()]));
    let me = Me { evm: Some("0xAbCdEf0000000000000000000000000000000001".into()), solana: None };
    assert_eq!(AgentReads::masked("from 0xabcdef0000000000000000000000000000000001 to 0x2", &me), "from me to 0x2");
}

/// A call's address argument is encoded only as 0x and 40 hex digits: written without the 0x it slipped past the chat's
/// checks of the addresses in a call (GoPlus, «yours or verified code»), which find them that way.
#[test]
fn a_calls_address_argument_needs_its_prefix() {
    let me = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
    let args = |a: &str, b: &str| vec![a.to_string(), b.to_string()];
    assert!(AgentReads::calldata("supplyFor(address,uint256)", &args("012f89ff307568f8409bbe379ca92ed41bc4d386", "100"), me).is_err());
    assert!(AgentReads::calldata("supplyFor(address,uint256)", &args("0X012f89ff307568f8409bbe379ca92ed41bc4d386", "100"), me).is_err());
    assert_eq!(AgentReads::calldata("supplyFor(address,uint256)", &args("0x012f89ff307568f8409bbe379ca92ed41bc4d386", "100"), me).unwrap().len(), 68);
    assert_eq!(AgentReads::calldata("deposit(uint256,address)", &args("100", "me"), me).unwrap().len(), 68);
}

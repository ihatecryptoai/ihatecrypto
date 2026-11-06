//! tests/golden.json is real Swift output: the types of SignerAPI.swift (and MsgPack's Codable, EVMSignature) copied
//! into a script and encoded with a plain JSONEncoder, one entry per request case and two replies; `_decodes` records
//! what Swift's decoder takes.

use serde_json::{Value, json};

use super::*;

fn golden() -> serde_json::Map<String, Value> {
    serde_json::from_str(include_str!("../tests/golden.json")).unwrap()
}

#[test]
fn every_request_and_reply_matches_swift() {
    let g = golden();
    let mut requests = 0;
    for (name, swift) in &g {
        if name.starts_with('_') { continue }
        let back = if name.starts_with("reply") {
            serde_json::to_value(serde_json::from_value::<SignerReply>(swift.clone()).unwrap_or_else(|e| panic!("{name}: {e}"))).unwrap()
        } else {
            requests += 1;
            serde_json::to_value(serde_json::from_value::<SignerRequest>(swift.clone()).unwrap_or_else(|e| panic!("{name}: {e}"))).unwrap()
        };
        assert_eq!(&back, swift, "{name}");
    }
    assert_eq!(requests, 43); // 42 cases, signSolana twice (with and without a sponsor)
}

#[test]
fn rust_writes_what_swift_writes() {
    let g = golden();
    let r = SignerRequest::SetAgentActive { active: true };
    assert_eq!(serde_json::to_value(&r).unwrap(), g["setAgentActive"]);
    assert_eq!(serde_json::to_value(SignerRequest::Status {}).unwrap(), g["status"]);
    let nested = SignerRequest::AsWallet { id: "9".into(), request: Box::new(SignerRequest::AsWallet {
        id: "8".into(), request: Box::new(SignerRequest::SignPersonal { message: vec![1, 2, 3, 0xfb, 0xff] }) }) };
    assert_eq!(serde_json::to_value(&nested).unwrap(), g["asWallet"]);
    assert_eq!(serde_json::to_value(SignerReply { ok: false, ..Default::default() }).unwrap(), g["replyMinimal"]);
    assert_eq!(serde_json::to_string(&SignerReply::error("no")).unwrap(), r#"{"ok":false,"error":"no","checks":[],"problems":[]}"#);
}

/// Where Swift's decoder refuses, so does this one; a nil written as null is read as nil by both.
#[test]
fn decodes_as_strictly_as_swift() {
    let g = golden();
    let swift = g["_decodes"].as_object().unwrap();
    let request = |s: &str| serde_json::from_str::<SignerRequest>(s).is_ok();
    assert_eq!(request(r#"{"status":null}"#), swift["status_null"].as_bool().unwrap());
    assert_eq!(request(r#""status""#), swift["status_string"].as_bool().unwrap());
    assert_eq!(request(r#"{"status":{},"lock":{}}"#), swift["two_keys"].as_bool().unwrap());
    assert_eq!(request(r#"{"signSolana":{"wire":"AQ==","accounts":[],"recipients":[],"tokenOwners":{}}}"#), swift["signSolana_no_extraPrograms"].as_bool().unwrap());
    assert_eq!(request(r#"{"signSolana":{"wire":"AQ==","accounts":[],"recipients":[],"tokenOwners":{},"extraPrograms":[],"sponsor":null}}"#),
               swift["signSolana_sponsor_null"].as_bool().unwrap());
    assert_eq!(serde_json::from_str::<SignerReply>(r#"{"ok":true}"#).is_ok(), swift["reply_no_checks"].as_bool().unwrap());
    assert_eq!(serde_json::from_str::<SignerAddress>(r#"{"name":"a","network":"b","address":"c"}"#).is_ok(), swift["address_no_memo"].as_bool().unwrap());
    // Swift writes "/" as "\/" in base64; both read either.
    let escaped = serde_json::from_str::<SignerRequest>(r#"{"signPersonal":{"message":"AQID+\/8="}}"#).unwrap();
    assert_eq!(escaped, SignerRequest::SignPersonal { message: vec![1, 2, 3, 0xfb, 0xff] });
}

#[test]
fn passcodes() {
    assert_eq!(passcode::problem("130579"), None);
    for bad in ["12345", "1234567", "12a456", "１２３４５６", "13057٣"] {
        assert_eq!(passcode::problem(bad).as_deref(), Some("The code is 6 digits."), "{bad}");
    }
    assert_eq!(passcode::problem("777777").as_deref(), Some("All the same digit is the first code anyone tries."));
    for run in ["123456", "890123", "654321", "210987"] {
        assert_eq!(passcode::problem(run).as_deref(), Some("A straight run like 123456 is guessed first."), "{run}");
    }
    assert_eq!(passcode::problem("123457"), None);
}

fn wallet(id: &str, kind: &str, automation: Option<bool>) -> SignerWallet {
    SignerWallet { id: id.into(), label: format!("W{id}"), kind: kind.into(), solana: Some("S".into()), evm: None, near: None, test: None, automation }
}

#[test]
fn wallets() {
    let w = wallet("2", "solana", None);
    assert_eq!(w.missing("solana"), None);
    assert_eq!(w.missing("evm").unwrap(),
               "The wallet «W2» has no EVM (Hyperliquid, Base…) key: it holds only a Solana key. Switch wallets (Settings › Wallet) or import one that has it.");
    assert_eq!(wallet("3", "phrase", None).missing("near").unwrap(),
               "The wallet «W3» has no NEAR key: it holds only a key. Switch wallets (Settings › Wallet) or import one that has it.");
    let list = vec![wallet("9", "phrase", Some(true)), wallet("1", "phrase", None), wallet("2", "evm", Some(false))];
    assert_eq!(SignerWallet::on(&list, Some("2")).unwrap().id, "2");
    assert_eq!(SignerWallet::on(&list, Some("9")).unwrap().id, "1"); // never the automation wallet
    assert_eq!(SignerWallet::on(&list, None).unwrap().id, "1");
    assert!(SignerWallet::on(&list[..1], None).is_none());
    let migrated = SignerWallet::migrated(None, true, None, Some("0xE".into()), Some("n".into()));
    assert_eq!((migrated[0].id.as_str(), migrated[0].label.as_str(), migrated[0].kind.as_str()), ("1", "Main", "phrase"));
    assert!(SignerWallet::migrated(None, true, None, None, Some("n".into())).is_empty());
    assert!(SignerWallet::migrated(None, false, Some("S".into()), None, None).is_empty());
    assert_eq!(SignerWallet::migrated(Some(list.clone()), false, None, None, None), list);
    assert_eq!(serde_json::to_value(&list[1]).unwrap(), json!({"id": "1", "label": "W1", "kind": "phrase", "solana": "S"}));
}

#[test]
fn same_address() {
    let a = SignerAddress { name: "Mom".into(), network: "base".into(), address: "0xAbC".into(), memo: String::new() };
    assert!(a.same("mom", "base", "0xabc"));
    assert!(!a.same("mom", "arbitrum", "0xabc"));
    let s = SignerAddress { name: "Mom".into(), network: "solana".into(), address: "AbC".into(), memo: String::new() };
    assert!(!s.same("Mom", "solana", "abc"));
}

#[test]
fn frames() {
    let mut pipe = Vec::new();
    write_frame(&mut pipe, br#"{"status":{}}"#).unwrap();
    write_frame(&mut pipe, b"").unwrap();
    assert_eq!(pipe[..4], [13, 0, 0, 0]);
    let mut input = pipe.as_slice();
    assert_eq!(read_frame(&mut input).unwrap(), br#"{"status":{}}"#);
    assert_eq!(read_frame(&mut input).unwrap(), b"");
    assert_eq!(read_frame(&mut input), None); // closed
    let too_long = ((MAX_FRAME + 1) as u32).to_le_bytes();
    assert_eq!(read_frame(&mut too_long.as_slice()), None);
    assert_eq!(read_frame(&mut [5u8, 0, 0, 0, 1].as_slice()), None); // cut short
}

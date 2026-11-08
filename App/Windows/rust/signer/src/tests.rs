//! The vault and the trusted settings with the stand-in for Windows Hello, in a folder of their own. The state is the
//! process's (as in the signer), so the cases run one after another in one test.

use std::sync::Once;

use signer_api::{SignerAddress, SignerWallet};

use crate::trusted::{self, Pref, State, takes_trust_away};
use crate::vault::{self, agents, code, recipients, seeds};

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("ihc-signer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: set once, before any thread reads them.
        unsafe {
            std::env::set_var("IHATECRYPTO_SIGNER_DIR", &dir);
            std::env::set_var("WINSEC_FAKE_HELLO", "1");
        }
    });
}

fn wallet(id: &str, label: &str) -> SignerWallet {
    SignerWallet { id: id.into(), label: label.into(), kind: "phrase".into(), solana: None, evm: Some("0xabc".into()), near: None, test: None, automation: None }
}

#[test]
fn the_vault_seals_opens_and_keeps_its_settings() {
    setup();
    // A seed under Windows Hello: a master key is made, the box doesn't hold the seed, and it opens again.
    let seed = [7u8; 64];
    seeds::save(&seed, true, "1", false).unwrap();
    assert!(vault::exists("master.json"));
    let sealed = seeds::sealed(&vault::seed_file("1")).unwrap().unwrap();
    assert!(sealed.presence && sealed.challenge.is_some());
    assert!(!sealed.sealed.windows(64).any(|w| w == seed));
    assert_eq!(&**seeds::load("sign", "1", true).unwrap(), &seed);
    assert!(seeds::without_prompt("1").is_none());
    // A test wallet opens with no prompt.
    seeds::save(&[9; 64], false, "2", true).unwrap();
    assert_eq!(&**seeds::without_prompt("2").unwrap(), &[9; 64]);

    // The trusted state: sealed with the key in hand, checked against it, refused when changed outside.
    vault::begin_request();
    let key = vault::key("test", true, None).unwrap();
    trusted::mutate(Some("add"), |s| { s.prefs.insert("wallets".into(), Pref::Data(serde_json::to_vec(&vec![wallet("1", "Main")]).unwrap())); }).unwrap();
    recipients::add(&["0x000000000000000000000000000000000000DEAD".into()], recipients::List::Money).unwrap();
    assert!(recipients::all(recipients::List::Money).contains("0x000000000000000000000000000000000000dead"));
    trusted::verify(&key, false).unwrap();
    let file = vault::read("state.json").unwrap();
    let mut planted: serde_json::Value = serde_json::from_slice(&file).unwrap();
    let mut body: State = serde_json::from_slice(&base64_decode(planted["body"].as_str().unwrap())).unwrap();
    body.recipients.insert("1".into(), vec!["0xbad".into()]);
    planted["body"] = serde_json::Value::String(base64_encode(&serde_json::to_vec(&body).unwrap()));
    vault::write("state.json", &serde_json::to_vec(&planted).unwrap()).unwrap();
    trusted::reload();
    assert_eq!(trusted::verify(&key, false).unwrap_err(), trusted::tampered());
    vault::write("state.json", &file).unwrap();
    trusted::reload();
    trusted::verify(&key, false).unwrap();

    // A rename with no key in hand waits in pending.json and counts; adding an address that way doesn't.
    vault::end_session();
    vault::begin_request();
    trusted::mutate(None, |s| { s.prefs.insert("wallets".into(), Pref::Data(serde_json::to_vec(&vec![wallet("1", "Savings")]).unwrap())); }).unwrap();
    assert!(vault::exists("pending.json"));
    let label = |s: &State| match s.prefs.get("wallets") { Some(Pref::Data(d)) => serde_json::from_slice::<Vec<SignerWallet>>(d).unwrap()[0].label.clone(), _ => String::new() };
    assert_eq!(label(&trusted::state()), "Savings");
    let key = vault::key("test", true, None).unwrap();
    assert!(!vault::exists("pending.json"));
    trusted::reload();
    trusted::verify(&key, false).unwrap();
    assert_eq!(label(&trusted::state()), "Savings");

    // The trading key sealed under Windows Hello, its address read without opening it, opened again after a restart.
    let agent = agents::make().unwrap();
    agents::save(&agent, "1", None).unwrap();
    let address = wallet::keys::evm_address(&agent).unwrap();
    assert_eq!(agents::address("1").as_deref(), Some(address.as_str()));
    agents::forget();
    vault::end_session();
    vault::begin_request();
    assert_eq!(agents::load("1").map(|k| k.to_vec()), Some(agent.to_vec()));

    // The passcode: right, wrong with tries left, and off after five.
    code::set("135790").unwrap();
    assert_eq!(code::enter("135790"), code::Entered::Right);
    assert_eq!(code::enter("000000"), code::Entered::Wrong { left: 4 });
    for _ in 0..3 { code::enter("000000"); }
    assert_eq!(code::enter("000000"), code::Entered::Off);
    assert!(!code::is_set());
}

#[test]
fn settings_cross_the_file_as_swift_writes_them() {
    let p = Pref::String("on".into());
    assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"string":{"_0":"on"}}"#);
    assert_eq!(serde_json::from_str::<Pref>(r#"{"int":{"_0":300}}"#).unwrap(), Pref::Int(300));
    assert_eq!(serde_json::from_str::<Pref>(r#"{"data":{"_0":"AQI="}}"#).unwrap(), Pref::Data(vec![1, 2]));
    assert!(serde_json::from_str::<Pref>(r#"{"bool":{"_0":true},"int":{"_0":1}}"#).is_err());
    // A state sealed before there were two lists has no «calls».
    let s: State = serde_json::from_str(r#"{"prefs":{},"recipients":{},"own":[],"book":[]}"#).unwrap();
    assert_eq!(s.calls, None);
    assert!(!serde_json::to_string(&s).unwrap().contains("calls"));
    assert!(serde_json::from_str::<State>(r#"{"prefs":{}}"#).is_err());
}

#[test]
fn what_takes_trust_away() {
    let mut old = State::default();
    old.recipients.insert("1".into(), vec!["a".into(), "b".into()]);
    old.book.push(SignerAddress { name: "Mom".into(), network: "base".into(), address: "0x1".into(), memo: String::new() });
    old.prefs.insert("sessionLength".into(), Pref::Int(300));
    old.prefs.insert("agentActive".into(), Pref::String("on".into()));
    let mut less = old.clone();
    less.recipients.insert("1".into(), vec!["a".into()]);
    less.book.clear();
    less.prefs.insert("sessionLength".into(), Pref::Int(60));
    less.prefs.insert("agentActive".into(), Pref::String("paused".into()));
    assert!(takes_trust_away(&old, &less));
    let mut more = old.clone();
    more.recipients.insert("1".into(), vec!["a".into(), "c".into()]);
    assert!(!takes_trust_away(&old, &more));
    let mut longer = old.clone();
    longer.prefs.insert("sessionLength".into(), Pref::Int(1800));
    assert!(!takes_trust_away(&old, &longer));
    let mut other = old.clone();
    other.prefs.insert("activeWallet".into(), Pref::String("2".into()));
    assert!(!takes_trust_away(&old, &other));
    let mut call = old.clone();
    call.calls = Some([("1".to_string(), vec!["0xc".to_string()])].into());
    assert!(!takes_trust_away(&old, &call));
    // Limits only lowered; a budget only spent, every entry still counted kept.
    let limits = |l: signer_api::SignerLimits| Pref::Data(serde_json::to_vec(&l).unwrap());
    let standard = signer_api::SignerLimits::STANDARD;
    let mut lower = old.clone();
    lower.prefs.insert("limits".into(), limits(signer_api::SignerLimits { agent_order: 10, ..standard }));
    assert!(takes_trust_away(&old, &lower));
    assert!(!takes_trust_away(&lower, &old));
    let mut higher = old.clone();
    higher.prefs.insert("limits".into(), limits(signer_api::SignerLimits { slippage_bps: 1000, ..standard }));
    assert!(!takes_trust_away(&old, &higher));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let spent = |e: &[[f64; 2]]| Pref::Data(serde_json::to_vec(e).unwrap());
    let mut once = old.clone();
    once.prefs.insert("spent-agent".into(), spent(&[[now - 10.0, 50.0]]));
    let mut twice = old.clone();
    twice.prefs.insert("spent-agent".into(), spent(&[[now - 10.0, 50.0], [now, 20.0]]));
    assert!(takes_trust_away(&old, &once) && takes_trust_away(&once, &twice));
    assert!(!takes_trust_away(&twice, &once));
    let mut refund = once.clone();
    refund.prefs.insert("spent-agent".into(), spent(&[[now - 10.0, 50.0], [now, -40.0]]));
    assert!(!takes_trust_away(&once, &refund));
    // An entry past the 7 days may go.
    let mut aged = old.clone();
    aged.prefs.insert("spent-agent".into(), spent(&[[now - 8.0 * 86_400.0, 50.0]]));
    assert!(takes_trust_away(&aged, &old));
}

fn base64_decode(s: &str) -> Vec<u8> { use base64::Engine; base64::engine::general_purpose::STANDARD.decode(s).unwrap() }
fn base64_encode(b: &[u8]) -> String { use base64::Engine; base64::engine::general_purpose::STANDARD.encode(b) }

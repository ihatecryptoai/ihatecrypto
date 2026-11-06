//! VaultTests.swift and AgentFallbackTests.swift: the Windows signer's storage with the stand-in for Windows Hello — a
//! wallet sealed under it, one prompt per move, and settings changed outside the signer stopping every signature. One
//! process, one folder, so the cases take turns. Off Windows only: a Windows runner has the real winsec and no
//! Windows Hello set up (the Swift tests' `.enabled(if: fakeHello)`).
#![cfg(not(windows))]

use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hyperliquid::{MsgPack, Order, OrderKind, action, dec};
use serde_json::Value;
use signer::trusted::{self, State};
use signer::vault::{self, agents, recipients, seeds};
use signer::core;
use signer_api::{SignerAddress, SignerReply, SignerRequest, SignerWallet};
use wallet::Result;

/// Hardhat's public test phrase: not the «abandon … about» one, which the signer lets through without prompts.
const PHRASE: &str = "test test test test test test test test test test test junk";

static TURN: Mutex<()> = Mutex::new(());

fn env(name: &str, value: &str) { unsafe { std::env::set_var(name, value) } }

/// A clean folder and a signer as if just started.
fn fresh() -> MutexGuard<'static, ()> {
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    env("IHATECRYPTO_SIGNER_DIR", &std::env::temp_dir().join(format!("ihc-vault-tests-{}", std::process::id())).to_string_lossy());
    env("WINSEC_FAKE_HELLO", "1");
    env("WINSEC_FAKE_CANCEL", "0");
    for f in vault::files() { vault::remove(&f) }
    restart();
    turn
}

/// What a new signer process forgets.
fn restart() {
    vault::end_session();
    trusted::reload();
    agents::forget();
}

fn send(request: SignerRequest) -> Result<SignerReply> {
    vault::begin_request();
    core::handle(request)
}

fn import() -> SignerReply { send(SignerRequest::ImportMnemonic { phrase: PHRASE.into(), passphrase: String::new(), require_presence: true }).unwrap() }

fn sign_hello() -> Result<SignerReply> { send(SignerRequest::SignPersonal { message: b"hello".to_vec() }) }

/// The state a file holds, and the file put back with another body (no new seal), as a program of the same user would.
fn plant(name: &str, change: impl FnOnce(&mut State)) {
    let mut file: Value = serde_json::from_slice(&vault::read(name).unwrap()).unwrap();
    let mut state: State = serde_json::from_slice(&STANDARD.decode(file["body"].as_str().unwrap()).unwrap()).unwrap();
    change(&mut state);
    file["body"] = Value::String(STANDARD.encode(serde_json::to_vec(&state).unwrap()));
    vault::write(name, &serde_json::to_vec(&file).unwrap()).unwrap();
}

const DEAD: &str = "0x000000000000000000000000000000000000dead";

#[test]
fn a_wallet_is_sealed_under_windows_hello_and_signs() {
    let _turn = fresh();
    let imported = import();
    assert!(imported.ok);
    assert_eq!(imported.evm_address.as_deref().map(str::to_lowercase).as_deref(), Some("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"));
    assert!(vault::exists("master.json"));
    let sealed = seeds::sealed(&vault::seed_file(SignerWallet::FIRST_ID)).unwrap().unwrap();
    assert!(sealed.presence && sealed.challenge.is_some());
    let seed = wallet::mnemonic::seed(PHRASE, "").unwrap();
    assert!(!sealed.sealed.windows(64).any(|w| w == &seed[..]));
    vault::end_session();
    let signed = sign_hello().unwrap();
    assert!(signed.ok && signed.signature.is_some());
    let status = send(SignerRequest::Status {}).unwrap();
    assert_eq!(status.wallets.map(|w| w.len()), Some(1));
    assert_eq!(status.evm_address, imported.evm_address);
}

#[test]
fn a_cancelled_prompt_signs_nothing() {
    let _turn = fresh();
    import();
    vault::end_session();
    env("WINSEC_FAKE_CANCEL", "1");
    assert_eq!(sign_hello().unwrap_err().0, "Cancelled, or Windows Hello didn't match: nothing was signed.");
}

#[test]
fn settings_changed_outside_the_signer_stop_every_signature() {
    let _turn = fresh();
    import();
    vault::end_session();
    plant("state.json", |s| { s.recipients.insert("1".into(), vec![DEAD.into()]); });
    trusted::reload();
    assert_eq!(sign_hello().unwrap_err(), trusted::tampered());
}

#[test]
fn a_file_changed_and_changed_back_before_the_check_still_stops() {
    let _turn = fresh();
    import();
    vault::end_session();
    let original = vault::read("state.json").unwrap();
    plant("state.json", |s| { s.recipients.insert("1".into(), vec![DEAD.into()]); });
    trusted::reload();
    // The signer reads the planted address as approved…
    assert!(recipients::all(recipients::List::Money).contains(DEAD));
    // …the file is put back before Windows Hello opens the key, and the check still refuses.
    vault::write("state.json", &original).unwrap();
    assert_eq!(sign_hello().unwrap_err(), trusted::tampered());
}

#[test]
fn a_session_checks_the_settings_again() {
    let _turn = fresh();
    import();
    sign_hello().unwrap(); // opens a session
    plant("state.json", |s| { s.recipients.insert("1".into(), vec![DEAD.into()]); });
    assert_eq!(send(SignerRequest::SignPersonal { message: b"again".to_vec() }).unwrap_err(), trusted::tampered());
}

fn label() -> String { send(SignerRequest::Status {}).unwrap().wallets.unwrap()[0].label.clone() }

#[test]
fn a_rename_waits_for_the_next_prompt_and_only_takes_trust_away() {
    let _turn = fresh();
    let id = import().active_wallet.unwrap();
    vault::end_session();
    send(SignerRequest::RenameWallet { id: id.clone(), label: "Savings".into() }).unwrap();
    assert!(vault::exists("pending.json"));
    assert_eq!(label(), "Savings");
    // A pending file that adds an address is read as nothing.
    plant("pending.json", |s| { s.recipients.insert(id.clone(), vec![DEAD.into()]); });
    trusted::reload();
    assert!(recipients::all(recipients::List::Money).is_empty());
    assert_eq!(label(), "Main");
}

#[test]
fn the_rename_is_sealed_at_the_next_prompt() {
    let _turn = fresh();
    let id = import().active_wallet.unwrap();
    vault::end_session();
    send(SignerRequest::RenameWallet { id, label: "Savings".into() }).unwrap();
    sign_hello().unwrap();
    assert!(!vault::exists("pending.json"));
    trusted::reload();
    assert_eq!(label(), "Savings");
}

#[test]
fn what_was_planted_before_the_first_wallet_is_dropped() {
    let _turn = fresh();
    // Before any wallet needs Windows Hello nothing is sealed: a program could plant a saved address then.
    let mut state = State::default();
    state.book = vec![SignerAddress { name: "mom".into(), network: "base".into(), address: "0x000000000000000000000000000000000000dEaD".into(), memo: String::new() }];
    state.recipients.insert("1".into(), vec![DEAD.into()]);
    let file = serde_json::json!({"body": STANDARD.encode(serde_json::to_vec(&state).unwrap())});
    vault::write("state.json", &serde_json::to_vec(&file).unwrap()).unwrap();
    trusted::reload();
    import();
    assert!(vault::book::all().is_empty() && recipients::all(recipients::List::Money).is_empty());
}

#[test]
fn a_new_address_asks_windows_hello_and_is_kept() {
    let _turn = fresh();
    import();
    vault::end_session();
    vault::begin_request();
    core::approve(&["0x000000000000000000000000000000000000beef".into()], false, "send 1 USDC").unwrap();
    trusted::reload();
    assert_eq!(recipients::all(recipients::List::Money), ["0x000000000000000000000000000000000000beef".to_string()].into());
}

#[test]
fn an_allowed_call_is_a_call_only_and_pending_cant_add_one() {
    let _turn = fresh();
    let id = import().active_wallet.unwrap();
    vault::end_session();
    let contract = "0x000000000000000000000000000000000000c0de".to_string();
    vault::begin_request();
    core::approve(std::slice::from_ref(&contract), true, "call a contract").unwrap();
    trusted::reload();
    // Kept among calls, never among the places money may go.
    assert_eq!(recipients::all(recipients::List::Calls), [contract.clone()].into());
    assert!(recipients::all(recipients::List::Money).is_empty());
    vault::end_session();
    send(SignerRequest::RenameWallet { id: id.clone(), label: "Savings".into() }).unwrap();
    plant("pending.json", |s| { s.calls = Some([(id.clone(), vec![contract.clone(), DEAD.into()])].into()); });
    trusted::reload();
    assert_eq!(recipients::all(recipients::List::Calls), [contract].into());
}

#[test]
fn a_test_wallet_signs_with_no_prompt() {
    let _turn = fresh();
    env("WINSEC_FAKE_HELLO", "0");
    let made = send(SignerRequest::CreateWallet { require_presence: false }).unwrap();
    assert!(made.ok && made.phrase.is_some());
    assert!(!vault::exists("master.json"));
    assert!(sign_hello().unwrap().ok);
}

/// After a restart the trading key waits for its first Windows Hello. An order no one is there for — the guardian's
/// stop — is refused with why, never handed to the main key, and the key doesn't ask again for ten minutes; an order
/// that opens asks at once.
#[test]
fn an_unopened_trading_key_refuses_a_stop_and_waits_before_asking_again() {
    let _turn = fresh();
    import();
    send(SignerRequest::EnableAgent {}).unwrap();
    assert_eq!(send(SignerRequest::SetAgentActive { active: true }).unwrap().agent_active, Some(true));
    restart();
    let stop = action::cancel(&[(0, 1)]);
    let signed = |a: &MsgPack, nonce| send(SignerRequest::SignHyperliquidL1 { action: a.clone(), nonce, mainnet: true });

    // No one at Windows Hello: refused, and the reason says what protects the position meanwhile.
    env("WINSEC_FAKE_CANCEL", "1");
    let said = signed(&stop, 1).unwrap_err().0;
    assert!(said.contains("wasn't opened") && said.contains("stop and take on the exchange"), "{said}");
    // Ten quiet minutes: someone there now still isn't asked by a stop.
    env("WINSEC_FAKE_CANCEL", "0");
    assert!(signed(&stop, 2).is_err());
    // An order that opens comes from a card just confirmed: it asks, opens the key and signs — through the trading key,
    // whose week now runs (the Swift Windows signer lost it and sent this to the main key).
    let open = action::order(&[Order { asset: 1, is_buy: true, price: dec("100"), size: dec("1"), reduce_only: false,
                                       kind: OrderKind::Limit { tif: "Gtc".into() }, cloid: None }], "na").unwrap();
    assert!(signed(&open, 3).unwrap().signature.is_some());
    // Open now: the next stop goes with no one there.
    env("WINSEC_FAKE_CANCEL", "1");
    assert!(signed(&stop, 4).unwrap().signature.is_some());
}

// AgentKeyTests.swift: the trading key sealed under Windows Hello as a seed is, opened once per run, its address
// readable without opening it; a DPAPI-only key is taken only by a wallet that signs with no one there.

#[test]
fn the_trading_key_is_sealed_and_opens_once_per_run() {
    let _turn = fresh();
    import();
    let id = SignerWallet::FIRST_ID;
    let agent = send(SignerRequest::EnableAgent {}).unwrap().agent_address.unwrap();
    assert!(vault::exists(&agents::file(id)) && !vault::exists(&agents::plain(id)));
    restart();
    // With no one at Windows Hello, the status still shows the key — and nothing opens it.
    env("WINSEC_FAKE_CANCEL", "1");
    assert_eq!(send(SignerRequest::Status {}).unwrap().agent_address.as_deref(), Some(agent.as_str()));
    vault::begin_request();
    assert!(agents::load(id).is_none());
    // One prompt opens it; after that it's in memory, with no one needed.
    env("WINSEC_FAKE_CANCEL", "0");
    agents::ask_again();
    let key = agents::load(id).unwrap();
    assert_eq!(wallet::keys::evm_address(&key).unwrap(), agent);
    env("WINSEC_FAKE_CANCEL", "1");
    assert_eq!(agents::load(id), Some(key.clone()));
    // Not the key in the clear, even under the stand-in's «DPAPI».
    let file = vault::unprotect(&vault::read(&agents::file(id)).unwrap()).unwrap();
    assert!(!file.windows(32).any(|w| w == &key[..]));
    // Any Windows Hello the signer asks for opens it too.
    env("WINSEC_FAKE_CANCEL", "0");
    restart();
    sign_hello().unwrap();
    env("WINSEC_FAKE_CANCEL", "1");
    assert_eq!(agents::load(id), Some(key));
}

/// S9: a key file another program puts beside a wallet whose keys are sealed is never taken: not shown, not used for an
/// order, and turning the key on approves a new one.
#[test]
fn a_planted_trading_key_is_never_taken_nor_approved() {
    let _turn = fresh();
    import();
    let id = SignerWallet::FIRST_ID;
    let planted = agents::make().unwrap();
    let planted_address = wallet::keys::evm_address(&planted).unwrap();
    vault::write(&agents::plain(id), &vault::protect(&planted).unwrap()).unwrap();
    restart();
    assert!(!agents::has(id) && agents::address(id).is_none());
    vault::begin_request();
    assert!(agents::load(id).is_none());
    let agent = send(SignerRequest::EnableAgent {}).unwrap().agent_address.unwrap();
    assert_ne!(agent.to_lowercase(), planted_address.to_lowercase());
    assert!(vault::exists(&agents::file(id)) && !vault::exists(&agents::plain(id)));
    // Put back after the sealed key is deleted: still never taken.
    vault::remove(&agents::file(id));
    vault::write(&agents::plain(id), &vault::protect(&planted).unwrap()).unwrap();
    restart();
    assert!(agents::address(id).is_none());
    vault::begin_request();
    assert!(agents::load(id).is_none());
    // A Ledger's trading key is sealed too: a file planted for it isn't one.
    let ledger = "ledger-0x70997970c51812dc3a010c7d01b50e0d17dc79c8";
    vault::write(&agents::plain(ledger), &vault::protect(&planted).unwrap()).unwrap();
    assert!(!agents::has(ledger) && agents::address(ledger).is_none());
}

#[test]
fn a_sealed_keys_address_is_taken_only_as_the_trusted_state_recorded_it() {
    let _turn = fresh();
    import();
    let id = SignerWallet::FIRST_ID;
    send(SignerRequest::EnableAgent {}).unwrap();
    let json = vault::unprotect(&vault::read(&agents::file(id)).unwrap()).unwrap();
    let mut sealed: Value = serde_json::from_slice(&json).unwrap();
    sealed["address"] = Value::String("0x000000000000000000000000000000000000dEaD".into());
    vault::write(&agents::file(id), &vault::protect(&serde_json::to_vec(&sealed).unwrap()).unwrap()).unwrap();
    restart();
    assert!(agents::address(id).is_none());
}

#[test]
fn a_wallet_that_signs_with_no_one_there_keeps_its_trading_key_under_dpapi() {
    let _turn = fresh();
    env("WINSEC_FAKE_HELLO", "0");
    let id = send(SignerRequest::CreateWallet { require_presence: false }).unwrap().active_wallet.unwrap();
    assert!(!agents::sealed_for(&id));
    let key = agents::make().unwrap();
    agents::save(&key, &id, None).unwrap();
    assert!(vault::exists(&agents::plain(&id)) && !vault::exists(&agents::file(&id)));
    restart();
    assert_eq!(agents::load(&id), Some(key));
}

// CodeTests.swift: the passcode kept as a stretched hash under DPAPI, a try counted before it's checked, the fifth wrong
// one in a row turning it off, and a damaged file opening nothing without Windows Hello.

fn enter(code: &str) -> SignerReply { send(SignerRequest::EnterCode { code: code.into(), sign: false }).unwrap() }

#[test]
fn the_passcode_is_checked_and_five_wrong_ones_turn_it_off() {
    let _turn = fresh();
    import();
    assert!(send(SignerRequest::SetCode { code: "123456".into() }).is_err());
    assert_eq!(send(SignerRequest::SetCode { code: "481902".into() }).unwrap().code_set, Some(true));
    assert_eq!(send(SignerRequest::Status {}).unwrap().code_set, Some(true));
    let file = vault::unprotect(&vault::read("code.json").unwrap()).unwrap();
    assert!(!String::from_utf8_lossy(&file).contains("481902"), "the code is kept as it was typed");
    let wrong = enter("481903");
    assert!(!wrong.ok && wrong.error.as_deref() == Some("Wrong code · 4 tries left") && wrong.code_set == Some(true));
    assert!(enter("481902").ok);
    // A right code starts the count again: four wrong ones after it still leave the code on.
    for _ in 0..4 { enter("000001"); }
    assert_eq!(send(SignerRequest::Status {}).unwrap().code_set, Some(true));
    let fifth = enter("000001");
    assert!(!fifth.ok && fifth.code_off == Some(true) && fifth.error.as_deref().is_some_and(|e| e.contains("Windows Hello")), "{:?}", fifth.error);
    assert_eq!(fifth.error.as_deref(), Some("5 wrong codes: the code is off. Only Windows Hello opens ihatecrypto now."));
    assert_eq!(send(SignerRequest::Status {}).unwrap().code_set, Some(false));
    assert!(!vault::exists("code.json"));
    // Even the right code opens nothing now.
    assert_eq!(enter("481902").code_off, Some(true));
}

#[test]
fn a_damaged_passcode_file_opens_nothing_without_windows_hello() {
    let _turn = fresh();
    import();
    send(SignerRequest::SetCode { code: "481902".into() }).unwrap();
    vault::write("code.json", b"{}").unwrap();
    let tried = enter("481902");
    assert!(!tried.ok && tried.code_off == Some(true));
}

// LedgerAgentKeyTests.swift (S10, R9-1): with only a Ledger, before anything asked Windows Hello, its trading key is still
// sealed, so a file another program put there is never the key the Ledger approves.

#[test]
fn a_ledgers_trading_key_is_sealed_even_before_any_windows_hello() {
    let _turn = fresh();
    let owner = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
    let id = format!("ledger-{}", owner.to_lowercase());
    assert!(vault::master().is_none());
    let planted = agents::make().unwrap();
    let planted_address = wallet::keys::evm_address(&planted).unwrap();
    vault::write(&agents::plain(&id), &vault::protect(&planted).unwrap()).unwrap();
    restart();
    let made = send(SignerRequest::LedgerAgent { owner: owner.into(), create: true }).unwrap().agent_address.unwrap();
    assert_ne!(made.to_lowercase(), planted_address.to_lowercase());
    assert!(vault::master().is_some());
    assert!(vault::exists(&agents::file(&id)) && !vault::exists(&agents::plain(&id)));
    restart();
    assert!(agents::has(&id) && agents::address(&id).as_deref() == Some(made.as_str()));
    assert_eq!(send(SignerRequest::LedgerAgent { owner: owner.into(), create: true }).unwrap().agent_address.as_deref(), Some(made.as_str()));
    agents::ask_again();
    vault::begin_request();
    assert_eq!(wallet::keys::evm_address(&agents::load(&id).unwrap()).unwrap(), made);
}

/// The binary itself over its pipes: a frame in, a frame out, as the engine talks to it.
#[test]
fn the_signer_answers_over_its_pipes() {
    let _turn = fresh();
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_ihatecrypto-signer")).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let request = serde_json::to_vec(&SignerRequest::Status {}).unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        signer_api::write_frame(stdin, &request).unwrap();
        stdin.flush().unwrap();
    }
    let body = signer_api::read_frame(child.stdout.as_mut().unwrap()).unwrap();
    drop(child.stdin.take());
    let reply: SignerReply = serde_json::from_slice(&body).unwrap();
    assert!(reply.ok && reply.has_wallet == Some(false));
    assert!(reply.checks.iter().any(|c| c.contains("Windows Hello")), "{:?}", reply.checks);
    assert!(child.wait().unwrap().success());
}

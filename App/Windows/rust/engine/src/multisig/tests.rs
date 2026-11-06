//! Multisig, offline: the keys' words, the set-up card's command and candidates, and every refusal said before anything
//! is read from a network or signed.

use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::position_command::WalletCommand;
use signer_api::{SignerAddress, SignerReply, SignerWallet};

use super::{MultisigDraft, MultisigKey, is_evm, word_decimal};
use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

const PC: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const PHONE: &str = "0x70997970c51812dc3a010c7d01b50e0d17dc79c8";

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("drawn while locked"));
    e.st_mut().lock.locked = true;
    e
}

fn key(name: &str, address: &str, kind: &str) -> MultisigKey { MultisigKey { name: name.into(), address: address.into(), kind: kind.into() } }

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

async fn locally(test: impl AsyncFnOnce(Rc<Engine>)) {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async { test(engine()).await }).await;
}

#[tokio::test]
async fn keys_are_counted_in_both_languages() {
    locally(async |e| {
        assert_eq!((e.keys_word(1), e.keys_word(3)), ("1 key".into(), "3 keys".into()));
        e.set_settings(|s| s.language = Some("ru".into()));
        let words: Vec<String> = [1, 2, 4, 5, 11, 12, 21, 22, 25, 101, 111].into_iter().map(|n| e.keys_word(n)).collect();
        assert_eq!(words, ["1 ключ", "2 ключа", "4 ключа", "5 ключей", "11 ключей", "12 ключей", "21 ключ", "22 ключа", "25 ключей", "101 ключ", "111 ключей"]);
    }).await;
}

#[test]
fn the_card_s_command_names_the_keys_that_aren_t_this_pc() {
    let d = MultisigDraft { keys: vec![key("This PC", PC, "pc"), key("Phone", PHONE, "address")], threshold: 2, networks: vec!["base".into()], salt: 7, address: None };
    assert_eq!(d.majority(), 2);
    assert_eq!(Engine::command(&d), WalletCommand::MultisigCreate { threshold: Some(2), of: Some(2), keys: vec!["Phone".into()], networks: vec!["base".into()] });
    assert!(is_evm(PC) && !is_evm(&PC[..41]) && !is_evm("0xzz58effd232b4033e47d90003d41ec34ecaeda94"));
    assert_eq!(word_decimal(&[[0u8; 31].as_slice(), &[42]].concat()), Some(42.into()));
    assert_eq!(word_decimal(&[1, 2]), None);
}

#[tokio::test]
async fn candidates_are_saved_evm_addresses_that_aren_t_keys_already() {
    locally(async |e| {
        assert!(e.multisig_candidates().is_empty());
        let saved = |name: &str, network: &str, address: &str| SignerAddress { name: name.into(), network: network.into(), address: address.into(), memo: String::new() };
        {
            let mut st = e.st_mut();
            st.status = Some(SignerReply { evm_address: Some(PC.into()), wallets: Some(vec![SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(),
                solana: None, evm: Some(PC.to_uppercase().replace("0X", "0x")), near: None, test: None, automation: None }]), ..Default::default() });
            st.book = vec![saved("Phone", "base", "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"), saved("Phone again", "arbitrum", PHONE), saved("Me", "base", PC),
                           saved("Sol", "solana", "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"), saved("HL", "hyperliquid", "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc")];
            st.multisig_draft = Some(MultisigDraft { keys: vec![key("This PC", PC, "pc")], threshold: 1, networks: vec!["base".into()], salt: 1, address: None });
        }
        assert_eq!(e.pc_key().as_deref(), Some(PC));
        let candidates = e.multisig_candidates();
        assert_eq!(candidates, [key("Phone", PHONE, "address"), key("HL", "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc", "address")]);
        // An address that isn't one can't be added from the card.
        let refused = e.multisig_key(Some("0x0000000000000000000000000000000000000001"), None).unwrap_err();
        assert_eq!(refused.0, "That address can't be a key here.");
    }).await;
}

#[tokio::test]
async fn nothing_is_made_or_paid_without_its_card() {
    locally(async |e| {
        e.prepare_multisig(&WalletCommand::MultisigCreate { threshold: None, of: None, keys: vec![], networks: vec![] }, None).await;
        assert_eq!(last(&e), "A multisig needs this computer's EVM key: add a wallet with a phrase first.");
        e.show_multisig(None);
        assert_eq!(last(&e), "There's no multisig yet: say «make a multisig».");
        e.prepare_multisig_send(&WalletCommand::MultisigSend { token: "USDC".into(), amount: None, to: "Alex".into(), network: None }, None).await;
        assert_eq!(last(&e), "There's no multisig yet: say «make a multisig».");
        assert_eq!(e.run_multisig_create().await.unwrap_err().to_string(), "The multisig's card is gone. Ask for it again.");
        // The same key twice: refused before anything is read or signed.
        e.st_mut().multisig_draft = Some(MultisigDraft { keys: vec![key("This PC", PC, "pc"), key("Phone", PC, "address")], threshold: 2, networks: vec!["base".into()], salt: 1, address: None });
        assert_eq!(e.run_multisig_create().await.unwrap_err().to_string(), "The same key is in the list twice. Nothing was signed.");
        let send = WalletCommand::MultisigSend { token: "USDC".into(), amount: None, to: "Alex".into(), network: Some("base".into()) };
        assert_eq!(e.run_multisig_send(&send).await.unwrap_err().to_string(), "The multisig payment's check is gone. Ask for it again.");
    }).await;
}

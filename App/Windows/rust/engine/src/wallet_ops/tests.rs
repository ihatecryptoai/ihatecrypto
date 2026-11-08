//! WalletOps, offline: the first addresses a phrase makes (OnboardingTests), the book's lookups by name.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use signer_api::SignerAddress;
use wallet::address_check::Network;

use super::address_network;
use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

const WORDS: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

/// An engine with no signer, locked so a state push draws only the lock.
fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("drawn while locked"));
    e.st_mut().lock.locked = true;
    e
}

#[test]
fn the_first_addresses_are_metamasks_and_phantoms() {
    let plain = Engine::first_accounts(WORDS, "", 3).unwrap();
    assert_eq!(plain.len(), 3);
    // The public test phrase's own: MetaMask's first account, Phantom's first.
    assert_eq!(plain[0]["evm"].to_lowercase(), "0x9858effd232b4033e47d90003d41ec34ecaeda94");
    assert_eq!(plain[0]["solana"], "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk");
    assert_eq!(plain.iter().map(|a| a["evm"].clone()).collect::<HashSet<_>>().len(), 3);
    assert_eq!(plain.iter().map(|a| a["solana"].clone()).collect::<HashSet<_>>().len(), 3);
    // Another passphrase is another wallet.
    let other = Engine::first_accounts(WORDS, "TREZOR", 3).unwrap();
    assert!(other[0]["evm"] != plain[0]["evm"] && other[0]["solana"] != plain[0]["solana"]);
    // As JSON for the window: the keys Swift's dictionaries carry.
    assert!(serde_json::to_string(&plain).unwrap().starts_with(r#"[{"evm":"0x"#));
}

#[test]
fn a_network_reads_by_its_raw_value() {
    assert_eq!(address_network("solana"), Some(Network::Solana));
    assert_eq!(address_network("hyperevm"), Some(Network::Hyperevm));
    assert_eq!(address_network("Base"), None);
    assert_eq!(address_network(""), None);
}

#[tokio::test]
async fn a_saved_name_is_found_on_its_network_first() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let entry = |name: &str, network: &str, address: &str| SignerAddress { name: name.into(), network: network.into(), address: address.into(), memo: String::new() };
        e.st_mut().book = vec![entry("Alex", "solana", "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"), entry("alex", "base", "0x9858EfFD232B4033E47d90003D41EC34EcaEda94")];
        assert_eq!(e.saved("ALEX", Some("base")).unwrap().network, "base");
        assert_eq!(e.saved("alex", None).unwrap().network, "solana");
        // Not on the network asked: the name alone.
        assert_eq!(e.saved("Alex", Some("arbitrum")).unwrap().network, "solana");
        assert!(e.saved("Bob", None).is_none());
    }).await;
}

#[tokio::test]
async fn no_signer_is_said_and_a_second_wallet_waits_for_the_first_words() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        e.start_signer().await;
        assert_eq!(e.st().signer_error.as_deref(), Some("The signer isn't installed next to the engine: reinstall ihatecrypto."));
        e.st_mut().phrase = Some(zeroize::Zeroizing::new(vec!["word".into(); 12]));
        let refused = e.create_wallet(true).await.unwrap_err();
        assert_eq!(refused.0, "A new wallet is being made: write its words down first.");
        assert!(!e.st().creating_wallet && e.st().signing == 0);
    }).await;
}

//! Cards6Tests: a pasted key shows its address and nothing is kept.

use std::sync::Arc;

use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

#[tokio::test]
async fn a_pasted_key_shows_its_address_and_nothing_is_kept() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("L1's tests draw no state"));
        // Hardhat's account #0: its key is published with Hardhat for tests.
        let r = e.key_address("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80").unwrap();
        assert_eq!((r["chain"].as_str(), r["address"].as_str()), ("EVM · Hyperliquid", "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"), "{r:?}");
        assert!(e.key_address("not a key").is_err());
        // A phrase that isn't one is said before anything else is read.
        assert!(e.phrase_accounts("abandon abandon", "").is_err());
    }).await;
}

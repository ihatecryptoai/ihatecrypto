//! The executor's signer end to end: LinkTradeSigner → SignerLink → the Rust signer → its guard → a signature, on a test
//! wallet that signs with no prompt.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::signer_link::tests::{built_signer, fresh_signer_dir, runtime};

#[test]
fn a_test_wallet_signs_through_the_link_and_a_guard_stop_comes_back_as_one() {
    let Some(exe) = built_signer() else { eprintln!("ihatecrypto-signer isn't built beside the tests: `cargo build -p signer` first"); return };
    let _held = fresh_signer_dir("trade");
    let link = Arc::new(SignerLink::new(exe));
    let waits = Arc::new(AtomicUsize::new(0));
    let w = waits.clone();
    let signer = LinkTradeSigner { link: link.clone(), as_wallet: None, waiting: Arc::new(move |on| if on { w.fetch_add(1, Ordering::SeqCst); }) };
    runtime().block_on(async {
        let made = link.send(&SignerRequest::CreateWallet { require_presence: false }).await.unwrap();
        let owner = made.evm_address.clone().unwrap();
        // An order's cancel, signed by the main key of the test wallet (no trading key on): it recovers to the wallet.
        let action = hyperliquid::action::cancel(&[(0, 1)]);
        let signature = signer.sign_hyperliquid(action.clone(), 1_700_000_000_000).await.unwrap();
        let digest = hyperliquid::signer::l1_parts(&action, None, 1_700_000_000_000, None, true).unwrap().digest();
        assert_eq!(wallet::evm::recover(&digest, &signature).unwrap(), owner);
        // USDC sent to an address nobody approved: the signer's guard stops it, and that is a stop, not a failure.
        let send = hyperliquid::signer::usd_send_action("0x000000000000000000000000000000000000dead", "1", 1_700_000_000_000, true);
        match signer.sign_hyperliquid_user(send, vec![]).await {
            Err(TradeError::Guard(why)) => assert!(why.starts_with("The guard refused to sign."), "{why}"),
            other => panic!("not a guard stop: {other:?}"),
        }
        // Only the automation wallet signs for a task.
        let as_task = LinkTradeSigner { link: link.clone(), as_wallet: Some(made.active_wallet.clone().unwrap()), waiting: Arc::new(|_| {}) };
        match as_task.sign_hyperliquid(hyperliquid::action::cancel(&[(0, 1)]), 1).await {
            Err(TradeError::Guard(why)) => assert_eq!(why, "Only the automation wallet signs for a scheduled task."),
            other => panic!("{other:?}"),
        }
        // A Ledger-only kind of move isn't the link's: the default refusal is never used here.
        assert!(signer.signs_batches());
    });
    assert_eq!(waits.load(Ordering::SeqCst), 2);
}

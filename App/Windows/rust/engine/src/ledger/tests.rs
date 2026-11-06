//! The Ledger's words in English and Russian, forgetting it, and the transport off Windows; offline, no device.

use std::rc::Rc;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::snapshot::Snapshot;
use crate::store;

fn engine() -> Rc<Engine> {
    fn draw(_: &Engine) -> Snapshot { unreachable!("the test window is locked") }
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), draw);
    e.st_mut().lock.locked = true;
    e.st_mut().settings.language = Some("en".into());
    e
}

#[test]
fn the_ledgers_words() {
    for (text, re) in [("use my Ledger", &USE), ("switch to ledger.", &USE), ("используй мой леджер", &USE), ("подписывай на леджере", &USE),
                       ("forget my ledger", &FORGET), ("отключи леджер!", &FORGET),
                       ("connect ledger", &CONNECT), ("подключи мой леджер", &CONNECT), ("ledger nano x", &CONNECT), ("set up a Ledger?", &CONNECT)] {
        assert!(matches(re, &text.to_lowercase()), "{text}");
    }
    for text in ["what's a ledger?", "use my ledger to buy SOL", "connect ledger and send 5 USDC"] {
        let lower = text.to_lowercase();
        assert!(matches(&MENTIONS, &lower) && ![&USE, &FORGET, &CONNECT].iter().any(|re| matches(re, &lower)), "{text}");
    }
}

#[tokio::test]
async fn forgetting_a_ledger_that_isnt_there_says_so() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        assert!(e.ledger_command("forget my Ledger").await);
        assert_eq!(e.st().messages().last().map(|m| m.text.as_str()), Some("No Ledger is connected."));
        // «use my Ledger» with none connected isn't a command; nor is a question.
        assert!(!e.ledger_command("use my Ledger").await);
        assert!(!e.ledger_command("is a Ledger safer?").await);
        assert!(!e.ledger_reachable().await);
    }).await;
}

#[cfg(not(windows))]
#[tokio::test]
async fn off_windows_no_ledger_answers() {
    let device = Device::new(LedgerAccount::default().transport());
    assert_eq!(device.open_app().await.map_err(|e| e.message), Err(NO_DEVICE.to_string()));
    assert!(usb_connected().is_none() && bluetooth_connect(Duration::from_secs(1)).await.is_err());
}

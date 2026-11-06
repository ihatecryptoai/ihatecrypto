//! Live13Tests' «the Ledger's trading key is never the main wallet's», and the key's view, words and cards, offline.

use std::rc::Rc;
use std::sync::Arc;

use signer_api::{SignerReply, SignerWallet};

use super::*;
use crate::ledger::LedgerAccount;
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

const ME: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const KEY: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";

fn last(e: &Engine) -> String { e.st().messages().last().map(|m| m.text.clone()).unwrap_or_default() }

#[tokio::test]
async fn the_ledgers_trading_key_is_never_the_main_wallets_while_the_ledger_is_off() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        assert!(!e.ledger_active());
        assert!(e.trading_key_command("approve the trading key on my Ledger").await);
        assert!(last(&e).starts_with("Switch to your Ledger first («use my Ledger»)"), "{}", last(&e));
        assert!(e.st().pending.is_none() && e.st().card.is_none(), "a card for the main wallet's key");
        e.st_mut().settings.language = Some("ru".into());
        assert!(e.trading_key_command("Одобри торговый ключ на леджере").await);
        assert!(last(&e).starts_with("Сначала переключись на Ledger"));
        // Other words aren't the command.
        assert!(!e.trading_key_command("what is a trading key?").await);
    }).await;
}

#[tokio::test]
async fn the_keys_view_follows_the_wallet_that_signs() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let v = e.trading_key_view();
        assert!(v.address.is_none() && !v.active && !v.paused && !v.ledger && !v.possible);
        assert_eq!((v.fee_rate.as_str(), v.referral.as_str()), ("0.05%", "IHATECRYPTO"));
        let w = SignerWallet { id: "1".into(), label: "Main".into(), kind: "phrase".into(), solana: None, evm: Some(ME.into()), near: None, test: None, automation: None };
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), evm_address: Some(ME.into()), wallets: Some(vec![w]), active_wallet: Some("1".into()),
                                               agent_address: Some(KEY.into()), agent_active: Some(false), ..SignerReply::default() });
        let v = e.trading_key_view();
        assert!(v.address.as_deref() == Some(KEY) && !v.active && v.paused && v.possible);
        // The Ledger on: its own key, none until read.
        e.set_ledger(|l| *l = Some(LedgerAccount { evm: Some(ME.into()), active: Some(true), ..LedgerAccount::default() }));
        assert!(e.agent_address().is_none() && !e.agent_active() && e.trading_key_view().ledger);
        e.st_mut().ledger_agent = Some(SignerReply { ok: true, agent_address: Some(KEY.into()), agent_active: Some(true), ..SignerReply::default() });
        assert!(e.agent_active() && e.agent_address().as_deref() == Some(KEY));
    }).await;
}

#[tokio::test]
async fn the_offers_not_now_card() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        assert!(!e.answer_trading_key("something else").await);
        assert!(e.answer_trading_key(TRADING_KEY_LATER).await);
        assert!(last(&e).starts_with("Left off. Settings › Wallet › Hyperliquid turns it on any time"));
        // Nothing to offer without money on Hyperliquid.
        e.offer_trading_key();
        assert!(e.st().settings.trading_key_offered.is_none());
    }).await;
}

//! Consent as the Mac asks it (Consent6Tests, Consent7Tests): a site's request in its own block, answered by its id and
//! only with a tick for each risk; it goes with its page or its canvas; a card the second check doubts confirms only with
//! its tick. In-process, public test data only, nothing goes to the network or the signer.

use std::sync::Arc;

use tokio::sync::oneshot;

use super::*;
use crate::browser::SiteAsk;
use crate::engine::Card;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

// Hardhat's public account #3, and a public contract (AERO on Base).
const MOM: &str = "0x90F79bf6EB2c4f870365E785982E1f101E93b906";
const CONTRACT: &str = "0x940181a94A35A4569E4529A3CDfB74e38FD98631";

fn en(a: &str, _: &str) -> String { a.into() }

fn with_engine(test: impl FnOnce(&Engine)) {
    let _dir = store::test_folder();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    tokio::task::LocalSet::new().block_on(&rt, async {
        let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("the tests keep the window locked"));
        // Locked, a state push sends the lock alone: no whole Snapshot is drawn here.
        e.st_mut().lock.locked = true;
        e.set_settings(|s| s.language = Some("en".into()));
        test(&e);
    });
}

/// A site's request waiting in its own block (what browser.rs's ask leaves), and how the page hears the answer.
fn ask(e: &Engine, origin: &str, title: &str, ticks: Vec<SiteTick>) -> (String, oneshot::Receiver<bool>) {
    let board = Board::new("site", title);
    let id = board.id.clone();
    e.set_boards(|b| b.insert(0, board));
    let host = origin.trim_start_matches("https://").to_string();
    let view = SiteAskView {
        id: id.clone(), origin: origin.into(), host, check: "known".into(), real: None, title: title.into(), lines: vec![], warnings: vec![], ticks,
        approve: "Sign".into(), canvas: e.st().canvas_id.clone(), lead: None, balances: vec![], checks: vec![], note: None,
    };
    let (tx, rx) = oneshot::channel();
    e.st_mut().site_ask = Some(SiteAsk::new(view, tx));
    (id, rx)
}

fn waiting(e: &Engine) -> Option<String> { e.st().site_ask.as_ref().map(|a| a.view.id.clone()) }

fn block(e: &Engine, id: &str) -> Board { e.st().boards().iter().find(|b| b.id == id).cloned().expect("its block") }

fn ticks(ids: &[&str]) -> Vec<String> { ids.iter().map(|s| s.to_string()).collect() }

#[test]
fn a_site_request_needs_a_tick_for_each_risk() {
    let payee = Engine::payee_tick(CONTRACT, &[MOM.to_lowercase()], false, en).unwrap();
    assert_eq!((payee.id.as_str(), payee.toggle.as_str()), ("payee", "Send to this address"));
    assert!(payee.detail.starts_with("0x90f79bf6…93b906 — check it on the site."), "{}", payee.detail);
    assert_eq!(Engine::payee_tick(CONTRACT, &[CONTRACT.to_lowercase()], true, en).unwrap().id, "contract");
    assert_eq!(Engine::payee_tick(CONTRACT, &[], false, en), None);
    assert_eq!(Engine::programs_tick(&[], en), None);
    let one = Engine::programs_tick(&ticks(&["JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"]), en).unwrap();
    assert_eq!((one.title.as_str(), one.toggle.as_str()), ("JUP6…TaV4 isn't on ihatecrypto's list", "Allow it for this one transaction"));
    assert_eq!(Engine::programs_tick(&ticks(&["A", "B"]), |_: &str, b: &str| b.to_string()).unwrap().toggle, "Разрешить для этой транзакции");

    with_engine(|e| {
        let lookalike = Engine::lookalike_tick("app.uniswap.org", en);
        assert_eq!(lookalike.toggle, "I understand this isn't app.uniswap.org");
        let (id, mut answer) = ask(e, "https://app-uniswap.org", "app-uniswap.org asks to send a transaction",
                                   vec![lookalike, payee, Engine::jev_tick("It pays an address the site named.", en)]);
        for partial in [&[][..], &["lookalike"], &["lookalike", "payee"], &["payee", "jev"]] {
            assert!(e.answer_site(Some(&id), true, &ticks(partial)).is_err(), "{partial:?}");
            assert_eq!(waiting(e).as_deref(), Some(id.as_str()));
        }
        assert_eq!(e.answer_site(Some(&id), true, &[]).unwrap_err().0, "Tick «I understand this isn't app.uniswap.org» first, or decline.");
        // Another block's id answers nothing.
        e.answer_site(Some("another"), true, &ticks(&["lookalike", "payee", "jev"])).unwrap();
        e.answer_site(None, true, &ticks(&["lookalike", "payee", "jev"])).unwrap();
        assert_eq!(waiting(e).as_deref(), Some(id.as_str()));
        e.answer_site(Some(&id), true, &ticks(&["lookalike", "payee", "jev"])).unwrap();
        assert_eq!(answer.try_recv(), Ok(true));
        assert_eq!((waiting(e), block(e, &id).state.as_deref()), (None, Some("done")));

        // Declining needs no tick.
        let (next, mut declined) = ask(e, "https://app-uniswap.org", "again", vec![Engine::lookalike_tick("app.uniswap.org", en)]);
        e.answer_site(Some(&next), false, &[]).unwrap();
        assert_eq!(declined.try_recv(), Ok(false));
        let b = block(e, &next);
        assert_eq!((b.state.as_deref(), b.lines.last().map(String::as_str)), (Some("cancelled"), Some("Declined: nothing was signed.")));
    });
}

#[test]
fn a_sites_request_goes_with_its_page_and_its_canvas() {
    with_engine(|e| {
        let (id, mut answer) = ask(e, "https://jup.ag", "jup.ag asks to connect", vec![]);
        // Another page going away leaves it; its own page going away declines it, and that's on its block.
        e.site_gone("https://app.hyperliquid.xyz");
        assert_eq!(waiting(e).as_deref(), Some(id.as_str()));
        e.site_gone("HTTPS://JUP.AG/");
        assert_eq!((answer.try_recv(), waiting(e)), (Ok(false), None));
        let b = block(e, &id);
        assert_eq!(b.state.as_deref(), Some("cancelled"));
        assert!(b.lines.last().unwrap().contains("The page closed before an answer"));
        // The canvas it's on is deleted: declined too, its ticks were only there.
        let (id, mut answer) = ask(e, "https://jup.ag", "jup.ag asks to sign", vec![]);
        e.decline_site_on_canvas(&[]);
        assert_eq!(waiting(e).as_deref(), Some(id.as_str()));
        let boards = e.st().boards().clone();
        e.decline_site_on_canvas(&boards);
        assert_eq!((answer.try_recv(), waiting(e)), (Ok(false), None));
        assert_eq!(block(e, &id).lines.last().unwrap(), "Its canvas was deleted: nothing was signed.");
        // Nothing waiting: nothing to decline.
        e.site_gone("https://jup.ag");
        e.decline_site("test");
    });
}

#[test]
fn a_doubted_card_confirms_only_with_its_tick() {
    with_engine(|e| {
        assert_eq!(e.unticked_on_card(&[]), None);
        let doubt = Engine::trade_tick("The plan risks $50, the message named $5.", en);
        e.st_mut().card = Some(Card { id: "B1".into(), title: "Long ETH".into(), lines: vec![], warnings: vec![], confirm: "Confirm".into(), ticks: vec![doubt], over_limit: false });
        assert_eq!(e.unticked_on_card(&[]).unwrap(), "Tick «I understand the second check's warning and execute anyway» on the card first, or cancel it.");
        assert!(e.unticked_on_card(&ticks(&["other"])).is_some());
        assert_eq!(e.unticked_on_card(&ticks(&["doubt"])), None);
        e.set_settings(|s| s.language = Some("ru".into()));
        e.st_mut().card.as_mut().unwrap().ticks = vec![Engine::trade_tick("…", |_: &str, b: &str| b.to_string())];
        assert_eq!(e.unticked_on_card(&[]).unwrap(), "Сначала отметь на карточке «Понимаю предупреждение второй проверки и выполняю всё равно» или отмени её.");
        // A card with nothing to tick confirms as before.
        e.st_mut().card.as_mut().unwrap().ticks.clear();
        assert_eq!(e.unticked_on_card(&[]), None);
    });
}

#[test]
fn modules_that_are_on_refuse_nothing_at_confirm() {
    with_engine(|e| {
        let mom = signer_api::SignerAddress { name: "Mom".into(), network: "base".into(), address: MOM.into(), memo: String::new() };
        assert_eq!(e.confirm_refusal(&Pending::Transfer(mom, rust_decimal::Decimal::ONE)), None);
        assert_eq!(e.confirm_refusal(&Pending::Wallet(WalletCommand::ClaimRewards)), None);
    });
}

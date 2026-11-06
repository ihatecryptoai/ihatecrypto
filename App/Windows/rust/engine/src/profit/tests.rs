//! The portfolio's points and flows, offline: a point at most every ten minutes, a year kept, the week's line thinned
//! to 200 with the first kept, and money moved in kept per wallet.

use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use signer_api::SignerReply;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::writer(std::io::sink()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("locked: no state is drawn"));
    e.st_mut().lock.locked = true;
    e
}

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(test));
}

#[test]
fn every_nth_point_is_kept_the_first_among_them() {
    assert_eq!(Engine::thinned((0..10).collect(), 3), [0, 4, 8]);
    assert_eq!(Engine::thinned((0..10).collect(), 5), [0, 2, 4, 6, 8]);
    assert_eq!(Engine::thinned((0..3).collect(), 5), [0, 1, 2]);
    assert_eq!(Engine::thinned((0..3).collect::<Vec<i32>>(), 0), [0, 1, 2]);
    assert_eq!(Engine::thinned((0..401).collect(), 200).len(), 134);
}

#[test]
fn a_point_is_kept_at_most_every_ten_minutes_for_a_year() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        let now = chrono::Utc::now();
        let old = EquityPoint { at: now - chrono::Duration::days(400), usd: Decimal::ONE, wallet: "So1".into() };
        let week = EquityPoint { at: now - chrono::Duration::days(3), usd: Decimal::from(90), wallet: "So1".into() };
        let other = EquityPoint { at: now - chrono::Duration::days(2), usd: Decimal::from(5), wallet: "Other".into() };
        store::save(&vec![old, week, other], "equity.json");
        e.record_equity(Decimal::from(100), "So1");
        e.record_equity(Decimal::from(101), "So1");
        let points: Vec<EquityPoint> = store::load("equity.json").unwrap();
        // The year-old point went; the second within ten minutes wasn't kept; another wallet's stays.
        assert_eq!(points.iter().map(|p| (p.wallet.as_str(), p.usd)).collect::<Vec<_>>(),
                   [("So1", Decimal::from(90)), ("Other", Decimal::from(5)), ("So1", Decimal::from(100))]);
        // The line on screen: the wallet's own week, no point for now until the balances are in.
        e.set_settings(|s| s.watch_solana = Some("So1".into()));
        e.load_equity();
        let st = e.st();
        assert_eq!(st.portfolio.equity.iter().map(|p| p.usd).collect::<Vec<_>>(), [Decimal::from(90), Decimal::from(100)]);
        // Fewer than three points: no change drawn.
        assert_eq!(st.portfolio.change7d, None);
    });
}

#[test]
fn money_moved_in_or_out_is_kept_per_wallet() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        // No wallet: nothing kept.
        e.record_flow(Decimal::from(5), "USDC in on Solana");
        assert!(store::load::<Vec<Flow>>("flows.json").is_none());
        e.st_mut().status = Some(SignerReply { ok: true, has_wallet: Some(true), solana_address: Some("So1".into()), ..Default::default() });
        e.record_flow(Decimal::ZERO, "nothing");
        e.record_flow(Decimal::from(-5), "sent to Mom");
        let flows: Vec<Flow> = store::load("flows.json").unwrap();
        assert_eq!(flows.iter().map(|f| (f.usd, f.note.as_str(), f.wallet.as_str())).collect::<Vec<_>>(), [(Decimal::from(-5), "sent to Mom", "So1")]);
        assert_eq!((e.signed_usd(Decimal::new(-125, 2)), e.signed_usd(Decimal::from(3))), ("−$1.25".to_string(), "+$3".to_string()));
    });
}

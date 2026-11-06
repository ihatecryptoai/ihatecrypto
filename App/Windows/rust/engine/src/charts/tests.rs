//! Model7Tests' one chart a canvas, and the movers' board from the markets already read, offline.

use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;

use ihatecrypto_core::charts::Range;

use super::*;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::writer(std::io::sink()), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("locked: no state is drawn"));
    e.st_mut().lock.locked = true;
    e
}

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(tokio::task::LocalSet::new().run_until(test));
}

fn line(coin: &str) -> ChartLine {
    ChartLine { title: format!("{coin} · Day"), closes: vec![1.0, 2.0], times: None, low: "1".into(), high: "2".into(), change: "+100%".into(),
                coin: Some(coin.into()), network: None, extras: None }
}

#[test]
fn a_canvas_has_one_chart() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        let first = e.place_chart(line("BTC"));
        e.set_boards(|b| b.insert(0, Board::new("result", "Rates")));
        let second = e.place_chart(line("ETH"));
        assert_eq!(first, second);
        let st = e.st();
        assert!(st.boards().iter().filter(|b| b.kind == "chart").count() == 1 && st.boards()[0].chart.as_ref().and_then(|c| c.coin.as_deref()) == Some("ETH"));
        assert_eq!(st.boards()[0].title, "ETH · Day");
    });
}

#[test]
fn the_movers_are_the_days_biggest_from_the_markets_read() {
    let _dir = store::test_folder();
    run(async {
        let e = engine();
        e.set_settings(|s| s.language = Some("en".into()));
        let market = |name: &str, now: i64, volume: i64| Market { prev_day_px: Some(Decimal::from(100)), volume24h: Decimal::from(volume), ..Market::new(name, Decimal::from(now), 2, 10) };
        e.st_mut().markets = [market("BTC", 110, 5_000_000), market("ETH", 95, 5_000_000), market("DOGE", 150, 10)]
            .into_iter().map(|m| (m.name.clone(), m)).collect();
        e.chart(ChartCommand::Movers, None, false).await;
        let said = e.st().messages().last().map(|m| (m.text.clone(), m.note.clone())).unwrap();
        // Too little traded: left out.
        assert_eq!(said, ("Up the most today: BTC 110 +10%, ETH 95 -5%.\nDown the most: ETH 95 -5%, BTC 110 +10%.".into(),
                          Some("Hyperliquid".into())));
        let board = e.st().boards()[0].clone();
        let movers = board.data.unwrap().movers.unwrap();
        assert_eq!(board.title, "Movers · 24 h");
        assert_eq!(movers.up.iter().map(|m| (m.name.as_str(), m.change)).collect::<Vec<_>>(), [("BTC", Decimal::from(10)), ("ETH", Decimal::from(-5))]);
        assert_eq!(board.lines.len(), 5);
        // Quiet: the board only.
        let before = e.st().messages().len();
        e.chart(ChartCommand::Movers, None, true).await;
        assert_eq!(e.st().messages().len(), before);
        assert_eq!((e.signed(Decimal::new(25, 1)), e.signed(Decimal::new(-3, 0))), ("+2.5%".to_string(), "-3%".to_string()));
        let _ = Range::Day;
    });
}

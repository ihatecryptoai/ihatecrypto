//! Earn, offline: Jupiter's DCA orders as their lines say them, every pinned DCA block shown as the orders are now.

use std::rc::Rc;
use std::sync::Arc;

use protocols::recurring::Order;
use rust_decimal::Decimal;

use super::lines;
use crate::canvas::{Board, CanvasDoc};
use crate::engine::Engine;
use crate::outbox::Outbox;
use crate::signer_link::SignerLink;
use crate::store;

fn engine() -> Rc<Engine> {
    let e = Engine::new(Outbox::silent(), Arc::new(SignerLink::new(store::url("no-signer-here"))), |_| unreachable!("drawn while locked"));
    e.st_mut().lock.locked = true;
    e
}

fn order(input: &str, output: &str) -> Order {
    Order {
        key: "k".into(), input_mint: input.into(), output_mint: output.into(), deposited: Decimal::from(100), used: Decimal::new(255, 1), per_cycle: Decimal::from(5),
        every: 86_400, received: Decimal::new(1_234_567, 7), created_at: None, fills: 5,
    }
}

const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

#[tokio::test]
async fn dca_lines_name_the_coins_and_the_pace() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let orders = [order(USDC, "So11111111111111111111111111111111111111112"), order(USDC, "Mint9999999999999999999999999999999999999999")];
        assert_eq!(e.dca_lines(&orders), ["25.5 / 100 USDC → 0.123457 SOL · 5 swaps, every 24 h", "25.5 / 100 USDC → 0.123457 Mint99… · 5 swaps, every 24 h"]);
        e.set_settings(|s| s.language = Some("ru".into()));
        assert_eq!(e.dca_lines(&orders[..1]), ["25.5 / 100 USDC → 0.123457 SOL · 5 обменов, раз в 24 ч"]);
        assert_eq!(lines("a\n\nb\n"), ["a", "b"]);
    }).await;
}

#[tokio::test]
async fn every_pinned_dca_block_shows_the_orders_as_they_are() {
    let _dir = store::test_folder();
    tokio::task::LocalSet::new().run_until(async {
        let e = engine();
        let dca = || Board { lines: vec!["old".into()], ..Board::new("result", "Jupiter DCA") };
        {
            let mut st = e.st_mut();
            let other = CanvasDoc { boards: vec![dca(), Board::new("result", "Rates")], ..CanvasDoc::new("Other") };
            st.canvases.push(other);
        }
        // Pinned only on the other canvas: not here.
        assert!(!e.read_dca(&[]));
        {
            let st = e.st();
            let other = &st.canvases[1].boards;
            assert_eq!(other[0].lines, ["No Jupiter DCA is running."]);
            assert!(other[0].data.is_none() && other[1].lines.is_empty() && st.jupiter_dca.is_none());
        }
        e.st_mut().canvases[0].boards.push(dca());
        assert!(e.read_dca(&[]));
        let saved: Vec<CanvasDoc> = store::load("canvases.json").unwrap();
        assert_eq!(saved[0].boards[0].lines, ["No Jupiter DCA is running."]);
    }).await;
}

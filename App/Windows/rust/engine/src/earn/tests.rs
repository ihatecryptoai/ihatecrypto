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

fn limit_view(sell: bool, coins: &str, price: &str) -> trade::limit_order::LimitOrderView {
    trade::limit_order::LimitOrderView { id: "o1".into(), sell, coin: "SOL".into(), coins: coins.parse().unwrap(), price: price.parse().unwrap(),
                                         filled: Decimal::ZERO, status: "OPEN".into(), until: None, market: None }
}

// The fill said as what came and its price with the fees, beside the order's own price, in English and Russian (the
// Mac's LimitEndWordsTests).
#[test]
fn limit_end_words_say_what_came() {
    use trade::limit_order::LimitOrderEnd;
    let o = limit_view(false, "0.00833811", "83.7");
    let (en, ru) = super::limit_end_words(&o, &LimitOrderEnd { status: "FILLED".into(), spent: "0.6979".parse().unwrap(), got: "0.008249201".parse().unwrap(),
                                                                  price: Some("84.6023".parse().unwrap()) });
    assert_eq!(en, "Limit order filled: bought ≈ 0.008249 SOL for 0.6979 USDC, $84.60 a SOL with 1Click's fees and the network's (the order's price $83.70). The SOL comes to your own address.");
    assert!(ru.starts_with("Лимитка исполнилась: куплено ≈ 0.008249 SOL за 0.6979 USDC, $84.60 за SOL"), "{ru}");
    let (en, _) = super::limit_end_words(&o, &LimitOrderEnd { status: "EXPIRED".into(), spent: Decimal::ZERO, got: Decimal::ZERO, price: None });
    assert_eq!(en, "Limit order expired: buy 0.00833811 SOL at $83.70. What didn't fill comes back to the address it came from.");
}

// «my limit orders» names each order, not only how many.
#[test]
fn limit_list_words_name_each_order() {
    let sell = limit_view(true, "0.012", "84.3");
    assert_eq!(super::limit_list_words(&[sell.clone()]).unwrap().1, "Один открытый лимитный ордер: продажа 0.012 SOL по $84.30. Он на холсте.");
    assert_eq!(super::limit_list_words(&[sell.clone(), sell]).unwrap().0, "2 open limit orders: sell 0.012 SOL at $84.30 · sell 0.012 SOL at $84.30. They're on the canvas.");
    assert!(super::limit_list_words(&[]).is_none());
}

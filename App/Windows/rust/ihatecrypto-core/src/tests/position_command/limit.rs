//! LimitOrderTests.swift: limit orders through NEAR Intents' 1Click, their list and their cancel, in English and Russian.

use super::p;
use crate::position_command::{SpotAsset, WalletCommand as W};
use crate::tests::d;

fn limit(sell: bool, coin: &str, on: &str, price: &str, amount: &str, usdc_on: &str, days: Option<i64>) -> W {
    W::LimitOrder { sell, coin: coin.into(), on: on.into(), price: d(price), amount: d(amount), usdc_on: usdc_on.into(), days }
}

#[test]
fn limit_orders_read_in_english() {
    assert_eq!(p("limit sell 0.1 SOL at 200"), Some(limit(true, "SOL", "solana", "200", "0.1", "solana", None)));
    assert_eq!(p("limit buy SOL at 120 for 20 USDC"), Some(limit(false, "SOL", "solana", "120", "20", "solana", None)));
    assert_eq!(p("limit buy SOL at $120.5 for $20 for 3 days"), Some(limit(false, "SOL", "solana", "120.5", "20", "solana", Some(3))));
    assert_eq!(p("limit sell 1 NEAR at 3.5 for USDC on base"), Some(limit(true, "NEAR", "near", "3.5", "1", "base", None)));
    assert_eq!(p("limit buy NEAR at 2 for 5 USDC"), Some(limit(false, "NEAR", "near", "2", "5", "solana", None)));
    assert_eq!(p("limit buy ETH on arbitrum at 3000 for 10 USDC on arbitrum for a week"), Some(limit(false, "ETH", "arbitrum", "3000", "10", "arbitrum", Some(7))));
    assert_eq!(p("limit sell 0.1 SOL privately at 200"), Some(limit(true, "SOL", "solana", "200", "0.1", "solana", None)));
    assert_eq!(p("show limit orders"), Some(W::LimitOrders));
    assert_eq!(p("my limit orders"), Some(W::LimitOrders));
    assert_eq!(p("cancel limit orders"), Some(W::LimitCancel(None)));
    assert_eq!(p("cancel limit orders SOL"), Some(W::LimitCancel(Some("SOL".into()))));
}

#[test]
fn limit_orders_read_in_russian() {
    assert_eq!(p("лимитка: продай 0,1 SOL по 200"), Some(limit(true, "SOL", "solana", "200", "0.1", "solana", None)));
    assert_eq!(p("лимитный ордер: купи SOL по 120 на 20 USDC на 3 дня"), Some(limit(false, "SOL", "solana", "120", "20", "solana", Some(3))));
    assert_eq!(p("лимиткой купи сол по цене 120 на 20 долларов"), Some(limit(false, "SOL", "solana", "120", "20", "solana", None)));
    assert_eq!(p("лимитка: продай 1 NEAR по 3,5 за USDC в бейсе"), Some(limit(true, "NEAR", "near", "3.5", "1", "base", None)));
    assert_eq!(p("мои лимитки"), Some(W::LimitOrders));
    assert_eq!(p("покажи лимитные ордера"), Some(W::LimitOrders));
    assert_eq!(p("отмени лимитки"), Some(W::LimitCancel(None)));
    assert_eq!(p("отмени лимитные ордера по SOL"), Some(W::LimitCancel(Some("SOL".into()))));
}

#[test]
fn a_limit_order_without_its_price_or_amount_is_nothing() {
    // The chat asks rather than resting an order at a guessed price or size.
    assert_eq!(p("limit sell SOL at 200"), None);
    assert_eq!(p("limit buy SOL for 20 USDC"), None);
    assert_eq!(p("limit buy SOL at 120"), None);
    assert_eq!(p("limit buy USDC at 1 for 5 USDC"), None);
    assert_eq!(p("limit sell 0.1 SOL at 200 for 90 days"), None);
    // One «limit order» is Hyperliquid's entry waiting to fill: a position command, never 1Click's orders.
    assert_eq!(p("cancel the limit order"), None);
    assert_eq!(p("отмени лимитку"), None);
    assert_eq!(p("cancel limit orders id order_EfzzwVuXY"), Some(W::LimitCancel(Some("id:order_EfzzwVuXY".into()))));
    // A plan's entry price and an agent wallet's daily limit are not limit orders.
    assert_eq!(p("long ETH stop 2% limit 2650"), None);
    assert_eq!(p("long SOL 5 USDC stop 1% limit order at 120"), None);
    assert_eq!(p("создай кошелёк агента с лимитом 20$ в день"), Some(W::AgentCreate { daily_usd: d("20") }));
    // Without «limit» a buy at a price stays the buy it always was.
    assert_eq!(p("buy SOL for 20 USDC"), Some(W::Buy { asset: SpotAsset::by_symbol("SOL"), usdc: Some(d("20")) }));
}

#[test]
fn limit_orders_write_back_to_themselves() {
    for c in [limit(true, "SOL", "solana", "200", "0.1", "base", Some(3)), limit(false, "NEAR", "near", "2.25", "5", "solana", None),
              limit(false, "ETH", "arbitrum", "3000", "10", "arbitrum", Some(7)), W::LimitOrders, W::LimitCancel(None),
              W::LimitCancel(Some("SOL".into())), W::LimitCancel(Some("id:order_Ab1-x".into()))] {
        assert_eq!(p(&c.summary()), Some(c.clone()), "{}", c.summary());
    }
    // The amount a card changes, and a private ask of it stays itself.
    assert_eq!(limit(true, "SOL", "solana", "200", "0.1", "solana", None).with_amount(d("0.2")), limit(true, "SOL", "solana", "200", "0.2", "solana", None));
    assert_eq!(W::LimitOrders.made_private(), Some(W::LimitOrders));
}

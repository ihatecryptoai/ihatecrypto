use rust_decimal::Decimal;

use super::{d, strings};
use crate::funding_plan::{Place, legs};

fn base() -> Place { Place::new("base", "Base", d("100")) }

// pending: each leg read back by WalletCommand.parse (PositionCommand round), in all four tests.
#[test]
fn funding_takes_solana_then_evm_then_hyperliquid_through_solana() {
    let got = legs(d("300"), "robinhood", "Robinhood", d("9"), &[base()], d("250"), Decimal::ZERO, None).unwrap();
    assert_eq!(got, strings(&["move 100 USDC from Base to Robinhood", "withdraw 194 USDC to Solana", "move 201 USDC from Solana to Robinhood"]));
}

#[test]
fn funding_for_solana_lands_each_source_there_and_sells_near_last() {
    let got = legs(d("50"), "solana", "Solana", Decimal::ZERO, &[Place::new("base", "Base", d("20"))], d("40"), Decimal::ZERO, None).unwrap();
    assert_eq!(got, strings(&["move 20 USDC from Base to Solana", "withdraw 32 USDC to Solana"]));
    let near = legs(d("10"), "solana", "Solana", Decimal::ZERO, &[], Decimal::ZERO, d("5"), Some(d("3"))).unwrap();
    assert_eq!(near, strings(&["sell 3.74 NEAR"]));
    // Not enough anywhere: no plan, the move's own check says so; under $2 a place isn't worth a leg.
    assert_eq!(legs(d("500"), "solana", "Solana", Decimal::ZERO, &[base()], d("40"), Decimal::ZERO, None), None);
    assert_eq!(legs(d("1"), "solana", "Solana", Decimal::ZERO, &[Place::new("base", "Base", d("1.5"))], Decimal::ZERO, Decimal::ZERO, None), None);
}

/// Cents count: a leg rounded down left this plan a cent short (1721.109778 USDC more on Robinhood).
#[test]
fn funding_covers_the_last_cent() {
    let got = legs(d("1721.109778"), "robinhood", "Robinhood", d("9.008635"), &[Place::new("base", "Base", d("1712.67116"))], d("4.007817"),
                   Decimal::ZERO, None).unwrap();
    assert_eq!(got, strings(&["move 1712.67 USDC from Base to Robinhood", "withdraw 2.44 USDC to Solana", "move 9.44 USDC from Solana to Robinhood"]));
}

/// Solana's leg names what arrives (fee on top): its whole balance asked to arrive could never be paid.
#[test]
fn solana_leg_leaves_room_for_the_fee() {
    let got = legs(d("50"), "robinhood", "Robinhood", d("34.823554"), &[Place::new("base", "Base", d("21.5"))], Decimal::ZERO, Decimal::ZERO, None).unwrap();
    assert_eq!(got, strings(&["move 17.18 USDC from Base to Robinhood", "move 33.82 USDC from Solana to Robinhood"]));
    // A cent on Solana isn't a leg.
    assert_eq!(legs(d("5"), "robinhood", "Robinhood", d("1.2"), &[base()], Decimal::ZERO, Decimal::ZERO, None), Some(strings(&["move 6 USDC from Base to Robinhood"])));
}

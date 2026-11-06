//! Approvals.swift's offline case (UnknownDecimalsTests): the approvals board says an amount it can't scale is unknown,
//! never 18's guess; and who a spender is.

use rust_decimal::Decimal;

use crate::Executor;
use crate::approvals::TokenApproval;

fn row(decimals: Option<u32>, units: Decimal) -> String {
    TokenApproval {
        chain: "base".into(), token: "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913".into(), symbol: "USDC".into(), decimals,
        spender: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8".into(), spender_name: None, amount: units, permit2: false,
    }.shown()
}

#[test]
fn an_approval_without_decimals_says_the_amount_is_unknown() {
    assert_eq!(row(None, Decimal::from(5_000_000_000_000u64)), "amount unknown: decimals() didn't answer");
    assert_eq!(row(Some(6), Decimal::from(5_000_000_000_000u64)), ihatecrypto_core::intent::Fmt::num(Decimal::from(5_000_000), 6));
    // Swift's 10³⁰ is past Decimal here: an allowance read past range is Decimal::MAX, still «unlimited».
    assert_eq!(row(None, Decimal::MAX), "unlimited");
}

#[test]
fn spenders_by_name() {
    assert_eq!(Executor::spender_name("0x000000000022D473030F116dDEE9F6B43aC78BA3", 8453), Some("Permit2 (Uniswap)"));
    assert_eq!(Executor::spender_name("0x2626664c2603336e57b271c5c0b26f421741e481", 8453), Some("Uniswap v3 router"));
    assert_eq!(Executor::spender_name("0xccc88a9d1b4ed6b0eaba998850414b24f1c315be", 1), Some("Relay"));
    assert_eq!(Executor::spender_name("0x70997970c51812dc3a010c7d01b50e0d17dc79c8", 8453), None);
}

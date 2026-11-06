//! KaminoBorrow.swift's offline cases (KaminoBorrowTests): the loan's math before anything is signed, and a withdrawal
//! right after a borrow waiting out Kamino's lag.

use chrono::{Duration, Utc};
use defi::kamino::{Collateral, Debt, Loan, MAIN_MARKET, BACKPACK_MARKET, Risk};
use execution::{Entry, Kind};
use rust_decimal::Decimal;

use super::d;
use super::g2_fake::{TempJournal, executor};
use crate::Executor;

fn loan(symbol: &str, mint: &str, tokens: &str, value: &str, owed: &str, limit: &str, liquidation: &str) -> Loan {
    Loan {
        collateral: vec![Collateral { reserve: "r".into(), symbol: symbol.into(), mint: mint.into(), tokens: d(tokens), value_usd: d(value) }],
        debts: vec![Debt { reserve: "u".into(), symbol: "USDC".into(), tokens: d(owed) }],
        deposit_usd: d(value), debt_usd: d(owed), borrow_limit_usd: d(limit), liquidation_limit_usd: d(liquidation), adjusted_debt_usd: d(owed),
    }
}

/// A loan on Kamino's Backpack market: 0.05 SPCX at $150 ($7.50) put up, borrowed against at most 30% of it, liquidated at 50%.
#[test]
fn kamino_loan_math_keeps_the_health_and_names_the_liquidation_price() {
    let spcx = Risk { max_ltv: d("0.3"), liquidation_ltv: d("0.5"), borrow_factor: d("1.5") };
    // A first loan: nothing there yet.
    let first = Executor::kamino_after(None, d("0.05"), Decimal::ZERO, d("150"), Some(&spcx), d("1"), d("1")).unwrap();
    assert_eq!(first.0, d("7.5"));
    assert_eq!(first.1, d("2.25"));
    assert_eq!(first.3, Some(d("3.75")));
    // Liquidation when 0.05 SPCX × p × 0.5 = $1: p = $40.
    assert_eq!(first.4, Some(d("40")));
    // More on the same position: its own limits plus the new stock's.
    let held = loan("SPCX", "m", "0.05", "7.5", "1", "2.25", "3.75");
    assert_eq!(held.health(), Some(d("3.75")));
    let more = Executor::kamino_after(Some(&held), d("0.05"), d("0.05"), d("150"), Some(&spcx), d("2"), d("1")).unwrap();
    assert_eq!(more.1, d("3.5"));
    assert_eq!(more.2, d("3"));
    assert_eq!(more.3, Some(d("2.5")));
    assert_eq!(more.4, Some(d("60")));
    // A borrow ihatecrypto would refuse: past 1.5.
    let deep = Executor::kamino_after(None, d("0.05"), Decimal::ZERO, d("150"), Some(&spcx), d("2.6"), d("1")).unwrap();
    assert!(deep.3.unwrap() < Executor::MINIMUM_HEALTH);
    // What may leave with $1 owed: what's left must carry it at 30%, 1% to spare.
    assert_eq!(Executor::kamino_free_collateral(&held, d("0.3"), d("0.5"), d("150")), d("1.25") / d("0.3") / d("150") * d("0.99"));
}

/// A loan against SOL on Kamino's main market (borrow up to 74%, liquidation at 80%): 0.5 SOL at $96 put up, 20 USDC borrowed.
#[test]
fn kamino_loan_against_sol_names_its_liquidation_price() {
    let sol = Risk { max_ltv: d("0.74"), liquidation_ltv: d("0.8"), borrow_factor: d("1.25") };
    let first = Executor::kamino_after(None, d("0.5"), Decimal::ZERO, d("96"), Some(&sol), d("20"), d("1")).unwrap();
    assert_eq!(first.0, d("48"));
    assert_eq!(first.1, d("35.52"));
    assert_eq!(first.3, Some(d("1.92")));
    // 0.5 SOL × p × 0.8 = $20: p = $50.
    assert_eq!(first.4, Some(d("50")));
    // What may leave with 20 USDC owed: Kamino's 74% limit would free $20.97 and leave a health factor of 1.08; the 1.5
    // line frees $10.50 (38.4 − 8.4 = 30 = 1.5 × 20), 1% to spare.
    let held = loan("SOL", "So11111111111111111111111111111111111111112", "0.5", "48", "20", "35.52", "38.4");
    assert_eq!(Executor::kamino_free_collateral(&held, d("0.74"), d("0.8"), d("96")), d("10.5") / d("96") * d("0.99"));
    // Kamino's numbers past range free nothing instead of taking the engine down.
    assert_eq!(Executor::kamino_free_collateral(&held, d("0.74"), d("0.8"), d("0.0000000000000000000000000001")), Decimal::ZERO);
    assert!(Executor::kamino_after(None, Decimal::MAX, Decimal::ZERO, Decimal::MAX, Some(&sol), d("1"), d("1")).is_err());
}

/// A withdrawal right after a borrow on the same market waits a minute: Kamino's API, which the 1.5 line reads, lags the borrow.
#[test]
fn a_withdrawal_waits_out_a_borrow() {
    let journal = TempJournal::new("kamino");
    let e = executor("", None, &journal);
    assert!(!e.kamino_borrowed_just_now(MAIN_MARKET));
    let mut old = Entry::new("solana", "a", Kind::Signed, "Kamino borrow 1 USDC on the main market; valid until block 1");
    old.at = Utc::now() - Duration::seconds(90);
    journal.journal().append(&old).unwrap();
    assert!(!e.kamino_borrowed_just_now(MAIN_MARKET));
    journal.journal().append(&Entry::new("solana", "b", Kind::Signed, "Kamino borrow 1 USDC on the main market; valid until block 2")).unwrap();
    assert!(e.kamino_borrowed_just_now(MAIN_MARKET));
    assert!(!e.kamino_borrowed_just_now(BACKPACK_MARKET));
}

#[test]
fn kamino_markets_by_name() {
    assert_eq!(Executor::kamino_market_name(BACKPACK_MARKET), "Kamino's Backpack market");
    assert_eq!(Executor::kamino_market_word(MAIN_MARKET), "main");
    assert_eq!(Executor::usdc_owed(Some(&loan("SOL", "m", "0.5", "48", "20", "35.52", "38.4"))), d("20"));
    assert_eq!(Executor::usdc_owed(None), Decimal::ZERO);
}

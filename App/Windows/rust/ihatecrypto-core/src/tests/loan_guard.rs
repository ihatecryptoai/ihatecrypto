use std::collections::HashSet;

use rust_decimal::Decimal;

use super::d;
use crate::loan_guard::{LoanGuard, LoanGuardCommand};
use crate::normalize::ChatRouter;
use crate::position_command::WalletCommand;

fn set(loan: Option<&str>, network: Option<&str>, below: Option<&str>) -> Option<LoanGuardCommand> {
    Some(LoanGuardCommand::Set { loan: loan.map(Into::into), network: network.map(Into::into), below: below.map(d) })
}

#[test]
fn loan_guard_forms_parse_in_english_and_russian() {
    let p = LoanGuardCommand::parse;
    assert_eq!(p("guard my loan on kamino"), set(Some("kamino"), None, None));
    assert_eq!(p("guard my loan on aave on arbitrum below 1.3"), set(Some("aave"), Some("arbitrum"), Some("1.3")));
    assert_eq!(p("guard my loan on Aave"), set(Some("aave"), None, None));
    assert_eq!(p("следи за займом на камино"), set(Some("kamino"), None, None));
    assert_eq!(p("не дай ликвидировать мой займ на aave в арбитрум, если health ниже 1,4"), set(Some("aave"), Some("arbitrum"), Some("1.4")));
    assert_eq!(p("защити займ"), set(None, None, None));
    assert_eq!(p("stop guarding my loans"), Some(LoanGuardCommand::Clear));
    assert_eq!(p("убери защиту займа"), Some(LoanGuardCommand::Clear));
    // A level that isn't a health factor is left to the default; neighbours of the form aren't read as it.
    assert_eq!(p("guard my loan on kamino below 50"), set(Some("kamino"), None, None));
    assert_eq!(p("repay all USDC on kamino"), None);
    assert_eq!(p("borrow 2 USDC on kamino against 0.05 SPCX"), None);
    assert_eq!(p("every hour: check: Aave health factor on Base; warn under 1.6"), None);
    // Asking to stop, asking about it, or a trade that mentions liquidation never starts one.
    assert_eq!(p("перестань следить за займом"), Some(LoanGuardCommand::Clear));
    assert_eq!(p("don't guard my loan anymore"), Some(LoanGuardCommand::Clear));
    assert_eq!(p("how does the loan guard work?"), None);
    assert_eq!(p("long ETH 5x, don't let it get liquidated"), None);
    assert_eq!(p("when SOL is below 150: guard my loan"), None);
    assert_eq!(p("следи за ценой SOL"), None);
    assert_eq!(p("защити кошелёк"), None);
    // «до» is the target they want, not the line.
    assert_eq!(p("следи за займом, гаси до 1.6, если ниже 1.3"), set(None, None, Some("1.3")));
}

#[test]
fn loan_guard_keeps_a_gap_and_re_arms_halfway() {
    let g = LoanGuard::new("aave", None, d("1.6"), LoanGuard::default_target());
    assert!(g.target == d("1.8") && g.rearm() == d("1.7") && g.network.as_deref() == Some("base"));
    let usual = LoanGuard::new("kamino", None, LoanGuard::default_below(), LoanGuard::default_target());
    assert!(usual.target == d("1.5") && usual.rearm() == d("1.375"));
}

#[test]
fn loan_guard_repays_back_to_the_target() {
    // Aave: $100 of collateral at 0.8, $64 owed → health 1.25; back at 1.5 the debt is 100 × 0.8 / 1.5 = 53.33.
    let health = d("100") * d("0.8") / d("64");
    let r = LoanGuard::repay(d("64"), health, d("1.5"));
    assert_eq!(r, d("10.67"));
    assert!(d("100") * d("0.8") / (d("64") - r) >= d("1.5"));
    assert_eq!(LoanGuard::repay(d("10"), d("2"), d("1.5")), Decimal::ZERO);
    assert!(LoanGuard::repay(d("10"), d("0.2"), d("1.5")) <= d("10"));
    let (below, target) = (LoanGuard::default_below(), LoanGuard::default_target());
    assert_eq!(LoanGuard::new("aave", None, below, target).repay_command(d("10.67")), "repay 10.67 USDC on Aave on base");
    assert_eq!(LoanGuard::new("kamino", Some("base"), below, target).network, None);
    assert_eq!(LoanGuard::new("kamino", None, below, target).repay_command(d("3.5")), "repay 3.5 USDC on kamino");
    // The repay it prepares is the move the router already knows.
    assert_eq!(
        WalletCommand::parse(&LoanGuard::new("aave", Some("arbitrum"), below, target).repay_command(d("10.67"))),
        Some(WalletCommand::AaveRepay { amount: Some(d("10.67")), network: "arbitrum".into() })
    );
    assert!(crate::loan_guard::reads_as_repay(&LoanGuard::new("kamino", None, below, target).repay_command(d("3.5"))));
}

/// The router lets the guard's forms through: «stop guarding my loans» was refused as no command at all.
#[test]
fn the_router_accepts_the_guard_forms() {
    assert!(ChatRouter::parses("stop guarding my loans", &HashSet::new()));
    assert!(ChatRouter::parses("guard my loan on kamino below 3.5", &HashSet::new()));
    // The line the user names is kept, however high: 3.5 came out as the default 1.25.
    assert_eq!(LoanGuardCommand::parse("guard my loan on kamino below 3.5"), set(Some("kamino"), None, Some("3.5")));
    assert_eq!(LoanGuardCommand::parse("следи за займом на камино, ниже 3,5 погашай"), set(Some("kamino"), None, Some("3.5")));
}

//! The health-factor arithmetic the card shows (UniswapRoutesTests' aaveBorrowIsReadAndPriced): Aave's collateral ×
//! weighted threshold ÷ debt; liquidation where that reaches 1.

use super::d;
use crate::aave_borrow::AaveAccount;

#[test]
fn aave_borrow_is_priced() {
    // $3000 of ETH (1 ETH), threshold 0.83, $1000 borrowed: health 2.49; liquidation when ETH is $1204.82.
    assert_eq!(AaveAccount::health_of(d("3000"), d("1000"), d("0.83")), Some(d("2.49")));
    let liq = AaveAccount::liquidation_price(d("1000"), d("0.83"), d("3000"), d("1"), d("3000")).unwrap();
    assert!(liq > d("1204.81") && liq < d("1204.83"));
    // $2000 of USDC beside the ETH covers the debt on its own: no ETH price liquidates it.
    assert_eq!(AaveAccount::liquidation_price(d("1000"), d("0.83"), d("5000"), d("1"), d("3000")), None);
    assert_eq!(AaveAccount { collateral: d("3000"), debt: d("0"), available: d("0"), threshold: d("0.83") }.health(), None);
}

use super::d;
use crate::intent::TradeIntent;
use crate::take_style::TakeStyle;

#[test]
fn take_profit_choices() {
    assert!(TakeStyle::asks_for_choice("поставь тейк, как советуют лучшие инвесторы"));
    assert!(TakeStyle::asks_for_choice("set the take profit like the best investors do"));
    assert!(TakeStyle::asks_for_choice("take profit options"));
    assert!(!TakeStyle::asks_for_choice("long ETH take 6%"));
    let plan = TradeIntent { take_pct: Some(d("6")), ..TradeIntent::default() };
    let partial = TakeStyle::Partial.apply(&plan);
    assert!(partial.take_pct.is_none() && partial.take_r == Some(d("2")) && partial.partial_take == Some(true) && partial.trailing_pct.is_none());
    let trend = TakeStyle::Trend.apply(&plan);
    assert!(trend.trailing_pct == Some(d("2")) && trend.partial_take == Some(true));
    assert_eq!(TakeStyle::Classic.payoff(d("0.25")), "+$0.50");
    assert_eq!(TakeStyle::Partial.payoff(d("0.25")), "+$0.38");
}

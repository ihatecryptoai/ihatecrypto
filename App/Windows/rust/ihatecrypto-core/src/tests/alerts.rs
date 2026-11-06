use super::{d, set};
use crate::alerts::{AlertCommand, PriceAlert};

fn alert(coin: &str, price: &str, above: Option<bool>, steps: &[&str]) -> Option<AlertCommand> {
    Some(AlertCommand::Set { coin: coin.into(), price: d(price), above, steps: steps.iter().map(|s| s.to_string()).collect() })
}

// pending: ChatRouter.parses (Normalize round).
#[test]
fn price_alerts() {
    let m = set(&["ETH", "BTC", "SOL", "xyz:CL"]);
    assert_eq!(AlertCommand::parse("alert me when ETH is above 2,800", &m), alert("ETH", "2800", Some(true), &[]));
    assert_eq!(AlertCommand::parse("скажи когда нефть упадёт ниже 90,5", &m), alert("xyz:CL", "90.5", Some(false), &[]));
    assert_eq!(AlertCommand::parse("tell me when BTC hits 70000", &m), alert("BTC", "70000", None, &[]));
    assert_eq!(AlertCommand::parse("cancel all alerts", &m), Some(AlertCommand::Clear));
    assert_eq!(AlertCommand::parse("long ETH risk $5 stop 2%", &m), None);
    assert_eq!(AlertCommand::parse("notify me about ETH", &m), None);
    let a = PriceAlert::new("ETH", false, d("2600"), None);
    assert!(a.fires(d("2599")) && a.fires(d("2600")) && !a.fires(d("2601")));
    // With steps after the colon: the level before it, and the numbers of the steps don't become the level.
    assert_eq!(AlertCommand::parse("when ETH is below 2,680: long ETH risk $0.28 stop 2.1% take 4.2% leverage 3x", &m),
               alert("ETH", "2680", Some(false), &["long ETH risk $0.28 stop 2.1% take 4.2% leverage 3x"]));
    assert_eq!(AlertCommand::parse("when SOL is above 250: sell SOL then lend USDC", &m), alert("SOL", "250", Some(true), &["sell SOL", "lend USDC"]));
    assert_eq!(AlertCommand::parse("when my portfolio grows: long ETH", &m), None);
    // Alerts kept before steps existed still load.
    let old: PriceAlert = serde_json::from_str(r#"{"id":"6B1E6C8A-3F0B-4C55-9C8E-1D2B3A4F5E6D","coin":"ETH","above":true,"price":1,"created":0}"#).unwrap();
    assert!(old.steps.is_none() && old.price == d("1"));
}

#[test]
fn an_alert_without_a_side_and_the_english_form() {
    let m = set(&["ETH", "BTC", "SOL"]);
    assert_eq!(AlertCommand::parse("alert me when ETH is at 2600", &m), alert("ETH", "2600", None, &[]));
    assert!(AlertCommand::parse("alert me when ETH is above 4000", &m).is_some());
}

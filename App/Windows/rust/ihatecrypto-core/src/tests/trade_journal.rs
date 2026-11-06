use super::d;
use crate::foundation::{adding, unix};
use crate::trade_journal::*;

#[test]
fn a_journal_entry_reviews_what_the_price_did_since() {
    let t0 = unix(1_800_000_000.0);
    let mut e = JournalEntry::new(t0, "HYPE", "long here");
    e.price = Some(d("20"));
    e.funding = Some(d("0.0000125"));
    e.market = vec!["BTC $60,000 (24h +1%)".into()];
    let mut big = e.clone();
    big.open_interest_usd = Some(d("1638197533"));
    big.volume24h = Some(d("585323002"));
    assert!(big.snapshot().contains("open interest $1.64B, 24h volume $585.3M"));
    assert!(e.review(Some(d("25")), adding(t0, 30.0 * 3600.0))
        .ends_with("[price 20, funding 0.0013% an hour; BTC $60,000 (24h +1%)] → since: 20 → 25 (+25%) in 30 h"));
    e.change24h = Some(d("-1.234"));
    e.position = Some("long 2 HYPE".into());
    assert_eq!(e.snapshot(), "price 20, 24h −1.23%, funding 0.0013% an hour; BTC $60,000 (24h +1%); their position: long 2 HYPE");
    assert!(e.review(None, t0).ends_with("HYPE: «long here» [price 20, 24h −1.23%, funding 0.0013% an hour; BTC $60,000 (24h +1%); their position: long 2 HYPE]"));
}

/// The journal file as Swift's `Codable` writes it reads back, and writes the same keys.
#[test]
fn an_entry_reads_as_swift_wrote_it() {
    let json = r#"{"id":"E621E1F8-C36C-495A-93FC-0C247A3E6E5F","at":821692800,"asset":"xyz:GOLD","words":"short, dollar ripping",
                   "price":2650.5,"openInterestUSD":1638197533,"market":[]}"#;
    let e: JournalEntry = serde_json::from_str(json).unwrap();
    assert!(e.asset == "xyz:GOLD" && e.at == unix(1_800_000_000.0) && e.price == Some(d("2650.5")) && e.open_interest_usd == Some(d("1638197533")));
    let written = serde_json::to_value(&e).unwrap();
    let keys: Vec<&String> = written.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["asset", "at", "id", "market", "openInterestUSD", "price", "words"]);
    assert_eq!(serde_json::from_value::<JournalEntry>(written).unwrap(), e);
    // Swift requires the id and the market list.
    assert!(serde_json::from_str::<JournalEntry>(&json.replace(r#","market":[]"#, "")).is_err());
}

/// metaAndAssetCtxs: the coins open × the mark price.
#[test]
fn open_interest_reads_from_the_markets_answer() {
    let json = r#"[{"universe": [{"name": "BTC", "szDecimals": 5}, {"name": "HYPE", "szDecimals": 2}]},
                   [{"openInterest": "30000.5", "markPx": "60000"}, {"openInterest": "18664207.1", "markPx": "87.77", "funding": "0.0000125"}]]"#;
    assert_eq!(JournalEntry::parse_open_interest_usd(json.as_bytes(), "HYPE"), Some(d("1638157457.167")));
    assert_eq!(JournalEntry::parse_open_interest_usd(json.as_bytes(), "ETH"), None);
    assert_eq!(JournalEntry::parse_open_interest_usd(b"[]", "HYPE"), None);
}

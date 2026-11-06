use chrono::Utc;
use serde_json::Value;

use super::d;
use crate::charts::Range;
use crate::foundation::{compact_usd, format_g, iso8601, unix};
use crate::token_research::*;
use crate::tokens::Point;

const POKEMON: &str = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6";

/// What Foundation printed for these (probed with swiftc, 2026-11-06): `usd` and `price` in the reports.
#[test]
fn numbers_read_as_foundation_writes_them() {
    for (v, want) in [("0", "$0.00"), ("0.5", "$0.500"), ("5", "$5.00"), ("19.6", "$19.6"), ("950", "$950"), ("999.5", "$1.00K"), ("999.4", "$999"),
                      ("1234", "$1.23K"), ("19621", "$19.6K"), ("400000", "$400K"), ("999999", "$1.00M"), ("1500000", "$1.50M"), ("216361860", "$216M"),
                      ("1000000000000", "$1.00T"), ("1500000000000000", "$1500T"), ("1000000000000000000", "$1,000,000T"), ("-5", "-$5.00"),
                      ("-19621", "-$19.6K"), ("0.001234", "$0.00123"), ("99.95", "$100"), ("1015", "$1.02K"), ("1025", "$1.02K"), ("1005", "$1.00K"),
                      ("10050", "$10.0K"), ("0.125", "$0.125")] {
        assert_eq!(compact_usd(d(v)), want, "{v}");
    }
    for (x, g, g4) in [(0.01, "0.01", "0.01"), (2.5, "2.5", "2.5"), (1.0, "1", "1"), (100.0, "100", "100"), (0.00002, "2e-05", "2e-05"),
                       (1234567.0, "1.23457e+06", "1.235e+06"), (123456.0, "123456", "1.235e+05"), (99999.0, "99999", "1e+05"),
                       (0.00012345, "0.00012345", "0.0001234"), (9.99995, "9.99995", "10"), (0.0, "0", "0"), (-0.5, "-0.5", "-0.5")] {
        assert_eq!((format_g(x, 6).as_str(), format_g(x, 4).as_str()), (g, g4), "{x}");
    }
    assert!(iso8601("2026-09-06T04:36:17Z").is_some_and(|t| t.timestamp() == 1_788_669_377));
    assert!(iso8601("2026-09-06T04:36:17+03:00").is_some_and(|t| t.timestamp() == 1_788_658_577));
    assert!(iso8601("2026-09-06T04:36:17+0300").is_some_and(|t| t.timestamp() == 1_788_658_577));
    for s in ["2026-09-06T04:36:17.123Z", "2026-09-06 04:36:17Z", "2026-09-06t04:36:17z", "2026-09-06T04:36Z"] { assert!(iso8601(s).is_none(), "{s}") }
}

#[test]
fn a_mint_copied_from_a_link_is_repaired() {
    // «%2F» of a link copied along with the mint.
    assert_eq!(repairs(&format!("2F{POKEMON}")).first().map(String::as_str), Some(POKEMON));
    assert_eq!(repairs(POKEMON), [POKEMON]);
    assert!(repairs("tooshort").is_empty());
}

#[test]
fn jupiter_facts_say_what_matters() {
    let json = format!(r#"
    [{{"id": "{POKEMON}", "name": "POKÉMON CARDS", "symbol": "POKEMON", "usdPrice": 0.00036, "mcap": 360198, "fdv": 360198, "liquidity": 43138,
      "holderCount": 1359, "launchpad": "stonkfun", "createdAt": "2026-09-06T04:36:17Z", "graduatedAt": "2026-09-06T04:46:19Z",
      "stats1h": {{"priceChange": 158.9}}, "stats24h": {{"priceChange": 88.2, "buyVolume": 101421, "sellVolume": 89812, "buyOrganicVolume": 7626,
      "sellOrganicVolume": 11995, "numBuys": 681, "numSells": 460, "numTraders": 266}},
      "audit": {{"mintAuthorityDisabled": true, "freezeAuthorityDisabled": false, "topHoldersPercentage": 21.19, "devBalancePercentage": 1.5}},
      "organicScoreLabel": "low", "tags": ["unknown", "token-2022"]}}]"#);
    let now = iso8601("2026-09-29T04:36:17Z").unwrap();
    let facts = jupiter_facts(json.as_bytes(), POKEMON, now).unwrap();
    assert!(facts.contains(&format!("POKÉMON CARDS (POKEMON), mint {POKEMON}, chain id solana")));
    assert!(facts.contains("not verified"));
    assert!(facts.contains("1h +158.9%"));
    assert!(facts.contains("organic (real users, not bots) $19.6K"));
    assert!(facts.contains("Created 23 days ago on the stonkfun launchpad"));
    assert!(facts.contains("mint authority off"));
    assert!(facts.contains("FREEZE AUTHORITY ON"));
    assert!(facts.contains("Price $0.00036, market cap $360K, FDV $360K, liquidity $43.1K, 1359 holders."), "{facts}");
    assert_eq!(jupiter_facts(json.as_bytes(), "other", now), None);
}

#[test]
fn the_token_lookup_carries_the_month_the_card_draws() {
    // 180 four-hour closes from Sep 4 00:00 UTC: 0.60 falling to 0.40 by the middle, back up to 0.44.
    let start = 1_788_480_000.0;
    let points: Vec<Point> = (0..180)
        .map(|i| {
            let close = if i < 90 { 0.60 - i as f64 * 0.2 / 89.0 } else { 0.40 + (i - 90) as f64 * 0.04 / 89.0 };
            Point { time: unix(start + i as f64 * 14_400.0), close: d(&format!("{close}")) }
        })
        .collect();
    let facts = chart_facts(&points, Range::Month, "The pool", &Utc).unwrap();
    assert!(facts.starts_with("The pool over 30 days: $0.6 → $0.44 (-26.7%)"), "{facts}");
    assert!(facts.contains("low $0.4 on Sep 18"));
    assert!(facts.contains("high $0.6 on Sep 4"));
    // A close a day, the last one the price now.
    let closes: Vec<&str> = facts.split("Closes: ").nth(1).unwrap().split(", ").collect();
    assert!(closes.len() == 30 && closes.last() == Some(&"Oct 3 $0.44."), "{closes:?}");
    assert!(chart_facts(&points[..96], Range::Day, "It", &Utc).unwrap().contains("low $0.4 on 20:00"));
    assert_eq!(chart_facts(&[], Range::Month, "The pool", &Utc), None);
}

#[test]
fn a_search_puts_the_deepest_pool_first() {
    let json = r#"
    {"pairs": [
      {"chainId": "base", "baseToken": {"address": "0xfake", "symbol": "BONK", "name": "Bonk"}, "priceUsd": "0.1", "liquidity": {"usd": 5000}},
      {"chainId": "solana", "baseToken": {"address": "DezX", "symbol": "Bonk", "name": "Bonk"}, "priceUsd": "0.00002", "liquidity": {"usd": 400000},
       "priceChange": {"h24": -3.2}},
      {"chainId": "solana", "baseToken": {"address": "DezX", "symbol": "Bonk", "name": "Bonk"}, "priceUsd": "0.00002", "liquidity": {"usd": 90000}}
    ]}"#;
    let (facts, top) = search_facts(json.as_bytes(), "BONK", Utc::now()).unwrap();
    assert_eq!(top, "DezX");
    let lines: Vec<&str> = facts.split('\n').collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[1].starts_with("Bonk (Bonk) on chain id solana, contract DezX"));
    assert!(lines[1].contains("liquidity $400K, 24h -3.2%"), "{}", lines[1]);
    assert!(lines[2].contains("0xfake"));
}

#[test]
fn buying_by_mint_knows_its_risks() {
    let json = format!(r#"
    [{{"id": "{POKEMON}", "name": "POKÉMON CARDS", "symbol": "POKEMON", "decimals": 6, "tokenProgram": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
      "liquidity": 43138, "audit": {{"mintAuthorityDisabled": true, "freezeAuthorityDisabled": false}}, "tags": ["unknown"]}}]"#);
    let t = parse_solana_token(json.as_bytes(), POKEMON).unwrap();
    assert!(t.decimals == 6 && t.token2022 && !t.verified);
    let en = t.warnings(false);
    assert!(en[0].contains("freeze"));
    assert!(en.iter().any(|w| w.contains("thin")));
    assert!(!en.iter().any(|w| w.contains("print")));
    let ru = t.warnings(true);
    assert!(ru[0].starts_with("создатель может заморозить"));
    assert_eq!(ru.len(), en.len());
}

/// A transaction's hash is no token: the report names it whole, so a chart or a call can't take its first 40 hex.
// pending (round 4): the rest of aPastedAddressIsLookedUpBeforeTheModelReads (ChatRouter.pastedAddress, addresses, route).
#[tokio::test]
async fn a_hash_is_named_whole() {
    let hash = "0x9f2c4e1ab3d57e8f0c1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70";
    assert_eq!(report(hash).await, format!("{hash} is no token contract (an EVM one is 0x and 40 hex): a transaction's hash, a Sui address or a bytes32."));
}

#[test]
fn a_ticker_finds_jupiters_verified_token_not_its_namesakes() {
    let json = r#"
    [{"id": "Fake1", "symbol": "WIF", "liquidity": 900000000, "tags": ["unknown"]},
     {"id": "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", "symbol": "$WIF", "isVerified": true, "liquidity": 6858079},
     {"id": "Small", "symbol": "$WIF", "isVerified": true, "liquidity": 100}]"#;
    assert_eq!(parse_solana_mint(json.as_bytes(), "WIF").as_deref(), Some("EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm"));
    assert_eq!(parse_solana_mint(json.as_bytes(), "BONK"), None);
    let named = r#"
    [{"id": "SPXmint", "symbol": "SPX", "name": "SPX6900 (Wormhole)", "isVerified": true, "liquidity": 3000000},
     {"id": "FAKEmint", "symbol": "SPX6900", "name": "SPX6900 real", "isVerified": false, "liquidity": 50000000}]"#;
    assert_eq!(parse_solana_mint(named.as_bytes(), "spx6900").as_deref(), Some("SPXmint"));
    assert_eq!(parse_solana_mint(named.as_bytes(), "$SPX").as_deref(), Some("SPXmint"));
    assert_eq!(parse_solana_mint(named.as_bytes(), "spx6900 real"), None);
}

/// getAccountInfo's jsonParsed answer for a Token-2022 mint with these extensions.
fn mint(extensions: &str) -> Value { serde_json::from_str(&format!(r#"{{"value":{{"data":{{"parsed":{{"info":{{"extensions":[{extensions}]}}}}}}}}}}"#)).unwrap() }

/// TokenRisksTests (research E-8): what a Token-2022 mint lets its creator do is said, worst first; PYUSD's 0 % fee and
/// empty hook aren't.
#[test]
fn token2022_extensions_are_warned_about() {
    // PYUSD as read live on 2026-11-05.
    let pyusd = mint(r#"
        {"extension":"permanentDelegate","state":{"delegate":"2apBGMsS6ti9RyF5TwQTDswXBWskiJP2LD4cUEDqYJjk"}},
        {"extension":"transferFeeConfig","state":{"newerTransferFee":{"epoch":605,"maximumFee":0,"transferFeeBasisPoints":0},
         "olderTransferFee":{"epoch":605,"maximumFee":0,"transferFeeBasisPoints":0}}},
        {"extension":"transferHook","state":{"authority":"2apBGMsS6ti9RyF5TwQTDswXBWskiJP2LD4cUEDqYJjk","programId":null}}"#);
    assert_eq!(extension_warnings(Some(&pyusd), false), ["its creator can take or burn it in any wallet, yours too (permanent delegate)"]);
    let worst = mint(r#"
        {"extension":"transferFeeConfig","state":{"newerTransferFee":{"transferFeeBasisPoints":250},"olderTransferFee":{"transferFeeBasisPoints":0}}},
        {"extension":"transferHook","state":{"programId":"HooK111111111111111111111111111111111111111"}},
        {"extension":"pausableConfig","state":{"authority":"9foMHsSDq7nMg4WPusSz9eY7tyxyukqborA8GyU5cUxD","paused":true}}"#);
    let said = extension_warnings(Some(&worst), false);
    assert_eq!(said.len(), 3);
    assert!(said[0].contains("paused now") && said[1].contains("transfer hook") && said[2].contains("2.5%"));
    assert_eq!(extension_warnings(Some(&mint(r#"{"extension":"pausableConfig","state":{"authority":"x","paused":false}}"#)), true),
               ["эмитент может поставить все переводы на паузу (pausable)"]);
    assert!(extension_warnings(None, false).is_empty());
    // A fee capped at 0 takes nothing; a small one reads as itself, not «0.0%».
    let capped = mint(r#"{"extension":"transferFeeConfig","state":{"newerTransferFee":{"maximumFee":0,"transferFeeBasisPoints":500},"olderTransferFee":{"maximumFee":0,"transferFeeBasisPoints":500}}}"#);
    assert!(extension_warnings(Some(&capped), false).is_empty());
    let small = mint(r#"{"extension":"transferFeeConfig","state":{"newerTransferFee":{"maximumFee":1000,"transferFeeBasisPoints":1},"olderTransferFee":{"maximumFee":1000,"transferFeeBasisPoints":1}}}"#);
    assert_eq!(extension_warnings(Some(&small), false), ["every transfer pays its creator 0.01% (transfer fee)"]);
}

/// JupiterBusyTests: Jupiter's keyless limit answers the 6th request in 10 s with 429 and a JSON error. That answer is
/// «Jupiter didn't answer», never «no such token».
#[test]
fn a_throttled_or_down_jupiter_is_asked_again_and_an_empty_list_is_no() {
    for status in [429, 503] { assert!(answer(br#"{"code":429}"#, status, "Jupiter").unwrap_err().again) }
    // A refusal a second ask won't change is final, and still no answer.
    assert!(!answer(b"", 400, "Jupiter").unwrap_err().again);
    // 200 and nothing found: Jupiter answered no.
    assert_eq!(parse_solana_mint(&answer(b"[]", 200, "Jupiter").unwrap(), "BONK"), None);
    let bonk = "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263";
    let found = answer(format!(r#"[{{"id": "{bonk}", "symbol": "Bonk", "isVerified": true, "liquidity": 1000}}]"#).as_bytes(), 200, "Jupiter").unwrap();
    assert_eq!(parse_solana_mint(&found, "BONK").as_deref(), Some(bonk));
}

#[tokio::test]
async fn jupiter_still_throttled_after_the_retries_says_it_didnt_answer() {
    let e = net::run(&[], |_| async { Ok((br#"{"code":429,"message":"Too many requests"}"#.to_vec(), 429)) }, |data, status| answer(data, status, "Jupiter"))
        .await
        .unwrap_err();
    assert!(e.0.contains("didn't answer") && !e.0.contains(';'), "{}", e.0);
}

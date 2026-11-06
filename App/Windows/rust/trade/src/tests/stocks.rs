//! RobinhoodRegistryTests, and EVMTests' fresh quote that's more than 10% worse (Stocks.swift).

use ihatecrypto_core::tokens::TokenLookup;

use super::d;
use crate::Executor;
use crate::stocks::RobinhoodStocks;

/// The registry's mark goes by the contract on Robinhood Chain, never by the ticker: a copy under «NVDA» isn't the stock.
#[test]
fn robinhood_registry_marks_by_contract_on_robinhood_chain() {
    let json = r#"
    {"assets": [
      {"tokenSymbol": "NVDA", "tokenName": "NVIDIA • Robinhood Token", "status": "ASSET_STATUS_ACTIVE", "tokenDecimals": 18,
       "deployments": [{"contractAddress": "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC", "chainId": 4663}]},
      {"tokenSymbol": "SPCX", "tokenName": "Space Exploration Technologies • Robinhood Token", "status": "ASSET_STATUS_ACTIVE",
       "deployments": [{"contractAddress": "0x4a0E65A3EcceC6dBe60AE065F2e7bb85Fae35eEa", "chainId": 4663}]},
      {"tokenSymbol": "OLD", "status": "ASSET_STATUS_INACTIVE",
       "deployments": [{"contractAddress": "0x1111111111111111111111111111111111111111", "chainId": 4663}]},
      {"tokenSymbol": "ELSE", "status": "ASSET_STATUS_ACTIVE",
       "deployments": [{"contractAddress": "0x2222222222222222222222222222222222222222", "chainId": 1}]}
    ]}"#;
    let registry = RobinhoodStocks::parse(json.as_bytes());
    assert_eq!(registry.iter().map(|t| t.symbol.as_str()).collect::<Vec<_>>(), ["NVDA", "SPCX"]);
    assert_eq!((registry[0].name.as_str(), registry[0].decimals, registry[1].decimals), ("NVIDIA", 18, 18));

    let listed = |address: &str, symbol: &str, chain: &str| RobinhoodStocks::listed(address, symbol, chain, &registry);
    assert_eq!(listed("0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC", "NVDA", "robinhood"), Some(true));
    assert_eq!(listed("0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec", "nvda", "robinhood"), Some(true));
    // A copy on Robinhood Chain under the registry's ticker.
    assert_eq!(listed("0xA3284897ad7c8FED700b04f573Cf64E4f6c10f2c", "NVDA", "robinhood"), Some(false));
    // Another network: no mark at all, even for the registry's own address.
    assert_eq!(listed("0x8edc6b4df4e29AC2D5DAbd170B8AB0F700C59C95", "SPCX", "base"), None);
    assert_eq!(listed("0x4a0E65A3EcceC6dBe60AE065F2e7bb85Fae35eEa", "SPCX", "base"), None);
    // A token whose ticker isn't the registry's.
    assert_eq!(listed("0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18", "AI", "robinhood"), None);
    // Skipped by parse: inactive, or deployed elsewhere.
    assert_eq!(listed("0x1111111111111111111111111111111111111111", "OLD", "robinhood"), None);
    assert_eq!(listed("0x2222222222222222222222222222222222222222", "ELSE", "robinhood"), None);
}

/// A Stock Token's Uniswap price is checked against Robinhood's bid–ask, a token by contract against DexScreener's
/// price: with no price to check against, the trade stops before anything is bridged or signed.
#[test]
fn stock_trades_stop_without_a_price_to_check_against() {
    // Robinhood's answers: a quote, an unknown symbol (404), no quotes, a zero side.
    assert_eq!(RobinhoodStocks::parse_quote(br#"{"quotes":[{"tokenSymbol":"TSLA","bid":"401.27","ask":"401.8","isTradingHalt":false}]}"#),
               Some((d("401.27"), d("401.8"))));
    assert_eq!(RobinhoodStocks::parse_quote(br#"{"code":5,"message":"no whitelisted asset with symbol \"ZZZZ\"","details":[]}"#), None);
    assert_eq!(RobinhoodStocks::parse_quote(br#"{"quotes":[]}"#), None);
    assert_eq!(RobinhoodStocks::parse_quote(br#"{"quotes":[{"bid":"0","ask":"401.8"}]}"#), None);
    assert_eq!(RobinhoodStocks::parse_quote(br#"{"quotes":[{"bid":"401.27","ask":"0"}]}"#), None);
    assert_eq!(RobinhoodStocks::parse_quote(b"<html>502</html>"), None);
    // A Stock Token with no Robinhood quote, a contract with no DexScreener price: stopped, nothing signed.
    assert!(matches!(Executor::price_band("TSLA", None, None), Err(crate::Error::Execution(_))));
    assert_eq!(Executor::price_band("TSLA", None, Some((d("401"), d("402")))).unwrap().1, d("402"));
    // Within 5% of the band, both before the bridge and asked again after it; a price of nothing is outside.
    let band = (d("401"), d("402"));
    assert!(!Executor::off_band(d("400"), band) && !Executor::off_band(d("422"), band) && !Executor::off_band(d("381"), band));
    assert!(Executor::off_band(d("423"), band) && Executor::off_band(d("380"), band) && Executor::off_band(d("0"), band));
    let ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18";
    let pair = |price: &str| format!(r#"{{"pairs":[{{"chainId":"robinhood","baseToken":{{"address":"{ai}","name":"AI","symbol":"AI"}},{price}"liquidity":{{"usd":90000}}}}]}}"#);
    let listing = TokenLookup::parse(pair(r#""priceUsd":"0.157","#).as_bytes(), ai);
    assert_eq!(Executor::price_band("AI", listing.first(), None).unwrap().0, d("0.157"));
    // Listed, with no price or a zero one: DexScreener's branch stops it, not Robinhood's.
    for price in ["", r#""priceUsd":"0","#] {
        let listing = TokenLookup::parse(pair(price).as_bytes(), ai);
        let listing = listing.first().unwrap();
        assert!(listing.price_usd.unwrap_or_default().is_zero());
        assert!(matches!(Executor::price_band("AI", Some(listing), Some((d("401"), d("402")))), Err(crate::Error::Execution(_))));
    }
}

// Re-quoted after the approve: more than 10% fewer tokens than the first quote stops the swap with a warning.
#[test]
fn requote_worse_than_ten_percent_stops() {
    assert_eq!(Executor::worse_by_more_than_ten_percent(d("1000"), d("950")), None);
    assert_eq!(Executor::worse_by_more_than_ten_percent(d("1000"), d("900")), None);
    assert_eq!(Executor::worse_by_more_than_ten_percent(d("1000"), d("850")), Some(d("15")));
    assert_eq!(Executor::worse_by_more_than_ten_percent(d("1000"), d("1200")), None);
}

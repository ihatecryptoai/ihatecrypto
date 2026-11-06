use std::collections::HashSet;

use super::d;
use crate::foundation::{seconds, unix};
use crate::tokens::*;

#[test]
fn token_market_takes_the_deepest_pair_on_the_chain() {
    let json = r#"
    {"pairs": [
      {"chainId": "robinhood", "dexId": "uniswap", "labels": ["v4"], "pairAddress": "0xpool1", "url": "https://dexscreener.com/robinhood/0xpool1",
       "baseToken": {"address": "0xAbC", "symbol": "AI"}, "quoteToken": {"symbol": "USDG"}, "priceUsd": "0.2185",
       "priceChange": {"m5": -0.33, "h24": -12.17}, "liquidity": {"usd": 1296765.77}, "fdv": 216361860, "marketCap": 218686541,
       "volume": {"h24": 3438123.08}, "txns": {"h24": {"buys": 2621, "sells": 2107}}, "pairCreatedAt": 1788444274000,
       "info": {"imageUrl": "https://cdn/ai.png", "websites": [{"url": "https://artificialinu.com/", "label": "Website"}],
                "socials": [{"url": "https://x.com/artificiallyinu", "type": "twitter"}]}},
      {"chainId": "robinhood", "dexId": "ramses", "pairAddress": "0xpool2", "baseToken": {"address": "0xabc"}, "liquidity": {"usd": 121830}},
      {"chainId": "base", "dexId": "aerodrome", "pairAddress": "0xpool3", "baseToken": {"address": "0xabc"}, "liquidity": {"usd": 9000000}},
      {"chainId": "robinhood", "dexId": "uniswap", "pairAddress": "0xpool4", "baseToken": {"address": "0xother"}, "liquidity": {"usd": 9000000}}
    ]}
    "#;
    let info = TokenMarket::parse(json.as_bytes(), "0xabc", "robinhood").unwrap();
    assert_eq!(info.pool, "0xpool1");
    assert_eq!(info.dex, "uniswap v4");
    assert_eq!(info.quote, "USDG");
    assert_eq!(info.price_usd, Some(d("0.2185")));
    assert_eq!(info.changes["h24"], d("-12.17"));
    assert!(info.buys24h == Some(2621) && info.sells24h == Some(2107));
    assert_eq!(info.links.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["Website", "X"]);
    assert_eq!(info.created, Some(unix(1_788_444_274.0)));
    assert_eq!(info.liquidity_usd, Some(d("1296765.77")));
    assert!(TokenMarket::parse(json.as_bytes(), "0xabc", "solana").is_none());
}

#[test]
fn token_history_is_oldest_first() {
    let json = r#"{"data": {"attributes": {"ohlcv_list": [[1790532000, 0.22, 0.23, 0.21, 0.2185, 100], [1790528400, 0.21, 0.22, 0.2, 0.22, 90]]}}}"#;
    let points = TokenMarket::parse_history(json.as_bytes());
    assert_eq!(points.iter().map(|p| seconds(p.time)).collect::<Vec<_>>(), [1_790_528_400.0, 1_790_532_000.0]);
    assert_eq!(points.last().unwrap().close, d("0.2185"));
}

#[test]
fn dexscreener_pairs_to_token_info() {
    let json = r#"{"pairs":[{"chainId":"robinhood","url":"https://dexscreener.com/robinhood/0xpool1","baseToken":{"address":"0x2E8C31162B855A2FFA90F6F8634643AD6F111E18","name":"Artificial Inu","symbol":"AI"},"priceUsd":"0.2366","priceChange":{"h24":-3.5},"liquidity":{"usd":1369513.5},"info":{"imageUrl":"https://cdn.dexscreener.com/cms/images/x"}},{"chainId":"robinhood","baseToken":{"address":"0x2e8c31162b855a2ffa90f6f8634643ad6f111e18","name":"Artificial Inu","symbol":"AI"},"priceUsd":"0.2352","liquidity":{"usd":3301648.7}},{"chainId":"robinhood","baseToken":{"address":"0xother","name":"X","symbol":"X"},"priceUsd":"9","liquidity":{"usd":9999999}}]}"#;
    let infos = TokenLookup::parse(json.as_bytes(), "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18");
    assert_eq!(infos.len(), 1);
    assert!(infos[0].symbol == "AI" && infos[0].liquidity_usd == Some(d("3301648.7")));   // the deepest pair wins
    assert_eq!(infos[0].price_usd, Some(d("0.2352")));
}

/// RobinhoodRegistryTests' listings: a price, none, or a zero one.
// pending: Executor.priceBand on them (Execution).
#[test]
fn a_listing_with_no_price() {
    let ai = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18";
    let pair = |price: &str| format!(r#"{{"pairs":[{{"chainId":"robinhood","baseToken":{{"address":"{ai}","name":"AI","symbol":"AI"}},{price}"liquidity":{{"usd":90000}}}}]}}"#);
    assert_eq!(TokenLookup::parse(pair(r#""priceUsd":"0.157","#).as_bytes(), ai)[0].price_usd, Some(d("0.157")));
    for price in ["", r#""priceUsd":"0","#] {
        assert_eq!(TokenLookup::parse(pair(price).as_bytes(), ai)[0].price_usd.unwrap_or_default(), d("0"));
    }
}

#[test]
fn lookalike_tickers_are_suspicious() {
    let usdc = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
    let genuine: HashSet<String> = [usdc.to_lowercase()].into();
    assert!(!TokenLookup::suspicious("USDC", usdc, &genuine));
    assert!(TokenLookup::suspicious("USDC", "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", &genuine));
    assert!(TokenLookup::suspicious("USD\u{0421}", usdc, &genuine));   // Cyrillic «С»
    assert!(!TokenLookup::suspicious("AI", "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18", &genuine));
    // USD₮0 on its own contract.
    assert!(!TokenLookup::suspicious("USD₮0", "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", &HashSet::new()));
}

fn favorite(address: &str, chain: Option<&str>) -> Option<TokenCommand> { Some(TokenCommand::Favorite { address: address.into(), chain: chain.map(Into::into) }) }

// pending: the WalletCommand.parse lines of favoriteTokenPhrases and ChatRouter.forms/parses of tokenCardsAndWatchingParse.
#[test]
fn favorite_token_phrases() {
    let ai = "0x2e8c31162b855a2ffa90f6f8634643ad6f111e18";
    assert_eq!(TokenCommand::parse(&format!("и добавь в favorite вот этот токен на робингуде {ai}")), favorite(ai, Some("robinhood")));
    assert_eq!(TokenCommand::parse(&format!("watch token {ai} on Base")), favorite(ai, Some("base")));
    assert_eq!(TokenCommand::parse(&format!("add {ai} to favorites")), favorite(ai, None));
    assert_eq!(TokenCommand::parse("remove token AI from favorites"), Some(TokenCommand::Unfavorite { symbol: "AI".into() }));
    assert_eq!(TokenCommand::parse(&format!("watch token {ai} on robinhood")), favorite(ai, Some("robinhood")));
    // An address for the book, or a transfer, is not a token; «купи 0x… токен на $2» is a trade, not a favorite.
    assert_eq!(TokenCommand::parse(&format!("save {ai} as Bybit on Base")), None);
    assert_eq!(TokenCommand::parse(&format!("купи {ai} токен на $2")), None);
    assert_eq!(TokenCommand::parse(&format!("send 1 USDC to {ai}")), None);
}

#[test]
fn token_cards_and_watching_parse() {
    let mint = "B5ANwoWkptDv1QszdPHXjCF2gTWdo27FGLffdfXzzoe6";
    assert_eq!(TokenCommand::parse(&format!("chart token {mint} on solana")), Some(TokenCommand::Show { address: mint.into(), chain: Some("solana".into()) }));
    assert_eq!(TokenCommand::parse("show token 0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18 on robinhood"),
               Some(TokenCommand::Show { address: "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18".into(), chain: Some("robinhood".into()) }));
    assert_eq!(TokenCommand::parse(&format!("watch token {mint} on solana")), favorite(mint, Some("solana")));
}

/// TokenLookup.Info as Swift's Codable writes it.
#[test]
fn lookup_info_keeps_swifts_shape() {
    let info = LookupInfo { chain: "base".into(), address: "0xabc".into(), name: "A".into(), symbol: "A".into(), price_usd: Some(d("0.5")),
                            change24h: None, liquidity_usd: Some(d("10")), image_url: None, pair_url: Some("https://x.y".into()) };
    assert_eq!(serde_json::to_value(&info).unwrap(),
               serde_json::json!({"chain": "base", "address": "0xabc", "name": "A", "symbol": "A", "priceUSD": 0.5, "liquidityUSD": 10, "pairURL": "https://x.y"}));
}

//! Any token by its contract, a token's board and the chat's token commands (`Tokens.swift`).

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wallet::{Result, WalletError};

use crate::address_book_command::AddressBookCommand;
use crate::charts::Range;
use crate::foundation::{Date, capitalized, decimal, first, group, has, json_decimal, number, unix};
use crate::stablecoins;

/// A string field as Swift's `as? String` reads it.
fn string<'a>(v: &'a Value, key: &str) -> Option<&'a str> { v.get(key)?.as_str() }

/// `dec(_:)` in both parsers: a string as `Decimal(string:)` reads it, or a number.
fn dec(v: Option<&Value>) -> Option<Decimal> {
    let v = v?;
    v.as_str().map_or_else(|| json_decimal(v), decimal)
}

/// `URL(string:)`: anything but an empty string makes one (Foundation encodes what a URL can't hold).
fn url(s: Option<&str>) -> Option<String> { s.filter(|s| !s.is_empty()).map(Into::into) }

async fn fetch(url: &str) -> Result<(Vec<u8>, u16)> { net::get(url, &[]).await.map_err(|e| WalletError(e.to_string())) }

/// Any token by its contract: name, symbol, price, 24 h move, liquidity and its icon, from DexScreener's public API
/// (api.dexscreener.com, no key). The chain is the one with the most liquidity for it.
pub struct TokenLookup;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookupInfo {
    /// DexScreener's chain id: «robinhood», «base», «arbitrum», «solana», «ethereum»…
    pub chain: String,
    pub address: String,
    pub name: String,
    pub symbol: String,
    #[serde(rename = "priceUSD", default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub price_usd: Option<Decimal>,
    #[serde(rename = "change24h", default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change24h: Option<Decimal>,
    #[serde(rename = "liquidityUSD", default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub liquidity_usd: Option<Decimal>,
    #[serde(rename = "imageURL", default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(rename = "pairURL", default, skip_serializing_if = "Option::is_none")]
    pub pair_url: Option<String>,
}

impl TokenLookup {
    /// A symbol to warn about: letters outside ASCII (a Cyrillic «С» in «USDС»), or a major coin's ticker on a
    /// contract that isn't one of that coin's `genuine` (lowercased) contracts.
    pub fn suspicious(symbol: &str, contract: &str, genuine: &std::collections::HashSet<String>) -> bool {
        // A genuine contract's own symbol may be outside ASCII: USD₮0 on its own contract.
        let own = stablecoins::BY_NETWORK.iter().flat_map(|(_, coins)| coins.iter())
            .any(|c| c.address.to_lowercase() == contract.to_lowercase() && (c.symbol == symbol || c.also.contains(&symbol)));
        if own { return false }
        if !symbol.is_ascii() { return true }
        const MAJORS: [&str; 12] = ["USDC", "USDT", "USDT0", "USDG", "DAI", "USDS", "USDE", "ETH", "WETH", "BTC", "WBTC", "SOL"];
        MAJORS.contains(&symbol.to_uppercase().as_str()) && !genuine.contains(&contract.to_lowercase())
    }

    pub async fn lookup(address: &str) -> Result<Vec<LookupInfo>> {
        let (body, _) = fetch(&format!("https://api.dexscreener.com/latest/dex/tokens/{address}")).await?;
        Ok(Self::parse(&body, address))
    }

    /// One entry per chain, from the deepest pair where the token is the base; deepest chain first.
    // ponytail: chains as deep as each other keep the order they came in; Swift's dictionary leaves it to its hash.
    pub fn parse(data: &[u8], address: &str) -> Vec<LookupInfo> {
        let Ok(json) = serde_json::from_slice::<Value>(data) else { return vec![] };
        let Some(pairs) = json.get("pairs").and_then(Value::as_array) else { return vec![] };
        let mut best: Vec<(String, Decimal, LookupInfo)> = Vec::new();
        for p in pairs {
            let Some(base) = p.get("baseToken").filter(|b| b.is_object()) else { continue };
            let Some(chain) = string(p, "chainId") else { continue };
            if string(base, "address").map(str::to_lowercase) != Some(address.to_lowercase()) { continue }
            let liquidity = dec(p.get("liquidity").and_then(|l| l.get("usd"))).unwrap_or_default();
            let deepest = best.iter().position(|(c, _, _)| c == chain);
            if liquidity <= deepest.map_or(Decimal::NEGATIVE_ONE, |i| best[i].1) { continue }
            let info = LookupInfo {
                chain: chain.into(), address: string(base, "address").unwrap_or(address).into(),
                name: string(base, "name").unwrap_or("?").into(), symbol: string(base, "symbol").unwrap_or("?").into(),
                price_usd: dec(p.get("priceUsd")), change24h: dec(p.get("priceChange").and_then(|c| c.get("h24"))), liquidity_usd: Some(liquidity),
                image_url: url(p.get("info").and_then(|i| string(i, "imageUrl"))), pair_url: url(string(p, "url")),
            };
            match deepest { Some(i) => best[i] = (chain.into(), liquidity, info), None => best.push((chain.into(), liquidity, info)) }
        }
        best.sort_by(|a, b| b.1.cmp(&a.1));
        best.into_iter().map(|(_, _, info)| info).collect()
    }
}

/// What a token's detail board shows: its deepest pair on one chain (DexScreener) and that pool's price history
/// (GeckoTerminal's public API, no key, about 30 calls a minute).
pub struct TokenMarket;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketInfo {
    pub pool: String,
    pub name: String,
    pub symbol: String,
    /// «uniswap v4», «ramses».
    pub dex: String,
    pub quote: String,
    pub price_usd: Option<Decimal>,
    /// «m5», «h1», «h6», «h24» → percent.
    pub changes: HashMap<String, Decimal>,
    pub liquidity_usd: Option<Decimal>,
    pub fdv: Option<Decimal>,
    pub market_cap: Option<Decimal>,
    pub volume24h: Option<Decimal>,
    pub buys24h: Option<i64>,
    pub sells24h: Option<i64>,
    pub created: Option<Date>,
    pub image_url: Option<String>,
    pub pair_url: Option<String>,
    /// The project's website and socials, as the listing gives them.
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub time: Date,
    pub close: Decimal,
}

/// `(x as? NSNumber)?.intValue`.
fn int(v: Option<&Value>) -> Option<i64> { v?.as_i64().or_else(|| v?.as_f64().map(|f| f as i64)) }

impl TokenMarket {
    pub async fn info(address: &str, chain: &str) -> Result<Option<MarketInfo>> {
        let (body, _) = fetch(&format!("https://api.dexscreener.com/latest/dex/tokens/{address}")).await?;
        Ok(Self::parse(&body, address, chain))
    }

    /// The deepest pair on `chain` where the token is the base.
    pub fn parse(data: &[u8], address: &str, chain: &str) -> Option<MarketInfo> {
        let json: Value = serde_json::from_slice(data).ok()?;
        let pairs = json.get("pairs")?.as_array()?;
        let liquidity = |p: &Value| dec(p.get("liquidity").and_then(|l| l.get("usd"))).unwrap_or_default();
        // Any address value as text («\(x)»), lowercased.
        let base_address = |p: &Value| p.get("baseToken")?.get("address").map(|a| a.as_str().map_or_else(|| a.to_string(), str::to_string).to_lowercase());
        let ours = pairs.iter().filter(|p| base_address(p) == Some(address.to_lowercase()) && string(p, "chainId") == Some(chain));
        // The first of the deepest, as Swift's max(by:) keeps it.
        let p = ours.fold(None::<&Value>, |best, p| match best { Some(b) if liquidity(p) <= liquidity(b) => Some(b), _ => Some(p) })?;
        let pool = string(p, "pairAddress")?;
        let txns = p.get("txns").and_then(|t| t.get("h24"));
        let info = p.get("info");
        let mut links = Vec::new();
        for w in info.and_then(|i| i.get("websites")).and_then(Value::as_array).into_iter().flatten() {
            if let Some(u) = url(string(w, "url")) { links.push(Link { label: string(w, "label").unwrap_or("Website").into(), url: u }) }
        }
        for s in info.and_then(|i| i.get("socials")).and_then(Value::as_array).into_iter().flatten() {
            let kind = string(s, "type").unwrap_or("link");
            if let Some(u) = url(string(s, "url")) { links.push(Link { label: if kind == "twitter" { "X".into() } else { capitalized(kind) }, url: u }) }
        }
        let labels = p.get("labels").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        let base = p.get("baseToken");
        let changes = p.get("priceChange").and_then(Value::as_object)
            .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), dec(Some(v))?))).collect()).unwrap_or_default();
        Some(MarketInfo {
            pool: pool.into(),
            name: base.and_then(|b| string(b, "name")).unwrap_or("?").into(),
            symbol: base.and_then(|b| string(b, "symbol")).unwrap_or("?").into(),
            dex: [string(p, "dexId").unwrap_or("?"), &labels].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" "),
            quote: p.get("quoteToken").and_then(|q| string(q, "symbol")).unwrap_or("?").into(),
            price_usd: dec(p.get("priceUsd")),
            changes,
            liquidity_usd: dec(p.get("liquidity").and_then(|l| l.get("usd"))),
            fdv: dec(p.get("fdv")),
            market_cap: dec(p.get("marketCap")),
            volume24h: dec(p.get("volume").and_then(|v| v.get("h24"))),
            buys24h: int(txns.and_then(|t| t.get("buys"))),
            sells24h: int(txns.and_then(|t| t.get("sells"))),
            created: p.get("pairCreatedAt").and_then(Value::as_f64).map(|ms| unix(ms / 1000.0)),
            image_url: url(info.and_then(|i| string(i, "imageUrl"))),
            pair_url: url(string(p, "url")),
            links,
        })
    }

    /// The pool's closes in USD over the range, oldest first: 15-minute bars for a day, hourly for a week, 4-hour for a
    /// month.
    pub async fn history(chain: &str, pool: &str, range: Range) -> Result<Vec<Point>> {
        let network = if chain == "ethereum" { "eth" } else { chain };
        let (frame, aggregate, limit) = match range { Range::Day => ("minute", 15, 96), Range::Week => ("hour", 1, 168), Range::Month => ("hour", 4, 180) };
        let (body, status) = fetch(&format!(
            "https://api.geckoterminal.com/api/v2/networks/{network}/pools/{pool}/ohlcv/{frame}?aggregate={aggregate}&limit={limit}&currency=usd&token=base"
        )).await?;
        if status == 429 { return Err(WalletError("GeckoTerminal is busy, try again in a minute".into())) }
        Ok(Self::parse_history(&body))
    }

    /// GeckoTerminal's `ohlcv_list`: [time, open, high, low, close, volume], newest first.
    pub fn parse_history(data: &[u8]) -> Vec<Point> {
        let Ok(json) = serde_json::from_slice::<Value>(data) else { return vec![] };
        let Some(list) = json.pointer("/data/attributes/ohlcv_list").and_then(Value::as_array) else { return vec![] };
        // Every row a list of numbers, or nothing at all (`as? [[NSNumber]]`).
        let rows: Option<Vec<Vec<&Value>>> = list.iter().map(|r| r.as_array().filter(|r| r.iter().all(Value::is_number)).map(|r| r.iter().collect())).collect();
        let mut points: Vec<Point> = rows.unwrap_or_default().into_iter()
            .filter(|r| r.len() >= 5)
            .filter_map(|r| Some(Point { time: unix(r[0].as_f64()?), close: json_decimal(r[4])? }))
            .collect();
        points.sort_by_key(|p| p.time);
        points
    }
}

/// «добавь в favorite вот этот токен на робингуде 0x…», «watch token 0x… on Base», «remove token AI from favorites»,
/// «chart token B5AN… on solana».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenCommand {
    /// `chain`: as the user named it (DexScreener's id), nil = wherever it trades most.
    Favorite { address: String, chain: Option<String> },
    Unfavorite { symbol: String },
    /// The token's card on the canvas: its chart, market and links.
    Show { address: String, chain: Option<String> },
}

impl TokenCommand {
    pub fn parse(text: &str) -> Option<Self> {
        let q = text.to_lowercase();
        if let Some(m) = first(text, r"(?i)(?:remove|unstar|убери|удали)\s+(?:the\s+)?(?:token|токен)\s+\$?([a-z0-9]{1,12})\s+(?:from|из)\s+(?:my\s+)?(?:favou?rites?|избранн\w*|watchlist)") {
            return Some(Self::Unfavorite { symbol: group(&m, 1).to_uppercase() });
        }
        let address = AddressBookCommand::address(text)?;
        let chain = [("robinhood|робинхуд|робингуд", "robinhood"), (r"\bbase\b|бейс", "base"), ("arbitrum|арбитрум", "arbitrum"),
                     (r"ethereum|эфириум|\beth\s+mainnet", "ethereum")]
            .into_iter()
            .find(|(p, _)| has(&q, p))
            .map(|(_, c)| c)
            .or_else(|| (has(&q, "solana|солан") || !address.starts_with("0x")).then_some("solana"))
            .map(String::from);
        if has(&q, r"^\s*(?:chart|show)\s+token\b") { return Some(Self::Show { address, chain }) }
        let watching = has(&q, r"\btoken|токен|\bcoin\b|монет|watch|следи|favou?rite|избранн|watchlist");
        // «save 0x… as Bybit» is an address for the book, not a token; «купи 0x… токен» is a trade.
        let trading = has(&q, r"\bbuy\b|куп\w*|purchase|приобрет\w*|\bsell\b|прода\w*");
        let naming = has(&q, r"\bas\s|\bcalled\b|\bnamed\b|\bкак\s|под\s+именем");
        (watching && !trading && !naming).then_some(Self::Favorite { address, chain })
    }
}

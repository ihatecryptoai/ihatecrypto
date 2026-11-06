//! What the chat's model learns about a token before it answers, the «token» lookup (`TokenResearch.swift`): by its
//! contract or mint on any chain, or by its name or ticker. Solana tokens come from Jupiter's token list (holders,
//! safety flags, organic volume), every chain from DexScreener (pools, links). Public APIs, no keys. Every address in a
//! report is one an API returned, never the model's.

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::TimeZone;
use rust_decimal::Decimal;
use serde_json::{Map, Value};
use wallet::keys::base58;
use wallet::Result;

use crate::charts::Range;
use crate::foundation::{
    self, Date, chars, compact_usd, count, decimal, double, first, format_g, group, iso8601, json_decimal, replacing, seconds, trim_set,
    trim_spaces, unix,
};
use crate::tokens::{MarketInfo, Point, TokenLookup, TokenMarket};

/// `Holding.plain` (Holdings.swift): a token's name or ticker on one line, short, with nothing hidden in it — line
/// breaks, control characters and invisible formatting dropped, at most `max` characters.
// pending: it moves to Holdings with the rest of `Holding` (round 5).
pub fn plain(text: &str, max: usize) -> String {
    let kept: String = text.chars().filter(|&c| !wallet::address_check::hidden(c)).collect();
    let s = trim_spaces(&kept);
    if count(s) > max { format!("{}…", chars(s)[..max.saturating_sub(1)].concat()) } else { s.into() }
}

/// What buying a Solana token by its mint needs, and what to warn about.
#[derive(Debug, Clone, PartialEq)]
pub struct SolanaToken {
    pub name: String,
    pub symbol: String,
    pub decimals: i64,
    pub token2022: bool,
    pub verified: bool,
    pub liquidity_usd: Option<Decimal>,
    pub freezable: bool,
    pub mintable: bool,
    /// % the top holders own.
    pub top_holders: Option<Decimal>,
}

impl SolanaToken {
    /// What is risky about it, plainly, worst first.
    pub fn warnings(&self, russian: bool) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if self.freezable {
            out.push(if russian { "создатель может заморозить токены держателей (freeze authority включён) — продать может не получиться" }
                     else { "its creator can freeze holders' tokens (freeze authority is on), so you might not be able to sell" }.into());
        }
        if self.mintable {
            out.push(if russian { "создатель может выпустить ещё (mint authority включён)" } else { "its creator can print more of it (mint authority is on)" }.into());
        }
        if let Some(l) = self.liquidity_usd.filter(|l| *l < Decimal::from(50_000)) {
            out.push(if russian { format!("пул тонкий ({}): даже небольшая продажа двигает цену", usd(Some(l))) }
                     else { format!("its pool is thin ({}): a small sale moves the price", usd(Some(l))) });
        }
        if !self.verified { out.push(if russian { "Jupiter его не проверял" } else { "Jupiter hasn't verified it" }.into()) }
        if let Some(top) = self.top_holders.filter(|t| *t > Decimal::from(50)) {
            out.push(if russian { format!("у крупнейших держателей {}", pct(top, false)) } else { format!("the top holders own {}", pct(top, false)) });
        }
        out
    }
}

/// The Dev copy's «Jupiter is busy» (`-jupiterBusyTest YES`): each Jupiter request is answered 429 without going out,
/// for a live run of what the chat says then.
pub static BUSY_TEST: AtomicBool = AtomicBool::new(false);

/// Jupiter's token search: keyless on api.jup.ag (lite-api.jup.ag is being retired), 5 requests in 10 s per IP.
pub const JUPITER_SEARCH: &str = "https://api.jup.ag/tokens/v2/search?query=";

/// For the model when Jupiter's search didn't answer: whether a token is the verified one isn't known.
pub const BUSY: &str = "Jupiter didn't answer just now, so whether it knows or verified this token isn't known: ask again in a minute.";

pub async fn report(query: &str) -> String {
    let q = query.trim();
    if let Some(m) = first(q, r"0x[0-9a-fA-F]{40}(?![0-9a-fA-F])") {
        let evm = group(&m, 0).to_string();
        return dex(&evm).await.unwrap_or(format!("DexScreener has no pool for {evm} on any chain: nothing trades it, or it isn't a token."));
    }
    // Named whole: its first 40 hex in the report would be an address the chat could chart or call.
    if let Some(long) = first(q, r"0x[0-9a-fA-F]{41,}") {
        return format!("{} is no token contract (an EVM one is 0x and 40 hex): a transaction's hash, a Sui address or a bytes32.", group(&long, 0));
    }
    if let Some(m) = first(q, r"[1-9A-HJ-NP-Za-km-z]{30,60}") {
        let raw = group(&m, 0).to_string();
        let mut unanswered = false;
        for mint in repairs(&raw) {
            let note = (mint != raw).then(|| {
                format!("The pasted «{raw}» is not a valid Solana address ({} characters). Without {} it is this token: {mint}.", count(&raw),
                        if raw.starts_with(&format!("2F{mint}")) { "the «2F» in front — a link's %2F («/») copied along —" } else { "the extra characters" })
            });
            let (said, pools) = tokio::join!(jupiter(&mint), dex(&mint));
            let said = said.unwrap_or_else(|_| { unanswered = true; None });
            let facts: Vec<String> = [note.clone(), said, pools].into_iter().flatten().collect();
            if facts.len() > usize::from(note.is_some()) {
                return if unanswered { [vec![BUSY.to_string()], facts].concat() } else { facts }.join("\n");
            }
        }
        if unanswered { return format!("{BUSY} DexScreener has no pool for {raw} either.") }
        return if valid(&raw) {
            format!("Neither Jupiter nor DexScreener knows {raw} as a token: it may be a wallet address, or a token with no pool yet.")
        } else {
            format!("«{raw}» is not a valid Solana address, and no near fix of it is a known token. Ask for the address again, copied from the token's page.")
        };
    }
    search(q).await
}

/// The address as pasted when it's a valid Solana one; else what it likely was: a link's «%2F» / «2F» dropped from the
/// front, or a character or two too many at either end.
pub fn repairs(s: &str) -> Vec<String> {
    if valid(s) { return vec![s.into()] }
    let mut out: Vec<String> = ["%2F", "2F", "%3A", "3A"].iter().filter_map(|p| s.strip_prefix(p)).map(str::to_string).collect();
    let c = chars(s);
    for head in 0..=2 {
        for tail in 0..=2usize {
            if head + tail == 0 { continue }
            // `dropFirst(head).dropLast(tail)`: empty when they overlap.
            let start = head.min(c.len());
            out.push(c[start..c.len().saturating_sub(tail).max(start)].concat());
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.into_iter().filter(|m| valid(m) && seen.insert(m.clone())).collect()
}

fn valid(address: &str) -> bool { base58::decode(address).is_ok_and(|b| b.len() == 32) }

/// What a Token-2022 mint's extensions let its creator do to a holder, from getAccountInfo's jsonParsed answer for the
/// mint (research E-8): Jupiter's token list doesn't carry them (checked 2026-11-05). Worst first. A 0 % transfer fee
/// and a hook with no program (PYUSD's, Ondo's) do nothing today and aren't named.
pub fn extension_warnings(account: Option<&Value>, russian: bool) -> Vec<String> {
    let info = account.and_then(|a| a.get("value")?.get("data")?.get("parsed")?.get("info")).filter(|i| i.is_object());
    let empty = Map::new();
    let mut state: std::collections::HashMap<&str, &Map<String, Value>> = std::collections::HashMap::new();
    // `as? [[String: Any]]`: a list of objects, or nothing.
    if let Some(list) = info.and_then(|i| i.get("extensions")?.as_array()).filter(|l| l.iter().all(Value::is_object)) {
        for e in list {
            if let Some(name) = e.get("extension").and_then(Value::as_str) { state.insert(name, e.get("state").and_then(Value::as_object).unwrap_or(&empty)); }
        }
    }
    let field = |ext: &str, key: &str| state.get(ext).and_then(|s| s.get(key));
    let set = |v: Option<&Value>| v.is_some_and(|v| !v.is_null());
    let mut out: Vec<String> = Vec::new();
    let say = |ru: &str, en: &str| if russian { ru.to_string() } else { en.to_string() };
    if set(field("permanentDelegate", "delegate")) {
        out.push(say("создатель может забрать или сжечь его в любом кошельке, в твоём тоже (permanent delegate)",
                     "its creator can take or burn it in any wallet, yours too (permanent delegate)"));
    }
    if state.contains_key("nonTransferable") {
        out.push(say("его нельзя перевести, а значит и продать (non-transferable)", "it can't be transferred, so it can't be sold (non-transferable)"));
    }
    if field("pausableConfig", "paused").and_then(Value::as_bool) == Some(true) {
        out.push(say("переводы сейчас на паузе: продать не получится (pausable)", "its transfers are paused now, so it can't be sold (pausable)"));
    } else if set(field("pausableConfig", "authority")) {
        out.push(say("эмитент может поставить все переводы на паузу (pausable)", "its issuer can pause all transfers (pausable)"));
    }
    if set(field("transferHook", "programId")) {
        out.push(say("каждый перевод запускает программу создателя, она может не дать продать (transfer hook)",
                     "every transfer runs its creator's program, which can refuse a sale (transfer hook)"));
    }
    // A fee with a cap of 0 takes nothing.
    let fee = ["newerTransferFee", "olderTransferFee"].iter()
        .filter_map(|key| {
            let f = field("transferFeeConfig", key)?.as_object()?;
            if ns_int(f.get("maximumFee")) == Some(0) { return None }
            ns_int(f.get("transferFeeBasisPoints"))
        })
        .max()
        .unwrap_or(0);
    if fee > 0 {
        let p = format!("{}%", format_g(fee as f64 / 100.0, 6));
        out.push(if russian { format!("с каждого перевода создатель берёт {p} (transfer fee)") } else { format!("every transfer pays its creator {p} (transfer fee)") });
    }
    out
}

/// The Jupiter-verified token trading under a ticker («BONK»; «WIF» finds «$WIF») or named so («SPX6900», whose
/// ticker is SPX), the most liquid when several do: its mint. Unverified namesakes — the usual fakes — are never
/// picked. None: Jupiter answered and has none. An error: Jupiter didn't answer (`get`).
pub async fn solana_mint(symbol: &str) -> Result<Option<String>> {
    let query = percent(trim_set(symbol, "$"), b"");
    Ok(parse_solana_mint(&get(&format!("{JUPITER_SEARCH}{query}")).await?, symbol))
}

pub fn parse_solana_mint(data: &[u8], symbol: &str) -> Option<String> {
    fn plain(s: &str) -> String { trim_set(&s.to_uppercase(), "$").to_string() }
    // «SPX6900 (Wormhole)»: the bridge in brackets isn't part of the name people say.
    fn name(s: &str) -> String { plain(&replacing(s, r"\s*\([^)]*\)\s*$", |_| String::new())) }
    let wanted = plain(symbol);
    let tokens = objects(data);
    let matching = tokens.iter().filter(|t| {
        verified(t) && (plain(t.get("symbol").and_then(Value::as_str).unwrap_or("")) == wanted || name(t.get("name").and_then(Value::as_str).unwrap_or("")) == wanted)
    });
    // The first of the most liquid, as `max(by:)` keeps it.
    let best = matching.fold(None::<&Value>, |best, t| match best {
        Some(b) if dec(t.get("liquidity")).unwrap_or_default() <= dec(b.get("liquidity")).unwrap_or_default() => Some(b),
        _ => Some(t),
    });
    best?.get("id")?.as_str().map(str::to_string)
}

/// None: Jupiter answered and doesn't know the mint. An error: Jupiter didn't answer (`get`).
pub async fn solana_token(mint: &str) -> Result<Option<SolanaToken>> {
    Ok(parse_solana_token(&get(&format!("{JUPITER_SEARCH}{mint}")).await?, mint))
}

pub fn parse_solana_token(data: &[u8], mint: &str) -> Option<SolanaToken> {
    let tokens = objects(data);
    let t = tokens.iter().find(|t| t.get("id").and_then(Value::as_str) == Some(mint))?;
    let decimals = ns_int(t.get("decimals"))?;
    let audit = t.get("audit");
    let tags = tags(t);
    Some(SolanaToken {
        name: plain(t.get("name").and_then(Value::as_str).unwrap_or("?"), 32),
        symbol: plain(t.get("symbol").and_then(Value::as_str).unwrap_or("?"), 16),
        decimals,
        token2022: t.get("tokenProgram").and_then(Value::as_str) == Some("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb") || tags.contains(&"token-2022"),
        verified: verified(t),
        liquidity_usd: dec(t.get("liquidity")),
        freezable: audit.and_then(|a| a.get("freezeAuthorityDisabled")?.as_bool()) == Some(false),
        mintable: audit.and_then(|a| a.get("mintAuthorityDisabled")?.as_bool()) == Some(false),
        top_holders: dec(audit.and_then(|a| a.get("topHoldersPercentage"))),
    })
}

// MARK: Sources

async fn jupiter(mint: &str) -> Result<Option<String>> { Ok(jupiter_facts(&get(&format!("{JUPITER_SEARCH}{mint}")).await?, mint, chrono::Utc::now())) }

/// Jupiter's entry for the mint, as lines.
pub fn jupiter_facts(data: &[u8], mint: &str, now: Date) -> Option<String> {
    let tokens = objects(data);
    let t = tokens.iter().find(|t| t.get("id").and_then(Value::as_str) == Some(mint))?;
    let audit = t.get("audit").filter(|a| a.is_object());
    let tags = tags(t);
    let text = |key: &str| t.get(key).and_then(Value::as_str);
    let mut lines = vec![format!(
        "Solana token {} ({}), mint {mint}, chain id solana. Jupiter: {}{}.",
        plain(text("name").unwrap_or("?"), 32), plain(text("symbol").unwrap_or("?"), 16), if verified(t) { "verified" } else { "not verified" },
        if tags.is_empty() { String::new() } else { format!(", tags {}", tags.join(", ")) }
    )];
    lines.push(format!(
        "Price {}, market cap {}, FDV {}, liquidity {}{}.",
        price(dec(t.get("usdPrice"))), usd(dec(t.get("mcap"))), usd(dec(t.get("fdv"))), usd(dec(t.get("liquidity"))),
        ns_int(t.get("holderCount")).map_or(String::new(), |n| format!(", {n} holders"))
    ));
    let windows = [("5m", "stats5m"), ("1h", "stats1h"), ("6h", "stats6h"), ("24h", "stats24h")];
    let changes: Vec<String> = windows.iter().filter_map(|(title, key)| dec(t.get(*key)?.get("priceChange")).map(|c| format!("{title} {}", pct(c, true)))).collect();
    if !changes.is_empty() { lines.push(format!("Price change: {}.", changes.join(", "))) }
    if let Some(day) = t.get("stats24h").filter(|d| d.is_object()) {
        let sum = |a: &str, b: &str| dec(day.get(a)).unwrap_or_default().checked_add(dec(day.get(b)).unwrap_or_default());
        lines.push(format!(
            "Last 24h: {} buys, {} sells, {} traders, volume {}, of it organic (real users, not bots) {}; organic score {}.",
            int(day.get("numBuys")), int(day.get("numSells")), int(day.get("numTraders")), usd(sum("buyVolume", "sellVolume")),
            usd(sum("buyOrganicVolume", "sellOrganicVolume")), text("organicScoreLabel").unwrap_or("?")
        ));
    }
    let created = text("createdAt").and_then(iso8601);
    let graduated = text("graduatedAt").and_then(iso8601);
    let mut origin = created.map_or("Creation date unknown".to_string(), |c| format!("Created {} ago", age(c, now)));
    if let Some(pad) = text("launchpad") { origin += &format!(" on the {pad} launchpad") }
    if let Some(g) = graduated { origin += &format!("; graduated to an open pool {} ago", age(g, now)) }
    lines.push(origin + ".");
    let mut safety: Vec<String> = Vec::new();
    if let Some(off) = audit.and_then(|a| a.get("mintAuthorityDisabled")?.as_bool()) {
        safety.push(if off { "mint authority off (no one can print more)" } else { "MINT AUTHORITY ON: the creator can print more" }.into());
    }
    if let Some(off) = audit.and_then(|a| a.get("freezeAuthorityDisabled")?.as_bool()) {
        safety.push(if off { "freeze authority off" } else { "FREEZE AUTHORITY ON: the creator can freeze holders' tokens" }.into());
    }
    if let Some(top) = dec(audit.and_then(|a| a.get("topHoldersPercentage"))) { safety.push(format!("top 10 holders own {}", pct(top, false))) }
    if let Some(dev) = dec(audit.and_then(|a| a.get("devBalancePercentage"))) { safety.push(format!("the developer holds {}", pct(dev, false))) }
    if !safety.is_empty() { lines.push(format!("Safety: {}.", safety.join(", "))) }
    Some(lines.join("\n"))
}

/// DexScreener: where it trades and its deepest pool, with that pool's last 30 days (the chart on the token's card).
async fn dex(address: &str) -> Option<String> {
    let data = get(&format!("https://api.dexscreener.com/latest/dex/tokens/{address}")).await.ok()?;
    let facts = dex_facts(&data, address, chrono::Utc::now())?;
    let month = async {
        let top = TokenLookup::parse(&data, address).into_iter().next()?;
        let m = TokenMarket::parse(&data, address, &top.chain)?;
        let points = TokenMarket::history(&top.chain, &m.pool, Range::Month).await.ok()?;
        chart_facts(&points, Range::Month, "The deepest pool (GeckoTerminal, as the token's card draws it)", &chrono::Local)
    };
    Some(match month.await { Some(month) => format!("{facts}\n{month}"), None => facts })
}

/// A chart as the canvas draws it: the move over the range, its low and high, and about thirty closes, so levels and a
/// buy range come from the line the user sees. `zone`: the time zone the dates are written in.
pub fn chart_facts<Z: TimeZone>(points: &[Point], range: Range, what: &str, zone: &Z) -> Option<String>
where Z::Offset: std::fmt::Display {
    let (first, last) = (points.first()?, points.last()?);
    if first.close <= Decimal::ZERO { return None }
    // The first of the lowest and of the highest, as `min(by:)` and `max(by:)` keep them.
    let low = points.iter().fold(first, |a, p| if p.close < a.close { p } else { a });
    let high = points.iter().fold(first, |a, p| if p.close > a.close { p } else { a });
    let format = match range { Range::Day => "%H:%M", Range::Week => "%b %-d %H:%M", Range::Month => "%b %-d" };
    let f = |d: Date| d.with_timezone(zone).format(format).to_string();
    // From the newest back, so the last close is always in: a day's close each on a month of 4-hour bars.
    let step = (points.len() / 30).max(1);
    let mut closes: Vec<&Point> = (0..points.len()).rev().step_by(step).map(|i| &points[i]).collect();
    closes.reverse();
    let change = (foundation::Num::of(last.close) - first.close) / first.close * 100;
    Some(format!(
        "{what} over {}: {} → {} ({}), low {} on {}, high {} on {}.\nCloses: {}.",
        range.title(), price(Some(first.close)), price(Some(last.close)), change.0.map_or("nan%".into(), |c| pct(c, true)),
        price(Some(low.close)), f(low.time), price(Some(high.close)), f(high.time),
        closes.iter().map(|p| format!("{} {}", f(p.time), price(Some(p.close)))).collect::<Vec<_>>().join(", ")
    ))
}

pub fn dex_facts(data: &[u8], address: &str, now: Date) -> Option<String> {
    let chains = TokenLookup::parse(data, address);
    let top = chains.first()?;
    let m = TokenMarket::parse(data, address, &top.chain)?;
    let also = if chains.len() > 1 { format!("; also trades on {}", chains[1..].iter().map(|c| c.chain.as_str()).collect::<Vec<_>>().join(", ")) } else { String::new() };
    let head = format!("DexScreener: {} ({}), contract {}, chain id {}{also}.", plain(&m.name, 32), plain(&m.symbol, 16), top.address, top.chain);
    Some([vec![head], pool_facts(&m, now)].concat().join("\n"))
}

/// The deepest pool as lines: what the token's card shows under its chart.
pub fn pool_facts(m: &MarketInfo, now: Date) -> Vec<String> {
    let mut lines = vec![format!(
        "Deepest pool: {} {}/{}, liquidity {}{}; price {}, market cap {}, volume 24h {}{}{}.",
        m.dex, m.symbol, m.quote, usd(m.liquidity_usd), m.created.map_or(String::new(), |c| format!(", opened {} ago", age(c, now))),
        price(m.price_usd), usd(m.market_cap.or(m.fdv)), usd(m.volume24h),
        m.buys24h.map_or(String::new(), |b| format!(", {b} buys")), m.sells24h.map_or(String::new(), |s| format!(" and {s} sells in 24h"))
    )];
    let changes: Vec<String> = [("m5", "5m"), ("h1", "1h"), ("h6", "6h"), ("h24", "24h")].iter()
        .filter_map(|(key, title)| m.changes.get(*key).map(|c| format!("{title} {}", pct(*c, true)))).collect();
    if !changes.is_empty() { lines.push(format!("Price change: {}.", changes.join(", "))) }
    if !m.links.is_empty() { lines.push(format!("Links: {}.", m.links.iter().map(|l| format!("{} {}", l.label, l.url)).collect::<Vec<_>>().join(", "))) }
    lines
}

/// A name or ticker: the tokens called so, deepest pool first, and the deepest one in full (saves the model a round).
async fn search(query: &str) -> String {
    // A ticker Jupiter verified on Solana is that token. A pool anyone can open may claim any liquidity (a copy of STONK
    // claimed $6 billion and came first), and the verified list can't be faked that way.
    let verified = async { if count(query) <= 12 && !query.contains(' ') { solana_mint(query).await } else { Ok(None) } };
    let url = format!("https://api.dexscreener.com/latest/dex/search?q={}", percent(query, b"-._~!$&'()*+,;=:@/?"));
    let (verified, data) = tokio::join!(verified, get(&url));
    let data = data.ok();
    let found = data.as_deref().and_then(|d| search_facts(d, query, chrono::Utc::now()));
    // Jupiter not answering is no «not verified»: the deepest pool below may be a copy, and the model is told so.
    let (mint, head) = match verified {
        Ok(mint) => (mint, vec![]),
        Err(_) => (None, vec![format!("Jupiter didn't answer just now, so which «{query}» on Solana is the verified one isn't known: the deepest pool below may be a copy. Ask again in a minute before buying by this name.")]),
    };
    if let Some(mint) = mint {
        let (said, pools) = tokio::join!(jupiter(&mint), dex(&mint));
        let mut lines = vec![format!("«{query}» on Solana is the Jupiter-verified token with mint {mint}; use this one. Other tokens under the name are copies.")];
        lines.extend([said.ok().flatten(), pools].into_iter().flatten());
        if let Some((list, _)) = found {
            lines.push("For comparison, everything DexScreener finds under the name (the copies claim any liquidity):".into());
            lines.push(list);
        }
        return lines.join("\n");
    }
    if data.is_none() { return [head, vec!["DexScreener didn't answer the search.".into()]].concat().join("\n") }
    let Some((list, top)) = found else { return [head, vec![format!("No token called «{query}» trades anywhere DexScreener sees.")]].concat().join("\n") };
    let (said, pools) = tokio::join!(async { if top.starts_with("0x") { None } else { jupiter(&top).await.ok().flatten() } }, dex(&top));
    [head, vec![list, "The deepest one in full:".into()], [said, pools].into_iter().flatten().collect()].concat().join("\n")
}

/// The matches as lines, and the address of the deepest.
// ponytail: pools as deep as each other keep the order they came in; Swift's dictionary leaves it to its hash.
pub fn search_facts(data: &[u8], query: &str, now: Date) -> Option<(String, String)> {
    let json: Value = serde_json::from_slice(data).ok()?;
    let pairs = json.get("pairs")?.as_array().filter(|p| p.iter().all(Value::is_object))?;
    let mut best: Vec<(String, Decimal, String, String)> = Vec::new();
    for p in pairs {
        let Some(base) = p.get("baseToken").filter(|b| b.is_object()) else { continue };
        let (Some(address), Some(chain)) = (base.get("address").and_then(Value::as_str), p.get("chainId").and_then(Value::as_str)) else { continue };
        let liquidity = dec(p.get("liquidity").and_then(|l| l.get("usd"))).unwrap_or_default();
        let key = format!("{chain}:{address}");
        let at = best.iter().position(|b| b.0 == key);
        if liquidity <= at.map_or(Decimal::NEGATIVE_ONE, |i| best[i].1) { continue }
        let opened = p.get("pairCreatedAt").and_then(Value::as_f64).map(|ms| unix(ms / 1000.0));
        let change = dec(p.get("priceChange").and_then(|c| c.get("h24")));
        let line = format!(
            "{} ({}) on chain id {chain}, contract {address}: price {}, liquidity {}{}{}",
            plain(base.get("symbol").and_then(Value::as_str).unwrap_or("?"), 16), plain(base.get("name").and_then(Value::as_str).unwrap_or("?"), 32),
            price(dec(p.get("priceUsd"))), usd(Some(liquidity)), change.map_or(String::new(), |c| format!(", 24h {}", pct(c, true))),
            opened.map_or(String::new(), |o| format!(", pool opened {} ago", age(o, now)))
        );
        let entry = (key, liquidity, address.to_string(), line);
        match at { Some(i) => best[i] = entry, None => best.push(entry) }
    }
    best.sort_by_key(|b| std::cmp::Reverse(b.1));
    let top = best.first()?.2.clone();
    let lines: Vec<&str> = best.iter().take(6).map(|b| b.3.as_str()).collect();
    Some((format!("Tokens matching «{query}», deepest pool first. Many tokens copy a famous name: the deep, old pool is usually the real one; a thin, fresh one is usually a copy.\n{}", lines.join("\n")), top))
}

// MARK: Formatting and the network

/// A GET whose status is read: a throttle (429) or a server error is asked again 1, 2, 4 and 8 s later (net's
/// retries), past Jupiter's 10-second window, and then is an error. Jupiter's 429 carries a JSON error, which read as
/// data was «no such token».
// ponytail: net's client waits 30 s for an answer; Swift's request 10 s.
pub async fn get(url: &str) -> Result<Vec<u8>> {
    let host = url.split_once("://").map_or("", |(_, rest)| rest).split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host).split(':').next().unwrap_or(host).to_string();
    let jupiter = host.ends_with("jup.ag");
    let busy = jupiter && BUSY_TEST.load(Ordering::Relaxed);
    let source = if jupiter { "Jupiter".to_string() } else if host.is_empty() { "The service".to_string() } else { host };
    net::run(&net::DELAYS, |_| async move {
        if busy { return Ok((br#"{"code":429,"message":"Too many requests"}"#.to_vec(), 429)) }
        net::get(url, &[("Accept", "application/json")]).await
    }, |data, status| answer(data, status, &source)).await
}

/// Data only with a 2xx. 429 and 5xx are worth asking again; anything else is final. Either way it is no answer.
pub fn answer(data: &[u8], status: u16, source: &str) -> std::result::Result<Vec<u8>, net::Again> {
    if (200..300).contains(&status) { return Ok(data.to_vec()) }
    let why = if status == 429 { "too many requests right now".to_string() } else { format!("HTTP {status}") };
    Err(net::Again::new(status == 429 || status >= 500, format!("{source} didn't answer ({why}). Nothing was signed — try again in a minute.")))
}

/// `as? [[String: Any]]`: a list of objects, or nothing.
fn objects(data: &[u8]) -> Vec<Value> {
    match serde_json::from_slice::<Value>(data) { Ok(Value::Array(list)) if list.iter().all(Value::is_object) => list, _ => vec![] }
}

fn tags(t: &Value) -> Vec<&str> {
    // `as? [String]`: a list of strings, or none.
    t.get("tags").and_then(Value::as_array).filter(|l| l.iter().all(Value::is_string)).map_or(vec![], |l| l.iter().filter_map(Value::as_str).collect())
}

fn verified(t: &Value) -> bool { t.get("isVerified").and_then(Value::as_bool) == Some(true) || tags(t).contains(&"verified") }

/// `(v as? String).flatMap { Decimal(string:) } ?? (v as? NSNumber)?.decimalValue`.
fn dec(v: Option<&Value>) -> Option<Decimal> {
    let v = v?;
    match v { Value::String(s) => decimal(s), Value::Bool(b) => Some(Decimal::from(u8::from(*b))), _ => json_decimal(v) }
}

/// `(v as? NSNumber)?.intValue`: a number (cut to a whole one) or a JSON bool.
fn ns_int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)).or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

fn int(v: Option<&Value>) -> i64 { ns_int(v).unwrap_or(0) }

/// «$19.6K»; «unknown» for nothing.
fn usd(d: Option<Decimal>) -> String { d.map_or("unknown".into(), compact_usd) }

/// «$0.2185», «$2e-05»: four significant digits.
fn price(d: Option<Decimal>) -> String { d.map_or("unknown".into(), |d| format!("${}", format_g(double(d), 4))) }

/// «+3.2%», «-26.7%»; `sign` false: no «+».
fn pct(d: Decimal, sign: bool) -> String { format!("{}{:.1}%", if sign && d >= Decimal::ZERO { "+" } else { "" }, double(d)) }

/// «40 minutes», «5 hours», «23 days», «4 months».
fn age(date: Date, now: Date) -> String {
    let s = (seconds(now) - seconds(date)).max(0.0);
    if s < 7200.0 { format!("{} minutes", (s / 60.0) as i64) }
    else if s < 172_800.0 { format!("{} hours", (s / 3600.0) as i64) }
    else if s < 5_184_000.0 { format!("{} days", (s / 86400.0) as i64) }
    else { format!("{} months", (s / 2_592_000.0) as i64) }
}

/// `addingPercentEncoding`: every byte but ASCII letters, digits and `keep` as %XX.
fn percent(s: &str, keep: &[u8]) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || keep.contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

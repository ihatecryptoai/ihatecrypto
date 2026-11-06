//! The chat's reads of anything public (AgentReads.swift): a contract's view function, a node's read call,
//! Hyperliquid's info API and a short list of public data APIs. Nothing here signs or sends. With them the model answers
//! what no lookup was written for («какой у меня health factor на Aave», «сколько в пуле Fluid», «какие у меня
//! approvals») instead of waiting for code. The user's own addresses go in as `me` ({evm}, {solana} in a URL) and come
//! back out as `me`: the model never sees them.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use evm::U256;
use ihatecrypto_core::foundation::decimal;
use ihatecrypto_core::holdings::BROWSER;
use net::solana_rpc::SolanaRpc;
use serde_json::{Map, Value};
use wallet::evm::keccak;
use wallet::{from_hex, to_hex};

use crate::{Error, EvmNetwork, Result, fail, hl_url};

/// The user's own addresses, which go in as `me` and come back out as `me`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Me {
    pub evm: Option<String>,
    pub solana: Option<String>,
}

pub struct AgentReads;

/// What JSONSerialization says of text that isn't JSON.
const UNREADABLE: &str = "The data couldn’t be read because it isn’t in the correct format.";

/// Swift's `.whitespaces`: spaces and tabs, not line breaks.
fn trimmed(s: &str) -> &str { s.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}')) }

/// A list's items, sorted and joined: «a, b, c».
fn sorted(items: &[&str]) -> String {
    let mut v = items.to_vec();
    v.sort_unstable();
    v.join(", ")
}

/// `split(separator:maxSplits:)` with empty pieces left out, as Swift does: the rest after the last split, as is.
fn split_max(s: &str, sep: char, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if out.len() == max { break }
        if c != sep { continue }
        if i > start { out.push(&s[start..i]) }
        start = i + c.len_utf8();
    }
    if start < s.len() { out.push(&s[start..]) }
    out
}

/// `name(args)` or, with `returns`, `name(args)(returns)`: Swift's `/(\w+)\(([^()]*)\)(?:\(([^()]*)\))?/` as a whole.
fn signature(s: &str, returns: bool) -> Option<(&str, &str, Option<&str>)> {
    let open = s.find('(')?;
    let name = &s[..open];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') { return None }
    let group = |rest: &'_ str| -> Option<(usize, usize)> {
        let close = rest[1..].find(['(', ')'])? + 1;
        (rest.as_bytes()[close] == b')').then_some((1, close))
    };
    let rest = &s[open..];
    let (a, b) = group(rest)?;
    let args = &rest[a..b];
    let after = &rest[b + 1..];
    if after.is_empty() { return Some((name, args, None)) }
    if !returns || !after.starts_with('(') { return None }
    let (c, d) = group(after)?;
    (d + 1 == after.len()).then_some((name, args, Some(&after[c..d])))
}

impl AgentReads {
    pub const TOOLS: [&'static str; 4] = ["call", "rpc", "hl", "fetch"];

    /// What one result may hold: the model rereads it with every later round.
    pub const CAP: usize = 6000;

    /// Read calls a node may be asked for; nothing that sends or signs.
    pub const EVM_READS: [&'static str; 14] = [
        "eth_call", "eth_getBalance", "eth_getCode", "eth_getTransactionCount", "eth_getTransactionReceipt", "eth_getTransactionByHash",
        "eth_blockNumber", "eth_gasPrice", "eth_feeHistory", "eth_getBlockByNumber", "eth_getLogs", "eth_getStorageAt", "eth_maxPriorityFeePerGas",
        "eth_chainId",
    ];
    pub const SOLANA_READS: [&'static str; 14] = [
        "getAccountInfo", "getBalance", "getMultipleAccounts", "getTokenAccountsByOwner", "getTokenAccountBalance", "getTokenSupply",
        "getTokenLargestAccounts", "getSignaturesForAddress", "getTransaction", "getSlot", "getEpochInfo", "getRecentPrioritizationFees",
        "getInflationRate", "getVoteAccounts",
    ];
    /// Public data APIs `fetch` reads (GET only), plus every Blockscout explorer.
    pub const HOSTS: [&'static str; 13] = [
        "api.llama.fi", "yields.llama.fi", "coins.llama.fi", "stablecoins.llama.fi", "bridges.llama.fi", "api.dexscreener.com",
        "api.geckoterminal.com", "api.coingecko.com", "lite-api.jup.ag", "api.jup.ag", "api.kamino.finance", "api-v2.pendle.finance",
        "explorer.optimism.io",
    ];

    pub async fn answer(tool: &str, arg: &str, me: &Me) -> String {
        let out = match tool {
            "call" => Self::call(arg, me).await,
            "rpc" => Self::rpc(arg, me).await,
            "hl" => Self::hl(arg, me).await,
            _ => Self::fetch(arg, me).await,
        };
        Self::masked(&out.unwrap_or_else(|e| format!("{tool} failed: {e}")), me)
    }

    fn keys() -> String { EvmNetwork::all().iter().map(|n| n.key).collect::<Vec<_>>().join(", ") }

    // MARK: call <network> <contract> <function(types)(returns)> [args…]

    async fn call(arg: &str, me: &Me) -> Result<String> {
        let parts: Vec<&str> = arg.split_whitespace().collect();
        let net = (parts.len() >= 3).then(|| Self::network(parts[0])).flatten();
        let Some(net) = net else {
            return fail(format!("Write it as: call <network> <contract> <function(argTypes)(returnTypes)> [args…], network one of {}.", Self::keys()));
        };
        let Some((name, types, returns)) = signature(parts[2], true) else {
            return fail(format!("«{}» isn't a function like balanceOf(address)(uint256): no spaces, no tuples.", parts[2]));
        };
        let (types, returns) = (Self::list(types), Self::list(returns.unwrap_or("")));
        let mine = me.evm.as_deref().unwrap_or("");
        let args: Vec<&str> = parts[3..].iter().map(|a| if *a == "me" { mine } else { a }).collect();
        if types.len() != args.len() { return fail(format!("{name} takes {} arguments, {} given.", types.len(), args.len())) }
        let result = net.rpc.read(if parts[1] == "me" { mine } else { parts[1] }, &Self::encode(name, &types, &args)?).await?;
        if returns.is_empty() {
            return Ok(format!("{name} on {} returned 0x{} (give the return types, e.g. (uint256), to read it)", net.name, to_hex(&result)));
        }
        Ok(format!("{name} on {} → {} (numbers in the token's smallest units: divide by 10^decimals)", net.name, Self::decode(&returns, &result).join(", ")))
    }

    /// «deposit(uint256,address)» with its arguments as written («me» = `me`): the call's data.
    pub fn calldata(function: &str, args: &[String], me: &str) -> Result<Vec<u8>> {
        let Some((name, types, _)) = signature(function, false) else {
            return fail(format!("«{function}» isn't a function like deposit(uint256,address)."));
        };
        let types = Self::list(types);
        if types.len() != args.len() { return fail(format!("{name} takes {} arguments, {} given.", types.len(), args.len())) }
        Self::encode(name, &types, &args.iter().map(|a| if a == "me" { me } else { a.as_str() }).collect::<Vec<_>>())
    }

    /// The call's data: the function's 4-byte selector, then a word per argument.
    pub fn encode(name: &str, types: &[&str], args: &[&str]) -> Result<Vec<u8>> {
        let mut data = keccak(format!("{name}({})", types.join(",")).as_bytes())[..4].to_vec();
        for (t, a) in types.iter().zip(args) { data.extend(Self::word(t, a)?) }
        Ok(data)
    }

    fn list(s: &str) -> Vec<&str> { s.split(',').map(trimmed).filter(|t| !t.is_empty()).collect() }

    fn network(name: &str) -> Option<&'static EvmNetwork> {
        let key = name.to_lowercase();
        let alias = match key.as_str() { "eth" | "mainnet" => "ethereum", "bsc" => "bnb", "arb" => "arbitrum", "op" => "optimism", "matic" => "polygon", k => k };
        EvmNetwork::named(alias)
    }

    fn word(ty: &str, value: &str) -> Result<[u8; 32]> {
        let hex = |v: &str| from_hex(v).filter(|d| d.len() <= 32);
        match ty {
            // Only as 0x and 40 hex digits: the chat's checks of the addresses in a call find them that way, and
            // abi::address would take one without the 0x too.
            "address" => {
                if !(value.len() == 42 && value.starts_with("0x") && value[2..].bytes().all(|b| b.is_ascii_hexdigit())) {
                    return fail(format!("«{value}» isn't an address: write it as 0x and 40 hex digits."));
                }
                Ok(evm::abi::address(value)?)
            }
            "bool" => Ok(evm::abi::uint(u64::from(["true", "1"].contains(&value.to_lowercase().as_str())))),
            t if t.starts_with("uint") || t.starts_with("int") => {
                // «max»: all of it (type(uint256).max), the way vaults and Compound take «everything».
                if value.eq_ignore_ascii_case("max") && t.starts_with("uint") { return Ok([0xff; 32]) }
                if value.starts_with("0x") && let Some(d) = hex(value) {
                    let mut w = [0u8; 32];
                    w[32 - d.len()..].copy_from_slice(&d);
                    return Ok(w);
                }
                match decimal(value).filter(|n| *n >= rust_decimal::Decimal::ZERO) {
                    Some(n) => Ok(evm::abi::word_decimal(n)?),
                    None => fail(format!("«{value}» isn't a whole number ≥ 0 for {t}.")),
                }
            }
            t if t.starts_with("bytes") && t.chars().count() > 5 => {
                let Some(d) = hex(value) else { return fail(format!("«{value}» isn't hex of at most 32 bytes for {t}.")) };
                let mut w = [0u8; 32];
                w[..d.len()].copy_from_slice(&d);
                Ok(w)
            }
            _ => fail(format!("Arguments of type «{ty}» aren't read here: address, bool, uint/int, bytes1–32 only.")),
        }
    }

    /// Each return value in words. A contract's answer is someone else's data: an offset or a length past what came
    /// reads as «?», never stops the app.
    // ponytail: numbers past 38 digits are printed whole; Swift's Decimal kept 38 of their digits.
    pub fn decode(types: &[&str], data: &[u8]) -> Vec<String> {
        let slot = |i: usize| data.get(i * 32..(i + 1) * 32);
        let dynamic = |i: usize| -> Option<&[u8]> {
            let offset = evm::abi::uint_in(data, i).filter(|o| o % 32 == 0)?;
            let length = evm::abi::uint_in(data, usize::try_from(offset / 32).ok()?).filter(|l| *l <= data.len() as u64)?;
            let start = usize::try_from(offset).ok()?.checked_add(32)?;
            data.get(start..start.checked_add(length as usize)?)
        };
        types.iter().enumerate().map(|(i, t)| {
            match *t {
                "string" => return dynamic(i).and_then(|d| std::str::from_utf8(d).ok()).map_or("?".into(), |s| format!("«{s}»")),
                "bytes" => return dynamic(i).map_or("?".into(), |d| format!("0x{}", to_hex(d))),
                _ => {}
            }
            let Some(w) = slot(i) else { return "?".into() };
            let number = U256::from_be_slice(w);
            match *t {
                "address" => format!("0x{}", to_hex(&w[12..])),
                "bool" => if w.iter().any(|&b| b != 0) { "true".into() } else { "false".into() },
                t if t.starts_with("uint") => if number == U256::MAX { "max (2^256−1, unlimited)".into() } else { number.to_string() },
                // Two's complement: a set top bit is negative.
                t if t.starts_with("int") => if w[0] >= 0x80 { format!("-{}", (!number) + U256::from(1)) } else { number.to_string() },
                _ => format!("0x{}", to_hex(w)),
            }
        }).collect()
    }

    // MARK: rpc <network|solana> <method> [JSON params]

    async fn rpc(arg: &str, me: &Me) -> Result<String> {
        let parts = split_max(trimmed(arg), ' ', 2);
        if parts.len() < 2 { return fail("Write it as: rpc <network|solana> <method> [JSON params array].") }
        let solana = parts[0].eq_ignore_ascii_case("solana");
        let mine = if solana { me.solana.as_deref() } else { me.evm.as_deref() }.unwrap_or("");
        let text = parts.get(2).copied().unwrap_or("[]").replace("\"me\"", &format!("\"{mine}\""));
        let parsed: Value = serde_json::from_str(&text).map_err(|_| Error::Execution(UNREADABLE.into()))?;
        let Value::Array(params) = parsed else { return fail("The params must be a JSON array.") };
        let result = if solana {
            if !Self::SOLANA_READS.contains(&parts[1]) { return fail(format!("Only reads on Solana: {}.", sorted(&Self::SOLANA_READS))) }
            SolanaRpc::new().call(parts[1], Value::Array(params)).await?
        } else {
            let Some(net) = Self::network(parts[0]) else { return fail(format!("No network «{}»: solana or {}.", parts[0], Self::keys())) };
            if !Self::EVM_READS.contains(&parts[1]) { return fail(format!("Only reads: {}.", sorted(&Self::EVM_READS))) }
            net.rpc.call(parts[1], Value::Array(params)).await?
        };
        Ok(Self::compact(&result))
    }

    // MARK: hl <JSON body> [find <words>]

    async fn hl(arg: &str, me: &Me) -> Result<String> {
        let (body, find) = Self::split(arg);
        let text = body.replace("\"me\"", &format!("\"{}\"", me.evm.as_deref().unwrap_or("")));
        let parsed: Value = serde_json::from_str(&text).map_err(|_| Error::Execution(UNREADABLE.into()))?;
        if !parsed.as_object().is_some_and(|o| o.contains_key("type")) {
            return fail("Write it as: hl {\"type\": \"…\", …} — the body of a Hyperliquid info request.");
        }
        let data = Self::cached(&format!("hl {text}"), || async {
            llm::http::post_json(&hl_url("/info"), &parsed, &[], 20).await.map_err(|e| Error::Wallet(e.to_string()))
        }).await?;
        Ok(Self::shaped(&data, &find))
    }

    // MARK: fetch <URL> [find <words>]

    async fn fetch(arg: &str, me: &Me) -> Result<String> {
        let (raw, find) = Self::split(arg);
        let address = raw.replace("{evm}", me.evm.as_deref().unwrap_or("")).replace("{solana}", me.solana.as_deref().unwrap_or(""));
        let url = reqwest::Url::parse(&address).ok().filter(|u| u.scheme() == "https");
        let Some((url, host)) = url.and_then(|u| Some((u.to_string(), u.host_str()?.to_string()))) else {
            return fail(format!("«{raw}» isn't an https URL."));
        };
        let listed = Self::HOSTS.contains(&host.as_str());
        if !listed && !host.ends_with(".blockscout.com") {
            return fail(format!("{host} isn't among the public APIs read here: {}, <chain>.blockscout.com.", sorted(&Self::HOSTS)));
        }
        // Blockscout turns away clients that don't look like a browser (403).
        let headers: &[(&str, &str)] = if listed && host != "explorer.optimism.io" { &[] } else { &BROWSER };
        let data = Self::cached(&url, || async {
            llm::http::get(&url, headers, 30).await.map_err(|e| Error::Wallet(e.to_string()))
        }).await?;
        Ok(Self::shaped(&data, &find))
    }

    /// The same request within five minutes answers from memory: a follow-up about DefiLlama's 11 MB of pools doesn't
    /// download them again (≈ 5 s each). The last eight answers are kept.
    async fn cached<F: Future<Output = Result<Vec<u8>>>>(key: &str, load: impl FnOnce() -> F) -> Result<Vec<u8>> {
        type Kept = HashMap<String, (Instant, Vec<u8>)>;
        static KEPT: LazyLock<Mutex<Kept>> = LazyLock::new(Default::default);
        let kept = || KEPT.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, hit)) = kept().get(key).filter(|(at, _)| at.elapsed() < Duration::from_secs(300)) { return Ok(hit.clone()) }
        let data = load().await?;
        let mut k = kept();
        k.insert(key.into(), (Instant::now(), data.clone()));
        // ponytail: oldest-first eviction over eight entries; an LRU if reads ever repeat across many URLs.
        while k.len() > 8 {
            let Some(oldest) = k.iter().min_by_key(|(_, (at, _))| *at).map(|(key, _)| key.clone()) else { break };
            k.remove(&oldest);
        }
        Ok(data)
    }

    /// «<what> find <words>»: the words keep only the list items holding all of them.
    pub fn split(arg: &str) -> (String, Vec<String>) {
        match arg.rfind(" find ") {
            None => (trimmed(arg).to_string(), vec![]),
            Some(i) => (trimmed(&arg[..i]).to_string(), arg[i + 6..].to_lowercase().split_whitespace().map(str::to_string).collect()),
        }
    }

    /// JSON, compact; with `find`, only the items of its longest list that hold every word.
    pub fn shaped(data: &[u8], find: &[String]) -> String {
        let cap = Self::CAP;
        let Ok(json) = serde_json::from_slice::<Value>(data) else {
            let text = String::from_utf8_lossy(&data[..data.len().min(cap)]).into_owned();
            return if text.to_lowercase().contains("<html") { "That's a web page, not JSON: only JSON APIs are read.".into() } else { text };
        };
        let prefix = |s: &str| s.chars().take(cap).collect::<String>();
        if find.is_empty() {
            let text = Self::compact(&json);
            if text.chars().count() <= cap { return text }
            return format!("{}… (cut at {cap} characters: add «find <words>» to keep only the items with them)", prefix(&text));
        }
        let Some(items) = Self::longest_list(&json, 0) else { return format!("No list in the answer to look through: {}", prefix(&Self::compact(&json))) };
        let hits: Vec<&Value> = items.iter().filter(|item| {
            let s = Self::compact(item).to_lowercase();
            find.iter().all(|w| s.contains(w.as_str()))
        }).collect();
        let mut out = format!("{} of {} items hold «{}»:", hits.len(), items.len(), find.join(" "));
        let mut length = out.chars().count();
        for hit in hits {
            let line = Self::compact(hit);
            let n = line.chars().count();
            if length + n >= cap {
                out += "\n… (the rest cut: narrow it with more words)";
                break;
            }
            out += "\n";
            out += &line;
            length += n + 1;
        }
        out
    }

    fn longest_list(json: &Value, depth: usize) -> Option<&Vec<Value>> {
        let mut best = json.as_array();
        if depth >= 4 { return best }
        for child in json.as_object().into_iter().flat_map(Map::values) {
            if let Some(found) = Self::longest_list(child, depth + 1) && found.len() > best.map_or(0, Vec::len) { best = Some(found) }
        }
        best
    }

    /// JSON in one line, without the nulls and empty values that fill half of an API's rows.
    pub fn compact(json: &Value) -> String { Self::pruned(json).to_string() }

    fn pruned(json: &Value) -> Value {
        let empty = |v: &Value| match v {
            Value::Null => true,
            Value::String(s) => s.is_empty(),
            Value::Array(a) => a.is_empty(),
            Value::Object(o) => o.is_empty(),
            _ => false,
        };
        match json {
            Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), Self::pruned(v))).filter(|(_, v)| !empty(v)).collect()),
            Value::Array(a) => Value::Array(a.iter().map(Self::pruned).collect()),
            other => other.clone(),
        }
    }

    /// The user's own addresses, back as `me`.
    pub fn masked(text: &str, me: &Me) -> String {
        let mut out = text.to_string();
        for a in [&me.evm, &me.solana].into_iter().flatten().filter(|a| a.chars().count() > 20) {
            let (lower, want) = (out.to_ascii_lowercase(), a.to_ascii_lowercase());
            let mut next = String::with_capacity(out.len());
            let mut at = 0;
            while let Some(i) = lower[at..].find(&want) {
                next.push_str(&out[at..at + i]);
                next.push_str("me");
                at += i + want.len();
            }
            next.push_str(&out[at..]);
            out = next;
        }
        out
    }
}

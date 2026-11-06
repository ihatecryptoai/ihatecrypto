//! A contract call the chat wrote (ContractCalls.swift): what the network's explorer says about the contract, what else
//! is known of it (TVL, GoPlus Security's reports, whether this wallet ever called it), the last checks before it's
//! signed — and the call itself.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use evm::simulation::{Grant, Simulation};
use ihatecrypto_core::foundation::{Num, iso8601, iso8601_fraction, units};
use ihatecrypto_core::holdings::{self, BLOCKSCOUT, BROWSER};
use ihatecrypto_core::stablecoins;
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::{Map, Value};

use crate::{EvmNetwork, Executor, Result, u256};

/// GET with the browser's User-Agent where it's asked for; the body, or None (not 2xx, no answer).
async fn get(url: &str, headers: &[(&str, &str)], seconds: u64) -> Option<Vec<u8>> { llm::http::get(url, headers, seconds).await.ok() }

fn object(data: Option<Vec<u8>>) -> Option<Map<String, Value>> {
    match serde_json::from_slice(&data?).ok()? { Value::Object(o) => Some(o), _ => None }
}

/// What the network's explorer says about a contract the chat wants to call: a contract at all, its source (and a
/// proxy's implementation) published and verified, its name and its age. Blockscout's; none for a network without one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContractTrust {
    pub is_contract: bool,
    pub verified: bool,
    pub name: Option<String>,
    /// The code behind a proxy, by name (Blockscout names only a verified one).
    pub implementation: Option<String>,
    pub days: Option<i64>,
    /// No explorer for this network, or it didn't answer: nothing is known.
    pub unknown: bool,
}

impl ContractTrust {
    pub async fn check(address: &str, network: &str) -> ContractTrust {
        let unknown = ContractTrust { unknown: true, ..Default::default() };
        let Some(host) = holdings::lookup(&BLOCKSCOUT, network) else { return unknown };
        let Some(a) = object(get(&format!("https://{host}/api/v2/addresses/{address}"), &BROWSER, 15).await) else { return unknown };
        let flag = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
        let mut t = ContractTrust {
            is_contract: flag("is_contract"), verified: flag("is_verified"), name: a.get("name").and_then(Value::as_str).map(str::to_string),
            ..Default::default()
        };
        if let Some(implementation) = a.get("implementations").and_then(Value::as_array).filter(|l| l.iter().all(Value::is_object)).and_then(|l| l.first()) {
            t.implementation = implementation.get("name").and_then(Value::as_str).map(str::to_string);
            if t.implementation.is_none() { t.verified = false }
        }
        if let Some(tx) = a.get("creation_transaction_hash").and_then(Value::as_str) {
            let stamp = object(get(&format!("https://{host}/api/v2/transactions/{tx}"), &BROWSER, 15).await)
                .and_then(|d| d.get("timestamp").and_then(Value::as_str).map(str::to_string));
            if let Some(date) = stamp.and_then(|s| iso8601_fraction(&s).or_else(|| iso8601(&s))) {
                t.days = Some((Utc::now() - date).num_milliseconds() / 86_400_000);
            }
        }
        t
    }

    /// «verified · FluidVault (proxy → FluidVaultT1) · 412 days»
    pub fn line(&self) -> String {
        if self.unknown { return "not checked: no explorer answered for this network".into() }
        if !self.is_contract { return "not a contract".into() }
        format!("{}{}{}{}", if self.verified { "verified source" } else { "unverified source" }, self.name.as_ref().map_or(String::new(), |n| format!(" · {n}")),
                self.implementation.as_ref().map_or(String::new(), |i| format!(" (proxy → {i})")), self.days.map_or(String::new(), |d| format!(" · {d} days old")))
    }
}

impl Executor {
    /// A contract call the chat wrote and the user confirmed: its exact approve first when it has one (with the call as
    /// one transaction where batches are on and no coins go with it), then the call with its coins. The contract is
    /// passed as a recipient when the approve lets it spend or coins go with the call: the signer asks for it in person,
    /// address whole, the first time. `approve`: the token and the units the call may spend.
    #[allow(clippy::too_many_arguments)]
    pub async fn contract_call(&self, net: &EvmNetwork, to: &str, data: &[u8], value: u64, approve: Option<(&str, Decimal)>, approve_note: &str,
                               note: &str) -> Result<String> {
        let contract = [to.to_string()];
        let hash = match approve {
            Some((token, units)) => self.approve_and_send(&net.rpc, token, to, u256(units)?, approve_note, to, data, note, &contract, value).await?,
            None => self.send_evm(&net.rpc, to, data, note, if value > 0 { &contract[..] } else { &[] }, None, value).await?,
        };
        Ok(format!("{note}: done. {}/tx/{hash}", net.explorer))
    }
}

/// What else is known of a contract outside ihatecrypto's own list: its TVL priced by DefiLlama (its supply × the price
/// DefiLlama gives its token — a vault's shares, stETH), whether GoPlus Security reports it, and whether this wallet ever
/// sent it a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractReputation {
    pub tvl_usd: Option<Decimal>,
    /// Who reports it as a scam and for what: GoPlus Security, or Blockscout's token reputation.
    pub scam: Option<Report>,
    pub first_time: bool,
    /// GoPlus didn't answer: said on the card, the other checks still hold.
    pub unchecked: bool,
}

impl Default for ContractReputation {
    fn default() -> Self { Self { tvl_usd: None, scam: None, first_time: true, unchecked: false } }
}

impl ContractReputation {
    /// Below this a protocol isn't one DefiLlama counts as having money in it.
    pub const MIN_TVL: Decimal = Decimal::from_parts(1_000_000, 0, 0, false, 0);
    pub const MIN_DAYS: i64 = 30;

    pub async fn check(contract: &str, net: &EvmNetwork, me: &str) -> ContractReputation {
        let mut r = ContractReputation::default();
        let (total_supply, decimals_call) = ([0x18, 0x16, 0x0d, 0xdd], [0x31, 0x3c, 0xe5, 0x67]);
        let (token_path, sent_path) = (format!("tokens/{contract}"), format!("addresses/{me}/transactions?filter=from"));
        let (listed, price, supply, decimals, token, sent) = tokio::join!(
            ScamList::shared().check(contract, net.rpc.chain_id),
            async { Self::prices(&[contract.to_string()], net).await.remove(&contract.to_lowercase()) },
            net.rpc.read(contract, &total_supply),
            net.rpc.read(contract, &decimals_call),
            Self::blockscout(net, &token_path),
            Self::blockscout(net, &sent_path),
        );
        if let (Some(p), Some(s), Some(d)) = (price, supply.ok().and_then(|s| evm::abi::u256_in(&s, 0)), decimals.ok().and_then(|d| evm::abi::decimals(Some(&d)))) {
            // The supply in tokens: past Decimal's 28 digits its lowest digits go, not the number.
            r.tvl_usd = units(&s.to_string(), d as i64).and_then(|tokens| (Num::of(tokens) * p).0);
        }
        match listed {
            Verdict::Reported(why) => r.scam = Some(why),
            Verdict::Unchecked => r.unchecked = true,
            Verdict::Clean => {}
        }
        if r.scam.is_none() && token.and_then(|t| t.get("reputation")?.as_str().map(str::to_lowercase)).as_deref() == Some("scam") {
            r.scam = Some(Report { en: "Blockscout marks this address as a scam".into(), ru: "Blockscout помечает этот адрес как скам".into() });
        }
        let items = sent.and_then(|s| s.get("items")?.as_array().filter(|l| l.iter().all(Value::is_object)).cloned()).unwrap_or_default();
        let lower = contract.to_lowercase();
        r.first_time = !items.iter().filter_map(|i| i.get("to")?.get("hash")?.as_str()).any(|to| to.to_lowercase() == lower);
        r
    }

    /// Dollar prices DefiLlama gives these contracts on the network (lowercased keys); missing when it has none.
    pub async fn prices(tokens: &[String], net: &EvmNetwork) -> HashMap<String, Decimal> {
        holdings::llama_prices(net.key, tokens).await.into_iter().map(|(k, (price, _))| (k, price)).collect()
    }

    /// The network's own coin in dollars (Relay's price); 0 when it doesn't answer.
    pub async fn coin_usd(net: &EvmNetwork) -> Decimal { relay::native_price(net.rpc.chain_id).await.ok().flatten().unwrap_or_default() }

    async fn blockscout(net: &EvmNetwork, path: &str) -> Option<Map<String, Value>> {
        let host = holdings::lookup(&BLOCKSCOUT, net.key)?;
        object(get(&format!("https://{host}/api/v2/{path}"), &BROWSER, 15).await)
    }
}

/// Who reports an address and for what, in English and Russian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub en: String,
    pub ru: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Unchecked,
    Reported(Report),
}

/// GoPlus Security's reports on an address (api.gopluslabs.io, SlowMist's and ScamSniffer's data among others), asked
/// without a key. ScamSniffer's own address list stopped in February 2024. A report is never «clean» by default: GoPlus
/// answers its rate limit with HTTP 200 and code 4029, and that, like any failure, is «unchecked». It doesn't know every
/// drainer, so nothing says an address is clean — only that GoPlus reports it or didn't answer.
#[derive(Debug, Clone)]
pub struct ScamList {
    base: String,
    seen: Arc<Mutex<HashMap<String, (Verdict, Instant)>>>,
}

impl Default for ScamList {
    fn default() -> Self { Self::at("https://api.gopluslabs.io") }
}

impl ScamList {
    /// The one the app asks: its answers are kept for everyone.
    pub fn shared() -> &'static ScamList {
        static SHARED: LazyLock<ScamList> = LazyLock::new(ScamList::default);
        &SHARED
    }

    /// One that asks GoPlus at `base` (a stand-in in tests).
    pub fn at(base: &str) -> Self { Self { base: base.into(), seen: Arc::default() } }

    /// GoPlus's networks among ours. Another (HyperEVM, Tempo) answers zeros even for a drainer: its labels are per
    /// address, so Ethereum's are asked.
    pub const CHAINS: [u64; 7] = [1, 10, 56, 137, 4663, 8453, 42161];
    pub fn chain(id: u64) -> u64 { if Self::CHAINS.contains(&id) { id } else { 1 } }

    /// GoPlus's fields in the order they're named; any other one set is named by its key.
    pub const LABELS: [(&'static str, &'static str, &'static str); 18] = [
        ("stealing_attack", "theft", "кража"), ("phishing_activities", "phishing", "фишинг"),
        ("blacklist_doubt", "suspected malice", "подозрительный адрес"), ("honeypot_related_address", "ties to a honeypot", "связан с ханипотом"),
        ("number_of_malicious_contracts_created", "malicious contracts it created", "создавал вредоносные контракты"),
        ("fake_token", "a fake token", "поддельный токен"), ("fake_standard_interface", "a fake standard interface", "поддельный стандартный интерфейс"),
        ("cybercrime", "cybercrime", "киберпреступление"), ("financial_crime", "financial crime", "финансовое преступление"),
        ("money_laundering", "money laundering", "отмывание денег"), ("mixer", "mixer use", "миксер"), ("sanctioned", "sanctions", "санкции"),
        ("darkweb_transactions", "darkweb deals", "сделки в даркнете"), ("blackmail_activities", "blackmail", "шантаж"),
        ("malicious_mining_activities", "malicious mining", "вредоносный майнинг"), ("gas_abuse", "gas abuse", "злоупотребление газом"),
        ("fake_kyc", "fake KYC", "поддельный KYC"), ("reinit", "a contract that can be redeployed", "контракт можно развернуть заново"),
    ];

    /// An hour's answer is kept; «unchecked» never is. Five seconds at most: it runs again after Sign, and a GoPlus that
    /// doesn't answer only says so. The wallet's own addresses aren't to be passed: GoPlus sees each one.
    pub async fn check(&self, address: &str, chain_id: u64) -> Verdict {
        let address = address.to_lowercase();
        if !(address.len() == 42 && address.starts_with("0x") && address[2..].bytes().all(|b| b.is_ascii_hexdigit())) { return Verdict::Unchecked }
        let chain = Self::chain(chain_id);
        let key = format!("{chain}:{address}");
        let seen = || self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((v, _)) = seen().get(&key).filter(|(_, at)| at.elapsed() < Duration::from_secs(3600)) { return v.clone() }
        let Some(data) = get(&format!("{}/api/v1/address_security/{address}?chain_id={chain}", self.base), &[], 5).await else { return Verdict::Unchecked };
        let v = Self::verdict(&data);
        if v != Verdict::Unchecked { seen().insert(key, (v.clone(), Instant::now())); }
        v
    }

    /// Each address asked at once, keyed lowercased.
    pub async fn verdicts(&self, addresses: impl IntoIterator<Item = String>, chain_id: u64) -> HashMap<String, Verdict> {
        let mut tasks = tokio::task::JoinSet::new();
        for a in addresses.into_iter().map(|a| a.to_lowercase()).collect::<BTreeSet<_>>() {
            let list = self.clone();
            tasks.spawn(async move { let v = list.check(&a, chain_id).await; (a, v) });
        }
        let mut out = HashMap::new();
        while let Some(done) = tasks.join_next().await {
            if let Ok((a, v)) = done { out.insert(a, v); }
        }
        out
    }

    /// Code 1 with a result, else unchecked. Any field set but contract_address (a contract or not) and data_source
    /// (whose data it is) is a report.
    pub fn verdict(data: &[u8]) -> Verdict {
        let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(data) else { return Verdict::Unchecked };
        let Some(result) = j.get("result").and_then(Value::as_object).filter(|r| j.get("code").and_then(Value::as_f64) == Some(1.0) && !r.is_empty()) else {
            return Verdict::Unchecked;
        };
        // `(v as? String) ?? (v as? NSNumber)?.stringValue ?? "0"`.
        let said = |v: &Value| match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => if *b { "1".into() } else { "0".into() },
            Value::Number(n) if n.as_f64() == Some(0.0) => "0".into(),
            Value::Number(n) => n.to_string(),
            _ => "0".into(),
        };
        let set: BTreeSet<&str> = result.iter()
            .filter(|(k, v)| !["contract_address", "data_source"].contains(&k.as_str()) && !["", "0"].contains(&said(v).as_str()))
            .map(|(k, _)| k.as_str()).collect();
        if set.is_empty() { return Verdict::Clean }
        let named: Vec<_> = Self::LABELS.iter().filter(|(k, _, _)| set.contains(k)).collect();
        let other: Vec<String> = set.iter().filter(|k| !named.iter().any(|(n, _, _)| n == *k)).map(|k| k.replace('_', " ")).collect();
        let en: Vec<String> = named.iter().map(|l| l.1.to_string()).chain(other.iter().cloned()).collect();
        let ru: Vec<String> = named.iter().map(|l| l.2.to_string()).chain(other).collect();
        let from = result.get("data_source").and_then(Value::as_str).unwrap_or("").split(',').map(str::trim).filter(|s| !s.is_empty())
            .collect::<Vec<_>>().join(", ");
        let list = |items: &[String]| if items.len() < 2 { items.concat() } else { format!("{} and {}", items[..items.len() - 1].join(", "), items[items.len() - 1]) };
        Verdict::Reported(Report {
            en: format!("GoPlus Security reports this address for {}{}", list(&en), if from.is_empty() { String::new() } else { format!(" ({from} data)") }),
            ru: format!("у GoPlus Security на этом адресе {}{}{}", if ru.len() == 1 { "метка " } else { "метки " },
                        ru.iter().map(|r| format!("«{r}»")).collect::<Vec<_>>().join(", "), if from.is_empty() { String::new() } else { format!(" (данные {from})") }),
        })
    }
}

/// What a call to a protocol outside the list puts at risk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Risk {
    Usd(Decimal),
    Unpriced(String),
    Uncounted(String),
}

/// The last checks of a contract call: the rights it hands out, the money it puts at risk, and whether what's about to
/// be signed still does what the card showed.
pub struct ContractCallGuard;

static CAP_USD: RwLock<Option<Decimal>> = RwLock::new(None);

impl ContractCallGuard {
    /// Settings › Wallet: the most a call to a protocol outside ihatecrypto's own list may put at risk, in dollars.
    pub const CAP_KEY: &str = "contractCallCapUSD";
    /// $50 unless the engine handed in the user's own (`set_cap_usd`).
    pub fn cap_usd() -> Decimal { CAP_USD.read().unwrap_or_else(|e| e.into_inner()).unwrap_or(Decimal::from(50)) }
    pub fn set_cap_usd(usd: Option<Decimal>) { *CAP_USD.write().unwrap_or_else(|e| e.into_inner()) = usd }

    /// Why a right this call hands out is refused: to anyone but the called contract, unlimited, over all NFTs, an
    /// operator's or an authorization, credit, ownership. Only a limited approve to the contract itself passes.
    pub fn grant_problem(grants: &[Grant], contract: &str) -> Option<String> {
        for g in grants {
            if g.to.to_lowercase() != contract.to_lowercase() {
                return Some(format!("it would give {} a right over your {} ({} at {})", g.to, if g.kind == "approve" { "tokens" } else { "assets" }, g.kind, g.asset));
            }
            if g.kind != "approve" || g.unlimited {
                let what = if g.unlimited && g.kind == "approve" { "unlimited spending".to_string() } else { format!("{} rights", g.kind) };
                return Some(format!("it would give the contract {what} over your assets"));
            }
        }
        None
    }

    /// What a call to a protocol outside the list puts at risk, in dollars: what leaves the wallet, or its approve when
    /// that's more (the contract may take the rest of an approve later). A token neither DefiLlama nor the dollar list
    /// prices fails closed, an approve too — it's a spend that hasn't happened yet, never $0. An outflow without decimals
    /// (no amount) can't be counted; its price is asked first, so an NFT leaving says it can't be priced. `leaving`:
    /// (token, symbol, amount); a sum past range can't be priced either (Swift's NaN would have passed the cap).
    pub fn at_risk(leaving: &[(String, String, Option<Decimal>)], approve: Option<(&str, &str, Decimal)>, priced: &HashMap<String, Decimal>) -> Risk {
        let price = |token: &str, symbol: &str| priced.get(&token.to_lowercase()).copied().or_else(|| stablecoins::is_dollar(symbol).then_some(Decimal::ONE));
        let mut sum = Num::of(Decimal::ZERO);
        for (token, symbol, amount) in leaving {
            let Some(p) = price(token, symbol) else { return Risk::Unpriced(symbol.clone()) };
            let Some(amount) = amount else { return Risk::Uncounted(symbol.clone()) };
            sum = sum + Num::of(*amount) * p;
            if sum.0.is_none() { return Risk::Unpriced(symbol.clone()) }
        }
        let sum = sum.0.unwrap_or_default();
        // An approve of 0 is a revoke: it lets the contract take nothing.
        let Some((token, symbol, amount)) = approve.filter(|a| a.2 > Decimal::ZERO) else { return Risk::Usd(sum) };
        let Some(p) = price(token, symbol) else { return Risk::Unpriced(symbol.into()) };
        match (Num::of(amount) * p).0 {
            Some(approved) => Risk::Usd(sum.max(approved)),
            None => Risk::Unpriced(symbol.into()),
        }
    }

    /// What the re-simulation changed against the card: None when every balance moves within 1% of what it showed and
    /// the rights are the same.
    pub fn drift(card: &Simulation, now: &Simulation) -> Option<String> {
        if let Some(revert) = &now.revert { return Some(format!("it would now fail — {revert}")) }
        for asset in card.changes.keys().chain(now.changes.keys()).collect::<BTreeSet<_>>() {
            let (a, b) = (card.changes.get(asset).copied().unwrap_or(0), now.changes.get(asset).copied().unwrap_or(0));
            // |b − a| > |a| / 100, exactly.
            let moved = if a == 0 || b == 0 { a != b } else { b.abs_diff(a).checked_mul(100).is_none_or(|d| d > a.unsigned_abs()) };
            if moved { return Some(format!("a balance would change differently than the card showed ({asset})")) }
        }
        (now.grants != card.grants).then(|| "it would now hand out other rights than the card showed".into())
    }
}

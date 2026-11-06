//! The window's browser and its sites (Browser.swift): a site's request waiting for the person, what reviewing one
//! found, and the sites this run connected.
//! Layer 2: MetaMask's phishing list (phishing.json, refreshed daily), `dapp` (each wallet method a page calls: the
//! window reads the page's origin itself, never from the page), `ask` (the request's own block, answered once through
//! consent.rs), the EVM and Solana reviews (calldata read back, the guard, a simulation of our balances, the second
//! check), and the browser bar's `site_state`. Nothing is signed before the person's yes, and the signer checks each
//! transaction again.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use evm::call::Call;
use evm::protocols::{cctp, usdt0};
use guard::jev;
use ihatecrypto_core::foundation::{self, Num};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::SpotAsset;
use ihatecrypto_core::site_check::{self, Verdict};
use ihatecrypto_core::stablecoins;
use net::evm_rpc::EvmRpc;
use net::solana_rpc::SolanaRpc;
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};
use signer_api::{SignerRequest, SignerWallet};
use tokio::sync::{oneshot, watch};
use wallet::evm::{EvmSignature, personal};
use wallet::keys::base58;
use wallet::typed_data::TypedData;
use wallet::{Result, WalletError, from_hex, to_hex};

use crate::canvas::{Board, Step};
use crate::chat::JevPost;
use crate::consent::{SiteBalance, SiteAskView, SiteTick};
use crate::engine::Engine;
use crate::link_signer::reason;
use crate::notices::Notice;
use crate::store;

/// A site's request waiting for the person (the Mac's SiteRequest): its own block on the canvas, apart from the
/// person's card, answered once by its id (consent.rs).
#[derive(Debug)]
pub struct SiteAsk {
    pub view: SiteAskView,
    answered: Option<oneshot::Sender<bool>>,
}

impl SiteAsk {
    pub fn new(view: SiteAskView, answered: oneshot::Sender<bool>) -> Self { SiteAsk { view, answered: Some(answered) } }

    /// The page gets its answer once; later answers change nothing.
    pub fn answer(&mut self, yes: bool) {
        if let Some(tx) = self.answered.take() { let _ = tx.send(yes); }
    }
}

/// A site's request refused, with the words the page gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteError(pub String);

impl std::fmt::Display for SiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for SiteError {}

impl From<SiteError> for WalletError {
    fn from(e: SiteError) -> Self { WalletError(e.0) }
}

/// The page's answer is a refusal with these words («code|message» for an EVM call).
fn refused<T>(text: impl Into<String>) -> Result<T> { Err(SiteError(text.into()).into()) }

/// What reviewing a site's transaction found, for its block.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SiteReview {
    pub lines: Vec<String>,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
    pub problems: Vec<String>,
    pub recipients: Vec<String>,
    pub gas: u64,
    pub accounts: Vec<String>,
    pub token_owners: HashMap<String, String>,
    pub unknown_programs: Vec<String>,
    /// What needs the person's tick before Approve counts (consent.rs).
    pub ticks: Vec<SiteTick>,
    /// The card's lead, balances and folded checks (a Solana transaction, the Mac's sheet).
    pub lead: Option<String>,
    pub balances: Vec<SiteBalance>,
    pub checks: Vec<String>,
}

/// The sites connected in this run: their origins, by chain, and the EVM chain each switched to (Arbitrum by default).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConnectedSites {
    pub solana: HashSet<String>,
    pub evm: HashSet<String>,
    pub chain: HashMap<String, u64>,
}

/// The EVM calls a site may make that only read.
pub const READ_ONLY: [&str; 14] = [
    "eth_call", "eth_blockNumber", "eth_getBalance", "eth_getCode", "eth_getTransactionCount", "eth_getTransactionReceipt", "eth_getTransactionByHash",
    "eth_estimateGas", "eth_gasPrice", "eth_feeHistory", "eth_getBlockByNumber", "eth_getLogs", "eth_maxPriorityFeePerGas", "eth_getStorageAt",
];
/// The EVM networks a site may use, as the Mac's browser (Ethereum, Arbitrum, Base): where the balance simulation is
/// known to run, so a card never shows only «Unavailable».
pub const SITE_CHAINS: [u64; 3] = [1, 42161, 8453];
/// arb1 crashes on eth_simulateV1; publicnode runs it.
pub const SIMULATION_RPC: [(u64, &str); 1] = [(42161, "https://arbitrum-one-rpc.publicnode.com")];
/// Solana's own programs: one quiet line after the ones that do the work.
pub const SOLANA_PLUMBING: [&str; 5] = ["System", "Compute Budget", "SPL Token", "Associated Token", "Address Lookup Table"];
/// What the window's own part asks while the app is locked: the phishing check, which the Mac's browser makes whatever
/// the lock. The page beside the chat stays alive under the lock screen, and a check refused there would read as one
/// the window couldn't make, so nothing loads.
pub const BROWSER_WHILE_LOCKED: [&str; 1] = ["siteState"];

/// JSONDecoder's and JSONSerialization's words for an answer that isn't JSON.
const UNREADABLE: &str = "The data couldn’t be read because it isn’t in the correct format.";

fn prefix(s: &str, n: usize) -> String { s.chars().take(n).collect() }

fn suffix(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// `URL(string:)?.scheme` and `.host()`: an origin's scheme and host (an IPv6 one without its brackets); None without a
/// host.
fn scheme_host(origin: &str) -> Option<(String, String)> {
    let url = reqwest::Url::parse(origin).ok()?;
    let host = url.host_str()?.trim_start_matches('[').trim_end_matches(']').to_string();
    Some((url.scheme().to_string(), host))
}

/// «+1.5», «−0.2», «0»: a balance change as the cards say it.
fn signed(d: Decimal) -> String {
    format!("{}{}", if d > Decimal::ZERO { "+" } else if d < Decimal::ZERO { "−" } else { "" }, Fmt::num(d.abs(), 9))
}

/// A change in a token's smallest units, said in the token: its raw units with their sign when it's past Decimal.
fn signed_units(raw: i128, decimals: i64) -> String {
    foundation::units(&raw.to_string(), decimals).map_or_else(|| format!("{}{}", if raw < 0 { "−" } else { "+" }, raw.unsigned_abs()), signed)
}

// MARK: MetaMask's phishing list

type Fetch = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Option<Vec<u8>>> + Send>> + Send + Sync>;

const DAY: Duration = Duration::from_secs(86_400);

/// MetaMask's list of phishing sites (eth-phishing-detect), read once a day into phishing.json: a page on it gets no
/// wallet. As the Mac's PhishingList: the saved copy first, a new one when it's a day old. One refresh at a time, and a
/// check never reads an empty list while one runs: with a saved copy it reads that, with none it waits for the download.
/// The lookalike check runs either way.
pub struct PhishingList {
    file: PathBuf,
    fetch: Fetch,
    lists: Mutex<Lists>,
}

#[derive(Default)]
struct Lists {
    blocked: HashSet<String>,
    allowed: HashSet<String>,
    loaded_saved: bool,
    checked: Option<SystemTime>,
    /// The download under way: it says true once it's done.
    refreshing: Option<watch::Receiver<bool>>,
}

impl PhishingList {
    pub const SOURCE: &str = "https://raw.githubusercontent.com/MetaMask/eth-phishing-detect/main/src/config.json";

    pub fn new(file: PathBuf, fetch: Fetch) -> Arc<Self> { Arc::new(PhishingList { file, fetch, lists: Mutex::new(Lists::default()) }) }

    /// The engine's list (Swift's `shared`), in the data folder: one per folder, so each test's folder has its own.
    pub fn shared() -> Arc<Self> {
        thread_local! { static SHARED: RefCell<Option<Arc<PhishingList>>> = const { RefCell::new(None) }; }
        let file = store::url("phishing.json");
        SHARED.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(p) = s.as_ref().filter(|p| p.file == file) { return p.clone() }
            let p = Self::new(file, Arc::new(|| Box::pin(async {
                let (data, status) = net::get(Self::SOURCE, &[]).await.ok()?;
                (status == 200).then_some(data)
            })));
            *s = Some(p.clone());
            p
        })
    }

    fn lists(&self) -> std::sync::MutexGuard<'_, Lists> { self.lists.lock().unwrap_or_else(|e| e.into_inner()) }

    pub async fn contains(self: &Arc<Self>, host: &str) -> bool {
        let waiting = {
            let mut l = self.lists();
            if !l.loaded_saved {
                l.loaded_saved = true;
                if let Some((blocked, allowed)) = Self::parse(std::fs::read(&self.file).ok().as_deref()) { (l.blocked, l.allowed) = (blocked, allowed) }
            }
            if l.checked.is_none_or(|c| SystemTime::now().duration_since(c).is_ok_and(|e| e > DAY)) { self.start_refresh(&mut l) }
            // Nothing saved yet: the one download is the only list there is.
            if l.blocked.is_empty() { l.refreshing.clone() } else { None }
        };
        if let Some(mut done) = waiting { let _ = done.wait_for(|d| *d).await; }
        let l = self.lists();
        site_check::listed(host, &l.blocked, &l.allowed)
    }

    /// The engine's start: the saved copy read and the day's download begun before any site asks.
    pub async fn warm(self: &Arc<Self>) { self.contains("").await; }

    fn start_refresh(self: &Arc<Self>, l: &mut Lists) {
        if l.refreshing.is_some() { return }
        l.checked = Some(SystemTime::now());
        let saved = std::fs::metadata(&self.file).and_then(|m| m.modified()).ok();
        // A day old or none: a new one (a date ahead of the clock counts as fresh, as Swift's negative interval).
        if saved.is_some_and(|t| SystemTime::now().duration_since(t).map_or(true, |e| e <= DAY)) { return }
        let (done, waited) = watch::channel(false);
        l.refreshing = Some(waited);
        let me = self.clone();
        tokio::spawn(async move {
            if let Some(data) = (me.fetch)().await && let Some(lists) = Self::parse(Some(&data)) {
                let _ = store::write_at(&me.file, &data);
                let mut l = me.lists();
                (l.blocked, l.allowed) = lists;
            }
            me.lists().refreshing = None;
            let _ = done.send(true);
        });
    }

    fn parse(bytes: Option<&[u8]>) -> Option<(HashSet<String>, HashSet<String>)> {
        let j: Value = serde_json::from_slice(bytes?).ok()?;
        let words = |v: Option<&Value>| v?.as_array()?.iter().map(|x| x.as_str().map(str::to_string)).collect::<Option<HashSet<String>>>();
        Some((words(j.as_object()?.get("blacklist"))?, words(j.get("whitelist")).unwrap_or_default()))
    }
}

// MARK: A page's requests

/// The dApp browser's requests, as the Mac's Browser handles them: the window's browser (a webview with the provider
/// script) passes each call with the page's origin, which it reads itself, never from the page. Every request that
/// gives the site an address or a signature is a card on the canvas; nothing is signed before Confirm, and the
/// signer checks each transaction again.
impl Engine {
    /// One call from a page: the answer as JSON text, or an error «code|message» for EIP-1193.
    pub async fn dapp(&self, method: &str, text: &str, origin: &str, title: &str) -> Result<String> {
        let Some(host) = scheme_host(origin).filter(|(scheme, _)| scheme == "https").map(|(_, host)| host) else {
            return refused("Only https sites get the wallet.");
        };
        let params: Map<String, Value> = serde_json::from_str::<Value>(text).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
        // Disconnect only takes access away, so it works whatever wallet is on — a Ledger, the automation wallet, none
        // (the Mac's disconnectHere). The bar's takes the site's wallet on every chain; a page's own Solana disconnect
        // leaves its EVM connection alone.
        if method == "disconnect" {
            let mut st = self.st_mut();
            st.sites.solana.remove(origin);
            if params.get("everywhere").and_then(Value::as_bool) == Some(true) {
                st.sites.evm.remove(origin);
                st.sites.chain.remove(origin);
            }
            return Ok("null".into());
        }
        let eth = method == "eth";
        let code = if eth { "4100|" } else { "" };
        let wallet = self.st().active_wallet().filter(|w| w.automation != Some(true));
        let Some(wallet) = wallet else { return refused(code.to_string() + &self.say("Add a wallet first.", "Сначала добавь кошелёк.")) };
        if self.st().ledger_active() {
            return refused(code.to_string() + &self.say("The browser signs with ihatecrypto's own wallets for now: switch from the Ledger to use a site.",
                                                        "Браузер пока подписывает только кошельками ihatecrypto: переключись с Ledger, чтобы пользоваться сайтом."));
        }
        if PhishingList::shared().contains(&host).await {
            return refused(format!("{code}{host} is on MetaMask's list of phishing sites: ihatecrypto gives it no wallet."));
        }
        let site = site_check::check(&host);
        let inner = params.get("method").and_then(Value::as_str).unwrap_or("");
        let signs = ["signMessage", "signTransaction", "signAndSendTransaction"].contains(&method)
            || (eth && ["personal_sign", "eth_signTypedData_v4", "eth_sendTransaction"].contains(&inner));
        let answer = self.dapp_call(method, &params, origin, &host, title, &site, &wallet).await;
        // A site's signature is a move of its own: its Windows Hello doesn't carry over to the next request.
        if signs {
            let signer = self.signer.clone();
            tokio::task::spawn_local(async move { let _ = signer.send(&SignerRequest::EndSession {}).await; });
        }
        answer
    }

    #[allow(clippy::too_many_arguments)]
    async fn dapp_call(&self, method: &str, params: &Map<String, Value>, origin: &str, host: &str, title: &str, site: &Verdict, wallet: &SignerWallet)
                       -> Result<String> {
        let connected = self.st().sites.solana.contains(origin);
        let b64 = |key: &str| params.get(key).and_then(Value::as_str).and_then(|b| STANDARD.decode(b).ok());
        match method {
            "eth" => {
                let inner = params.get("method").and_then(Value::as_str).unwrap_or("");
                self.ethereum(inner, params.get("params").and_then(Value::as_str).unwrap_or("[]"), origin, host, title, site).await
            }
            "connect" => {
                let Some(owner) = wallet.solana.clone() else { return refused(wallet.missing("solana").unwrap_or_else(|| "No Solana address.".into())) };
                if !connected {
                    if params.get("silent").and_then(Value::as_bool) == Some(true) { return Ok("null".into()) }
                    let lines = [self.say("It sees your address and can ask you to sign. Nothing is signed without your Confirm.",
                                          "Сайт увидит адрес и сможет просить подпись. Без твоего «Подтвердить» ничего не подписывается."), owner.clone()];
                    let asked = self.say(&format!("{host} asks to connect to your Solana wallet"), &format!("{host} просит подключить твой кошелёк Solana"));
                    if !self.ask(host, origin, site, true, &asked, &lines, vec![], vec![], None, None).await { return refused("The user declined.") }
                    self.st_mut().sites.solana.insert(origin.into());
                }
                Ok(json!({"address": owner, "publicKey": STANDARD.encode(base58::decode(&owner)?)}).to_string())
            }
            "signMessage" => {
                let Some(bytes) = b64("message").filter(|_| connected) else { return refused("Connect the site first.") };
                let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| format!("(not text: {} bytes)", bytes.len()));
                let lines = [text, self.say("Signing text doesn't move money. The signer refuses it if it's a hidden transaction.",
                                            "Подпись текста не двигает деньги. Подписант откажет, если это скрытая транзакция.")];
                let asked = self.say(&format!("{host} asks to sign a message"), &format!("{host} просит подписать сообщение"));
                if !self.ask(host, origin, site, false, &asked, &lines, vec![], vec![], None, None).await { return refused("The user declined.") }
                let reply = self.signer.send(&SignerRequest::SignSolanaMessage { message: bytes }).await?;
                let Some(signature) = reply.signature_bytes.clone().filter(|_| reply.ok) else { return refused(reason(&reply)) };
                Ok(json!({"signature": STANDARD.encode(signature)}).to_string())
            }
            "signTransaction" | "signAndSendTransaction" => {
                let (Some(owner), true, Some(wire)) = (wallet.solana.clone(), connected, b64("transaction")) else { return refused("Connect the site first.") };
                let send = method == "signAndSendTransaction";
                let review = self.solana_review(&wire, &owner, host, title).await?;
                if !review.problems.is_empty() {
                    let balance = self.say("Your balance, simulated: ", "Твой баланс, по симуляции: ");
                    let lines = review.problems.iter().cloned().chain(review.lead.clone()).chain(review.changes.iter().map(|c| balance.clone() + c))
                        .chain(review.lines.iter().cloned()).collect();
                    self.pin_kind("result", &self.say(&format!("{host}: transaction refused"), &format!("{host}: транзакция отклонена")), lines, None, None, None);
                    let problems = review.problems.join(" ");
                    self.reply(&(self.say(&format!("{host} sent a transaction ihatecrypto won't sign: "), &format!("{host} прислал транзакцию, которую ihatecrypto не подпишет: "))
                                 + &problems), Some("browser".into()));
                    return refused(format!("Transaction declined: {problems}"));
                }
                let asked = if send { self.say(&format!("{host} asks to sign and send a transaction"), &format!("{host} просит подписать и отправить транзакцию")) }
                            else { self.say(&format!("{host} asks to sign a transaction"), &format!("{host} просит подписать транзакцию")) };
                let note = if send { self.say("After signing, ihatecrypto sends the transaction to the network.", "После подписи ihatecrypto сам отправит транзакцию в сеть.") }
                           else { self.say("The site gets the signed transaction and sends it itself.", "Сайт получит подписанную транзакцию и отправит её сам.") };
                if !self.ask(host, origin, site, false, &asked, &review.lines, review.warnings.clone(), review.ticks.clone(), Some(&review), Some(note)).await {
                    return refused("Transaction declined.");
                }
                let reply = self.signer.send(&SignerRequest::SignSolana {
                    wire: wire.clone(), accounts: review.accounts.clone(), recipients: vec![], token_owners: review.token_owners.clone().into_iter().collect(),
                    extra_programs: review.unknown_programs.clone(), sponsor: None,
                }).await?;
                let Some(signed) = reply.signed_wire.clone().filter(|_| reply.ok) else { return refused(reason(&reply)) };
                // The signer must return the page's transaction with only our signature added.
                let at = 1 + 64;
                if signed.len() != wire.len() || wire.len() < at || signed[at..] != wire[at..] { return refused("What was signed isn't what was checked.") }
                let signature = signed[1..at].to_vec();
                let short = prefix(&base58::encode(&signature), 12);
                self.reply(&self.say(&format!("{host}: transaction signed{} — {short}…", if send { " and sent" } else { "" }),
                                     &format!("{host}: транзакция подписана{} — {short}…", if send { " и отправлена" } else { "" })), Some("browser".into()));
                let me = self.rc();
                tokio::task::spawn_local(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    me.refresh().await;
                });
                if send {
                    SolanaRpc::new().send(&signed).await?;
                    return Ok(json!({"signature": STANDARD.encode(&signature)}).to_string());
                }
                Ok(json!({"signed": STANDARD.encode(&signed)}).to_string())
            }
            _ => refused(format!("ihatecrypto doesn't support {method}.")),
        }
    }

    /// A request waits on the canvas for the person (the Mac's ask): one at a time, so a page can't swap the one being
    /// read for another, and never in the place of their own card — it has its block and its answer (consent.rs), with a
    /// tick for each risk it carries before Approve counts.
    #[allow(clippy::too_many_arguments)]
    pub async fn ask(&self, host: &str, origin: &str, site: &Verdict, connect: bool, title: &str, lines: &[String], warnings: Vec<String>,
                     ticks: Vec<SiteTick>, review: Option<&SiteReview>, note: Option<String>) -> bool {
        if self.st().site_ask.is_some() { return false }
        let (mut warnings, mut ticks) = (warnings, ticks);
        let (mut check, mut real) = ("known", None);
        match site {
            Verdict::Lookalike(r) => {
                check = "lookalike";
                real = Some(r.clone());
                ticks.insert(0, Engine::lookalike_tick(r, |en, ru| self.say(en, ru)));
            }
            Verdict::Unknown => {
                check = "unknown";
                warnings.push(self.say(&format!("ihatecrypto doesn't know {host}: check the address bar."), &format!("ihatecrypto не знает {host}: проверь адрес в строке.")));
            }
            Verdict::Known => {}
            Verdict::Local => check = "local",
        }
        let view = SiteAskView {
            id: foundation::Uuid::new().to_string(), origin: origin.into(), host: host.into(), check: check.into(), real, title: title.into(), lines: lines.to_vec(),
            warnings, ticks, approve: if connect { self.say("Connect", "Подключить") } else { self.say("Sign", "Подписать") }, canvas: self.st().canvas_id.clone(),
            lead: review.and_then(|r| r.lead.clone()), balances: review.map(|r| r.balances.clone()).unwrap_or_default(),
            checks: review.map(|r| r.checks.clone()).unwrap_or_default(), note,
        };
        // Its block says the same in words: the lead, each balance and the checks, beside the lines.
        let said: Vec<String> = view.lead.iter().cloned()
            .chain(view.balances.iter().map(|b| {
                format!("{} {}", b.symbol, b.amount) + &b.usd.as_ref().map_or_else(String::new, |u| format!(" ≈ {u}"))
                    + &b.note.as_ref().map_or_else(String::new, |n| format!(" · {n}"))
            }))
            .chain(lines.iter().cloned()).chain(view.checks.iter().cloned()).chain(view.note.iter().cloned()).collect();
        self.out.event("showMain", &HashMap::from([("why", "site")]));
        // Only while the window isn't in front: in front, the request's own card is the ask (the Mac shows its sheet alone).
        self.notify(Notice::Confirm, host, &self.say("Asks for your wallet: look at ihatecrypto's window.", "Просит кошелёк: посмотри в окно ihatecrypto."),
                    false, true, None, None);
        // The window's call gives up after 900 s (main.rs dapp_call): declined before that, never signed for a page that left.
        let (me, id) = (self.rc(), view.id.clone());
        tokio::task::spawn_local(async move { me.expire_site(&id).await });
        let flagged: Vec<String> = view.warnings.iter().chain(view.ticks.iter().map(|t| &t.title)).map(|w| format!("⚠ {w}")).collect();
        let label = self.say("SITE · browser", "САЙТ · браузер");
        let id = view.id.clone();
        let (answered, answer) = oneshot::channel();
        self.st_mut().site_ask = Some(SiteAsk::new(view, answered));
        self.pin(Board { id, lines: flagged, steps: Some(vec![Step { label, title: title.into(), lines: said, money: None, status: None }]),
                         state: Some("waiting".into()), ..Board::new("move", title) });
        self.changed();
        answer.await.unwrap_or(false)
    }

    /// EIP-1193 requests. Answers are JSON text; errors are «code|message».
    async fn ethereum(&self, method: &str, params_json: &str, origin: &str, host: &str, title: &str, site: &Verdict) -> Result<String> {
        let wallet = self.st().active_wallet();
        let Some(address) = wallet.as_ref().and_then(|w| w.evm.clone()) else {
            return refused(format!("4100|{}", wallet.and_then(|w| w.missing("evm")).unwrap_or_else(|| "No EVM address.".into())));
        };
        let args: Vec<Value> = serde_json::from_str::<Value>(params_json).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default();
        let (chain, connected) = {
            let st = self.st();
            (st.sites.chain.get(origin).copied().unwrap_or(42161), st.sites.evm.contains(origin))
        };
        let rpc = || -> Result<EvmRpc> {
            match trade::EvmNetwork::of(chain).filter(|_| SITE_CHAINS.contains(&chain)) {
                Some(n) => Ok(n.rpc.clone()),
                None => refused(format!("4901|No RPC for chain {chain}.")),
            }
        };
        let hex = |s: &EvmSignature| format!("0x{}{}{:02x}", to_hex(&s.r), to_hex(&s.s), s.v);
        match method {
            "eth_chainId" => Ok(json!(format!("0x{chain:x}")).to_string()),
            "net_version" => Ok(json!(chain.to_string()).to_string()),
            "eth_accounts" => Ok(json!(if connected { vec![address] } else { vec![] }).to_string()),
            "eth_requestAccounts" => {
                if !connected {
                    let lines = [self.say("It sees your EVM address, which is also your Hyperliquid address, and can ask you to sign. Nothing is signed without your Confirm.",
                                          "Сайт увидит EVM-адрес — он же твой адрес на Hyperliquid — и сможет просить подпись. Без твоего «Подтвердить» ничего не подписывается."),
                                 self.say("ihatecrypto never signs token spending permissions (Permit).", "ihatecrypto никогда не подписывает разрешения на трату токенов (Permit)."),
                                 address.clone()];
                    let asked = self.say(&format!("{host} asks to connect to your EVM wallet"), &format!("{host} просит подключить твой EVM-кошелёк"));
                    if !self.ask(host, origin, site, true, &asked, &lines, vec![], vec![], None, None).await { return refused("4001|The user declined.") }
                    self.st_mut().sites.evm.insert(origin.into());
                }
                Ok(json!([address]).to_string())
            }
            "wallet_switchEthereumChain" => {
                let asked = args.first().and_then(|a| a.get("chainId")).and_then(Value::as_str)
                    .and_then(|id| u64::from_str_radix(&id.chars().skip(2).collect::<String>(), 16).ok())
                    .filter(|n| SITE_CHAINS.contains(n) && trade::EvmNetwork::of(*n).is_some());
                let Some(n) = asked else { return refused("4902|ihatecrypto works with Ethereum, Arbitrum and Base.") };
                self.st_mut().sites.chain.insert(origin.into(), n);
                Ok("null".into())
            }
            "personal_sign" => {
                let Some(raw) = args.first().and_then(Value::as_str).filter(|_| connected) else { return refused("4100|Connect the site first.") };
                let message = if raw.starts_with("0x") { from_hex(raw).unwrap_or_else(|| raw.as_bytes().to_vec()) } else { raw.as_bytes().to_vec() };
                if personal::is_blind_hash(&message) { return refused("4001|The site asks to sign a hash, not text: ihatecrypto doesn't sign that.") }
                let lines = [String::from_utf8(message.clone()).unwrap_or_else(|_| format!("{} bytes", message.len())),
                             self.say("Signing text doesn't move money. The signer refuses it if it's a hidden transaction.",
                                      "Подпись текста не двигает деньги. Подписант откажет, если это скрытая транзакция.")];
                let asked = self.say(&format!("{host} asks to sign a message"), &format!("{host} просит подписать сообщение"));
                if !self.ask(host, origin, site, false, &asked, &lines, vec![], vec![], None, None).await { return refused("4001|The user declined.") }
                let reply = self.signer.send(&SignerRequest::SignPersonal { message }).await?;
                let Some(s) = reply.signature.as_ref().filter(|_| reply.ok) else { return refused(format!("4001|{}", reason(&reply))) };
                Ok(json!(hex(s)).to_string())
            }
            "eth_signTypedData_v4" => {
                if !connected || args.len() < 2 { return refused("4100|Connect the site first.") }
                let body = match &args[1] { Value::String(s) => s.as_bytes().to_vec(), other => serde_json::to_vec(other).map_err(|e| WalletError(e.to_string()))? };
                let typed = TypedData::parse(&body).map_err(|e| WalletError(format!("4200|{}", e.0)))?;
                let owner = self.st().evm_address();
                if let Some(refusal) = typed.refusal_for(owner.as_deref()) {
                    self.reply(&format!("Site {host} asked for a “{}” signature. Declined — {refusal}", typed.primary_type), Some("browser".into()));
                    return refused(format!("4001|{refusal}"));
                }
                let p = &typed.primary_type;
                let lines: Vec<String> = std::iter::once(format!("{p} for “{}”, chain {}", typed.domain_name, typed.chain_id))
                    .chain(typed.fields.iter().map(|f| format!("{}: {}", f.0, f.2)))
                    .chain([self.say("The signer computes what it signs from this same data, and refuses token permissions.",
                                     "Подписант считает подпись из этих же данных и отказывает в разрешениях на токены.")]).collect();
                let asked = self.say(&format!("{host} asks to sign “{p}”"), &format!("{host} просит подписать «{p}»"));
                if !self.ask(host, origin, site, false, &asked, &lines, vec![], vec![], None, None).await { return refused("4001|The user declined.") }
                let reply = self.signer.send(&SignerRequest::SignTypedData { json: body }).await?;
                let Some(s) = reply.signature.as_ref().filter(|_| reply.ok) else { return refused(format!("4001|{}", reason(&reply))) };
                Ok(json!(hex(s)).to_string())
            }
            "eth_sign" => refused("4200|eth_sign signs a hash blindly — ihatecrypto doesn't support it."),
            "eth_signTransaction" => refused("4200|ihatecrypto signs EVM transactions only when it also sends them (eth_sendTransaction)."),
            "eth_sendTransaction" => {
                let Some(p) = args.first().and_then(Value::as_object).filter(|_| connected) else { return refused("4100|Connect the site first.") };
                let Some(to) = p.get("to").and_then(Value::as_str).filter(|t| from_hex(t).is_some_and(|d| d.len() == 20)) else {
                    return refused("4200|ihatecrypto doesn't sign contract creation.");
                };
                if p.get("from").and_then(Value::as_str).map_or_else(|| address.to_lowercase(), str::to_lowercase) != address.to_lowercase() {
                    return refused("4100|The sender isn't your address.");
                }
                let data = from_hex(p.get("data").or_else(|| p.get("input")).and_then(Value::as_str).unwrap_or("0x")).unwrap_or_default();
                let Some(value) = Self::quantity(p.get("value")) else { return refused("4200|The amount is larger than ihatecrypto will sign.") };
                let rpc = rpc()?;
                let review = self.evm_review(to, value, &data, &address, &rpc, host, title).await;
                if !review.problems.is_empty() {
                    self.pin_kind("result", &self.say(&format!("{host}: EVM transaction refused"), &format!("{host}: EVM-транзакция отклонена")),
                                  review.problems.iter().chain(&review.lines).cloned().collect(), None, None, None);
                    self.reply(&format!("Site {host}: EVM transaction declined — {}", review.problems.join(" ")), Some("browser".into()));
                    return refused(format!("4001|{}", review.problems.first().map_or("Declined.", String::as_str)));
                }
                let name = trade::EvmNetwork::of(chain).map(|n| n.name);
                let asked = self.say(&format!("{host} asks to send a transaction on {}", name.map_or_else(|| format!("chain {chain}"), str::to_string)),
                                     &format!("{host} просит отправить транзакцию в {}", name.map_or_else(|| format!("сети {chain}"), str::to_string)));
                let balance = self.say("Balance: ", "Баланс: ");
                let lines: Vec<String> = review.lines.iter().cloned().chain(review.changes.iter().map(|c| balance.clone() + c)).collect();
                let note = self.say("Simulated on the latest block, before the network fee. A token names itself — check its address. The signer decodes the call again.",
                                    "Симуляция на последнем блоке, без комиссии сети. Токен называет себя сам — проверь его адрес. Подписант разберёт вызов ещё раз.");
                if !self.ask(host, origin, site, false, &asked, &lines, review.warnings.clone(), review.ticks.clone(), None, Some(note)).await {
                    return refused("4001|The user declined.");
                }
                let nonce = rpc.nonce(&address).await?;
                let (max_priority_fee, max_fee) = rpc.fees().await?;
                let reply = self.signer.send(&SignerRequest::SignEvmTransaction {
                    chain_id: chain, nonce, max_priority_fee, max_fee, gas_limit: review.gas, to: to.into(), value, data, recipients: review.recipients.clone(),
                }).await?;
                let Some(raw) = reply.signed_raw.clone().filter(|_| reply.ok) else { return refused(format!("4001|{}", reason(&reply))) };
                match rpc.send(&raw).await {
                    Ok(hash) => {
                        self.reply(&format!("Site {host}: EVM transaction sent on chain {chain} — {}…", prefix(&hash, 14)), Some("browser".into()));
                        Ok(json!(hash).to_string())
                    }
                    Err(e) => refused(format!("-32000|The network rejected the transaction: {}", e.0)),
                }
            }
            m if READ_ONLY.contains(&m) => {
                // The JSON-RPC body is made here and only bytes cross to the network.
                let body = json!({"jsonrpc": "2.0", "id": 1, "method": m, "params": args});
                let answer = net::HTTP.post(rpc()?.url).header("Content-Type", "application/json").body(body.to_string()).timeout(Duration::from_secs(20))
                    .send().await.map_err(|e| WalletError(e.to_string()))?;
                let data = answer.bytes().await.map_err(|e| WalletError(e.to_string()))?;
                // JSONSerialization's: an object or a list, never a bare value.
                let j = match serde_json::from_slice::<Value>(&data) {
                    Ok(Value::Object(j)) => j,
                    Ok(Value::Array(_)) => return refused("4900|The RPC didn't respond."),
                    _ => return Err(WalletError(UNREADABLE.into())),
                };
                if let Some(e) = j.get("error").and_then(Value::as_object) {
                    let message = e.get("message").map_or_else(|| "RPC error".into(), |m| m.as_str().map_or_else(|| m.to_string(), str::to_string));
                    return refused(format!("-32000|{message}"));
                }
                Ok(j.get("result").unwrap_or(&Value::Null).to_string())
            }
            _ => refused(format!("4200|ihatecrypto doesn't support {method}.")),
        }
    }

    /// A JSON-RPC quantity («0x…») as u64; None when it does not fit. Absent is 0; a number is read as NSNumber's
    /// uint64Value reads it.
    pub fn quantity(v: Option<&Value>) -> Option<u64> {
        match v {
            None => Some(0),
            Some(Value::String(s)) if s.starts_with("0x") => if s == "0x" { Some(0) } else { u64::from_str_radix(&s[2..], 16).ok() },
            Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_i64().map(|i| i as u64)).or_else(|| n.as_f64().map(|f| f as u64)),
            Some(Value::Bool(b)) => Some(u64::from(*b)),
            _ => None,
        }
    }

    /// Decodes the call, names the token, simulates our balances and estimates gas, as the Mac does. Problems mean «don't
    /// sign».
    #[allow(clippy::too_many_arguments)]
    pub async fn evm_review(&self, to: &str, value: u64, data: &[u8], from: &str, rpc: &EvmRpc, host: &str, title: &str) -> SiteReview {
        let call = Call::decode(data);
        let mut problems: Vec<String> = [call.refusal(from, Some(rpc.chain_id)), Call::target_refusal(to, data, from)].into_iter().flatten().collect();
        let mut payees = call.payees(to, value, rpc.chain_id);
        if let Some(target) = call.unread_target(to, value, rpc.chain_id) { payees.push(target) }
        let recipients: Vec<String> = payees.iter().map(|p| p.to_lowercase()).filter(|p| *p != from.to_lowercase())
            .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        let native = trade::EvmNetwork::of(rpc.chain_id).map_or("coins", |n| n.gas_coin);
        let token = async |address: &str| -> (String, i64) {
            let symbol = rpc.read(address, &[0x95, 0xd8, 0x9b, 0x41]).await.ok().and_then(|d| evm::abi::string(&d)).unwrap_or_else(|| format!("token {}…", prefix(address, 8)));
            let decimals = rpc.read(address, &[0x31, 0x3c, 0xe5, 0x67]).await.ok().and_then(|d| evm::abi::uint_in(&d, 0)).unwrap_or(18);
            (symbol, i64::try_from(decimals).unwrap_or(i64::MAX))
        };
        let mut lines = vec![self.call_line(&call, to, value, from, rpc.chain_id, native, &token).await];
        if !recipients.is_empty() {
            lines.push(self.say("Money or the right to spend it goes to: ", "Деньги или право их тратить уходят: ") + &recipients.join(", "));
        }
        let mut changes: Vec<String> = vec![];
        let simulator = SIMULATION_RPC.iter().find(|(c, _)| *c == rpc.chain_id).map_or_else(|| rpc.clone(), |(c, url)| EvmRpc::pinned(url, *c));
        match simulator.simulate(from, &[(to.to_string(), value, data.to_vec())]).await {
            Ok(sim) => {
                if let Some(revert) = &sim.revert { problems.push(format!("Simulation: the call would revert — {revert}")) }
                for (asset, delta) in &sim.changes {
                    if asset == evm::simulation::NATIVE {
                        changes.insert(0, format!("{native}: {}", signed_units(*delta, 18)));
                        continue;
                    }
                    let (symbol, decimals) = token(asset).await;
                    let nft = sim.nfts.contains(asset);
                    let name = format!("{symbol}{} ({}…{})", if nft { " NFT" } else { "" }, prefix(asset, 6), suffix(asset, 4));
                    changes.push(format!("{name}: {}", signed_units(*delta, if nft { 0 } else { decimals })));
                }
                if sim.revert.is_none() && changes.is_empty() { changes.push(self.say("None of your balances change.", "Твои балансы не меняются.")) }
            }
            Err(e) => {
                changes.push(format!("Unavailable: {}", e.0));
                let tx = json!({"from": from, "to": to, "value": format!("0x{value:x}"), "data": format!("0x{}", to_hex(data))});
                if let Err(e) = rpc.call("eth_call", json!([tx, "latest"])).await { problems.push(format!("Simulation: the call would revert — {}", e.0)) }
            }
        }
        let mut gas: u64 = 0;
        if problems.is_empty() {
            match rpc.estimate_gas(from, to, value, data, None).await {
                Ok(estimate) => {
                    gas = estimate.saturating_mul(12) / 10;
                    if let Ok((_, max_fee)) = rpc.fees().await && let Some(fee) = foundation::units(&(u128::from(gas) * u128::from(max_fee)).to_string(), 18) {
                        lines.push(self.say("Network fee up to ", "Комиссия сети до ") + &format!("{} {native}", Fmt::num(fee, 8)));
                    }
                }
                Err(e) => problems.push(format!("Couldn't estimate gas: {}", e.0)),
            }
        }
        let unread = matches!(call, Call::Contract { .. });
        let mut ticks: Vec<SiteTick> = Engine::payee_tick(to, &recipients, unread, |en, ru| self.say(en, ru)).into_iter().collect();
        let seen: Vec<String> = lines.iter().chain(&changes).cloned().collect();
        self.site_second_check(host, title, &[format!("EVM {}: {to}", rpc.chain_id)], &seen, &mut lines, &mut ticks).await;
        SiteReview { lines, changes, warnings: vec![], problems, recipients, gas, ticks, ..SiteReview::default() }
    }

    /// What the call does, each token by its symbol and decimals (the Mac's evmReview lines), not raw units.
    #[allow(clippy::too_many_arguments)]
    pub async fn call_line(&self, call: &Call, to: &str, value: u64, from: &str, chain_id: u64, native: &str, token: impl AsyncFn(&str) -> (String, i64)) -> String {
        let amount = |word: &[u8], decimals: i64| -> String {
            if word.iter().all(|&b| b == 0xff) { return self.say("unlimited", "без лимита") }
            let n = evm::word_value(word);
            foundation::units(&n.to_string(), decimals).map_or_else(|| n.to_string(), |d| Fmt::num(d, 6))
        };
        let big = |n: &evm::U256, decimals: i64| foundation::units(&n.to_string(), decimals).map_or_else(|| n.to_string(), |d| Fmt::num(d, 6));
        let coins = |wei: u64| Fmt::num(Decimal::from(wei) / Decimal::from(1_000_000_000_000_000_000u64), 8);
        let whose = |a: &str| if a.to_lowercase() == from.to_lowercase() { "this wallet".to_string() } else { a.to_string() };
        match call {
            Call::Plain => format!("Transfer {} {native} to {to}.", coins(value)),
            Call::Approve { spender, amount: a } | Call::IncreaseAllowance { spender, amount: a } => {
                let (symbol, decimals) = token(to).await;
                format!("Allow {spender} to spend up to {} {symbol}.", amount(a, decimals))
            }
            Call::Transfer { to: r, amount: a } => {
                let (symbol, decimals) = token(to).await;
                format!("Transfer {} {symbol} to {r}.", amount(a, decimals))
            }
            Call::NftTransfer { from: f, to: r } => format!("Move NFTs of {to} from {} to {r}.", whose(f)),
            Call::Delegation { to: d, granted } => if *granted { format!("Let {d} borrow against or manage your lending positions.") }
                                                   else { format!("Revoke {d}'s rights over your lending positions.") },
            Call::TransferFrom { from: f, to: r, amount: a } => {
                let (symbol, decimals) = token(to).await;
                format!("Move {} {symbol} from {f} to {r}.", amount(a, decimals))
            }
            Call::SetApprovalForAll { operator, approved } => format!("{} for {operator}.", if *approved { "Rights to all NFTs" } else { "Revoke NFT rights" }),
            Call::Permit => "Call to permit (permission to spend).".into(),
            Call::Swap { recipient } => format!("Uniswap swap paying {}.", whose(recipient)),
            // Not on the Windows branch's EVMCall: main's Mac browser line.
            Call::KyberSwap(k) => {
                let (symbol, decimals) = token(&k.src_token).await;
                let fee = if k.fee_receivers.is_empty() { String::new() } else { format!(", a fee to {}", k.fee_receivers.join(", ")) };
                format!("KyberSwap swap of {} {symbol} paying {}{fee}.", big(&k.amount, decimals), whose(&k.dst_receiver))
            }
            Call::Borrow { asset, amount: a, on_behalf_of } => {
                let (symbol, decimals) = token(asset).await;
                format!("Borrow {} {symbol} from {to}, the debt on {}.", amount(a, decimals), whose(on_behalf_of))
            }
            Call::Repay { asset, amount: a, on_behalf_of } => {
                let (symbol, decimals) = token(asset).await;
                let how = if a.iter().all(|&b| b == 0xff) { "all the".to_string() } else { amount(a, decimals) };
                format!("Repay {how} {symbol} debt of {} to {to}.", whose(on_behalf_of))
            }
            Call::MultisigExec { .. } => format!("Carry out a signed move of the multisig {to}: {}.", call.summary(chain_id, to, value)),
            Call::CctpBurn { recipient, domain, amount: a } => format!("Circle's CCTP: burn {} USDC here to mint on {} to {}.", amount(a, 6), cctp::chain_name(*domain),
                                                                       recipient.as_deref().map_or_else(|| "a non-EVM account".into(), whose)),
            Call::OftSend { recipient, eid, amount: a, plain, .. } => {
                let known = usdt0::on(chain_id).is_some_and(|d| d.oft == to.to_lowercase());
                format!("LayerZero: send {} to {} on {}{}.", if known { format!("{} USDT", amount(a, 6)) } else { format!("tokens of {to}") },
                        recipient.as_deref().map_or_else(|| "an account that can't be read".into(), whose), usdt0::network_name(*eid),
                        if *plain { "" } else { ", with options or a message of its own" })
            }
            Call::V4Swap(s) => {
                let ((a, ad), (b, bd)) = (token(s.currency_in()).await, token(s.currency_out()).await);
                format!("Uniswap v4 swap: {} {a} for at least {} {b}, paid to this wallet.", big(&s.amount_in, ad), big(&s.amount_out_minimum, bd))
            }
            Call::Permit2Approve { token: contract, spender, amount: a, expiration } => {
                let (symbol, decimals) = token(contract).await;
                format!("Allow {spender} to spend up to {} {symbol} through Permit2, until {}.", amount(a, decimals), Fmt::date(foundation::unix(*expiration as f64)))
            }
            Call::Vault { receiver, owner } => format!("Vault {} crediting {}.", if owner.is_none() { "deposit" } else { "withdrawal" }, whose(receiver)),
            Call::Lending { beneficiary } => format!("Lending pool call (Aave supply or withdraw) crediting {}.", whose(beneficiary)),
            Call::Claim { recipients } => format!("Merkl rewards claim paying {}.", if recipients.iter().all(|u| u.to_lowercase() == from.to_lowercase()) {
                "this wallet".to_string() } else { recipients.join(", ") }),
            Call::Wrap => format!("Wrap {} {native} into its token at {to}{}.", coins(value),
                                  if Call::Wrap.payees(to, value, chain_id).is_empty() { ", the network's own WETH" } else { " — not this network's WETH" }),
            Call::Unwrap { amount: a } => format!("Unwrap {} of the token at {to} into {native}.", amount(a, 18)),
            Call::Contract { selector } => format!("Contract call {to}, function {selector}{}.", if value > 0 { format!(", with {} {native}", coins(value)) } else { String::new() }),
        }
    }

    /// Programs, the guard's verdict and the simulated change of every balance of ours the transaction touches.
    pub async fn solana_review(&self, wire: &[u8], owner: &str, host: &str, title: &str) -> Result<SiteReview> {
        let rpc = SolanaRpc::new();
        let message = solana::message::Message::parse_transaction(wire)?;
        let accounts = rpc.resolve_accounts(&message).await?;
        let programs: std::collections::BTreeSet<String> = message.instructions.iter().filter_map(|i| accounts.get(i.program_index).cloned()).collect();
        let known = guard::sol::known_programs();
        let unknown: Vec<String> = programs.iter().filter(|p| !known.contains_key(*p)).cloned().collect();
        let mut allowed = known.clone();
        for p in &unknown { allowed.insert(p.clone(), format!("unknown program {}…", prefix(p, 6))); }
        let tokens = self.token_accounts(owner, &rpc).await?;
        let touched: BTreeMap<String, (String, u64, i64)> = tokens.into_iter().filter(|(a, _)| accounts.contains(a)).collect();
        let owners: HashMap<String, String> = touched.keys().map(|a| (a.clone(), owner.to_string())).collect();
        let policy = guard::sol::Policy { programs: allowed, trust_lookup_tables: false, ..guard::sol::Policy::new(owner) };
        let verdict = guard::sol::check(&message, &accounts, &policy, &owners);
        let sol_before = rpc.balance(owner).await?;
        let watch: Vec<String> = std::iter::once(owner.to_string()).chain(touched.keys().cloned()).collect();
        let simulation = rpc.simulate(wire, &watch, false).await?;
        let mut problems = verdict.problems.clone();
        let mut deltas: Vec<(String, String, Decimal)> = vec![];
        if let Some(error) = &simulation.error {
            problems.push(format!("Simulation failed: {error}."));
        } else {
            let sol_after = simulation.after.get(owner).map_or(sol_before, |s| s.0);
            let sol = foundation::units(&(i128::from(sol_after) - i128::from(sol_before)).to_string(), 9).unwrap_or_default();
            deltas.push(("SOL".into(), SpotAsset::all()[0].mint.clone(), sol));
            for (address, (mint, amount, decimals)) in &touched {
                let after = simulation.after.get(address).and_then(|s| s.1).unwrap_or(*amount);
                let delta = foundation::units(&(i128::from(after) - i128::from(*amount)).to_string(), *decimals).unwrap_or_default();
                if !delta.is_zero() {
                    let symbol = if *mint == SpotAsset::usdc().mint { "USDC".to_string() }
                                 else { SpotAsset::all().iter().find(|a| a.mint == *mint).map_or_else(|| format!("token {}…", prefix(mint, 6)), |a| a.symbol.clone()) };
                    deltas.push((symbol, mint.clone(), delta));
                }
            }
        }
        let changes: Vec<String> = deltas.iter().map(|d| format!("{}: {}", d.0, signed(d.2))).collect();
        let program_names: Vec<(String, bool)> = programs.iter()
            .map(|p| known.get(p).map_or_else(|| (format!("{}…{}", prefix(p, 4), suffix(p, 4)), false), |n| (n.clone(), true))).collect();
        let not_listed = self.say(" (not on ihatecrypto's list)", " (нет в списке ihatecrypto)");
        let named: Vec<String> = program_names.iter().map(|(n, k)| if *k { n.clone() } else { n.clone() + &not_listed }).collect();
        let mut lines = vec![Self::calls_line(&program_names, |en, ru| self.say(en, ru))];
        let mut seen = HashSet::new();
        let checks: Vec<String> = verdict.checks.iter().filter(|c| !c.starts_with("Calls unknown program") && seen.insert((*c).clone())).cloned().collect();
        let mut ticks: Vec<SiteTick> = Engine::programs_tick(&unknown, |en, ru| self.say(en, ru)).into_iter().collect();
        self.site_second_check(host, title, &named, &changes, &mut lines, &mut ticks).await;
        let names: Vec<String> = program_names.iter().map(|p| p.0.clone()).collect();
        let lead = Self::solana_lead(&deltas, &names, |en, ru| self.say(en, ru));
        let sol_price = self.st().markets.get("SOL").map(|m| m.mark_px);
        let balances = deltas.iter().map(|(symbol, mint, amount)| {
            let fee = Self::fee_like(symbol, *amount, deltas.len());
            let price = if stablecoins::is_dollar(symbol) { Some(Decimal::ONE) } else if symbol == "SOL" { sol_price } else { self.held_solana_price(mint) };
            SiteBalance {
                symbol: symbol.clone(), amount: signed(*amount), up: *amount > Decimal::ZERO, usd: price.and_then(|p| (Num::of(amount.abs()) * p).0).map(Fmt::usd),
                note: fee.then(|| self.say("network fee and rent for new accounts", "комиссия сети и залог за новые счета")),
            }
        }).collect();
        Ok(SiteReview { lines, changes, warnings: vec![], problems, accounts, token_owners: owners, unknown_programs: unknown, ticks, lead, balances, checks,
                        ..SiteReview::default() })
    }

    /// SOL that only pays the fee and rent beside a real move (the Mac's feeLike): said so, not read as a send.
    pub fn fee_like(symbol: &str, amount: Decimal, count: usize) -> bool {
        symbol == "SOL" && amount < Decimal::ZERO && -amount < Decimal::new(5, 2) && count > 1
    }

    /// «You send 5 USDC to Kamino Vault», «You send 5 USDC and get 0.02 SOL», «You get …», «Only the network fee» (the
    /// Mac's summary): what leaves and arrives, fee-sized SOL left out, and the protocol it goes to. Each delta is
    /// (symbol, mint, amount).
    pub fn solana_lead(deltas: &[(String, String, Decimal)], programs: &[String], say: impl Fn(&str, &str) -> String) -> Option<String> {
        let moved: Vec<&(String, String, Decimal)> = deltas.iter().filter(|d| !Self::fee_like(&d.0, d.2, deltas.len())).collect();
        let out: Vec<String> = moved.iter().filter(|d| d.2 < Decimal::ZERO).map(|d| format!("{} {}", Fmt::num(-d.2, 6), d.0)).collect();
        let into: Vec<String> = moved.iter().filter(|d| d.2 > Decimal::ZERO).map(|d| format!("{} {}", Fmt::num(d.2, 6), d.0)).collect();
        // Programs of one protocol («Kamino Vault», «Kamino Farms») are that protocol: the money goes to Kamino.
        let work: Vec<&String> = programs.iter().filter(|p| !SOLANA_PLUMBING.contains(&p.as_str()) && !p.contains('…')).collect();
        let brands: HashSet<&str> = work.iter().map(|p| p.split(' ').find(|w| !w.is_empty()).unwrap_or(p)).collect();
        let target: Option<String> = if work.len() > 1 && brands.len() == 1 { brands.iter().next().map(|b| b.to_string()) } else { work.first().map(|p| p.to_string()) };
        let list = |items: &[String]| -> String {
            match items.split_last() {
                Some((last, rest)) if !rest.is_empty() => rest.join(", ") + &say(" and ", " и ") + last,
                _ => items.concat(),
            }
        };
        match (out.is_empty(), into.is_empty()) {
            (false, true) => Some(say(&format!("You send {}", list(&out)), &format!("Отправляешь {}", list(&out)))
                                  + &target.map_or_else(String::new, |t| say(&format!(" to {t}"), &format!(" в {t}")))),
            (false, false) => Some(say(&format!("You send {} and get {}", list(&out), list(&into)), &format!("Отдаёшь {}, получаешь {}", list(&out), list(&into)))),
            (true, false) => Some(say(&format!("You get {}", list(&into)), &format!("Получаешь {}", list(&into)))),
            (true, true) => (!deltas.is_empty()).then(|| say("Only the network fee", "Только комиссия сети")),
        }
    }

    /// «It calls: Kamino Vault · 7xKX…sAsU (not on ihatecrypto's list) · and Solana's own: System, Compute Budget». Each
    /// program is (name, known).
    pub fn calls_line(programs: &[(String, bool)], say: impl Fn(&str, &str) -> String) -> String {
        let plumbing = |n: &str| SOLANA_PLUMBING.contains(&n);
        let mut parts: Vec<String> = programs.iter().filter(|p| !plumbing(&p.0))
            .map(|p| if p.1 { p.0.clone() } else { p.0.clone() + &say(" (not on ihatecrypto's list)", " (нет в списке ihatecrypto)") }).collect();
        let own: Vec<&str> = programs.iter().map(|p| p.0.as_str()).filter(|n| plumbing(n)).collect();
        if !own.is_empty() { parts.push(say("and Solana's own: ", "и служебные Solana: ") + &own.join(", ")) }
        say("It calls: ", "Вызывает: ") + &parts.join(" · ")
    }

    /// A held Solana token's price, from the portfolio's row for its mint: the ≈ $ beside a site's balance change.
    pub fn held_solana_price(&self, mint: &str) -> Option<Decimal> {
        let st = self.st();
        let row = st.portfolio.networks.iter().find(|n| n.key == "solana")?.rows.iter().find(|r| r.contract.as_deref() == Some(mint))?;
        if row.value <= Decimal::ZERO { return None }
        row.usd?.checked_div(row.value)
    }

    /// The second check on a site's transaction (the Mac's jevNote): its concern as a tick, and when it didn't answer or
    /// isn't connected the card says so — the code checks decide.
    async fn site_second_check(&self, host: &str, title: &str, programs: &[String], changes: &[String], lines: &mut Vec<String>, ticks: &mut Vec<SiteTick>) {
        let Some(key) = self.jev_key().filter(|_| self.second_check()) else {
            lines.push(self.say("The second check isn't connected (Settings › AI model › Second check). The code checks decide.",
                                "Вторая проверка не подключена (Настройки › Модель ИИ › Вторая проверка). Решают проверки кода."));
            return;
        };
        match jev::reviewer::site(host, title, programs, changes, &JevPost(key)).await {
            Ok(r) => if r.concern { ticks.push(Engine::jev_tick(&r.text, |en, ru| self.say(en, ru))) },
            Err(_) => lines.push(self.say("The second check didn't answer — the code checks decide.", "Вторая проверка не ответила — решают проверки кода.")),
        }
    }

    /// Our token accounts, classic SPL and Token-2022 alike: address → mint, raw amount, decimals.
    async fn token_accounts(&self, owner: &str, rpc: &SolanaRpc) -> Result<BTreeMap<String, (String, u64, i64)>> {
        let mut items: Vec<Value> = vec![];
        for program in [solana::spl::TOKEN_PROGRAM, solana::spl::TOKEN_2022_PROGRAM] {
            let r = rpc.call("getTokenAccountsByOwner", json!([owner, {"programId": program}, {"encoding": "jsonParsed", "commitment": "confirmed"}])).await?;
            items.extend(r.get("value").and_then(Value::as_array).cloned().unwrap_or_default());
        }
        let mut out = BTreeMap::new();
        for item in &items {
            let Some(address) = item.get("pubkey").and_then(Value::as_str) else { continue };
            let Some(info) = item.pointer("/account/data/parsed/info").filter(|i| i.is_object()) else { continue };
            let (Some(mint), Some(amount)) = (info.get("mint").and_then(Value::as_str), info.get("tokenAmount").filter(|a| a.is_object())) else { continue };
            let raw = amount.get("amount").and_then(Value::as_str).unwrap_or("0").parse::<u64>().unwrap_or(0);
            let decimals = amount.get("decimals").and_then(|d| d.as_i64().or_else(|| d.as_f64().map(|f| f as i64))).unwrap_or(0);
            out.insert(address.to_string(), (mint.to_string(), raw, decimals));
        }
        Ok(out)
    }

    /// What the browser's bar shows for the page open in it: whether this run connected it, how ihatecrypto knows the
    /// site («known», «lookalike» of `real`, «unknown», «local»), and whether MetaMask's list names it as phishing.
    /// Strings, as every answer: «true» or «false».
    pub async fn site_state(&self, origin: &str) -> HashMap<String, String> {
        let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
        let Some((_, host)) = scheme_host(origin) else { return HashMap::from([pair("connected", "false"), pair("check", "unknown"), pair("blocked", "false")]) };
        let connected = {
            let st = self.st();
            st.sites.solana.contains(origin) || st.sites.evm.contains(origin)
        };
        let blocked = PhishingList::shared().contains(&host).await;
        let mut out = HashMap::from([pair("connected", &connected.to_string()), pair("blocked", &blocked.to_string())]);
        match site_check::check(&host) {
            Verdict::Known => { out.insert("check".into(), "known".into()); }
            Verdict::Lookalike(real) => {
                out.insert("check".into(), "lookalike".into());
                out.insert("real".into(), real);
            }
            Verdict::Unknown => { out.insert("check".into(), "unknown".into()); }
            Verdict::Local => { out.insert("check".into(), "local".into()); }
        }
        out
    }
}

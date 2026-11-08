//! Top up (TopUp.swift), as the Mac's ReceiveBoard and TopUpPage: what this wallet holds of the chosen coin on the chosen
//! network (read from the network itself, for «now → after»), whether dollars there would be stuck without the gas
//! coin, and — once a way is used — the balance watched every 4 s until money lands. «From a browser wallet» is a page
//! this PC serves to its own browser on 127.0.0.1, so the recipient comes from here and no website can swap it.
//! Here: the networks Top up offers, the order, what's shown and watched (`TopUpPage`), the page's server on 127.0.0.1
//! and its HTML, and the Solana transfers the page asks to sign.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::{Rc, Weak};
use std::time::Duration;

use evm::abi::u256_in;
use evm::erc20;
use evm::protocols::Permit3009;
use ihatecrypto_core::foundation::{decimal, shifted, text as text_of, units};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::modules::ModuleNetwork;
use ihatecrypto_core::top_up::{Coin, Network, TopUpOrder};
use net::solana_rpc::SolanaRpc;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::{Map, Value, json};
use solana::{Instruction, compile_message, spl, unsigned_transaction};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use trade::EvmNetwork;
use wallet::WalletError;
use wallet::address_check::{self, Network as Checked};

use crate::engine::Engine;
use crate::json;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopUpView {
    pub networks: Vec<TopUpNet>,
    pub network: String,
    pub coin: String,
    /// The coin at this wallet there, by symbol (USDC and the gas coin for «any coin»); empty until the network answers.
    #[serde(with = "json::numbers")]
    pub held: HashMap<String, Decimal>,
    /// The network's gas coin when the wallet holds less of it than a move there needs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_short: Option<String>,
    /// A way was used (a QR or an address shown, the page opened): the balance is watched.
    pub waiting: bool,
    /// «Landed on Solana: +20 USDC, 5 → 25».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrived: Option<String>,
    /// What the browser page said it sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent: Option<TopUpSent>,
}

/// `TopUpView.Sent`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopUpSent {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// `TopUpView.Net`: a network the wallet receives on, for the window's chips and its pay links (TopUpOrder.payLink).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopUpNet {
    pub key: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    pub usdc: String,
    pub usdc_symbol: String,
    pub usdc_decimals: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    pub recipient: String,
    /// Dollars there move by signature, Relay paying the gas (Permit3009): the gas line says so.
    pub by_signature: bool,
}

/// Solana's system program.
pub const SYSTEM: &str = "11111111111111111111111111111111";

/// A request to the top-up page: its method, path, lowercased headers and body.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl PageRequest {
    /// One HTTP/1.1 request from what was read; None while it isn't whole yet (its head, then `Content-Length` bytes).
    pub fn parse(data: &[u8]) -> Option<PageRequest> {
        let end = data.windows(4).position(|w| w == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&data[..end]);
        let mut lines = head.split("\r\n");
        let first: Vec<&str> = lines.next().unwrap_or("").split(' ').filter(|s| !s.is_empty()).collect();
        if first.len() < 2 { return None }
        let mut headers = HashMap::new();
        for line in lines {
            let Some((name, value)) = line.split_once(':') else { continue };
            headers.insert(name.to_lowercase(), value.trim_matches([' ', '\t']).to_string());
        }
        let body = &data[end + 4..];
        let wanted = headers.get("content-length").map_or(Some(0), |l| l.parse::<usize>().ok()).unwrap_or(0);
        if body.len() < wanted { return None }
        Some(PageRequest { method: first[0].into(), path: first[1].split('?').next().unwrap_or("").into(), headers, body: body.to_vec() })
    }
}

/// The page's answer: status, type and body, with the headers that keep it from being framed or cached.
pub fn response(status: u16, kind: &str, body: &[u8]) -> Vec<u8> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        if status == 200 { "OK" } else { "Error" }, body.len());
    [head.as_bytes(), body].concat()
}

/// `TopUpOrder.Coin`'s raw value: «usdc», «native», «any».
pub fn coin_raw(c: Coin) -> &'static str {
    match c { Coin::Usdc => "usdc", Coin::Native => "native", Coin::Any => "any" }
}

/// `TopUpOrder.Coin(rawValue:)`.
pub fn coin_named(raw: &str) -> Option<Coin> { [Coin::Usdc, Coin::Native, Coin::Any].into_iter().find(|c| coin_raw(*c) == raw) }

/// `TopUpOrder.Network(EVMNetwork)`: an EVM network as Top up offers it.
pub fn evm_network(n: &EvmNetwork) -> Network {
    Network {
        key: n.key.into(), title: n.name.into(), chain_id: Some(n.rpc.chain_id), usdc: n.usdc.into(), usdc_symbol: evm::dollar(n.usdc).unwrap_or("USDC").into(),
        usdc_decimals: n.usdc_decimals, coin: n.fee_token.is_none().then(|| n.gas_coin.into()), explorer: n.explorer.into(), rpc: n.rpc.url.clone(),
        // A USDC transfer on Ethereum: ≈ 1¢ at 0.07 gwei (2026-10-15), ≈ 15¢ at 0.74 (2026-09-29); the rest stay under a cent.
        cost: if n.key == "ethereum" { "fee 1–20¢ · lands in ~15 s" } else { "fee under 1¢ · lands in seconds" }.into(),
    }
}

/// `trimmingCharacters(in: .whitespaces)`: spaces and tabs, not line breaks.
fn trim_spaces(s: &str) -> &str { s.trim_matches(|c: char| c == '\t' || (c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'))) }

impl Engine {
    /// Solana, then the EVM networks as a search for money takes them; a network off in Settings › Modules isn't offered.
    pub fn top_up_networks(&self) -> Vec<Network> {
        let (solana, evm) = { let st = self.st(); (st.solana_address(), st.evm_address()) };
        let solana = solana.is_some_and(|a| address_check::problem(&a, Checked::Solana).is_none()) && trade::modules().is_on(ModuleNetwork::Solana);
        let evm = evm.is_some_and(|a| address_check::problem(&a, Checked::Base).is_none());
        let mut out = if solana { vec![Network::solana()] } else { vec![] };
        if evm { out.extend(EvmNetwork::searched().into_iter().map(evm_network)) }
        out
    }

    pub fn top_up_order(&self, network: Option<&str>, coin: Option<&str>, amount: Option<&str>) -> Result<TopUpOrder, WalletError> {
        let nets = self.top_up_networks();
        let Some(net) = nets.iter().find(|n| Some(n.key.as_str()) == network).or(nets.first()).cloned() else {
            return Err(WalletError(self.say("Add a wallet first: Settings › Wallet.", "Сначала добавь кошелёк: Настройки › Кошелёк.")));
        };
        let typed = amount.unwrap_or("").replace(',', ".");
        let typed = trim_spaces(&typed);
        let recipient = { let st = self.st(); if net.chain_id.is_none() { st.solana_address() } else { st.evm_address() } }.unwrap_or_default();
        let coin = coin_named(coin.unwrap_or("")).unwrap_or(Coin::Usdc);
        Ok(TopUpOrder::new(net, coin, if typed.is_empty() { None } else { Some(decimal(typed).unwrap_or_default()) }, &recipient))
    }

    /// The page shows `network`/`coin`; `watch` once a way is used. The same three again change nothing.
    pub fn top_up(&self, network: Option<&str>, coin: Option<&str>, watch: bool) -> Result<(), WalletError> {
        let o = self.top_up_order(network, coin, None)?;
        TopUpPage::show(o, watch, self);
        Ok(())
    }

    /// The browser-wallet way: a page for this order at a new address; a page opened before stops working.
    pub async fn top_up_open(&self, network: Option<&str>, coin: Option<&str>, amount: Option<&str>) -> Result<String, WalletError> {
        let o = self.top_up_order(network, coin, amount)?;
        if o.units().is_none() {
            return Err(WalletError(self.say("Pick a coin and an amount above zero first.", "Сначала выбери монету и сумму больше нуля.")));
        }
        let url = TopUpPage::open(o.clone(), self)?;
        TopUpPage::show(o, true, self);
        Ok(url)
    }

    pub fn top_up_stop(&self) { TopUpPage::stop() }
}

/// What Top up shows and watches, and the page it serves: one per engine (Swift's `TopUpPage.shared` on the MainActor —
/// here the engine's one thread).
#[derive(Default)]
pub struct TopUpPage {
    view: Option<TopUpView>,
    engine: Weak<Engine>,
    /// What's on screen and watched.
    shown: Option<(TopUpOrder, bool)>,
    watching: Option<JoinHandle<()>>,
    /// The page being served: its order and its one-time path.
    server: Option<PageServer>,
    order: Option<TopUpOrder>,
    secret: String,
}

thread_local! {
    static PAGE: RefCell<TopUpPage> = RefCell::default();
}

impl TopUpPage {
    fn with<R>(f: impl FnOnce(&mut TopUpPage) -> R) -> R { PAGE.with(|p| f(&mut p.borrow_mut())) }

    fn engine() -> Option<Rc<Engine>> { Self::with(|p| p.engine.upgrade()) }

    /// What Top up shows now (the snapshot's `topUp`).
    pub fn view() -> Option<TopUpView> { Self::with(|p| p.view.clone()) }

    pub fn show(o: TopUpOrder, watch: bool, engine: &Engine) {
        let shown = Self::with(|p| {
            p.engine = Rc::downgrade(&engine.rc());
            p.shown.as_ref().is_some_and(|(s, w)| s.network == o.network && s.coin == o.coin && (*w || !watch))
        });
        if shown { return }
        let (solana, evm) = { let st = engine.st(); (st.solana_address(), st.evm_address()) };
        let nets: Vec<TopUpNet> = engine.top_up_networks().into_iter().map(|n| TopUpNet {
            recipient: if n.chain_id.is_none() { solana.clone() } else { evm.clone() }.unwrap_or_default(),
            by_signature: n.chain_id.is_some_and(|id| Permit3009::supports(id, &n.usdc)),
            key: n.key, title: n.title, chain_id: n.chain_id, usdc: n.usdc, usdc_symbol: n.usdc_symbol, usdc_decimals: i64::from(n.usdc_decimals), coin: n.coin,
        }).collect();
        Self::with(|p| {
            let old = p.view.take().filter(|v| v.network == o.network.key && v.coin == coin_raw(o.coin));
            p.view = Some(TopUpView {
                networks: nets, network: o.network.key.clone(), coin: coin_raw(o.coin).into(), held: old.as_ref().map(|v| v.held.clone()).unwrap_or_default(),
                gas_short: old.as_ref().and_then(|v| v.gas_short.clone()), waiting: watch, arrived: None, sent: old.and_then(|v| v.sent),
            });
            p.shown = Some((o.clone(), watch));
            if let Some(t) = p.watching.take() { t.abort() }
            p.watching = Some(tokio::task::spawn_local(Self::watch(o, watch)));
        });
        engine.changed();
    }

    pub fn stop() {
        let engine = Self::with(|p| {
            if let Some(t) = p.watching.take() { t.abort() }
            if let Some(s) = p.server.take() { s.stop() }
            (p.order, p.secret, p.shown, p.view) = (None, String::new(), None, None);
            p.engine.upgrade()
        });
        if let Some(e) = engine { e.changed() }
    }

    /// Reads the order's balance (shown as now → after) and its gas; once a way is used, again every 4 s for up to 20 min.
    /// When it rises, a line says what landed and the whole portfolio is read again (which says in the chat what came in).
    /// Cancelled (`abort`) at its next wait.
    async fn watch(o: TopUpOrder, waiting: bool) {
        let (before, short) = tokio::join!(Self::balances(&o), Self::gas_short(&o));
        let engine = Self::with(|p| {
            if let Some(v) = p.view.as_mut() { (v.held, v.gas_short) = (before.clone(), short) }
            p.engine.upgrade()
        });
        if let Some(e) = engine { e.changed() }
        if !waiting || before.is_empty() { return }
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_secs(4)).await;
            let now = Self::balances(&o).await;
            let mut up: Vec<(&String, Decimal, Decimal)> = now.iter().filter_map(|(s, v)| before.get(s).filter(|was| v > *was).map(|was| (s, *was, *v))).collect();
            if up.is_empty() { continue }
            // ponytail: sorted by symbol; Swift's dictionary order is its hash's.
            up.sort_by(|a, b| a.0.cmp(b.0));
            let digits = |s: &str| if s == o.network.usdc_symbol { 2 } else { 6 };
            let lines = up.iter().map(|(s, was, after)| {
                format!("+{} {s}, {} → {}", Fmt::num(after.saturating_sub(*was), digits(s)), Fmt::num(*was, digits(s)), Fmt::num(*after, digits(s)))
            }).collect::<Vec<_>>().join(" · ");
            let Some(engine) = Self::engine() else { return };
            let said = engine.say(&format!("Landed on {}: ", o.network.title), &format!("Пришло в {}: ", o.network.title)) + &lines;
            Self::with(|p| if let Some(v) = p.view.as_mut() { (v.held, v.arrived) = (now, Some(said)) });
            engine.changed();
            engine.refresh().await;
            return;
        }
    }

    // MARK: Reading the network

    /// The order's coin at its recipient, by symbol, from the network: USDC and the gas coin for «any coin». Empty when
    /// the network didn't answer.
    pub async fn balances(o: &TopUpOrder) -> HashMap<String, Decimal> {
        let net = &o.network;
        let coins = if o.coin == Coin::Any { vec![Coin::Usdc, Coin::Native] } else { vec![o.coin] };
        let mut out = HashMap::new();
        for coin in coins.into_iter().filter(|c| *c == Coin::Usdc || net.coin.is_some()) {
            let one = TopUpOrder::new(net.clone(), coin, None, &o.recipient);
            // The smallest units, as digits.
            let raw: Option<String> = if net.chain_id.is_none() {
                let rpc = SolanaRpc::new();
                if coin == Coin::Usdc { rpc.token_balance(&o.recipient, &net.usdc).await.ok() } else { rpc.balance(&o.recipient).await.ok() }.map(|u| u.to_string())
            } else if let Some(n) = EvmNetwork::named(&net.key) {
                if coin == Coin::Usdc {
                    match erc20::balance_of(&o.recipient) {
                        Ok(call) => n.rpc.read(&net.usdc, &call).await.ok().and_then(|d| u256_in(&d, 0)).map(|u| u.to_string()),
                        Err(_) => None,
                    }
                } else {
                    n.rpc.balance(&o.recipient).await.ok().map(|w| w.to_string())
                }
            } else {
                None
            };
            if let Some(amount) = raw.and_then(|r| units(&r, i64::from(one.decimals()))) { out.insert(one.symbol(), amount); }
        }
        out
    }

    /// The network's gas coin when this wallet holds less of it than a move needs there (None: enough, or no gas coin).
    pub async fn gas_short(o: &TopUpOrder) -> Option<String> {
        let coin = o.network.coin.clone()?;
        let floor = if o.network.chain_id.is_none() { shifted(Decimal::from(TopUpOrder::SOLANA_FEE_FLOOR), 9) }
            else { EvmNetwork::named(&o.network.key).and_then(|n| shifted(Decimal::from(n.gas_floor), 18)) }.unwrap_or_default();
        let held = *Self::balances(&TopUpOrder::new(o.network.clone(), Coin::Native, None, &o.recipient)).await.get(&coin)?;
        (held < floor).then_some(coin)
    }

    // MARK: The page

    /// Serves a page for `o` at a new random path and returns its address; the path before it stops working.
    pub fn open(o: TopUpOrder, engine: &Engine) -> Result<String, WalletError> {
        let started = Self::with(|p| p.server.is_some());
        let server = if started { None } else { Some(PageServer::start(Self::handle)?) };
        let mut secret = [0u8; 16];
        getrandom::fill(&mut secret).map_err(|_| WalletError("The page couldn't start.".into()))?;
        Ok(Self::with(|p| {
            p.engine = Rc::downgrade(&engine.rc());
            if let Some(s) = server { p.server = Some(s) }
            p.order = Some(o);
            if let Some(v) = p.view.as_mut() { v.sent = None }
            p.secret = secret.iter().map(|b| format!("{b:02x}")).collect();
            format!("http://127.0.0.1:{}/{}", p.server.as_ref().map_or(0, |s| s.port), p.secret)
        }))
    }

    /// Only this page, as the Mac's: its own address in Host (a name pointed at 127.0.0.1 shows another, so a site can't
    /// rebind to it), its own origin on a POST (no other site can post here), the order's path, which changes with each
    /// page, and the recipient always the order's.
    pub async fn handle(r: PageRequest) -> Vec<u8> {
        let text = |status: u16, s: &str| response(status, "text/plain; charset=utf-8", s.as_bytes());
        let json = |value: Value| response(200, "application/json", value.to_string().as_bytes());
        const CLOSED: &str = "This page has closed: open it again from ihatecrypto.";
        let (order, port, secret) = Self::with(|p| (p.order.clone(), p.server.as_ref().map(|s| s.port), p.secret.clone()));
        let (Some(order), Some(port)) = (order, port) else { return text(403, CLOSED) };
        if secret.is_empty() || !Self::allows(&r, port) { return text(403, CLOSED) }
        let root = format!("/{secret}");
        let body: Option<Map<String, Value>> = serde_json::from_slice(&r.body).ok();
        let field = |k: &str| body.as_ref().and_then(|b| b.get(k)).and_then(Value::as_str).map(str::to_string);
        let solana_account = |a: &str| address_check::problem(a, Checked::Solana).is_none();
        match (r.method.as_str(), r.path.as_str()) {
            ("GET", p) if p == root => response(200, "text/html; charset=utf-8", Self::page(&order).as_bytes()),
            ("POST", p) if p == format!("{root}/solana") => {
                let Some(from) = field("from").filter(|f| order.network.chain_id.is_none() && solana_account(f)) else { return text(400, "No Solana account") };
                // The page's amount («Send more» picks a new one); the recipient is always this order's.
                let Some(units) = field("units").and_then(|u| u.parse::<u64>().ok()).filter(|u| *u > 0) else { return text(400, "No amount to send") };
                match Self::solana_transfer(&order, units, &from).await {
                    Ok(wire) => json(json!({"transaction": wallet::b64::serialize(&wire, serde_json::value::Serializer).unwrap_or_default()})),
                    Err(e) => text(502, &e.0),
                }
            }
            ("POST", p) if p == format!("{root}/sent") => {
                let Some(id) = field("id").filter(|id| transfer_id(id)) else { return text(400, "No transfer id") };
                let engine = Self::with(|p| {
                    if let Some(v) = p.view.as_mut().filter(|v| v.network == order.network.key) { v.sent = Some(TopUpSent { link: Some(order.explorer(&id)), id }) }
                    p.engine.upgrade()
                });
                if let Some(e) = engine { e.changed() }
                json(json!({}))
            }
            ("POST", p) if p == format!("{root}/balance") => {
                // The order's coin at the sending account and at this wallet, for «Send more»'s now → after.
                let Some(from) = field("from").filter(|f| if order.network.chain_id.is_none() { solana_account(f) } else { evm_address(f) }) else {
                    return text(400, "No account");
                };
                let theirs = TopUpOrder::new(order.network.clone(), order.coin, None, &from);
                let (mine, theirs) = tokio::join!(Self::balances(&order), Self::balances(&theirs));
                let symbol = order.symbol();
                let mut out = Map::new();
                for (k, v) in [("to", mine.get(&symbol)), ("from", theirs.get(&symbol))] {
                    if let Some(v) = v { out.insert(k.into(), Value::String(text_of(*v))); }
                }
                json(Value::Object(out))
            }
            _ => text(404, "Not here"),
        }
    }

    /// The request comes from this page itself: its own address in Host, and on a POST its own origin.
    pub fn allows(r: &PageRequest, port: u16) -> bool {
        let me = format!("127.0.0.1:{port}");
        r.headers.get("host") == Some(&me) && (r.method == "GET" || r.headers.get("origin") == Some(&format!("http://{me}")))
    }

    /// The transfer as the connected wallet will sign it: `from` pays the fee, USDC goes by TransferChecked into this
    /// wallet's account (made if missing), SOL by a system transfer. USDC to a wallet with almost no SOL brings
    /// `TopUpOrder::SOLANA_FEE_TOP_UP` along in the same transaction: USDC alone can't move on Solana.
    pub async fn solana_transfer(order: &TopUpOrder, units: u64, from: &str) -> Result<Vec<u8>, WalletError> {
        let rpc = SolanaRpc::new();
        let (blockhash, _) = rpc.latest_blockhash().await?;
        if order.coin == Coin::Native {
            let transfer = Instruction {
                program: SYSTEM.into(), accounts: vec![(from.into(), true, true), (order.recipient.clone(), false, true)], data: system_transfer(units),
            };
            return Ok(unsigned_transaction(&compile_message(&[transfer], from, &blockhash, 20_000)?));
        }
        if let Ok(sol) = rpc.balance(&order.recipient).await && sol < TopUpOrder::SOLANA_FEE_FLOOR {
            return Ok(unsigned_transaction(&Self::usdc_with_fees(order, units, from, &blockhash)?));
        }
        Ok(unsigned_transaction(&spl::transfer_message(from, &order.recipient, &order.network.usdc, units, 6, &blockhash, 20_000, spl::TOKEN_PROGRAM, false)?))
    }

    /// USDC into this wallet's account (made if missing) and `TopUpOrder::SOLANA_FEE_TOP_UP` SOL to the wallet itself, in
    /// one message `from` signs and pays for.
    pub fn usdc_with_fees(order: &TopUpOrder, units: u64, from: &str, blockhash: &str) -> Result<Vec<u8>, WalletError> {
        let mint = order.network.usdc.as_str();
        let (mine, theirs) = (spl::associated_account(from, mint, spl::TOKEN_PROGRAM)?, spl::associated_account(&order.recipient, mint, spl::TOKEN_PROGRAM)?);
        let create = Instruction {
            program: spl::ASSOCIATED_TOKEN_PROGRAM.into(),
            accounts: vec![(from.into(), true, true), (theirs.clone(), false, true), (order.recipient.clone(), false, false), (mint.into(), false, false),
                           (SYSTEM.into(), false, false), (spl::TOKEN_PROGRAM.into(), false, false)],
            data: vec![1],
        };
        let usdc = Instruction {
            program: spl::TOKEN_PROGRAM.into(),
            accounts: vec![(mine, false, true), (mint.into(), false, false), (theirs, false, true), (from.into(), true, false)],
            data: [&[12][..], &units.to_le_bytes(), &[6]].concat(),
        };
        let fees = Instruction {
            program: SYSTEM.into(), accounts: vec![(from.into(), true, true), (order.recipient.clone(), false, true)],
            data: system_transfer(TopUpOrder::SOLANA_FEE_TOP_UP),
        };
        compile_message(&[create, usdc, fees], from, blockhash, 20_000)
    }

    /// The page: the order, the wallets this browser has (Wallet Standard for Solana, EIP-6963 for EVM), one button each.
    pub fn page(order: &TopUpOrder) -> String {
        let (start, end) = address_check::ends(&order.recipient);
        let net = &order.network;
        // An EVM wallet is switched to the network, or told about it first (wallet_addEthereumChain) when it doesn't know it.
        let chain = net.chain_id.map_or(json!({}), |id| json!({
            "chainId": format!("0x{id:x}"), "chainName": net.title, "rpcUrls": [net.rpc], "blockExplorerUrls": [net.explorer],
            "nativeCurrency": {"name": net.coin.as_deref().unwrap_or("USD"), "symbol": net.coin.as_deref().unwrap_or("USD"), "decimals": 18},
        }));
        let amount = order.amount.map(text_of).unwrap_or_default();
        let config = json!({
            "solana": net.chain_id.is_none(), "chain": chain, "recipient": order.recipient, "units": order.units().unwrap_or_else(|| "0".into()),
            "token": if net.chain_id.is_some() && order.coin == Coin::Usdc { net.usdc.as_str() } else { "" },
            "explorer": order.explorer(""), "amount": amount, "symbol": order.symbol(), "decimals": order.decimals(),
            "digits": if order.coin == Coin::Usdc { 2 } else { 6 },
        });
        // `<` escaped in the inline JSON: nothing in it can close the script.
        let config = config.to_string().replace('<', "\\u003c");
        PAGE_HTML
            .replace("{{amount}}", &amount)
            .replace("{{symbol}}", &order.symbol())
            .replace("{{network}}", &net.title)
            .replace("{{cost}}", &net.cost)
            .replace("{{start}}", &start).replace("{{end}}", &end)
            .replace("{{recipient}}", &order.recipient)
            .replace("{{wallets}}", if net.chain_id.is_none() { "Phantom, Solflare or Backpack" } else { "MetaMask, Rabby, Phantom or Coinbase Wallet" })
            .replace("{{config}}", &config)
    }
}

/// The system program's Transfer: its index 2 (u32), then the lamports (u64), little-endian.
fn system_transfer(lamports: u64) -> Vec<u8> { [&2u32.to_le_bytes()[..], &lamports.to_le_bytes()].concat() }

/// A transaction's id as a wallet gives it: an EVM hash or a Solana signature.
fn transfer_id(id: &str) -> bool {
    let evm = id.len() == 66 && id.starts_with("0x") && id[2..].bytes().all(|b| b.is_ascii_hexdigit());
    let solana = (64..=90).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() && !matches!(b, b'0' | b'O' | b'I' | b'l'));
    evm || solana
}

/// «0x» and 40 hex digits.
fn evm_address(a: &str) -> bool { a.len() == 42 && a.starts_with("0x") && a[2..].bytes().all(|b| b.is_ascii_hexdigit()) }

/// The page's server: a socket bound to 127.0.0.1 itself, so nothing on the network can connect; each connection is one
/// request read whole (64 KB at most, 5 s at most), answered and closed. It runs on the engine's thread (`spawn_local`).
pub struct PageServer {
    pub port: u16,
    task: JoinHandle<()>,
}

impl PageServer {
    pub fn start<F, Fut>(handle: F) -> Result<PageServer, WalletError>
    where F: Fn(PageRequest) -> Fut + 'static, Fut: Future<Output = Vec<u8>> + 'static {
        let failed = |_| WalletError("The page couldn't start.".into());
        // In its container the engine takes no connection from the person's browser: the window listens, reads each
        // request whole (64 KB, 5 s at most, as `serve`) and hands it over, and gives the browser this answer.
        if let Some(broker) = net::broker::get() {
            let (port, mut requests) = broker.serve(0).map_err(WalletError)?;
            let task = tokio::task::spawn_local(async move {
                while let Some((conn, data)) = requests.recv().await {
                    let reply = match PageRequest::parse(&data) { Some(r) => handle(r).await, None => response(400, "text/plain", b"Bad request") };
                    broker.reply(conn, &reply);
                }
            });
            return Ok(PageServer { port, task });
        }
        let socket = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(failed)?;
        socket.set_nonblocking(true).map_err(failed)?;
        let port = socket.local_addr().map_err(failed)?.port();
        let listening = tokio::net::TcpListener::from_std(socket).map_err(failed)?;
        // ponytail: one connection at a time; a page sends a few small requests, so they never queue long.
        let task = tokio::task::spawn_local(async move {
            while let Ok((c, _)) = listening.accept().await { Self::serve(c, &handle).await }
        });
        Ok(PageServer { port, task })
    }

    pub fn stop(&self) {
        self.task.abort();
        if let Some(broker) = net::broker::get() { broker.unserve(self.port) }
    }

    async fn serve<F, Fut>(mut c: TcpStream, handle: &F)
    where F: Fn(PageRequest) -> Fut, Fut: Future<Output = Vec<u8>> {
        let (mut data, mut buffer, mut request) = (Vec::new(), vec![0u8; 16_384], None);
        while request.is_none() && data.len() < 65_536 {
            let got = match tokio::time::timeout(Duration::from_secs(5), c.read(&mut buffer)).await { Ok(Ok(n)) if n > 0 => n, _ => return };
            data.extend_from_slice(&buffer[..got]);
            request = PageRequest::parse(&data);
        }
        let Some(request) = request else { return };
        let reply = handle(request).await;
        let _ = c.write_all(&reply).await;
    }
}

impl Drop for PageServer {
    fn drop(&mut self) { self.stop() }
}

/// The page's HTML: its {{…}} filled by `page`.
const PAGE_HTML: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Top up ihatecrypto</title>
<style>
:root{--bg:#0d0e11;--card:#18191d;--line:#2a2b31;--text:#f4f4f5;--dim:#9a9ba3;--accent:#ff7a2f;--ok:#4ade80;--bad:#f87171}
@media (prefers-color-scheme:light){:root{--bg:#f3f3f1;--card:#fff;--line:#e4e4e1;--text:#141417;--dim:#6b6c72;--ok:#16a34a;--bad:#dc2626}}
*{box-sizing:border-box}body{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--text);font:15px/1.45 "Segoe UI Variable","Segoe UI",system-ui,sans-serif;padding:24px 16px}
main{width:min(440px,100%);background:var(--card);border:1px solid var(--line);border-radius:26px;padding:26px}
header{display:flex;align-items:center;gap:10px;font-weight:700;font-size:16px}header i{width:30px;height:30px;border-radius:50%;background:var(--accent);display:inline-block}
.amount{font-size:38px;font-weight:800;letter-spacing:-.02em;margin:22px 0 2px}.dim{color:var(--dim)}
.to{margin:16px 0;padding:12px 14px;border-radius:14px;background:var(--bg);font-size:13px}
.to b{font:600 15px ui-monospace,Consolas,monospace}.to code{display:block;margin-top:4px;font-size:11px;color:var(--dim);word-break:break-all}
h2{font-size:11px;letter-spacing:.12em;text-transform:uppercase;color:var(--dim);margin:20px 0 8px;font-weight:600}
button{all:unset;box-sizing:border-box;display:flex;align-items:center;gap:12px;width:100%;padding:12px 14px;margin-bottom:8px;border:1.5px solid var(--line);border-radius:14px;cursor:pointer;font-weight:600}
button:hover:not(:disabled){border-color:var(--accent)}button:disabled{opacity:.45;cursor:default}button img{width:28px;height:28px;border-radius:7px}
button span{flex:1}button i{font-style:normal;color:var(--dim)}
#status{min-height:22px;font-size:14px;margin-top:6px}#status.ok{color:var(--ok)}#status.bad{color:var(--bad)}#status a{color:inherit}
footer{margin-top:18px;font-size:12px;color:var(--dim)}
[hidden]{display:none!important}#more{margin-top:14px}.bal{margin-bottom:12px;padding:10px 14px;border-radius:14px;background:var(--bg);font-size:13px}
.bal div{display:flex;justify-content:space-between;gap:12px;padding:2px 0}.bal b{font-weight:600;text-align:right}
.field{display:flex;align-items:baseline;gap:8px;margin-bottom:10px;padding-bottom:4px;border-bottom:1.5px solid var(--line);font-size:30px;font-weight:800}
.field:focus-within{border-color:var(--accent)}.field input{all:unset;flex:1;min-width:0}
#warn{color:var(--bad);font-size:13px;min-height:18px}
button.go{justify-content:center;background:var(--accent);border-color:var(--accent);color:#141417}
#cancel{display:block;text-align:center;font-size:13px;color:var(--dim);cursor:pointer}
</style></head><body><main>
<header><i></i>Top up ihatecrypto</header>
<div class="amount">{{amount}} {{symbol}}</div><div class="dim">on {{network}} · {{cost}}</div>
<div class="to">To your ihatecrypto wallet <b>{{start}} … {{end}}</b><code>{{recipient}}</code></div>
<h2>Top up from</h2><div id="wallets"></div>
<div id="none" class="dim" hidden>No wallet found in this browser. Install {{wallets}}, or scan the QR in ihatecrypto with your phone.</div>
<div id="status"></div>
<section id="more" hidden>
  <div class="bal"><div><span>Your ihatecrypto wallet</span><b id="toBal">…</b></div><div><span id="fromName"></span><b id="fromBal">…</b></div></div>
  <button id="again"><span>Send more</span><i>+</i></button>
  <form id="form" hidden>
    <label class="field"><input id="amount" inputmode="decimal" autocomplete="off" aria-label="Amount"><span>{{symbol}}</span></label>
    <div id="warn"></div>
    <button class="go" id="go" type="submit"></button>
    <a id="cancel">Cancel</a>
  </form>
</section>
<footer>This page comes from ihatecrypto on this PC, not from a website: the address above can't be swapped. Your wallet shows the transfer before you approve it.</footer>
</main><script>
const C = {{config}};
const list = document.getElementById('wallets'), statusLine = document.getElementById('status'), seen = new Set();
const say = (html, kind) => { statusLine.innerHTML = html; statusLine.className = kind || ''; };
const post = async (what, body) => {
  const r = await fetch(location.pathname + '/' + what, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(await r.text());
  return r.json();
};
const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
const base58 = bytes => {
  let n = 0n, s = '';
  for (const b of bytes) n = n * 256n + BigInt(b);
  while (n > 0n) { s = B58[Number(n % 58n)] + s; n /= 58n; }
  for (const b of bytes) { if (b) break; s = '1' + s; }
  return s;
};
// «12.5» → the coin's smallest units; 0n for nothing, a typo or more digits than the coin has.
const toUnits = s => {
  const m = /^\s*(\d*)(?:[.,](\d*))?\s*$/.exec(s);
  if (!m || !(m[1] || m[2]) || (m[2] || '').length > C.decimals) return 0n;
  return BigInt((m[1] || '0') + (m[2] || '').padEnd(C.decimals, '0'));
};
const coins = u => Number(u) / 10 ** C.decimals;
const fmt = v => v == null || isNaN(v) ? '…' : v.toLocaleString('en-US', { maximumFractionDigits: C.digits }) + ' ' + C.symbol;
const $ = id => document.getElementById(id);
// The wallet that sent last ({ name, pay, from }), which «Send more» sends from again; balances as last read.
let units = BigInt(C.units), last = null, bal = {};
function render() {
  const n = $('form').hidden ? 0 : coins(toUnits($('amount').value));
  const line = (now, after) => fmt(now) + (n && now != null && after >= 0 ? ' → ' + fmt(after) : '');
  $('toBal').textContent = line(bal.to, bal.to + n);
  $('fromBal').textContent = line(bal.from, bal.from - n);
  const short = n > 0 && bal.from != null && n > bal.from;
  $('warn').textContent = short ? last.name + ' has only ' + fmt(bal.from) : '';
  $('go').textContent = n ? 'Confirm · send ' + fmt(n) : 'Type an amount';
  $('go').disabled = !n || short;
}
async function balances() {
  if (!last) return;
  const b = await post('balance', { from: last.from }).catch(() => null);
  // A side the network didn't answer keeps its last value.
  if (b) { bal = { to: b.to == null ? bal.to : Number(b.to), from: b.from == null ? bal.from : Number(b.from) }; render(); }
}
const escape = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
async function send(name, pay) {
  for (const x of document.querySelectorAll('button')) x.disabled = true;
  try {
    say('Connecting ' + escape(name) + '…');
    const { id, from } = await pay(units, () => say('Approve the transfer in ' + escape(name) + '…'));
    // Sent is sent: a closed page (the card was closed meanwhile) only means ihatecrypto learns it from the balance.
    await post('sent', { id }).catch(() => {});
    last = { name, pay, from };
    say('Sent ' + fmt(coins(units)) + ' ✓ ihatecrypto shows it the moment it lands. You can close this tab. <a target="_blank" rel="noreferrer" href="' + escape(C.explorer + id) + '">See it ↗</a>', 'ok');
    $('fromName').textContent = name;
    $('more').hidden = false;
    $('form').hidden = true;
    $('again').hidden = $('again').disabled = false;
    render();
    balances();
  } catch (e) {
    const no = e && (e.code === 4001 || /reject|denied|cancel|declined/i.test(e.message || ''));
    say(no ? 'Cancelled in the wallet. Nothing was sent.' : 'It didn’t go through: ' + escape((e && e.message) || e), 'bad');
    for (const x of document.querySelectorAll('button')) x.disabled = false;
    render();
  }
}
// «Send more»: the amount picked here, its balances now → after, Confirm, then the wallet asks again.
$('again').onclick = () => {
  $('again').hidden = true;
  $('form').hidden = false;
  $('amount').value = C.amount;
  $('amount').focus();
  $('amount').select();
  render();
  balances();
};
$('cancel').onclick = () => { $('form').hidden = true; $('again').hidden = false; render(); };
$('amount').oninput = render;
$('form').onsubmit = e => {
  e.preventDefault();
  units = toUnits($('amount').value);
  if (units > 0n && last) send(last.name, last.pay);
};
// What lands shows here too while the tab is open.
setInterval(() => { if (!document.hidden) balances(); }, 5000);
function wallet(name, icon, pay) {
  if (seen.has(name)) return;
  seen.add(name);
  $('none').hidden = true;
  const b = document.createElement('button');
  b.innerHTML = (icon ? '<img alt="">' : '') + '<span></span><i>→</i>';
  if (icon) b.querySelector('img').src = icon;
  b.querySelector('span').textContent = name;
  b.onclick = () => send(name, pay);
  list.append(b);
}
if (C.solana) {
  // Wallet Standard: wallets register when the page says it's ready, or announce themselves when they load later.
  const api = { register: (...ws) => { ws.forEach(solana); return () => {}; } };
  window.addEventListener('wallet-standard:register-wallet', e => e.detail(api));
  window.dispatchEvent(new CustomEvent('wallet-standard:app-ready', { detail: api }));
  function solana(w) {
    const f = w.features || {};
    if (!f['standard:connect'] || !f['solana:signAndSendTransaction'] || !(w.chains || []).includes('solana:mainnet')) return;
    wallet(w.name, w.icon, async (units, approve) => {
      const { accounts } = await f['standard:connect'].connect();
      const account = accounts[0] || w.accounts[0];
      const { transaction } = await post('solana', { from: account.address, units: units.toString() });
      approve();
      const bytes = Uint8Array.from(atob(transaction), c => c.charCodeAt(0));
      const [out] = await f['solana:signAndSendTransaction'].signAndSendTransaction({ account, chain: 'solana:mainnet', transaction: bytes });
      return { id: base58(out.signature), from: account.address };
    });
  }
} else {
  // EIP-6963: every EVM wallet announces itself; window.ethereum is the fallback for an older one.
  window.addEventListener('eip6963:announceProvider', e => evm(e.detail.info.name, e.detail.info.icon, e.detail.provider));
  window.dispatchEvent(new Event('eip6963:requestProvider'));
  setTimeout(() => { if (!seen.size && window.ethereum) evm('Browser wallet', '', window.ethereum); }, 800);
  function evm(name, icon, p) {
    wallet(name, icon, async (units, approve) => {
      const [from] = await p.request({ method: 'eth_requestAccounts' });
      try { await p.request({ method: 'wallet_switchEthereumChain', params: [{ chainId: C.chain.chainId }] }); }
      catch (e) {
        if (e.code !== 4902) throw e;
        await p.request({ method: 'wallet_addEthereumChain', params: [C.chain] });
      }
      approve();
      const amount = units.toString(16);
      const tx = C.token
        ? { from, to: C.token, data: '0xa9059cbb' + C.recipient.slice(2).toLowerCase().padStart(64, '0') + amount.padStart(64, '0') }
        : { from, to: C.recipient, value: '0x' + amount };
      return { id: await p.request({ method: 'eth_sendTransaction', params: [tx] }), from };
    });
  }
}
setTimeout(() => { if (!seen.size) $('none').hidden = false; }, 1500);
</script></body></html>"#;

#[cfg(test)]
#[path = "top_up/tests.rs"]
mod tests;

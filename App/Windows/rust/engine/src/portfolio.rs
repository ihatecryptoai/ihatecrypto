//! The balances the window shows (Portfolio.swift): a card per network, the coins on it with their dollar value, and the
//! total; every network read side by side (`refresh`, one read at a time, one asked meanwhile right after it), money
//! arriving said in the chat, the lending readings kept through a node that doesn't answer, balances.json, and the token
//! card's P/L line (`held_paid`).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::task::Poll;

use chrono::{Datelike, Local};
use defi::jupiter_lend::JupiterLend;
use defi::kamino::Kamino;
use evm::protocols::{aave, morpho};
use execution::trade_signer::TradeWallet;
use ihatecrypto_core::foundation::{Date, Num, Rounding, decimal, number, rounded, units};
use ihatecrypto_core::holdings::{self, Holding};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::{Hyperliquid, Near, Solana};
use ihatecrypto_core::plan::Market;
use ihatecrypto_core::pnl::CostBasis;
use ihatecrypto_core::stablecoins;
use net::evm_rpc::EvmRpc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::SignerWallet;
use tokio::sync::oneshot;
use trade::EvmNetwork;
use trade::executor::HyperliquidAccount;
use trade::morpho::MorphoRates;
use trade::stocks::{RobinhoodStocks, Token};

use crate::engine::{Engine, ReadWaiter};
use crate::notices::Notice;
use crate::{json, store};

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioView {
    pub networks: Vec<Network>,
    #[serde(with = "number")]
    pub total: Decimal,
    /// When the networks were last read: every read, whole or not — what the checks and the chat go by.
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub updated_at: Option<Date>,
    /// The balance on screen is whole (the Mac's balanceReady): this wallet's Solana and Hyperliquid and its Automation
    /// wallet have answered since launch or the switch. Until then the screen shows the last full balance, dimmed.
    pub ready: bool,
    pub errors: Vec<String>,
    /// The wallet's value over the last 7 days, oldest first, the last point now: the sparkline.
    pub equity: Vec<Point>,
    /// Its change over those days; None with fewer than three points, as the Mac draws no line then.
    #[serde(rename = "change7d", skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change7d: Option<Decimal>,
}

/// `PortfolioView.Row`: a coin on a network.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub symbol: String,
    pub amount: String,
    /// The amount as a number, for the «money arrived» check.
    #[serde(with = "number")]
    pub value: Decimal,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub usd: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Where its cost is kept (CostBasis): the mint on Solana, «chain:contract» elsewhere; None for dollars and perps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// «+$1.20 (+5.0%)» against what it cost, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pnl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub gain: Option<Decimal>,
    /// How far the price moved in 24 hours, in %, where its market says; None for dollars.
    #[serde(rename = "change24h", skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change24h: Option<Decimal>,
    /// A market list vouches for it (true), or nothing does (false: «unverified»); None for dollars and gas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    /// «tokens», or «defi» for money put in somewhere (Jupiter Lend, Kamino, Aave, Morpho).
    pub section: String,
    /// Under a deposit's name: «earning», «Allez USDC · 5.42%».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// A token found by listing the address (Holding.id): the person can hide it or show it again; None for the
    /// network's dollars, gas and deposits, which always show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// It looks real (Holding.looksReal): verified, or a real pool and a cent's worth. Shown unless the person hid it;
    /// the others are tucked until the person shows them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real: Option<bool>,
    /// One of Robinhood Chain's stock tokens: its Buy and Sell name the stock, not the contract (the Mac's tokenPhrases).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock: Option<bool>,
    /// A deposit managed on its own site (Jupiter's other platforms): a click opens it in the browser, as the Mac's
    /// opens it beside the chat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

impl Row {
    /// `Row(symbol:amount:value:)`, in the tokens section; the rest set after.
    pub fn new(symbol: &str, amount: &str, value: Decimal) -> Self {
        Row {
            symbol: symbol.into(), amount: amount.into(), value, usd: None, icon: None, key: None, contract: None, pnl: None, gain: None, change24h: None,
            verified: None, section: "tokens".into(), note: None, id: None, real: None, stock: None, site: None,
        }
    }
}

/// `PortfolioView.Network`: one network's card.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    pub key: String,
    pub name: String,
    #[serde(with = "number")]
    pub usd: Decimal,
    pub rows: Vec<Row>,
    /// What its money does, as the Mac's network card says it: «idle $757.59 · earning $2.51», «$0.01 · margin».
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Tokens tucked under «N hidden · show»: hidden by the person, or airdrops that look like scams. Out of `usd`.
    pub hidden: Vec<Row>,
    /// Tucked beyond the first hundred (a spammed address has hundreds): counted, not sent.
    pub hidden_more: i64,
}

/// `PortfolioView.Point`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Point {
    #[serde(with = "json::date")]
    pub at: Date,
    #[serde(with = "number")]
    pub usd: Decimal,
}

/// `Engine.Read`: one network's read — its card (None: nothing there), or why it failed (the network, the reason).
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    Success(Option<Network>),
    Failure(String, String),
}

/// What a Robinhood Stock Token is known by while the networks are read: its registry, read once (blocks.rs
/// robinhood_stock keeps it in `State::stock_registry`); None until then.
type Registry = Option<Vec<Token>>;

/// `[String: Decimal]` as Swift's JSONEncoder writes it: the dollars as numbers (balances.json).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
struct Usd(#[serde(with = "number")] Decimal);

fn sum(values: impl IntoIterator<Item = Decimal>) -> Decimal { values.into_iter().fold(Decimal::ZERO, Decimal::saturating_add) }

/// Every future at once, their outputs in order: Swift's task group read back by index.
async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut running: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut out: Vec<Option<F::Output>> = running.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (f, o) in running.iter_mut().zip(out.iter_mut()) {
            if o.is_none() {
                match f.as_mut().poll(cx) { Poll::Ready(v) => *o = Some(v), Poll::Pending => pending = true }
            }
        }
        if pending { Poll::Pending } else { Poll::Ready(()) }
    }).await;
    out.into_iter().map(|o| o.expect("each future ran to its end")).collect()
}

/// A Double from an API as Swift's `Decimal(Double)`: its shortest text.
fn from_double(x: f64) -> Option<Decimal> { if x.is_finite() { decimal(&format!("{x}")) } else { None } }

impl Engine {
    /// Every network read at once, then drawn together. A network that didn't answer keeps its last card. One read at a
    /// time; one asked for meanwhile (a wallet switch's, a move's) runs right after it instead of being dropped.
    /// Asked during a read, it returns once the next read has landed — one begun after the ask — as the Mac's
    /// `await ws.refresh()` does: the rail's spinner and a chain's «reads balances after this one» wait for it.
    pub async fn refresh(&self) {
        let waiting = {
            let mut st = self.st_mut();
            if st.refreshing {
                st.refresh_again = true;
                let (done, landed) = oneshot::channel();
                let pass = st.reads_started + 1;
                st.read_waiters.push(ReadWaiter { pass, done });
                Some(landed)
            } else {
                st.refreshing = true;
                None
            }
        };
        if let Some(landed) = waiting {
            let _ = landed.await;
            return;
        }
        // Swift's `defer { refreshing = false }`: also when the read is dropped half-way.
        struct Done<'a>(&'a Engine);
        impl Drop for Done<'_> {
            fn drop(&mut self) { self.0.st_mut().refreshing = false }
        }
        let _done = Done(self);
        loop {
            {
                let mut st = self.st_mut();
                st.refresh_again = false;
                st.reads_started += 1;
            }
            self.read_once().await;
            self.land_reads();
            if !self.st().refresh_again { break }
        }
    }

    /// The week's closes are read with the read that first brings an account's positions (the new wallet's after a
    /// switch) or sees them change, after the positions and their notices (the Mac's readFills, Workspace.swift:2119).
    pub fn reads_closed(read_for: Option<&str>, account: &str, before: &[String], now: &[String]) -> bool {
        read_for != Some(account) || before.iter().collect::<HashSet<_>>() != now.iter().collect::<HashSet<_>>()
    }

    /// A read landed: the callers waiting for it, or for an earlier one, go on.
    pub fn land_reads(&self) {
        let landed: Vec<ReadWaiter> = {
            let mut st = self.st_mut();
            let done = st.reads_started;
            let (landed, waiting) = std::mem::take(&mut st.read_waiters).into_iter().partition(|w| w.pass <= done);
            st.read_waiters = waiting;
            landed
        };
        for w in landed { let _ = w.done.send(()); }
    }

    /// Nothing the last wallet's reads left is the new one's (the Mac's walletChanged clears solana, hl, dexBalances, hlp,
    /// its orders and recentlyClosed): the balances, positions and orders, what each network has answered since it came
    /// on screen, the Solana tokens that price an unpriced read, Hyperliquid's details, the closed trades and the
    /// notices' memory of its positions.
    pub fn forget_wallet_reads(&self) {
        let mut st = self.st_mut();
        st.portfolio = PortfolioView::default();
        st.positions.clear();
        st.hl_orders.clear();
        st.positions_for = None;
        st.seen_solana_priced = None;
        st.seen_hl = false;
        st.solana_holdings.clear();
        st.place_facts.hyperliquid = None;
        st.recently_closed.clear();
        // The last wallet's limit orders (the Mac's switch sets limitOrderList to nil): `limit_views` stays, so the loop
        // reads this one's on its next tick.
        st.limit_orders = None;
        st.notice_positions = None;
        st.notice_entries.clear();
    }

    async fn read_once(&self) {
        let (me, near, last_holdings, registry) = {
            let st = self.st();
            (st.read_wallet(), st.near_address(), st.solana_holdings.clone(), st.stock_registry.clone())
        };
        let Some(me) = me else { return };
        let automation_wallet = self.automation_wallet();
        // Another wallet since the read began: nothing of it is this one's — not its balances, its positions or what the
        // guardian does with them — and the new wallet's own read follows (the Mac's same() / user == hlAddress).
        let same = || {
            let st = self.st();
            st.read_wallet().map(|w| w.solana + &w.evm) == Some(format!("{}{}", me.solana, me.evm)) && st.near_address() == near
        };
        // A wallet of one chain (a key, a watched address) has nothing to read on the other.
        let solana_read = async {
            if me.solana.is_empty() { (Read::Success(None), vec![], false) } else { Self::solana(&me.solana, &last_holdings).await }
        };
        let hl_read = async { if me.evm.is_empty() { (Read::Success(None), None) } else { Self::hyperliquid(&me.evm).await } };
        let evm_read = async { if me.evm.is_empty() { vec![] } else { Self::evm(&me.evm, registry).await } };
        let (automation, markets, (solana_part, holdings, unpriced), (hl_part, hl_facts), evm_parts, near_part) = tokio::join!(
            Self::automation_money(automation_wallet.clone()), Hyperliquid::markets(), solana_read, hl_read, evm_read, Self::near(near.as_deref()));
        if let Ok(m) = markets { self.st_mut().markets = m }
        if !same() { return }
        let solana_ok = matches!(solana_part, Read::Success(_));
        let hl_ok = matches!(hl_part, Read::Success(_));
        let (last, choices) = {
            let mut st = self.st_mut();
            // A read that failed keeps what the details board showed.
            if hl_facts.is_some() || me.evm.is_empty() { st.place_facts.hyperliquid = hl_facts }
            if solana_ok && !me.solana.is_empty() { st.solana_holdings = holdings }
            let last: HashMap<String, Network> = st.portfolio.networks.iter().map(|n| (n.key.clone(), n.clone())).collect();
            (last, st.settings.token_choices.clone().unwrap_or_default())
        };
        let (mut networks, mut errors) = (vec![], vec![]);
        for part in [solana_part, hl_part].into_iter().chain(evm_parts).chain([near_part]) {
            match part {
                Read::Success(Some(n)) => {
                    // Solana's and Hyperliquid's cards stay once their read answered, at $0 too, and a network whose tokens
                    // are all tucked keeps its card for «N hidden · show» (the Mac's PortfolioNetworks.present).
                    let n = Self::tucked(self.priced_network(n), &choices);
                    if !n.rows.is_empty() || !n.hidden.is_empty() || n.hidden_more > 0 || n.key == "solana" || n.key == "hyperliquid" {
                        networks.push(self.noted(n));
                    }
                }
                Read::Success(None) => {}
                Read::Failure(key, why) => {
                    errors.push(why);
                    if let Some(kept) = last.get(&key) { networks.push(kept.clone()) }
                }
            }
        }
        // Shares a contract call brought that the explorer doesn't list yet (the Mac's missingDeposits).
        if !me.evm.is_empty() { networks = self.with_missing_deposits(networks, &me.evm).await }
        if !same() { return }
        // Solana, then Hyperliquid, then the rest by their money, as on the Mac.
        let order = |key: &str| match key { "solana" => 0, "hyperliquid" => 1, _ => 2 };
        networks.sort_by(|a, b| order(&a.key).cmp(&order(&b.key)).then(b.usd.cmp(&a.usd)));
        let owner = format!("{}{}", me.solana, me.evm);
        let (next, same_wallet, old) = {
            let mut st = self.st_mut();
            // A read that failed keeps the last one: read as it came, the balance would drop by Automation's money.
            if automation_wallet.is_none() {
                st.automation_usd = None;
            } else if let Some((dollars, sol)) = automation {
                let px = st.markets.get("SOL").map_or(Decimal::ZERO, |m| m.mark_px);
                if let Some(usd) = (Num::of(dollars) + Num::of(sol) * px).0 { st.automation_usd = Some(usd) }
            }
            // The screen stays «loading» under this wallet's last full balance until Solana (priced), Hyperliquid and the
            // Automation wallet have each answered since it came on screen — never the sum of what came so far (the Mac's
            // balanceReady). Only the screen: the checks and the chat read each read as it came, as the Mac's do.
            if st.portfolio_owner.as_deref() != Some(&owner) {
                st.seen_solana_priced = None;
                st.seen_hl = false;
            }
            if solana_ok { st.seen_solana_priced = Some(!unpriced) }
            if hl_ok { st.seen_hl = true }
            let total = sum(networks.iter().map(|n| n.usd));
            let ready = Self::balance_ready(!me.solana.is_empty(), !me.evm.is_empty(), st.seen_solana_priced, st.seen_hl,
                                            automation_wallet.is_none() || st.automation_usd.is_some());
            let next = PortfolioView { networks, total, updated_at: Some(chrono::Utc::now()), ready, errors, ..Default::default() };
            let same_wallet = st.portfolio_owner.as_deref() == Some(&owner);
            (next, same_wallet, st.portfolio.clone())
        };
        // Another wallet since the last read: its balances aren't money that arrived.
        if old.updated_at.is_some() && same_wallet { self.announce_incoming(&old, &next) }
        let shown = self.with_profit(next.clone());
        {
            let mut st = self.st_mut();
            st.portfolio = shown;
            st.portfolio_owner = Some(owner);
        }
        let me_ = self.rc();
        tokio::task::spawn_local(async move { me_.load_costs().await });
        // The history and the 7-day line measure what the P/L does, the gas coins left out (the Mac's equity).
        if next.errors.is_empty() { self.record_equity(self.rule_equity().unwrap_or_default(), &Self::balance_key(&me)) }
        // The whole balance, gas coins and Automation's money in, for the next launch or switch (the Mac's rememberBalance).
        if next.ready {
            let automation_usd = self.st().automation_usd.unwrap_or_default();
            Self::remember_balance(next.total.saturating_add(automation_usd), &Self::balance_key(&me));
        }
        self.load_equity();
        if !me.evm.is_empty() {
            // A request that failed keeps what was read before: read as «none», a position would look closed by its stop.
            let before: Vec<String> = self.st().positions.iter().map(|p| p.coin.clone()).collect();
            let (read, orders) = tokio::join!(HyperliquidAccount::positions(&me.evm), HyperliquidAccount::open_orders(&me.evm));
            if !same() { return }
            let (read_for, now) = {
                let mut st = self.st_mut();
                let read_for = st.positions_for.clone();
                if let Ok(read) = read {
                    st.positions = read.into_iter().filter(|p| !p.size.is_zero()).collect();
                    st.positions_for = Some(me.evm.to_lowercase());
                }
                if let Ok(orders) = orders { st.hl_orders = orders }
                (read_for, st.positions.iter().map(|p| p.coin.clone()).collect::<Vec<_>>())
            };
            // Another wallet since the last read: its positions aren't this one's, closed.
            if same_wallet { self.note_closed_positions(&before).await }
            if Self::reads_closed(read_for.as_deref(), &me.evm.to_lowercase(), &before, &now) { self.read_closed().await }
            // The guardian runs after the read, in the 30 s loop (dispatch.rs), as the Mac's watch(): a stop waiting for
            // Windows Hello never holds a refresh, the rail's spinner or a chain's «reads balances after this one».
        }
        self.refresh_rates().await;
        self.refresh_favorites().await;
        self.check_alerts().await;
        self.notice_hyperliquid().await;
        self.read_transit().await;
        // Every network's gas coin is known now: dollars without it get their way out offered (once a network).
        self.offer_gas_ways();
        self.changed();
    }

    /// Dollars and SOL that came in since the last read, said in the chat, as the Mac does. Not after a move of the
    /// user's own (two minutes), whose change the chat already told.
    pub fn announce_incoming(&self, old: &PortfolioView, new: &PortfolioView) {
        {
            let st = self.st();
            if st.running.is_some() || (chrono::Utc::now() - st.last_own_move).num_milliseconds() <= 120_000 { return }
        }
        fn amounts(p: &PortfolioView) -> HashMap<(String, String), Decimal> {
            let mut out: HashMap<(String, String), Decimal> = HashMap::new();
            for n in &p.networks {
                for r in n.rows.iter().filter(|r| r.id.is_none() && ["USDC", "USDG", "SOL"].contains(&r.symbol.as_str())) {
                    let e = out.entry((n.name.clone(), r.symbol.clone())).or_default();
                    *e = e.saturating_add(r.value);
                }
            }
            out
        }
        let (before, after) = (amounts(old), amounts(new));
        let mut keys: Vec<&(String, String)> = after.keys().collect();
        keys.sort();
        let lines: Vec<String> = keys.iter().filter_map(|key| {
            let (name, symbol) = key;
            let (now, was) = (after[*key], before.get(*key).copied().unwrap_or_default());
            let sol = symbol == "SOL";
            let least = if sol { Decimal::new(5, 4) } else { Decimal::new(1, 2) };
            let came = now.checked_sub(was)?;
            if came < least || !(old.networks.iter().any(|n| &n.name == name) || was.is_zero()) { return None }
            Some(format!("+{} {symbol} {} {name}", Fmt::num(came, if sol { 4 } else { 2 }), self.say("on", "на")))
        }).collect();
        if lines.is_empty() { return }
        // What came in, in dollars, so the P/L counts it as money moved in, not as profit.
        let sol_px = self.st().markets.get("SOL").map_or(Decimal::ZERO, |m| m.mark_px);
        for key in keys {
            let (name, symbol) = key;
            let (now, was) = (after[key], before.get(key).copied().unwrap_or_default());
            if now <= was { continue }
            let came = Num::of(now) - was;
            let usd = if symbol == "SOL" { came * sol_px } else { came };
            if let Some(usd) = usd.0 { self.record_flow(usd, &format!("{symbol} in on {name}")) }
        }
        let text = self.say("💸 Received ", "💸 Пришло ") + &lines.join(", ") + ".";
        self.notify(Notice::Incoming, &self.say("Money arrived", "Пришли деньги"), &text, false, false, None, None);
        self.reply(&text, Some("incoming".into()));
    }

    /// Coins with no price of their own take Hyperliquid's mark for the same coin (SOL, ETH, HYPE…), and its day's move.
    fn priced_network(&self, mut n: Network) -> Network {
        let st = self.st();
        for r in &mut n.rows {
            if r.usd.is_none() && let Some(market) = st.markets.get(&r.symbol) {
                r.usd = (Num::of(market.mark_px) * r.value).0;
                r.change24h = r.change24h.or_else(|| market.change24h());
            }
        }
        n.usd = sum(n.rows.iter().map(|r| r.usd.unwrap_or_default()));
        n
    }

    pub fn noted(&self, mut n: Network) -> Network {
        let russian = self.st().russian();
        n.note = Self::note(&n, russian);
        if n.key == "robinhood" {
            // Robinhood's own registry tells a stock token from any other; read once, the rows marked from the next read on.
            let stocks: Option<HashSet<String>> = self.st().stock_registry.as_ref().map(|r| r.iter().map(|t| t.address.to_lowercase()).collect());
            if let Some(stocks) = stocks {
                // A stock's day off its Hyperliquid market (xyz:TSLA), as the Mac's stockRow; none there, none shown.
                let st = self.st();
                for r in &mut n.rows {
                    if r.contract.as_ref().is_some_and(|c| stocks.contains(&c.to_lowercase())) {
                        r.stock = Some(true);
                        r.change24h = Self::stock_day(&r.symbol, &st.markets);
                    }
                }
            } else {
                let me = self.rc();
                tokio::task::spawn_local(async move { me.robinhood_stock("").await; });
            }
        }
        // A deposit's «earning» in the window's language; a vault's own name and rate stay as they are.
        if russian {
            for r in &mut n.rows {
                if let Some(rest) = r.note.as_deref().and_then(|note| note.strip_prefix("earning")) { r.note = Some(format!("приносит доход{rest}")) }
            }
        }
        n
    }

    pub fn stock_day(symbol: &str, markets: &HashMap<String, Market>) -> Option<Decimal> {
        markets.get(&format!("xyz:{symbol}")).or_else(|| markets.get(&format!("xyz:{}", symbol.to_uppercase())))?.change24h()
    }

    /// The line under a network's name, as the Mac's NetworkCard writes it: Hyperliquid's margin, Tempo's purpose, else
    /// what's idle (dollars and the big stablecoins) and what's earning (deposits), the bigger first; None when neither.
    /// Swift's default `russian` is false.
    pub fn note(n: &Network, russian: bool) -> Option<String> {
        let usd = |r: &Row| r.usd.unwrap_or_default();
        match n.key.as_str() {
            "hyperliquid" => {
                let hlp = n.rows.iter().find(|r| r.symbol == "HLP").map_or(Decimal::ZERO, usd);
                let perps = n.rows.iter().find(|r| r.symbol == "Perps").map_or(Decimal::ZERO, usd);
                Some(format!("{} · {}{}", Fmt::usd(perps), if russian { "маржа" } else { "margin" },
                             if hlp > Decimal::ZERO { format!(" · HLP {}", Fmt::usd(hlp)) } else { String::new() }))
            }
            "tempo" => Some(if russian { "USDC для платных сервисов" } else { "USDC for paid services" }.into()),
            _ => {
                // Dollars carry no cost key (Engine.row): USDC, USDG and the genuine stablecoins.
                let idle = sum(n.rows.iter().filter(|r| r.section == "tokens" && r.key.is_none()).map(usd));
                let earning = sum(n.rows.iter().filter(|r| r.section == "defi").map(usd));
                let mut sums: Vec<(&str, Decimal)> = [(if russian { "в работе" } else { "earning" }, earning), (if russian { "свободно" } else { "idle" }, idle)]
                    .into_iter().filter(|s| s.1 >= Decimal::new(1, 2)).collect();
                sums.sort_by_key(|s| std::cmp::Reverse(s.1));
                (!sums.is_empty()).then(|| sums.iter().map(|(w, d)| format!("{w} {}", Fmt::usd(*d))).collect::<Vec<_>>().join(" · "))
            }
        }
    }

    /// Until this wallet's first full read: its last full balance — every network, gas coins and the Automation wallet —
    /// shown dimmed (the Mac's lastBalance, kept in balances.json).
    pub fn last_full_balance(&self) -> Option<Decimal> {
        let me = {
            let st = self.st();
            if st.portfolio.ready { return None }
            st.read_wallet()?
        };
        store::load::<HashMap<String, Usd>>("balances.json")?.get(&Self::balance_key(&me)).map(|u| u.0)
    }

    /// The address a wallet's balance and equity points are kept under: Solana's, else the EVM one.
    pub fn balance_key(me: &TradeWallet) -> String { if me.solana.is_empty() { me.evm.clone() } else { me.solana.clone() } }

    pub fn remember_balance(usd: Decimal, wallet: &str) {
        let mut all = store::load::<HashMap<String, Usd>>("balances.json").unwrap_or_default();
        if all.get(wallet) == Some(&Usd(usd)) { return }
        all.insert(wallet.into(), Usd(usd));
        store::save(&all, "balances.json");
    }

    /// The balance on screen is whole (the Mac's balanceReady): Solana has answered priced and Hyperliquid has answered
    /// since the wallet came on screen (each kept through a later read that failed), and the Automation wallet is read.
    pub fn balance_ready(solana: bool, evm: bool, solana_priced: Option<bool>, hl: bool, automation: bool) -> bool {
        (!solana || solana_priced == Some(true)) && (!evm || hl) && automation
    }

    /// The automation wallet's money, as the Mac counts it beside the balance: its Solana USDC and tokens and its USDC on
    /// every EVM network in dollars, and its SOL (priced by the caller). None without the wallet, or when Solana didn't
    /// answer or came unpriced.
    pub async fn automation_money(wallet: Option<SignerWallet>) -> Option<(Decimal, Decimal)> {
        let wallet = wallet?;
        let evm = async {
            let Some(owner) = &wallet.evm else { return Decimal::ZERO };
            let held = join_all(EvmNetwork::all().iter().map(|net| net.usdc_balance(owner)).collect()).await;
            sum(held.into_iter().map(|u| Decimal::from(u.unwrap_or(0)) / Decimal::from(1_000_000)))
        };
        let solana = async {
            let Some(owner) = &wallet.solana else { return Some((Decimal::ZERO, Decimal::ZERO)) };
            let b = Solana::balance(owner, false).await.ok().filter(|b| !b.unpriced)?;
            Some((b.usdc.saturating_add(sum(b.tokens.iter().filter(|t| t.looks_real()).map(|t| t.usd().unwrap_or_default()))), b.sol))
        };
        let (evm, solana) = tokio::join!(evm, solana);
        let (dollars, sol) = solana?;
        Some((dollars.saturating_add(evm), sol))
    }

    /// `chain` and `contract` for a coin whose P/L counts (not a dollar, not the perps' margin). Swift's defaults: all
    /// None.
    #[allow(clippy::too_many_arguments)]
    pub fn row(symbol: &str, amount: Decimal, usd: Option<Decimal>, icon: Option<String>, chain: Option<&str>, contract: Option<&str>,
               change24h: Option<Decimal>, verified: Option<bool>) -> Row {
        let key = match (chain, contract) {
            (Some(chain), Some(contract)) if !stablecoins::is_genuine(contract, chain) => Some(match chain {
                "solana" => contract.to_string(),
                "hyperliquid" => format!("hyperliquid:{}", contract.to_lowercase()),
                _ => format!("{chain}:{}", contract.to_lowercase()),
            }),
            _ => None,
        };
        // A dollar has no day's move to show, and no P/L to keep; it keeps its contract, so USDT, USDS, USDe and DAI open
        // their card and trade from their row as any token (the Mac's opener).
        Row {
            usd, icon, contract: contract.map(str::to_string), change24h: if key.is_none() { None } else { change24h }, verified, key,
            ..Row::new(symbol, &Fmt::num(amount, if amount < Decimal::ONE { 6 } else { 4 }), amount)
        }
    }

    /// `row(_ h: Holding)`: a token found by listing the address, with its day's move and whether a market list vouches
    /// for it.
    pub fn row_holding(h: &Holding) -> Row {
        // As the Mac marks it: verified, or «unverified» — never on Hyperliquid's spot book, where a name is one token.
        let verified = if h.verified { Some(true) } else if h.unverified() { Some(false) } else { None };
        Row {
            id: Some(h.id()), real: Some(h.looks_real()),
            ..Self::row(&h.symbol, h.amount, h.usd(), h.icon.clone(), Some(&h.chain), Some(&h.contract), h.change24h, verified)
        }
    }

    /// USDC in one Aave market or Morpho vault: «earning 4.12%» under its name (the Mac's NetworkLine), its last reading
    /// when the node doesn't answer, None when nothing's lent. Off Base a click opens its site, as the Mac's; Base's has
    /// its details board. The rate is read again at most every 10 minutes. Swift's default `now` is now.
    #[allow(clippy::too_many_arguments)]
    pub async fn lent<R, A>(name: &str, place: &str, owner: &str, on_base: bool, read: impl FnOnce() -> R, apy: impl FnOnce() -> A, now: Date) -> Option<Row>
    where R: Future<Output = Option<u64>>, A: Future<Output = Option<Decimal>> {
        let key = format!("{}|{place}", owner.to_lowercase());
        let last = LendingReadings::get(&key);
        let reading = match read().await {
            Some(raw) => {
                let mut reading = None;
                if raw >= 10_000 {
                    let usd = Decimal::from(raw) / Decimal::from(1_000_000);
                    let fresh = last.as_ref().filter(|l| (now - l.apy_at).num_milliseconds() < 600_000 && l.apy.is_some());
                    reading = Some(match fresh {
                        Some(l) => LendingReading { usd, apy: l.apy, apy_at: l.apy_at },
                        None => LendingReading { usd, apy: apy().await, apy_at: now },
                    });
                }
                LendingReadings::set(&key, reading.clone());
                reading
            }
            None => last,
        };
        Some(Self::lent_row(name, &reading?, on_base))
    }

    pub fn lent_row(name: &str, r: &LendingReading, on_base: bool) -> Row {
        let rate = r.apy.map_or(String::new(), |a| format!(" {}%", Fmt::num(rounded(a, 2, Rounding::Plain), 2)));
        let mut row = Self::deposit(name, r.usd, &format!("earning{rate}"));
        if !on_base { row.site = Some(if name == "Aave" { "app.aave.com" } else { "app.morpho.org" }.into()) }
        row
    }

    /// Robinhood's Stock Tokens as the Mac's loadRobinhood: the ones bought here (remembered by symbol) and any the explorer
    /// lists that Robinhood's registry knows, each read at Robinhood's bid; the explorer's copy is replaced, and a registry
    /// token is never tucked as an unknown airdrop. Without the registry the rows stay as the explorer gave them.
    /// `known`: the registry as the engine keeps it (Swift's `Engine.stockRegistry`), read here when there's none yet.
    pub async fn stocks(rows: Vec<Row>, owner: &str, rpc: &EvmRpc, known: Registry) -> Vec<Row> {
        let registry = match known {
            Some(r) => r,
            None => match RobinhoodStocks::registry().await { Ok(r) => r, Err(_) => return rows },
        };
        let bid = |symbol: String| async move { RobinhoodStocks::quote(&symbol).await.ok().flatten().map(|q| q.0) };
        let bought: HashSet<String> = Self::bought_stocks().into_iter().collect();
        let (mut out, mut seen) = (vec![], HashSet::new());
        for r in rows {
            let found = r.contract.as_ref().map(|c| c.to_lowercase()).and_then(|c| registry.iter().find(|t| t.address.to_lowercase() == c).map(|t| (c, t)));
            let Some((c, t)) = found else { out.push(r); continue };
            seen.insert(c);
            let price = bid(t.symbol.clone()).await;
            out.push(Self::stock_row(t, r.value, price, Some(&r)));
        }
        for t in registry.iter().filter(|t| bought.contains(&t.symbol.to_uppercase()) && !seen.contains(&t.address.to_lowercase())) {
            let Ok(call) = evm::erc20::balance_of(owner) else { continue };
            let Ok(raw) = rpc.read(&t.address, &call).await else { continue };
            let Some(amount) = evm::abi::u256_in(&raw, 0).and_then(|u| units(&u.to_string(), i64::from(t.decimals))) else { continue };
            if amount <= Decimal::ZERO { continue }
            let price = bid(t.symbol.clone()).await;
            out.push(Self::stock_row(t, amount, price, None));
        }
        out
    }

    /// A Stock Token's row: its symbol and Robinhood's logo, valued at the bid (the explorer's price when Robinhood gives
    /// none); kept on show, as the registry vouches for it.
    pub fn stock_row(t: &Token, amount: Decimal, bid: Option<Decimal>, explorer: Option<&Row>) -> Row {
        let usd = match bid { Some(b) => (Num::of(b) * amount).0, None => explorer.and_then(|e| e.usd) };
        let icon = explorer.and_then(|e| e.icon.clone()).filter(|i| !i.is_empty()).or_else(|| t.logo.clone());
        Row {
            id: explorer.and_then(|e| e.id.clone()), real: Some(true), stock: Some(true),
            ..Self::row(&t.symbol, amount, usd, icon, Some("robinhood"), Some(&t.address), explorer.and_then(|e| e.change24h), Some(true))
        }
    }

    /// The Stock Tokens bought through ihatecrypto, by symbol (the Mac's «stockTokens»): their balances are read with
    /// every refresh, listed by the explorer or not yet.
    pub fn bought_stocks() -> Vec<String> { store::load("stock-tokens.json").unwrap_or_default() }

    pub fn remember_stock(symbol: &str) {
        let s = symbol.to_uppercase();
        if s.starts_with("0X") { return }
        let mut list = Self::bought_stocks();
        if !list.contains(&s) {
            list.push(s);
            store::save(&list, "stock-tokens.json");
        }
    }

    /// A deposit in the DeFi section: its dollars, and what it does under its name.
    pub fn deposit(name: &str, usd: Decimal, note: &str) -> Row {
        Row { section: "defi".into(), note: Some(note.into()), ..Self::row(name, usd, Some(usd), None, None, None, None, None) }
    }

    /// Jupiter's portfolio API (its own platforms only), as the Mac's jupiterPositions: every element but Earn, which
    /// Jupiter Lend already reads. None when it doesn't answer.
    // ponytail: net's client waits 30 s; Swift's request 20 s.
    pub async fn jupiter_platforms(wallet: &str) -> Option<Vec<Row>> {
        #[derive(Deserialize)]
        struct Answer {
            elements: Vec<JupiterElement>,
        }
        let (data, _) = net::get(&format!("https://api.jup.ag/portfolio/v1/positions/{wallet}"), &[]).await.ok()?;
        let answer: Answer = serde_json::from_slice(&data).ok()?;
        Some(Self::jupiter_rows(&answer.elements))
    }

    /// The rows for Jupiter's elements: «Jupiter», its name and label, its rate a year when it earns one.
    pub fn jupiter_rows(elements: &[JupiterElement]) -> Vec<Row> {
        elements.iter().filter_map(|e| {
            if e.fetcher.as_deref() == Some("jupiter-exchange-earn") { return None }
            let value = rounded(from_double(e.value?)?, 2, Rounding::Plain);
            let name = [&e.name, &e.label].into_iter().flatten().cloned().collect::<Vec<_>>().join(" · ");
            let apy = e.apy.filter(|a| *a > 0.0).and_then(from_double).and_then(|a| (Num::of(a) * 100).rounded(2, Rounding::Plain).0)
                .map_or(String::new(), |a| format!(" · {}%", Fmt::num(a, 2)));
            let what = if name.is_empty() { e.platform.clone().unwrap_or_else(|| "position".into()) } else { name };
            // In dollars: a platform's position is no USDC (the Mac's line shows Fmt.usd of it).
            Some(Row {
                icon: Some("logos/jup.png".into()), site: Some("jup.ag".into()), amount: Fmt::usd(value),
                ..Self::deposit("Jupiter", value, &format!("{what}{apy}"))
            })
        }).collect()
    }

    /// `last`: the last read's tokens, which price a read Jupiter didn't answer. With the read: its tokens (for the next
    /// one) and whether any is still unpriced — a coin new since the last read. Swift's default `last` is none.
    pub async fn solana(owner: &str, last: &[Holding]) -> (Read, Vec<Holding>, bool) {
        let (lend, kamino) = (JupiterLend::default(), Kamino::default());
        let (balance, lend, kamino_read, vaults, jupiter) = tokio::join!(
            Solana::balance(owner, false), lend.positions(owner), kamino.positions(owner), kamino.vault_positions(owner), Self::jupiter_platforms(owner));
        let b = match balance {
            Ok(b) => b.pricing(last),
            Err(e) => return (Read::Failure("solana".into(), format!("Solana: {}", e.0)), vec![], false),
        };
        let mut rows = vec![];
        if b.usdc > Decimal::ZERO { rows.push(Self::row("USDC", b.usdc, Some(b.usdc), None, None, None, None, None)) }
        if b.sol > Decimal::ZERO { rows.push(Self::row("SOL", b.sol, None, None, Some("solana"), Some(CostBasis::WSOL), None, None)) }
        rows.extend(b.tokens.iter().map(Self::row_holding));
        // What's put in, as the Mac's DeFi lines: Jupiter Lend and Kamino Lend earning, Kamino's vaults by name and rate.
        let cent = Decimal::new(1, 2);
        let lend = sum(lend.unwrap_or_default().iter().map(|p| Decimal::from(p.assets) / Decimal::from(1_000_000)));
        if lend >= cent { rows.push(Self::deposit("Jupiter Lend", lend, "earning")) }
        let kamino = sum(kamino_read.unwrap_or_default().iter().map(|p| p.value_usd));
        if kamino >= cent { rows.push(Self::deposit("Kamino Lend", kamino, "earning")) }
        for v in vaults.unwrap_or_default().iter().filter(|v| v.value_usd >= cent) {
            // In its own coin and managed on its site, as the Mac's ProtocolPosition: «0.5 SOL», ≈ its dollars, kamino.com.
            rows.push(Row {
                amount: format!("{} {}", Fmt::num(v.tokens, 2), v.symbol), site: Some("kamino.com".into()),
                ..Self::deposit("Kamino", v.value_usd, &format!("{} · {}%", v.name, Fmt::num(rounded(v.apy, 2, Rounding::Plain), 2)))
            });
        }
        // Jupiter's other platforms (staked JUP, perps, predictions…), so a deposit made on its site still shows.
        rows.extend(jupiter.unwrap_or_default().into_iter().filter(|r| r.usd.unwrap_or_default() >= cent));
        (Read::Success(Some(Network::new("solana", "Solana", rows))), b.tokens, b.unpriced)
    }

    /// Each EVM network: its dollar (USDC, USDG on Robinhood), its gas coin, and the other tokens an explorer lists.
    /// `registry`: Robinhood's stock tokens as the engine keeps them (`stocks`).
    pub async fn evm(owner: &str, registry: Registry) -> Vec<Read> {
        join_all(EvmNetwork::all().iter().map(|net| Self::evm_network(net, owner, registry.clone())).collect()).await
    }

    async fn evm_network(net: &'static EvmNetwork, owner: &str, registry: Registry) -> Read {
        let gas = async { if net.fee_token.is_none() { net.rpc.balance(owner).await.map_err(|e| e.0) } else { Ok(0) } };
        let (dollars, gas) = tokio::join!(net.usdc_balance(owner), gas);
        let failed = |why: String| Read::Failure(net.key.into(), format!("{}: {why}", net.name));
        let usd = match dollars { Ok(u) => Decimal::from(u) / Decimal::from(1_000_000), Err(e) => return failed(e.to_string()) };
        let coin = match gas { Ok(g) => units(&g.to_string(), 18).unwrap_or_default(), Err(e) => return failed(e) };
        let mut rows = vec![];
        if usd > Decimal::ZERO { rows.push(Self::row(if net.key == "robinhood" { "USDG" } else { "USDC" }, usd, Some(usd), None, None, None, None, None)) }
        if coin > Decimal::ZERO { rows.push(Self::row(net.gas_coin, coin, None, None, Some(net.key), Some("native"), None, None)) }
        let skip = HashSet::from([net.usdc.to_lowercase()]);
        rows.extend(holdings::evm(net.key, owner, false, &skip, &[]).await.unwrap_or_default().iter().map(Self::row_holding));
        // USDC lent on Aave or in Morpho's vault here, with its rate; a market that doesn't answer keeps its last reading
        // (the Mac's loadLending), so the total and the curve don't dip.
        let on_base = net.key == "base";
        for m in aave::MARKETS.iter().filter(|m| m.chain == net.key) {
            let read = || async { net.rpc.token_balance(m.a_usdc, owner).await.ok().map(|u| u64::try_from(u).unwrap_or(0)) };
            let apy = || async { trade::aave::aave_verify(m).await.ok() };
            if let Some(r) = Self::lent("Aave", &m.place(), owner, on_base, read, apy, chrono::Utc::now()).await { rows.push(r) }
        }
        for v in morpho::VAULTS.iter().filter(|v| v.chain == net.key) {
            let read = || async { trade::morpho::morpho_value(owner, v).await.ok() };
            let apy = || MorphoRates::net_apy(v);
            if let Some(r) = Self::lent("Morpho", &v.place(), owner, on_base, read, apy, chrono::Utc::now()).await { rows.push(r) }
        }
        if net.key == "robinhood" { rows = Self::stocks(rows, owner, &net.rpc, registry).await }
        Read::Success(Some(Network::new(net.key, net.name, rows)))
    }

    pub async fn near(account: Option<&str>) -> Read {
        let Some(account) = account else { return Read::Success(None) };
        let (near, wrapped) = match Near::balance(account).await {
            Ok(b) => b,
            Err(e) => return Read::Failure("near".into(), format!("NEAR: {}", e.0)),
        };
        let total = near.saturating_add(wrapped);
        // Keyed «near:native» as the Mac's, so it has its P/L and isn't counted with the idle dollars.
        let mut rows = if total > Decimal::ZERO { vec![Self::row("NEAR", total, None, None, Some("near"), Some("native"), None, None)] } else { vec![] };
        rows.extend(holdings::near(account).await.unwrap_or_default().iter().filter(|h| h.contract != "native" && h.contract != "wrap.near").map(Self::row_holding));
        Read::Success(Some(Network::new("near", "NEAR", rows)))
    }

    // MARK: The token card's P/L

    /// The token card's held P/L as the Mac's TokenBoard says it under the number: «Bought 0.3968 NEAR for $2.00 at
    /// $5.04 · 0.0766 more came in from elsewhere, counted at the price then: $5.10 · worth $2.39 now», and since when
    /// («since you bought on Oct 10»; «since Oct 13» when some of it came in unpaid). A number past the arithmetic says
    /// there's no price, where Swift's Decimal went on.
    pub fn held_paid(symbol: &str, amount: Decimal, gain: Option<Decimal>, cost: Option<&CostBasis>, ru: bool) -> (String, Option<String>) {
        let say = |en: String, r: String| if ru { r } else { en };
        let none = || (say("No price yet to count its P/L from".into(), "Пока нет цены, от которой считать прибыль".into()), None);
        let (Some(gain), Some(cost)) = (gain, cost) else { return none() };
        if cost.amount <= Decimal::ZERO { return none() }
        let counted = amount.min(cost.amount);
        let figures = || -> Option<(Decimal, Decimal, Decimal, Decimal, Decimal)> {
            let share = (Num::of(counted) / cost.amount).0?;
            let came = (Num::of(cost.received.unwrap_or_default()) * share).0?;
            let came_usd = (Num::of(cost.received_usd.unwrap_or_default()) * share).0?;
            let paid = (Num::of(cost.usd) * share).0?;
            Some((came, came_usd, (Num::of(counted) - came).0?, (Num::of(paid) - came_usd).0?, (Num::of(paid) + gain).0?))
        };
        let Some((came, came_usd, bought, spent, worth)) = figures() else { return none() };
        let price = |usd: Decimal, of: Decimal| (Num::of(usd) / of).0.map_or_else(|| "—".into(), Self::token_price);
        let mut parts = vec![];
        if bought > Decimal::ZERO {
            let (b, s, at) = (Fmt::num(bought, 4), Fmt::usd(spent), price(spent, bought));
            parts.push(say(format!("Bought {b} {symbol} for {s} at {at}"), format!("Куплено {b} {symbol} за {s} по {at}")));
        }
        if came > Decimal::ZERO {
            let (c, at, more) = (Fmt::num(came, 4), price(came_usd, came), bought > Decimal::ZERO);
            parts.push(say(format!("{c} {} came in from elsewhere, counted at the price then: {at}", if more { "more" } else { symbol }),
                           format!("{}{c}{} пришло со стороны, по цене того дня: {at}", if more { "ещё " } else { "" }, if more { String::new() } else { format!(" {symbol}") })));
        }
        parts.push(say(format!("worth {} now", Fmt::usd(worth)), format!("сейчас стоит {}", Fmt::usd(worth))));
        if amount > counted {
            let rest = Fmt::num(amount - counted, 4);
            parts.push(say(format!("{rest} more not counted: no price for it"), format!("ещё {rest} не посчитано: для них нет цены")));
        }
        let since = cost.since.map(|day| {
            let d = short_day(day, ru);
            if cost.received.unwrap_or_default() > Decimal::ZERO { say(format!("since {d}"), format!("с {d}")) }
            else { say(format!("since you bought on {d}"), format!("с покупки {d}")) }
        });
        (parts.join(" · "), since)
    }

    /// A token's price with the digits a small one needs: $5.04, $0.0001234 (the Mac's tokenPrice).
    pub fn token_price(d: Decimal) -> String {
        if d >= Decimal::ONE { return Fmt::usd(d) }
        let (mut digits, mut x) = (2u32, d);
        while x < Decimal::ONE && x > Decimal::ZERO && digits < 10 {
            x *= Decimal::TEN;
            digits += 1;
        }
        format!("${}", Fmt::num(d, (digits + 2).min(12)))
    }
}

/// `DateFormatter` with the template «d MMM» in en_US («Oct 10») or ru_RU («10 окт.»), this computer's time zone.
pub(crate) fn short_day(d: Date, ru: bool) -> String {
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const RU: [&str; 12] = ["янв.", "февр.", "мар.", "апр.", "мая", "июн.", "июл.", "авг.", "сент.", "окт.", "нояб.", "дек."];
    let local = d.with_timezone(&Local);
    let m = local.month0() as usize;
    if ru { format!("{} {}", local.day(), RU[m]) } else { format!("{} {}", EN[m], local.day()) }
}

impl Network {
    /// `Network(key:name:usd:rows:)` with its usd 0: the read's card before pricing.
    fn new(key: &str, name: &str, rows: Vec<Row>) -> Self {
        Network { key: key.into(), name: name.into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 }
    }
}

/// One element of Jupiter's portfolio answer (the Mac's jupiterRows tuple).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct JupiterElement {
    #[serde(rename = "fetcherId")]
    pub fetcher: Option<String>,
    #[serde(rename = "platformId")]
    pub platform: Option<String>,
    pub name: Option<String>,
    pub label: Option<String>,
    pub value: Option<f64>,
    #[serde(rename = "netApy")]
    pub apy: Option<f64>,
}

/// `LendingReadings.Reading`: an Aave market's or Morpho vault's last reading.
#[derive(Debug, Clone, PartialEq)]
pub struct LendingReading {
    pub usd: Decimal,
    pub apy: Option<Decimal>,
    pub apy_at: Date,
}

/// Each Aave market's and Morpho vault's last reading, by owner and place, for the portfolio's reads that run side by
/// side (Swift's `LendingReadings.shared`).
pub struct LendingReadings;

static READINGS: LazyLock<Mutex<HashMap<String, LendingReading>>> = LazyLock::new(Default::default);

impl LendingReadings {
    pub fn get(key: &str) -> Option<LendingReading> { READINGS.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned() }

    pub fn set(key: &str, r: Option<LendingReading>) {
        let mut all = READINGS.lock().unwrap_or_else(|e| e.into_inner());
        match r { Some(r) => { all.insert(key.into(), r); } None => { all.remove(key); } }
    }
}

#[cfg(test)]
#[path = "portfolio/tests.rs"]
mod tests;

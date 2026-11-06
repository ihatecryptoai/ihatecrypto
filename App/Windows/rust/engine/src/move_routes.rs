//! A move's route on its block (MoveRoutes.swift): FROM, BRIDGE, SWAP, SIGN… with the balances each step changes.
//! The Mac's route for the moves Windows drew as one card (Canvases.swift route(_:)): a stock or a token bought or sold
//! on an EVM network, swaps off Solana and across networks, a revoke, NEAR, a coin sent through NEAR Intents, a paid
//! request, a multisig, CCTP, USDT0, Aave's debt, Merkl's rewards, USDC to a saved address, a close. Each quote lands on
//! its own step as it comes (`quote_route`), and the card says what the move risks (`move_cautions`), and the footer
//! under each route (`route_note`).

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;

use evm::protocols::merkl::{self, Reward};
use evm::protocols::{aave, usdt0};
use fancy_regex::Regex;
use ihatecrypto_core::foundation::{self, Num, Rounding, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::stablecoins;
use ihatecrypto_core::tokens::TokenLookup;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use signer_api::SignerAddress;
use trade::{EvmNetwork, Executor};
use wallet::address_check::{self, Network};

use crate::canvas::{Money, Step};
use crate::engine::{ClaimOwed, Engine, Pending, WayQuotes};
use crate::from_core::RouterOption;
use crate::route_edit::{self, Way, stock_way_name, way_key};

/// Contracts' symbols looked up for a route's labels (the Mac's tokenNames), each asked once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContractNames {
    pub known: HashMap<String, String>,
    pub asked: HashSet<String>,
}

/// What pays a stock buy (`Engine::stock_funding`, Swift's labelled tuple): the USDG on Robinhood Chain and the USDC on
/// Solana now, whether the cents short aren't worth a bridge, what the buy spends, what's brought, and the EVM network
/// that sends it (with what it sends) when Solana is short.
#[derive(Debug, Clone)]
pub struct StockFunding {
    pub usdg: Decimal,
    pub solana: Decimal,
    pub fits: bool,
    pub usdc: Decimal,
    pub bring: Decimal,
    pub elsewhere: Option<(&'static EvmNetwork, Decimal)>,
}

/// «0x3C44…93BC»: the first and last characters, by Character as Swift's prefix/suffix.
fn short(s: &str, head: usize, tail: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    format!("{}…{}", c.iter().take(head).collect::<String>(), c[c.len().saturating_sub(tail)..].iter().collect::<String>())
}

/// «7xKX 9fQm … 3kPq 2Wd1».
fn ends(address: &str) -> String {
    let (start, end) = address_check::ends(address);
    format!("{start} … {end}")
}

/// `AddressCheck.Network(rawValue:)?.title`.
fn network_title(raw: &str) -> Option<&'static str> { serde_json::from_value::<Network>(serde_json::Value::from(raw)).ok().map(Network::title) }

/// A network's name as the cards say it: AddressCheck's title, else the place's.
fn net_title(raw: &str) -> String { network_title(raw).map_or_else(|| WalletCommand::place_name(raw), str::to_string) }

/// `replacingOccurrences(of:with:options: .caseInsensitive)` for an ASCII needle (a contract).
fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() { return hay.into() }
    let (h, n) = (hay.to_ascii_lowercase(), needle.to_ascii_lowercase());
    let mut out = String::new();
    let mut last = 0;
    for (i, _) in h.match_indices(&n) {
        out.push_str(&hay[last..i]);
        out.push_str(with);
        last = i + n.len();
    }
    out + &hay[last..]
}

/// Swift's `.whitespaces`: tab and the space separators, not line breaks.
fn is_space(c: char) -> bool { c == '\t' || (c.is_whitespace() && !matches!(c, '\n'..='\r' | '\u{85}' | '\u{2028}' | '\u{2029}')) }

static QUOTED_AMOUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d[\d,]*(?:\.\d+)?").expect("a regex"));

/// What `user` can claim now on Merkl's chains (Merkl.rewards); None when an answer fails.
async fn merkl_rewards(user: &str) -> Option<Vec<Reward>> {
    let mut out = vec![];
    for chain in merkl::CHAINS {
        let url = format!("https://api.merkl.xyz/v4/users/{user}/rewards?chainId={chain}");
        let response = net::HTTP.get(&url).header("Accept", "application/json").timeout(Duration::from_secs(15)).send().await.ok()?;
        if response.status().as_u16() != 200 { return None }
        out.extend(merkl::parse(&response.bytes().await.ok()?, user).ok()?);
    }
    Some(out)
}

/// The real dollars and wrapped ether where the app moves money: their tickers on any other contract are a fake.
static GENUINE: LazyLock<HashSet<String>> = LazyLock::new(|| {
    EvmNetwork::all().iter().map(|n| n.usdc.to_lowercase())
        .chain(stablecoins::BY_NETWORK.iter().flat_map(|(_, coins)| coins.iter()).map(|c| c.address.to_lowercase()))
        .chain([
            "0x4200000000000000000000000000000000000006", // WETH · Base
            "0x82af49447d8a07e3bd95bd0d56f35241523fbab1", // WETH · Arbitrum
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", // WETH · Ethereum
            "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT · Ethereum
            "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", // USDT · Arbitrum
        ].map(String::from))
        .collect()
});

impl Engine {
    // MARK: What a card risks

    /// «A token by its contract is anyone's coin»: a buy or a swap into a token named only by its contract.
    pub fn by_contract(&self) -> String {
        self.say("A token by its contract is anyone’s coin: its price can halve in a day and nobody vouches for it. Put in only what you can lose.",
                 "Токен по контракту может выпустить кто угодно: его цена может упасть вдвое за день, и никто за него не ручается. Вкладывай только то, что не жалко потерять.")
    }

    /// The cautions under a move's route that aren't about whom it pays (the Mac's MoveRoute.warning): a token taken by
    /// its contract, and what a Robinhood Stock Token is.
    pub fn move_cautions(&self, c: &WalletCommand) -> Vec<String> {
        match c {
            WalletCommand::BuyStock { symbol, .. } => vec![if symbol.starts_with("0x") { self.by_contract() } else {
                self.say("Robinhood Stock Tokens give economic exposure, not the shares, and aren’t offered to US persons or in Canada, the UK and Switzerland.",
                         "Токены акций Robinhood повторяют цену акции, но акцией не являются, и не продаются жителям США, Канады, Великобритании и Швейцарии.")
            }],
            WalletCommand::BuyOn { token, .. } if token.starts_with("0x") => vec![self.by_contract()],
            WalletCommand::SwapOn { place, to, .. } if to.starts_with("0x") && EvmNetwork::named(place).is_some() && place != "tempo" => vec![self.by_contract()],
            _ => vec![],
        }
    }

    /// An exchange's XRP address credits a deposit only by its destination tag: without it the money is lost to the user.
    pub fn xrp_tag_warning(&self, entry: &SignerAddress) -> Option<String> {
        if entry.network != "xrp" || !entry.memo.trim_matches(is_space).is_empty() { return None }
        Some(self.say("An exchange’s XRP address needs its destination tag: without it the exchange can’t credit you. Save the tag as this address’s memo first.",
                      "XRP-адресу биржи нужен тег назначения: без него биржа не зачислит перевод. Сначала сохрани тег как memo этого адреса."))
    }

    /// Kamino's first-deposit rent stopped by the loss cap: Jupiter Lend takes the same deposit without it, one card away
    /// (the Mac's choice beside «Send anyway», Execution.swift:735).
    pub fn rent_free_instead(&self, p: &Pending) -> Vec<RouterOption> {
        let Pending::Wallet(WalletCommand::Kamino { deposit: true, symbol, amount: Some(a) }) = p else { return vec![] };
        if symbol != "USDC" && symbol != "SOL" { return vec![] }
        vec![RouterOption::new(&self.say("Jupiter Lend instead · no rent", "Лучше в Jupiter Lend · без аренды"),
                               &self.say("The same deposit, no accounts to pay for", "Тот же депозит, без платы за аккаунты"),
                               &WalletCommand::LendDeposit(symbol.clone(), Some(*a)).summary(), false)]
    }

    // MARK: Quotes on the route

    /// The waiting move's quotes as they came (route_edit's quote_ways, quote_route), the best first; None until then.
    pub fn quotes(&self, c: &WalletCommand) -> Option<Vec<Way>> {
        self.st().way_quotes.as_ref().filter(|q| &q.command == c).map(|q| q.ways.clone())
    }

    /// The amount a quote says arrives: «2.41 AERO», «1,234.5 USDC · 0.3% impact» → 2.41, 1234.5. Our own quotes'
    /// wording, never the user's (the Mac's quotedAmount).
    pub fn quoted_amount(gets: Option<&str>) -> Option<Decimal> {
        let m = QUOTED_AMOUNT.find(gets?).ok().flatten()?;
        foundation::decimal(&m.as_str().replace(',', ""))
    }

    /// The route drawn again from what is known now: a quote that came lands on its step, and the footer says the way it
    /// takes now (a cross swap found through NEAR Intents, Tempo's exchange or Relay's swap there).
    pub fn redraw_route(&self, board: &str, c: &WalletCommand) {
        if self.redraw_chain(board) { return }
        let Some(steps) = self.route_steps(c) else {
            self.changed();
            return;
        };
        let note = self.route_note(c);
        self.update_board(Some(board), |b| {
            b.steps = Some(steps);
            if let Some(note) = note { b.note = Some(note) }
        });
        self.name_tokens(board, c);
    }

    /// A chain's waiting card drawn again whole (canvas.rs steps) once its first move's quote or a token's symbol comes:
    /// the Mac quotes and names the chain's first move as any card's (WithdrawBoard, chainSteps). Swift's result is
    /// discardable.
    pub fn redraw_chain(&self, board: &str) -> bool {
        let p = {
            let st = self.st();
            match st.pending.as_ref() {
                Some(p @ Pending::Chain(_)) if st.card.as_ref().is_some_and(|c| c.id == board) => p.clone(),
                _ => return false,
            }
        };
        let steps = self.steps(&p, "", &[]);
        self.update_board(Some(board), |b| b.steps = Some(steps));
        // Its footer from the route as it is now (the way a quote picked, a contract's name), as the Mac's live route.
        self.note_route(board, &p);
        true
    }

    /// A move's title with each token it names by contract as people read it, «⚠ USDC (0x3C44…93BC)» (the Mac's
    /// readable title); only a title that is the move's own words, and only its tokens — never a recipient's address.
    pub fn name_tokens(&self, board: &str, c: &WalletCommand) {
        let tokens: Vec<(&str, &str)> = match c {
            WalletCommand::BuyOn { place, token, .. } | WalletCommand::SellOn { place, token, .. } => vec![(token, place)],
            WalletCommand::SwapOn { place, from, to, .. } => vec![(from, place), (to, place)],
            WalletCommand::CrossSwap { from, on, to, at, .. } => vec![(from, on), (to, at)],
            WalletCommand::BuyStock { symbol, .. } | WalletCommand::SellStock { symbol, .. } => vec![(symbol, "robinhood")],
            _ => vec![],
        };
        let contracts: Vec<(&str, &str)> = tokens.into_iter().filter(|(t, _)| t.starts_with("0x") && t.chars().count() == 42).collect();
        if contracts.is_empty() { return }
        let summary = c.summary();
        let named = contracts.iter().fold(summary.clone(), |s, (t, p)| replace_ci(&s, t, &self.token_name(t, p)));
        // Named before its symbol was known: the contract shortened.
        let unnamed = contracts.iter().fold(summary.clone(), |s, (t, _)| replace_ci(&s, t, &short(t, 6, 4)));
        let ours = |title: &str| title == summary || title == unnamed;
        let title = named.clone();
        self.update_board(Some(board), |b| if ours(&b.title) { b.title = title });
        if let Some(card) = self.st_mut().card.as_mut().filter(|c| c.id == board && ours(&c.title)) { card.title = named }
    }

    /// The waiting card's id is `board`.
    fn card_is(&self, board: &str) -> bool { self.st().card.as_ref().is_some_and(|c| c.id == board) }

    /// The waiting move's quote for its route (the Mac's quoteSwap, all but Jupiter's): asked once and again every 15 s
    /// while it fails; Aave's health factor every 30 s while the card waits, as it moves with the price. Nothing is
    /// signed. False for a move this doesn't quote.
    pub async fn quote_route(&self, c: &WalletCommand, board: &str) -> bool {
        if matches!(c, WalletCommand::ClaimRewards | WalletCommand::SweepRewards { .. }) {
            // What Merkl owes, by network, is the route itself.
            let me = self.st().evm_address();
            let Some(me) = me else { return true };
            let Some(owed) = merkl_rewards(&me).await.filter(|o| !o.is_empty()) else { return true };
            if !self.card_is(board) { return true }
            self.st_mut().claim_owed = Some(ClaimOwed { board: board.into(), command: c.clone(), owed: owed.clone() });
            // Leading a chain, the claim is its first move's route and the later steps, title and footer stay (the Mac's
            // route.steps + chainSteps()): Confirm still runs the whole chain.
            if self.redraw_chain(board) { return true }
            let (steps, note) = (self.claim_steps(&owed, c), self.claim_note(&owed, c));
            // Its title names what's claimed (the Mac's claimRoute), not the command's words.
            let sweep = matches!(c, WalletCommand::SweepRewards { .. });
            let title = if sweep { self.say("Sweep rewards · ", "Собрать награды · ") } else { self.say("Claim rewards · ", "Забрать награды · ") }
                + &owed.iter().map(|r| format!("{} {}", Fmt::num(r.claimable(), 6), r.symbol)).collect::<Vec<_>>().join(", ");
            let t = title.clone();
            self.update_board(Some(board), |b| {
                b.steps = Some(steps);
                b.note = Some(note);
                b.title = t;
            });
            if let Some(card) = self.st_mut().card.as_mut() { card.title = title }
            return true;
        }
        if !self.route_quoted(c) { return false }
        while self.card_is(board) {
            if let Some(ways) = self.route_quote(c).await {
                if !self.card_is(board) { return true }
                self.set_way_quotes(Some(WayQuotes { command: c.clone(), ways: ways.clone() }));
                self.redraw_route(board, c);
                if !matches!(c, WalletCommand::AaveBorrow { .. }) { return true }
                self.liquidation_caution(board, &ways);
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }
            self.quote_failed(c);
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
        true
    }

    /// The moves `route_quote` can quote: Solana's own go to Jupiter (quoteCard), the EVM trades to route_edit's quote_ways.
    fn route_quoted(&self, c: &WalletCommand) -> bool {
        match c {
            WalletCommand::HlSpotBuy { usdc: Some(_), .. } | WalletCommand::HlSpotSell { amount: Some(_), .. } | WalletCommand::CrossSwap { .. }
            | WalletCommand::AaveBorrow { amount: Some(_), .. } => true,
            WalletCommand::SwapOn { place, amount, .. } => {
                if place == "hyperliquid" || place == "near" { amount.is_some() } else { place != "solana" && EvmNetwork::named(place).is_some() }
            }
            WalletCommand::Usdt { from, to, .. } | WalletCommand::Bridge { from, to, .. } => EvmNetwork::named(from).is_some() && EvmNetwork::named(to).is_some(),
            WalletCommand::SendToken { name, token, amount: Some(_), network } => matches!(self.across_recipient(name, token, network.as_deref()), Ok(Some(_))),
            _ => false,
        }
    }

    /// One read of the quote; None when it failed or the move has none.
    async fn route_quote(&self, c: &WalletCommand) -> Option<Vec<Way>> {
        let e = self.executor().ok()?;
        let one = |way: &str, gets: &str| vec![Way { way: way.into(), gets: gets.into(), worth: String::new(), less: None, usd: None }];
        let near = self.st().near_address();
        match c {
            WalletCommand::HlSpotBuy { symbol, usdc: Some(u) } => Executor::spot_quote(symbol, Some(*u), None).await.ok().flatten().map(|g| one("Hyperliquid", &g)),
            WalletCommand::HlSpotSell { symbol, amount: Some(x) } => Executor::spot_quote(symbol, None, Some(*x)).await.ok().flatten().map(|g| one("Hyperliquid", &g)),
            WalletCommand::SwapOn { place, from, to, amount: Some(x) } if place == "hyperliquid" => {
                Executor::spot_swap_quote(from, to, *x).await.ok().map(|g| one("Hyperliquid", &g))
            }
            WalletCommand::SwapOn { place, from, to, amount: Some(x) } if place == "near" => {
                e.cross_quote_text(from, "near", to, "near", Some(*x), near.as_deref()).await.ok().map(|(gets, way)| one(&way, &gets))
            }
            WalletCommand::SwapOn { place, from, to, amount } => {
                let net = EvmNetwork::named(place)?;
                e.pair_quote(net, from, to, *amount).await.ok().map(|(gets, way)| one(&way, &gets))
            }
            WalletCommand::CrossSwap { from, on, to, at, amount } => {
                e.cross_quote_text(from, on, to, at, *amount, near.as_deref()).await.ok().map(|(gets, way)| one(&way, &gets))
            }
            WalletCommand::Usdt { from, to, amount, layer_zero } => {
                let (a, b) = (EvmNetwork::named(from)?, EvmNetwork::named(to)?);
                let ways = e.usdt_ways(a, b, *amount, *layer_zero).await.ok().filter(|w| !w.is_empty())?;
                Some(ways.into_iter().map(|w| Way { way: w.way, gets: w.gets, worth: Fmt::usd(w.worth), less: None, usd: None }).collect())
            }
            WalletCommand::Bridge { from, to, amount } => {
                let (a, b) = (EvmNetwork::named(from)?, EvmNetwork::named(to)?);
                let ways = e.bridge_ways(a, b, *amount).await.ok().filter(|w| !w.is_empty())?;
                Some(ways.into_iter().map(|w| Way { way: w.way, gets: w.gets, worth: Fmt::usd(w.worth), less: None, usd: None }).collect())
            }
            WalletCommand::AaveBorrow { amount: Some(x), network } => {
                let m = aave::MARKETS.iter().find(|m| m.chain == network)?;
                let (now, after, liquidation) = e.aave_borrow_preview(m, *x).await.ok()?;
                let (h, a) = (now.health().map_or_else(|| "∞".into(), |h| Fmt::num(h, 2)), after.map_or_else(|| "∞".into(), |h| Fmt::num(h, 2)));
                Some([one("health", &format!("{h} → {a}")), one("liquidation", &liquidation.map(|l| format!("ETH ≤ {}", Fmt::usd(l))).unwrap_or_default()),
                      one("collateral", &Fmt::usd(now.collateral))].concat())
            }
            WalletCommand::SendToken { name, token, amount: Some(x), network } => {
                let (entry, symbol, address) = self.across_recipient(name, token, network.as_deref()).ok().flatten()?;
                let (q, _, _, source) = e.across_quote(&symbol, *x, &entry.network, &address).await.ok()?;
                // What it's worth there, and what 1Click and the network keep: a small BTC send loses a fifth to fees.
                let cost = foundation::decimal(&q.amount_in_formatted).unwrap_or_default();
                let worth = q.amount_out_usd;
                let fees = worth.and_then(|w| (Num::of(cost) - w).0).map(|f| if f > Decimal::ZERO && cost > Decimal::ZERO {
                    let pct = (Num::of(f) / cost * 100).0.map(|p| Fmt::num(p, 0)).unwrap_or_default();
                    self.say(&format!(" · fees ≈ {} ({pct}%)", Fmt::usd(f)), &format!(" · комиссии ≈ {} ({pct}%)", Fmt::usd(f)))
                } else { String::new() }).unwrap_or_default();
                let time = q.time_estimate.map(|t| if t >= 120 {
                    self.say(&format!(" · ≈ {} min", t / 60), &format!(" · ≈ {} мин", t / 60))
                } else { self.say(&format!(" · ≈ {t} s"), &format!(" · ≈ {t} с")) }).unwrap_or_default();
                let (cost, worth) = (Fmt::num(cost, 2), worth.map_or_else(|| "?".into(), Fmt::usd));
                Some(one(source.map_or("Solana", |n| n.name), &(self.say(&format!("≈ {cost} USDC for {worth} of it"), &format!("≈ {cost} USDC за {worth}")) + &fees + &time)))
            }
            _ => None,
        }
    }

    /// «Liquidation if ETH ≤ $X» under a borrow's route, as its last quote says; replaced by each new one.
    pub fn liquidation_caution(&self, board: &str, ways: &[Way]) {
        let mark = self.say("Liquidation if", "Ликвидация, если");
        let text = ways.iter().find(|w| w.way == "liquidation").filter(|w| !w.gets.is_empty()).map(|w| {
            self.say(&format!("Liquidation if {}: Aave sells your collateral at a penalty. Keep an eye on it, or borrow less.", w.gets),
                     &format!("Ликвидация, если {}: Aave продаст твой залог со штрафом. Следи за ценой или займи меньше.", w.gets))
        });
        if let Some(card) = self.st_mut().card.as_mut() {
            card.warnings.retain(|w| !w.starts_with(&mark));
            if let Some(t) = &text { card.warnings.push(t.clone()) }
        }
        let marked = format!("⚠ {mark}");
        self.update_board(Some(board), |b| {
            b.lines.retain(|l| !l.starts_with(&marked));
            if let Some(t) = text { b.lines.push(format!("⚠ {t}")) }
        });
    }

    // MARK: The routes

    /// A contract's symbol from a favorite, the portfolio's rows on `place`, or one looked up before.
    fn known_symbol(&self, key: &str, place: &str) -> Option<String> {
        let st = self.st();
        st.favorites.iter().find(|f| f.address.to_lowercase() == key).map(|f| f.symbol.clone())
            .or_else(|| st.portfolio.networks.iter().find(|n| n.key == place)
                .and_then(|n| n.rows.iter().find(|r| r.contract.as_ref().is_some_and(|c| c.to_lowercase() == key))).map(|r| r.symbol.clone()))
            .or_else(|| st.contract_names.known.get(key).cloned())
    }

    /// A token as people read it (the Mac's tokenLabel): a contract as «AI (0x2E8c…1e18)», with ⚠ before a symbol that
    /// imitates another — a ticker the app moves money in, on any other contract — and shortened until its symbol is
    /// known (a favorite, the portfolio, else looked up once); a ticker as written.
    pub fn token_name(&self, token: &str, place: &str) -> String {
        if !token.starts_with("0x") { return stablecoins::find(token, place).map_or_else(|| token.to_uppercase(), |c| c.symbol.into()) }
        let short = short(token, 6, 4);
        let Some(symbol) = self.known_symbol(&token.to_lowercase(), place) else {
            self.learn_name(token);
            return short;
        };
        format!("{}{symbol} ({short})", if TokenLookup::suspicious(&symbol, token, Self::genuine_contracts()) { "⚠ " } else { "" })
    }

    /// A contract's ticker when known (the Mac's tokenSymbol): a favorite's, the portfolio's, a learned one; None until then.
    pub fn token_symbol(&self, token: &str, place: &str) -> Option<String> {
        let symbol = self.known_symbol(&token.to_lowercase(), place);
        if symbol.is_none() { self.learn_name(token) }
        symbol
    }

    /// The real dollars and wrapped ether where the app moves money: their tickers on any other contract are a fake.
    pub fn genuine_contracts() -> &'static HashSet<String> { &GENUINE }

    /// An unknown contract's symbol, asked once (DexScreener); the waiting card is drawn again with it.
    fn learn_name(&self, contract: &str) {
        if !self.st_mut().contract_names.asked.insert(contract.to_lowercase()) { return }
        let (me, contract) = (self.rc(), contract.to_string());
        tokio::task::spawn_local(async move { me.learned_name(&contract).await });
    }

    async fn learned_name(&self, contract: &str) {
        let Some(symbol) = TokenLookup::lookup(contract).await.ok().and_then(|l| l.into_iter().next()).map(|i| i.symbol) else { return };
        self.st_mut().contract_names.known.insert(contract.to_lowercase(), symbol);
        let (pending, id) = {
            let st = self.st();
            (st.pending.clone(), st.card.as_ref().map(|c| c.id.clone()))
        };
        match (pending, id) {
            (Some(Pending::Wallet(c)), Some(id)) => self.redraw_route(&id, &c),
            (_, Some(id)) => { self.redraw_chain(&id); }
            _ => {}
        }
    }

    /// A token that arrives: what's held of it now (0 when none) and that plus what arrives, «≈».
    pub fn gained(&self, token: &str, label: &str, place: &str, delta: Decimal) -> Money {
        let was = self.held_there(token, place).unwrap_or_default();
        Money { place: WalletCommand::place_name(place), symbol: label.into(), was: Self::amount(was, label),
                after: format!("≈ {}", Self::amount(was.saturating_add(delta), label)) }
    }

    /// A price per token from the portfolio's row, when it's held.
    fn row_price(&self, token: &str, place: &str) -> Option<Decimal> {
        let st = self.st();
        let r = st.portfolio.networks.iter().find(|n| n.key == place)?.rows.iter()
            .find(|r| r.symbol.to_uppercase() == token.to_uppercase() || r.contract.as_ref().is_some_and(|c| c.to_lowercase() == token.to_lowercase()))?;
        r.usd.and_then(|u| if r.value > Decimal::ZERO { u.checked_div(r.value) } else { None })
    }

    /// A market's mark price.
    fn mark(&self, coin: &str) -> Option<Decimal> { self.st().markets.get(coin).map(|m| m.mark_px) }

    fn label_from(&self, place: &str) -> String { self.say("FROM · ", "ОТКУДА · ") + &WalletCommand::place_name(place).to_uppercase() }
    fn label_to(&self, place: &str) -> String { self.say("TO · ", "КУДА · ") + &WalletCommand::place_name(place).to_uppercase() }
    fn saved_label(&self) -> String { self.say("TO · SAVED ADDRESS", "КУДА · СОХРАНЁННЫЙ АДРЕС") }
    fn quoting(&self) -> String { self.say("quoting…", "считаю котировку…") }
    fn no_quote(&self) -> String { self.say("no quote now · asked again every 30 s", "котировки сейчас нет · спрошу снова через 30 с") }

    /// The moves `route_steps` hands on: None for one drawn as one step.
    pub fn card_route(&self, c: &WalletCommand) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        match c {
            WalletCommand::BuyStock { symbol, usdc: Some(u) } => Some(self.stock_buy(symbol, *u)),
            WalletCommand::SellStock { symbol, amount } => Some(self.stock_sale(symbol, *amount)),
            WalletCommand::BuyOn { place, token, usdc: Some(u), from } => Some(self.evm_trade(place, token, Some(*u), None, from.as_deref(), false)),
            WalletCommand::SellOn { place, token, amount, keep } => Some(self.evm_trade(place, token, None, *amount, None, *keep)),
            WalletCommand::SwapOn { place, from, to, amount } => self.pair(c, place, from, to, *amount),
            WalletCommand::CrossSwap { from, on, to, at, amount } => Some(self.cross(c, from, on, to, at, *amount)),
            WalletCommand::RevokeApproval { place, token, spender, permit2 } => {
                let (symbol, short) = (self.token_name(token, place), short(spender, 6, 4));
                let who = EvmNetwork::named(place).and_then(|n| Executor::spender_name(spender, n.rpc.chain_id)).map_or_else(|| short.clone(), str::to_string);
                Some(vec![self.step(&(say("TOKEN · ", "ТОКЕН · ") + &WalletCommand::place_name(place).to_uppercase()), &symbol,
                                    &if *permit2 { say("through Permit2", "через Permit2") } else { say("its own approve", "его собственное разрешение") }, None),
                          self.step(&say("REVOKE · TO ZERO", "ОТЗЫВ · В НОЛЬ"), &who, &say(&format!("{short} can't spend it after"), &format!("{short} больше не сможет его тратить")), None)])
            }
            WalletCommand::BuyNear { usdc: Some(u) } => Some(self.near_buy(*u)),
            WalletCommand::SellNear { amount, to } => Some(self.near_sale(*amount, to.as_deref())),
            WalletCommand::SendNear { name, amount } => self.near_out(name, *amount, false),
            WalletCommand::CloseNear { name } => self.near_out(name, None, true),
            WalletCommand::Pay { .. } => self.payment(c),
            WalletCommand::MultisigCreate { .. } => self.multisig_creation(),
            WalletCommand::MultisigSend { .. } => self.multisig_sending(c),
            WalletCommand::Cctp { from: src, to: dst, amount: x } => {
                let have = self.usdc_free(src);
                let moving = x.map(|x| x.min(have.unwrap_or(x))).or(have);
                let shown = moving.map_or_else(|| say("all USDC", "всё USDC"), |m| format!("{} USDC", Fmt::num(m, 2)));
                let (name, stable) = (WalletCommand::place_name(src), if dst == "robinhood" { "USDG" } else { "USDC" });
                Some(vec![self.step(&self.label_from(src), &shown, &say(&format!("USDC on {name}"), &format!("USDC в {name}")), self.moved("USDC", src, moving.map(|m| -m), false)),
                          self.step(&say("BURN & MINT · CIRCLE'S CCTP", "СЖЕЧЬ И ВЫПУСТИТЬ · CCTP CIRCLE"), &shown,
                                    &say("≈ 1.3 bp + ≈ $0.08 of gas · Circle mints there", "≈ 0.013% + ≈ $0.08 газа · Circle выпускает там"), None),
                          self.step(&self.label_to(dst), &moving.map_or_else(|| "USDC".into(), |m| format!("≈ {} {stable}", Fmt::num(m, 2))),
                                    &say("your own address ✓", "твой собственный адрес ✓"), moving.and_then(|m| self.moved(stable, dst, Some(m), true)))])
            }
            WalletCommand::Usdt { from, to, amount, layer_zero } => Some(self.usdt(c, from, to, *amount, *layer_zero)),
            WalletCommand::AaveBorrow { .. } | WalletCommand::AaveRepay { .. } | WalletCommand::AaveCollateral { .. } => self.aave_debt(c),
            _ => None,
        }
    }

    /// What pays a stock buy: the USDG on Robinhood Chain, and what's missing brought from Solana, or straight from the
    /// EVM network holding USDC when Solana is short. A few cents short ($1 or 2% at most) isn't worth a bridge: the buy
    /// spends what's there (Executor.buyStock).
    pub fn stock_funding(&self, asked: Decimal) -> StockFunding {
        let (usdg, solana) = (self.usdc_free("robinhood").unwrap_or_default(), self.usdc_free("solana").unwrap_or_default());
        let fits = usdg > Decimal::ZERO && usdg < asked && asked - usdg <= Decimal::ONE.max(asked / Decimal::from(50));
        let usdc = if fits { usdg } else { asked };
        let bring = (usdc - usdg).max(Decimal::ZERO);
        let elsewhere = if bring > Decimal::ZERO { EvmNetwork::source(bring, solana, "robinhood", |k| self.usdc_free(k)) } else { None };
        StockFunding { usdg, solana, fits, usdc, bring, elsewhere }
    }

    /// A Robinhood Stock Token (or a token by its contract) bought on Robinhood Chain: the USDG there pays first, Solana
    /// or the EVM network holding USDC brings the rest over Relay, an exact approve, and the way that gives the most.
    fn stock_buy(&self, s: &str, asked: Decimal) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let ways = self.quotes(&WalletCommand::BuyStock { symbol: s.into(), usdc: Some(asked) });
        let symbol = self.token_name(s, "robinhood");
        let price = if s.starts_with("0x") { self.row_price(s, "robinhood") } else {
            self.mark(&format!("xyz:{s}")).or_else(|| self.mark(&format!("xyz:{}", s.to_uppercase()))).or_else(|| self.row_price(s, "robinhood"))
        };
        let f = self.stock_funding(asked);
        let n = |d: Decimal| Fmt::num(d, 2);
        let mut steps = if f.bring.is_zero() {
            vec![self.step(&say("FROM · ROBINHOOD CHAIN", "ОТКУДА · ROBINHOOD CHAIN"), &format!("{} USDG", n(f.usdc)),
                           &if f.fits { say("what's there · the cents short aren't worth a bridge", "что есть · ради нехватки в центы мост не нужен") }
                            else { say("already there · no bridge", "уже там · без моста") },
                           self.moved("USDG", "robinhood", Some(-f.usdc), false))]
        } else if let Some((net, send)) = f.elsewhere {
            let free = n(self.usdc_free(net.key).unwrap_or_default());
            vec![self.step(&self.label_from(net.key), &format!("≈ {} USDC", n(send)),
                           &say(&format!("Solana has only {} · free there: {free} USDC", n(f.solana)), &format!("на Solana только {} · свободно там: {free} USDC", n(f.solana))),
                           self.moved("USDC", net.key, Some(-send), true))]
        } else {
            let line = if f.usdg > Decimal::ZERO { say(&format!("the rest of {} is on Robinhood · ", n(f.usdc)), &format!("остальное из {} — на Robinhood · ", n(f.usdc))) } else { String::new() }
                + &self.usdc_free("solana").map(|x| say(&format!("free: {} USDC", n(x)), &format!("свободно: {} USDC", n(x)))).unwrap_or_default();
            vec![self.step(&self.label_from("solana"), &format!("{} USDC", n(f.bring)), &line, self.moved("USDC", "solana", Some(-f.bring), false))]
        };
        if f.bring > Decimal::ZERO {
            steps.push(self.step(&say("BRIDGE · RELAY", "МОСТ · RELAY"), &f.elsewhere.map_or_else(|| "USDG → Robinhood".into(), |(net, _)| format!("{} → Robinhood", net.name)),
                                 &(if f.elsewhere.is_none() { String::new() } else { say("straight, no stop on Solana · ", "напрямую, без Solana · ") }
                                   + &say("+ ETH for gas when there's none", "+ ETH на газ, если его нет")),
                                 self.moved("USDG", "robinhood", Some(f.bring), true)));
        }
        let best = ways.as_ref().and_then(|w| w.first()).cloned();
        let best_way = best.as_ref().map(|b| b.way.as_str());
        steps.push(if best_way == Some("v4") {
            self.step(&say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), &format!("≈ {} USDG", n(f.usdc)),
                      &say("exactly this · Permit2 lets Uniswap v4 take it for 30 min", "ровно столько · Permit2 даёт Uniswap v4 взять это на 30 мин"), None)
        } else { self.approve(&format!("≈ {} USDG", n(f.usdc)), if best_way == Some("relay") { "Relay" } else { "Uniswap" }) });
        let mut buy = match (&ways, &best) {
            (Some(ways), Some(best)) => {
                let others: Vec<String> = ways.iter().skip(1).map(|w| format!("{} ≈ {}", stock_way_name(&w.way), w.gets)).collect();
                self.step(&(say("BUY · ", "ПОКУПКА · ") + &stock_way_name(&best.way).to_uppercase() + &say(" · BEST", " · ЛУЧШИЙ")), &symbol,
                          &(format!("≈ {}", best.gets) + &if others.is_empty() { say(" · the only way that quotes it", " · единственный путь с котировкой") } else { format!(" · {}", others.join(" · ")) }),
                          None)
            }
            (Some(_), None) => self.step(&say("BUY · UNISWAP", "ПОКУПКА · UNISWAP"), &symbol, &price.map_or_else(|| say("at the pool price", "по цене пула"), |p| {
                let (coins, usd, one) = (f.usdc.checked_div(p).map_or_else(|| "NaN".into(), |x| Fmt::num(x, 2)), Fmt::usd(f.usdc), Fmt::price(p));
                say(&format!("≈ {coins} {symbol} worth ≈ {usd} · 1 {symbol} = ${one}"), &format!("≈ {coins} {symbol} на ≈ {usd} · 1 {symbol} = ${one}"))
            }), None),
            _ => self.step(&say("BUY · COMPARING", "ПОКУПКА · СРАВНИВАЮ"), &symbol, &say("Uniswap v4, v3 and Relay’s swap…", "Uniswap v4, v3 и обмен Relay…"), None),
        };
        let got = Self::quoted_amount(best.as_ref().map(|b| b.gets.as_str())).or_else(|| price.and_then(|p| if p > Decimal::ZERO { f.usdc.checked_div(p) } else { None }));
        if let Some(got) = got { buy.money = Some(self.gained(s, &symbol, "robinhood", got)) }
        steps.push(buy);
        steps
    }

    /// A stock token sold on Robinhood Chain: approved exactly, sold the way that gives the most, USDG back there.
    fn stock_sale(&self, s: &str, x: Option<Decimal>) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let (name, held) = (self.token_name(s, "robinhood"), self.held_there(s, "robinhood"));
        let amount = x.or(held);
        let price = self.mark(&format!("xyz:{s}")).or_else(|| self.mark(&format!("xyz:{}", s.to_uppercase()))).or_else(|| self.row_price(s, "robinhood"));
        let usd = amount.and_then(|a| price.and_then(|p| a.checked_mul(p)));
        let ways = self.quotes(&WalletCommand::SellStock { symbol: s.into(), amount: x });
        let best = ways.as_ref().and_then(|w| w.first()).cloned();
        let all = || say(&format!("all {name}"), &format!("всё {name}"));
        let mut sell = self.step(
            &(say("SELL · ", "ПРОДАЖА · ") + &best.as_ref().map_or_else(|| "UNISWAP".into(), |b| stock_way_name(&b.way).to_uppercase() + &say(" · BEST", " · ЛУЧШИЙ"))),
            &usd.map(|u| say(&format!("≈ {} of {name}", Fmt::usd(u)), &format!("≈ {} в {name}", Fmt::usd(u))))
                .unwrap_or_else(|| x.map_or_else(all, |x| format!("{} {name}", Fmt::num(x, 6)))),
            &amount.map_or_else(|| "Robinhood Chain".into(), |a| format!("{} {name} · Robinhood Chain", Fmt::num(a, 6))),
            self.moved(s, "robinhood", x.map(|x| -x), false));
        if let (Some(ways), Some(best)) = (&ways, &best) {
            let others: Vec<String> = ways.iter().skip(1).map(|w| format!("{} ≈ {}", stock_way_name(&w.way), w.gets)).collect();
            sell.lines.insert(0, format!("≈ {}", best.gets) + &if others.is_empty() { String::new() } else { format!(" · {}", others.join(" · ")) });
        }
        let back = Self::quoted_amount(best.as_ref().map(|b| b.gets.as_str())).or(usd);
        vec![self.approve(&amount.map_or_else(all, |a| format!("{} {name}", Fmt::num(a, 6))), if best.as_ref().is_some_and(|b| b.way == "relay") { "Relay" } else { "Uniswap" }),
             sell,
             self.step(&say("TO · USDG", "КУДА · USDG"), &back.map_or_else(|| "USDG".into(), |b| format!("≈ {} USDG", Fmt::num(rounded(b, 2, Rounding::Plain), 2))),
                       &say("on Robinhood Chain · then “move USDG from Robinhood to Solana”", "в Robinhood Chain · потом «move USDG from Robinhood to Solana»"),
                       back.and_then(|b| self.moved("USDG", "robinhood", Some(b), true)))]
    }

    /// Where a buy or sale on an EVM network pays from (the Mac's swapRoute): `payer`, another EVM network whose USDC
    /// Relay brings first; `here`, done on the network itself with Uniswap (the USDC there, or brought from that network)
    /// — else Relay's ways from Solana. (payer, here)
    pub fn evm_trade_source(&self, place: &str, usdc: Option<Decimal>, src: Option<&str>, keep: bool, command: &WalletCommand) -> (Option<String>, bool) {
        let quoted = self.quotes(command);
        let picked = way_key(command).and_then(|k| self.st().route_way.as_ref().filter(|r| r.key == k).map(|r| r.way.clone()));
        // Solana short and the network without the USDC: it comes straight from the EVM network holding it (EvmNetwork::source).
        let auto = usdc.and_then(|u| if src.is_none() && picked.is_none() && self.usdc_free(place).unwrap_or_default() < u {
            EvmNetwork::source(u, self.usdc_free("solana").unwrap_or_default(), place, |k| self.usdc_free(k)).map(|(n, _)| n.key.to_string())
        } else { None });
        let chosen = src.map(str::to_string).or(auto);
        let payer = chosen.clone().filter(|p| p != "solana" && p != place);
        let here = keep || chosen.as_deref().is_some_and(|c| c != "solana")
            || usdc.is_some_and(|u| quoted.as_ref().and_then(|q| q.first())
                .map_or_else(|| src != Some("solana") && self.usdc_free(place).unwrap_or_default() >= u, |w| w.way == "here"));
        (payer, here)
    }

    /// A token on an EVM network: bought with the USDC already there (or brought straight from the network holding it),
    /// else with USDC from Solana by Relay's swap or Relay's bridge and Uniswap; sold back to Solana, or for USDC that
    /// stays there (`keep`).
    fn evm_trade(&self, place: &str, token: &str, usdc: Option<Decimal>, sell: Option<Decimal>, src: Option<&str>, keep: bool) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let (name, label) = (WalletCommand::place_name(place), self.token_name(token, place));
        let net = name.to_uppercase();
        let command = match usdc {
            Some(_) => WalletCommand::BuyOn { place: place.into(), token: token.into(), usdc, from: src.map(str::to_string) },
            None => WalletCommand::SellOn { place: place.into(), token: token.into(), amount: sell, keep },
        };
        let quoted = self.quotes(&command);
        let picked = way_key(&command).and_then(|k| self.st().route_way.as_ref().filter(|r| r.key == k).map(|r| r.way.clone()));
        let (payer, here) = self.evm_trade_source(place, usdc, src, keep, &command);
        let moving = usdc.map(|u| format!("{} USDC", Fmt::num(u, 2)))
            .unwrap_or_else(|| sell.map_or_else(|| say(&format!("all {label}"), &format!("всё {label}")), |s| format!("{} {label}", Fmt::num(s, 8))));
        if here {
            let gets = quoted.as_ref().and_then(|q| q.iter().find(|w| w.way == "here")).map(|w| w.gets.clone());
            let mut start = if let Some(payer) = &payer {
                vec![self.step(&self.label_from(payer), &moving, &self.usdc_free(payer).map(|x| say(&format!("free there: {} USDC", Fmt::num(x, 2)), &format!("свободно там: {} USDC", Fmt::num(x, 2)))).unwrap_or_default(),
                               usdc.and_then(|u| self.moved("USDC", payer, Some(-u), false))),
                     self.step(&say("BRIDGE · RELAY", "МОСТ · RELAY"), &format!("USDC → {name}"), &say("a few seconds · no stop on Solana", "несколько секунд · без Solana"),
                               usdc.and_then(|u| self.moved("USDC", place, Some(u), true)))]
            } else if let Some(u) = usdc {
                vec![self.step(&self.label_from(place), &format!("{} USDC", Fmt::num(u, 2)), &say("already there · no bridge", "уже там · без моста"), self.moved("USDC", place, Some(-u), false))]
            } else {
                vec![self.step(&self.label_from(place), &moving, &say(&format!("on {name}"), &format!("в {name}")), self.moved(token, place, sell.map(|s| -s), false))]
            };
            if let Some(u) = usdc {
                start.push(if payer.is_none() { self.approve(&format!("{} USDC", Fmt::num(u, 2)), "Uniswap") } else {
                    self.step(&say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), &format!("{} USDC", Fmt::num(u, 2)), &say("what arrives · only Uniswap may take it", "то, что придёт · взять может только Uniswap"), None)
                });
                start.push(self.step(&(say("BUY · UNISWAP ON ", "ПОКУПКА · UNISWAP В ") + &net), &label, &gets.as_ref().map_or_else(|| say("at the pool price", "по цене пула"), |g| format!("≈ {g}")),
                                     Self::quoted_amount(gets.as_deref()).map(|g| self.gained(token, &label, place, g))));
                return start;
            }
            start.push(self.approve(&moving, "Uniswap"));
            start.push(self.step(&(say("TO · USDC ON ", "КУДА · USDC В ") + &net), &gets.as_ref().map_or_else(|| "USDC".into(), |g| format!("≈ {g}")),
                                 &say(&format!("stays on {name} · your own address ✓"), &format!("остаётся в {name} · твой собственный адрес ✓")),
                                 Self::quoted_amount(gets.as_deref()).and_then(|g| self.moved("USDC", place, Some(g), true))));
            return start;
        }
        // The way picked in the step's details, else the one that leaves more now.
        let way = match picked.as_deref() {
            Some("relay") => self.step(&say("RELAY SWAP", "ОБМЕН RELAY"), &say("one step", "один шаг"),
                                       &if usdc.is_some() { say("Relay swaps on the way · one Solana signature", "Relay меняет по пути · одна подпись в Solana") }
                                        else { say("Relay swaps it back to USDC on Solana", "Relay меняет обратно в USDC на Solana") }, None),
            Some("uniswap") => self.step("RELAY + UNISWAP", &if usdc.is_some() { say("bridge, then Uniswap", "мост, потом Uniswap") } else { say("Uniswap, then bridge", "Uniswap, потом мост") },
                                         &if usdc.is_some() { say(&format!("USDC to {name}, then Uniswap with an exact approve"), &format!("USDC в {name}, потом Uniswap с точным разрешением")) }
                                          else { say(&format!("Uniswap on {name}, then Relay's bridge to Solana"), &format!("Uniswap в {name}, потом мост Relay на Solana")) }, None),
            _ => self.best_way(quoted.as_deref()),
        };
        let first = quoted.as_ref().and_then(|q| q.first());
        let via_uniswap = picked.as_deref().or(first.map(|w| w.way.as_str())) == Some("uniswap");
        let gets = first.map(|w| w.gets.clone());
        if let Some(u) = usdc {
            let mut out = vec![self.step(&self.label_from("solana"), &format!("{} USDC", Fmt::num(u, 2)),
                                         &self.usdc_free("solana").map(|x| say(&format!("free: {} USDC", Fmt::num(x, 2)), &format!("свободно: {} USDC", Fmt::num(x, 2)))).unwrap_or_default(),
                                         self.moved("USDC", "solana", Some(-u), false)), way];
            if via_uniswap {
                out.push(self.step(&say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), &format!("≈ {} USDC", Fmt::num(u, 2)),
                                   &say("exactly what arrives · only Uniswap may take it", "ровно то, что придёт · взять может только Uniswap"), None));
            }
            out.push(self.step(&(say("BUY · ON ", "ПОКУПКА · В ") + &net), &label,
                               &gets.as_ref().map_or_else(|| say("at the best price found", "по лучшей найденной цене"), |g| say(&format!("≈ {g} arrives"), &format!("придёт ≈ {g}"))),
                               Self::quoted_amount(gets.as_deref()).map(|g| self.gained(token, &label, place, g))));
            return out;
        }
        let gas = EvmNetwork::named(place).map_or("ETH", |n| n.gas_coin);
        let mut out = vec![self.step(&self.label_from(place), &moving, &say(&format!("{gas} for gas comes along when there’s none"), &format!("{gas} на газ придёт, если его нет")),
                                     self.moved(token, place, sell.map(|s| -s), false))];
        if via_uniswap { out.push(self.approve(&moving, "Uniswap")) }
        out.push(way);
        out.push(self.step(&say("TO · SOLANA", "КУДА · SOLANA"), "USDC",
                           &gets.as_ref().map_or_else(|| say("your wallet", "твой кошелёк"), |g| say(&format!("≈ {g} · your wallet"), &format!("≈ {g} · твой кошелёк"))),
                           Self::quoted_amount(gets.as_deref()).and_then(|g| self.moved("USDC", "solana", Some(g), true))));
        out
    }

    pub fn way_name(way: &str) -> String { if way == "relay" { "Relay swap" } else { "Relay + Uniswap" }.into() }

    /// The way that leaves more right now and by how much it beats the other; comparing while both are quoted.
    fn best_way(&self, ways: Option<&[Way]>) -> Step {
        let say = |a: &str, b: &str| self.say(a, b);
        let label = say("RELAY OR UNISWAP · COMPARING", "RELAY ИЛИ UNISWAP · СРАВНИВАЮ");
        let Some(ways) = ways else { return self.step(&label, &say("comparing…", "сравниваю…"), &say("Relay swap vs Relay + Uniswap", "обмен Relay или Relay + Uniswap"), None) };
        let Some(best) = ways.first() else { return self.step(&label, &say("no quote now", "котировки нет"), &say("asking again every 30 s", "спрошу снова через 30 с"), None) };
        let other = Self::way_name(if best.way == "relay" { "uniswap" } else { "relay" });
        let dollars = |s: &str| foundation::decimal(&s.chars().filter(|c| c.is_numeric() || *c == '.').collect::<String>());
        let margin = ways.get(1).and_then(|o| dollars(&best.worth).and_then(|b| dollars(&o.worth).and_then(|x| b.checked_sub(x))));
        let line = match margin {
            None => say(&format!("{other} doesn't quote it now"), &format!("{other} сейчас не даёт котировку")),
            Some(m) if m < Decimal::new(1, 2) => say(&format!("about the same as {other}"), &format!("примерно как {other}")),
            Some(m) => say(&format!("{} more than {other}", Fmt::usd(m)), &format!("на {} больше, чем {other}", Fmt::usd(m))),
        };
        self.step(&(Self::way_name(&best.way).to_uppercase() + &say(" · BEST NOW", " · ЛУЧШЕ СЕЙЧАС")), &format!("≈ {}", best.gets), &line, None)
    }

    /// One token for another off Solana (the Mac's pairRoute): NEAR Intents on NEAR, two spot orders on Hyperliquid,
    /// Tempo's exchange (or Relay's swap there), and Uniswap's best on an EVM network; what comes out once quoted.
    fn pair(&self, c: &WalletCommand, place: &str, a: &str, b: &str, x: Option<Decimal>) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let quoted = self.quotes(c);
        let gets = quoted.as_ref().and_then(|q| q.first()).map(|w| format!("≈ {}", w.gets));
        let a_name = self.token_name(a, place);
        let moving = x.map_or_else(|| say(&format!("all {a_name}"), &format!("всё {a_name}")), |x| format!("{} {a_name}", Fmt::num(x, 8)));
        let to_name = self.token_name(b, place);
        let got = Self::quoted_amount(gets.as_deref());
        match place {
            "near" => {
                let near = self.held_there("NEAR", "near");
                Some(vec![self.step(&self.label_from("near"), &moving, &say("your NEAR account", "твой аккаунт NEAR"),
                                    if a.to_uppercase() == "NEAR" { self.moved("NEAR", "near", Some(-x.or(near).unwrap_or_default()), false) } else { None }),
                          self.step(&say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), &to_name, &gets.clone().unwrap_or_else(|| self.quoting()), None),
                          self.step(&self.label_to("near"), &to_name, &say("your NEAR account ✓", "твой аккаунт NEAR ✓"),
                                    if b.to_uppercase() == "NEAR" { got.and_then(|g| self.moved("NEAR", "near", Some(g), true)) } else { None })])
            }
            "hyperliquid" => Some(vec![
                self.step(&say("FROM · SPOT BALANCE", "ОТКУДА · СПОТОВЫЙ БАЛАНС"), &moving,
                          &if x.is_none() { say("all available · on Hyperliquid", "всё, что есть · на Hyperliquid") } else { say("on Hyperliquid", "на Hyperliquid") },
                          self.moved(a, "hyperliquid", x.map(|x| -x), false)),
                self.step(&say("SELL · SPOT ORDER", "ПРОДАЖА · СПОТ"), "→ USDC", &say("IOC · at most 1% under the mid", "IOC · не дешевле середины на 1%"), None),
                self.step(&say("BUY · SPOT ORDER", "ПОКУПКА · СПОТ"), &to_name,
                          &gets.as_ref().map_or_else(|| say("IOC · at most 1% over the mid", "IOC · не дороже середины на 1%"), |g| say(&format!("{g} · at the mids"), &format!("{g} · по серединам"))),
                          got.map(|g| self.gained(b, &to_name, "hyperliquid", g))),
            ]),
            "tempo" => {
                // Where Tempo's exchange has no liquidity for the pair, Relay's swap on Tempo is the way the quote found.
                let relay = quoted.as_ref().and_then(|q| q.first()).is_some_and(|w| w.way == "Relay's swap");
                let label = if relay { say("SWAP · RELAY ON TEMPO", "ОБМЕН · RELAY В TEMPO") } else { say("SWAP · TEMPO'S EXCHANGE", "ОБМЕН · БИРЖА TEMPO") };
                let line = quoted.as_ref().map_or_else(|| self.quoting(), |q| q.first().map_or_else(
                    || say("no liquidity now · asked again every 30 s", "ликвидности сейчас нет · спрошу снова через 30 с"), |w| format!("≈ {}", w.gets)));
                Some(vec![self.step(&self.label_from("tempo"), &moving, &if x.is_none() { say("all available · on Tempo", "всё, что есть · в Tempo") } else { say("on Tempo", "в Tempo") },
                                    self.moved(a, "tempo", x.map(|x| -x), false)),
                          self.approve(&moving, &if relay { "Relay".into() } else { say("Tempo's exchange", "биржа Tempo") }),
                          self.step(&label, &to_name, &line, got.map(|g| self.gained(b, &to_name, "tempo", g)))])
            }
            _ => {
                let net = EvmNetwork::named(place)?;
                let (coin_in, name) = (a.to_uppercase() == net.gas_coin.to_uppercase(), WalletCommand::place_name(place));
                let mut steps = vec![self.step(&self.label_from(place), &moving,
                                               &if coin_in { say(&format!("your {} · its fees stay", net.gas_coin), &format!("твой {} · на комиссии остаётся", net.gas_coin)) }
                                                else { say(&format!("on {name}"), &format!("в {name}")) },
                                               self.moved(a, place, x.map(|x| -x), false))];
                if !coin_in { steps.push(self.approve(&moving, "Uniswap")) }
                let way = quoted.as_ref().and_then(|q| q.first()).map_or("Uniswap", |w| w.way.as_str()).to_uppercase();
                steps.push(self.step(&(say("SWAP · ", "ОБМЕН · ") + &way + &say(" ON ", " В ") + &name.to_uppercase()), &to_name,
                                     &gets.unwrap_or_else(|| say("the best of v2, v3 and v4", "лучший из v2, v3 и v4")), got.map(|g| self.gained(b, &to_name, place, g))));
                Some(steps)
            }
        }
    }

    /// A token on one network for one on another (the Mac's crossRoute): Relay's swap across, with an exact approve for a
    /// token on an EVM network; NEAR Intents for NEAR or a pair only 1Click quotes.
    fn cross(&self, c: &WalletCommand, a: &str, src: &str, b: &str, dst: &str, x: Option<Decimal>) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let quoted = self.quotes(c);
        let first = quoted.as_ref().and_then(|q| q.first()).cloned();
        let a_name = self.token_name(a, src);
        let moving = x.map_or_else(|| say(&format!("all {a_name}"), &format!("всё {a_name}")), |x| format!("{} {a_name}", Fmt::num(x, 8)));
        let (to_name, own) = (self.token_name(b, dst), say("your own address there", "твой собственный адрес там"));
        let line = quoted.as_ref().map_or_else(|| self.quoting(), |q| q.first().map_or_else(|| self.no_quote(), |w| format!("≈ {} · {}", w.gets, w.way)));
        let out = if src == "near" && a.to_uppercase() == "NEAR" {
            self.moved("NEAR", "near", Some(-x.or_else(|| self.held_there("NEAR", "near")).unwrap_or_default()), false)
        } else { self.moved(a, src, x.map(|x| -x), false) };
        let arrives = Self::quoted_amount(first.as_ref().map(|w| w.gets.as_str())).map(|g| self.gained(b, &to_name, dst, g));
        if src == "near" || dst == "near" || first.as_ref().is_some_and(|w| w.way.starts_with("NEAR Intents")) {
            return vec![self.step(&self.label_from(src), &moving, &own, out),
                        self.step(&say("SWAP ACROSS · NEAR INTENTS", "ОБМЕН МЕЖДУ СЕТЯМИ · NEAR INTENTS"), &to_name, &line, None),
                        self.step(&self.label_to(dst), &to_name, &own, arrives)];
        }
        let coin = if src == "solana" { "SOL".into() } else { EvmNetwork::named(src).map_or_else(|| "ETH".into(), |n| n.gas_coin.to_uppercase()) };
        let (coin_in, upper, from) = (a.to_uppercase() == coin, a.to_uppercase(), WalletCommand::place_name(src));
        let mut steps = vec![self.step(&self.label_from(src), &moving,
                                       &if coin_in { say(&format!("your {upper} · its fees stay"), &format!("твой {upper} · на комиссии остаётся")) }
                                        else { say(&format!("on {from}"), &format!("в {from}")) }, out)];
        if src != "solana" && !coin_in { steps.push(self.approve(&moving, "Relay")) }
        steps.push(self.step(&say("SWAP ACROSS · RELAY", "ОБМЕН МЕЖДУ СЕТЯМИ · RELAY"), &to_name, &line, None));
        steps.push(self.step(&self.label_to(dst), &to_name, &own, arrives));
        steps
    }

    /// USDC → native NEAR through NEAR Intents (1Click): from Solana, or straight from the EVM network holding it when
    /// Solana is short — the executor's own rule (Executor::near_source).
    fn near_buy(&self, u: Decimal) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let on_solana = self.usdc_free("solana").unwrap_or_default();
        let src = Executor::near_source(u, on_solana, |k| self.usdc_free(k));
        let key = src.map_or("solana", |n| n.key);
        let near = self.mark("NEAR").and_then(|p| if p > Decimal::ZERO { u.checked_div(p) } else { None });
        vec![self.step(&self.label_from(key), &format!("{} USDC", Fmt::num(u, 2)),
                       &src.map(|n| say(&format!("Solana has only {} · straight from {}, no bridge", Fmt::num(on_solana, 2), n.name),
                                        &format!("на Solana только {} · напрямую из {}, без моста", Fmt::num(on_solana, 2), n.name))).unwrap_or_default(),
                       self.moved("USDC", key, Some(-u), false)),
             self.step(&say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), "1Click", &say("the quote is checked before signing", "котировку проверю перед подписью"), None),
             self.step(&self.label_to("near"), &near.map_or_else(|| "NEAR".into(), |n| format!("≈ {} NEAR", Fmt::num(n, 4))),
                       &say("your own NEAR account ✓", "твой собственный аккаунт NEAR ✓"), near.and_then(|n| self.moved("NEAR", "near", Some(n), true)))]
    }

    /// Native NEAR → USDC through 1Click, on Solana or at our address on another network; «all» keeps ≈ 0.09 NEAR for
    /// storage and prepaid gas, most of it back after the sale.
    fn near_sale(&self, asked: Option<Decimal>, dst: Option<&str>) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let amount = asked.unwrap_or_else(|| self.held_there("NEAR", "near").unwrap_or_default().saturating_sub(Decimal::new(9, 2)).max(Decimal::ZERO));
        let usd = self.mark("NEAR").and_then(|p| amount.checked_mul(p));
        let place = dst.unwrap_or("solana");
        let name = WalletCommand::place_name(place);
        vec![self.step(&self.label_from("near"), &format!("{} NEAR", Fmt::num(amount, 4)),
                       &usd.map(|u| say(&format!("≈ {} · ≈ 0.09 NEAR stays for storage and gas, most of it back after", Fmt::usd(u)),
                                        &format!("≈ {} · ≈ 0.09 NEAR остаётся на хранение и газ, большая часть вернётся", Fmt::usd(u)))).unwrap_or_default(),
                       self.moved("NEAR", "near", Some(-amount), false)),
             self.step(&say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), "1Click", &say("the quote is checked before signing", "котировку проверю перед подписью"), None),
             self.step(&say("ARRIVES · ≈ 30–40 S", "ПРИДЁТ · ≈ 30–40 С"), &format!("→ {name}"), &say("1Click sends the USDC over", "1Click переводит USDC"), None),
             self.step(&self.label_to(place), &usd.map_or_else(|| "USDC".into(), |u| format!("≈ {} USDC", Fmt::num(rounded(u, 2, Rounding::Plain), 2))),
                       &say("your own address ✓", "твой собственный адрес ✓"), usd.and_then(|u| self.moved("USDC", place, Some(u), true)))]
    }

    /// NEAR to a saved NEAR account, or the whole NEAR account closed into it.
    fn near_out(&self, name: &str, asked: Option<Decimal>, close: bool) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let e = self.saved_address(name, "near")?;
        let near = self.held_there("NEAR", "near").unwrap_or_default();
        let amount = if close { near } else { asked.unwrap_or_else(|| near.saturating_sub(Decimal::new(12, 3)).max(Decimal::ZERO)) };
        let usd = self.mark("NEAR").and_then(|p| amount.checked_mul(p));
        let rest = if close { say(" · everything", " · всё") } else if asked.is_none() { say(" · ≈ 0.012 NEAR stays", " · ≈ 0.012 NEAR остаётся") } else { String::new() };
        Some(vec![self.step(&self.label_from("near"), &format!("{} NEAR", Fmt::num(amount, 4)), &usd.map(|u| format!("≈ {}", Fmt::usd(u)) + &rest).unwrap_or_default(),
                            self.moved("NEAR", "near", Some(-amount), false)),
                  self.step(&self.saved_label(), &e.name, &ends(&e.address), None)])
    }

    /// A coin to a saved address on a network only NEAR Intents reaches: the USDC it costs and where from (once quoted),
    /// 1Click's swap for exactly that much, and the saved address. `r` is across_recipient's (entry, symbol, address).
    pub fn across_steps(&self, c: &WalletCommand, r: &(SignerAddress, String, String), amount: Option<Decimal>) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let quoted = self.quotes(c);
        let first = quoted.as_ref().and_then(|q| q.first()).cloned();
        let payer = first.as_ref().map_or("Solana", |w| w.way.as_str()).to_string();
        let key = EvmNetwork::all().iter().find(|n| n.name == payer).map_or("solana", |n| n.key);
        let what = format!("{} {}", amount.map_or_else(|| "?".into(), |a| Fmt::num(a, 9)), r.1);
        vec![self.step(&(say("FROM · ", "ОТКУДА · ") + &payer.to_uppercase()), "USDC",
                       &quoted.as_ref().map_or_else(|| self.quoting(), |q| q.first().map_or_else(|| self.no_quote(),
                                                                                                  |w| say(&format!("{} · the rest comes back", w.gets), &format!("{} · остаток вернётся", w.gets)))),
                       Self::quoted_amount(first.as_ref().map(|w| w.gets.as_str())).and_then(|g| self.moved("USDC", key, Some(-g), true))),
             self.step(&say("SWAP · NEAR INTENTS", "ОБМЕН · NEAR INTENTS"), &what, &say("exactly this much arrives", "придёт ровно столько"), None),
             self.step(&self.saved_label(), &r.0.name, &(ends(&r.0.address) + " · " + &network_title(&r.0.network).map_or_else(|| r.0.network.clone(), str::to_string)), None)]
    }

    /// Where a top-up of Tempo comes from, as the executor's ensure_evm decides (the Mac's tempoSource): None = Solana,
    /// when it holds enough; else the EVM network that does, with what it sends (a dollar over for Relay).
    pub fn tempo_source(&self, need: Decimal) -> Option<(&'static EvmNetwork, Decimal)> {
        EvmNetwork::source(need, self.usdc_free("solana").unwrap_or_default(), "tempo", |k| self.usdc_free(k))
    }

    /// What's short on Tempo for a paid request: a dollar's float and a cent for fees (Executor::TEMPO_FLOAT); None when
    /// enough.
    pub fn tempo_need(&self, price: Decimal) -> Option<Decimal> {
        let float = Decimal::from(Executor::TEMPO_FLOAT) / Decimal::from(1_000_000);
        let (there, cent) = (self.tempo_usdc(), Decimal::new(1, 2));
        if there < (Num::of(price) + cent).0? { (Num::of(price.max(float)) - there + cent).0 } else { None }
    }

    /// A paid request (MPP): USDC on Tempo (a dollar's float first when short, from Solana or the network holding it),
    /// the payment, the service.
    fn payment(&self, c: &WalletCommand) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let WalletCommand::Pay { url, .. } = c else { return None };
        let q = self.st().payment.clone().filter(|q| &q.command == c)?;
        let ch = &q.challenge;
        let usdc = ch.usdc();
        let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| ch.realm().into());
        let price = Fmt::num(usdc, 6);
        let there = self.tempo_usdc();
        let float = Decimal::from(Executor::TEMPO_FLOAT) / Decimal::from(1_000_000);
        let first = if let Some(need) = self.tempo_need(usdc) {
            let requests = (Num::of(usdc.max(float)) / usdc).rounded(0, Rounding::Down).0.and_then(|r| r.to_i64()).unwrap_or(0);
            let src = self.tempo_source(need);
            let (key, name, sends) = src.map_or(("solana", "Solana", need), |(n, s)| (n.key, n.name, s));
            let mut first = self.step(&(say("FROM · ", "ОТКУДА · ") + &name.to_uppercase() + " → TEMPO"), &format!("{} USDC", Fmt::num(sends, 6)),
                                      &say(&format!("a float for ≈ {requests} requests like this · Relay ≈ $0.02"), &format!("запас на ≈ {requests} таких запросов · Relay ≈ $0.02")),
                                      self.moved("USDC", key, Some(-sends), false));
            first.lines.push(self.usdc_free(key).map_or_else(|| say("from your Solana USDC", "из твоих USDC на Solana"),
                                                             |x| say(&format!("{} USDC on {name}", Fmt::num(x, 2)), &format!("{} USDC в {name}", Fmt::num(x, 2)))));
            first
        } else {
            self.step(&self.label_from("tempo"), &format!("{price} USDC"), &say("your USDC on Tempo", "твои USDC в Tempo"),
                      Some(Money { place: "Tempo".into(), symbol: "USDC".into(), was: Self::amount(there, "USDC"),
                                   after: Self::amount(there.saturating_sub(usdc).max(Decimal::ZERO), "USDC") }))
        };
        Some(vec![first, self.step(&say("PAY · MPP", "ОПЛАТА · MPP"), &format!("{price} USDC"), &ch.purpose().map_or_else(|| say("one request", "один запрос"), str::to_string), None),
                  self.step(&(say("TO · ", "КУДА · ") + &q.service.to_uppercase()), &say("the answer comes here", "ответ придёт сюда"), &host, None)])
    }

    /// A multisig made: its keys checked, then one Safe per network at the same address.
    fn multisig_creation(&self) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let d = self.st().multisig_draft.clone()?;
        let plain = |label: String, title: String, lines: Vec<String>| Step { label, title, lines, money: None, status: None };
        let check = plain(say("CHECK · KEYS", "ПРОВЕРКА · КЛЮЧИ"), say(&format!("{} of {} keys", d.threshold, d.keys.len()), &format!("{} из {} ключей", d.threshold, d.keys.len())),
                          d.keys.iter().map(|k| format!("{}\t{}", k.name, short(&k.address, 6, 4))).collect());
        Some(std::iter::once(check).chain(d.networks.iter().filter_map(|n| EvmNetwork::named(n)).map(|net| plain(
            say("MULTISIG · ", "МУЛЬТИСИГ · ") + &net.name.to_uppercase(), say(&format!("Create on {}", net.name), &format!("Создать в {}", net.name)),
            vec![say("address\t", "адрес\t") + &d.address.as_deref().map_or_else(|| "—".into(), |a| short(a, 6, 4)),
                 say("Safe 1.4.1 · fee from this computer", "Safe 1.4.1 · комиссия с этого компьютера")]))).collect())
    }

    /// A payment out of a multisig: each key here signs it, then this computer sends it and pays the gas.
    fn multisig_sending(&self, c: &WalletCommand) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let p = self.st().multisig_payment.clone().filter(|p| &p.command == c)?;
        let net = EvmNetwork::named(&p.net)?;
        let amount = format!("{} {}", Fmt::num(p.amount, 6), p.symbol);
        let signs = p.signers.iter().enumerate().map(|(i, k)| Step {
            label: say("SIGN · ", "ПОДПИСЬ · ") + &if k.kind == "ledger" { "LEDGER".into() } else { say("THIS COMPUTER", "ЭТОТ КОМПЬЮТЕР") },
            title: say(&format!("Signature {} of {}", i + 1, p.threshold), &format!("Подпись {} из {}", i + 1, p.threshold)),
            lines: vec![say("to\t", "кому\t") + &p.to.name, say("amount\t", "сумма\t") + &amount, say("network\t", "сеть\t") + net.name], money: None, status: None,
        });
        let multisig = short(&p.multisig.address, 6, 4);
        let to = Step {
            label: say("TO · ", "КУДА · ") + &p.to.name.to_uppercase(), title: format!("{amount} → {}", p.to.name),
            lines: vec![say(&format!("from the multisig {multisig}"), &format!("из мультисига {multisig}")), say("gas from this computer", "газ с этого компьютера")],
            // The multisig's own balance going down, as the Mac's card shows it.
            money: Some(Money { place: say("Multisig", "Мультисиг"), symbol: p.symbol.clone(), was: Self::amount(p.held, &p.symbol),
                                after: Self::amount(p.held.saturating_sub(p.amount).max(Decimal::ZERO), &p.symbol) }),
            status: None,
        };
        Some(signs.chain(std::iter::once(to)).collect())
    }

    pub fn bridge_name(way: &str) -> String {
        match way { "relay" => "Relay", "cctp" => "CCTP", "1click" => "NEAR Intents", "layerzero" => "LayerZero", _ => way }.into()
    }

    /// The bridge step of a move between networks once its bridges are quoted: the one it goes by with what it lands,
    /// then a row for each bridge — what it costs and how long it takes. `fallback` (label, line) until then.
    pub fn bridge_step(&self, ways: Option<&[Way]>, title: &str, fallback: (&str, &str)) -> Step {
        let Some((taken, ways)) = ways.and_then(|w| w.first().map(|t| (t, w))) else { return self.step(fallback.0, title, fallback.1, None) };
        let parts = |w: &Way| w.gets.split(" · ").map(str::to_string).collect::<Vec<_>>();
        Step {
            label: self.say("BRIDGE · ", "МОСТ · ") + &Self::bridge_name(&taken.way).to_uppercase(),
            title: parts(taken).first().cloned().unwrap_or_else(|| title.into()),
            lines: ways.iter().map(|w| format!("{}\t{}", Self::bridge_name(&w.way), parts(w)[1..].join(" · ")))
                .chain([self.say("prices now · asked again before signing", "цены сейчас · перед подписью спрошу снова")]).collect(),
            money: None, status: None,
        }
    }

    /// USDT from one of USDT0's networks to another, both our own address: Relay's swap of it, or Tether's own bridge on
    /// LayerZero when asked by name.
    fn usdt(&self, c: &WalletCommand, src: &str, dst: &str, x: Option<Decimal>, layer_zero: bool) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let deployment = |p: &str| EvmNetwork::named(p).and_then(|n| usdt0::on(n.rpc.chain_id));
        let token = |p: &str| deployment(p).map(|d| d.token);
        let have = token(src).and_then(|t| self.held_there(t, src)).or_else(|| self.held_there("USDT", src));
        let moving = x.map(|x| x.min(have.unwrap_or(x))).or(have);
        let shown = moving.map_or_else(|| say("all USDT", "всё USDT"), |m| format!("{} USDT", Fmt::num(m, 2)));
        let wait = deployment(src).map_or_else(|| say("a while", "какое-то время"), |d| usdt0::wait(d.minutes));
        let quoted = self.quotes(c);
        let name = WalletCommand::place_name(src);
        let fallback = if layer_zero {
            (say("BRIDGE · LAYERZERO", "МОСТ · LAYERZERO"), say(&format!("Tether's USDT0 · cents of gas · {wait}"), &format!("USDT0 от Tether · центы газа · {wait}")))
        } else { (say("BRIDGE · RELAY", "МОСТ · RELAY"), say("a few cents · seconds", "несколько центов · секунды")) };
        let lands = quoted.as_ref().and_then(|q| q.first()).map(|w| w.gets.split(" · ").next().unwrap_or("").to_string())
            .or_else(|| moving.map(|m| format!("≈ {} USDT", Fmt::num(m, 2)))).unwrap_or_else(|| "USDT".into());
        vec![self.step(&self.label_from(src), &shown, &say(&format!("USDT on {name}"), &format!("USDT в {name}")), self.moved(token(src).unwrap_or("USDT"), src, moving.map(|m| -m), false)),
             self.bridge_step(quoted.as_deref(), &shown, (&fallback.0, &fallback.1)),
             self.step(&self.label_to(dst), &lands, &say("your own address ✓", "твой собственный адрес ✓"),
                       moving.and_then(|m| self.moved(token(dst).unwrap_or("USDT"), dst, Some(m), !layer_zero)))]
    }

    /// Borrowing on Aave, paying it back, or ETH supplied as its collateral: the collateral, the move with the health
    /// factor before and after (once quoted), where the money ends.
    fn aave_debt(&self, c: &WalletCommand) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let (x, net) = match c {
            WalletCommand::AaveBorrow { amount, network } | WalletCommand::AaveRepay { amount, network } | WalletCommand::AaveCollateral { amount, network } => (*amount, network),
            _ => return None,
        };
        let (name, what) = (WalletCommand::place_name(net), x.map_or_else(|| say("all", "всё"), |x| Fmt::num(x, 6)));
        let quotes = self.quotes(c);
        let quoted = |way: &str| quotes.as_ref().and_then(|q| q.iter().find(|w| w.way == way)).map(|w| w.gets.clone());
        Some(match c {
            WalletCommand::AaveRepay { .. } => vec![
                self.step(&self.label_from(net), &format!("{what} USDC"), &say(&format!("USDC on {name}"), &format!("USDC в {name}")), self.moved("USDC", net, x.map(|x| -x), false)),
                self.approve(&format!("{what} USDC"), "Aave"),
                self.step(&say("REPAY · AAVE", "ВОЗВРАТ · AAVE"), &format!("{what} USDC"), &say("the debt goes down, the collateral stays", "долг уменьшается, залог остаётся"), None),
            ],
            WalletCommand::AaveCollateral { .. } => vec![
                self.step(&self.label_from(net), &format!("{what} ETH"), &say("its fees stay", "на комиссии остаётся"), self.moved("ETH", net, x.map(|x| -x), false)),
                self.step(&say("WRAP · WETH", "ОБЁРТКА · WETH"), &format!("{what} WETH"), &say("the coin as a token, one to one", "монета как токен, один к одному"), None),
                self.step(&say("COLLATERAL · AAVE", "ЗАЛОГ · AAVE"), &format!("{what} ETH"), &say("borrowable against, ≈ 80% of its value", "под него можно занять ≈ 80% стоимости"), None),
            ],
            _ => vec![
                self.step(&say("COLLATERAL · AAVE", "ЗАЛОГ · AAVE"), &quoted("collateral").unwrap_or_else(|| say("reading…", "читаю…")),
                          &say(&format!("what you supplied on Aave on {name}"), &format!("что ты внёс в Aave в {name}")), None),
                self.step(&say("BORROW · AAVE", "ЗАЁМ · AAVE"), &format!("{what} USDC"),
                          &quoted("health").map_or_else(|| say("health factor: reading…", "фактор здоровья: читаю…"), |h| say(&format!("health factor {h}"), &format!("фактор здоровья {h}"))), None),
                self.step(&self.label_to(net), &format!("{what} USDC"), &say("your own address", "твой собственный адрес"), x.and_then(|x| self.moved("USDC", net, Some(x), false))),
            ],
        })
    }

    /// Merkl's rewards claimed: each reward by network, the wallet it lands in; a sweep then sells each worth a dollar.
    pub fn claim_steps(&self, owed: &[Reward], c: &WalletCommand) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let chain_name = |id: u64| EvmNetwork::of(id).map_or_else(|| format!("chain {id}"), |n| n.name.into());
        let mut steps = vec![];
        for chain in merkl::CHAINS.into_iter().filter(|chain| owed.iter().any(|r| r.chain_id == *chain)) {
            let (net, name) = (EvmNetwork::of(chain), chain_name(chain));
            for r in owed.iter().filter(|r| r.chain_id == chain) {
                steps.push(Step {
                    label: say("CLAIM · MERKL ON ", "ЗАБРАТЬ · MERKL В ") + &name.to_uppercase(), title: format!("{} {}", Fmt::num(r.claimable(), 6), r.symbol),
                    lines: r.worth_usd().map(|w| say("worth\t≈ ", "стоит\t≈ ") + &Fmt::usd(w)).into_iter().chain([say("from Merkl's distributor", "из распределителя Merkl")]).collect(),
                    money: net.map(|n| self.gained(&r.token, &r.symbol, n.key, r.claimable())), status: None,
                });
            }
            steps.push(self.step(&(say("TO · ", "КУДА · ") + &name.to_uppercase()), &say("this wallet", "этот кошелёк"), &say("the rewards stay there as tokens", "награды остаются там токенами"), None));
        }
        if let WalletCommand::SweepRewards { into, .. } = c {
            for r in merkl::to_sell(owed, into.as_deref()) {
                let name = chain_name(r.chain_id);
                steps.push(Step {
                    label: say("SELL · ON ", "ПРОДАЖА · В ") + &name.to_uppercase(), title: format!("{} {}", Fmt::num(r.claimable(), 6), r.symbol),
                    lines: std::iter::once(say("for USDC\ton Solana", "за USDC\tна Solana"))
                        .chain(into.as_ref().map(|i| say(&format!("then buys\t{i} on {name}"), &format!("потом покупает\t{i} в {name}")))).collect(),
                    money: None, status: None,
                });
            }
        }
        steps
    }

    /// USDC to a saved address on its network, as every other transfer is drawn.
    pub fn transfer_steps(&self, e: &SignerAddress, amount: Decimal) -> Vec<Step> {
        let say = |a: &str, b: &str| self.say(a, b);
        let net = e.network.as_str();
        let free = self.usdc_free(net);
        let line = free.map(|f| if f < amount && net != "solana" {
            let short = Fmt::num(amount.saturating_sub(f), 2);
            say(&format!("{short} USDC short: it comes from Solana first"), &format!("не хватает {short} USDC: сначала придут с Solana"))
        } else if net == "hyperliquid" { say("perp balance · no bridge, no gas", "баланс perp · без моста и газа") } else {
            say(&format!("free: {} USDC", Fmt::num(f, 2)), &format!("свободно: {} USDC", Fmt::num(f, 2)))
        }).unwrap_or_default();
        // What Solana gives when the money there is short (the Mac's `bridged`): its balance going down, a second row in
        // the same box under this network's.
        let short = amount.saturating_sub(free.unwrap_or(amount)).max(Decimal::ZERO);
        let top_up = if net != "solana" && short > Decimal::ZERO && self.usdc_free("solana").unwrap_or_default() >= short { self.moved("USDC", "solana", Some(-short), true) } else { None };
        let mut from = self.step(&(say("FROM · ", "ОТКУДА · ") + &net_title(net).to_uppercase()), &format!("{} USDC", Fmt::num(amount, 2)), &line,
                                 self.moved("USDC", net, Some(-amount), false));
        if let Some(m) = top_up { from.lines.push(format!("{}\t{} → {} {}", m.place, m.was, m.after, m.symbol)) }
        vec![from, self.step(&self.saved_label(), &e.name, &ends(&e.address), None)]
    }

    /// A position closed (all or a share): what it makes or loses now, its size and entry, the USDC back on Hyperliquid.
    pub fn close_steps(&self, share: Decimal, coin: &str) -> Option<Vec<Step>> {
        let say = |a: &str, b: &str| self.say(a, b);
        let (p, now) = {
            let st = self.st();
            (st.positions.iter().find(|p| p.coin == coin).cloned()?, st.markets.get(coin).map(|m| m.mark_px))
        };
        let name = coin.strip_prefix("xyz:").unwrap_or(coin);
        let (long, pnl) = (p.size > Decimal::ZERO, p.unrealized_pnl.checked_mul(share).unwrap_or_default());
        let size = p.size.abs().checked_mul(share).unwrap_or_default();
        Some(vec![Step {
            label: say("CLOSE · ", "ЗАКРЫТИЕ · ") + &name.to_uppercase() + " " + &if long { say("LONG", "ЛОНГ") } else { say("SHORT", "ШОРТ") },
            title: format!("{}{}", if pnl < Decimal::ZERO { "−" } else { "+" }, Fmt::usd(pnl.abs())),
            lines: [say("share\t", "доля\t") + &Fmt::pct(share.saturating_mul(Decimal::ONE_HUNDRED)), say("size\t", "размер\t") + &format!("{} {name}", Fmt::num(size, 6)),
                    say("entry\t", "вход\t") + &Fmt::price(p.entry)].into_iter()
                .chain(now.map(|m| say("now\t≈ ", "сейчас\t≈ ") + &Fmt::price(m))).collect(),
            money: None, status: None,
        }, self.step(&say("TO · HYPERLIQUID", "КУДА · HYPERLIQUID"), "USDC", &say("back to the trading balance", "обратно на торговый баланс"), None)])
    }

    // MARK: The footer under each route

    /// What the route signs and how it's checked, from what the route does now (the Mac's MoveRoute footers,
    /// Canvases.swift): where the money comes from, the way the quote took, a coin going in as itself. The rest as
    /// route_edit's `note` says it. With «One transaction for approve and swap» on, a footer that counts an approve says
    /// the two go out as one (the Mac's MoveRoute.init).
    pub fn route_note(&self, c: &WalletCommand) -> Option<String> {
        let note = self.own_note(c).or_else(|| route_edit::note(c, |a, b| self.say(a, b)))?;
        Some(if Executor::batches_on() && (note.contains("approve") || note.contains("разрешени")) {
            note + &self.say(" With «One transaction for approve and swap» on, the approve and what spends it go out as one transaction under one Windows Hello.",
                             " Включена «Одна транзакция для approve и обмена»: разрешение и то, что его тратит, уйдут одной транзакцией под одним Windows Hello.")
        } else { note })
    }

    fn own_note(&self, c: &WalletCommand) -> Option<String> {
        let say = |a: &str, b: &str| self.say(a, b);
        let name = WalletCommand::place_name;
        let confirm = say(" Press Confirm.", " Нажми «Подтвердить».");
        match c {
            WalletCommand::ClaimRewards | WalletCommand::SweepRewards { .. } => {
                let owed = {
                    let st = self.st();
                    st.claim_owed.as_ref().filter(|o| &o.command == c && st.card.as_ref().map(|c| &c.id) == Some(&o.board)).map(|o| o.owed.clone())?
                };
                Some(self.claim_note(&owed, c))
            }
            WalletCommand::BuyOn { place, usdc: Some(u), from, .. } => self.here_note(c, place, Some(*u), from.as_deref(), false),
            WalletCommand::SellOn { place, keep, .. } => self.here_note(c, place, None, None, *keep),
            WalletCommand::BuyStock { symbol, usdc: Some(u) } => {
                let f = self.stock_funding(*u);
                let start = if f.bring.is_zero() {
                    say("Two signatures on Robinhood Chain with the USDG already there: ", "Две подписи в Robinhood Chain с USDG, которые уже там: ")
                } else if let Some((net, _)) = f.elsewhere {
                    say(&format!("An exact approve and Relay's deposit on {}, then two on Robinhood Chain: ", net.name),
                        &format!("Точное разрешение и депозит Relay в {}, потом две в Robinhood Chain: ", net.name))
                } else {
                    say("One Solana signature for the bridge, then two on Robinhood Chain: ", "Одна подпись в Solana для моста, потом две в Robinhood Chain: ")
                };
                Some(start
                    + &say("an exact USDG approve and the swap the best way takes (Uniswap v4, v3 or Relay’s swap, compared in the step), which may only pay this wallet. ",
                           "точное разрешение на USDG и обмен лучшим путём (Uniswap v4, v3 или обмен Relay — сравнение в шаге), который может платить только этому кошельку. ")
                    + &if symbol.starts_with("0x") {
                        say("The token is taken by its contract: it must trade on Robinhood Chain with $50k+ of liquidity, and the price must sit within 5% of DexScreener's.",
                            "Токен взят по контракту: он должен торговаться в Robinhood Chain с ликвидностью от $50 тыс., а цена — не дальше 5% от DexScreener.")
                    } else {
                        say("The token is Robinhood's registered contract, checked on-chain, and the price must sit within Robinhood's own quote.",
                            "Токен — зарегистрированный контракт Robinhood, проверенный в сети, а цена должна быть в пределах собственной котировки Robinhood.")
                    }
                    + &confirm)
            }
            WalletCommand::SwapOn { place, .. } if place == "tempo" => {
                let relay = self.quotes(c).and_then(|q| q.into_iter().next()).is_some_and(|w| w.way == "Relay's swap");
                Some(say(&format!("An exact approve to {}, then the swap at no less than {}, paying only this wallet; Tempo's fees come out of the USDC, a tenth of a cent each.",
                                  if relay { "Relay's swap on Tempo" } else { "Tempo's own exchange" }, if relay { "Relay's checked quote" } else { "0.1% under its quote" }),
                         &format!("Точное разрешение для {}, потом обмен не хуже {}, платит только этому кошельку; комиссии Tempo — из USDC, по десятой цента.",
                                  if relay { "обмена Relay в Tempo" } else { "собственной биржи Tempo" }, if relay { "проверенной котировки Relay" } else { "котировки на 0,1%" })) + &confirm)
            }
            WalletCommand::SwapOn { place, from: a, to: b, .. } if !["solana", "near", "hyperliquid"].contains(&place.as_str()) => {
                let net = EvmNetwork::named(place)?;
                let coin = net.gas_coin;
                let (coin_in, coin_out, n) = (a.to_uppercase() == coin.to_uppercase(), b.to_uppercase() == coin.to_uppercase(), name(place));
                Some(say(&format!("One swap on {n}, no USDC in the middle: the best of Uniswap v2, v3 and v4, at most 0.5–3% under its quote by the price impact, paying only this wallet."),
                         &format!("Один обмен в {n}, без USDC посередине: лучший из Uniswap v2, v3 и v4, не хуже котировки больше чем на 0,5–3% по влиянию на цену, платит только этому кошельку."))
                    + &if coin_in { say(&format!(" {coin} goes in as itself into a v4 pool of it, else wrapped into W{coin} first (one more transaction)."),
                                        &format!(" {coin} идёт как есть в пул v4 с ним, иначе сначала оборачивается в W{coin} (ещё одна транзакция).")) }
                       else { say(" An exact approve first.", " Сначала точное разрешение.") }
                    + &if coin_out { say(&format!(" {coin} comes out as itself, or is unwrapped after."), &format!(" {coin} выходит как есть или разворачивается после.")) } else { String::new() }
                    + &confirm)
            }
            WalletCommand::CrossSwap { from: a, on: src, to: b, at: dst, .. } => {
                let (from, to, label) = (name(src), name(dst), self.token_name(b, dst));
                if src == "near" || dst == "near" || self.quotes(c).and_then(|q| q.into_iter().next()).is_some_and(|w| w.way.starts_with("NEAR Intents")) {
                    return Some(say(&format!("One deposit on {from} to the 1Click address NEAR Intents binds to this quote — checked with 1Click before signing; that address needs Windows Hello the first time — and {label} comes to your own address on {to}; a refund comes back to {from}."),
                                    &format!("Один депозит в {from} на адрес 1Click, который NEAR Intents привязывает к этой котировке, — сверяется с 1Click перед подписью; этому адресу в первый раз нужен Windows Hello, — и {label} придёт на твой же адрес в {to}; возврат — обратно в {from}.")) + &confirm);
                }
                let coin = if src == "solana" { "SOL".into() } else { EvmNetwork::named(src).map_or_else(|| "ETH".into(), |n| n.gas_coin.to_uppercase()) };
                let (coin_in, upper) = (a.to_uppercase() == coin, a.to_uppercase());
                Some(say(&format!("One signature on {from}: Relay's solver swaps on the way and pays {label} to your own address on {to}, usually within seconds. If more than 3% (or 15 cents) of the value would be lost, nothing is signed."),
                         &format!("Одна подпись в {from}: решатель Relay обменивает по пути и платит {label} на твой же адрес в {to}, обычно за секунды. Если потерялось бы больше 3% (или 15 центов) стоимости, ничего не подписывается."))
                    + &if src != "solana" && coin_in { say(&format!(" The {upper} goes to Relay's depository: that address needs Windows Hello the first time."), &format!(" {upper} уходит в хранилище Relay: этому адресу в первый раз нужен Windows Hello.")) } else { String::new() }
                    + &if src != "solana" && !coin_in { say(" An exact approve to Relay comes first, in the same transaction where the network allows.", " Сначала точное разрешение для Relay, в той же транзакции, где сеть это умеет.") } else { String::new() }
                    + &confirm)
            }
            WalletCommand::SendToken { name: who, token, amount: Some(x), network } => {
                let (entry, symbol, _) = self.across_recipient(who, token, network.as_deref()).ok().flatten()?;
                let (place, what) = (net_title(&entry.network), format!("{} {symbol}", Fmt::num(*x, 8)));
                Some(say(&format!("NEAR Intents' 1Click turns USDC into {symbol} on {place} and pays this saved address exactly {what}; what's left of the USDC comes back. Its deposit address is fresh: it needs Windows Hello before anything is sent."),
                         &format!("1Click от NEAR Intents превращает USDC в {symbol} в {place} и платит этому сохранённому адресу ровно {what}; остаток USDC возвращается. Его адрес депозита новый: перед отправкой нужен Windows Hello.")) + &confirm)
            }
            WalletCommand::Pay { url, .. } => {
                let q = self.st().payment.clone().filter(|q| &q.command == c)?;
                let ch = &q.challenge;
                let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| ch.realm().into());
                let (price, (start, end), need) = (Fmt::num(ch.usdc(), 6), address_check::ends(&ch.payee), self.tempo_need(ch.usdc()));
                let src = need.and_then(|n| self.tempo_source(n)).map_or("Solana", |(n, _)| n.name);
                Some(say(&format!("One Tempo transfer of {price} USDC to the address {host} named ({start} … {end}); "), &format!("Один перевод в Tempo на {price} USDC по адресу, который назвал {host} ({start} … {end}); "))
                    + &if need.is_some() { say(&format!("what's short comes from {src} over Relay first; "), &format!("недостающее сначала придёт из {src} через Relay; ")) } else { String::new() }
                    + &say("the fee, ≈ $0.001, comes out of the USDC. Then the request goes again with the payment's hash.", "комиссия ≈ $0,001 — из этих USDC. Потом запрос уходит снова с хешем платежа.") + &confirm)
            }
            WalletCommand::Aave { deposit: true, amount: Some(x), network: net } | WalletCommand::Morpho { deposit: true, amount: Some(x), network: net } => {
                // The Mac's lendingRoute footer: Solana's part when the USDC or the gas there is short, and Ethereum's gas said.
                let morpho = matches!(c, WalletCommand::Morpho { .. });
                let (n, coin) = (name(net), EvmNetwork::named(net).map_or("ETH", |n| n.gas_coin));
                let short = self.usdc_free(net).unwrap_or_default() < *x || self.gas_low(net);
                let (x, what, read) = (Fmt::num(*x, 2), if morpho { "deposit; the shares come to this wallet only" } else { "supply" },
                                       if morpho { "депозит — доли приходят только на этот кошелёк" } else { "вклад" });
                if net == "base" {
                    return Some(if short { say("One Solana signature for the bridge, then two", "Одна подпись в Solana для моста, потом две") } else { say("Two", "Две") }
                        + &say(&format!(" on Base: approve exactly {x} USDC, then {what}. {} re-read from Base first.", if morpho { "The vault’s asset is" } else { "Pool and rate are" }),
                               &format!(" транзакции в Base: разрешение ровно на {x} USDC, потом {read}. {} сначала перечитываются из Base.", if morpho { "Актив хранилища" } else { "Пул и ставка" }))
                        + &confirm);
                }
                Some(if short {
                    say(&format!("What's missing on {n} comes from Solana over Relay, with {coin} for gas when there's none; then two"),
                        &format!("Недостающее на {n} придёт с Solana через Relay, с {coin} на газ, если его нет; потом две"))
                } else { say("Two", "Две") }
                    + &say(&format!(" {n} transactions: approve exactly {x} USDC, then {what}. {} re-read from {n} first", if morpho { "The vault's asset is" } else { "Pool and aToken are" }),
                           &format!(" транзакции в {n}: разрешение ровно на {x} USDC, потом {read}. {} сначала перечитываются из {n}", if morpho { "Актив хранилища" } else { "Пул и aToken" }))
                    + &if net == "ethereum" {
                        say("; Ethereum's gas changes by the hour (cents to dollars): ask the chat «how much is gas?» for today's",
                            "; газ в Ethereum меняется каждый час (от центов до долларов): спроси в чате «сколько стоит газ?», чтобы узнать сегодняшний")
                    } else { String::new() }
                    + "." + &confirm)
            }
            WalletCommand::MultisigCreate { .. } => Some(say("One transaction per network from this computer to Safe's factory (Safe 1.4.1, the same contracts and the same address on each). The keys and how many must sign are set in it; ihatecrypto never changes them.",
                                                             "Одна транзакция на сеть с этого компьютера на фабрику Safe (Safe 1.4.1, те же контракты и тот же адрес в каждой). Ключи и сколько их нужно для подписи задаются в ней; ihatecrypto их никогда не меняет.") + &confirm),
            WalletCommand::MultisigSend { .. } => {
                let p = self.st().multisig_payment.clone().filter(|p| &p.command == c)?;
                let net = EvmNetwork::named(&p.net)?;
                Some(say(&format!("Each key signs the same move: a plain payment, money only to {}. Then this computer sends it with the signatures and pays the gas; the multisig needs no {}.", p.to.name, net.gas_coin),
                         &format!("Каждый ключ подписывает одно и то же: обычный платёж, деньги только {}. Потом этот компьютер отправляет его с подписями и платит газ; мультисигу {} не нужен.", p.to.name, net.gas_coin)) + &confirm)
            }
            WalletCommand::ContractCall(_) => {
                let (function, net, approve) = {
                    let st = self.st();
                    let r = st.call_review.as_ref().filter(|r| &r.command == c)?;
                    (r.call.function.clone(), r.net.name, r.approve.is_some())
                };
                Some(say(&format!("Encoded by ihatecrypto from {function}, simulated on {net}"), &format!("Закодировано ihatecrypto из {function}, симуляция в {net}"))
                    + &if approve { say(" with its exact approve", " с точным разрешением") } else { String::new() }
                    + &say("; the signer reads it again and asks for a new spender in person.", "; подписант читает его снова и о новом получателе разрешения спрашивает лично.") + &confirm)
            }
            _ => None,
        }
    }

    /// A buy or sale done on the EVM network itself (the USDC there, or brought from another EVM network by Relay): its
    /// Uniswap footer (the Mac's tokenRoute); None for Relay's ways from Solana, which route_edit's note says.
    fn here_note(&self, c: &WalletCommand, place: &str, usdc: Option<Decimal>, src: Option<&str>, keep: bool) -> Option<String> {
        let (payer, here) = self.evm_trade_source(place, usdc, src, keep, c);
        if !here { return None }
        let say = |a: &str, b: &str| self.say(a, b);
        let n = WalletCommand::place_name(place);
        Some(payer.map_or_else(|| say("Two", "Две"), |p| {
            let p = WalletCommand::place_name(&p);
            say(&format!("First Relay moves the USDC from {p} to {n}: an exact approve and its deposit. Then two"),
                &format!("Сначала Relay перевезёт USDC из {p} в {n}: точное разрешение и его депозит. Потом две"))
        })
            + &say(&format!(" {n} transactions: an exact approve and the Uniswap swap, which may only pay this wallet"),
                   &format!(" транзакции в {n}: точное разрешение и обмен в Uniswap, который может платить только этому кошельку"))
            + &if usdc.is_none() { say(&format!("; the USDC stays on {n}."), &format!("; USDC остаются в {n}.")) } else { ".".into() }
            + &say(" Press Confirm.", " Нажми «Подтвердить»."))
    }

    /// Merkl's claim or sweep (the Mac's claimRoute footer), once what's owed is read: one transaction a network.
    pub fn claim_note(&self, owed: &[Reward], c: &WalletCommand) -> String {
        let mut names: Vec<String> = owed.iter().map(|r| EvmNetwork::of(r.chain_id).map_or_else(|| format!("chain {}", r.chain_id), |n| n.name.into()))
            .collect::<HashSet<_>>().into_iter().collect();
        names.sort();
        let names = names.join(", ");
        let sweep = matches!(c, WalletCommand::SweepRewards { .. });
        self.say(&format!("One {names} transaction a chain: Merkl's distributor pays each reward to this wallet — the signer refuses a claim that pays anyone else. "),
                 &format!("Одна транзакция на сеть ({names}): распределитель Merkl платит каждую награду этому кошельку — подписант откажет в получении, которое платит кому-то ещё. "))
            + &if sweep {
                self.say("Then each reward worth $1 or more is sold for USDC on Solana (Relay's swap or Uniswap and the bridge, whichever leaves more), paying only this wallet; dust stays. ",
                         "Потом каждая награда от $1 продаётся за USDC на Solana (обмен Relay или Uniswap и мост — что оставит больше), только на этот кошелёк; мелочь остаётся. ")
            } else { String::new() }
            + &self.say("The proofs are asked for again right before signing; gas missing there comes from Solana over Relay. Press Confirm.",
                        "Доказательства запрашиваются снова прямо перед подписью; недостающий газ придёт с Solana через Relay. Нажми «Подтвердить».")
    }

    /// The save card's line (the Mac's PendingAddressBoard): taken from the message, or resolved from a domain just now.
    pub fn address_note(&self, e: &SignerAddress) -> String {
        let net = net_title(&e.network);
        let domain = self.st().saving_extras.get(&Self::extra_key(&e.name, &e.network)).and_then(|x| x.note.as_ref())
            .map(|n| n.split(" — ").next().unwrap_or_default().to_string());
        match domain {
            Some(d) => self.say(&format!("Resolved from {d} on {net} just now and checked. Save asks for Windows Hello with the whole address on the prompt."),
                                &format!("Получено из {d} в {net} только что и проверено. «Сохранить» спросит Windows Hello с полным адресом в окне.")),
            None => self.say(&format!("Taken from your own message and checked for {net}. Compare it with the source: Save asks for Windows Hello with the whole address on the prompt."),
                             &format!("Взято из твоего сообщения и проверено для {net}. Сверь с источником: «Сохранить» спросит Windows Hello с полным адресом в окне.")),
        }
    }

    /// USDC to a saved address (the Mac's transferRoute footer): to one of the person's own wallets it says so; on
    /// Ethereum mainnet, that the gas is in ETH and can be dollars.
    pub fn transfer_note(&self, e: &SignerAddress, amount: Decimal) -> String {
        let net = net_title(&e.network);
        let amount = Fmt::num(amount, 2);
        (if self.is_own_wallet(&e.address) {
            self.say(&format!("To your own wallet «{}», one transfer on {net}.", e.name), &format!("На твой собственный кошелёк «{}», один перевод в {net}.", e.name))
        } else {
            self.say(&format!("Only this saved address may receive the money: the guard and the signer check it, the simulation must show exactly {amount} USDC leaving."),
                     &format!("Получить деньги может только этот сохранённый адрес: это проверяют защита и подписант, а симуляция должна показать, что уходит ровно {amount} USDC."))
        }) + &if e.network == "ethereum" {
            self.say(" Ethereum mainnet: the fee is in ETH and can be dollars; it's bought for you if the wallet has none there.",
                     " Основная сеть Ethereum: комиссия — в ETH и может быть в долларах; если его на кошельке нет, его купят за тебя.")
        } else { String::new() }
            + &self.say(" Press Confirm.", " Нажми «Подтвердить».")
    }
}

#[cfg(test)]
#[path = "move_routes/tests.rs"]
mod tests;

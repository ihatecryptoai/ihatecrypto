//! What the Mac checks and says before a move's card (MoveChecks.swift; the Mac's Canvases.swift, Execution.swift,
//! Workspace.swift): a move that can't go through with what the wallet holds is said at once, with the ways on as cards;
//! a card shows its route step by step, each with the balance it changes; a recurring buy is offered two ways; a new
//! Hyperliquid account is asked its way in; a waiting card waits on its canvas. No types of its own.

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use ihatecrypto_core::address_book_command::AddressBookCommand;
use ihatecrypto_core::chat_language::ChatWords;
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded};
use ihatecrypto_core::funding_plan::{self, Place};
use ihatecrypto_core::intent::{Fmt, TradeIntent};
use ihatecrypto_core::modules::ModuleProtocol;
use ihatecrypto_core::network::{Amount, Bridge, Hyperliquid};
use ihatecrypto_core::plan::{Evaluation, Market, Plan};
use ihatecrypto_core::position_command::WalletCommand as W;
use ihatecrypto_core::stablecoins;
use rust_decimal::Decimal;
use signer_api::SignerAddress;
use trade::dust::Dust;
use trade::{EvmNetwork, Executor};
use wallet::address_check;

use crate::automation::task_run;
use crate::canvas::{Money, Step};
use crate::engine::{ChainStep, Engine, Parked, Pending};
use crate::from_core::RouterOption;
use crate::portfolio::{Network, Row};
use crate::store;

/// A decimal literal of the code.
fn d(s: &str) -> Decimal { s.parse().expect("a decimal literal") }

/// Balances read from outside, added up: None past Decimal's range (where Swift's Decimal goes NaN) — a balance that can't
/// be added up counts as not read, and the move's own checks say what's wrong.
fn sum(values: impl IntoIterator<Item = Decimal>) -> Option<Decimal> { values.into_iter().fold(Num::of(Decimal::ZERO), |a, v| a + v).0 }

/// A checked number as Fmt prints it; «NaN» where Swift's Decimal went NaN.
fn shown(n: Num, places: u32) -> String { n.0.map_or_else(|| "NaN".into(), |x| Fmt::num(x, places)) }

/// The key sent-to.json keeps an address by: a 0x one in lower case.
fn sent_key(address: &str) -> String { if address.starts_with("0x") { address.to_lowercase() } else { address.into() } }

/// The addresses money already went to from here: the first transfer to any other is a card with a caution.
fn sent_to() -> BTreeSet<String> { store::load::<Vec<String>>("sent-to.json").unwrap_or_default().into_iter().collect() }

/// Saved sorted, as Swift saves `Array(newValue).sorted()`.
fn save_sent_to(set: &BTreeSet<String>) { store::save(&set.iter().collect::<Vec<_>>(), "sent-to.json") }

/// Money into a deposit: its balance there now → after.
fn into_place(place: &str, symbol: &str, was: Decimal, delta: Option<Decimal>) -> Option<Money> {
    let after = (Num::of(was) + delta?).0?.max(Decimal::ZERO);
    Some(Money { place: place.into(), symbol: symbol.into(), was: Engine::amount(was, symbol), after: Engine::amount(after, symbol) })
}

/// AddressCheck.Network(rawValue:)'s title, or the raw value.
fn network_title(raw: &str) -> String {
    serde_json::from_value::<address_check::Network>(serde_json::Value::String(raw.into())).map_or_else(|_| raw.into(), |n| n.title().into())
}

/// A contract as tokenName writes it, «AI (0x2E8c…1e18)»: its two ends.
static CONTRACT_ENDS: LazyLock<fancy_regex::Regex> =
    LazyLock::new(|| fancy_regex::Regex::new(r"\((0x[0-9a-fA-F]{4})…([0-9a-fA-F]{4})\)$").expect("a regex"));

impl Engine {
    // MARK: What the wallet holds, as last read

    /// A network's rows on screen; None before the first read (a balance not loaded yet leaves it to the move's checks).
    fn rows_read(&self, net: &str) -> Option<Vec<Row>> {
        let st = self.st();
        st.portfolio.updated_at?;
        Some(st.portfolio.networks.iter().find(|n| n.key == net).map(|n| n.rows.clone()).unwrap_or_default())
    }

    /// USDC free at a place (USDG on Robinhood Chain, Hyperliquid's withdrawable), the Mac's usdc(on:).
    pub fn usdc_free(&self, net: &str) -> Option<Decimal> {
        let hl = self.st().place_facts.hyperliquid.as_ref().map(|h| h.withdrawable);
        if net == "hyperliquid" && let Some(free) = hl { return Some(free) }
        let dollar = if net == "robinhood" { "USDG" } else { "USDC" };
        let rows = self.rows_read(net)?;
        sum(rows.iter().filter(|r| r.section == "tokens" && r.symbol == dollar && r.contract.is_none()).map(|r| r.value))
    }

    /// A token held at a place, by its symbol or contract; None when the place isn't read.
    pub fn held_there(&self, symbol: &str, net: &str) -> Option<Decimal> {
        let (up, low) = (symbol.to_uppercase(), symbol.to_lowercase());
        if up == "USDC" || up == "USDG" { return self.usdc_free(net) }
        let rows = self.rows_read(net)?;
        sum(rows.iter()
            .filter(|r| r.section == "tokens" && (r.symbol.to_uppercase() == up || r.contract.as_ref().is_some_and(|c| c.to_lowercase() == low)))
            .map(|r| r.value))
    }

    /// The network's own coin under the floor a lending card reads, as the Mac's: Base's aaveRoute by 0.000005 ETH, the
    /// other networks' lendingRoute by the network's gas floor. Unread counts as none.
    pub fn gas_low(&self, net: &str) -> bool {
        let gas = self.rows_read(net).and_then(|rows| sum(rows.iter().filter(|r| r.contract.as_deref() == Some("native")).map(|r| r.value)));
        let floor = if net == "base" { d("0.000005") } else {
            EvmNetwork::named(net).map_or(Decimal::ZERO, |n| Decimal::from(n.gas_floor) / Decimal::from(1_000_000_000_000_000_000u64))
        };
        gas.unwrap_or_default() < floor
    }

    /// What a deposit row holds: «Jupiter Lend», «Kamino Lend», «Kamino» (a vault), «Aave», «Morpho».
    fn deposit_there(&self, names: &[&str], net: &str) -> Decimal {
        self.rows_read(net).and_then(|rows| sum(rows.iter().filter(|r| r.section == "defi" && names.contains(&r.symbol.as_str())).map(|r| r.value)))
            .unwrap_or_default()
    }

    fn hlp_equity(&self) -> Decimal { self.st().place_facts.hyperliquid.as_ref().map_or(Decimal::ZERO, |h| h.hlp_equity) }

    /// The Kamino Earn vault «withdraw … from kamino» takes from when Lend holds nothing: its row's name, «0.5 SOL», and
    /// the tokens.
    fn kamino_vault(&self, symbol: &str) -> Option<(String, String, Decimal)> {
        if self.deposit_there(&["Kamino Lend"], "solana") != Decimal::ZERO { return None }
        let suffix = format!(" {}", symbol.to_uppercase());
        let r = self.rows_read("solana")?.into_iter().find(|r| r.section == "defi" && r.symbol == "Kamino" && r.amount.ends_with(&suffix))?;
        let tokens = decimal(&r.amount.split(' ').find(|s| !s.is_empty()).unwrap_or("").replace(',', ""))?;
        let note = r.note.clone().unwrap_or_else(|| "Kamino".into());
        let name = note.split('·').find(|s| !s.is_empty()).unwrap_or("Kamino").trim().to_string();
        Some((name, r.amount, tokens))
    }

    /// One of this computer's wallets, by its Solana, EVM or NEAR address.
    pub fn is_own_wallet(&self, address: &str) -> bool {
        let a = address.to_lowercase();
        let st = self.st();
        st.status.as_ref().and_then(|s| s.wallets.as_ref()).is_some_and(|ws| {
            ws.iter().any(|w| [&w.solana, &w.evm, &w.near].into_iter().any(|x| x.as_ref().is_some_and(|x| x.to_lowercase() == a)))
        })
    }

    // MARK: Sweep dust (5)

    /// What «sweep dust» sells: the verified tokens on Base, Arbitrum and Polygon worth from $0.20 up to `under` (None =
    /// $5), the most first; the network's own coin stays, it's gas (the Mac's dustTokens).
    pub fn dust_tokens(&self, under: Option<Decimal>) -> Vec<Dust> { Self::dust(&self.st().portfolio.networks, under) }

    pub fn dust(networks: &[Network], under: Option<Decimal>) -> Vec<Dust> {
        let limit = under.unwrap_or(Decimal::from(5));
        let mut out: Vec<Dust> = networks.iter().filter(|n| Executor::DUST_NETWORKS.contains(&n.key.as_str())).flat_map(|n| {
            n.rows.iter().filter_map(move |r| {
                let c = r.contract.as_deref().filter(|c| *c != "native")?;
                let u = r.usd?;
                (r.section == "tokens" && r.verified == Some(true) && u >= d("0.2") && u < limit).then(|| Dust::new(&n.key, c, &r.symbol, u))
            })
        }).collect();
        out.sort_by_key(|d| std::cmp::Reverse(d.usd));
        out
    }

    // MARK: Before a card (7, 8)

    /// A move that can't go through with what the wallet holds, said at once instead of a card whose Confirm could only
    /// fail (the Mac's refusalBeforeCard).
    pub fn refusal_before_card(&self, command: &W) -> Option<String> {
        if let Some(why) = self.funding_shortfall(command) { return Some(why) }
        let lacks = |have: Option<Decimal>, asked: Option<Decimal>, place: &str, unit: &str| -> Option<String> {
            let have = have?;
            if have <= Decimal::ZERO {
                return Some(self.say(&format!("{place} has {} {unit}, so there’s nothing to move from there — nothing was prepared.", Fmt::num(have, 2)),
                                     &format!("На {place} {} {unit} — оттуда нечего переводить, ничего не готовлю.", Fmt::num(have, 2))));
            }
            if let Some(asked) = asked && asked > have {
                return Some(self.say(&format!("Only {} {unit} is on {place}, less than {}. Say an amount up to that.", Fmt::num(have, 2), Fmt::num(asked, 2)),
                                     &format!("На {place} только {} {unit} — меньше, чем {}. Назови сумму не больше.", Fmt::num(have, 2), Fmt::num(asked, 2))));
            }
            None
        };
        let dollar = |net: &str| if net == "robinhood" { "USDG" } else { "USDC" };
        let solana_usdc = self.usdc_free("solana");
        match command {
            W::WithdrawToSolana(asked) => lacks(self.usdc_free("hyperliquid"), *asked, "Hyperliquid", "USDC"),
            W::Kamino { deposit: false, symbol, amount } if symbol == "USDC" => {
                lacks(self.rows_read("solana").map(|_| self.deposit_there(&["Kamino Lend", "Kamino"], "solana")), *amount, "Kamino", "USDC")
            }
            W::LendWithdraw(s, asked) if s == "USDC" => {
                lacks(self.rows_read("solana").map(|_| self.deposit_there(&["Jupiter Lend"], "solana")), *asked, "Jupiter Lend", "USDC")
            }
            W::BaseToSolana(asked) => lacks(self.usdc_free("base"), *asked, "Base", "USDC"),
            W::ArbitrumToSolana(asked) => lacks(self.usdc_free("arbitrum"), *asked, "Arbitrum", "USDC"),
            W::RobinhoodToSolana(asked) => lacks(self.usdc_free("robinhood"), *asked, "Robinhood Chain", "USDG"),
            W::EvmToSolana(net, asked) | W::Bridge { from: net, amount: asked, .. } => {
                lacks(self.usdc_free(net), *asked, &W::place_name(net), dollar(net))
            }
            W::Usdt { from: net, amount: asked, .. } => lacks(self.held_there("USDT", net), *asked, &W::place_name(net), "USDT"),
            W::BuyOn { usdc: Some(asked), from: Some(from), .. } if from != "solana" => {
                lacks(self.usdc_free(from), Some(*asked), &W::place_name(from), dollar(from))
            }
            W::Hlp { deposit: false, amount: asked } => {
                let until = self.st().place_facts.hyperliquid.as_ref().and_then(|h| h.hlp_locked_until);
                if let Some(until) = until && until > chrono::Utc::now() && self.hlp_equity() > Decimal::ZERO {
                    return Some(self.say(&format!("HLP is locked until {}: Hyperliquid lets deposits out 4 days after they go in.", Fmt::date(until)),
                                         &format!("HLP заблокирован до {}: Hyperliquid выпускает депозит через 4 дня после внесения.", Fmt::date(until))));
                }
                let read = self.st().place_facts.hyperliquid.is_some();
                lacks(read.then(|| self.hlp_equity()), *asked, "HLP", "USDC")
            }
            W::LendDeposit(s, Some(asked)) if s == "USDC" => lacks(solana_usdc, Some(*asked), "Solana", "USDC"),
            W::Kamino { deposit: true, symbol, amount: Some(asked) } if symbol == "USDC" => lacks(solana_usdc, Some(*asked), "Solana", "USDC"),
            // All of it goes in at once, and Jupiter starts at 100: «say less» would only meet its minimum.
            W::JupiterDca { usdc: Some(asked), .. } => {
                if let Some(have) = solana_usdc && *asked > have {
                    return Some(if have < W::jupiter_dca_minimum() {
                        self.say(&format!("Only {} USDC is on Solana, and Jupiter's DCA starts at 100 — all of it goes in at once. Top up Solana first.", Fmt::num(have, 2)),
                                 &format!("На Solana только {} USDC, а DCA у Jupiter — от 100, и вся сумма вносится сразу. Сначала пополни Solana.", Fmt::num(have, 2)))
                    } else {
                        self.say(&format!("Only {} USDC is on Solana, and the DCA puts all {} in at once. Say up to {}.", Fmt::num(have, 2), Fmt::num(*asked, 2), Fmt::num(have, 2)),
                                 &format!("На Solana только {} USDC, а DCA вносит все {} сразу. Назови сумму до {}.", Fmt::num(have, 2), Fmt::num(*asked, 2), Fmt::num(have, 2)))
                    });
                }
                if let Some(sol) = self.held_there("SOL", "solana") && sol < d("0.007") {
                    return Some(self.say(&format!("The DCA's order account takes ≈ 0.006 SOL of rent, back when it ends. Solana has {} SOL.", Fmt::num(sol, 4)),
                                         &format!("Аккаунт ордера DCA берёт ≈ 0.006 SOL залога (вернётся в конце), а на Solana {} SOL.", Fmt::num(sol, 4))));
                }
                None
            }
            // «move 100 USDC to arbitrum» read as from Solana, where little lay: said before the card, with where the money is.
            W::SolanaTo(_, Some(asked)) => {
                let why = lacks(solana_usdc, Some(*asked), "Solana", "USDC")?;
                let Some(net) = EvmNetwork::funding(*asked, |k| self.usdc_free(k)) else { return Some(why) };
                let there = Fmt::num(self.usdc_free(net.key).unwrap_or_default(), 2);
                Some(why + &self.say(&format!(" {} has {there} USDC: say «from {}» to move it from there.", net.name, net.name),
                                     &format!(" На {} — {there} USDC: скажи «с {}», чтобы перевести оттуда.", net.name, net.name)))
            }
            W::LendDeposit(s, Some(asked)) if s == "SOL" => lacks(self.held_there("SOL", "solana"), Some(*asked), "Solana", "SOL"),
            W::Kamino { deposit: true, symbol, amount: Some(asked) } if symbol == "SOL" => lacks(self.held_there("SOL", "solana"), Some(*asked), "Solana", "SOL"),
            W::Stake(Some(asked)) => lacks(self.held_there("SOL", "solana"), Some(*asked), "Solana", "SOL"),
            W::Sell { asset, amount } if asset.resolved() => {
                let have = self.held_there(if asset.symbol == "SOL" { "SOL" } else { &asset.mint }, "solana")?;
                if have <= Decimal::ZERO {
                    return Some(self.say(&format!("There’s no {} on Solana to sell — nothing was prepared.", asset.symbol),
                                         &format!("На Solana нет {} — продавать нечего, ничего не готовлю.", asset.symbol)));
                }
                lacks(Some(have), *amount, "Solana", &asset.symbol)
            }
            W::SweepDust { under } => {
                if self.st().portfolio.updated_at.is_none() || !self.dust_tokens(*under).is_empty() { return None }
                let limit = Fmt::usd(under.unwrap_or(Decimal::from(5)));
                Some(self.say(&format!("No verified token worth $0.20 to {limit} on Base, Arbitrum or Polygon — no dust to sweep, nothing was prepared."),
                              &format!("На Base, Arbitrum и Polygon нет проверенных токенов от $0.20 до {limit} — пыли нет, ничего не готовлю.")))
            }
            W::SellNear { amount: asked, .. } => {
                // What a sale keeps on the account while it is sent: storage and prepaid gas.
                let near = self.held_there("NEAR", "near")?;
                let sellable = (Num::of(near) - d("0.09")).0.unwrap_or_default().max(Decimal::ZERO);
                let amount = asked.unwrap_or(sellable);
                if !(amount < d("0.05") || amount > sellable) { return None }
                let (have, most) = (Fmt::num(near, 4), Fmt::num(sellable, 4));
                let nothing = sellable < d("0.05");
                Some(self.say(
                    &format!("You have {have} NEAR. A sale needs at least 0.05 NEAR, and ≈ 0.09 NEAR stays on the account for storage and gas while it is sent, so {}.",
                             if nothing { "there is nothing to sell now".to_string() } else { format!("at most {most} NEAR can be sold") }),
                    &format!("У тебя {have} NEAR. Продажа — от 0.05 NEAR, и ≈ 0.09 NEAR остаётся на аккаунте под хранение и газ, пока она идёт, так что {}.",
                             if nothing { "сейчас продавать нечего".to_string() } else { format!("можно продать не больше {most} NEAR") })))
            }
            _ => None,
        }
    }

    /// What a deposit or a buy spends and where, and how much of it that place lacks; None when it has it, or the move
    /// brings its own money (a source the user named).
    fn funding_need(&self, c: &W) -> Option<(Decimal, String)> {
        let (amount, net): (Decimal, String) = match c {
            W::Aave { deposit: true, amount: Some(a), network: n } | W::Morpho { deposit: true, amount: Some(a), network: n }
            | W::BuyOn { place: n, usdc: Some(a), from: None, .. } => (*a, n.clone()),
            W::BuyStock { usdc: Some(a), .. } => (*a, "robinhood".into()),
            W::LendDeposit(s, Some(a)) if s == "USDC" => (*a, "solana".into()),
            W::Kamino { deposit: true, symbol, amount: Some(a) } if symbol == "USDC" => (*a, "solana".into()),
            W::Buy { usdc: Some(a), .. } | W::JupiterDca { usdc: Some(a), .. } => (*a, "solana".into()),
            _ => return None,
        };
        let have = self.usdc_free(&net)?;
        let short = (Num::of(amount) - have).0?;
        (short > Decimal::ZERO).then_some((short, net))
    }

    /// One place pays all of it inside the move itself, one card: Solana for a move elsewhere, or one EVM network sending
    /// straight (EvmNetwork::source, as the Executor signs it).
    fn one_source(&self, short: Decimal, net: &str) -> bool {
        let solana = self.usdc_free("solana").unwrap_or_default();
        net != "solana"
            && ((Num::of(short) + d("0.1")).0.is_some_and(|need| solana >= need)
                || EvmNetwork::source(short, solana, net, |k| self.usdc_free(k)).is_some())
    }

    fn funding_legs(&self, short: Decimal, net: &str) -> Option<Vec<String>> {
        let evm: Vec<Place> = EvmNetwork::searched().into_iter().map(|n| Place::new(n.key, n.name, self.usdc_free(n.key).unwrap_or_default())).collect();
        let near_price = self.st().markets.get("NEAR").map(|m| m.mark_px);
        funding_plan::legs(short, net, &W::place_name(net), self.usdc_free("solana").unwrap_or_default(), &evm,
                           self.usdc_free("hyperliquid").unwrap_or_default(), self.held_there("NEAR", "near").unwrap_or_default(), near_price)
    }

    /// All the wallet's free money together can't pay the move: said before a card that would fail.
    pub fn funding_shortfall(&self, c: &W) -> Option<String> {
        let (short, net) = self.funding_need(c)?;
        if self.one_source(short, &net) || self.funding_legs(short, &net).is_some() { return None }
        let mut free: Vec<(String, Decimal)> = vec![("Solana".into(), self.usdc_free("solana").unwrap_or_default())];
        free.extend(EvmNetwork::searched().into_iter().filter_map(|n| self.usdc_free(n.key).filter(|x| *x > Decimal::ZERO).map(|x| (n.name.to_string(), x))));
        free.push(("Hyperliquid".into(), self.usdc_free("hyperliquid").unwrap_or_default()));
        let price = self.st().markets.get("NEAR").map(|m| m.mark_px);
        if let Some(price) = price && let Some(near) = self.held_there("NEAR", "near") && near > Decimal::ZERO && let Some(usd) = (Num::of(near) * price).0 {
            free.push(("NEAR".into(), usd));
        }
        let listed = free.iter().filter(|(_, x)| *x >= d("0.01")).map(|(n, x)| format!("{n} {}", Fmt::usd(*x))).collect::<Vec<_>>().join(", ");
        let place = W::place_name(&net);
        Some(self.say(&format!("This needs {} USDC more on {place}, and all the free money together isn't enough: {listed}. Top up, or say a smaller amount.", Fmt::num(short, 2)),
                      &format!("Нужно ещё {} USDC на {place}, а всех свободных денег не хватает: {listed}. Пополни или назови сумму меньше.", Fmt::num(short, 2))))
    }

    /// A move asking for more than its place holds: what there is in a line, and the ways on as cards — bring the rest from
    /// elsewhere first, take all there is, or top up (the Mac's shortChoices). None when there's enough, the move pays it
    /// within itself, or the holding isn't known.
    pub fn short_choices(&self, c: &W) -> Option<(String, Vec<RouterOption>)> {
        if let Some((short, net)) = self.funding_need(c) && !self.one_source(short, &net) {
            let (have, place) = (self.usdc_free(&net).unwrap_or_default(), W::place_name(&net));
            let mut options = vec![];
            if let Some(legs) = self.funding_legs(short, &net) && !legs.is_empty() && W::parse(&c.summary()).as_ref() == Some(c) {
                let path: Vec<String> = legs.into_iter().chain([c.summary()]).collect();
                options.push(RouterOption::new(&self.say(&format!("Bring {} USDC to {place} first", Fmt::num(short, 2)), &format!("Сначала довезти {} USDC на {place}", Fmt::num(short, 2))),
                                               &path.join(" → "), &path.join(" then "), true));
            }
            let recommended = options.is_empty();
            options.extend(self.all_there(c, have, "USDC", recommended));
            options.push(self.top_up_first(&place));
            let asks = shown(Num::of(have) + short, 2);
            return Some((self.say(&format!("On {place} there's {} USDC, and this asks {asks}. Pick a way below.", Fmt::num(have, 2)),
                                  &format!("На {place} {} USDC, а нужно {asks}. Выбери ниже, как быть.", Fmt::num(have, 2))), options));
        }
        // «send all USDT on solana to …» with none there: said at once, not a card that fails at the signature.
        if let W::SendToken { token, amount: None, network: Some(net), .. } = c && self.held_there(token, net) == Some(Decimal::ZERO) {
            let place = W::place_name(net);
            return Some((self.say(&format!("There's no {} on {place} to send.", token.to_uppercase()), &format!("На {place} нет {} — отправлять нечего.", token.to_uppercase())),
                         vec![self.top_up_first(&place)]));
        }
        let (symbol, amount, net): (String, Decimal, String) = match c {
            W::SwapOn { place, from, amount: Some(x), .. } => (from.clone(), *x, place.clone()),
            W::SellOn { place, token, amount: Some(x), .. } => (token.clone(), *x, place.clone()),
            W::CrossSwap { from, on, amount: Some(x), .. } => (from.clone(), *x, on.clone()),
            W::Sell { asset, amount: Some(x) } => {
                (if asset.symbol == "SOL" || asset.mint.is_empty() { asset.symbol.clone() } else { asset.mint.clone() }, *x, "solana".into())
            }
            W::SendToken { token, amount: Some(x), network: Some(net), .. } => (token.clone(), *x, net.clone()),
            _ => return None,
        };
        let have = self.held_there(&symbol, &net)?;
        if amount <= have { return None }
        let place = W::place_name(&net);
        let symbol = if symbol.chars().count() > 12 {
            let st = self.st();
            st.portfolio.networks.iter().find(|n| n.key == net).and_then(|n| n.rows.iter().find(|r| r.contract.as_deref() == Some(symbol.as_str())))
                .map_or_else(|| symbol.clone(), |r| r.symbol.clone())
        } else {
            symbol.to_uppercase()
        };
        let gap = (Num::of(amount) - have).0;
        let mut bring: Vec<RouterOption> = vec![];
        if symbol == "USDC" && let Some(gap) = gap && let Some(legs) = self.funding_legs(gap, &net) && !legs.is_empty() {
            let path: Vec<String> = legs.into_iter().chain([c.summary()]).collect();
            bring = vec![RouterOption::new(&self.say(&format!("Bring {} USDC to {place} first", Fmt::num(gap, 2)), &format!("Сначала довезти {} USDC на {place}", Fmt::num(gap, 2))),
                                           &path.join(" → "), &path.join(" then "), true)];
        }
        // Dollars short on Solana where its USDC can buy them: the buy first, then the same move.
        let missing = gap.map(|g| rounded(g, 2, Rounding::Up));
        if let Some(missing) = missing && symbol != "USDC" && stablecoins::is_dollar(&symbol) && net == "solana"
            && self.usdc_free("solana").unwrap_or_default() >= missing {
            let buy = format!("buy {symbol} on solana for {} USDC", Fmt::num(missing, 2));
            bring = vec![RouterOption::new(&self.say(&format!("Buy {} {symbol} for USDC first", Fmt::num(missing, 2)), &format!("Сначала купить {} {symbol} за USDC", Fmt::num(missing, 2))),
                                           &format!("{buy} → {}", c.summary()), &format!("{buy} then {}", c.summary()), true)];
        }
        let line = self.say(&format!("On {place} there's {} {symbol}, and this asks {}. Pick a way below.", Fmt::num(have, 6), Fmt::num(amount, 6)),
                            &format!("На {place} {} {symbol}, а нужно {}. Выбери ниже, как быть.", Fmt::num(have, 6), Fmt::num(amount, 6)));
        let recommended = bring.is_empty();
        let mut options = bring;
        options.extend(self.all_there(c, have, &symbol, recommended));
        options.push(self.top_up_first(&place));
        Some((line, options))
    }

    /// «All there is»: the same move for what the place holds, when that's worth a move.
    fn all_there(&self, c: &W, have: Decimal, unit: &str, recommended: bool) -> Vec<RouterOption> {
        let amount = if unit == "USDC" { rounded(have, 2, Rounding::Down) } else { have };
        if amount <= Decimal::ZERO || (unit == "USDC" && amount < d("0.5")) { return vec![] }
        let all = c.with_amount(amount);
        if all == *c { return vec![] }
        vec![RouterOption::new(&self.say(&format!("All there is: {} {unit}", Fmt::num(amount, 6)), &format!("Всё, что есть: {} {unit}", Fmt::num(amount, 6))),
                               &all.summary(), &all.summary(), recommended)]
    }

    fn top_up_first(&self, place: &str) -> RouterOption {
        RouterOption::new(&self.say("Top up first", "Сначала пополнить"),
                          &self.say(&format!("Your addresses on the canvas: send the rest to {place} and ask again."),
                                    &format!("Адреса на холсте: пришли недостающее на {place} и попроси снова.")),
                          "top up", false)
    }

    /// Everything handle_wallet checks before a card, in the Mac's order: true when it answered and nothing else is drawn.
    pub fn checked_before_card(&self, command: &mut W, note: Option<&str>) -> bool {
        // Jupiter's DCA: the swaps when none were named, and Jupiter's own minimums before a card that would fail.
        if let W::JupiterDca { asset, usdc: Some(u), orders, every } = command.clone() {
            let orders = orders.unwrap_or_else(|| W::jupiter_dca_orders(u));
            if let Some(problem) = W::jupiter_dca_problem(u, orders) {
                self.reply(&problem, note.map(Into::into));
                return true;
            }
            *command = W::JupiterDca { asset, usdc: Some(u), orders: Some(orders), every };
        }
        if let Some((line, options)) = self.short_choices(command) {
            self.reply(&line, note.map(Into::into));
            self.set_options(options);
            return true;
        }
        if let Some(why) = self.refusal_before_card(command) {
            self.reply(&why, note.map(Into::into));
            self.note_refusal(&command.summary(), &why, note);
            return true;
        }
        false
    }

    /// A move refused before its card: once the message is through, the model works out why from the balances and offers
    /// the way that works, as the Mac's noteFailure → explainFailure. Not inside a request the model carries on (it hears
    /// it there), a scheduled run or a run on the automation wallet.
    pub fn note_refusal(&self, what: &str, why: &str, note: Option<&str>) {
        if self.st().agent_goal.is_some() || task_run().is_some() || note == Some("schedule") { return }
        let (me, what, why) = (self.rc(), what.to_string(), why.to_string());
        tokio::task::spawn_local(async move { me.explain_once_free(&what, &why).await });
    }

    /// A failure caught as an error (Swift's `noteRefusal(_:_ error:note:)`). A stop by the guard, the signer or a scam
    /// check (GuardStop) and a costly move with its own «Send anyway» (CostlyMove) get nothing offered around them (the
    /// Mac's noteFailure, a3f7edb).
    pub fn note_refusal_error(&self, what: &str, error: &trade::Error, note: Option<&str>) {
        if !Self::explains_after(error) { return }
        self.note_refusal(what, &error.to_string(), note);
    }

    /// Whether the model may offer a way after this failure: not after the guard, the signer or a scam check, and not
    /// after a costly move, which has its own «Send anyway» (the owner's rule, a3f7edb).
    pub fn explains_after(error: &trade::Error) -> bool { !matches!(error, trade::Error::Guard(_) | trade::Error::Costly(_)) }

    async fn explain_once_free(&self, what: &str, why: &str) {
        // ponytail: polls the turn's end (30 s at most); a hook at the end of send() if it ever misses one.
        for _ in 0..150 {
            let thinking = self.st().thinking;
            if !thinking { break }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let busy = {
            let st = self.st();
            st.thinking || st.pending.is_some() || st.running.is_some() || st.agent_goal.is_some()
        };
        if busy { return }
        self.explain_failure(what, why).await;
    }

    /// The USDC (or USDT0) a move takes from an EVM network, read from its node now: the last balance read may be a minute
    /// old, and a move right after money landed was refused with «0 there» (the Mac's freshenSource). Only that row changes.
    pub async fn freshen_source(&self, command: &W) {
        if task_run().is_some() { return }
        let (owner, read) = {
            let st = self.st();
            (st.evm_address().filter(|o| !o.is_empty()), st.portfolio.updated_at.is_some())
        };
        let Some(owner) = owner else { return };
        if !read { return }
        let (place, usdt): (Option<&str>, bool) = match command {
            W::EvmToSolana(n, _) | W::Bridge { from: n, .. } | W::Cctp { from: n, .. } => (Some(n.as_str()), false),
            W::BuyOn { from: Some(f), .. } => (Some(f.as_str()), false),
            W::Usdt { from, .. } => (Some(from.as_str()), true),
            W::BaseToSolana(_) => (Some("base"), false),
            W::ArbitrumToSolana(_) => (Some("arbitrum"), false),
            W::RobinhoodToSolana(_) => (Some("robinhood"), false),
            _ => (None, false),
        };
        let Some(place) = place else { return };
        let Some(net) = EvmNetwork::named(place) else { return };
        let token = if usdt { stablecoins::find("USDT", place).map(|c| c.address) } else { None };
        if usdt && token.is_none() { return }
        // EVMRPC.tokenBalance: 0 when the balance doesn't fit 64 bits.
        let units = match token {
            Some(t) => net.rpc.token_balance(t, &owner).await.ok().map(|x| u64::try_from(x).unwrap_or(0)),
            None => net.usdc_balance(&owner).await.ok(),
        };
        let Some(units) = units else { return };
        let amount = Decimal::from(units) / Decimal::from(1_000_000);
        self.patch_dollar(place, if usdt { "USDT" } else if place == "robinhood" { "USDG" } else { "USDC" }, token, amount);
    }

    /// A network's dollar row set to what was just read, the rest of its card as it was.
    fn patch_dollar(&self, place: &str, symbol: &str, contract: Option<&str>, amount: Decimal) {
        let mut st = self.st_mut();
        let networks = &mut st.portfolio.networks;
        let i = match networks.iter().position(|n| n.key == place) {
            Some(i) => i,
            None if amount > Decimal::ZERO => {
                networks.push(Network { key: place.into(), name: W::place_name(place), usd: Decimal::ZERO, rows: vec![], note: None, hidden: vec![], hidden_more: 0 });
                networks.len() - 1
            }
            None => return,
        };
        let rows = &mut networks[i].rows;
        let matches = |r: &Row| r.section == "tokens" && r.symbol == symbol
            && match contract { None => r.contract.is_none(), Some(c) => r.contract.as_ref().is_some_and(|x| x.to_lowercase() == c.to_lowercase()) };
        if let Some(old) = rows.iter().position(matches) {
            if amount > Decimal::ZERO {
                rows[old].value = amount;
                rows[old].amount = Fmt::num(amount, 2);
                rows[old].usd = Some(amount);
            } else {
                rows.remove(old);
            }
        } else if amount > Decimal::ZERO {
            let mut r = Self::row(symbol, amount, Some(amount), None, None, None, None, None);
            r.contract = contract.map(Into::into);
            rows.insert(0, r);
        }
    }

    // MARK: «How much?» (14)

    /// What there is to use, so the answer to «How much?» is easy.
    pub fn amount_hint(&self, command: &W) -> String {
        match command {
            W::LendDeposit(s, _) | W::Kamino { symbol: s, .. } if s == "SOL" => self.sol_hint(),
            W::Stake(_) => self.sol_hint(),
            W::Hlp { .. } => self.usdc_free("hyperliquid").map_or_else(String::new, |x| {
                self.say(&format!(" {} is free on Hyperliquid — answer with a number, e.g. “5”.", Fmt::usd(x)), &format!(" На Hyperliquid свободно {} — ответь числом, например «5».", Fmt::usd(x)))
            }),
            _ => self.usdc_free("solana").map_or_else(String::new, |x| {
                self.say(&format!(" You have {} USDC on Solana — answer with a number, e.g. “1”.", Fmt::num(x, 2)), &format!(" На Solana {} USDC — ответь числом, например «1».", Fmt::num(x, 2)))
            }),
        }
    }

    fn sol_hint(&self) -> String {
        self.held_there("SOL", "solana").map_or_else(String::new, |x| {
            self.say(&format!(" You have {} SOL on Solana — answer with a number, e.g. “0.01”.", Fmt::num(x, 4)), &format!(" На Solana {} SOL — ответь числом, например «0.01».", Fmt::num(x, 4)))
        })
    }

    /// The reply to «How much …?» while a command waits for its amount: true when it was the answer (or asked again).
    /// An amount in another unit («$5» to a SOL stake) or under the coin's smallest one is asked again in the question's
    /// unit, never read as that coin.
    pub async fn answered_amount(&self, text: &str, declines: bool) -> bool {
        let waiting = self.st().awaiting_amount.clone();
        let Some(waiting) = waiting else { return false };
        if declines {
            self.st_mut().awaiting_amount = None;
            self.reply(&self.say("OK, not doing that.", "Хорошо, не делаю."), None);
            return true;
        }
        if let Some(value) = W::amount_answer_for(text, &waiting) {
            self.st_mut().awaiting_amount = None;
            self.handle_wallet(&waiting.with_amount(value), None).await;
            return true;
        }
        if let Some(value) = W::amount_answer(text) {
            let unit = waiting.amount_asset();
            let (least, example) = if unit == "SOL" { ("0.000000001", "0.01") } else { ("0.000001", "1") };
            let line = if value < d(least) {
                self.say(&format!("That's less than the smallest {unit} amount ({least}). How much {unit}? Write a number in {unit}, for example “{example}”."),
                         &format!("Это меньше самой малой суммы {unit} ({least}). Сколько {unit}? Напиши число в {unit}, например «{example}»."))
            } else {
                self.say(&format!("The question is in {unit}. How much {unit}? Write a number in {unit}, for example “{example}”."),
                         &format!("Вопрос в {unit}. Сколько {unit}? Напиши число в {unit}, например «{example}»."))
            };
            self.reply(&line, None);
            return true;
        }
        if ChatWords::confirms(text) {
            self.reply(&self.say("How much? Just the number, for example “1”.", "Сколько? Просто число, например «1»."), None);
            return true;
        }
        self.st_mut().awaiting_amount = None;
        false
    }

    // MARK: Addresses (9, 10, 11)

    /// The message with each address already saved in Addresses written as its name, when it asks to send: a saved address
    /// was confirmed on its card, so writing it is the same as writing the name. On several networks, the one the message
    /// names wins (the Mac's savedNames).
    pub fn saved_names(&self, text: &str) -> String {
        let found = guard::injection::findings(text);
        if !found.transfer_verb { return text.into() }
        let want = AddressBookCommand::network(&text.to_lowercase());
        let book = self.st().book.clone();
        found.addresses.iter().fold(text.to_string(), |out, address| {
            let saved: Vec<&SignerAddress> = book.iter()
                .filter(|e| if address.starts_with("0x") { e.address.to_lowercase() == address.to_lowercase() } else { e.address == *address })
                .collect();
            let Some(entry) = saved.iter().find(|e| Some(&e.network) == want.as_ref()).or(saved.first()) else { return out };
            out.replace(address.as_str(), &entry.name)
        })
    }

    /// «Send a small test amount first»: for someone else's address the first time — never for one of the user's own wallets.
    pub fn first_send_warning(&self, address: &str) -> Option<String> {
        if sent_to().contains(&sent_key(address)) || self.is_own_wallet(address) { return None }
        Some(self.say("First transfer to this address. Sending a small test amount first is the safe habit.",
                      "Первый перевод на этот адрес. Безопаснее сначала отправить небольшую пробную сумму."))
    }

    /// An entry saved for one EVM network, sent on another: the user's own wallet has it there too, an exchange may not.
    pub fn other_network_warning(&self, saved: &str, sent: &str) -> Option<String> {
        if saved == sent { return None }
        let (a, b) = (network_title(saved), network_title(sent));
        Some(self.say(&format!("Saved for {a}, sent on {b} to the same address. Your own wallet has it there too, but an exchange's deposit address may not — check that it takes {b}."),
                      &format!("Сохранён для {a}, отправка в {b} на тот же адрес. Твой кошелёк там тоже есть, а адрес биржи может не принимать {b} — проверь.")))
    }

    /// The cautions on a move's card: whom a send goes to the first time, an XRP exchange address without its tag, and
    /// what the move itself risks (move_routes.rs).
    pub fn send_cautions(&self, c: &W) -> Vec<String> {
        let address: Option<String> = match c {
            W::SendSol { .. } | W::SendNear { .. } | W::CloseNear { .. } => self.native_recipient(c).ok().flatten().map(|e| e.address),
            W::SendToken { name, token, network, .. } => {
                // Through NEAR Intents (Bitcoin, XRP, TRON…): an exchange's XRP address without its tag loses the money.
                if let Ok(Some(r)) = self.across_recipient(name, token, network.as_deref()) {
                    if let Some(tag) = self.xrp_tag_warning(&r.0) { return vec![tag] }
                    Some(r.0.address)
                } else {
                    self.token_recipient(name, network.as_deref()).ok().map(|(e, _)| e.address)
                }
            }
            _ => None,
        };
        address.and_then(|a| self.first_send_warning(&a)).into_iter().chain(self.move_cautions(c)).collect()
    }

    /// After a move that paid someone: the address is known from now on.
    pub fn note_sent(&self, done: &Pending) {
        let to: Vec<String> = match done {
            Pending::Transfer(e, _) => vec![e.address.clone()],
            // The address the send went to, found as the send found it (a partial name, another wallet here).
            Pending::Wallet(c) if matches!(self.native_recipient(c), Ok(Some(_))) => self.native_recipient(c).ok().flatten().map(|e| e.address).into_iter().collect(),
            Pending::Wallet(W::SendToken { name, token, network, .. }) => {
                self.across_recipient(name, token, network.as_deref()).ok().flatten().map(|r| r.0.address)
                    .or_else(|| self.token_recipient(name, network.as_deref()).ok().map(|(e, _)| e.address))
                    .into_iter().collect()
            }
            Pending::Chain(steps) => steps.iter().filter_map(|step| match step {
                ChainStep::Transfer(e, _) => Some(e.address.clone()),
                ChainStep::Wallet(c) => self.native_recipient(c).ok().flatten().map(|e| e.address),
                ChainStep::Said(_) => None,
            }).collect(),
            _ => vec![],
        };
        if to.is_empty() { return }
        let mut known = sent_to();
        known.extend(to.iter().map(|a| sent_key(a)));
        save_sent_to(&known);
    }

    /// The guard's «money goes to saved addresses»: the save phrase goes into the chat field, ready to send.
    pub fn offer_save(&self, address: &str, text: &str) {
        let net = AddressBookCommand::network(&text.to_lowercase()).unwrap_or_else(|| if address.starts_with("0x") { "base" } else { "solana" }.into());
        let phrase = self.say(&format!("save {address} as NAME on {net}"), &format!("сохрани {address} как ИМЯ на {net}"));
        self.out.event("suggest", &HashMap::from([("text", phrase)]));
        self.reply(&self.say(&format!("Money goes only to saved addresses, so a swapped address can't receive it. Save this one first: the phrase is in the field — put a name instead of NAME (and another network if it isn't {net}), send it, and check the address on the Windows Hello window. Then say “send <amount> USDC to <name>”."),
                             &format!("Деньги уходят только на сохранённые адреса — так подменённый адрес ничего не получит. Сначала сохрани этот: фраза уже в поле — впиши имя вместо ИМЯ (и другую сеть, если не {net}), отправь и сверь адрес в окне Windows Hello. Потом скажи «переведи <сумма> USDC <имя>»."),
                             ),
                   Some("guard: sends go to saved addresses".into()));
    }

    // MARK: A recurring buy, two ways (12)

    /// Buying over time, the two ways as cards: Jupiter's DCA on-chain (a budget in rounds of at least 50, runs with the app
    /// closed) or the app's own recurring buys (the money stays in the wallet, no minimum, the app must run). True when
    /// offered; the pick comes back as its command and goes straight.
    pub fn offer_recurring_ways(&self, command: &W, note: Option<&str>) -> bool {
        let W::Schedule { asset, usdc, days } = command else { return false };
        let picked = self.st().dca_choice.as_ref() == Some(command);
        if picked || !trade::modules().allows(ModuleProtocol::JupiterDca) {
            self.st_mut().dca_choice = None;
            return false;
        }
        let period = match *days {
            1 => self.say("day", "день"),
            7 => self.say("week", "неделю"),
            30 => self.say("month", "месяц"),
            n => self.say(&format!("{n} days"), &format!("{n} дн.")),
        };
        let minimum = W::jupiter_dca_minimum();
        let half = minimum / Decimal::TWO;
        // ponytail: a budget past Decimal's range isn't offered Jupiter's way; the app's own recurring buy goes on.
        let Some((total, rounds)) = (if *usdc >= half { (Num::of(*usdc) * 4).0.map(|t| (t, 4)) } else { Some((minimum, 2)) }) else { return false };
        let jupiter = W::JupiterDca { asset: asset.clone(), usdc: Some(total), orders: Some(rounds), every: days.saturating_mul(86400) };
        self.st_mut().dca_choice = Some(command.clone());
        let each = shown(Num::of(total) / rounds, 2);
        self.reply(&self.say(&format!("{} over time can be bought two ways — pick one below.", asset.symbol),
                             &format!("Покупать {} регулярно можно двумя способами — выбери ниже.", asset.symbol)),
                   note.map(Into::into));
        let (total, usdc_text) = (Fmt::num(total, 2), Fmt::num(*usdc, 2));
        self.set_options(vec![
            RouterOption::new(&format!("Jupiter DCA: {rounds} × {each} USDC → {}", asset.symbol),
                              &self.say(&format!("On-chain: {total} USDC goes in now and Jupiter buys every {period}, even with the app closed. From 100 USDC, 50 a round, 0.1% fee. Cancel returns the rest."),
                                        &format!("На блокчейне: {total} USDC вносятся сразу, Jupiter покупает раз в {period}, даже при закрытом приложении. От 100 USDC, по 50 за раз, комиссия 0.1%. Отмена вернёт остаток.")),
                              &jupiter.summary(), *usdc >= half),
            RouterOption::new(&self.say(&format!("In the app: {usdc_text} USDC → {} every {period}", asset.symbol), &format!("В приложении: {usdc_text} USDC → {} раз в {period}", asset.symbol)),
                              &self.say("The money stays in your wallet until each buy, no minimum. Buys while ihatecrypto runs, each signed with Windows Hello or by the automation wallet.",
                                        "Деньги лежат у тебя до каждой покупки, минимума нет. Покупает, пока ihatecrypto работает. Каждая покупка — с Windows Hello или с кошелька автоматизации."),
                              &command.summary(), *usdc < half),
        ]);
        true
    }

    // MARK: A new Hyperliquid account, two ways (13)

    pub const OPEN_VIA: &str = "open hyperliquid via ";

    /// A new Hyperliquid account opens with this trade: the user picks the bridge on two cards, with what each costs and how
    /// long it takes. Relay lands in seconds and carries Hyperliquid's one-time opening fee; NEAR Intents charges none but
    /// takes 2–4 min (the Mac's askOpeningRoute). True when asked: the plan waits for the pick.
    pub async fn ask_opening_route(&self, resolved: &TradeIntent, evaluation: &Evaluation, market: Option<&Market>, note: Option<&str>) -> bool {
        let (asked, evm) = {
            let st = self.st();
            (st.open_via_1click.is_some(), st.evm_address())
        };
        let Some(evm) = evm.filter(|_| !asked) else { return false };
        let amount = if let Some(a) = resolved.amount_usdc {
            Some(Amount::Send(a))
        } else {
            Plan::bridge_need(resolved, evaluation, self.usdc_free("hyperliquid"), market.map(|m| m.mark_px)).map(|need| {
                Amount::Arrive(Executor::bridge_arrive(need, evaluation.sizing.as_ref().map_or(Decimal::ZERO, |s| s.margin), self.usdc_free("solana")))
            })
        };
        let Some(amount) = amount else { return false };
        if Hyperliquid::is_new_account(&evm).await.ok() != Some(true) { return false }
        let Ok(q) = Bridge::dry_quote(amount, true, None, false).await else { return false };
        if !(q.opening && q.alternative_fee.is_some() && q.amount_in <= self.usdc_free("solana").unwrap_or_default()) { return false }
        // ponytail: a quote whose fee is past range isn't offered; the plan goes on as if the account were known.
        let Some(fee) = (Num::of(q.amount_in) - q.reserve - q.amount_out).0 else { return false };
        let other = q.alternative_fee.unwrap_or_default();
        let (relay, near) = if q.provider == "Relay" { (fee, other) } else { (other, fee) };
        self.reply(&self.say("Your Hyperliquid account opens with this trade. How should the USDC come from Solana?", "Аккаунт Hyperliquid откроется этой сделкой. Как довезти USDC с Solana?"),
                   note.map(Into::into));
        let (relay, near) = (Fmt::usd(relay), Fmt::usd(near));
        self.set_options(vec![
            RouterOption::new(&self.say("Open via Relay", "Открыть через Relay"),
                              &self.say(&format!("seconds · ≈ {relay}, incl. Hyperliquid's one-time opening fee"), &format!("секунды · ≈ {relay}, включая разовую плату Hyperliquid за открытие")),
                              &format!("{}relay", Self::OPEN_VIA), false),
            RouterOption::new(&self.say("Open via NEAR Intents", "Открыть через NEAR Intents"),
                              &self.say(&format!("2–4 min · ≈ {near}, no opening fee · the entry waits for the price to hold"), &format!("2–4 мин · ≈ {near}, без платы за открытие · вход — если цена устоит")),
                              &format!("{}oneclick", Self::OPEN_VIA), false),
        ]);
        true
    }

    /// The card picked: the plan is prepared with that way.
    pub async fn opening_route_command(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        if !lower.starts_with(Self::OPEN_VIA) { return false }
        self.st_mut().open_via_1click = Some(lower.ends_with("oneclick"));
        let intent = self.st().intent.clone();
        if intent != TradeIntent::default() { self.plan(&intent, None, "").await }
        true
    }

    // MARK: A waiting card on a canvas left (15)

    /// Leaving a canvas: its waiting card waits there, a quarter of an hour (the Mac's parkedPending). A site's request
    /// isn't a card: it waits in its own block until answered (consent.rs).
    pub fn park_pending(&self) -> bool {
        let mut st = self.st_mut();
        if st.pending.is_none() || st.card.is_none() { return false }
        let (Some(pending), Some(card)) = (st.pending.take(), st.card.take()) else { return false };
        let (canvas, intent) = (st.canvas_id.clone(), st.intent.clone());
        st.parked.insert(canvas, Parked { pending, card, intent, at: chrono::Utc::now() });
        true
    }

    /// Back on a canvas whose move was waiting for Confirm: it waits again; after a quarter of an hour it's said gone.
    pub fn unpark_pending(&self) {
        let parked = {
            let mut st = self.st_mut();
            let canvas = st.canvas_id.clone();
            st.parked.remove(&canvas)
        };
        let Some(p) = parked else { return };
        if chrono::Utc::now() - p.at >= chrono::TimeDelta::minutes(15) {
            let line = self.say("Waited a quarter of an hour: nothing was signed.", "Ждала четверть часа: ничего не подписано.");
            self.update_board(Some(&p.card.id), |b| {
                b.state = Some("cancelled".into());
                b.lines.push(line);
            });
            let mut st = self.st_mut();
            if st.chain_state.after_call.as_ref().is_some_and(|a| a.board == p.card.id) { st.chain_state.after_call = None }
            return;
        }
        let board = p.card.id.clone();
        let waiting = p.pending.clone();
        {
            let mut st = self.st_mut();
            st.pending = Some(p.pending);
            st.card = Some(p.card);
            st.intent = p.intent;
        }
        if let Pending::Wallet(c) = waiting {
            let me = self.rc();
            tokio::task::spawn_local(async move { me.quote_card(&c, &board).await });
        }
    }

    // MARK: The route, step by step (6)

    /// A balance a step changes, read now: `symbol` on `place` and what `delta` makes of it (None: all of it leaves).
    pub fn moved(&self, symbol: &str, place: &str, delta: Option<Decimal>, approx: bool) -> Option<Money> {
        let was = self.held_there(symbol, place)?;
        let after = match delta { Some(delta) => (Num::of(was) + delta).0?, None => Decimal::ZERO }.max(Decimal::ZERO);
        let shown = if symbol.chars().count() > 12 {
            let st = self.st();
            st.portfolio.networks.iter().find(|n| n.key == place).and_then(|n| n.rows.iter().find(|r| r.contract.as_deref() == Some(symbol)))
                .map_or_else(|| symbol.to_string(), |r| r.symbol.clone())
        } else {
            symbol.to_string()
        };
        Some(Money { place: W::place_name(place), was: Self::amount(was, &shown), after: format!("{}{}", if approx { "≈ " } else { "" }, Self::amount(after, &shown)), symbol: shown })
    }

    /// Dollars to the cent; another coin to 4 places from 1 up (2.4722 AERO), 6 under it (0.074852 SOL): the Mac's
    /// MoneyLine.amount.
    pub fn amount(d: Decimal, symbol: &str) -> String {
        let size = d.abs();
        Fmt::num(d, if ["USDC", "USDG", "USDT", "USDC.e", "pathUSD"].contains(&symbol) || size >= Decimal::ONE_HUNDRED { 2 } else if size >= Decimal::ONE { 4 } else { 6 })
    }

    /// Swift's `st(_:_:_:_:)` (`st` is the state's borrow here): one card of a route.
    pub fn step(&self, label: &str, title: &str, line: &str, money: Option<Money>) -> Step {
        Step { label: label.into(), title: title.into(), lines: if line.is_empty() { vec![] } else { vec![line.into()] }, money, status: None }
    }

    pub fn approve(&self, amount: &str, spender: &str) -> Step {
        self.step(&self.say("APPROVE · EXACT", "РАЗРЕШЕНИЕ · ТОЧНО"), amount,
                  &self.say(&format!("exactly this · only {spender} may take it"), &format!("ровно столько · взять может только {spender}")), None)
    }

    /// The Mac's route for a money move: where from, through what, where to, each with the balance it changes; None for a
    /// command drawn as one step (its quotes and way picker live on that step). «all USDC» reads as the amount itself.
    pub fn route_steps(&self, c: &W) -> Option<Vec<Step>> {
        let steps = self.raw_route_steps(c)?;
        let steps = self.amounts_for_all(steps);
        Some(self.with_jupiter(c, steps))
    }

    /// «all USDT» on a step reads as the number held, «all available» small under it (the Mac's step(): the user wants
    /// the number, not «all»); the later steps saying the same «all» take the same number. Left as words when unread.
    pub fn amounts_for_all(&self, steps: Vec<Step>) -> Vec<Step> {
        let mut found: HashMap<String, Decimal> = HashMap::new();
        let all = self.say("all available", "всё, что есть");
        steps.into_iter().map(|mut s| {
            let Some(word) = ["all ", "всё "].into_iter().find(|w| s.title.starts_with(w)) else { return s };
            let unit = s.title[word.len()..].to_string();
            let Some(x) = found.get(&s.title).copied().or_else(|| self.held_for_all(&unit, &s.label)).filter(|x| *x > Decimal::ZERO) else { return s };
            found.insert(s.title.clone(), x);
            s.title = format!("{} {unit}", Fmt::num(x, 8));
            // An approve says exactly what it allows; the others say it's all there is.
            if !s.label.starts_with("APPROVE") && !s.label.starts_with("РАЗРЕШЕНИЕ") {
                let first = s.lines.first().cloned().unwrap_or_default();
                let head = if first.is_empty() { all.clone() } else { format!("{all} · {first}") };
                s.lines = std::iter::once(head).chain(s.lines.into_iter().skip(1)).collect();
            }
            s
        }).collect()
    }

    /// What «all <unit>» comes to at the step's place («FROM · KAMINO», «FROM · BASE»…); None when that isn't read.
    fn held_for_all(&self, unit: &str, label: &str) -> Option<Decimal> {
        let place = label.split('·').skip(1).last().unwrap_or("").trim().to_uppercase();
        // «FROM · AAVE · BASE»: what's lent there, not the wallet's free dollars.
        for what in ["Aave", "Morpho"] {
            if !label.to_uppercase().contains(&what.to_uppercase()) { continue }
            let key = EvmNetwork::all().iter().map(|n| n.key).find(|k| W::place_name(k).to_uppercase() == place)?;
            self.rows_read(key)?;
            return Some(self.deposit_there(&[what], key));
        }
        match place.as_str() {
            "" => None,
            "KAMINO" | "KAMINO LEND" => self.rows_read("solana").map(|_| self.deposit_there(&["Kamino Lend"], "solana")),
            "KAMINO EARN" => self.kamino_vault(unit).map(|v| v.2),
            "JUPITER LEND" => self.rows_read("solana").map(|_| self.deposit_there(&["Jupiter Lend"], "solana")),
            "HLP VAULT" | "ХРАНИЛИЩЕ HLP" => self.st().place_facts.hyperliquid.as_ref().map(|h| h.hlp_equity),
            "JITOSOL" => self.held_there("JitoSOL", "solana"),
            _ => {
                let key = ["solana", "hyperliquid", "near"].into_iter().chain(EvmNetwork::all().iter().map(|n| n.key))
                    .find(|k| W::place_name(k).to_uppercase() == place || k.to_uppercase() == place)?;
                // A contract as tokenName writes it, «AI (0x2E8c…1e18)» (⚠ before a lookalike): its row by the contract's ends.
                if let Ok(Some(m)) = CONTRACT_ENDS.captures(unit) {
                    let (head, tail) = (m[1].to_lowercase(), m[2].to_lowercase());
                    return self.rows_read(key).and_then(|rows| {
                        sum(rows.iter().filter(|r| r.section == "tokens" && r.contract.as_ref().is_some_and(|c| {
                            let c = c.to_lowercase();
                            c.starts_with(&head) && c.ends_with(&tail)
                        })).map(|r| r.value))
                    });
                }
                self.held_there(unit, key)
            }
        }
    }

    fn raw_route_steps(&self, c: &W) -> Option<Vec<Step>> {
        let say = |en: &str, ru: &str| self.say(en, ru);
        let st = |label: &str, title: &str, line: &str, money: Option<Money>| self.step(label, title, line, money);
        let n = |x: Option<Decimal>, unit: &str| x.map_or_else(|| say(&format!("all {unit}"), &format!("всё {unit}")), |x| format!("{} {unit}", Fmt::num(x, 6)));
        let neg = |x: Option<Decimal>| x.map(|v| -v);
        let usd_line = |x: Decimal| Fmt::num(x, 2);
        let sol_usdc = self.usdc_free("solana").map_or_else(String::new, |x| say(&format!("free: {} USDC", usd_line(x)), &format!("свободно: {} USDC", usd_line(x))));
        let sol_sol = self.held_there("SOL", "solana").map_or_else(String::new, |x| say(&format!("free: {} SOL", Fmt::num(x, 4)), &format!("свободно: {} SOL", Fmt::num(x, 4))));
        let own = say("your own address ✓", "твой собственный адрес ✓");
        let relay = st(&say("BRIDGE · RELAY", "МОСТ · RELAY"), "", &say("a few seconds", "несколько секунд"), None);
        let bridged = |title: String, line: String| Step { title, lines: vec![line], ..relay.clone() };
        let from_solana = say("FROM · SOLANA", "ОТКУДА · SOLANA");
        let to_solana = say("TO · SOLANA", "КУДА · SOLANA");
        let wallet = say("your wallet", "твой кошелёк");
        let approx_usdc = |x: Option<Decimal>| x.map_or_else(|| "USDC".to_string(), |x| format!("≈ {} USDC", usd_line(x)));
        let saved = |e: &SignerAddress| {
            let (start, end) = address_check::ends(&e.address);
            st(&(say("TO · ", "КУДА · ") + &say("SAVED ADDRESS", "СОХРАНЁННЫЙ АДРЕС")), &e.name, &format!("{start} … {end}"), None)
        };
        Some(match c {
            W::WithdrawToSolana(asked) => {
                let x = asked.or_else(|| self.usdc_free("hyperliquid"));
                vec![st(&say("FROM · HYPERLIQUID", "ОТКУДА · HYPERLIQUID"), &n(x, "USDC"), &say("position margin isn’t touched", "маржа позиций не трогается"), self.moved("USDC", "hyperliquid", neg(x), false)),
                     bridged(n(x, "USDC"), say("≈ 4 s · small Relay fee", "≈ 4 с · небольшая комиссия Relay")),
                     st(&to_solana, &approx_usdc(x), &own, x.and_then(|x| self.moved("USDC", "solana", Some(x), true)))]
            }
            W::SolanaTo(place, Some(x)) => {
                let (name, stable) = (W::place_name(place), if place == "robinhood" { "USDG" } else { "USDC" });
                let coin = EvmNetwork::named(place).map_or("ETH", |n| n.gas_coin);
                vec![st(&from_solana, &n(Some(*x), "USDC"), &sol_usdc, self.moved("USDC", "solana", Some(-*x), false)),
                     bridged(n(Some(*x), "USDC"), if place == "hyperliquid" {
                         say("a few seconds · a first deposit pays Hyperliquid's ≈ $1 opening fee", "несколько секунд · первый депозит платит ≈ $1 Hyperliquid за открытие")
                     } else {
                         say(&format!("a few seconds · + {coin} for gas when there's none"), &format!("несколько секунд · + {coin} на газ, если его нет"))
                     }),
                     st(&(say("TO · ", "КУДА · ") + &name.to_uppercase()), &format!("≈ {} {stable}", usd_line(*x)), &own, self.moved(stable, place, Some(*x), true))]
            }
            W::EvmToSolana(net, x) => self.back_route(net, *x),
            W::Bridge { from, to, amount } if to == "solana" => self.back_route(from, *amount),
            W::BaseToSolana(x) => self.back_route("base", *x),
            W::ArbitrumToSolana(x) => self.back_route("arbitrum", *x),
            W::RobinhoodToSolana(x) => self.back_route("robinhood", *x),
            W::Bridge { from, to, amount } => {
                let (a, b) = (W::place_name(from), W::place_name(to));
                let moving = amount.or_else(|| self.usdc_free(from));
                // The bridge it goes by once quoted (Relay, unless CCTP or 1Click lands more), with what each costs and lands.
                let ways = self.quotes(c);
                let taken = ways.as_ref().and_then(|w| w.first()).map_or("relay", |w| w.way.as_str());
                let spender = match taken { "cctp" => "Circle's CCTP", "1click" => "NEAR Intents", _ => "Relay" };
                let lands = ways.as_ref().and_then(|w| w.first()).map(|w| w.gets.split(" · ").next().unwrap_or("").to_string()).unwrap_or_else(|| approx_usdc(moving));
                let free = self.usdc_free(from).map_or_else(String::new, |x| say(&format!("free there: {} USDC", usd_line(x)), &format!("свободно там: {} USDC", usd_line(x))));
                let fallback = (say("BRIDGE · RELAY", "МОСТ · RELAY"), say(&format!("{a} → {b} · no stop on Solana"), &format!("{a} → {b} · без остановки на Solana")));
                vec![st(&(say("FROM · ", "ОТКУДА · ") + &a.to_uppercase()), &n(moving, "USDC"), &free, self.moved("USDC", from, neg(moving), false)),
                     self.approve(&n(moving, "USDC"), spender),
                     self.bridge_step(ways.as_deref(), &n(moving, "USDC"), (&fallback.0, &fallback.1)),
                     st(&(say("TO · ", "КУДА · ") + &b.to_uppercase()), &lands, &own,
                        moving.and_then(|x| self.moved(if to == "robinhood" { "USDG" } else { "USDC" }, to, Some(x), true)))]
            }
            W::Aave { deposit: true, amount: Some(x), network: net } | W::Morpho { deposit: true, amount: Some(x), network: net } => {
                let morpho = matches!(c, W::Morpho { .. });
                let (name, what) = (W::place_name(net), if morpho { "Morpho" } else { "Aave" });
                let have = self.usdc_free(net).unwrap_or_default();
                let short = (Num::of(*x) - have).0.unwrap_or_default().max(Decimal::ZERO);
                let need_gas = self.gas_low(net);
                let lent = self.deposit_there(&[what], net);
                let mut steps: Vec<Step>;
                if net == "base" {
                    // The Mac's aaveRoute on Base: what Solana sends with Relay's fee, the gas coming along, the rate read.
                    let over = short.max(if need_gas { d("0.5") } else { Decimal::ZERO });
                    let sends = (Num::of(over) + if need_gas { d("0.14") } else { d("0.03") }).0.unwrap_or(over);
                    let rate = self.st().place_facts.rates.get(if morpho { "morpho" } else { "aave" }).copied();
                    let rate = rate.map_or_else(|| say("rate from the pool", "ставка пула"), |r| say(&format!("≈ {}% a year", Fmt::num(r, 2)), &format!("≈ {}% в год", Fmt::num(r, 2))));
                    steps = if short > Decimal::ZERO || need_gas {
                        let mut from = vec![
                            st(&from_solana, &self.usdc_free("solana").map_or_else(|| "—".to_string(), |x| format!("{} USDC", usd_line(x))),
                               &say(&format!("sends ≈ {} USDC", usd_line(sends)), &format!("отправит ≈ {} USDC", usd_line(sends))), self.moved("USDC", "solana", Some(-sends), true)),
                            st(&say("BRIDGE · RELAY", "МОСТ · RELAY"), &format!("{} USDC → Base", usd_line(over)),
                               &if need_gas { say("+ $0.03 of ETH for gas · ≈ 2 s", "+ $0.03 в ETH на газ · ≈ 2 с") } else { say("≈ 2 s", "≈ 2 с") },
                               self.moved("USDC", "base", Some(over), true)),
                        ];
                        if need_gas {
                            from.push(st(&say("GAS · COMES ALONG", "ГАЗ · ПРИДЁТ С МОСТОМ"), &say("≈ $0.03 of ETH", "≈ $0.03 в ETH"),
                                         &say("there’s no ETH on Base yet · it comes with the bridge", "ETH на Base пока нет · он придёт с мостом"), None));
                        }
                        from
                    } else {
                        vec![st(&say("ON BASE", "В BASE"), &format!("{} USDC", usd_line(have)), &say("already there, gas too", "уже там, газ тоже"), self.moved("USDC", net, Some(-*x), false))]
                    };
                    let vault = if morpho { format!("MORPHO · {}", evm::protocols::morpho::VAULTS[0].name.to_uppercase()) } else { "AAVE V3 · BASE".into() };
                    steps.push(self.approve(&n(Some(*x), "USDC"), what));
                    steps.push(st(&vault, &n(Some(*x), "USDC"), &rate, into_place(what, "USDC", lent, Some(*x))));
                    return Some(steps);
                }
                let coin = EvmNetwork::named(net).map_or("ETH", |n| n.gas_coin);
                let arrive = short.max(d("0.5"));
                // The Mac's lendingRoute: no gas there brings it from Solana too, with half a dollar at least.
                steps = if short > Decimal::ZERO || need_gas {
                    let already = if have > Decimal::ZERO { say(&format!("{} already on {name} · ", usd_line(have)), &format!("{} уже в {name} · ", usd_line(have))) } else { String::new() };
                    vec![st(&from_solana, &n(Some(arrive), "USDC"), &(already + &sol_usdc), self.moved("USDC", "solana", Some(-arrive), false)),
                         st(&say("BRIDGE · RELAY", "МОСТ · RELAY"), &format!("USDC → {name}"),
                            &if need_gas { say(&format!("+ {coin} for gas · ≈ 2 s"), &format!("+ {coin} на газ · ≈ 2 с")) } else { say("≈ 2 s", "≈ 2 с") },
                            self.moved("USDC", net, Some(arrive), true))]
                } else {
                    vec![st(&(say("ON ", "В ") + &name.to_uppercase()), &format!("{} USDC", usd_line(have)), &say("already there, gas too", "уже там, газ тоже"), self.moved("USDC", net, Some(-*x), false))]
                };
                steps.push(self.approve(&n(Some(*x), "USDC"), what));
                steps.push(st(&format!("{}{}", if morpho { "MORPHO · " } else { "AAVE V3 · " }, name.to_uppercase()), &n(Some(*x), "USDC"),
                              &say(&format!("rate from the {}", if morpho { "vault" } else { "pool" }), &format!("ставка {}", if morpho { "хранилища" } else { "пула" })),
                              into_place(what, "USDC", lent, Some(*x))));
                steps
            }
            W::Aave { deposit: false, amount: x, network: net } | W::Morpho { deposit: false, amount: x, network: net } => {
                let what = if matches!(c, W::Morpho { .. }) { "Morpho" } else { "Aave" };
                let lent = self.deposit_there(&[what], net);
                let moving = x.or((lent > Decimal::ZERO).then_some(lent));
                vec![st(&format!("{}{} · {}", say("FROM · ", "ОТКУДА · "), what.to_uppercase(), W::place_name(net).to_uppercase()), &n(moving, "USDC"),
                        &say("with the interest", "с процентами"), into_place(what, "USDC", lent, neg(moving))),
                     self.approve(&n(moving, "USDC"), "Relay"),
                     bridged(n(moving, "USDC"), say("a few seconds", "несколько секунд")),
                     st(&to_solana, &approx_usdc(moving), &own, moving.and_then(|x| self.moved("USDC", "solana", Some(x), true)))]
            }
            W::LendDeposit(s, x) => {
                let a = x.or_else(|| self.held_there(s, "solana"));
                vec![st(&from_solana, &n(*x, s), if s == "SOL" { &sol_sol } else { &sol_usdc }, self.moved(s, "solana", neg(a), false)),
                     st(&say("INTO · JUPITER LEND", "В · JUPITER LEND"), &n(*x, s), &say("earns the vault rate · withdraw any time", "приносит ставку хранилища · вывести можно в любой момент"),
                        if s == "USDC" { into_place("Jupiter Lend", s, self.deposit_there(&["Jupiter Lend"], "solana"), a) } else { None })]
            }
            W::LendWithdraw(s, x) => {
                let lent = self.deposit_there(&["Jupiter Lend"], "solana");
                let moving = x.or((s == "USDC" && lent > Decimal::ZERO).then_some(lent));
                let held = if lent > Decimal::ZERO { say(&format!("in the vault: {} USDC", usd_line(lent)), &format!("в хранилище: {} USDC", usd_line(lent))) } else { String::new() };
                vec![st(&say("FROM · JUPITER LEND", "ОТКУДА · JUPITER LEND"), &n(moving, s), &held, if s == "USDC" { into_place("Jupiter Lend", s, lent, neg(moving)) } else { None }),
                     st(&to_solana, &n(moving, s), &wallet, moving.and_then(|x| self.moved(s, "solana", Some(x), false)))]
            }
            W::Kamino { deposit: true, symbol: s, amount: x } => {
                let a = x.or_else(|| self.held_there(s, "solana"));
                vec![st(&from_solana, &n(*x, s), if s == "SOL" { &sol_sol } else { &sol_usdc }, self.moved(s, "solana", neg(a), false)),
                     st(&say("INTO · KAMINO LEND", "В · KAMINO LEND"), &n(*x, s), &say("first deposit pays ≈ 0.03 SOL rent", "первый депозит берёт ≈ 0.03 SOL залога"),
                        if s == "USDC" { into_place("Kamino Lend", s, self.deposit_there(&["Kamino Lend"], "solana"), a) } else { None })]
            }
            W::Kamino { deposit: false, symbol: s, amount: x } => {
                // From Lend when it holds anything, else the Earn vault holding the coin (the Mac's kaminoVault).
                let (lent, vault) = (self.deposit_there(&["Kamino Lend"], "solana"), self.kamino_vault(s));
                let moving = x.or(vault.as_ref().map(|v| v.2)).or((s == "USDC" && lent > Decimal::ZERO).then_some(lent));
                let label = if vault.is_none() { say("FROM · KAMINO LEND", "ОТКУДА · KAMINO LEND") } else { say("FROM · KAMINO EARN", "ОТКУДА · KAMINO EARN") };
                vec![st(&label, &n(moving, s), &vault.as_ref().map_or_else(String::new, |v| format!("{}: {}", v.0, v.1)),
                        if vault.is_none() && s == "USDC" { into_place("Kamino Lend", s, lent, neg(moving)) } else { None }),
                     st(&to_solana, &n(moving, s), &wallet, moving.and_then(|x| self.moved(s, "solana", Some(x), false)))]
            }
            W::Stake(x) => vec![
                st(&from_solana, &n(*x, "SOL"), &sol_sol, self.moved("SOL", "solana", neg(*x), false)),
                st(&say("INTO · JITOSOL", "В · JITOSOL"), &say("liquid staking", "ликвидный стейкинг"),
                   &say("earns the staking yield · swap back any time", "приносит доход стейкинга · обменять обратно можно в любой момент"), None),
            ],
            W::Unstake(x) => vec![
                st(&say("FROM · JITOSOL", "ОТКУДА · JITOSOL"), &n(*x, "JitoSOL"), "", None),
                st(&to_solana, "SOL", &wallet, None),
            ],
            W::Hlp { deposit: true, amount: x } => {
                let a = x.or_else(|| self.usdc_free("hyperliquid"));
                let out = chrono::Local::now().checked_add_days(chrono::Days::new(4)).map_or_else(chrono::Utc::now, |d| d.with_timezone(&chrono::Utc));
                let free = self.usdc_free("hyperliquid").map_or_else(String::new, |x| say(&format!("free: {}", Fmt::usd(x)), &format!("свободно: {}", Fmt::usd(x))));
                vec![st(&say("FROM · HYPERLIQUID", "ОТКУДА · HYPERLIQUID"), &n(*x, "USDC"), &free, self.moved("USDC", "hyperliquid", neg(a), false)),
                     st(&say("INTO · HLP VAULT", "В · ХРАНИЛИЩЕ HLP"), &n(*x, "USDC"), &say("min $5 · 4-day lockup", "от $5 · блокировка 4 дня"), into_place("HLP", "USDC", self.hlp_equity(), a)),
                     st(&say("LOCKED · 4 DAYS", "ЗАБЛОКИРОВАНО · 4 ДНЯ"), &n(*x, "USDC"),
                        &say(&format!("HLP lets it out from {}", Fmt::date(out)), &format!("HLP выпустит его с {}", Fmt::date(out))), None)]
            }
            W::Hlp { deposit: false, amount: x } => {
                let equity = self.hlp_equity();
                let moving = x.or((equity > Decimal::ZERO).then_some(equity));
                vec![st(&say("FROM · HLP VAULT", "ОТКУДА · ХРАНИЛИЩЕ HLP"), &n(moving, "USDC"), "", into_place("HLP", "USDC", equity, neg(moving))),
                     st(&say("TO · HYPERLIQUID", "КУДА · HYPERLIQUID"), "USDC", &say("trading balance", "торговый баланс"), moving.and_then(|x| self.moved("USDC", "hyperliquid", Some(x), false)))]
            }
            W::Buy { asset, usdc } => vec![
                st(&from_solana, &n(*usdc, "USDC"), &sol_usdc, self.moved("USDC", "solana", neg(*usdc), false)),
                st(&say("BUY · JUPITER", "ПОКУПКА · JUPITER"), &asset.symbol, &say("best route, simulated first", "лучший маршрут, сначала симуляция"), None),
            ],
            W::Sell { asset, amount: x } => {
                let key = if asset.symbol == "SOL" || asset.mint.is_empty() { asset.symbol.clone() } else { asset.mint.clone() };
                let held = self.held_there(&key, "solana").map_or_else(String::new, |h| {
                    say(&format!("held: {} {}", Fmt::num(h, 6), asset.symbol), &format!("есть: {} {}", Fmt::num(h, 6), asset.symbol))
                });
                vec![st(&say("SELL · JUPITER", "ПРОДАЖА · JUPITER"), &n(*x, &asset.symbol), &held, self.moved(&key, "solana", neg(*x), false)),
                     st(&to_solana, "USDC", &wallet, None)]
            }
            W::SwapOn { place, from, to, amount: x } if place == "solana" => vec![
                st(&from_solana, &n(*x, &from.to_uppercase()), &wallet, self.moved(from, "solana", neg(*x), false)),
                st(&say("SWAP · JUPITER", "ОБМЕН · JUPITER"), &to.to_uppercase(), &say("Jupiter's best route", "лучший маршрут Jupiter"), None),
                st(&to_solana, &to.to_uppercase(), &say("your wallet ✓", "твой кошелёк ✓"), None),
            ],
            W::JupiterDca { asset, usdc: u, orders: o, every: e } => {
                let orders = o.or(u.map(W::jupiter_dca_orders)).unwrap_or(2);
                let each = u.map_or_else(|| "?".to_string(), |u| shown(Num::of(u) / orders, 2));
                vec![st(&from_solana, &n(*u, "USDC"), &say("all goes in now", "вносится всё сразу"), self.moved("USDC", "solana", neg(*u), false)),
                     st(&format!("{}{} · JUPITER DCA", say("EVERY ", "КАЖДЫЕ "), W::every_text(*e).to_uppercase()), &format!("{orders} × {each} USDC → {}", asset.symbol),
                        &say("Jupiter's keepers buy on-chain, the first now · 0.1% fee · cancel any time",
                             "Jupiter покупает на блокчейне, первая покупка сейчас · комиссия 0.1% · отменить можно в любой момент"), None)]
            }
            W::JupiterDcaCancel(a) => vec![
                st(&say("FROM · JUPITER DCA", "ОТКУДА · JUPITER DCA"),
                   &a.as_ref().map_or_else(|| say("all orders", "все ордера"), |a| say(&format!("{} orders", a.symbol), &format!("ордера {}", a.symbol))),
                   &say("what they haven't spent", "то, что они не потратили"), None),
                st(&to_solana, "USDC", &say("your wallet · the ≈ 0.005 SOL rent too", "твой кошелёк · и ≈ 0.005 SOL залога"), None),
            ],
            W::SendSol { amount: x, .. } => {
                let Ok(Some(e)) = self.native_recipient(c) else { return None };
                vec![st(&from_solana, &n(Some(*x), "SOL"), &sol_sol, self.moved("SOL", "solana", Some(-*x), false)), saved(&e)]
            }
            W::SendToken { name, token, amount: x, network: net } => {
                if let Ok(Some(r)) = self.across_recipient(name, token, net.as_deref()) { return Some(self.across_steps(c, &r, *x)) }
                let Ok((e, network)) = self.token_recipient(name, net.as_deref()) else { return None };
                let place = W::place_name(&network);
                let held = self.held_there(token, &network).map_or_else(String::new, |h| say(&format!("held: {}", Fmt::num(h, 6)), &format!("есть: {}", Fmt::num(h, 6))));
                vec![st(&(say("FROM · ", "ОТКУДА · ") + &place.to_uppercase()), &n(*x, &token.to_uppercase()), &held, self.moved(token, &network, neg(*x), false)), saved(&e)]
            }
            // What Merkl owes, once read for the waiting card (quote_route): before that, one step with the command's words.
            W::ClaimRewards | W::SweepRewards { .. } => {
                let (owed, card) = {
                    let st = self.st();
                    (st.claim_owed.clone(), st.card.as_ref().map(|c| c.id.clone()))
                };
                let o = owed?;
                if o.command != *c || Some(&o.board) != card.as_ref() { return None }
                self.claim_steps(&o.owed, c)
            }
            W::SweepDust { under } => {
                let dust = self.dust_tokens(*under);
                if dust.is_empty() { return None }
                let mut steps: Vec<Step> = dust.iter().take(8).map(|d| {
                    st(&(say("SELL · ", "ПРОДАЖА · ") + &W::place_name(&d.chain).to_uppercase()), &d.symbol, &format!("≈ {} → USDC", Fmt::usd(d.usd)), self.moved(&d.contract, &d.chain, None, false))
                }).collect();
                if dust.len() > 8 {
                    let more = dust.len() - 8;
                    steps.push(st(&say(&format!("AND {more} MORE"), &format!("И ЕЩЁ {more}")), &say(&format!("{more} tokens"), &format!("{more} токенов")), &say("the same way", "так же"), None));
                }
                let total = sum(dust.iter().map(|d| d.usd)).map_or_else(|| "NaN".to_string(), Fmt::usd);
                steps.push(st(&say("TO · USDC", "В · USDC"), &format!("≈ {total}"), &say("on each network · your own address ✓", "в каждой сети · твой собственный адрес ✓"), None));
                steps
            }
            W::Gas { place, usd } => {
                let (name, coin, u) = (W::place_name(place), EvmNetwork::named(place).map_or("ETH", |n| n.gas_coin), usd.unwrap_or(Decimal::ONE));
                let paid = (Num::of(u) + d("0.58")).0;
                vec![st(&from_solana, &format!("{} USDC", shown(Num(paid), 2)), &sol_usdc, paid.and_then(|p| self.moved("USDC", "solana", Some(-p), true))),
                     bridged(format!("{} of {coin}", Fmt::usd(u)), say("+ 0.5 USDC along · a few seconds", "+ 0.5 USDC вместе · несколько секунд")),
                     st(&(say("GAS · ", "ГАЗ · ") + &name.to_uppercase()), &format!("≈ {} of {coin}", Fmt::usd(u)), &say("for the fees of the next moves there", "на комиссии следующих действий там"), None)]
            }
            W::HlSpotBuy { symbol, usdc: Some(u) } => {
                let gets = self.quotes(c).and_then(|w| w.first().map(|w| w.gets.clone()));
                let up = symbol.to_uppercase();
                vec![st(&say("FROM · HYPERLIQUID", "ОТКУДА · HYPERLIQUID"), &n(Some(*u), "USDC"), &say("spot balance · perp tops it up when short", "спотовый баланс · при нехватке доливается с perp"), None),
                     st(&say("BUY · SPOT ORDER", "ПОКУПКА · СПОТ"), &up,
                        &gets.as_ref().map_or_else(|| say("IOC · at most 1% over the mid", "IOC · не дороже середины на 1%"),
                                                   |g| say(&format!("≈ {g} · at most 1% over the mid"), &format!("≈ {g} · не дороже середины на 1%"))), None),
                     st(&say("TO · SPOT BALANCE", "КУДА · СПОТОВЫЙ БАЛАНС"), &up, &say("on Hyperliquid, yours ✓", "на Hyperliquid, твой ✓"),
                        Self::quoted_amount(gets.as_deref()).map(|x| self.gained(symbol, &up, "hyperliquid", x)))]
            }
            W::HlSpotSell { symbol, amount: x } => {
                let gets = self.quotes(c).and_then(|w| w.first().map(|w| w.gets.clone()));
                vec![st(&say("FROM · SPOT BALANCE", "ОТКУДА · СПОТОВЫЙ БАЛАНС"), &n(*x, &symbol.to_uppercase()), &say("on Hyperliquid", "на Hyperliquid"), self.moved(symbol, "hyperliquid", neg(*x), false)),
                     st(&say("SELL · SPOT ORDER", "ПРОДАЖА · СПОТ"), "→ USDC",
                        &gets.as_ref().map_or_else(|| say("IOC · at most 1% under the mid", "IOC · не дешевле середины на 1%"),
                                                   |g| say(&format!("≈ {g} · at most 1% under the mid"), &format!("≈ {g} · не дешевле середины на 1%"))), None),
                     st(&say("TO · SPOT USDC", "КУДА · СПОТ USDC"), "USDC", &say("on Hyperliquid, yours ✓", "на Hyperliquid, твой ✓"), None)]
            }
            _ => return self.card_route(c),
        })
    }

    /// An EVM network's USDC (USDG on Robinhood Chain) back to Solana: an exact approve and Relay's deposit.
    fn back_route(&self, net: &str, x: Option<Decimal>) -> Vec<Step> {
        let (name, stable) = (W::place_name(net), if net == "robinhood" { "USDG" } else { "USDC" });
        let held = self.usdc_free(net);
        let moving = x.map(|x| x.min(held.unwrap_or(x))).or(held);
        let n = |d: Option<Decimal>| d.map_or_else(|| self.say(&format!("all {stable}"), &format!("всё {stable}")), |d| format!("{} {stable}", Fmt::num(d, 2)));
        let free = held.map_or_else(String::new, |h| self.say(&format!("free there: {} {stable}", Fmt::num(h, 2)), &format!("свободно там: {} {stable}", Fmt::num(h, 2))));
        vec![self.step(&(self.say("FROM · ", "ОТКУДА · ") + &name.to_uppercase()), &n(moving), &free, self.moved(stable, net, moving.map(|m| -m), false)),
             self.approve(&n(moving), "Relay"),
             self.step(&self.say("BRIDGE · RELAY", "МОСТ · RELAY"), &n(moving), &self.say("≈ $0.02 · a few seconds", "≈ $0.02 · несколько секунд"), None),
             self.step(&self.say("TO · SOLANA", "КУДА · SOLANA"), &moving.map_or_else(|| "USDC".to_string(), |m| format!("≈ {} USDC", Fmt::num(m, 2))),
                       &self.say("your own address ✓", "твой собственный адрес ✓"), moving.and_then(|m| self.moved("USDC", "solana", Some(m), true)))]
    }
}

#[cfg(test)]
#[path = "move_checks/tests.rs"]
mod tests;

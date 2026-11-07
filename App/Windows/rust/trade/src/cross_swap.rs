//! Any token on one network for any token on another through Relay, in one signature (CrossSwap.swift): SOL on Solana
//! for ETH on Arbitrum, AERO on Base for SOL on Solana. Relay's solver swaps on the way. What goes out is exactly the
//! amount said; what comes in is judged by value — at most 3% or 15 cents lost, as Relay's own sale. From Solana: Relay's
//! Depository deposit (SOL, USDC, a token it takes as is); a route through other programs is refused by the guard. From an
//! EVM network: an exact approve to Relay's approval proxy and its call, or to its depository for a token it holds as is
//! (USDC) — one EIP-7702 transaction where it batches — or the network's coin sent to the depository, an address the
//! signer allows in person once.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use evm::abi::address_in;
use execution::Kind;
use guard::sol::{BalanceRule, Policy};
use ihatecrypto_core::foundation::{Num, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::modules::ModuleProtocol;
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::{stablecoins, token_research};
use protocols::relay;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use solana::message::Message;
use solana::spl;

use crate::aave::EvmNetwork;
use crate::executor::BridgeStep;
use crate::swap::{approves, d, dec, no_value, pairs, scale, sleep, tokens, wei, whole};
use crate::{Error, Executor, Result, fail, modules, num, stop, u256, word};

/// One side of a swap across networks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossSide {
    pub chain: u64,
    pub currency: String,
    pub decimals: u32,
    pub symbol: String,
    /// The network's own coin: SOL, ETH, POL.
    pub coin: bool,
    pub token2022: bool,
}

/// Relay's checked quote for a swap across networks (cross_quote).
#[derive(Debug, Clone, PartialEq)]
pub struct CrossQuote {
    pub quote: relay::Quote,
    pub origin: CrossSide,
    pub destination: CrossSide,
    pub units: Decimal,
}

fn place_name(place: &str) -> String { WalletCommand::place_name(place) }

impl Executor {
    pub const EVM_COIN: &str = "0x0000000000000000000000000000000000000000";

    pub async fn cross_side(&self, token: &str, place: &str) -> Result<CrossSide> {
        let side = |chain, currency: &str, decimals, symbol: &str, coin| CrossSide { chain, currency: currency.into(), decimals, symbol: symbol.into(), coin, token2022: false };
        if place == "solana" {
            return match token.to_uppercase().as_str() {
                "SOL" => Ok(side(relay::chain::SOLANA, relay::currency::SOLANA_SOL, 9, "SOL", true)),
                "USDC" => Ok(side(relay::chain::SOLANA, relay::currency::SOLANA_USDC, 6, "USDC", false)),
                _ => {
                    // USDT, USDS and USDe by their mints (Stablecoins): a ticker search can land on a lookalike.
                    let stable = stablecoins::find(token, "solana").map(|c| c.address.to_string());
                    // Jupiter not answering throws its own words (token_research's get): «knows no verified» is only its «no».
                    let mint = if token.chars().count() >= 32 { Some(token.to_string()) } else if stable.is_some() { stable } else { token_research::solana_mint(token).await? };
                    let info = match &mint { Some(m) => token_research::solana_token(m).await?, None => None };
                    let (Some(mint), Some(info)) = (mint, info) else { return fail(format!("Jupiter knows no verified {token} on Solana. Nothing was signed.")) };
                    Ok(CrossSide { token2022: info.token2022, ..side(relay::chain::SOLANA, &mint, u32::try_from(info.decimals).unwrap_or(0), &info.symbol, false) })
                }
            };
        }
        let Some(net) = EvmNetwork::named(place) else { return fail("Relay swaps between Solana, Base, Arbitrum, Ethereum, Polygon and Robinhood Chain.") };
        if Self::is_coin(token, net) { return Ok(side(net.relay_chain, Self::EVM_COIN, 18, net.gas_coin, true)) }
        let t = self.evm_token(token, net).await?;
        Ok(side(net.relay_chain, &t.address, t.decimals, &t.symbol, false))
    }

    /// Relay's quote for the swap, checked: from this wallet to this wallet, and no more value lost than allowed.
    /// `amount` None = all of a token held now (never all of a coin: its fees come from it). `capped` false: for the card,
    /// which shows the cost instead of stopping at it, priced at the amount asked even when the wallet holds less (the run
    /// checks it). No dollar price for what goes in stops both, so the card shows the way Confirm takes.
    pub async fn cross_quote(&self, a: &str, from: &str, b: &str, to: &str, amount: Option<Decimal>, capped: bool) -> Result<CrossQuote> {
        if from == to { return fail(format!("Both sides are on {}: that's a swap there, not between networks.", place_name(from))) }
        let (origin, destination) = tokio::join!(self.cross_side(a, from), self.cross_side(b, to));
        let (origin, destination) = (origin?, destination?);
        let (sol, evm) = (self.wallet.solana.as_str(), self.wallet.evm.as_str());
        let held = if from == "solana" {
            Decimal::from(if origin.coin { self.rpc.balance(sol).await? } else { self.rpc.token_balance(sol, &origin.currency).await? })
        } else {
            let Some(net) = EvmNetwork::named(from) else { return fail("Relay swaps between Solana, Base, Arbitrum, Ethereum, Polygon and Robinhood Chain.") };
            if origin.coin { wei(net.rpc.balance(evm).await?)? } else { self.balance(&origin.currency, &net.rpc).await? }
        };
        // «All» is what was just read, priced here: a second pass would read it again and drop `capped`.
        if amount.is_none() {
            if origin.coin { return fail(format!("Say how much {}: its network fees come out of it. Nothing was signed.", origin.symbol)) }
            if held <= Decimal::ZERO { return fail(format!("No {} on {}.", origin.symbol, place_name(from))) }
        }
        let amount = amount.unwrap_or_else(|| tokens(held, origin.decimals));
        let units = whole(num(Num::of(amount) * scale(origin.decimals))?);
        if units <= Decimal::ZERO { return fail(format!("{} {} is less than its smallest unit.", Fmt::num(amount, 2), origin.symbol)) }
        // A coin goes as one transaction's value, under 2⁶⁴ wei (the signer's u64): 18.44 coins.
        if origin.coin && units > Decimal::from(u64::MAX) { return fail(format!("At most 18 {} at once. Nothing was signed.", origin.symbol)) }
        if units > held && capped {
            return fail(format!("{} {} on {}, and {} is more. Nothing was signed.", Fmt::num(tokens(held, origin.decimals), 8), origin.symbol, place_name(from),
                                Fmt::num(amount, 8)));
        }
        let me = if from == "solana" { sol } else { evm };
        let recipient = if to == "solana" { sol } else { evm };
        let quote = self.relay_quote(&relay::Ask {
            user: me.into(), recipient: recipient.into(), origin_chain: origin.chain, destination_chain: destination.chain, origin_currency: origin.currency.clone(),
            destination_currency: destination.currency.clone(), units: Some(text(units)), ..Default::default()
        }).await?;
        let binding = guard::binding_check(&pairs(&[("sender", &quote.sender), ("recipient", &quote.recipient)]), &pairs(&[("sender", me), ("recipient", recipient)]),
                                           "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let (worth_in, worth_out) = (dec(&quote.amount_in_usd), dec(&quote.amount_out_usd));
        if worth_out <= Decimal::ZERO { return fail(format!("Relay doesn't quote {} for {} now. Nothing was signed.", origin.symbol, destination.symbol)) }
        Self::holds_cross_loss(worth_in, worth_out, &format!("Relay doesn't price {} on {}", origin.symbol, place_name(from)), "Relay's swap", capped)?;
        Ok(CrossQuote { quote, origin, destination, units })
    }

    /// A swap across networks judged by value: no dollar price for either side stops it before signing (read as $0, it
    /// let any loss through, or asked about a loss that wasn't); `capped`, at most 3% (or $0.15) lost, past that a
    /// CostlyMove to ask about.
    pub fn holds_cross_loss(worth_in: Decimal, worth_out: Decimal, unpriced: &str, what: &str, capped: bool) -> Result<()> {
        if !(worth_in > Decimal::ZERO && worth_out > Decimal::ZERO) {
            return fail(format!("{unpriced} now, so what {what} keeps can't be checked. Nothing was signed."));
        }
        if !capped { return Ok(()) }
        Self::check_loss(num(Num::of(worth_in) - worth_out)?, worth_in, num(Num::of(worth_in) * d("0.03"))?.max(d("0.15")), what)
    }

    /// A failure of Relay's that NEAR Intents may take over: no route, no quote, the network. Never a stop by the guard or
    /// the signer (nothing is offered around it), nor a loss past the cap («Send anyway» asks about this route). The card
    /// and the run go by this one rule, so the card shows the way Confirm takes.
    pub fn intents_may_take(error: &Error) -> bool { !matches!(error, Error::Guard(_) | Error::Costly(_)) }

    /// The swap's quote for the card: what arrives, its worth, and how long Relay says it takes. Nothing is signed.
    /// NEAR's side, or a pair Relay has no route for, is NEAR Intents' (intent_swap.rs). A stop by the guard or the
    /// signer on Relay's quote stays a stop: no quote, not NEAR's. (gets, way)
    pub async fn cross_quote_text(&self, a: &str, from: &str, b: &str, to: &str, amount: Option<Decimal>, near: Option<&str>) -> Result<(String, String)> {
        if from == "near" || to == "near" {
            let Some(amount) = amount else { return fail(format!("Say how much {}.", a.to_uppercase())) };
            return self.intent_quote_text(a, from, b, to, amount, near, false).await;
        }
        match self.cross_quote(a, from, b, to, amount, false).await {
            Ok(q) => Ok((format!("{} {} ≈ {}", Fmt::num(dec(&q.quote.amount_out_formatted), 8), q.destination.symbol, Fmt::usd(dec(&q.quote.amount_out_usd))),
                         format!("≈ {} s", q.quote.seconds.max(1)))),
            Err(error) if Self::intents_may_take(&error) => {
                let Some(amount) = amount else { return Err(error) };
                self.intent_quote_text(a, from, b, to, amount, near, false).await.or(Err(error))
            }
            Err(error) => Err(error),
        }
    }

    /// The swap: Relay's quote checked, its one transaction signed on the origin network, then Relay's word that it
    /// landed. NEAR's side goes through NEAR Intents, and so does a pair Relay has no route for when 1Click lists both
    /// tokens. A stop by the guard or the signer on Relay's quote reaches the user as it is, never taken to NEAR Intents.
    pub async fn cross_swap(&self, a: &str, from: &str, b: &str, to: &str, amount: Option<Decimal>, near: Option<&str>) -> Result<String> {
        if from == "near" || to == "near" { return self.intent_swap(a, from, b, to, amount, near, false).await }
        if let Some(why) = modules().refusal(ModuleProtocol::Relay, &[], false) { return fail(why) }
        let quoted = match self.cross_quote(a, from, b, to, amount, true).await {
            Ok(q) => q,
            Err(error) if Self::intents_may_take(&error) => {
                let Some(amount) = amount else { return Err(error) };
                if self.intent_quote(a, from, b, to, amount, near, true, false).await.is_err() { return Err(error) }
                self.report(&format!("Relay: {error} NEAR Intents has a route: taking it."));
                return self.intent_swap(a, from, b, to, Some(amount), near, false).await;
            }
            Err(error) => return Err(error),
        };
        let CrossQuote { quote, origin, destination, units } = quoted;
        let sent = format!("{} {}", Fmt::num(tokens(units, origin.decimals), 8), origin.symbol);
        let gets = format!("{} {}", Fmt::num(dec(&quote.amount_out_formatted), 8), destination.symbol);
        let (from_name, to_name) = (place_name(from), place_name(to));
        let what = format!("{from_name} {}→{to_name} {} {}", origin.symbol, destination.symbol, Fmt::num(tokens(units, origin.decimals), 8));
        self.report(&format!("Relay: {sent} on {from_name} (≈ {}) → ≈ {gets} on {to_name} (≈ {}), ≈ {} s.", Fmt::usd(dec(&quote.amount_in_usd)),
                             Fmt::usd(dec(&quote.amount_out_usd)), quote.seconds.max(1)));
        let started = Instant::now();
        let link = if from == "solana" {
            format!("https://solscan.io/tx/{}", self.cross_from_solana(&quote, &origin, destination.chain, units, &what).await?)
        } else {
            let Some(net) = EvmNetwork::named(from) else { return fail(format!("ihatecrypto doesn't move money on {from}.")) };
            let hash = self.cross_from_evm(&quote, &origin, net, units, &sent).await?;
            self.record("relay", &quote.request_id, Kind::Signed, &format!("{what}, tx {hash}"))?;
            let link = format!("{}/tx/{hash}", net.explorer);
            while started.elapsed() < Duration::from_secs(240) {
                let status = relay::status(&quote.request_id).await?.0;
                if status == "success" {
                    self.record("relay", &quote.request_id, Kind::Settled, &status)?;
                    break;
                }
                if ["refund", "failure"].contains(&status.as_str()) {
                    self.record("relay", &quote.request_id, Kind::Failed, &status)?;
                    return fail(format!("Relay returned {status}: the {} comes back on {from_name}. {link}", origin.symbol));
                }
                sleep(2).await;
            }
            link
        };
        let landed = relay::status(&quote.request_id).await?.0 == "success";
        Ok(if landed {
            format!("Swapped {sent} on {from_name} for ≈ {gets} on {to_name} through Relay, in {} s. {link}", started.elapsed().as_secs())
        } else {
            format!("{sent} left {from_name}, and ≈ {gets} is on its way to {to_name} (Relay order {}, in transit on the portfolio). {link}", quote.request_id)
        })
    }

    /// Solana's side: Relay's instructions compiled, checked by the guard (Relay's Depository only), simulated, then sent
    /// as a Relay bridge is (send_bridge waits for the order to land). Returns the transaction's signature.
    async fn cross_from_solana(&self, quote: &relay::Quote, origin: &CrossSide, chain: u64, units: Decimal, what: &str) -> Result<String> {
        let sol = &self.wallet.solana;
        let (blockhash, last_valid) = self.rpc.latest_blockhash().await?;
        let unsigned = solana::unsigned_transaction(&solana::compile_message(&quote.solana_instructions, sol, &blockhash, 20_000)?);
        let message = Message::parse_transaction(&unsigned)?;
        let account = if origin.coin { None } else {
            Some(spl::associated_account(sol, &origin.currency, if origin.token2022 { spl::TOKEN_2022_PROGRAM } else { spl::TOKEN_PROGRAM })?)
        };
        let owners: BTreeMap<String, String> = account.iter().map(|a| (a.clone(), sol.clone())).collect();
        let mut verdict = guard::sol::check(&message, &message.static_keys, &Policy::new(sol), &owners.clone().into_iter().collect());
        if !verdict.ok() {
            return stop(format!("Relay routes {} through programs ihatecrypto doesn't sign for ({}). Sell it for USDC or SOL on Solana first, then swap that. \
                                 Nothing was signed.", origin.symbol, verdict.problems.join(" ")));
        }
        let lamports = self.rpc.balance(sol).await?;
        let held = match &account { Some(_) => Some(self.rpc.token_balance(sol, &origin.currency).await?), None => None };
        let watch: Vec<String> = std::iter::once(sol.clone()).chain(account.clone()).collect();
        let simulation = self.rpc.simulate(&unsigned, &watch, false).await?;
        if let Some(error) = simulation.error { return fail(format!("The simulation failed, so nothing was signed: {error}")) }
        let i64_of = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        let spend = units.to_i64().unwrap_or(i64::MAX);
        let mut checks = vec![BalanceRule {
            label: "lamports".into(), before: i64_of(lamports), after: i64_of(simulation.after.get(sol).map_or(0, |s| s.0)),
            at_least: (if origin.coin { spend } else { 0 }).saturating_neg().saturating_sub(5_000_000), at_most: None,
        }];
        if let (Some(account), Some(held)) = (&account, held) {
            checks.push(BalanceRule {
                label: origin.symbol.clone(), before: i64_of(held), after: i64_of(simulation.after.get(account).and_then(|s| s.1).unwrap_or(0)),
                at_least: spend.saturating_neg(), at_most: None,
            });
        }
        verdict.merge(guard::sol::balances(&checks));
        if !verdict.ok() { return stop(format!("The guard stopped the swap: {}", verdict.problems.join(" "))) }
        let step = BridgeStep {
            request_id: quote.request_id.clone(), unsigned, accounts: message.static_keys, token_owners: owners, last_valid, spend: Decimal::ZERO,
            arriving: Decimal::ZERO, fees_usd: quote.fees_usd.clone(), chain, opening: Decimal::ZERO, note: Some(what.into()),
        };
        self.send_bridge(&step).await
    }

    /// An EVM network's side: the coin straight to Relay's depository for exactly the amount, or an exact approve to its
    /// approval proxy and the proxy's call. Returns the transaction's hash.
    async fn cross_from_evm(&self, quote: &relay::Quote, origin: &CrossSide, net: &EvmNetwork, units: Decimal, sent: &str) -> Result<String> {
        let note = format!("Relay: {sent} on {} → {}", net.name, if quote.recipient == self.wallet.solana { "Solana" } else { "another network" });
        if origin.coin {
            let depository = relay::depository(net.relay_chain).await?.map(|d| d.to_lowercase());
            let value = units.to_u64().filter(|v| Decimal::from(*v) == units);
            let one = quote.evm.first().filter(|_| quote.evm.len() == 1);
            let (Some(depository), Some(call), Some(value)) = (depository, one, value) else {
                return fail(format!("Relay's transaction isn't one deposit of exactly {sent} to the depository it publishes for {}. Nothing was signed.", net.name));
            };
            if !(call.to.to_lowercase() == depository && call.chain_id == net.relay_chain && dec(&call.value) == units) {
                return fail(format!("Relay's transaction isn't one deposit of exactly {sent} to the depository it publishes for {}. Nothing was signed.", net.name));
            }
            // Coins to Relay's depository pay Relay: the signer asks to allow that address in person the first time.
            return self.send_evm(&net.rpc, &call.to, &call.data, &note, &[depository], None, value).await;
        }
        // A token Relay swaps goes through its approval proxy; one it holds as is (USDC) straight into its depository,
        // depositErc20(this wallet, the token, exactly the amount, the order's id). Either way the approve is exact.
        let proxy = relay::approval_proxy(net.relay_chain).await?.map(|p| p.to_lowercase());
        let depository = relay::depository(net.relay_chain).await?.map(|d| d.to_lowercase());
        let token = origin.currency.to_lowercase();
        let Some(call) = quote.evm.last().filter(|c| c.chain_id == net.relay_chain && no_value(c)) else {
            return fail(format!("Relay's transaction isn't a plain call on {}. Nothing was signed.", net.name));
        };
        let args = call.data.get(4..).unwrap_or(&[]);
        let to = Some(call.to.to_lowercase());
        let spender = if to == proxy { proxy } else if to == depository && call.data.starts_with(&[0xe8, 0x01, 0x79, 0x52]) && args.len() == 4 * 32
            && address_in(args, 0).map(|a| a.to_lowercase()) == Some(self.wallet.evm.to_lowercase()) && address_in(args, 1).map(|a| a.to_lowercase()) == Some(token.clone())
            && word(args, 2) == Some(units) { depository } else { None };
        let Some(spender) = spender.filter(|s| quote.evm[..quote.evm.len() - 1].iter().all(|step| approves(step, &token, s, units))) else {
            return fail(format!("Relay's transactions don't go to the approval proxy or the depository it publishes for {}, for exactly {sent}. Nothing was signed.",
                                net.name));
        };
        self.approve_and_send(&net.rpc, &origin.currency, &spender, u256(units)?, &format!("Approve exactly {sent} to Relay"), &call.to, &call.data, &note, &[], 0).await
    }
}

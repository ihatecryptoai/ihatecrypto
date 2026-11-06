//! USDC between Solana, Base, Arbitrum and Ethereum through NEAR Intents' 1Click, beside Relay (OneClickBridge.swift):
//! each bridge quotes both and takes the one that lands more (`bridge`, `solana_to_evm`). 1Click is a deposit to an
//! address its quote names; the live quote must stay within 1% of the one compared, and 1Click itself must have that
//! address bound to our recipient, our refund address, the asset and the amount (guard::deposit_check) before anything
//! is sent. Also coins on networks ihatecrypto has no wallet on, sent and received through 1Click.

use std::collections::HashMap;
use std::time::Instant;

use chrono::Utc;
use evm::{U256, erc20};
use execution::{Envelope, Kind};
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::WalletCommand;
use protocols::one_click::{self, Ask, Quote, asset};
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::Value;
use wallet::address_check::Network;

use crate::{Error, EvmNetwork, Executor, Result, fail, num, stop};

/// `Presence.name` and `.short` on Windows: what the signer asks for before it signs.
const PRESENCE: &str = "Windows Hello";

fn millionths(units: u64) -> Decimal { Decimal::from(units) / Decimal::from(1_000_000) }

/// `10^n`, as `Decimal(sign: .plus, exponent: n, significand: 1)`.
fn pow10(n: u32) -> Decimal { Decimal::from_i128_with_scale(10i128.pow(n.min(28)), 0) }

/// `AddressCheck.Network(rawValue:)?.title ?? network`.
fn title(network: &str) -> String {
    serde_json::from_value::<Network>(Value::String(network.into())).map_or_else(|_| network.to_string(), |n| n.title().to_string())
}

/// The id at the end of what a send returned (Swift's `split(separator: "/").last ?? ""`).
fn last_part(s: &str) -> String { s.split('/').rfind(|p| !p.is_empty()).unwrap_or("").to_string() }

/// 1Click's asset for a coin on a network ihatecrypto has no wallet on: (ticker, asset id, decimals).
fn across(symbol: &str, network: &str) -> Option<(&'static str, &'static str, u32)> {
    let want = symbol.to_uppercase();
    asset::across(network).iter().copied().find(|(s, _, _)| *s == want)
}

/// `(amount * 10^decimals).whole`; a number past range stops the move.
fn whole_units(amount: Decimal, decimals: u32) -> Result<Decimal> {
    Ok(rounded(num(Num::of(amount) * pow10(decimals))?, 0, Rounding::Down))
}

impl Executor {
    /// 1Click's asset for USDC on a place: an EVM network's key, Hyperliquid's perp balance, or None for Solana.
    pub fn one_click_usdc(place: Option<&str>) -> Option<&'static str> {
        match place {
            Some("hyperliquid") => Some(asset::HYPER_CORE_USDC),
            Some(net) => asset::usdc_on_evm(net),
            None => Some(asset::SOLANA_USDC),
        }
    }

    /// The wallet's address on a place: Solana's for None, the EVM one elsewhere.
    fn one_click_address(&self, place: Option<&str>) -> &str { if place.is_none() { &self.wallet.solana } else { &self.wallet.evm } }

    /// What 1Click lands for `units` of USDC from `from` to `to` (None = Solana), dry: nothing is issued. None when it
    /// doesn't quote the pair.
    pub async fn one_click_quote(&self, from: Option<&str>, to: Option<&str>, units: u64) -> Option<Quote> {
        let (origin, destination) = (Self::one_click_usdc(from)?, Self::one_click_usdc(to)?);
        one_click::quote(&Ask::new(true, origin, destination, &units.to_string(), self.one_click_address(from), self.one_click_address(to))).await.ok()
    }

    /// The move through 1Click: a fresh quote (within 1% of `compared`), the deposit address checked against what 1Click
    /// bound to it, the USDC sent there from `from` (Solana's own transfer, or an exact ERC-20 transfer the signer asks
    /// to allow in person), then 1Click's states on the running card until it lands. Returns what arrived.
    pub async fn one_click_move(&self, from: Option<&str>, to: Option<&str>, units: u64, compared: Decimal, label: &str) -> Result<Decimal> {
        let (Some(origin), Some(destination)) = (Self::one_click_usdc(from), Self::one_click_usdc(to)) else {
            return fail("1Click doesn't move USDC there.");
        };
        let (refund_to, recipient) = (self.one_click_address(from), self.one_click_address(to));
        let amount = units.to_string();
        let quote = one_click::quote(&Ask::new(false, origin, destination, &amount, refund_to, recipient)).await?;
        let Some(deposit) = quote.deposit_address.clone() else { return fail("1Click gave no deposit address. Nothing was sent.") };
        let mut envelope = Envelope::approve_default(millionths(units), compared, Utc::now());
        envelope.destination = Some(deposit.clone());
        let problems = envelope.violations(decimal(&quote.amount_in_formatted).unwrap_or(Decimal::MAX), decimal(&quote.amount_out_formatted).unwrap_or_default(),
                                           Some(&deposit), Utc::now());
        if !problems.is_empty() {
            return fail(format!("1Click's price moved past the one compared: {} Nothing was sent.", problems.join(" ")));
        }
        let binding = guard::deposit_check(&one_click::bound_request(&deposit).await?, recipient, refund_to, origin, destination, &amount);
        if !binding.ok() { return stop(format!("The guard stopped the deposit: {}", binding.problems.join(" "))) }
        let tx = match from.and_then(EvmNetwork::named) {
            Some(net) => {
                self.ensure_evm(net, units, None).await?;
                self.send_evm(&net.rpc, net.usdc, &erc20::transfer(&deposit, U256::from(units))?, &format!("Send {label} to the 1Click deposit"),
                              std::slice::from_ref(&deposit), None, 0).await?
            }
            None => last_part(&self.send_solana_usdc(units, &deposit, "the 1Click deposit").await?),
        };
        self.record("1click", &deposit, Kind::Signed, &format!("USDC {}→{} {label}", from.unwrap_or("solana"), to.unwrap_or("solana")))?;
        if !tx.is_empty() { let _ = one_click::submit_deposit(&tx, &deposit).await; }
        let back = from.map_or_else(|| "Solana".to_string(), WalletCommand::place_name);
        let s = self.one_click_settled(&deposit, &format!("USDC went back to {back}"), quote.time_estimate).await?;
        Ok(s.amount_out_formatted.as_deref().and_then(decimal).or_else(|| decimal(&quote.amount_out_formatted)).unwrap_or_default())
    }

    /// Whether 1Click is worth taking over Relay for this move: it must land more than Relay's `relay_out` by at least
    /// `one_click_edge` — its fresh deposit address is one more address to allow in person, and it takes ≈ 20–30 s
    /// against Relay's few (live 2026-10-05: 1Click landed 1–3 cents more on 5 to 200 USDC). Said either way on the card.
    pub async fn one_click_beats(&self, from: Option<&str>, to: Option<&str>, units: u64, relay_out: Decimal) -> Option<Quote> {
        let q = self.one_click_quote(from, to, units).await?;
        let out = decimal(&q.amount_out_formatted)?;
        let more = (Num::of(out) - relay_out).0;
        let better = more.is_some_and(|m| m >= Self::one_click_edge(millionths(units)));
        let took = q.time_estimate.map_or(String::new(), |t| format!(" in ≈ {t} s"));
        let taking = if better {
            format!("1Click — its fresh deposit address needs {PRESENCE} first.")
        } else if out > relay_out {
            format!("Relay (1Click's {} more isn't worth another {PRESENCE} and the wait).", Fmt::usd(more.unwrap_or_default()))
        } else {
            "Relay.".into()
        };
        self.report(&format!("Relay lands ≈ {} USDC, NEAR Intents' 1Click ≈ {}{took}: taking {taking}", Fmt::num(relay_out, 4), Fmt::num(out, 4)));
        better.then_some(q)
    }

    /// What sending `amount` of `symbol` on a network ihatecrypto has no wallet on costs in USDC, and where that USDC
    /// comes from: Solana, or an EVM network 1Click takes USDC from when Solana is short (near_source). Dry: nothing is
    /// issued. Returns the quote, 1Click's asset, the amount in its units and the EVM network that pays (None: Solana).
    pub async fn across_quote(&self, symbol: &str, amount: Decimal, network: &str, recipient: &str)
        -> Result<(Quote, &'static str, Decimal, Option<&'static EvmNetwork>)> {
        let Some((ticker, id, decimals)) = across(symbol, network) else {
            return fail(format!("NEAR Intents doesn't pay out {} on {}.", symbol.to_uppercase(), title(network)));
        };
        let units = whole_units(amount, decimals)?;
        if units <= Decimal::ZERO { return fail(format!("{} {ticker} is less than its smallest unit.", Fmt::num(amount, 2))) }
        let ask = |origin: &str, refund_to: &str| Ask { exact_output: true, ..Ask::new(true, origin, id, &text(units), refund_to, recipient) };
        let solana_quote = one_click::quote(&ask(asset::SOLANA_USDC, &self.wallet.solana)).await?;
        let cost = decimal(&solana_quote.amount_in_formatted).unwrap_or_default();
        let on_solana = millionths(self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?);
        let mut idle: HashMap<&str, Decimal> = HashMap::new();
        for (net, usdc) in self.idle_usdc().await {
            let sum = idle.get(net.key).map_or(Some(usdc), |had| had.checked_add(usdc));
            idle.insert(net.key, sum.unwrap_or(Decimal::MAX));
        }
        let source = Self::near_source(cost, on_solana, |k| idle.get(k).copied());
        let Some((src, origin)) = source.and_then(|s| Some((s, asset::usdc_on_evm(s.key)?))) else {
            if on_solana < cost {
                return fail(format!("{} {ticker} costs ≈ {} USDC, but Solana has {} and no network 1Click takes USDC from has that much. Nothing was signed.",
                                    Fmt::num(amount, 2), Fmt::num(cost, 2), Fmt::num(on_solana, 2)));
            }
            return Ok((solana_quote, id, units, None));
        };
        let q = one_click::quote(&ask(origin, &self.wallet.evm)).await?;
        Ok((q, id, units, Some(src)))
    }

    /// `amount` of a coin on a network ihatecrypto has no wallet on — BTC, ZEC, TRX or USDT on TRON, XRP, DOGE, SUI — to
    /// a saved address there, paid in USDC through 1Click: exact output, so the address gets `amount` and 1Click returns
    /// what's left of the USDC sent. The live quote must stay within 1% of the one compared, and 1Click itself must have
    /// the deposit address bound to this recipient, our refund address, the assets and the amount before anything is
    /// sent. The USDC goes to 1Click's fresh deposit address, which the signer asks to allow in person.
    pub async fn send_across(&self, symbol: &str, amount: Decimal, network: &str, recipient: &str, label: &str) -> Result<String> {
        let (shown, id, units, src) = self.across_quote(symbol, amount, network, recipient).await?;
        let origin = src.and_then(|s| asset::usdc_on_evm(s.key)).unwrap_or(asset::SOLANA_USDC);
        let refund_to = if src.is_none() { &self.wallet.solana } else { &self.wallet.evm };
        let wanted = text(units);
        let quote = one_click::quote(&Ask { exact_output: true, ..Ask::new(false, origin, id, &wanted, refund_to, recipient) }).await?;
        let (Some(deposit), Some(send_units)) = (quote.deposit_address.clone(), quote.amount_in.parse::<u64>().ok().filter(|u| *u > 0)) else {
            return fail("1Click gave no deposit address. Nothing was sent.");
        };
        let (cost, shown_cost) = (millionths(send_units), decimal(&shown.amount_in_formatted).unwrap_or_default());
        if (Num::of(shown_cost) * Decimal::new(101, 2) + Decimal::new(1, 2)).0.is_some_and(|most| cost > most) {
            return fail(format!("1Click now wants {} USDC, more than the {} quoted. Nothing was sent.", Fmt::num(cost, 2), Fmt::num(shown_cost, 2)));
        }
        let binding = guard::deposit_check(&one_click::bound_request(&deposit).await?, recipient, refund_to, origin, id, &wanted);
        if !binding.ok() { return stop(format!("The guard stopped the deposit: {}", binding.problems.join(" "))) }
        let from = src.map_or("Solana", |s| s.name);
        let what = format!("{} {}", Fmt::num(amount, 9), symbol.to_uppercase());
        self.report(&format!("NEAR Intents: {} USDC from {from} → {what} to «{label}». What isn't used comes back.", Fmt::num(cost, 2)));
        let tx = match src {
            Some(src) => {
                self.ensure_evm(src, send_units, None).await?;
                self.send_evm(&src.rpc, src.usdc, &erc20::transfer(&deposit, U256::from(send_units))?,
                              &format!("Send {} USDC to the 1Click deposit for {what}", Fmt::num(cost, 2)), std::slice::from_ref(&deposit), None, 0).await?
            }
            None => last_part(&self.send_solana_usdc(send_units, &deposit, "the 1Click deposit").await?),
        };
        self.record("1click", &deposit, Kind::Signed, &format!("{from} USDC→{what} to {label}"))?;
        if !tx.is_empty() { let _ = one_click::submit_deposit(&tx, &deposit).await; }
        let started = Instant::now();
        let s = self.one_click_settled(&deposit, &format!("USDC went back to {from}"), quote.time_estimate).await?;
        Ok(format!("Sent {} {} to «{label}» for {} USDC from {from}, through NEAR Intents, in {} s.",
                   Fmt::num(s.amount_out_formatted.as_deref().and_then(decimal).unwrap_or(amount), 9), symbol.to_uppercase(), Fmt::num(cost, 2),
                   started.elapsed().as_secs()))
    }

    /// A deposit address on a network ihatecrypto has no wallet on — Sui, Bitcoin, TRON… — for about `amount` of `symbol`
    /// sent there from an exchange or another wallet: 1Click swaps what arrives into USDC on Solana, to this wallet.
    /// Nothing is signed. The address is 1Click's, checked to be bound to our Solana address, the assets and the amount;
    /// any deposit from the quote's minimum is taken (FLEX_INPUT) for an hour. A refund goes to the wallet's NEAR
    /// Intents account (`near_account`, its own NEAR key) — ihatecrypto has no address of the user's there. Returns the
    /// deposit address, the quote and the least it takes.
    // ponytail: getting a refund out of NEAR Intents isn't built; it waits in that account, under the wallet's NEAR key.
    pub async fn receive_across(&self, symbol: &str, amount: Decimal, network: &str, near_account: &str) -> Result<(String, Quote, Decimal)> {
        let Some((ticker, id, decimals)) = across(symbol, network) else {
            return fail(format!("NEAR Intents doesn't take {} on {}.", symbol.to_uppercase(), title(network)));
        };
        if !(near_account.len() == 64 && near_account.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))) {
            return fail("A refund needs the wallet's NEAR account, and it has none.");
        }
        let units = whole_units(amount, decimals)?;
        if units <= Decimal::ZERO { return fail(format!("{} {ticker} is less than its smallest unit.", Fmt::num(amount, 2))) }
        let wanted = text(units);
        let ask = Ask { flexible: true, refund_to_intents: true, minutes: 60.0, ..Ask::new(false, id, asset::SOLANA_USDC, &wanted, near_account, &self.wallet.solana) };
        let quote = one_click::quote(&ask).await?;
        let Some(deposit) = quote.deposit_address.clone() else { return fail("1Click gave no deposit address.") };
        let binding = guard::deposit_check(&one_click::bound_request(&deposit).await?, &self.wallet.solana, near_account, id, asset::SOLANA_USDC, &wanted);
        if !binding.ok() { return stop(format!("The guard refused 1Click's address: {}", binding.problems.join(" "))) }
        self.record("1click", &deposit, Kind::Signed, &format!("Receive: {} {ticker}→Solana USDC {}", title(network), Fmt::num(amount, 2)))?;
        let least = quote.min_amount_in.as_deref().and_then(decimal).unwrap_or(units);
        let minimum = (Num::of(least) / pow10(decimals)).0.ok_or_else(|| Error::Execution(crate::OUT_OF_RANGE.into()))?;
        Ok((deposit, quote, minimum))
    }

    /// What 1Click must land beyond Relay to be taken: 10 cents, or 0.05% of a bigger move.
    pub fn one_click_edge(usdc: Decimal) -> Decimal { Decimal::new(10, 2).max((Num::of(usdc) * Decimal::new(5, 4)).0.unwrap_or(Decimal::MAX)) }
}

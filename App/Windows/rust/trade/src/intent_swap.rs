//! Any token for any other through NEAR Intents' 1Click (IntentSwap.swift) — the ≈ 200 tokens its /v0/tokens lists: on
//! NEAR itself (NEAR for AURORA), between NEAR and any network ihatecrypto has a wallet on, and where Relay has no route.
//! Always to this wallet's own address on the other side, the refund to its own on this one. What goes in is a plain
//! transfer to the deposit address 1Click binds to this quote — read back from 1Click before signing (deposit_check) —
//! native NEAR through the signer's wrap.near deposit; the live quote stays within 1% of the one shown (Envelope).

use std::time::Instant;

use chrono::Utc;
use execution::{Envelope, Kind};
use ihatecrypto_core::foundation::{Num, decimal, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::Near;
use ihatecrypto_core::position_command::WalletCommand;
use protocols::one_click;
use rust_decimal::Decimal;
use wallet::near;

use crate::aave::EvmNetwork;
use crate::swap::{dec, plain, scale, whole};
use crate::{Executor, Result, fail, num, stop};

/// 1Click's name for each place ihatecrypto has a wallet on (its `blockchain`).
pub const INTENT_CHAINS: [(&str, &str); 8] =
    [("solana", "sol"), ("base", "base"), ("arbitrum", "arb"), ("optimism", "op"), ("ethereum", "eth"), ("polygon", "pol"), ("bnb", "bsc"), ("near", "near")];

/// A token as 1Click lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentSide {
    pub asset: String,
    pub symbol: String,
    pub decimals: u32,
}

/// 1Click's quote for a swap, with both sides and the addresses it pays and refunds (intent_quote).
#[derive(Debug, Clone, PartialEq)]
pub struct IntentQuote {
    pub quote: one_click::Quote,
    pub origin: IntentSide,
    pub destination: IntentSide,
    pub units: String,
    pub recipient: String,
    pub refund: String,
}

fn place_name(place: &str) -> String { WalletCommand::place_name(place) }

impl Executor {
    /// The token as 1Click lists it on that network, by ticker or contract; NEAR on NEAR is its wNEAR.
    pub async fn intent_side(&self, token: &str, place: &str) -> Result<IntentSide> {
        let Some(chain) = INTENT_CHAINS.iter().find(|(p, _)| *p == place).map(|(_, c)| *c) else {
            return fail(format!("NEAR Intents swaps for this wallet on Solana, NEAR and the EVM networks, not on {}.", place_name(place)));
        };
        let want = if token.to_uppercase() == "NEAR" && place == "near" { "WNEAR".to_string() } else { plain(token) };
        let contract = token.to_lowercase();
        let found = one_click::tokens().await?.into_iter()
            .find(|t| t.blockchain == chain && (t.symbol.to_uppercase() == want || t.contract.as_deref() == Some(contract.as_str())));
        let Some(t) = found else { return fail(format!("NEAR Intents has no {token} on {}. Nothing was signed.", place_name(place))) };
        Ok(IntentSide { asset: t.asset_id, symbol: if t.symbol == "wNEAR" { "NEAR".into() } else { t.symbol }, decimals: u32::try_from(t.decimals).unwrap_or(0) })
    }

    /// This wallet's own address on `place`: Solana's, the EVM one, or its NEAR account (`near`, from the signer).
    fn intent_address(&self, place: &str, near: Option<&str>) -> Result<String> {
        if place == "near" {
            let Some(near) = near.filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) else {
                return fail("The signer has no NEAR account for this wallet.");
            };
            return Ok(near.into());
        }
        Ok(if place == "solana" { self.wallet.solana.clone() } else { self.wallet.evm.clone() })
    }

    /// 1Click's quote for the swap: `dry` for the card, else with a deposit address bound to it.
    #[allow(clippy::too_many_arguments)]
    pub async fn intent_quote(&self, a: &str, from: &str, b: &str, to: &str, amount: Decimal, near: Option<&str>, dry: bool) -> Result<IntentQuote> {
        let (origin, destination) = tokio::join!(self.intent_side(a, from), self.intent_side(b, to));
        let (origin, destination) = (origin?, destination?);
        if origin.asset == destination.asset { return fail("That's the same token on both sides. Nothing to swap.") }
        let whole_units = whole(num(Num::of(amount) * scale(origin.decimals))?);
        let units = text(whole_units);
        if units == "0" { return fail(format!("{} {} is less than its smallest unit.", Fmt::num(amount, 2), origin.symbol)) }
        // An EVM coin's deposit is one transfer, under 2⁶⁴ wei: said before 1Click opens an order.
        if let Some(net) = EvmNetwork::named(from) {
            if Self::is_coin(a, net) && whole_units >= Decimal::from(u64::MAX) { return fail(format!("At most 18 {} at once. Nothing was signed.", net.gas_coin)) }
        }
        // A dry quote to NEAR before the signer has named the account: any implicit account prices it the same.
        let zero = "0".repeat(64);
        let near = if dry { Some(near.unwrap_or(&zero)) } else { near };
        let recipient = self.intent_address(to, near)?;
        let refund = self.intent_address(from, near)?;
        let quote = one_click::quote(&one_click::Ask::new(dry, &origin.asset, &destination.asset, &units, &refund, &recipient)).await?;
        Ok(IntentQuote { quote, origin, destination, units, recipient, refund })
    }

    /// What the swap gets, for the card: «≈ 12.4 AURORA ≈ $1.98», and the way.
    pub async fn intent_quote_text(&self, a: &str, from: &str, b: &str, to: &str, amount: Decimal, near: Option<&str>) -> Result<(String, String)> {
        let q = self.intent_quote(a, from, b, to, amount, near, true).await?;
        let usd = q.quote.amount_out_usd.map_or_else(String::new, |u| format!(" ≈ {}", Fmt::usd(u)));
        let time = q.quote.time_estimate.map_or_else(String::new, |t| format!(" · ≈ {t} s"));
        Ok((format!("{} {}{usd}", Fmt::num(dec(&q.quote.amount_out_formatted), 6), q.destination.symbol), format!("NEAR Intents{time}")))
    }

    /// The swap itself: quoted again with a deposit address, checked, the deposit sent, 1Click followed until it pays.
    pub async fn intent_swap(&self, a: &str, from: &str, b: &str, to: &str, amount: Option<Decimal>, near: Option<&str>) -> Result<String> {
        let Some(amount) = amount else { return fail(format!("Say how much {}: NEAR Intents swaps the amount said. Nothing was signed.", a.to_uppercase())) };
        let shown = self.intent_quote(a, from, b, to, amount, near, true).await?;
        let mut envelope = Envelope::approve_default(amount, dec(&shown.quote.amount_out_formatted), Utc::now());
        let live = self.intent_quote(a, from, b, to, amount, near, false).await?;
        let Some(deposit) = live.quote.deposit_address.clone() else { return fail("1Click gave no deposit address. Nothing was signed.") };
        envelope.destination = Some(deposit.clone());
        let problems = envelope.violations(decimal(&live.quote.amount_in_formatted).unwrap_or(Decimal::MAX), dec(&live.quote.amount_out_formatted), Some(&deposit),
                                           Utc::now());
        if !problems.is_empty() { return fail(format!("The price moved past what was approved: {}", problems.join(" "))) }
        // No price for either side stops here too: Relay's refusal of an unpriced swap brings it to this way.
        Self::holds_cross_loss(live.quote.amount_in_usd.unwrap_or_default(), live.quote.amount_out_usd.unwrap_or_default(),
                               &format!("1Click doesn't price {} for {}", live.origin.symbol, live.destination.symbol), "NEAR Intents", true)?;
        let binding = guard::deposit_check(&one_click::bound_request(&deposit).await?, &live.recipient, &live.refund, &live.origin.asset, &live.destination.asset,
                                           &live.units);
        if !binding.ok() { return stop(format!("The guard stopped the deposit: {}", binding.problems.join(" "))) }
        let gets = dec(&live.quote.amount_out_formatted);
        self.report(&format!("NEAR Intents: {} {} on {} → ≈ {} {} on {}.", Fmt::num(amount, 8), live.origin.symbol, place_name(from), Fmt::num(gets, 6),
                             live.destination.symbol, place_name(to)));
        // The deposit: a transfer to 1Click's address, which the signer asks to allow in person the first time.
        let tx = if from == "near" {
            let (true, Some(account)) = (live.origin.symbol == "NEAR", near) else {
                return fail("From NEAR, ihatecrypto sends NEAR itself; its other tokens there can't be signed yet. Nothing was signed.");
            };
            let context = Near::tx_context(account).await?;
            let yocto: u128 = live.units.parse().unwrap_or(0);
            let keep = u128::from(context.storage_bytes).saturating_mul(10_000_000_000_000_000_000)
                .saturating_add(near::intent_deposit_overhead(context.gas_price.parse().unwrap_or(100_000_000)));
            let balance: u128 = context.balance.parse().unwrap_or(0);
            if yocto.saturating_add(keep) > balance {
                return fail(format!("{} NEAR can go: the account keeps {} for its storage and the transaction.", near::near(balance.saturating_sub(keep)), near::near(keep)));
            }
            let signed = self.signer.sign_near_intent_deposit(yocto, deposit.clone(), context.nonce, context.block_hash.clone()).await?;
            self.record("1click", &deposit, Kind::Signed, &format!("{}→{} {} near → {to}", live.origin.symbol, live.destination.symbol, text(amount)))?;
            Near::send(&signed).await?
        } else {
            let sent = self.send_token(a, Some(amount), from, &deposit, "the 1Click deposit").await?;
            self.record("1click", &deposit, Kind::Signed, &format!("{}→{} {} {from} → {to}", live.origin.symbol, live.destination.symbol, text(amount)))?;
            sent.split('/').filter(|s| !s.is_empty()).last().unwrap_or("").trim().to_string()
        };
        if !tx.is_empty() { let _ = one_click::submit_deposit(&tx, &deposit).await; }
        let started = Instant::now();
        let s = self.one_click_settled(&deposit, &format!("{} went back to {}", live.origin.symbol, place_name(from)), live.quote.time_estimate).await?;
        let line = format!("Swapped {} {} on {} for {} {} on {} through NEAR Intents in {} s — at your own address there.", Fmt::num(amount, 8), live.origin.symbol,
                           place_name(from), Fmt::num(s.amount_out_formatted.as_deref().and_then(decimal).unwrap_or(gets), 6), live.destination.symbol,
                           place_name(to), started.elapsed().as_secs());
        self.report(&line);
        Ok(line)
    }
}

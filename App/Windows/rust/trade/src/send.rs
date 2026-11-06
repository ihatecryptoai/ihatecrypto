//! Sends (Send.swift): USDC to an address from the user's address book — on Solana directly, on an EVM network from its
//! own USDC (bridged from Solana when short), on Hyperliquid from the perp balance; native NEAR and SOL; any token the
//! wallet holds, as it is. The recipient is passed to the guard and the signer as the one approved address; nothing else
//! may receive money, and the simulation must show exactly the amount leaving.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use evm::{U256, abi, erc20, tempo};
use execution::Kind;
use execution::trade_signer::EvmFees;
use guard::sol::{BalanceRule, Policy};
use ihatecrypto_core::foundation::{Num, Rounding, rounded, shifted, text, units};
use ihatecrypto_core::holdings::{self, Pick};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::{Amount, Hyperliquid, Near, Solana};
use ihatecrypto_core::stablecoins;
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use solana::message::Message;
use solana::{Instruction, spl};
use wallet::evm::keccak;
use wallet::keys::base58;
use wallet::to_hex;

use crate::aave::EvmNetwork;
use crate::executor::{HlReply, hl_exchange, uint64};
use crate::swap::EvmTokens;
use crate::wallet::{d, i, millionths, prefix, shift, sleep, times_pow10, units_of, usd};
use crate::{Error, Executor, OUT_OF_RANGE, Result, fail, landings, num, stop};

/// Where `send_usdc` sends: an address on Solana, an EVM network or Hyperliquid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Solana(String),
    Base(String),
    Arbitrum(String),
    /// USDG on Robinhood Chain.
    Robinhood(String),
    /// Another Hyperliquid account: USDC from the perp balance; what it lacks is bridged in first.
    Hyperliquid(String),
    Tempo(String),
    Ethereum(String),
    Polygon(String),
    Hyperevm(String),
    Optimism(String),
    Bnb(String),
}

/// `/[1-9A-HJ-NP-Za-km-z]{32,44}/`: a Solana mint written out.
fn base58_address(s: &str) -> bool { (32..=44).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() && !b"0OIl".contains(&b)) }
/// `/0x[0-9a-fA-F]{40}/`
fn evm_address(s: &str) -> bool { s.len() == 42 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit()) }
/// Wei (or any 18-decimal units) as a coin amount, past 64 bits too.
fn coins(wei: u128) -> String { Fmt::num(units(&wei.to_string(), 18).unwrap_or_default(), 18) }

impl Executor {
    pub async fn send_usdc(&self, amount: Decimal, destination: &Destination, label: &str) -> Result<String> {
        let units = units_of(amount, 6);
        if units == 0 { return fail("Nothing to send.") }
        match destination {
            Destination::Solana(owner) => self.send_solana_usdc(units, owner, label).await,
            Destination::Base(r) => self.send_evm_usdc(units, EvmNetwork::base(), r, label).await,
            Destination::Arbitrum(r) => self.send_evm_usdc(units, EvmNetwork::arbitrum(), r, label).await,
            Destination::Robinhood(r) => self.send_evm_usdc(units, EvmNetwork::robinhood(), r, label).await,
            Destination::Hyperliquid(r) => self.send_hyperliquid_usdc(amount, r, label).await,
            Destination::Tempo(r) => self.send_evm_usdc(units, EvmNetwork::tempo(), r, label).await,
            Destination::Ethereum(r) => self.send_evm_usdc(units, EvmNetwork::ethereum(), r, label).await,
            Destination::Polygon(r) => self.send_evm_usdc(units, EvmNetwork::polygon(), r, label).await,
            Destination::Hyperevm(r) => self.send_evm_usdc(units, EvmNetwork::hyperevm(), r, label).await,
            Destination::Optimism(r) => self.send_evm_usdc(units, EvmNetwork::optimism(), r, label).await,
            Destination::Bnb(r) => self.send_evm_usdc(units, EvmNetwork::bnb(), r, label).await,
        }
    }

    /// A payment for a service on Tempo (MPP «charge», push mode): USDC to the payee named in the service's challenge,
    /// with the memo it asked for; what's missing comes from Solana first. The hash is the proof the service checks.
    pub async fn pay_tempo(&self, units: u64, payee: &str, memo: Option<&[u8]>, label: &str) -> Result<String> {
        if units == 0 { return fail("Nothing to pay.") }
        let net = EvmNetwork::tempo();
        // The amount is the service's 402: the fee rides on it below (here and in ensure_evm), and must fit in 64 bits.
        if units > u64::MAX - net.fee_reserve() { return fail("The service asks more USDC than exists. Nothing was paid.") }
        // Short there: a dollar comes over (or the price, if more), so the next requests don't each pay a bridge.
        let there = net.usdc_balance(&self.wallet.evm).await?;
        self.ensure_evm(net, if there >= units + net.fee_reserve() { units } else { units.max(Self::TEMPO_FLOAT) }, None).await?;
        let data = match memo { Some(m) => erc20::transfer_with_memo(payee, units, m)?, None => erc20::transfer(payee, U256::from(units))? };
        self.send_evm(&net.rpc, net.usdc, &data, &format!("Pay {} USDC to {label}", Self::usdc(units)), &[payee.to_string()], None, 0).await
    }

    /// What a payment brings to Tempo when it's short there: a dollar, the user's ask (2026-09-30: «предлагай сразу
    /// перевести 1 бакс… чтобы по многу раз не переводить»).
    pub const TEMPO_FLOAT: u64 = 1_000_000;

    async fn send_evm_usdc(&self, units: u64, net: &EvmNetwork, recipient: &str, label: &str) -> Result<String> {
        self.ensure_evm(net, units, None).await?;
        let hash = self.send_evm(&net.rpc, net.usdc, &erc20::transfer(recipient, net.raw(units))?, &format!("Send {} USDC to {label}", Self::usdc(units)),
                                 &[recipient.to_string()], None, 0).await?;
        Ok(format!("Sent {} USDC to {label} on {}. {}/tx/{hash}", Self::usdc(units), net.name, net.explorer))
    }

    /// usdSend from the perp balance, signed by the main key (the trading key can't move money out); the guard and the
    /// signer accept only this destination.
    async fn send_hyperliquid_usdc(&self, amount: Decimal, destination: &str, label: &str) -> Result<String> {
        let evm = self.wallet.evm.as_str();
        // To the micro-dollar, not the cent: «send all» leaves nothing behind.
        let usdc = rounded(amount, 6, Rounding::Down);
        let to = destination.to_lowercase();
        if to == evm.to_lowercase() { return fail("That is this wallet's own Hyperliquid account.") }
        // The first transfer to an account Hyperliquid hasn't seen yet loses 1 USDC on the way: 1 more goes, so the amount
        // asked arrives. With no answer nothing goes. Asked again a second later first (a blip).
        let is_new = match Hyperliquid::is_new_account(&to).await {
            Ok(n) => n,
            Err(_) => {
                sleep(1).await;
                match Hyperliquid::is_new_account(&to).await {
                    Ok(n) => n,
                    Err(e) => return fail(format!("Hyperliquid didn't say whether {label}'s account is new ({}), so nothing was sent: \
                                                   a first transfer to a new account loses 1 USDC on the way. Try again in a minute.", e.0)),
                }
            }
        };
        let opening = if is_new { Decimal::ONE } else { Decimal::ZERO };
        let total = num(Num::of(usdc) + opening)?;
        self.ensure_hyperliquid(total).await?;
        let free = Hyperliquid::balance(evm).await?.withdrawable;
        if !(usdc > Decimal::ZERO && total <= free) {
            return fail(format!("{} free on Hyperliquid — can't send {}{}", usd(free), Fmt::num(usdc, 2),
                                if opening > Decimal::ZERO { " plus the 1 USDC Hyperliquid keeps once for a new account." } else { "." }));
        }
        let time = Utc::now().timestamp_millis().max(0) as u64;
        let action = hyperliquid::signer::usd_send_action(&to, &text(total), time, true);
        let signature = self.signer.sign_hyperliquid_user(action.clone(), vec![to.clone()]).await?;
        self.record("hyperliquid", &time.to_string(), Kind::Signed, &format!("send {} USDC to {label}", Fmt::num(usdc, 2)))?;
        let reply = HlReply::parse(&hl_exchange(&action, time, &signature).await?);
        if let HlReply::Error(e) = &reply {
            self.record("hyperliquid", &time.to_string(), Kind::Failed, e)?;
            return fail(format!("Hyperliquid refused the transfer: {e}."));
        }
        self.record("hyperliquid", &time.to_string(), Kind::Confirmed, &reply.to_string())?;
        Ok(format!("Sent {} USDC to {label} on Hyperliquid{}", Fmt::num(usdc, 2),
                   if opening > Decimal::ZERO { ", plus 1 USDC Hyperliquid kept once for opening their account." } else { "." }))
    }

    /// «move 11 USDC from Solana to Hyperliquid»: that much USDC from Solana over Relay to the wallet's own Hyperliquid
    /// account; the first deposit opens it (Relay's price then takes the ≈ $1 opening fee). Waits until it lands.
    pub async fn solana_to_hyperliquid(&self, usdc: Decimal) -> Result<String> {
        if usdc < Self::MIN_BRIDGE_USDC { return fail(format!("At least {} USDC: less isn't worth the bridge.", Fmt::num(Self::MIN_BRIDGE_USDC, 2))) }
        let (step, changes) = self.prepare_bridge(Amount::Send(usdc), relay::chain::HYPERLIQUID, relay::currency::HYPERLIQUID_USDC, 8, None, false).await?;
        self.report(&format!("{} USDC from Solana → ≈ {} USDC on Hyperliquid. {}", Fmt::num(step.spend, 2), Fmt::num(step.arriving, 2), changes.join(" ")));
        self.send_bridge(&step).await?;
        let now = Hyperliquid::balance(&self.wallet.evm).await.ok().and_then(|b| (Num::of(b.withdrawable) + b.spot_usdc).0);
        Ok(format!("Moved ≈ {} USDC to Hyperliquid{} Its smallest order is $10. Say “withdraw USDC to Solana” to bring it back.", Fmt::num(step.arriving, 2),
                   now.map_or_else(|| ".".to_string(), |n| format!(": {} USDC free there now.", Fmt::num(n, 2)))))
    }

    /// What Hyperliquid lacks for a send comes over first, as ensure_evm does for an EVM network: from Solana over Relay,
    /// else straight from the EVM network holding it.
    pub(crate) async fn ensure_hyperliquid(&self, want: Decimal) -> Result<()> {
        let free = Hyperliquid::balance(&self.wallet.evm).await?.withdrawable;
        if want <= free { return Ok(()) }
        let short = num(Num::of(want) - free)?;
        let on_solana = millionths(self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?);
        if on_solana < num(Num::of(short) + d("0.1"))? {
            let idle: HashMap<&str, Decimal> = self.idle_usdc().await.into_iter().map(|(n, u)| (n.key, u)).collect();
            if let Some((from, send)) = EvmNetwork::source(short, on_solana, "hyperliquid", |k| idle.get(k).copied()) {
                self.report(&format!("Hyperliquid has {} and Solana {} USDC: bringing {} straight from {}.", usd(free), Fmt::num(on_solana, 2), Fmt::num(send, 2),
                                     from.name));
                let line = self.bridge(from, None, true, Some(send)).await?;
                self.report(&line);
                // Inside a chain the bridge returns before it lands; the send needs the money there.
                if let Some(landings) = landings() {
                    for line in landings.wait(Some("hyperliquid")).await? { self.report(&line) }
                }
                return Ok(());
            }
        }
        let (step, changes) = self.prepare_bridge(Amount::Arrive(Self::bridge_arrive(short, Decimal::ZERO, Some(on_solana))), relay::chain::HYPERLIQUID,
                                                  relay::currency::HYPERLIQUID_USDC, 8, None, false).await?;
        self.report(&format!("Hyperliquid has {}: {} USDC from Solana → {} USDC there first. {}", usd(free), Fmt::num(step.spend, 2), Fmt::num(step.arriving, 2),
                             changes.join(" ")));
        self.send_bridge(&step).await?;
        Ok(())
    }

    /// Millionths of a dollar as USDC, to the last unit.
    pub fn usdc(units: u64) -> String { Fmt::num(millionths(units), 6) }

    pub(crate) async fn send_solana_usdc(&self, units: u64, owner: &str, label: &str) -> Result<String> {
        self.send_spl(relay::currency::SOLANA_USDC, units, 6, "USDC", spl::TOKEN_PROGRAM, owner, label, false).await
    }

    /// An SPL token from the wallet's own account to `owner`'s, made for them when they have none (≈ 0.002 SOL of rent,
    /// paid here); the simulation must show exactly `units` leaving. `program`: the mint's, SPL Token or Token-2022.
    /// Swift's defaults: `program` SPL Token, `close` false.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn send_spl(&self, mint: &str, units: u64, decimals: u8, symbol: &str, program: &str, owner: &str, label: &str, close: bool)
        -> Result<String> {
        let sol = self.wallet.solana.as_str();
        // Fmt::num shows at most 28 places: past them its scale is past Decimal.
        let f = |u: u64| Fmt::num(shift(u, decimals as i64), (decimals as u32).min(28));
        let have = self.rpc.token_balance(sol, mint).await?;
        if have < units { return fail(format!("{} {symbol} on Solana, but the transfer needs {}.", f(have), f(units))) }
        let (blockhash, last_valid) = self.rpc.latest_blockhash().await?;
        let unsigned = solana::unsigned_transaction(&spl::transfer_message(sol, owner, mint, units, decimals, &blockhash, 20_000, program, close && units == have)?);
        let message = Message::parse_transaction(&unsigned)?;
        let mine = spl::associated_account(sol, mint, program)?;
        let owners = BTreeMap::from([(mine.clone(), sol.to_string()), (spl::associated_account(owner, mint, program)?, owner.to_string())]);
        let policy = Policy { recipients: HashSet::from([owner.to_string()]), ..Policy::new(sol) };
        let mut verdict = guard::sol::check(&message, &message.static_keys, &policy, &owners.clone().into_iter().collect());
        let sol_before = self.rpc.balance(sol).await?;
        let simulation = self.rpc.simulate(&unsigned, &[mine.clone(), sol.to_string()], false).await?;
        // A failed simulation says nothing about balances: stop here rather than quote made-up changes.
        if let Some(error) = &simulation.error { return fail(format!("The simulation failed, so nothing was signed: {error}")) }
        verdict.merge(guard::sol::balances(&[
            BalanceRule { label: symbol.into(), before: i(have), after: i(simulation.after.get(&mine).and_then(|s| s.1).unwrap_or(0)), at_least: -i(units),
                          at_most: None },
            // The fee, plus the recipient's token account rent (≈ 0.002 SOL) when they have none yet.
            BalanceRule { label: "lamports".into(), before: i(sol_before), after: i(simulation.after.get(sol).map_or(0, |s| s.0)), at_least: -3_000_000,
                          at_most: None },
        ]));
        if !verdict.ok() { return stop(format!("The guard stopped the transfer: {}", verdict.problems.join(" "))) }
        let signed = self.signer.sign_solana(unsigned.clone(), message.static_keys.clone(), vec![owner.into()], owners).await?;
        if signed.len() != unsigned.len() || signed.len() < 65 || signed[65..] != unsigned[65..] { return stop("What was signed isn't what was checked.") }
        let signature = base58::encode(&signed[1..65]);
        self.record("solana", &signature, Kind::Signed, &format!("send {} {symbol} to {label}; valid until block {last_valid}", f(units)))?;
        match self.broadcast_solana(&signed, &signature, last_valid).await? {
            net::solana_rpc::Outcome::Confirmed => {
                self.record("solana", &signature, Kind::Confirmed, "")?;
                Ok(format!("Sent {} {symbol} to {label} on Solana. https://solscan.io/tx/{signature}", f(units)))
            }
            net::solana_rpc::Outcome::Failed(e) => {
                self.record("solana", &signature, Kind::Failed, &e)?;
                fail(format!("The transfer failed: {e}. The money stayed."))
            }
            net::solana_rpc::Outcome::Expired => {
                self.record("solana", &signature, Kind::Expired, "")?;
                fail("The transfer didn't make it into a block, so nothing moved. You can retry.")
            }
        }
    }

    // MARK: Native NEAR and SOL

    /// NEAR to a saved NEAR account; None = all but the account's storage and 0.01 NEAR — a transfer that opens a new
    /// implicit account prepays ≈ 0.0076 NEAR of gas at NEAR's pessimistic price, refunded after.
    pub async fn send_near(&self, requested: Option<Decimal>, receiver: &str, account: &str, label: &str) -> Result<String> {
        if receiver == account { return fail("That is this wallet's own NEAR account.") }
        // A named account can't be created by a transfer: sending to one that doesn't exist would only fail.
        if !crate::wallet::implicit(receiver) && Near::balance(receiver).await.map_or(Decimal::ZERO, |b| b.0).is_zero() {
            return fail(format!("{receiver} doesn't exist on NEAR: check the name in Addresses."));
        }
        let context = Near::tx_context(account).await?;
        let micro: u128 = 1_000_000_000_000_000_000; // 10⁻⁶ NEAR
        let balance: u128 = context.balance.parse().unwrap_or(0);
        let keep = context.storage_bytes as u128 * 10_000_000_000_000_000_000 + 10_000_000_000_000_000_000_000;
        let free = if balance > keep { (balance - keep) / micro * micro } else { 0 };
        let yocto = Self::near_yocto(requested, micro, free)?;
        if !(yocto > 0 && yocto <= free) {
            return fail(format!("{} NEAR can be sent: the account keeps {} for its storage and the fee.", wallet::near::near(free), wallet::near::near(keep)));
        }
        let signed = self.signer.sign_near_transfer(yocto, receiver.into(), vec![receiver.into()], context.nonce, context.block_hash.clone()).await?;
        let hash = Near::send(&signed).await?;
        self.record("near", &hash, Kind::Confirmed, &format!("send {} NEAR to {label}", wallet::near::near(yocto)))?;
        Ok(format!("Sent {} NEAR to {label}. https://nearblocks.io/txns/{hash}", wallet::near::near(yocto)))
    }

    /// `requested` NEAR in yocto, whole micro-NEAR; None = `free` (and so is an amount `UInt128(_:)` doesn't read).
    fn near_yocto(requested: Option<Decimal>, micro: u128, free: u128) -> Result<u128> {
        let Some(r) = requested else { return Ok(free) };
        let Some(m) = (Num::of(r) * 1_000_000).rounded(0, Rounding::Down).0 else { return fail(OUT_OF_RANGE) };
        match text(m).parse::<u128>() {
            Ok(m) => m.checked_mul(micro).ok_or_else(|| Error::Execution(OUT_OF_RANGE.into())),
            Err(_) => Ok(free),
        }
    }

    /// Deletes the wallet's NEAR account; NEAR sends everything on it, storage included, to `beneficiary`. Refused
    /// unless the beneficiary exists: NEAR would burn the balance otherwise.
    pub async fn close_near(&self, beneficiary: &str, account: &str, label: &str) -> Result<String> {
        if beneficiary == account { return fail("An account can't be closed into itself.") }
        if Near::balance(beneficiary).await.map_or(Decimal::ZERO, |b| b.0) <= Decimal::ZERO {
            return fail(format!("{label} doesn't exist on NEAR yet: closing into it would burn the NEAR. Send it a little NEAR first."));
        }
        let context = Near::tx_context(account).await?;
        let signed = self.signer.sign_near_delete_account(beneficiary.into(), vec![beneficiary.into()], context.nonce, context.block_hash.clone()).await?;
        let hash = Near::send(&signed).await?;
        self.record("near", &hash, Kind::Confirmed, &format!("NEAR account closed into {label}"))?;
        Ok(format!("Closed your NEAR account: its {} NEAR went to {label}. https://nearblocks.io/txns/{hash}",
                   wallet::near::near(context.balance.parse().unwrap_or(0))))
    }

    /// SOL to a saved Solana address: only that recipient (the guard and the signer check), and the simulation must
    /// show exactly this SOL leaving, plus the fee. None = all of it: the wallet's empty token accounts close in the same
    /// transaction and their rent goes too, so only nothing stays.
    pub async fn send_sol(&self, amount: Option<Decimal>, owner: &str, label: &str) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        if owner == sol { return fail("That is this wallet's own address.") }
        let have = self.rpc.balance(sol).await?;
        let empties = if amount.is_none() { self.empty_token_accounts().await? } else { vec![] };
        let all = empties.iter().fold(have, |sum, (_, l)| sum.saturating_add(*l));
        // No priority fee on «all»: the fee is then exactly Solana's 5,000 lamports a signature, and nothing is left over.
        let lamports = amount.map_or(all.saturating_sub(5_000), |s| units_of(s, 9));
        let shown = Fmt::num(shift(lamports, 9), 9);
        if lamports == 0 { return fail("Nothing to send.") }
        if !(amount.is_none() || have >= lamports.saturating_add(20_000)) {
            return fail(format!("{} SOL on Solana: not enough for {shown} and the fee.", Fmt::num(shift(have, 9), 9)));
        }
        // An empty account opens only with Solana's rent minimum, the network's word: less than that is refused by the network.
        if self.rpc.balance(owner).await? == 0 {
            let least = self.rpc.rent_minimum(0).await?;
            if lamports < least {
                return fail(format!("{label}'s Solana account is empty: Solana opens it only with at least {} SOL (its rent minimum). Send at least that.",
                                    Fmt::num(shift(least, 9), 9)));
            }
        }
        let (blockhash, last_valid) = self.rpc.latest_blockhash().await?;
        let mut data = vec![2, 0, 0, 0];
        data.extend(lamports.to_le_bytes());
        let transfer = Instruction { program: guard::sol::SYSTEM.into(), accounts: vec![(sol.into(), true, true), (owner.into(), false, true)], data };
        // CloseAccount(account, destination, owner): each empty token account's rent comes back first, then all of it goes.
        let mut instructions: Vec<Instruction> = empties.iter().map(|(a, _)| Instruction {
            program: spl::TOKEN_PROGRAM.into(), accounts: vec![(a.clone(), false, true), (sol.into(), false, true), (sol.into(), true, false)], data: vec![9],
        }).collect();
        instructions.push(transfer);
        let unsigned = solana::unsigned_transaction(&solana::compile_message(&instructions, sol, &blockhash, if amount.is_none() { 0 } else { 20_000 })?);
        let message = Message::parse_transaction(&unsigned)?;
        let owners: BTreeMap<String, String> = empties.iter().map(|(a, _)| (a.clone(), sol.to_string())).collect();
        let policy = Policy { recipients: HashSet::from([owner.to_string()]), ..Policy::new(sol) };
        let mut verdict = guard::sol::check(&message, &message.static_keys, &policy, &owners.clone().into_iter().collect());
        let simulation = self.rpc.simulate(&unsigned, &[sol.to_string()], false).await?;
        if let Some(error) = &simulation.error { return fail(format!("The simulation failed, so nothing was signed: {error}")) }
        verdict.merge(guard::sol::balances(&[BalanceRule {
            label: "lamports".into(), before: i(have), after: i(simulation.after.get(sol).map_or(0, |s| s.0)), at_least: (-i(lamports)).saturating_sub(20_000), at_most: None,
        }]));
        // «All» means nothing stays: a fee other than counted shows here, before anything is signed.
        if amount.is_none() && let Some((left, _)) = simulation.after.get(sol) && *left != 0 {
            return fail(format!("The simulation leaves {} SOL behind, so nothing was signed. Send a number instead.", Fmt::num(shift(*left, 9), 9)));
        }
        if !verdict.ok() { return stop(format!("The guard stopped the transfer: {}", verdict.problems.join(" "))) }
        let signed = self.signer.sign_solana(unsigned.clone(), message.static_keys.clone(), vec![owner.into()], owners).await?;
        if signed.len() != unsigned.len() || signed.len() < 65 || signed[65..] != unsigned[65..] { return stop("What was signed isn't what was checked.") }
        let signature = base58::encode(&signed[1..65]);
        self.record("solana", &signature, Kind::Signed, &format!("send {shown} SOL to {label}; valid until block {last_valid}"))?;
        match self.broadcast_solana(&signed, &signature, last_valid).await? {
            net::solana_rpc::Outcome::Confirmed => {
                self.record("solana", &signature, Kind::Confirmed, "")?;
                let closed = if empties.is_empty() {
                    String::new()
                } else {
                    format!(" {} empty token account{} closed, their rent went with it.", empties.len(), if empties.len() == 1 { "" } else { "s" })
                };
                Ok(format!("Sent {shown} SOL to {label} on Solana.{closed} https://solscan.io/tx/{signature}"))
            }
            net::solana_rpc::Outcome::Failed(e) => {
                self.record("solana", &signature, Kind::Failed, &e)?;
                fail(format!("The transfer failed: {e}. The SOL stayed."))
            }
            net::solana_rpc::Outcome::Expired => {
                self.record("solana", &signature, Kind::Expired, "")?;
                fail("The transfer didn't make it into a block, so nothing moved. You can retry.")
            }
        }
    }

    // MARK: Any token, as it is

    /// Any token the wallet holds on one network to a saved address — no swap, no bridge: Solana's SPL and Token-2022
    /// tokens, an EVM network's ERC-20s and its own coin, named by ticker, contract or mint and read from the chain. Only
    /// `recipient` may receive it. None amount = all of it; of a network's own coin, all but the fee.
    pub async fn send_token(&self, query: &str, amount: Option<Decimal>, network: &str, recipient: &str, label: &str) -> Result<String> {
        if network == "solana" { return self.send_solana_token(query, amount, recipient, label).await }
        let Some(net) = EvmNetwork::named(network) else {
            return fail(format!("ihatecrypto sends tokens as they are on Solana and the EVM networks, not on {network}."));
        };
        if recipient.to_lowercase() == self.wallet.evm.to_lowercase() { return fail("That is this wallet's own address.") }
        let (contract, symbol) = self.held_evm(query, net).await?;
        if contract == "native" { return self.send_native(amount, net, recipient, label).await }
        let Some(decimals) = abi::uint_in(&net.rpc.read(&contract, &[0x31, 0x3c, 0xe5, 0x67]).await?, 0).filter(|d| *d <= 36) else {
            return fail(format!("{symbol} on {} doesn't answer decimals(): not a token ihatecrypto can send.", net.name));
        };
        let decimals = decimals as i64;
        let f = |units: Decimal| Fmt::num(shifted(units, decimals).unwrap_or_default(), (decimals as u32).min(28));
        let have = self.balance(&contract, &net.rpc).await?;
        let mut units = match amount {
            Some(a) => match times_pow10(a, decimals) { Some(x) => rounded(x, 0, Rounding::Down), None => return fail(OUT_OF_RANGE) },
            None => have,
        };
        // All of the coin fees are paid in (Tempo's USDC): what this very transfer may cost stays for its fee, a tenth
        // over, not a whole cent.
        if amount.is_none() && net.fee_token == Some(contract.to_lowercase().as_str()) {
            let gas = net.rpc.estimate_gas(&self.wallet.evm, &contract, 0, &erc20::transfer(recipient, crate::u256(have)?)?, None).await?;
            let max_fee = net.rpc.fees().await?.1;
            let Some(attodollars) = gas.saturating_add(gas / 4).checked_mul(max_fee) else { return fail(OUT_OF_RANGE) };
            let fee = Decimal::from(tempo::fee_units(attodollars));
            units = rounded(num(Num::of(have) - Num::of(fee) * d("1.1"))?.max(Decimal::ZERO), 0, Rounding::Down);
        }
        if units <= Decimal::ZERO { return fail(format!("No {symbol} on {} to send.", net.name)) }
        if units > have { return fail(format!("{} {symbol} on {} — can't send {}.", f(have), net.name, f(units))) }
        let hash = self.send_evm(&net.rpc, &contract, &erc20::transfer(recipient, crate::u256(units)?)?, &format!("Send {} {symbol} to {label}", f(units)),
                                 &[recipient.to_string()], None, 0).await?;
        Ok(format!("Sent {} {symbol} to {label} on {}. {}/tx/{hash}", f(units), net.name, net.explorer))
    }

    /// The token on `net` the user named: the network's coin, its dollar (USDC, USDG on Robinhood Chain), a contract, or a
    /// ticker among what the wallet holds there (holdings::pick). (contract, symbol); «native» for the coin.
    async fn held_evm(&self, query: &str, net: &EvmNetwork) -> Result<(String, String)> {
        let q = query.to_uppercase();
        if q == net.gas_coin && net.fee_token.is_none() { return Ok(("native".into(), net.gas_coin.into())) }
        if (q == "USDC" && net.key != "robinhood") || (q == "USDG" && net.key == "robinhood") { return Ok((net.usdc.into(), q)) }
        if evm_address(query) {
            let symbol = abi::string(&net.rpc.read(query, &[0x95, 0xd8, 0x9b, 0x41]).await?).unwrap_or_else(|| format!("{}…", prefix(query, 10)));
            return Ok((query.into(), symbol));
        }
        // A dollar coin by its ticker is the real one, read from the node: Blockscout's list of a wallet's tokens came back
        // empty on Polygon and stale on Base while the USDT sat there (2026-10-13).
        if let Some(coin) = stablecoins::find(query, net.key).filter(|c| c.address.starts_with("0x")) { return Ok((coin.address.into(), coin.symbol.into())) }
        let held = holdings::evm(net.key, &self.wallet.evm, false, &HashSet::new(), &[]).await;
        if let Some(held) = &held {
            match holdings::pick(query, held) {
                Pick::One(h) => return Ok((h.contract, h.symbol)),
                Pick::Several(list) => {
                    return fail(format!("Several tokens on {} are called {query}: {}. Name it by its contract.", net.name,
                                        list.iter().map(|h| format!("{} ({})", h.contract, Fmt::num(h.amount, 2))).collect::<Vec<_>>().join(", ")));
                }
                Pick::None => {}
            }
        }
        // Not in the list (it lags a token that just arrived) or no list: a token the lists know by that ticker, when the
        // node says this wallet holds it.
        if let Ok(Some(t)) = EvmTokens::find(query, net.rpc.chain_id).await
            && self.balance(&t.address, &net.rpc).await.unwrap_or_default() > Decimal::ZERO {
            return Ok((t.address, t.symbol));
        }
        fail(if held.is_none() {
            format!("Couldn't read the tokens on {} just now: name it by its contract, or try again.", net.name)
        } else {
            format!("This wallet holds no {query} on {}.", net.name)
        })
    }

    /// The network's own coin to a saved address: a plain transfer, the fee on top of it. None = all of it but the fee: a
    /// plain transfer's gas is exact, so only the part of the fee the network doesn't take stays, a speck.
    async fn send_native(&self, amount: Option<Decimal>, net: &EvmNetwork, recipient: &str, label: &str) -> Result<String> {
        let (me, rpc, coin) = (self.wallet.evm.as_str(), &net.rpc, net.gas_coin);
        let have = rpc.balance(me).await?;
        let (tip, max_fee) = rpc.fees().await?;
        let (wei, gas_limit): (u64, u64);
        if let Some(amount) = amount {
            let wei_decimal = times_pow10(amount, 18).map(|w| rounded(w, 0, Rounding::Down));
            if wei_decimal.map_or(amount <= Decimal::ZERO, |w| w <= Decimal::ZERO) {
                return fail(format!("Not an amount of {coin} to send: {}.", Fmt::num(amount, 18)));
            }
            // One transfer carries under 2⁶⁴ wei (the signer's value is a UInt64): 18.44 coins.
            let Some(w) = wei_decimal.filter(|w| *w < Decimal::from(u64::MAX)) else { return fail(format!("At most 18 {coin} at once. Nothing was signed.")) };
            wei = uint64(w);
            let estimate = rpc.estimate_gas(me, recipient, wei, &[], None).await?;
            gas_limit = estimate.saturating_add(estimate / 4);
        } else {
            let estimate = rpc.estimate_gas(me, recipient, 1, &[], None).await?;
            gas_limit = if estimate == 21_000 { estimate } else { estimate.saturating_add(estimate / 4) };
            let l1 = self.l1_fee_bound(net).await?;
            let Some(fee) = gas_limit.checked_mul(max_fee).and_then(|f| f.checked_add(l1)) else { return fail(OUT_OF_RANGE) };
            if have <= fee as u128 { return fail(format!("{} {coin} on {}: no more than the fee (up to {}), so nothing goes.", coins(have), net.name, coins(fee as u128))) }
            let Ok(all) = u64::try_from(have - fee as u128) else { return fail(format!("At most 18 {coin} at once: say how much. Nothing was signed.")) };
            wei = all;
        }
        let Some(fee) = gas_limit.checked_mul(max_fee) else { return fail(OUT_OF_RANGE) };
        if wei as u128 + fee as u128 > have {
            return fail(format!("{} {coin} on {}: not enough for {} and the fee (up to {}).", coins(have), net.name, coins(wei as u128), coins(fee as u128)));
        }
        let note = format!("Send {} {coin} to {label}", coins(wei as u128));
        let nonce = self.next_nonce(rpc).await?;
        let fees = EvmFees { chain_id: rpc.chain_id, nonce, max_priority_fee: tip, max_fee, gas_limit };
        let raw = self.signer.sign_evm_transfer(fees, recipient.into(), wei, vec![recipient.into()]).await?;
        let hash = format!("0x{}", to_hex(&keccak(&raw)));
        self.record(net.key, &hash, Kind::Signed, &note)?;
        rpc.send(&raw).await?;
        if !rpc.wait_for_receipt(&hash, Duration::from_secs(120)).await? {
            self.record(net.key, &hash, Kind::Failed, &note)?;
            return fail(format!("{note} failed on {}: {}/tx/{hash}", net.name, net.explorer));
        }
        self.record(net.key, &hash, Kind::Confirmed, "")?;
        self.confirmed(nonce.saturating_add(1), rpc);
        Ok(format!("Sent {} {coin} to {label} on {}. {}/tx/{hash}", coins(wei as u128), net.name, net.explorer))
    }

    /// What an OP-stack network (Base, Optimism) charges on top of gas for posting a plain transfer to Ethereum: the
    /// oracle's upper bound for a 150-byte transaction. 0 elsewhere (Arbitrum's estimate holds its L1 part).
    async fn l1_fee_bound(&self, net: &EvmNetwork) -> Result<u64> {
        if !["base", "optimism"].contains(&net.key) { return Ok(0) }
        let call = [&Self::L1_FEE_UPPER_BOUND[..], &abi::uint(150)].concat();
        Ok(abi::uint_in(&net.rpc.read("0x420000000000000000000000000000000000000F", &call).await?, 0).unwrap_or(0))
    }

    /// getL1FeeUpperBound(uint256): keccak's first four bytes.
    pub(crate) const L1_FEE_UPPER_BOUND: [u8; 4] = [0xf1, 0xc7, 0xa5, 0x8b];

    /// An SPL or Token-2022 token, or SOL with or without a number, from the Solana wallet.
    async fn send_solana_token(&self, query: &str, amount: Option<Decimal>, owner: &str, label: &str) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let q = query.to_uppercase();
        if q == "SOL" { return self.send_sol(amount, owner, label).await }
        if owner == sol { return fail("That is this wallet's own address.") }
        let (mint, symbol): (String, String) = if q == "USDC" {
            (relay::currency::SOLANA_USDC.into(), "USDC".into())
        } else if let Some(coin) = stablecoins::on("solana").and_then(|c| c.iter().find(|c| c.symbol.to_uppercase() == q)) {
            // A dollar coin by its ticker is the real one (USDT is Tether's mint), and needs no read of the whole wallet.
            (coin.address.into(), coin.symbol.into())
        } else if base58_address(query) {
            (query.into(), format!("{}…", prefix(query, 4)))
        } else {
            let held = Solana::balance(sol, true).await?.tokens;
            match holdings::pick(query, &held) {
                Pick::One(h) => (h.contract, h.symbol),
                Pick::None => return fail(format!("This wallet holds no {query} on Solana.")),
                Pick::Several(list) => {
                    return fail(format!("Several tokens on Solana are called {query}: {}. Name it by its mint.",
                                        list.iter().map(|h| format!("{} ({})", h.contract, Fmt::num(h.amount, 2))).collect::<Vec<_>>().join(", ")));
                }
            }
        };
        // The mint's owner is its token program: SPL Token or Token-2022, and the accounts derive from it.
        let program = self.token_program(&mint).await?;
        let mine = spl::associated_account(sol, &mint, &program)?;
        let value = self.rpc.call("getTokenAccountBalance", json!([mine, {"commitment": "finalized"}])).await.ok().and_then(|r| r.get("value").cloned());
        let have = value.as_ref().and_then(|v| v.get("amount")).and_then(Value::as_str).and_then(|a| a.parse::<u64>().ok());
        let decimals = value.as_ref().and_then(|v| v.get("decimals")).and_then(Value::as_u64).map(|d| d as u8);
        let (Some(have), Some(decimals)) = (have, decimals) else { return fail(format!("This wallet holds no {symbol} on Solana.")) };
        let wanted = amount.map(|a| times_pow10(a, decimals as i64).map(|w| rounded(w, 0, Rounding::Down)));
        let sendable = match wanted { Some(w) => w.is_some_and(|w| w > Decimal::ZERO && w <= Decimal::from(u64::MAX)), None => have > 0 };
        if !sendable { return fail("Nothing to send.") }
        let units = wanted.flatten().map_or(have, uint64);
        // All of a token: its account closes and the rent (≈ 0.002 SOL) comes back. USDC's stays: the wallet lives on it.
        let close = amount.is_none() && mint != relay::currency::SOLANA_USDC && program == spl::TOKEN_PROGRAM;
        self.send_spl(&mint, units, decimals, &symbol, &program, owner, label, close).await
    }

    /// The wallet's empty classic SPL token accounts, not frozen, up to 20 (what fits one transaction): (address, lamports).
    // ponytail: classic SPL only; an empty Token-2022 account can hold withheld fees that make closing it fail.
    pub(crate) async fn empty_token_accounts(&self) -> Result<Vec<(String, u64)>> {
        let r = self.rpc.call("getTokenAccountsByOwner", json!([self.wallet.solana, {"programId": spl::TOKEN_PROGRAM},
                                                                  {"encoding": "jsonParsed", "commitment": "finalized"}])).await?;
        Ok(r.get("value").and_then(Value::as_array).into_iter().flatten().filter_map(|item| {
            let address = item.get("pubkey")?.as_str()?;
            let account = item.get("account")?;
            let info = account.pointer("/data/parsed/info")?;
            let empty = info.pointer("/tokenAmount/amount").and_then(Value::as_str) == Some("0") && info.get("state").and_then(Value::as_str) != Some("frozen");
            empty.then_some(())?;
            Some((address.to_string(), account.get("lamports")?.as_u64()?))
        }).take(20).collect())
    }
}

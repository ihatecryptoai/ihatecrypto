//! The automation wallet signs with no one at the PC: the signer itself counts the dollars that leave it, held to the
//! person's day and week (Settings), and asks in person (a prompt that closes after a minute) for anything past them or
//! anything that isn't dollars going somewhere other than the person's own wallets — never the engine's word
//! (`Automation.swift`, the 2026-11-08 review). Read from the same request the signer then checks and signs.

use std::collections::BTreeSet;
use std::str::FromStr;

use evm::call::Call;
use evm::protocols::relay;
use guard::sol;
use hyperliquid::action::{self, Opening};
use rust_decimal::Decimal;
use signer_api::SignerRequest;
use solana::message::Message;
use solana::spl;
use wallet::Result;

use crate::core::{self, relay_orders};
use crate::limits;
use crate::vault::{own, recipients};
use crate::wallets;

/// Dollars the signer counts by mint, with their decimals.
fn solana_dollars(mint: &str) -> Option<u32> {
    matches!(mint, "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" | "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB").then_some(6)
}

/// Programs a task's Solana move may call without the person: fees, accounts, transfers, Jupiter's swaps and fills.
const SOLANA_PROGRAMS: [&str; 7] = [sol::COMPUTE_BUDGET, sol::SYSTEM, spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM, spl::ASSOCIATED_TOKEN_PROGRAM,
                                    sol::JUPITER_SWAP, sol::JUPITER_Z];

/// What leaves, counted: dollars, and in words whatever the signer can't count as dollars.
#[derive(Default)]
struct Tally {
    dollars: Decimal,
    others: Vec<String>,
}

impl Tally {
    fn add(&mut self, units: Decimal, decimals: Option<u32>, to: Option<&str>, name: impl FnOnce() -> String) {
        if to.is_some_and(is_own) { return }
        match decimals {
            Some(d) => self.dollars = self.dollars.saturating_add(units / Decimal::from(10u64.pow(d))),
            None => self.others.push(name()),
        }
    }
}

/// `n` / 10^`scale` exactly, as Swift's `Decimal(n) / pow(10, scale)` writes it.
fn shifted(n: u64, scale: u32) -> String { Decimal::from_i128_with_scale(n as i128, scale).normalize().to_string() }

/// What `request` spends in dollars, already taken from the budget — after the person was asked about whatever the
/// signer can't count or what goes past the limits. The caller keeps it once the move is signed, else it's given back.
pub fn spend(request: &SignerRequest) -> Result<limits::Hold> {
    use SignerRequest as R;
    let mut t = Tally::default();
    match request {
        R::SignSolana { wire, accounts, recipients, token_owners, extra_programs, sponsor } => {
            let owner = core::address("solana")?;
            let message = Message::parse_transaction(wire)?;
            let policy = sol::Policy { recipients: recipients.iter().cloned().collect(), trust_lookup_tables: false, sponsor: sponsor.clone(),
                                       ..sol::Policy::new(&owner) };
            let verdict = sol::check(&message, accounts, &policy, &core::verified_owners(token_owners, accounts));
            for o in &verdict.outflows {
                t.add(Decimal::from(o.units), solana_dollars(&o.asset), o.to.as_deref(), || {
                    if o.asset == "SOL" { format!("{} SOL", shifted(o.units, 9)) } else { format!("{} units of {}…", o.units, prefix(&o.asset, 6)) }
                });
            }
            let called: BTreeSet<&String> = message.instructions.iter().filter_map(|ix| accounts.get(ix.program_index)).collect();
            t.others.extend(called.into_iter().filter(|p| !SOLANA_PROGRAMS.contains(&p.as_str())).map(|p| format!("a call to {}…", prefix(p, 6))));
            if !extra_programs.is_empty() { t.others.push("a program not on ihatecrypto's list".into()) }
        }
        R::SignEvmTransaction { chain_id, to, value, data, .. } => evm(&mut t, *chain_id, to, *value, data),
        R::SignEvmBatch { chain_id, calls, .. } => for c in calls { evm(&mut t, *chain_id, &c.to, 0, &c.data) },
        R::SignSponsoredTransfer { token, to, amount, .. } => {
            t.add(Decimal::from_str(amount).unwrap_or_default(), dollar_decimals(token), Some(to), || format!("{amount} units of {}…", prefix(token, 8)));
        }
        R::SignRelayMulticallPermit { chain_id, token, value, valid_before, relayer, refund_to, nft_recipient, metadata, calls } => {
            // Into Relay's depository for an order whose payee the signer checked: to this wallet's own address elsewhere
            // it's the person's money moving, else spending.
            let m = relay::MulticallPermit {
                chain_id: *chain_id, token: token.clone(), from: core::address("evm")?, value: *value, valid_before: *valid_before, relayer: relayer.clone(),
                refund_to: refund_to.clone(), nft_recipient: nft_recipient.clone(), metadata: metadata.clone(),
                calls: calls.iter().map(|c| relay::Call { target: c.target.clone(), allow_failure: c.allow_failure, value: c.value, call_data: c.data.clone() }).collect(),
            };
            let paid_to_own = m.deposit_id().and_then(|id| relay_orders::order(&id)).is_some_and(|o| is_own(&o.recipient));
            if !paid_to_own { t.add(Decimal::from(*value), dollar_decimals(token), None, || format!("{value} units of {}…", prefix(token, 8))) }
        }
        R::SignHyperliquidL1 { action: act, .. } => match action::opening(act) {
            Opening::Dollars(d) => t.dollars = t.dollars.saturating_add(d),
            Opening::Person(why) => t.others.push(why),
        },
        R::SignHyperliquidUser { action: act, .. } => {
            let to = act.text("destination").unwrap_or("");
            let token = act.text("token").map_or("USDC".to_string(), |k| k.split(':').next().unwrap_or("").to_string());
            let amount = act.text("amount").and_then(|a| Decimal::from_str(a).ok()).unwrap_or_default();
            match action::kind(act) {
                Some("usdClassTransfer") => {}
                Some("vaultTransfer") => t.others.push("a Hyperliquid vault transfer".into()),
                _ => t.add(amount, (token == "USDC").then_some(0), Some(to), || format!("{amount} {token}")),
            }
        }
        R::SignNearIntentDeposit { amount, .. } => t.others.push(format!("{amount} yoctoNEAR")),
        R::SignNearTransfer { amount, receiver, .. } => t.add(Decimal::ZERO, None, Some(receiver), || format!("{amount} yoctoNEAR")),
        R::SignRelayNonceMapping { .. } | R::RelayOrder { .. } | R::SignReceiveAuthorization { .. } => {}
        _ => t.others.push("a request a task doesn't make".into()),
    }
    // Taken from the day and week at once (two tasks at the same moment can't both fit in what's left); past them, or for
    // anything that isn't dollars, the person allows it and it's counted all the same.
    let taken = if t.others.is_empty() { limits::take_automation(t.dollars, &limits::current()) }
                else { Err(format!("the automation wallet spends without you only dollars, not {}", t.others.join(", "))) };
    match taken {
        Ok(held) => Ok(held),
        Err(why) => {
            core::confirm_afresh(&format!("a scheduled task's move: {why}"))?;
            Ok(limits::take(limits::Budget::AUTOMATION, t.dollars))
        }
    }
}

/// An EVM call's outflows: coins sent with it, tokens transferred or approved for someone else to pull, dollars burned
/// or sent across.
fn evm(t: &mut Tally, chain_id: u64, to: &str, value: u64, data: &[u8]) {
    let call = Call::decode(data);
    if value > 0 && !call.returns_coins(to, value, chain_id) {
        t.add(Decimal::from(value), None, Some(to), || format!("{} {}", shifted(value, 18), evm::coin(chain_id)));
    }
    let token = to.to_lowercase();
    let decimals = dollar_decimals(&token);
    let units = |word: &[u8]| -> (Decimal, String) {
        let n = evm::word_value(word);
        (u128::try_from(n).ok().and_then(|n| Decimal::try_from_i128_with_scale(n as i128, 0).ok()).unwrap_or(Decimal::MAX), n.to_string())
    };
    let short = prefix(&token, 8);
    match &call {
        Call::Transfer { to: r, amount } => { let (u, s) = units(amount); t.add(u, decimals, Some(r), || format!("{s} units of {short}…")) }
        Call::Approve { spender, amount } | Call::IncreaseAllowance { spender, amount } => {
            let (u, s) = units(amount);
            t.add(u, decimals, Some(spender), || format!("a right to {s} units of {short}…"));
        }
        Call::Permit2Approve { token: pt, spender, amount, .. } => {
            let (u, s) = units(amount);
            t.add(u, dollar_decimals(pt), Some(spender), || format!("a right to {s} units of {}…", prefix(pt, 8)));
        }
        Call::CctpBurn { recipient, amount, .. } | Call::OftSend { recipient, amount, .. } => {
            let (u, s) = units(amount);
            t.add(u, decimals, recipient.as_deref(), || format!("{s} units sent across"));
        }
        _ => {}
    }
}

fn dollar_decimals(token: &str) -> Option<u32> { evm::dollar(token).map(|_| evm::dollar_decimals(token) as u32) }

/// One of the person's own wallets on this PC: money moving there isn't spent. Not a test wallet, which signs without a
/// prompt itself: money could leave the limits through it.
fn is_own(address: &str) -> bool {
    let key = recipients::key(address);
    let all = wallets::all();
    let holds = |w: &&signer_api::SignerWallet| [&w.solana, &w.evm, &w.near].into_iter().flatten().any(|a| recipients::key(a) == key);
    if all.iter().filter(|w| w.is_test()).any(|w| holds(&w)) { return false }
    own::all().contains(&key) || all.iter().any(|w| holds(&w))
}

fn prefix(s: &str, n: usize) -> String { s.chars().take(n).collect() }

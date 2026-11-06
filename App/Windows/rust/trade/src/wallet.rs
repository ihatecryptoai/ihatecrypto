//! Moving money between the user's own places (Wallet.swift): Hyperliquid back to Solana, Solana USDC into a vault and
//! out, Jupiter's swaps and DCA, NEAR through 1Click, HLP, the spread over the best rates, Kamino — and `sign_and_send`,
//! the guard, simulation, signature and send every Solana move here goes through.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use chrono::Utc;
use defi::allocation::{self, Place, Slice};
use defi::jupiter_lend::JupiterLend;
use defi::kamino::{self, Kamino, VaultPosition};
use defi::yields;
use evm::protocols::{aave, morpho};
use evm::{U256, erc20};
use execution::{Envelope, Kind};
use guard::sol::{BalanceRule, Policy};
use hyperliquid::MsgPack;
use ihatecrypto_core::foundation::{Num, Rounding, decimal, iso8601_fraction, rounded, shifted, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::{Hyperliquid, Near};
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use ihatecrypto_core::top_up::TopUpOrder;
use net::solana_rpc::{Outcome, SolanaRpc};
use protocols::one_click::{self, asset};
use protocols::{jupiter, recurring, relay};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Value, json};
use solana::message::Message;
use solana::{Instruction, spl};
use wallet::evm::EvmSignature;
use wallet::keys::base58;

use crate::aave::EvmNetwork;
use crate::executor::{HlReply, HyperliquidAccount, hl_exchange, hl_markets, uint64};
use crate::{CostlyMove, Error, Executor, JUPITER_Z_FIRST, OUT_OF_RANGE, Result, SWAP_TAIL_TEST, fail, landings, loss_accepted, modules, num, stop};

// MARK: Small helpers the G2 modules share

pub(crate) const WSOL: &str = "So11111111111111111111111111111111111111112";

pub(crate) fn d(s: &str) -> Decimal { s.parse().expect("a decimal literal") }
pub(crate) fn sleep(seconds: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_secs(seconds)) }
pub(crate) fn usd(x: Decimal) -> String { Fmt::usd(x) }
/// `Decimal(units) / 1_000_000`.
pub(crate) fn millionths(units: u64) -> Decimal { Decimal::from_i128_with_scale(units as i128, 6) }
/// Lamports (a change: may be negative) in SOL.
pub(crate) fn sol_of(lamports: i128) -> Decimal { Decimal::from_i128_with_scale(lamports, 9) }
/// `Decimal(units) / pow(10, decimals)`.
pub(crate) fn shift(units: u64, decimals: i64) -> Decimal { shifted(Decimal::from(units), decimals).unwrap_or_default() }
/// `x * pow(10, decimals)`; None past Decimal's range.
pub(crate) fn times_pow10(x: Decimal, decimals: i64) -> Option<Decimal> {
    let mut left = u32::try_from(decimals).ok()?;
    let mut v = x;
    while left > 0 {
        let step = left.min(28);
        v = v.checked_mul(Decimal::from_i128_with_scale(10i128.pow(step), 0))?;
        left -= step;
    }
    Some(v)
}
/// `NSDecimalNumber(decimal: (x * pow(10, decimals)).rounded(0, .down)).uint64Value`: 0 when it doesn't fit.
pub(crate) fn units_of(x: Decimal, decimals: i64) -> u64 { times_pow10(x, decimals).map_or(0, |v| uint64(rounded(v, 0, Rounding::Down))) }
/// `Int64(n)`, held to its range.
pub(crate) fn i(n: u64) -> i64 { i64::try_from(n).unwrap_or(i64::MAX) }
/// `x * 99 / 100` and `x * 101 / 100` in Int64, without the trap.
pub(crate) fn pct(x: i64, percent: i128) -> i64 { i64::try_from(x as i128 * percent / 100).unwrap_or(i64::MAX) }
/// The wallet's own SPL Token account for `mint`.
pub(crate) fn ata(owner: &str, mint: &str) -> Result<String> { Ok(spl::associated_account(owner, mint, spl::TOKEN_PROGRAM)?) }
pub(crate) fn owners(pairs: &[(&str, &str)]) -> BTreeMap<String, String> { pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
/// `/[0-9a-f]{64}/`: a NEAR implicit account.
pub(crate) fn implicit(account: &str) -> bool { account.len() == 64 && account.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
/// The first `n` characters (`prefix(n)`).
pub(crate) fn prefix(s: &str, n: usize) -> String { s.chars().take(n).collect() }
/// «0x» + r ‖ s ‖ v, as Relay takes a signature.
fn signature_hex(s: &EvmSignature) -> String { format!("0x{}{}{:02x}", wallet::to_hex(&s.r), wallet::to_hex(&s.s), s.v) }
fn pairs(p: &[(&str, &str)]) -> HashMap<String, String> { p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
fn add(a: Decimal, b: Decimal) -> Decimal { a.checked_add(b).unwrap_or(Decimal::MAX) }
fn sub(a: Decimal, b: Decimal) -> Decimal { a.checked_sub(b).unwrap_or(Decimal::MIN) }

/// A (balance rule) as `sign_and_send` takes it: account, label, a token balance (else lamports), the least change.
pub type Rule = (String, String, bool, i64);

/// Where a spread's money comes from (Executor::spread_moves): (from, to, usdc) moves, and what each place gives.
pub type SpreadMoves = (Vec<(String, String, Decimal)>, Vec<(String, Decimal)>);

/// «Spread USDC across the best rates» as data, for the studio canvas: where each slice goes and what was skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct AllocationPreview {
    pub asset: String,
    pub free: Decimal,
    pub max_share: Decimal,
    pub slices: Vec<Slice>,
    /// (place, apy, reason)
    pub skipped: Vec<(String, Decimal, String)>,
    pub leftover: Decimal,
    pub yearly: Decimal,
    pub yearly_usd: Option<Decimal>,
    /// Where the free money is: Solana and each EVM network with idle USDC.
    pub sources: Vec<(String, Decimal)>,
    /// What the plan takes from each place, and the moves (from, to, amount) between them before the deposits.
    pub drawn: Vec<(String, Decimal)>,
    pub moves: Vec<(String, String, Decimal)>,
}

impl Executor {
    // MARK: Hyperliquid → Solana

    /// Relay from the Hyperliquid perp balance to our Solana address. `usdc: None` = everything withdrawable.
    pub async fn withdraw_to_solana(&self, requested: Option<Decimal>) -> Result<String> {
        let (evm, sol) = (self.wallet.evm.as_str(), self.wallet.solana.as_str());
        let mut free = Hyperliquid::balance(evm).await?.withdrawable;
        // Margin left on a HIP-3 exchange (xyz, after an oil or stock trade) comes back to the main balance first;
        // what an open position there uses stays (only the withdrawable part moves).
        if requested.is_none_or(|r| r > free) {
            for dex in Hyperliquid::HIP3_DEXES {
                let idle = rounded(Hyperliquid::dex_balance(evm, dex).await?.1, 2, Rounding::Down);
                if idle <= Decimal::ZERO { continue }
                self.move_margin(dex, "", idle).await?;
            }
            free = Hyperliquid::balance(evm).await?.withdrawable;
        }
        let usdc = rounded(requested.unwrap_or(free), 2, Rounding::Down);
        if !(usdc > Decimal::ZERO && usdc <= free) {
            return fail(format!("{} free on Hyperliquid — can't withdraw {}.", usd(free), Fmt::num(usdc, 2)));
        }
        let units = units_of(usdc, 8); // Hyperliquid perp USDC: 8 decimals in Relay
        let quote = self.relay_quote(&relay::Ask {
            user: evm.into(), recipient: sol.into(), origin_chain: relay::chain::HYPERLIQUID, destination_chain: relay::chain::SOLANA,
            origin_currency: relay::currency::HYPERLIQUID_USDC.into(), destination_currency: relay::currency::SOLANA_USDC.into(), amount: units,
            ..Default::default()
        }).await?;
        let arriving = decimal(&quote.amount_out_formatted).unwrap_or_default();
        let before = self.rpc.token_balance(sol, relay::currency::SOLANA_USDC).await?;
        let started = Instant::now();
        let fee = decimal(&quote.fees_usd).map_or_else(|| quote.fees_usd.clone(), usd);
        self.send_from_hyperliquid(&quote, usdc, units, sol, num(Num::of(usdc) - arriving)?, &format!("Hyperliquid→Solana USDC {}", text(usdc)),
                                   &format!("Relay withdrawal: {} USDC from Hyperliquid → {} USDC on Solana, fee {fee}.", Fmt::num(usdc, 2),
                                            Fmt::num(arriving, 2))).await?;
        let request_id = quote.request_id.clone();
        // Inside a chain the next step is signed while this lands on Solana (Landings).
        if let Some(landings) = landings() {
            let me = self.arc();
            landings.add("solana", async move { me.withdrawal_landing(&request_id, before, started).await });
            return Ok(format!("Sent {} USDC from Hyperliquid: ≈ {} lands on Solana in seconds, and what needs it waits for it.", Fmt::num(usdc, 2),
                              Fmt::num(arriving, 2)));
        }
        self.withdrawal_landing(&request_id, before, started).await
    }

    /// Relay's Hyperliquid steps for `quote`, checked and sent: the depository binding signed and posted, then the sendAsset
    /// of exactly `usdc` to Relay's depository. The quote must be from this wallet to `recipient`, losing at most 3% (≤ $1).
    #[allow(clippy::too_many_arguments)]
    async fn send_from_hyperliquid(&self, quote: &relay::Quote, usdc: Decimal, units: u64, recipient: &str, lost: Decimal, note: &str, line: &str)
        -> Result<()> {
        let Some(hl) = &quote.hyperliquid else { return fail("The Relay quote has no Hyperliquid steps.") };
        let published = relay::depository(relay::chain::HYPERLIQUID).await?.map(|d| d.to_lowercase());
        let me = self.wallet.evm.as_str();
        // An EVM address in any case; a Solana one exactly (base58 letters differ by case).
        let evm = recipient.starts_with("0x");
        let to = if evm { quote.recipient.to_lowercase() } else { quote.recipient.clone() };
        let expected_recipient = if evm { recipient.to_lowercase() } else { recipient.to_string() };
        let amount_in = units.to_string();
        let mut verdict = guard::binding_check(
            &pairs(&[("sender", &quote.sender), ("recipient", &to), ("amountIn", &quote.amount_in), ("wallet", &hl.wallet), ("depositor", &hl.depositor),
                     ("destination", &hl.destination), ("published depository", published.as_deref().unwrap_or("none")), ("token", &hl.token)]),
            &pairs(&[("sender", me), ("recipient", &expected_recipient), ("amountIn", &amount_in), ("wallet", me), ("depositor", me),
                     ("destination", relay::HYPERLIQUID_DEPOSITORY), ("published depository", relay::HYPERLIQUID_DEPOSITORY),
                     ("token", "USDC:0x6d1e7cde53ba9467b783cb7c530ce054")]),
            "Relay quote");
        if decimal(&hl.amount) != Some(usdc) { verdict.problems.push(format!("sendAsset amount {} instead of {}.", hl.amount, text(usdc))) }
        if lost > (usdc * d("0.03")).min(Decimal::ONE) { verdict.problems.push(format!("Relay charges {} USDC: more than allowed.", Fmt::num(lost, 2))) }
        if !verdict.ok() { return stop(format!("The guard stopped the withdrawal: {}", verdict.problems.join(" "))) }
        self.report(line);

        let Some(id) = wallet::from_hex(&hl.mapping_id) else { return fail("Invalid Relay mapping id.") };
        let mapping = self.signer.sign_relay_mapping(hl.wallet.clone(), hl.depositor.clone(), id, hl.mapping_nonce).await?;
        relay::authorize(&hl.authorize_body, &signature_hex(&mapping)).await?;

        let action = hyperliquid::signer::send_asset_action(&hl.destination, &hl.source_dex, &hl.destination_dex, &hl.token, &hl.amount, hl.nonce, true);
        let signature = self.signer.sign_hyperliquid_user(action.clone(), vec![relay::HYPERLIQUID_DEPOSITORY.into()]).await?;
        self.record("relay", &quote.request_id, Kind::Signed, note)?;
        if let HlReply::Error(e) = HlReply::parse(&hl_exchange(&action, hl.nonce, &signature).await?) {
            self.record("relay", &quote.request_id, Kind::Failed, &e)?;
            return fail(format!("Hyperliquid rejected the transfer: {e}."));
        }
        Ok(())
    }

    /// `usd` of `net`'s own coin for gas, out of the Hyperliquid balance over Relay: no gas anywhere is needed, so it brings
    /// gas where the dollars can't move by signature (BNB Chain, HyperEVM). Waits until the coin is there.
    pub(crate) async fn hyperliquid_gas(&self, net: &EvmNetwork, usd_amount: Decimal) -> Result<String> {
        let evm = self.wallet.evm.as_str();
        let usdc = rounded(usd_amount, 2, Rounding::Up);
        let units = units_of(usdc, 8);
        let free = Hyperliquid::balance(evm).await?.withdrawable;
        if usdc > free { return fail(format!("{} free on Hyperliquid, but gas for {} needs {}.", usd(free), net.name, usd(usdc))) }
        let quote = self.relay_quote(&relay::Ask {
            user: evm.into(), recipient: evm.into(), origin_chain: relay::chain::HYPERLIQUID, destination_chain: net.relay_chain,
            origin_currency: relay::currency::HYPERLIQUID_USDC.into(), destination_currency: "0x0000000000000000000000000000000000000000".into(),
            amount: units, ..Default::default()
        }).await?;
        let worth = decimal(&quote.amount_out_usd).unwrap_or_default();
        if worth <= Decimal::ZERO {
            return fail(format!("Relay doesn't quote {} on {} from Hyperliquid now. Nothing was signed.", net.gas_coin, net.name));
        }
        let before = net.rpc.balance(evm).await?;
        self.send_from_hyperliquid(&quote, usdc, units, evm, num(Num::of(usdc) - worth)?, &format!("Hyperliquid→{} {} {}", net.name, net.gas_coin, text(usdc)),
                                   &format!("Gas for {} from Hyperliquid: {} USDC → ≈ {} {} over Relay.", net.name, Fmt::num(usdc, 2),
                                            quote.amount_out_formatted, net.gas_coin)).await?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) {
            let status = relay::status(&quote.request_id).await?.0;
            if ["refund", "failure"].contains(&status.as_str()) {
                self.record("relay", &quote.request_id, Kind::Failed, &status)?;
                return fail(format!("Relay returned {status}: the USDC should come back to Hyperliquid."));
            }
            if status == "success" && net.rpc.balance(evm).await? > before {
                self.record("relay", &quote.request_id, Kind::Settled, &status)?;
                return Ok(format!("Gas from Hyperliquid on {} in {} s.", net.name, started.elapsed().as_secs()));
            }
            sleep(2).await;
        }
        fail(format!("Relay didn't bring the gas within 3 minutes. Order {} is in the journal.", quote.request_id))
    }

    /// A Relay withdrawal from Hyperliquid, sent: Relay's word until it lands on Solana.
    async fn withdrawal_landing(&self, request_id: &str, before: u64, started: Instant) -> Result<String> {
        while started.elapsed() < Duration::from_secs(180) {
            let status = relay::status(request_id).await?.0;
            if status == "success" {
                self.record("relay", request_id, Kind::Settled, &status)?;
                let after = self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC).await?;
                let line = format!("Withdrawal done in {} s: Solana USDC +{}.", started.elapsed().as_secs(),
                                   Fmt::num(Decimal::from_i128_with_scale(after as i128 - before as i128, 6), 6));
                self.report(&line);
                return Ok(line);
            }
            if ["refund", "failure"].contains(&status.as_str()) {
                self.record("relay", request_id, Kind::Failed, &status)?;
                return fail(format!("Relay returned {status}: the money should come back to Hyperliquid."));
            }
            sleep(1).await;
        }
        fail(format!("Relay didn't finish the withdrawal within 3 minutes. Order {request_id} is in the journal."))
    }

    // MARK: Jupiter Lend

    /// Jupiter Lend Earn, USDC vault. `usdc: None` on withdraw = the whole position. (Swift's `lend(usdc:deposit:)`.)
    pub async fn lend_usdc(&self, requested: Option<Decimal>, deposit: bool) -> Result<String> { self.lend("USDC", requested, deposit).await }

    /// Jupiter Lend Earn for any of its assets: USDC, SOL (the jlWSOL vault), USDT, JupUSD… `amount` in the asset;
    /// withdraw with None takes everything. After a full withdraw the empty share account is closed and its rent returns.
    pub async fn lend(&self, symbol: &str, requested: Option<Decimal>, deposit: bool) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let wanted = if symbol.to_uppercase() == "SOL" { "WSOL".to_string() } else { symbol.to_uppercase() };
        let jl = JupiterLend::default();
        let Some(vault) = jl.vaults().await?.into_iter().find(|v| v.asset_symbol.to_uppercase() == wanted) else {
            return fail(format!("Jupiter Lend has no {symbol} vault."));
        };
        let native = vault.asset_mint == WSOL;
        let name = if native { "SOL".to_string() } else { vault.asset_symbol.clone() };
        let shown = |u: u64| shift(u, vault.decimals);
        let my_asset = ata(sol, &vault.asset_mint)?;
        let my_shares = ata(sol, &vault.address)?;
        let owners = owners(&[(&my_asset, sol), (&my_shares, sol)]);
        let position = jl.positions(sol).await?.into_iter().find(|p| p.vault == vault.symbol);
        let in_vault = position.as_ref().map_or(0, |p| p.assets);
        let mut units = match requested {
            Some(r) => units_of(r, vault.decimals),
            None if deposit => return fail("How much should go into the vault?"),
            None => in_vault,
        };
        if deposit {
            let have = if native { self.rpc.balance(sol).await? } else { self.rpc.token_balance(sol, &vault.asset_mint).await? };
            let spendable = if native { have.saturating_sub(12_000_000) } else { have }; // SOL: keep fees and account rent
            if !(units > 0 && units <= spendable) {
                return fail(format!("You have {} {name}{}, and the deposit needs {}.", Fmt::num(shown(spendable), 6),
                                    if native { " (0.012 SOL is kept for fees and the account)" } else { "" }, Fmt::num(shown(units), 6)));
            }
            // The API's deposit expects our share account to exist; Jupiter's own site creates it first.
            let info = self.rpc.call("getAccountInfo", json!([my_shares, {"encoding": "base64"}])).await.ok().filter(Value::is_object);
            if info.and_then(|i| i.get("value").cloned()).is_some_and(|v| v.is_null()) {
                let create = Instruction {
                    program: spl::ASSOCIATED_TOKEN_PROGRAM.into(),
                    accounts: vec![(sol.into(), true, true), (my_shares.clone(), false, true), (sol.into(), false, false), (vault.address.clone(), false, false),
                                   (guard::sol::SYSTEM.into(), false, false), (spl::TOKEN_PROGRAM.into(), false, false)],
                    data: vec![1], // CreateIdempotent
                };
                let (hash, last_valid) = self.rpc.latest_blockhash().await?;
                let wire = solana::unsigned_transaction(&solana::compile_message(&[create], sol, &hash, 20_000)?);
                self.sign_and_send(&wire, last_valid, &owners, &[], &format!("{} share account", vault.symbol), -5_000_000, &HashMap::new(), false, None).await?;
            }
            if native {
                // Jupiter Lend takes wrapped SOL from our wSOL account; it does not wrap by itself.
                let mut transfer = vec![2, 0, 0, 0];
                transfer.extend(units.to_le_bytes());
                let wrap = [
                    Instruction {
                        program: spl::ASSOCIATED_TOKEN_PROGRAM.into(),
                        accounts: vec![(sol.into(), true, true), (my_asset.clone(), false, true), (sol.into(), false, false), (vault.asset_mint.clone(), false, false),
                                       (guard::sol::SYSTEM.into(), false, false), (spl::TOKEN_PROGRAM.into(), false, false)],
                        data: vec![1],
                    },
                    Instruction { program: guard::sol::SYSTEM.into(), accounts: vec![(sol.into(), true, true), (my_asset.clone(), false, true)], data: transfer },
                    Instruction { program: spl::TOKEN_PROGRAM.into(), accounts: vec![(my_asset.clone(), false, true)], data: vec![17] }, // SyncNative
                ];
                let (hash, last_valid) = self.rpc.latest_blockhash().await?;
                self.sign_and_send(&solana::unsigned_transaction(&solana::compile_message(&wrap, sol, &hash, 20_000)?), last_valid, &owners,
                                   &[(my_asset.clone(), "wSOL".into(), true, i(units))], "wrap SOL", (-i(units)).saturating_sub(8_000_000), &HashMap::new(), false, None).await?;
            }
        } else {
            units = units.min(in_vault);
            if units == 0 {
                // Nothing in the vault; an empty share account left from an earlier attempt still holds rent.
                let line = format!("The {} vault is empty.", vault.symbol)
                    + &self.close_if_empty(&my_shares, &vault.address, &format!("{} share account", vault.symbol)).await?.unwrap_or_default();
                self.report(&line);
                return Ok(line);
            }
        }
        // Everything out goes by shares: withdrawing the reported amount fails by a rounding unit.
        let all = !deposit && units >= in_vault;
        let wire = if deposit {
            jl.deposit_transaction(&vault.asset_mint, sol, units).await?
        } else if all {
            jl.redeem_transaction(&vault.asset_mint, sol, position.as_ref().map_or(0, |p| p.shares)).await?
        } else {
            jl.withdraw_transaction(&vault.asset_mint, sol, units).await?
        };
        let last_valid = self.rpc.latest_blockhash().await?.1;
        // SOL goes through our wSOL account (wrapped before the deposit, unwrapped after the withdraw), so the rule
        // is the same token rule for every asset.
        let before = self.rpc.token_balance(sol, &vault.asset_mint).await?;
        let mut rules: Vec<Rule> = if deposit { vec![(my_shares.clone(), format!("{} shares", vault.symbol), true, 1)] } else { vec![] };
        rules.push((my_asset.clone(), if native { "wSOL".into() } else { name.clone() }, true, if deposit { -i(units) } else { i(units) - if all { 2 } else { 1 } }));
        self.sign_and_send(&wire, last_valid, &owners, &rules, &format!("Jupiter Lend {} {name}", if deposit { "deposit" } else { "withdraw" }), -5_000_000,
                           &HashMap::new(), false, None).await?;
        let after = self.rpc.token_balance(sol, &vault.asset_mint).await?;
        let mut line = if deposit {
            format!("Deposited {} {name} into the {} vault ({}% APY).", Fmt::num(shown(units), 9), vault.symbol, Fmt::num(vault.apy, 2))
        } else {
            format!("Withdrawn from the {} vault: {name} +{}{}.", vault.symbol,
                    Fmt::num(shifted(Decimal::from(after as i128 - before as i128), vault.decimals).unwrap_or_default(), 9),
                    if native { " (net of fees)" } else { "" })
        };
        if !deposit {
            // Emptied the vault: the share account's rent comes back. For SOL, closing the wSOL account unwraps it.
            line += &self.close_if_empty(&my_shares, &vault.address, &format!("{} share account", vault.symbol)).await?.unwrap_or_default();
            if native { line += &self.close_wrapped_sol(&my_asset).await? }
        }
        self.report(&line);
        Ok(line)
    }

    // MARK: Jupiter DCA

    /// Jupiter's on-chain DCA: `usdc` into `asset` in `orders` swaps `every` seconds apart, the first right away. One
    /// signature moves the whole amount into an order account of the wallet's own; Jupiter's keepers swap, and the coin
    /// comes to the wallet after each. `dry_run`: built, guarded and simulated, not signed.
    pub async fn jupiter_dca(&self, asset: &SpotAsset, usdc: Decimal, orders: i64, every: i64, dry_run: bool) -> Result<String> {
        if let Some(problem) = WalletCommand::jupiter_dca_problem(usdc, orders) { return fail(problem) }
        let usdc_mint = SpotAsset::usdc().mint;
        if !(asset.resolved() && asset.mint != usdc_mint) { return fail("Which coin should the DCA buy?") }
        let sol = self.wallet.solana.as_str();
        let units = units_of(usdc, 6);
        let have = self.rpc.token_balance(sol, &usdc_mint).await?;
        // A dry run still shows what the guard says of Jupiter's transaction when the wallet is short.
        if !(units <= have || dry_run) {
            return fail(format!("You have {} USDC on Solana, but the DCA puts in {} at once.", Fmt::num(millionths(have), 2), Fmt::num(usdc, 2)));
        }
        let wire = recurring::create_order(sol, &usdc_mint, &asset.mint, units, orders, every).await?.1;
        let last_valid = self.rpc.latest_blockhash().await?.1;
        let my_usdc = ata(sol, &usdc_mint)?;
        let my_coin = ata(sol, &asset.mint)?;
        // Exactly the amount leaves the USDC account; SOL pays the fee and ≈ 0.005 SOL of rent the order returns at its end.
        let changes = self.sign_and_send(&wire, last_valid, &owners(&[(&my_usdc, sol), (&my_coin, sol)]), &[(my_usdc.clone(), "USDC".into(), true, -i(units))],
                                         &format!("Jupiter DCA {} USDC into {}", Fmt::num(usdc, 2), asset.symbol), -12_000_000, &HashMap::new(), dry_run, None).await?;
        if dry_run { return Ok(format!("Dry run, nothing signed: {}", changes.join("; "))) }
        let line = format!("Jupiter DCA set: {} USDC into {}, {orders} swaps of {} USDC every {}. The first runs now, and {} comes to the wallet after each.",
                           Fmt::num(usdc, 2), asset.symbol, Fmt::num(num(Num::of(usdc) / orders)?, 2), WalletCommand::every_text(every), asset.symbol);
        self.report(&line);
        Ok(line)
    }

    /// Closes the running Jupiter DCA orders (`asset` None = all of them): what they haven't spent comes back, with their rent.
    pub async fn cancel_jupiter_dca(&self, asset: Option<&SpotAsset>, dry_run: bool) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let open: Vec<recurring::Order> = self.jupiter_dca_orders().await?.into_iter().filter(|o| asset.is_none_or(|a| o.output_mint == a.mint)).collect();
        if open.is_empty() { return Ok(format!("No Jupiter DCA{} is running.", asset.map_or(String::new(), |a| format!(" into {}", a.symbol)))) }
        let mut notes = vec![];
        for order in &open {
            let wire = recurring::cancel_order(sol, &order.key).await?.1;
            let last_valid = self.rpc.latest_blockhash().await?.1;
            let my_in = ata(sol, &order.input_mint)?;
            let my_out = ata(sol, &order.output_mint)?;
            // Money only comes back: the input account can't go down, SOL only by the fee.
            let changes = self.sign_and_send(&wire, last_valid, &owners(&[(&my_in, sol), (&my_out, sol)]), &[(my_in.clone(), "unspent input".into(), true, 0)],
                                             &format!("cancel Jupiter DCA {}…", prefix(&order.key, 4)), -1_000_000, &HashMap::new(), dry_run, None).await?;
            if dry_run { notes.push(changes.join("; ")) }
        }
        if dry_run { return Ok(format!("Dry run, nothing signed: {}", notes.join(" | "))) }
        let back = open.iter().fold(Num::of(Decimal::ZERO), |sum, o| sum + (Num::of(o.deposited) - o.used).0.map(|left| left.max(Decimal::ZERO)).map_or(Num(None), Num::of));
        let line = format!("Cancelled {}: about {} of what they hadn't spent and their rent are back in the wallet.",
                           if open.len() == 1 { "the Jupiter DCA".to_string() } else { format!("{} Jupiter DCAs", open.len()) },
                           back.0.map_or("NaN".into(), |b| Fmt::num(b, 2)));
        self.report(&line);
        Ok(line)
    }

    /// The wallet's running Jupiter DCA orders, newest first.
    pub async fn jupiter_dca_orders(&self) -> Result<Vec<recurring::Order>> { Ok(recurring::orders(&self.wallet.solana).await?) }

    /// Whether `account` is there, by getAccountInfo's value; a node that fails says no.
    async fn account_exists(&self, account: &str) -> bool {
        self.rpc.call("getAccountInfo", json!([account, {"encoding": "base64"}])).await.ok().filter(Value::is_object)
            .is_some_and(|r| r.get("value").is_some_and(Value::is_object))
    }

    /// Closes our token account when it holds nothing; returns the line saying how much rent came back.
    async fn close_if_empty(&self, account: &str, mint: &str, label: &str) -> Result<Option<String>> {
        if !self.account_exists(account).await || self.rpc.token_balance(&self.wallet.solana, mint).await? != 0 { return Ok(None) }
        Ok(Some(self.close_account(account, label, 1_000_000).await?))
    }

    /// Closing the wSOL account turns the wrapped SOL and its rent back into ordinary SOL.
    async fn close_wrapped_sol(&self, account: &str) -> Result<String> {
        if !self.account_exists(account).await { return Ok(String::new()) }
        self.close_account(account, "wSOL account (SOL unwrapped)", 0).await
    }

    /// Swift's private `close(_:label:least:)` (the name `close` is the position's here).
    async fn close_account(&self, account: &str, label: &str, least: i64) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let instruction = Instruction {
            program: spl::TOKEN_PROGRAM.into(), accounts: vec![(account.into(), false, true), (sol.into(), false, true), (sol.into(), true, false)], data: vec![9],
        }; // CloseAccount
        let (hash, last_valid) = self.rpc.latest_blockhash().await?;
        let before = self.rpc.balance(sol).await?;
        self.sign_and_send(&solana::unsigned_transaction(&solana::compile_message(&[instruction], sol, &hash, 20_000)?), last_valid, &owners(&[(account, sol)]),
                           &[], &format!("close {label}"), least, &HashMap::new(), false, None).await?;
        let after = self.rpc.balance(sol).await?;
        Ok(format!(" Closed {label}: +{} SOL.", Fmt::num(sol_of(after as i128 - before as i128), 6)))
    }

    // MARK: Swaps on Solana (Jupiter)

    /// Buy (from USDC) or sell (for USDC); kept for the chat commands.
    pub async fn swap(&self, asset: &SpotAsset, buy: bool, requested: Option<Decimal>, equity: Option<Decimal>, shown: Option<u64>) -> Result<String> {
        let usdc = SpotAsset::usdc();
        let (from, to) = if buy { (&usdc, asset) } else { (asset, &usdc) };
        self.convert(from, to, requested, equity, false, shown).await
    }

    /// Any verified token into any other through Jupiter: buying and selling, and staking SOL into JitoSOL and back.
    /// The simulated balances must show at least Jupiter's minimum output arriving in our own account. `amount` in the
    /// input token; None = all of it (SOL keeps 0.01 for fees). Spending USDC on one coin obeys the concentration rule.
    /// `shown`: the output the card showed, in the token's units — the swap doesn't go for more than 1% less.
    pub async fn convert(&self, from: &SpotAsset, to: &SpotAsset, requested: Option<Decimal>, equity: Option<Decimal>, dry_run: bool,
                         shown: Option<u64>) -> Result<String> {
        if from == to { return fail("That's the same token.") }
        let sol = self.wallet.solana.as_str();
        let usdc = SpotAsset::usdc();
        let (native_in, native_out) = (from.symbol == "SOL", to.symbol == "SOL");
        let (in_shift, out_shift) = (|u: u64| shift(u, from.decimals), |u: u64| shift(u, to.decimals));
        // SOL bought where there's none for fees is gas, not a bet: the concentration rule doesn't count it.
        let lamports = self.rpc.balance(sol).await?;
        let buying_gas = *from == usdc && to.symbol == "SOL" && lamports < TopUpOrder::SOLANA_FEE_FLOOR;
        if *from == usdc && !buying_gas && let Some(equity) = equity.filter(|e| *e > Decimal::ZERO) {
            let share = num(Num::of(requested.unwrap_or_default()) / equity * 100)?;
            if share > Decimal::from(50) {
                return fail(format!("That's {} of your capital in one coin. The rule: no more than half at once — buy in parts, e.g. {} a week.",
                                    Fmt::pct(rounded(share, 0, Rounding::Plain)), usd(rounded(equity / Decimal::TEN, 2, Rounding::Down))));
            }
            if share > Decimal::from(25) {
                self.report(&format!("That's {} of your capital in {}: big buys are usually split over time (DCA).", Fmt::pct(rounded(share, 0, Rounding::Plain)),
                                     to.symbol));
            }
        }
        let have = if native_in { self.rpc.balance(sol).await? } else { self.rpc.token_balance(sol, &from.mint).await? };
        let spendable = if native_in { have.saturating_sub(10_000_000) } else { have };
        let units = requested.map_or(spendable, |r| units_of(r, from.decimals));
        if !(units > 0 && units <= spendable) {
            return fail(format!("You have {} {}{}, and the swap needs {}.", Fmt::num(in_shift(spendable), 6), from.symbol,
                                if native_in { " (0.01 SOL is kept for fees)" } else { "" }, Fmt::num(in_shift(units), 6)));
        }
        let program = |a: &SpotAsset| if a.token2022 { spl::TOKEN_2022_PROGRAM } else { spl::TOKEN_PROGRAM };
        let in_account = spl::associated_account(sol, &from.mint, program(from))?;
        let out_account = spl::associated_account(sol, &to.mint, program(to))?;
        let owners = owners(&[(&in_account, sol), (&out_account, sol)]); // wSOL accounts too, for the native side
        let out_before = if native_out { self.rpc.balance(sol).await? } else { self.rpc.token_balance(sol, &to.mint).await? };
        let mut changes: Vec<String> = vec![];
        // The price moved more than 1% (and Jupiter's own fee, when its wallet pays) under what the card showed: not signed.
        let holds_shown = |out: u64, fee_bps: Decimal| -> Result<()> {
            let Some(shown) = shown else { return Ok(()) };
            let floor = Num::of(Decimal::from(shown)) * (Num::of(Decimal::ONE) - (Num::of(Decimal::ONE_HUNDRED) + fee_bps) / 10_000);
            if !floor.0.is_some_and(|f| Decimal::from(out) < f) { return Ok(()) }
            fail(format!("Jupiter now gives {} {}, more than 1% under the {} the card showed. Nothing was signed. Refresh shows the new price.",
                         Fmt::num(out_shift(out), 8), to.symbol, Fmt::num(out_shift(shown), 8)))
        };
        // The swap moves the price past impactCap: asked, not signed. What goes in, in USD: the USDC itself, else what comes
        // out (Jupiter's own price for it) before the impact.
        let holds_impact = |impact: Decimal, worth_usd: Decimal, out: u64| -> Result<()> {
            let worth = if *to == usdc { millionths(out) } else { worth_usd };
            let paid = if *from == usdc { millionths(units) } else if impact < Decimal::ONE { num(Num::of(worth) / (Decimal::ONE - impact))? } else { Decimal::ZERO };
            Self::check_impact(impact, paid, in_shift(units), &from.symbol, "Jupiter", None)
        };
        // Jupiter's /order, where someone else pays the fee and signs first, our wallet second: JupiterZ's market makers when
        // their price beats the pools' (/quote) — big swaps mostly — and, under the fee floor (USDC came without SOL),
        // Jupiter's own gasless swap, which Jupiter grants a wallet that can't pay from $5.
        let low_sol = !native_in && lamports < TopUpOrder::SOLANA_FEE_FLOOR;
        // Jupiter's other routers left out, so its market makers answer a small size: the Dev copy's JupiterZ run, and SOL
        // for fees bought under Jupiter's $5, which only a maker pays the fee for.
        let makers_only = JUPITER_Z_FIRST.load(Relaxed) || buying_gas && units < 5_000_000;
        let routers: Vec<String> = if makers_only { ["metis", "iris", "dflow", "okx"].map(String::from).to_vec() } else { vec![] };
        let order = jupiter::order(&from.mint, &to.mint, units, sol, &routers).await.ok();
        let payer = order.as_ref().filter(|o| o.gasless).and_then(|o| Message::parse_transaction(&o.wire).ok()).and_then(|m| m.static_keys.first().cloned())
            .filter(|p| p != sol);
        let mut sponsored: Option<(jupiter::Order, String)> = None;
        if let (Some(order), Some(payer)) = (&order, &payer) && Self::sponsorable(&order.router, &order.wire, &from.mint, &to.mint) {
            if low_sol {
                sponsored = Some((order.clone(), payer.clone()));
            } else if order.router == "jupiterz"
                && let Ok(pools) = jupiter::quote(&from.mint, &to.mint, units, 50, &[]).await
                && (order.out_amount > pools.out_amount || JUPITER_Z_FIRST.load(Relaxed)) {
                sponsored = Some((order.clone(), payer.clone()));
            }
        }
        // Not even the rent of an account the swap opens (the temporary wSOL one, or the coin's own: a 165-byte token account,
        // its minimum read from the network) and a fee, and Jupiter won't pay: said before a simulation fails on it.
        if sponsored.is_none() && !native_in && (native_out || out_before == 0) && lamports < 2_100_000
            && lamports < self.rpc.rent_minimum(165).await.ok().map_or(2_100_000, |r| r + 60_000) {
            return fail(format!("There's {} SOL for the network fee, too little for this swap, and neither a market maker nor Jupiter (from $5) took its fee. \
                                 Swap more, or add ≈ 0.01 SOL first.", Fmt::num(sol_of(lamports as i128), 6)));
        }
        if let Some((order, sponsor)) = sponsored.clone() {
            let maker = order.router == "jupiterz";
            holds_shown(order.out_amount, Decimal::from(order.fee_bps))?;
            holds_impact(order.impact, order.usd, order.out_amount)?;
            self.report(&format!("{} → {} via {}: {} → ≈ {}, at least {} ({}% fee). Route: {}.", from.symbol, to.symbol,
                                 if maker { "JupiterZ, a market maker filling it and paying the fee" } else { "Jupiter, its wallet paying the fee" },
                                 Fmt::num(in_shift(units), 8), Fmt::num(out_shift(order.out_amount), 8), Fmt::num(out_shift(order.min_out), 8),
                                 Fmt::num(Decimal::from(order.fee_bps) / Decimal::ONE_HUNDRED, 2), if order.route.is_empty() { &order.router } else { &order.route }));
            let mut rules: Vec<Rule> = if native_in { vec![] } else { vec![(in_account.clone(), from.symbol.clone(), true, -i(units))] };
            if !native_out { rules.push((out_account.clone(), format!("{} (quote minimum)", to.symbol), true, i(order.min_out))) }
            // Not a lamport of ours pays: SOL in leaves exactly, SOL bought arrives whole, at least the minimum. A maker leaves
            // the rent of the coin's new account to the wallet: only when that account doesn't exist yet, and the guard lets
            // no SOL of the wallet's go to the maker.
            let out_new = maker && !native_out && self.rpc.balance(&out_account).await? == 0;
            let rent: i64 = if out_new { 2_100_000 } else { 0 };
            let least = if native_in { (-i(units)).saturating_sub(rent) } else if native_out { i(order.min_out) } else { -rent };
            let note = format!("Jupiter {}→{}, fee paid by {}", from.symbol, to.symbol, if maker { "JupiterZ's market maker" } else { "Jupiter" });
            match self.sign_and_send(&order.wire, order.last_valid_block_height, &owners, &rules, &note, least, &HashMap::new(), dry_run,
                                     Some((&sponsor, &order.request_id))).await {
                Ok(c) => changes = c,
                // Refused by the app's own checks before the signer was asked (the guard, or a simulation off the quote): the
                // pools' route below instead. A «no» at the signer's prompt stops here, never asks again for another route.
                Err(e) if maker && !low_sol && (matches!(&e, Error::Guard(m) if m.starts_with("The guard stopped"))
                                                 || matches!(&e, Error::Execution(m) if m.starts_with("The simulation failed"))) => {
                    self.report(&format!("JupiterZ's fill didn't pass the checks, nothing was sent: {e} Taking the pools' route."));
                    sponsored = None;
                }
                Err(e) => return Err(e),
            }
        }
        if sponsored.is_none() {
            // Jupiter's route can fail before anything is signed: its transaction is over Solana's 1232 bytes, or the
            // simulation ends under its own minimum — 6001, SlippageToleranceExceeded, when a pool quoted more than it
            // gives. Up to three routes, each without the failed ones' pools.
            let mut excluded: Vec<String> = vec![];
            let mut first_failure: Option<Error> = None;
            for attempt in 1..=3 {
                let quote = match jupiter::quote(&from.mint, &to.mint, units, 50, &excluded).await {
                    Ok(q) => q,
                    Err(e) => return Err(first_failure.unwrap_or(e.into())),
                };
                holds_shown(quote.out_amount, Decimal::ZERO)?;
                holds_impact(quote.impact, quote.usd, quote.out_amount)?;
                self.report(&format!("{} → {} via Jupiter: {} → ≈ {}, at least {}. Route: {}.", from.symbol, to.symbol, Fmt::num(in_shift(units), 8),
                                     Fmt::num(out_shift(quote.out_amount), 8), Fmt::num(out_shift(quote.min_out), 8), quote.route));
                let (wire, last_valid) = jupiter::swap_transaction(&quote, sol).await?;
                let mut rules: Vec<Rule> = vec![];
                if !native_in { rules.push((in_account.clone(), from.symbol.clone(), true, -i(units))) }
                if !native_out { rules.push((out_account.clone(), format!("{} (quote minimum)", to.symbol), true, i(quote.min_out))) }
                let fee: i64 = 5_000_000;
                // Up to ~0.0021 SOL more when the output token's account is created (rent, returned when it is closed).
                let least = if native_in { (-i(units)).saturating_sub(fee) } else if native_out { i(quote.min_out) - fee } else { -fee };
                let (failure, why) = if wire.len() > 1232 {
                    let why = format!("is {} bytes, over Solana's limit of 1232", wire.len());
                    (Error::Execution(format!("Jupiter's route {} {why}, so nothing was signed.", quote.route)), why)
                } else {
                    match self.sign_and_send(&wire, last_valid, &owners, &rules, &format!("Jupiter {}→{}", from.symbol, to.symbol), least, &HashMap::new(),
                                             dry_run, None).await {
                        Ok(c) => {
                            changes = c;
                            break;
                        }
                        Err(Error::Execution(m)) if m.starts_with("The simulation failed") && m.contains(r#"{"Custom":6001}"#) => {
                            (Error::Execution(m), "would give less than its minimum (Jupiter's 6001)".to_string())
                        }
                        Err(e) => return Err(e),
                    }
                };
                if attempt >= 3 { return Err(failure) }
                first_failure.get_or_insert(failure);
                let new: Vec<String> = quote.dexes.iter().filter(|x| !excluded.contains(x)).cloned().collect();
                excluded.extend(new);
                self.report(&format!("Route {} {why}, so nothing was signed. Asking Jupiter for a route without {}.", quote.route, excluded.join(", ")));
            }
        }
        if dry_run { return Ok(format!("Simulation (nothing signed): {}", changes.join("; "))) }
        if SWAP_TAIL_TEST.load(Relaxed) { return Err(Error::Wallet("Solana RPC getTokenAccountsByOwner: HTTP 429 (swapTailTest)".into())) }
        let out_after = if native_out { self.rpc.balance(sol).await? } else { self.rpc.token_balance(sol, &to.mint).await? };
        let got = shifted(Decimal::from(out_after as i128 - out_before as i128), to.decimals).unwrap_or_default();
        let mut line = format!("Swapped {} {} → {} +{}{}.", Fmt::num(in_shift(units), 8), from.symbol, to.symbol, Fmt::num(got, 8),
                               if native_out { " (net of fees)" } else { "" });
        if let Some(swapped) = &self.swapped { swapped(from, to, in_shift(units), got) }
        // Out of a token entirely: its empty account still holds ~0.002 SOL of rent (≈ 0.00156 an Ondo stock's Token-2022
        // one); closing returns it, under the token's own program. The swap went through either way: a failed close says so.
        if !native_in && self.rpc.token_balance(sol, &from.mint).await? == 0 {
            let close = Instruction {
                program: program(from).into(), accounts: vec![(in_account.clone(), false, true), (sol.into(), false, true), (sol.into(), true, false)], data: vec![9],
            }; // CloseAccount
            let closed: Result<String> = async {
                let (hash, last_valid) = self.rpc.latest_blockhash().await?;
                let before = self.rpc.balance(sol).await?;
                self.sign_and_send(&solana::unsigned_transaction(&solana::compile_message(&[close], sol, &hash, 20_000)?), last_valid,
                                   &BTreeMap::from([(in_account.clone(), sol.to_string())]), &[], &format!("close empty {} account", from.symbol), 1_000_000,
                                   &HashMap::new(), false, None).await?;
                let after = self.rpc.balance(sol).await?;
                Ok(format!(" Empty {} account closed: +{} SOL of rent returned.", from.symbol, Fmt::num(sol_of(after as i128 - before as i128), 6)))
            }.await;
            line += &match closed {
                Ok(said) => said,
                Err(e) => format!(" Its empty {} account stays open, with its rent (closing it failed: {e}).", from.symbol),
            };
        }
        self.report(&line);
        Ok(line)
    }

    /// Whether a gasless order may be taken: a JupiterZ fill only when it swaps exactly what was asked — on the low-SOL path
    /// too, since the guard takes the wallet's account for any mint and the simulation watches only the asked two. Jupiter's
    /// own routes are read by the guard.
    pub fn sponsorable(router: &str, wire: &[u8], from: &str, to: &str) -> bool { router != "jupiterz" || Self::fill_swaps(wire, from, to) }

    /// A JupiterZ order's one fill swaps exactly these mints: its input mint (account 6) and output mint (account 8), read
    /// from the transaction's own keys. A mint a lookup table names is proven by the taker's own account for it instead
    /// (input 2, output 4): a token account's address derives from its mint.
    pub fn fill_swaps(wire: &[u8], from: &str, to: &str) -> bool {
        let Ok(m) = Message::parse_transaction(wire) else { return false };
        let fills: Vec<&solana::message::Instruction> =
            m.instructions.iter().filter(|ix| ix.program_index < m.static_keys.len() && m.static_keys[ix.program_index] == guard::sol::JUPITER_Z).collect();
        let [ix] = fills.as_slice() else { return false };
        if ix.accounts.len() <= 8 { return false }
        let key = |i: usize| m.static_keys.get(ix.accounts[i]);
        let swaps = |at: usize, account: usize, mint: &str| -> bool {
            if let Some(named) = key(at) { return named == mint }
            let (Some(taker), Some(own)) = (key(0), key(account)) else { return false };
            [spl::TOKEN_PROGRAM, spl::TOKEN_2022_PROGRAM].iter().any(|p| spl::associated_account(taker, mint, p).ok().as_ref() == Some(own))
        };
        swaps(6, 2, from) && swaps(8, 4, to)
    }

    // MARK: Signing a Solana move

    /// Guard and simulation here, the signer's own guard, then send and wait. `rules`: (account, label, token?, least change).
    /// Returns the simulated change per watched balance; `dry_run` stops before signing. `sponsor`: (Jupiter's wallet,
    /// its order id) — it pays the fee and signs first, and the transaction goes back through Jupiter's /execute. `at_most`:
    /// the most a watched account (the wallet itself for its SOL) may grow by, for a move that brings money in.
    /// Swift's defaults: `lamports_at_least` −5_000_000, `at_most` empty, `dry_run` false, `sponsor` None.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn sign_and_send(&self, unsigned: &[u8], last_valid: u64, owners: &BTreeMap<String, String>, rules: &[Rule], note: &str,
                                      lamports_at_least: i64, at_most: &HashMap<String, i64>, dry_run: bool, sponsor: Option<(&str, &str)>)
        -> Result<Vec<String>> {
        let sol = self.wallet.solana.as_str();
        let message = Message::parse_transaction(unsigned)?;
        let accounts = self.rpc.resolve_accounts(&message).await?;
        let token_owners: HashMap<String, String> = owners.clone().into_iter().collect();
        let policy = Policy { sponsor: sponsor.map(|s| s.0.to_string()), ..Policy::new(sol) };
        let mut verdict = guard::sol::check(&message, &accounts, &policy, &token_owners);
        // A program or an account the guard refuses stops it before the simulation: that answer needs no balances.
        if !verdict.ok() { return stop(format!("The guard stopped {note}: {}", verdict.problems.join(" "))) }
        let mut watch: Vec<String> = rules.iter().map(|r| r.0.clone()).collect();
        watch.push(sol.into());
        // Every balance before in one read. A node that fails stops the move: a made-up 0 would pass a route that takes
        // more than approved or pays its output to someone else. An account that isn't there yet holds 0 (the node's null).
        let now = match self.rpc.accounts(&watch).await {
            Ok(now) => now,
            Err(e) => return fail(format!("Couldn't read the balances before {note}, so nothing was signed: {}", e.0)),
        };
        let mut spec: Vec<(String, String, bool)> = rules.iter().map(|r| (r.0.clone(), r.1.clone(), r.2)).collect();
        spec.push((sol.into(), "lamports".into(), false));
        let before = match solana::message::before(&spec, &now) {
            Ok(b) => b,
            Err(e) => return stop(format!("The guard stopped {note}: {}", e.0)),
        };
        let simulation = self.rpc.simulate(unsigned, &watch, false).await?;
        // A failed simulation says nothing about balances: stop here rather than quote made-up changes.
        if let Some(error) = &simulation.error { return fail(format!("The simulation failed, so nothing was signed: {error}")) }
        let after = |account: &str, token: bool| -> i64 {
            i(simulation.after.get(account).map_or(0, |s| if token { s.1.unwrap_or(0) } else { s.0 }))
        };
        let was = |account: &str| before.get(account).copied().unwrap_or(0);
        let mut balance_rules: Vec<BalanceRule> = rules.iter().map(|(account, label, token, least)| BalanceRule {
            label: label.clone(), before: was(account), after: after(account, *token), at_least: *least, at_most: at_most.get(account).copied(),
        }).collect();
        balance_rules.push(BalanceRule { label: "lamports".into(), before: was(sol), after: after(sol, false), at_least: lamports_at_least, at_most: at_most.get(sol).copied() });
        verdict.merge(guard::sol::balances(&balance_rules));
        if !verdict.ok() { return stop(format!("The guard stopped {note}: {}", verdict.problems.join(" "))) }
        let mut changes: Vec<String> = rules.iter().map(|(account, label, token, _)| {
            format!("{label}: {} (raw units)", after(account, *token) as i128 - was(account) as i128)
        }).collect();
        let sol_delta = sol_of(after(sol, false) as i128 - was(sol) as i128);
        changes.push(format!("SOL: {} ({})", Fmt::num(sol_delta, 6),
                             if sol_delta < Decimal::ZERO { "fee and rent for new accounts" } else { "rent back from closed accounts" }));
        if dry_run { return Ok(changes) }
        if let Some((sponsor, request_id)) = sponsor {
            // Only our slot, the second signature, may differ from what was checked.
            let signed = self.signer.sign_solana_sponsored(unsigned.to_vec(), accounts, owners.clone(), sponsor.into()).await?;
            if signed.len() != unsigned.len() || signed.len() < 129 || signed[..65] != unsigned[..65] || signed[129..] != unsigned[129..] {
                return stop("What was signed isn't what was checked.");
            }
            let ours = base58::encode(&signed[65..129]);
            self.record("solana", &ours, Kind::Signed, &format!("{note}; Jupiter order {request_id}, valid until block {last_valid}"))?;
            match jupiter::execute(&signed, request_id).await {
                Ok(landed) => self.record("solana", &ours, Kind::Confirmed, &format!("transaction {landed}"))?,
                // No answer, or Jupiter's «Failed», isn't the chain's: an ask before it may have sent it. Only the chain says.
                Err(e) => self.settle_sponsored(&ours, &message.blockhash, simulation.slot, &e.0, Duration::from_secs(120)).await?,
            }
            return Ok(changes);
        }
        let signed = self.signer.sign_solana(unsigned.to_vec(), accounts, vec![], owners.clone()).await?;
        if signed.len() != unsigned.len() || signed.len() < 65 || signed[65..] != unsigned[65..] { return stop("What was signed isn't what was checked.") }
        let signature = base58::encode(&signed[1..65]);
        self.record("solana", &signature, Kind::Signed, &format!("{note}; valid until block {last_valid}"))?;
        match self.broadcast_solana(&signed, &signature, last_valid).await? {
            Outcome::Confirmed => self.record("solana", &signature, Kind::Confirmed, "")?,
            Outcome::Failed(e) => {
                self.record("solana", &signature, Kind::Failed, &e)?;
                return fail(format!("The transaction failed: {e}."));
            }
            Outcome::Expired => {
                self.record("solana", &signature, Kind::Expired, "")?;
                return fail("The transaction didn't make it into a block. You can retry.");
            }
        }
        Ok(changes)
    }

    /// A swap with no gas of ours that Jupiter's /execute didn't confirm (`jupiter`: what it said, a timeout after its
    /// retries): journaled as the chain has it (wait_for_cosigned). Found: confirmed, the swap goes on. Failed there, or
    /// past use without landing: said, so it may be tried again. The chain silent: it stays «signed», so the chat says
    /// «sent, status unknown» with no «Try again». None of these errors starts as the guard's or the simulation's, after
    /// which the swap takes the pools' route: a fill that went out would be swapped twice. Swift's `timeout` is 120 s.
    pub(crate) async fn settle_sponsored(&self, ours: &str, blockhash: &str, slot: u64, jupiter: &str, timeout: Duration) -> Result<()> {
        let sol = self.wallet.solana.as_str();
        self.report(&format!("Jupiter didn't confirm the swap ({jupiter}). Looking for it on chain before saying how it went, up to {} s.", timeout.as_secs()));
        let Ok((outcome, transaction)) = self.rpc.wait_for_cosigned(ours, sol, blockhash, slot, timeout).await else {
            return fail(format!("Jupiter didn't say whether it sent the swap ({jupiter}), and the chain doesn't show it yet. \
                                 Check https://solscan.io/account/{sol} before swapping again."));
        };
        match outcome {
            Outcome::Confirmed => {
                let tx = transaction.unwrap_or_else(|| ours.into());
                self.record("solana", ours, Kind::Confirmed, &format!("transaction {tx}"))?;
                self.report(&format!("Found on chain: transaction {tx} went through."));
                Ok(())
            }
            Outcome::Failed(e) => {
                self.record("solana", ours, Kind::Failed, &e)?;
                fail(format!("The swap reached the chain and failed: {e}."))
            }
            Outcome::Expired => {
                self.record("solana", ours, Kind::Expired, jupiter)?;
                fail(format!("Jupiter didn't send the swap ({jupiter}), and now it can't land: nothing moved, you can retry."))
            }
        }
    }

    // MARK: NEAR through 1Click

    /// Where «buy NEAR» takes its USDC — the one rule the card shows and the executor signs: Solana when it holds the
    /// amount, else the first EVM network 1Click takes USDC from (Base, Arbitrum, Ethereum — Ethereum last) that holds it,
    /// straight into 1Click. None: Solana.
    pub fn near_source(usdc: Decimal, on_solana: Decimal, held: impl Fn(&str) -> Option<Decimal>) -> Option<&'static EvmNetwork> {
        if on_solana >= usdc { return None }
        EvmNetwork::searched().into_iter().find(|n| asset::usdc_on_evm(n.key).is_some() && held(n.key).unwrap_or_default() >= usdc)
    }

    /// Native NEAR for USDC through NEAR Intents 1Click, delivered to `account` — the wallet's own NEAR implicit account.
    /// The USDC is Solana's, or an EVM network's when Solana is short (`near_source`), sent to 1Click's deposit address
    /// there; a refund goes back to the wallet on the same network. The live quote must stay within 1% of the first one,
    /// and 1Click itself must have the deposit address bound to our recipient, refund address, asset and amount.
    pub async fn buy_near(&self, usdc: Decimal, account: &str) -> Result<String> {
        let (sol, evm) = (self.wallet.solana.as_str(), self.wallet.evm.as_str());
        let units = units_of(usdc, 6);
        if usdc < Decimal::ONE { return fail("Buy NEAR from 1 USDC: below that 1Click's fee eats too much.") }
        if !implicit(account) { return fail("That is not a NEAR implicit account.") }
        let on_solana = millionths(self.rpc.token_balance(sol, relay::currency::SOLANA_USDC).await?);
        let idle: HashMap<&str, Decimal> = self.idle_usdc().await.into_iter().map(|(n, u)| (n.key, u)).collect();
        let src = Self::near_source(usdc, on_solana, |k| idle.get(k).copied());
        let origin = src.and_then(|n| asset::usdc_on_evm(n.key)).unwrap_or(asset::SOLANA_USDC);
        let refund_to = if src.is_none() { sol } else { evm };
        let amount = units.to_string();
        let shown = one_click::quote(&one_click::Ask::new(true, origin, asset::NEAR, &amount, refund_to, account)).await?;
        let mut envelope = Envelope::approve_default(usdc, decimal(&shown.amount_out_formatted).unwrap_or_default(), Utc::now());
        let quote = one_click::quote(&one_click::Ask::new(false, origin, asset::NEAR, &amount, refund_to, account)).await?;
        let Some(deposit) = quote.deposit_address.clone() else { return fail("1Click gave no deposit address.") };
        envelope.destination = Some(deposit.clone());
        let problems = envelope.violations(decimal(&quote.amount_in_formatted).unwrap_or(Decimal::MAX), decimal(&quote.amount_out_formatted).unwrap_or_default(),
                                           Some(&deposit), Utc::now());
        if !problems.is_empty() { return fail(format!("The price moved past what was approved: {}", problems.join(" "))) }
        let binding = guard::deposit_check(&one_click::bound_request(&deposit).await?, account, refund_to, origin, asset::NEAR, &amount);
        if !binding.ok() { return stop(format!("The guard stopped the deposit: {}", binding.problems.join(" "))) }
        let from = src.map_or("Solana", |n| n.name);
        self.report(&format!("NEAR Intents: {} USDC from {from} → ≈ {} NEAR to your NEAR account.", Fmt::num(usdc, 2),
                             Fmt::num(decimal(&quote.amount_out_formatted).unwrap_or_default(), 4)));
        let tx = if let Some(src) = src {
            // Gas there first; the USDC goes to the deposit address, which the signer asks to allow in person.
            self.ensure_evm(src, units, None).await?;
            self.send_evm(&src.rpc, src.usdc, &erc20::transfer(&deposit, U256::from(units))?, &format!("Send {} USDC to the 1Click deposit", Fmt::num(usdc, 2)),
                          std::slice::from_ref(&deposit), None, 0).await?
        } else {
            self.send_solana_usdc(units, &deposit, "the 1Click deposit").await?.split('/').rfind(|s| !s.is_empty()).unwrap_or("").to_string()
        };
        self.record("1click", &deposit, Kind::Signed, &format!("USDC→NEAR {} from {from} to {account}", Fmt::num(usdc, 2)))?;
        if !tx.is_empty() { let _ = one_click::submit_deposit(&tx, &deposit).await; }
        let started = Instant::now();
        let s = self.one_click_settled(&deposit, &format!("USDC went back to {from}"), quote.time_estimate).await?;
        let line = format!("Bought {} NEAR for {} USDC from {from} in {} s — native NEAR on your NEAR account.",
                           Fmt::num(decimal(s.amount_out_formatted.as_deref().unwrap_or("")).unwrap_or_default(), 4), Fmt::num(usdc, 2), started.elapsed().as_secs());
        self.report(&line);
        Ok(line)
    }

    /// What each of 1Click's states means, said on the running card as it changes.
    pub const ONE_CLICK_WORDS: [(&str, &str); 7] = [
        ("PENDING_DEPOSIT", "waiting for the deposit"), ("KNOWN_DEPOSIT_TX", "deposit sent, waiting for it to land"),
        ("INCOMPLETE_DEPOSIT", "the deposit came short: 1Click refunds it"), ("PROCESSING", "deposit received, swapping"),
        ("SUCCESS", "done"), ("REFUNDED", "refunded"), ("FAILED", "failed"),
    ];

    /// `ONE_CLICK_WORDS[status]`.
    pub fn one_click_word(status: &str) -> Option<&'static str> { Self::ONE_CLICK_WORDS.iter().find(|(k, _)| *k == status).map(|(_, w)| *w) }

    /// 1Click's progress on `deposit` until it settles, each new state a line on the running card («1Click · deposit
    /// received, swapping · 12 s of ≈ 30»), written to the journal at the end. Fails on a refund or a failure, saying
    /// what went back (`refund`), and after 10 minutes without an end.
    pub(crate) async fn one_click_settled(&self, deposit: &str, refund: &str, estimate: Option<i64>) -> Result<one_click::Status> {
        let started = Instant::now();
        let mut last = String::new();
        while started.elapsed() < Duration::from_secs(600) {
            let s = one_click::status(deposit).await?;
            if s.status != last {
                last = s.status.clone();
                let took = started.elapsed().as_secs();
                let word = Self::one_click_word(&s.status).map_or_else(|| s.status.to_lowercase(), str::to_string);
                self.report(&format!("1Click · {word} · {took} s{}", estimate.map_or(String::new(), |e| format!(" of ≈ {e}"))));
            }
            if s.is_final() {
                self.record("1click", deposit, if s.status == "SUCCESS" { Kind::Settled } else { Kind::Failed }, &s.status)?;
                if s.status != "SUCCESS" {
                    return fail(format!("1Click {}: {}.", s.status.to_lowercase(),
                                        s.refunded_formatted.as_ref().map_or_else(|| "see the journal".to_string(), |r| format!("{r} {refund}"))));
                }
                return Ok(s);
            }
            sleep(3).await;
        }
        fail(format!("1Click hasn't finished within 10 minutes. The deposit {deposit} is in the journal. It settles or refunds on its own."))
    }

    /// Native NEAR from the wallet's NEAR account → USDC through NEAR Intents 1Click (the reverse of `buy_near`), on our
    /// Solana address or, with `net`, at our own address on that EVM network (Base, Arbitrum, Ethereum). None = all the
    /// account can give: its storage stays, and so does what the transaction may need. The live quote must stay within 1%
    /// of the one shown; 1Click must have the deposit address bound to our address there as recipient and our NEAR account
    /// for refunds; the signer builds the wrap.near transaction itself from the amount and the deposit address.
    pub async fn sell_near(&self, requested: Option<Decimal>, account: &str, net: Option<&EvmNetwork>) -> Result<String> {
        if !implicit(account) { return fail("That is not a NEAR implicit account.") }
        let destination = match net {
            Some(n) => match asset::usdc_on_evm(n.key) {
                Some(a) => a,
                None => return fail(format!("1Click pays NEAR's USDC on Solana, Base, Arbitrum and Ethereum, not on {}.", n.name)),
            },
            None => asset::SOLANA_USDC,
        };
        let recipient = if net.is_none() { self.wallet.solana.clone() } else { self.wallet.evm.clone() };
        let place = net.map_or_else(|| "your Solana address".to_string(), |n| format!("your address on {}", n.name));
        let context = Near::tx_context(account).await?;
        let micro: u128 = 1_000_000_000_000_000_000; // 10⁻⁶ NEAR: amounts are whole micro-NEAR
        let storage = context.storage_bytes as u128 * 10_000_000_000_000_000_000; // 10¹⁹ yocto a byte
        let balance: u128 = context.balance.parse().unwrap_or(0);
        let overhead = wallet::near::intent_deposit_overhead(context.gas_price.parse().unwrap_or(100_000_000));
        let Some(keep) = storage.checked_add(overhead) else { return fail(OUT_OF_RANGE) };
        let free = if balance > keep { (balance - keep) / micro * micro } else { 0 };
        let yocto = Self::yocto(requested, micro, free)?;
        if yocto < 50_000 * micro { return fail("Sell NEAR from 0.05: below that 1Click's fee eats too much.") }
        if yocto > free {
            return fail(format!("{} NEAR can be sold: the account keeps {} for its storage and the transaction.", wallet::near::near(free), wallet::near::near(keep)));
        }
        let near = decimal(&wallet::near::near(yocto)).unwrap_or_default();
        let units = yocto.to_string();
        let shown = one_click::quote(&one_click::Ask::new(true, asset::NEAR, destination, &units, account, &recipient)).await?;
        let mut envelope = Envelope::approve_default(near, decimal(&shown.amount_out_formatted).unwrap_or_default(), Utc::now());
        let quote = one_click::quote(&one_click::Ask::new(false, asset::NEAR, destination, &units, account, &recipient)).await?;
        let Some(deposit) = quote.deposit_address.clone().filter(|d| implicit(d)) else { return fail("1Click gave no NEAR deposit account.") };
        envelope.destination = Some(deposit.clone());
        let out = decimal(&quote.amount_out_formatted).unwrap_or_default();
        let mut problems = envelope.violations(decimal(&quote.amount_in_formatted).unwrap_or(Decimal::MAX), out, Some(&deposit), Utc::now());
        if quote.deadline.as_deref().and_then(iso8601_fraction).is_some_and(|d| d < Utc::now() + chrono::Duration::seconds(300)) {
            problems.push("The quote expires in less than 5 minutes.".into());
        }
        if !problems.is_empty() { return fail(format!("The price moved past what was approved: {}", problems.join(" "))) }
        let binding = guard::deposit_check(&one_click::bound_request(&deposit).await?, &recipient, account, asset::NEAR, destination, &units);
        if !binding.ok() { return stop(format!("The guard stopped the deposit: {}", binding.problems.join(" "))) }
        self.report(&format!("NEAR Intents: {} NEAR → ≈ {} USDC to {place}.", Fmt::num(near, 6), Fmt::num(out, 2)));
        let signed = self.signer.sign_near_intent_deposit(yocto, deposit.clone(), context.nonce, context.block_hash.clone()).await?;
        self.record("1click", &deposit, Kind::Signed, &format!("NEAR→USDC {} to {recipient} on {}", Fmt::num(near, 6), net.map_or("Solana", |n| n.name)))?;
        let hash = Near::send(&signed).await?;
        self.record("near", &hash, Kind::Confirmed, &format!("wNEAR to 1Click {deposit}"))?;
        let _ = one_click::submit_deposit(&hash, &deposit).await;
        let (started, estimate) = (Instant::now(), quote.time_estimate);
        let me = self.arc();
        let landing = async move {
            let s = me.one_click_settled(&deposit, "wNEAR went back to your NEAR account", estimate).await?;
            let line = format!("Sold {} NEAR for {} USDC in {} s — on {place}. https://nearblocks.io/txns/{hash}", Fmt::num(near, 6),
                               Fmt::num(decimal(s.amount_out_formatted.as_deref().unwrap_or("")).unwrap_or_default(), 2), started.elapsed().as_secs());
            me.report(&line);
            Ok(line)
        };
        // Inside a chain the next step is signed while 1Click swaps and pays (Landings).
        if let Some(landings) = landings() {
            landings.add(net.map_or("solana", |n| n.key), landing);
            return Ok(format!("Sent {} NEAR to NEAR Intents: ≈ {} USDC lands on {} in about a minute, and what needs it waits for it.", Fmt::num(near, 6),
                              Fmt::num(out, 2), net.map_or_else(|| "your Solana address".to_string(), |n| format!("your address on {}", n.name))));
        }
        landing.await
    }

    /// `requested` NEAR in yocto, whole micro-NEAR (Swift's UInt128 of the rounded amount): None = `free`; an amount that
    /// isn't a whole non-negative number reads as None, as `UInt128(_:)` does.
    fn yocto(requested: Option<Decimal>, micro: u128, free: u128) -> Result<u128> {
        let Some(r) = requested else { return Ok(free) };
        let Some(m) = (Num::of(r) * 1_000_000).rounded(0, Rounding::Down).0 else { return fail(OUT_OF_RANGE) };
        match text(m).parse::<u128>() {
            Ok(m) => m.checked_mul(micro).ok_or_else(|| Error::Execution(OUT_OF_RANGE.into())),
            Err(_) => Ok(free),
        }
    }

    // MARK: HLP

    /// Hyperliquid's HLP vault from the main perp balance. Deposits are locked for 4 days; withdraw with None takes
    /// everything that can be withdrawn now.
    pub async fn hlp(&self, deposit: bool, requested: Option<Decimal>) -> Result<String> {
        let evm = self.wallet.evm.as_str();
        let mut free = Hyperliquid::balance(evm).await?.withdrawable;
        let (equity, withdrawable, locked_until) = HyperliquidAccount::vault_equity(evm, guard::hl::HLP_VAULT).await?;
        let amount;
        if deposit {
            // Hyperliquid refuses vault deposits under $5 (checked live 2026-09-25); say so before signing.
            let Some(requested) = requested.filter(|r| *r >= Decimal::from(5)) else { return fail("Hyperliquid accepts vault deposits from $5.") };
            // Margin left on a HIP-3 exchange (after an oil or stock trade) comes back to the main balance first.
            for dex in Hyperliquid::HIP3_DEXES {
                if free >= requested { continue }
                let there = Hyperliquid::dex_balance(evm, dex).await?.1;
                let pull = rounded(there.min(num(Num::of(requested) - free)?), 2, Rounding::Up);
                if !(pull > Decimal::ZERO && pull <= there) { continue }
                self.move_margin(dex, "", pull).await?;
                free = Hyperliquid::balance(evm).await?.withdrawable;
            }
            if requested > free { return fail(format!("{} free on Hyperliquid — can't deposit {}.", usd(free), usd(requested))) }
            amount = requested;
        } else {
            if withdrawable <= Decimal::ZERO {
                return fail(if equity > Decimal::ZERO {
                    format!("{} in HLP, locked for 4 days after a deposit: withdrawable from {}.", usd(equity), locked_until.map_or_else(|| "soon".into(), Fmt::date))
                } else {
                    "Nothing in HLP.".into()
                });
            }
            amount = requested.unwrap_or(withdrawable).min(withdrawable);
        }
        let units = units_of(amount, 6);
        let action = MsgPack::map(vec![("type", MsgPack::str("vaultTransfer")), ("vaultAddress", MsgPack::str(guard::hl::HLP_VAULT)),
                                       ("isDeposit", MsgPack::Bool(deposit)), ("usd", MsgPack::Uint(units))]);
        let reply = self.post(&action, if deposit { "HLP deposit" } else { "HLP withdraw" }).await?;
        if reply.is_error() { return fail(format!("Hyperliquid rejected it: {reply}.")) }
        let line = if deposit {
            let apr = yields::hyperliquid_vault_apr(guard::hl::HLP_VAULT).await.unwrap_or_default();
            format!("Deposited {} into HLP ({}% APY historically). You can withdraw after 4 days, and there are losing periods.", usd(amount),
                    Fmt::num(rounded(apr, 2, Rounding::Plain), 2))
        } else {
            format!("Withdrew {} from HLP to the main Hyperliquid balance.", usd(amount))
        };
        self.report(&line);
        Ok(line)
    }

    // MARK: Spread over the best rates

    /// "Spread USDC across the best rates": plans with `allocation::plan` over the live rates and what is already
    /// deposited, and with `execute` carries the slices out one by one through the same checked paths. USDC is counted in
    /// dollars, SOL in SOL; SOL keeps 0.02 for fees and account rent. Returns the text, whether there is anything to
    /// confirm, and the preview. `from`: only that network's free money ("solana", "base"…), what a slice elsewhere needs
    /// bridged from it. Swift's defaults: `max_share` 50, `limit` None, `without` empty, `from` None.
    pub async fn allocate(&self, symbol: &str, max_share: Decimal, limit: Option<Decimal>, without: &[String], from: Option<&str>, execute: bool)
        -> Result<(String, bool, AllocationPreview)> {
        let sol = self.wallet.solana.as_str();
        let is_sol = symbol.to_uppercase() == "SOL";
        let asset = if is_sol { "SOL" } else { "USDC" };
        // «закинь 5 долларов из бейз в лучшие волты» took 1.69 from Solana and 3.41 from Base: with `from` the rest stays put.
        let on_solana = if from.is_some() && from != Some("solana") && !is_sol {
            Decimal::ZERO
        } else if is_sol {
            (sol_of(self.rpc.balance(sol).await? as i128) - d("0.02")).max(Decimal::ZERO)
        } else {
            millionths(self.rpc.token_balance(sol, relay::currency::SOLANA_USDC).await?)
        };
        // USDC idle on the EVM networks is free too: a slice there takes it where it is, and what Solana's slices lack
        // comes over from them (below).
        let idle: Vec<_> = if is_sol || from == Some("solana") {
            vec![]
        } else {
            self.idle_usdc().await.into_iter().filter(|(n, _)| from.is_none_or(|f| n.key == f)).collect()
        };
        let held = num(idle.iter().fold(Num::of(on_solana), |sum, (_, u)| sum + *u))?;
        // «spread 2 USDC»: at most that much of what is free.
        let free = limit.map_or(held, |l| held.min(l));
        let jl = JupiterLend::default();
        let kam = Kamino::default();
        let (options, vaults, jup, kpos, markets) =
            tokio::join!(yields::options(Some(asset)), jl.vaults(), jl.positions(sol), kam.positions(sol), hl_markets());
        let sol_price = markets?.get("SOL").map_or(Decimal::ZERO, |m| m.mark_px);
        let wanted = if is_sol { "WSOL" } else { "USDC" };
        let vault = vaults.ok().unwrap_or_default().into_iter().find(|v| v.asset_symbol.to_uppercase() == wanted);
        let jup_positions = jup.ok().unwrap_or_default();
        let jupiter = vault.as_ref().and_then(|v| jup_positions.iter().find(|p| p.vault == v.symbol).map(|p| shift(p.assets, v.decimals))).unwrap_or_default();
        let kamino_usd = num(kpos.ok().unwrap_or_default().iter().filter(|p| p.symbol.to_uppercase() == asset).fold(Num::of(Decimal::ZERO), |s, p| s + p.value_usd))?;
        let kamino = if is_sol { if sol_price > Decimal::ZERO { num(Num::of(kamino_usd) / sol_price)? } else { Decimal::ZERO } } else { kamino_usd };
        let mut jito = Decimal::ZERO;
        if is_sol && let Ok(held) = self.rpc.token_balance(sol, &SpotAsset::jito_sol().mint).await && held > 0
            && let Ok(q) = jupiter::quote(&SpotAsset::jito_sol().mint, &SpotAsset::all()[0].mint, held, 50, &[]).await {
            jito = shift(q.out_amount, 9);
        }
        // Aave and Morpho on the EVM networks, USDC only: rates and balances from the pools and vaults themselves.
        let evm = if is_sol { vec![] } else { self.evm_places().await };
        let total = num(evm.iter().fold(Num::of(free) + jupiter + kamino + jito, |s, p| s + p.supplied))?;
        let kamino_rent_usd = num(Num::of(Self::kamino_first_rent(&self.rpc).await) * sol_price)?;
        // A vault switched off in Settings › Modules is skipped, with the reason in the notes; so is one they asked to leave out.
        let modules = modules();
        let left_out = |place: &str| -> Option<String> {
            without.iter().any(|w| place.to_lowercase().contains(w.as_str())).then(|| "left out, as asked".to_string())
        };
        let cap = |existing: Decimal| num(Num::of(total) * max_share / 100 - existing);
        // The first deposit's rent is a cost, not a refundable deposit: worth it only for a big enough slice.
        let kamino_slice = num(Num::of(free.min(cap(kamino)?)) * if is_sol { sol_price } else { Decimal::ONE })?;
        let mut all: Vec<Place> = options.iter().filter_map(|o| {
            let open = |existing: Decimal, blocked: Option<String>| Place { existing, blocked, ..Place::new(&o.place, o.apy) };
            match o.place.as_str() {
                "Jupiter Lend" => Some(open(jupiter, left_out(&o.place).or_else(|| modules.refusal_vault(&o.place)))),
                "JitoSOL staking" => Some(open(jito, left_out(&o.place).or_else(|| modules.refusal_vault(&o.place)))),
                "Kamino Lend" => {
                    let blocked = (kamino.is_zero() && kamino_rent_usd > kamino_slice / Decimal::TEN).then(|| {
                        format!("the first deposit creates Kamino accounts for ≈ {} of rent, more than 10% of the {} slice", usd(kamino_rent_usd), usd(kamino_slice))
                    });
                    Some(open(kamino, left_out(&o.place).or_else(|| modules.refusal_vault(&o.place)).or(blocked)))
                }
                "HLP (Hyperliquid)" => Some(open(Decimal::ZERO, Some(
                    "another network: bridge, $5 minimum and a 4-day lock — use a separate command, «deposit 5 USDC to HLP»".into()))),
                _ => None,
            }
        }).collect();
        // A known place whose rate, balance or gas didn't answer this time is named with why, not left out silently.
        if !is_sol {
            let read: HashSet<&str> = evm.iter().map(|p| p.name.as_str()).collect();
            for name in aave::MARKETS.iter().map(|m| m.place()).chain(morpho::VAULTS.iter().map(|v| v.place())) {
                if read.contains(name.as_str()) || modules.refusal_vault(&name).is_some() { continue }
                let apy = options.iter().find(|o| o.place == name).map_or(Decimal::ZERO, |o| o.apy);
                let blocked = left_out(&name).unwrap_or_else(|| "its rate, balance or gas couldn't be read just now — ask again in a minute".into());
                all.push(Place { blocked: Some(blocked), ..Place::new(&name, apy) });
            }
        }
        for p in &evm {
            // Worth it only when the round trip costs less than half a year of the slice's interest: on Ethereum that
            // takes a large slice, on the L2s a few dollars.
            let slice = free.min(cap(p.supplied)?).max(Decimal::ZERO);
            let yearly = num(Num::of(slice) * p.apy / 100)?;
            all.push(Place {
                existing: p.supplied, blocked: left_out(&p.name).or_else(|| modules.refusal_vault(&p.name)).or_else(|| p.too_costly(yearly)),
                protocol_name: p.protocol_name.clone(), ..Place::new(&p.name, p.apy)
            });
        }
        let plan = allocation::plan(free, &all, max_share, if is_sol { d("0.003") } else { d("0.5") });
        let yearly = plan.yearly();
        let amount = |x: Decimal| format!("{} {asset}", Fmt::num(x, if is_sol { 6 } else { 2 }));
        let mut lines = vec![format!("Spread {} free ({} total in {asset}), at most {} per protocol:", amount(free), amount(total), Fmt::pct(max_share))];
        let mut sources: Vec<(String, Decimal)> = if on_solana > Decimal::ZERO { vec![("Solana".into(), on_solana)] } else { vec![] };
        sources.extend(idle.iter().map(|(n, u)| (n.name.to_string(), *u)));
        if !idle.is_empty() { lines.push(format!("· free now: {}", sources.iter().map(|(p, a)| format!("{} on {p}", amount(*a))).collect::<Vec<_>>().join(", "))) }
        lines.extend(plan.slices.iter().map(|s| format!("· {}: {} at {}%", s.place, amount(s.amount), Fmt::num(rounded(s.apy, 2, Rounding::Plain), 2))));
        if plan.slices.is_empty() { lines.push("· nowhere to put it: everything is at its limit or unavailable.".into()) }
        lines.extend(plan.notes.iter().map(|n| format!("· {n}")));
        if plan.leftover > Decimal::ZERO { lines.push(format!("Left free: {}.", amount(plan.leftover))) }
        let yearly_usd = if is_sol && sol_price > Decimal::ZERO { Some(num(Num::of(yearly) * sol_price)?) } else { None };
        if yearly > Decimal::ZERO {
            lines.push(format!("Income from the new deposits ≈ {} a year{}.", amount(yearly), yearly_usd.map_or(String::new(), |u| format!(" (≈ {})", usd(u)))));
        }
        // Where each slice's money comes from, straight from the network that holds it (spread_moves).
        let network = |place: &str| -> Option<&'static str> { aave::market(place).map(|m| m.chain).or_else(|| morpho::vault(place).map(|v| v.chain)) };
        let name = |key: &str| -> String { if key == "solana" { "Solana".into() } else { EvmNetwork::named(key).map_or_else(|| key.to_string(), |n| n.name.to_string()) } };
        let (moves, drawn) = Self::spread_moves(&plan.slices.iter().map(|s| (network(&s.place).map(String::from), s.amount)).collect::<Vec<_>>(),
                                                &idle.iter().map(|(n, u)| (n.key.to_string(), *u)).collect::<Vec<_>>(), on_solana);
        let preview = AllocationPreview {
            asset: asset.into(), free, max_share, slices: plan.slices.clone(),
            skipped: all.iter().filter_map(|p| p.blocked.as_ref().map(|b| (p.name.clone(), p.apy, b.clone()))).collect(),
            leftover: plan.leftover, yearly, yearly_usd, sources,
            drawn: drawn.iter().map(|(k, u)| (name(k), *u)).collect(),
            moves: moves.iter().map(|(f, t, u)| (name(f), name(t), *u)).collect(),
        };
        // The preview goes back to the caller, which adds how to confirm.
        if !execute || plan.slices.is_empty() { return Ok((lines.join("\n"), !plan.slices.is_empty(), preview)) }
        self.report(&lines.join("\n"));
        let mut done: Vec<String> = vec![];
        // The money goes first, each move straight from the network that holds it to the one a slice is on (Solana's
        // slices included); what Solana itself gives an EVM slice, the deposit brings on its own (ensure_evm).
        for (from_key, to_key, usdc) in moves.iter().filter(|m| m.0 != "solana") {
            let Some(src) = EvmNetwork::named(from_key) else { continue };
            self.ensure_evm(src, 0, None).await?;
            done.push(if to_key == "solana" {
                self.evm_to_solana(src, Some(*usdc)).await?
            } else {
                self.bridge(src, EvmNetwork::named(to_key), false, Some(*usdc)).await?
            });
        }
        // EVM slices first: their shortfall leaves Solana before Solana's own slices spend it.
        let mut slices = plan.slices.clone();
        slices.sort_by_key(|s| network(&s.place).is_none());
        for slice in slices {
            let mut piece = rounded(slice.amount, if is_sol { 6 } else { 2 }, Rounding::Down);
            if !is_sol && network(&slice.place).is_none() {
                // Relay's fees on the way leave Solana a few cents under the plan: a slice takes at most what is there.
                let there = millionths(self.rpc.token_balance(sol, relay::currency::SOLANA_USDC).await?);
                piece = piece.min(rounded(there, 2, Rounding::Down));
            } else if let Some(key) = network(&slice.place)
                && moves.iter().any(|m| m.1 == key && m.0 != "solana")
                && let Some(net) = EvmNetwork::named(key) {
                // The same on an EVM network a move just reached.
                let there = millionths(net.usdc_balance(&self.wallet.evm).await?);
                piece = piece.min(rounded(there - millionths(net.fee_reserve()), 2, Rounding::Down));
            }
            match slice.place.as_str() {
                "Jupiter Lend" => done.push(self.lend(asset, Some(piece), true).await?),
                "Kamino Lend" => done.push(self.kamino(asset, Some(piece), true, false, false).await?),
                "JitoSOL staking" => done.push(self.convert(&SpotAsset::all()[0], &SpotAsset::jito_sol(), Some(piece), None, false, None).await?),
                place => {
                    if let Some(m) = aave::market(place) {
                        done.push(self.aave(true, Some(piece), m).await?);
                    } else if let Some(v) = morpho::vault(place) {
                        done.push(self.morpho(true, Some(piece), v).await?);
                    }
                }
            }
        }
        let line = format!("Spread across {} {}.", done.len(), if done.len() == 1 { "protocol" } else { "protocols" });
        self.report(&line);
        Ok((line, false, preview))
    }

    /// Where a spread's money comes from. A slice on an EVM network (`network` set) takes the USDC idle there first;
    /// Solana's slices take Solana's own. What a place still lacks comes straight from the idle USDC of the other
    /// networks, in the order given (Ethereum last) — a dime over each for Relay's fee, nothing under a bridge's half
    /// dollar — and an EVM slice's last gap from what Solana has left («solana» → network, brought by the deposit).
    /// `drawn`: what each place gives in all, its own slices and its moves; network keys, «solana» for Solana.
    pub fn spread_moves(slices: &[(Option<String>, Decimal)], idle: &[(String, Decimal)], on_solana: Decimal) -> SpreadMoves {
        let mut spare: HashMap<String, Decimal> = HashMap::new();
        for (k, u) in idle { spare.entry(k.clone()).and_modify(|v| *v = add(*v, *u)).or_insert(*u); }
        let mut drawn: HashMap<String, Decimal> = HashMap::new();
        fn give(drawn: &mut HashMap<String, Decimal>, k: &str, u: Decimal) { drawn.entry(k.into()).and_modify(|v| *v = add(*v, u)).or_insert(u); }
        let (mut short, mut on_solana_needs): (Vec<(String, Decimal)>, Decimal) = (vec![], Decimal::ZERO);
        for (network, amount) in slices {
            let Some(key) = network else {
                on_solana_needs = add(on_solana_needs, *amount);
                continue;
            };
            let take = spare.get(key).copied().unwrap_or_default().min(*amount);
            spare.insert(key.clone(), sub(spare.get(key).copied().unwrap_or_default(), take));
            give(&mut drawn, key, take);
            if *amount > take { short.push((key.clone(), sub(*amount, take))) }
        }
        let own = on_solana.min(on_solana_needs);
        let mut solana_left = sub(on_solana, own);
        give(&mut drawn, "solana", own);
        if on_solana_needs > own { short.push(("solana".into(), sub(on_solana_needs, own))) }
        let mut moves: Vec<(String, String, Decimal)> = vec![];
        for (to, mut need) in short {
            for (key, _) in idle {
                if !(need > Decimal::ZERO && *key != to) { continue }
                let send = rounded(spare.get(key).copied().unwrap_or_default().min(add(need, d("0.1"))), 2, Rounding::Down);
                if send < d("0.5") { continue }
                moves.push((key.clone(), to.clone(), send));
                spare.insert(key.clone(), sub(spare.get(key).copied().unwrap_or_default(), send));
                give(&mut drawn, key, send);
                need = sub(need, send);
            }
            if need > Decimal::ZERO && to != "solana" && solana_left > Decimal::ZERO {
                let send = solana_left.min(need);
                moves.push(("solana".into(), to.clone(), send));
                solana_left = sub(solana_left, send);
                give(&mut drawn, "solana", send);
            }
        }
        let order = std::iter::once("solana").chain(idle.iter().map(|(k, _)| k.as_str()));
        let drawn_list = order.filter_map(|k| drawn.get(k).filter(|v| **v > Decimal::ZERO).map(|v| (k.to_string(), *v))).collect();
        (moves, drawn_list)
    }

    /// What comes over to Solana from each EVM network before a spread (spread_moves' moves into Solana).
    pub fn pull_to_solana(slices: &[(Option<String>, Decimal)], idle: &[(String, Decimal)], on_solana: Decimal) -> Vec<(String, Decimal)> {
        Self::spread_moves(slices, idle, on_solana).0.into_iter().filter(|m| m.1 == "solana").map(|m| (m.0, m.2)).collect()
    }

    // MARK: Kamino

    /// SOL of rent Kamino's first deposit opens its accounts with: 0.0298 measured 2026-09-27, when an empty account's
    /// minimum was 650,240 lamports, scaled to today's (rent keeps falling, SIMD-0437). That measure when the node is silent.
    pub async fn kamino_first_rent(rpc: &SolanaRpc) -> Decimal {
        let measured = d("0.0298");
        let Ok(now) = rpc.rent_minimum(0).await else { return measured };
        measured * Decimal::from(now) / Decimal::from(650_240)
    }

    /// SOL spent according to a simulation's change lines (the «SOL: −0.0298» line), as a positive number.
    pub fn sol_spent(changes: &[String]) -> Decimal {
        let Some(line) = changes.iter().find(|c| c.starts_with("SOL: ")) else { return Decimal::ZERO };
        // en_US: «,» only groups thousands
        let number = line["SOL: ".len()..].chars().take_while(|c| !c.is_whitespace()).collect::<String>().replace(',', "").replace('−', "-");
        (-decimal(&number).unwrap_or_default()).max(Decimal::ZERO)
    }

    /// The first Kamino deposit's rent and fee from its simulation's change lines: what SOL left beyond `deposited`, the
    /// SOL that went in itself.
    pub fn kamino_rent_spent(changes: &[String], deposited: Decimal) -> Decimal {
        (Num::of(Self::sol_spent(changes)) - deposited).0.map_or(Decimal::ZERO, |r| r.max(Decimal::ZERO))
    }

    /// What «withdraw all» asks Kamino for, in tokens: the position's dollars at the token's price, 1% over. 0 when that
    /// is past Decimal's range (the move then says there's no position to take).
    pub fn kamino_all(value_usd: Decimal, price_usd: Decimal) -> Decimal {
        (Num::of(value_usd) / price_usd * d("1.01")).rounded(6, Rounding::Up).0.unwrap_or_default()
    }

    /// The first Kamino deposit's rent against what goes in: over a tenth of it, and over what the user took on with
    /// «Send anyway» (`accepted`, dollars), is a stop to ask about; None lets the deposit go.
    pub fn kamino_rent(sol: Decimal, sol_price: Decimal, deposit_usd: Decimal, accepted: Decimal) -> Option<CostlyMove> {
        let rent = (Num::of(sol) * sol_price).0.unwrap_or(Decimal::MAX);
        if !(rent > deposit_usd / Decimal::TEN && rent > accepted) { return None }
        let worth_from = (Num::of(rent) * 10).rounded(0, Rounding::Up).0.unwrap_or(Decimal::MAX);
        Some(CostlyMove {
            words: Some(format!("The first Kamino deposit creates your accounts: rent ≈ {} SOL (≈ {}) on a {} deposit — over 10% of the amount. \
                                 Jupiter Lend has no rent, and Kamino makes sense from ≈ {}. Nothing was signed.", Fmt::num(sol, 4), usd(rent), usd(deposit_usd),
                                usd(worth_from))),
            ..CostlyMove::new(rent, deposit_usd, "Kamino")
        })
    }

    /// Kamino Lend: the main market, and the Backpack market for Backpack's stocks (kamino::market) — there a deposit is
    /// collateral to borrow USDC against. `symbol`: USDC, SOL, USDT, SPCX… `amount` in tokens; withdraw with None takes
    /// the whole position (its dollars at the token's price). `dry_run` only simulates: the first deposit's rent shows up
    /// here. `accept_rent`: the plan showed the first deposit's rent, so the 10% rule does not ask.
    pub async fn kamino(&self, symbol: &str, requested: Option<Decimal>, deposit: bool, dry_run: bool, accept_rent: bool) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let k = Kamino::default();
        let market = kamino::market(symbol);
        let stock = market == kamino::BACKPACK_MARKET;
        let Some(reserve) = k.reserves(market).await?.into_iter().find(|r| r.symbol.to_uppercase() == symbol.to_uppercase()) else {
            return fail(format!("Kamino has no {symbol} market."));
        };
        let native = reserve.mint == WSOL;
        let supply = self.rpc.call("getTokenSupply", json!([reserve.mint])).await?;
        let decimals = supply.get("value").and_then(|v| v.get("decimals")).and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))).unwrap_or(6);
        let shown = |u: u64| shift(u, decimals);
        // On the Backpack market a position is collateral, and a loan may stand on it; on the main market one may too (USDC
        // borrowed against SOL), so a withdrawal reads it.
        let loan = if stock || !deposit { k.loan(sol, market).await? } else { None };
        let position_usd = if stock {
            loan.as_ref().and_then(|l| l.collateral.iter().find(|c| c.reserve == reserve.address)).map(|c| c.value_usd)
        } else {
            k.positions(sol).await?.into_iter().find(|p| p.reserve == reserve.address).map(|p| p.value_usd)
        };
        // The 1.5 line here reads Kamino's API, which lags a borrow by 10–30 s: right after one, its debt may read as none.
        if !deposit && position_usd.is_some() && self.kamino_borrowed_just_now(market) {
            return fail("A Kamino loan was taken on this market less than a minute ago, and Kamino's numbers lag it: \
                         say the withdrawal again in a minute, when how much can come out is sure. Nothing was signed.");
        }
        if !deposit && let Some(loan) = &loan && loan.debt_usd > Decimal::ZERO && loan.collateral.iter().any(|c| c.reserve == reserve.address) {
            // Kamino lets collateral out only while what's left still carries the debt at its borrow limit and at a health factor of 1.5.
            let risk = k.risk(&reserve.address).await?;
            let free = Self::kamino_free_collateral(loan, risk.max_ltv, risk.liquidation_ltv, reserve.price_usd);
            if requested.is_none_or(|r| r > free) {
                return fail(format!("A loan of {} stands on this {symbol} on Kamino: {}Pay it back first — «repay all USDC on kamino{}» — and the rest comes out. \
                                     Nothing was signed.", usd(loan.debt_usd),
                                    if free > Decimal::ZERO { format!("up to {} {symbol} can come out now. ", Fmt::num(free, 6)) } else { String::new() },
                                    if stock { "" } else { " against SOL" }));
            }
        }
        // A deposit made on kamino.com sits in a Kamino Vault (Earn), not in Lend.
        // ponytail: Lend first when both hold the token; «all» then leaves the vault, say it again for that.
        if !deposit && !stock && position_usd.is_none()
            && let Some(v) = k.vault_positions(sol).await?.into_iter().find(|v| v.mint == reserve.mint && v.shares > Decimal::ZERO) {
            return self.kamino_vault(&v, requested, decimals, dry_run).await;
        }
        // Everything: ask for 1% over the value — Kamino caps a withdrawal at what is there, so no dust stays.
        let all = !deposit && requested.is_none();
        if all && position_usd.is_some() && reserve.price_usd <= Decimal::ZERO {
            return fail(format!("Kamino gave no {symbol} price just now. Say how much to withdraw."));
        }
        let amount = requested.or_else(|| if deposit { None } else { position_usd.map(|p| Self::kamino_all(p, reserve.price_usd)) });
        let Some(amount) = amount.filter(|a| *a > Decimal::ZERO) else {
            return fail(if deposit { "How much should go into Kamino?".to_string() } else { format!("No {symbol} position in Kamino.") });
        };
        let units = units_of(amount, decimals);
        // What should come out: the position's value when taking everything, else what was asked.
        let expected = i(if all { (units as u128 * 100 / 101) as u64 } else { units });
        // Backpack's stocks are Token-2022 tokens: their account derives from that program.
        let my_token = spl::associated_account(sol, &reserve.mint, &self.token_program(&reserve.mint).await?)?;
        let token_owners = owners(&[(&my_token, sol)]);
        // SOL put in leaves the wallet the fees of what comes after — a repay, a withdrawal — and the first deposit's rent
        // (the first move needs gas): `kept` SOL stays in the wallet.
        let kept = sol_of(TopUpOrder::SOLANA_FEE_FLOOR as i128);
        let short_of_sol = |have: u64, rent: Decimal| -> Error {
            let room = (Num::of(shown(have)) - rent - kept).0.map_or(Decimal::ZERO, |r| rounded(r.max(Decimal::ZERO), 4, Rounding::Down));
            Error::Execution(format!("Putting {} SOL into Kamino would leave the wallet under {} SOL, too little for the fees of a repay or a withdrawal later{}: \
                                      up to {} SOL can go in now. Nothing was signed.", Fmt::num(amount, 6), Fmt::num(kept, 3),
                                     if rent > Decimal::ZERO { format!(" (this first deposit also pays ≈ {} SOL of rent for Kamino's accounts)", Fmt::num(rent, 4)) }
                                     else { String::new() },
                                     Fmt::num(room, 4)))
        };
        // Short of `amount` and its rent and what stays: an amount past range is short too.
        let short = |have: u64, rent: Decimal| (Num::of(amount) + rent + kept).0.is_none_or(|need| shown(have) < need);
        let have = if deposit { if native { self.rpc.balance(sol).await? } else { self.rpc.token_balance(sol, &reserve.mint).await? } } else { 0 };
        if deposit && have < units {
            return fail(format!("You have {} {symbol}, and the deposit needs {}.", Fmt::num(shown(have), 6), Fmt::num(amount, 6)));
        }
        let wire = if deposit {
            k.deposit_transaction(sol, &reserve.address, amount, market).await?
        } else {
            k.withdraw_transaction(sol, &reserve.address, amount, market).await?
        };
        let last_valid = self.rpc.latest_blockhash().await?.1;
        // The first deposit pays rent for the user's Kamino accounts and lookup table; later ones only the fee. On the
        // Backpack market the position is one for all its stocks.
        let first_time = (if stock { loan.is_none() } else { position_usd.is_none() }) && deposit;
        let mut rules: Vec<Rule> = vec![];
        if !native { rules.push((my_token.clone(), symbol.into(), true, if deposit { -i(units) } else { pct(expected, 99) })) }
        let lamports: i64 = if native {
            if deposit { (-i(units)).saturating_sub(if first_time { 60_000_000 } else { 5_000_000 }) } else { pct(expected, 99) - 5_000_000 }
        } else if first_time { -60_000_000 } else { -5_000_000 };
        if native && deposit && !first_time && have < units.saturating_add(TopUpOrder::SOLANA_FEE_FLOOR) { return Err(short_of_sol(have, Decimal::ZERO)) }
        // A stock put up on the Backpack market isn't asked about: its position's rent (≈ 0.0176 SOL) comes back when
        // everything is taken out — Kamino closes the position then (measured live 2026-10-28).
        if first_time && !dry_run && (native || (!accept_rent && !stock)) {
            // The first deposit pays ~0.03–0.04 SOL of rent for Kamino's user accounts and lookup table. Not worth it for
            // a small amount: simulate first, and when the rent is over a tenth of what goes in, stop and ask (CostlyMove:
            // «Send anyway», or Jupiter Lend, which has no rent).
            let preview = match self.sign_and_send(&wire, last_valid, &token_owners, &rules, &format!("Kamino {symbol}"), lamports, &HashMap::new(), true, None).await {
                Ok(p) => p,
                Err(e) => {
                    if native {
                        // Too little SOL for the rent as well: a small deposit's simulation says what the rent is, and so how much fits.
                        if let Some(rent) = self.kamino_deposit_cost(&reserve, d("0.001"), market).await && short(have, rent) {
                            return Err(short_of_sol(have, rent));
                        }
                    }
                    return Err(e);
                }
            };
            // A SOL deposit's own amount leaves the wallet too: the rent is what left beyond it.
            let rent_sol = Self::kamino_rent_spent(&preview, if native { amount } else { Decimal::ZERO });
            if native && short(have, rent_sol) { return Err(short_of_sol(have, rent_sol)) }
            if !accept_rent && !stock {
                let sol_price = hl_markets().await?.get("SOL").map_or(Decimal::ZERO, |m| m.mark_px);
                let value_usd = if native { num(Num::of(amount) * sol_price)? } else { amount };
                if let Some(stop) = Self::kamino_rent(rent_sol, sol_price, value_usd, loss_accepted()) { return Err(stop.into()) }
            }
        }
        // A withdrawal brings what was asked, 1% over at most (all: what's there, asked 1% over): more would be collateral
        // the card didn't say leaves, past the 1.5 line on a loan. SOL also gets back the rent of the accounts Kamino closes
        // when everything comes out (≈ 0.0176 for the position): 0.03 SOL of room for it, and the wallet's own wrapped SOL,
        // which Kamino's transaction unwraps as it closes that account.
        let wrapped = if native && !deposit { i(self.rpc.token_balance(sol, &reserve.mint).await?) } else { 0 };
        let most: HashMap<String, i64> = if deposit {
            HashMap::new()
        } else if native {
            HashMap::from([(sol.to_string(), pct(i(units), 101).saturating_add(30_000_000).saturating_add(wrapped))])
        } else {
            HashMap::from([(my_token.clone(), pct(i(units), 101))])
        };
        let changes = self.sign_and_send(&wire, last_valid, &token_owners, &rules, &format!("Kamino {} {symbol}", if deposit { "deposit" } else { "withdraw" }),
                                         lamports, &most, dry_run, None).await?;
        if stock && !dry_run {
            // Read on: the portfolio right after must find the stock under Kamino, not lose it.
            let value = |l: Option<&kamino::Loan>| l.and_then(|l| l.collateral.iter().find(|c| c.reserve == reserve.address)).map_or(Decimal::ZERO, |c| c.value_usd);
            let was = value(loan.as_ref());
            let _ = self.kamino_loan(market, |l| if deposit { value(l) > was } else { value(l) < was }).await;
        }
        let did = if deposit {
            format!("deposited {} {symbol}", Fmt::num(amount, 6))
        } else if all {
            "withdrew everything".to_string()
        } else {
            format!("withdrew {} {symbol}", Fmt::num(amount, 6))
        };
        let head = if dry_run {
            "Kamino simulation (nothing signed): ".to_string()
        } else if stock {
            format!("Kamino's Backpack market: {}. ", if deposit { format!("put up {} {symbol} as collateral", Fmt::num(amount, 6)) } else { did })
        } else {
            format!("Kamino {symbol} {}% APY: {did}. ", Fmt::num(rounded(reserve.supply_apy, 2, Rounding::Plain), 2))
        };
        let line = format!("{head}{}.", changes.join(", "));
        self.report(&line);
        Ok(line)
    }

    /// Out of a Kamino Vault: `amount` of its token (None = all), asked for in shares at Kamino's price per share, which
    /// runs a hair above what comes out. Shares staked in the vault's farm are unstaked in the same transaction.
    pub(crate) async fn kamino_vault(&self, v: &VaultPosition, requested: Option<Decimal>, decimals: i64, dry_run: bool) -> Result<String> {
        let sol = self.wallet.solana.as_str();
        let cent = d("0.01");
        // A cent short of the whole is the whole: «выведи 5» from 4.999 takes every share instead of failing.
        let all = requested.is_none_or(|r| (Num::of(v.tokens) - cent).0.is_none_or(|whole| r >= whole));
        if let Some(r) = requested && (Num::of(v.tokens) + cent).0.is_some_and(|most| r > most) {
            return fail(format!("Only {} {} in {} on Kamino — can't withdraw {}.", Fmt::num(v.tokens, 6), v.symbol, v.name, Fmt::num(r, 6)));
        }
        let amount = if all { v.tokens } else { requested.unwrap_or(v.tokens) };
        if amount <= Decimal::ZERO { return fail(format!("Say how much to withdraw from {}.", v.name)) }
        let shares = if all { v.shares } else { v.shares.min(num((Num::of(amount) * v.shares / v.tokens).rounded(6, Rounding::Up))?) };
        let expected = times_pow10(amount, decimals).and_then(|x| rounded(x, 0, Rounding::Down).to_i64()).unwrap_or(0);
        let my_token = ata(sol, &v.mint)?;
        let wire = Kamino::default().vault_withdraw_transaction(sol, &v.vault, shares).await?;
        let last_valid = self.rpc.latest_blockhash().await?.1;
        // ponytail: a SOL vault pays out wSOL, left wrapped in the token account.
        let changes = self.sign_and_send(&wire, last_valid, &owners(&[(&my_token, sol)]), &[(my_token.clone(), v.symbol.clone(), true, pct(expected, 99))],
                                         &format!("Kamino {} withdraw", v.name), -5_000_000, &HashMap::new(), dry_run, None).await?;
        let head = if dry_run {
            "Kamino simulation (nothing signed): ".to_string()
        } else {
            format!("Kamino {}: withdrew {}{} {}. ", v.name, if all { "everything, " } else { "" }, Fmt::num(amount, 6), v.symbol)
        };
        let line = format!("{head}{}.", changes.join(", "));
        self.report(&line);
        Ok(line)
    }
}

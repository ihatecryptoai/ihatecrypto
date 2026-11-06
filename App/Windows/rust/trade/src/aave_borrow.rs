//! Borrowing USDC on Aave against what the wallet supplied there (ETH as collateral, or its own USDC), and paying it back
//! (AaveBorrow.swift). A borrow leaves a health factor of at least `MINIMUM_HEALTH` — ihatecrypto's own risk limit, above
//! Aave's 1.0 at which the collateral is sold at a penalty — and the card says at which price of the collateral that
//! would happen.

use std::time::Duration;

use evm::abi::address_in;
use evm::protocols::{aave, uniswap};
use ihatecrypto_core::foundation::{Num, Rounding, text};
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::aave::{EvmNetwork, coin, rpc_of};
use crate::executor::uint64;
use crate::{Executor, Result, fail, num, token_balance, u256, word};

/// The wallet's account on an Aave market, in USD as Aave's oracle prices it.
#[derive(Debug, Clone, PartialEq)]
pub struct AaveAccount {
    pub collateral: Decimal,
    pub debt: Decimal,
    pub available: Decimal,
    /// The collateral's weighted liquidation threshold, 0…1.
    pub threshold: Decimal,
}

impl AaveAccount {
    /// None without debt (Swift's `health`).
    pub fn health(&self) -> Option<Decimal> { Self::health_of(self.collateral, self.debt, self.threshold) }

    /// Swift's static `health(collateral:debt:threshold:)`.
    pub fn health_of(collateral: Decimal, debt: Decimal, threshold: Decimal) -> Option<Decimal> {
        if debt > Decimal::ZERO { (Num::of(collateral) * threshold / debt).0 } else { None }
    }

    /// The price of a volatile collateral (`amount` of it at `price` now, the rest of the collateral steady) at which the
    /// health factor reaches 1 with `debt`; None when the rest alone covers the debt. Uses the account's weighted threshold.
    // ponytail: one threshold for all the collateral; per-asset thresholds if positions mix assets with different ones.
    pub fn liquidation_price(debt: Decimal, threshold: Decimal, collateral: Decimal, amount: Decimal, price: Decimal) -> Option<Decimal> {
        if threshold <= Decimal::ZERO || amount <= Decimal::ZERO || price <= Decimal::ZERO { return None }
        let rest = Num::of(collateral) - Num::of(amount) * price;
        let needed = (Num::of(debt) / threshold - rest).0?;
        if needed > Decimal::ZERO { (Num::of(needed) / amount).0 } else { None }
    }
}

fn explorer(m: &aave::Market) -> &'static str { EvmNetwork::of(m.chain_id).map_or("", |n| n.explorer) }
fn health(a: &AaveAccount) -> String { a.health().map_or("∞".into(), |h| Fmt::num(h, 2)) }
fn millionths(x: Decimal) -> Decimal { x / Decimal::from(1_000_000) }

impl Executor {
    /// What a borrow may leave: Aave liquidates at 1.0; ihatecrypto stops at 1.5.
    pub const MINIMUM_HEALTH: Decimal = Decimal::from_parts(15, 0, 0, false, 1);

    /// The wallet's account on an Aave market.
    pub async fn aave_account(&self, m: &aave::Market) -> Result<AaveAccount> {
        let d = rpc_of(m.chain_id)?.read(m.pool, &aave::account_data(&self.wallet.evm)?).await?;
        let usd = |w| word(&d, w).unwrap_or_default() / Decimal::from(100_000_000);
        Ok(AaveAccount { collateral: usd(0), debt: usd(1), available: usd(2), threshold: word(&d, 3).unwrap_or_default() / Decimal::from(10_000) })
    }

    /// The account once its debt shows a change from `before` (Swift's `aaveAccount(_:changedFrom:)`): a node a block
    /// behind answered the old one right after the receipt. Ten seconds at most.
    pub(crate) async fn aave_account_changed(&self, m: &aave::Market, before: &AaveAccount) -> Result<AaveAccount> {
        let mut now = self.aave_account(m).await?;
        for _ in 0..10 {
            if now.debt != before.debt { break }
            tokio::time::sleep(Duration::from_secs(1)).await;
            now = self.aave_account(m).await?;
        }
        Ok(now)
    }

    /// ETH supplied there, by its aToken, and its price on Aave's oracle; None without any.
    pub(crate) async fn aave_eth_collateral(&self, m: &aave::Market) -> Result<Option<(Decimal, Decimal)>> {
        let rpc = rpc_of(m.chain_id)?;
        let Some((_, weth)) = uniswap::v2(m.chain_id) else { return Ok(None) };
        let Some(a_weth) = address_in(&rpc.read(m.pool, &aave::get_reserve_data(weth)?).await?, 8) else { return Ok(None) };
        let held = self.balance(&a_weth, &rpc).await? / Decimal::from(1_000_000_000_000_000_000u64);
        if held <= Decimal::ZERO { return Ok(None) }
        let Some(oracle) = address_in(&rpc.read(m.provider, &aave::GET_PRICE_ORACLE).await?, 0) else { return Ok(None) };
        let Some(price) = word(&rpc.read(&oracle, &aave::asset_price(weth)?).await?, 0) else { return Ok(None) };
        Ok(Some((held, price / Decimal::from(100_000_000))))
    }

    /// What borrowing `usdc` would do, read now: the health factor before and after, and the price of ETH at which the
    /// position would be liquidated. Nothing is signed.
    pub async fn aave_borrow_preview(&self, m: &aave::Market, usdc: Decimal) -> Result<(AaveAccount, Option<Decimal>, Option<Decimal>)> {
        let account = self.aave_account(m).await?;
        let debt = num(Num::of(account.debt) + usdc)?;
        let after = AaveAccount::health_of(account.collateral, debt, account.threshold);
        let eth = self.aave_eth_collateral(m).await?;
        let liquidation = eth.and_then(|(amount, price)| AaveAccount::liquidation_price(debt, account.threshold, account.collateral, amount, price));
        Ok((account, after, liquidation))
    }

    /// Borrows `usdc` on Aave (variable rate) to this wallet, refused when the health factor would fall under
    /// `MINIMUM_HEALTH` or past what Aave lets it borrow.
    pub async fn aave_borrow(&self, m: &aave::Market, usdc: Decimal) -> Result<String> {
        let units = uint64(num((Num::of(usdc) * 1_000_000).rounded(0, Rounding::Down))?);
        if units == 0 { return fail("Say how many USDC to borrow.") }
        let (now, after, liquidation) = self.aave_borrow_preview(m, usdc).await?;
        if now.collateral <= Decimal::ZERO {
            return fail(format!("Nothing on Aave on {} to borrow against: supply collateral first — «supply 0.01 ETH as collateral on Aave on {}».",
                                m.chain_name, m.chain_name));
        }
        if usdc > now.available {
            return fail(format!("Aave lets this wallet borrow {} more on {} now, and {} USDC is past it. Nothing was signed.", Fmt::usd(now.available), m.chain_name,
                                Fmt::num(usdc, 2)));
        }
        let Some(after) = after.filter(|a| *a >= Self::MINIMUM_HEALTH) else {
            return fail(format!("That would leave a health factor of {}, while ihatecrypto borrows only down to {} (Aave sells the collateral at 1.0). \
                                 Borrow less, or add collateral. Nothing was signed.", Fmt::num(after.unwrap_or_default(), 2), text(Self::MINIMUM_HEALTH)));
        };
        let liquidation_words = |sep: &str| liquidation.map(|l| format!("{sep} liquidation if ETH ≤ {}", Fmt::usd(l))).unwrap_or_default();
        self.report(&format!("Aave on {}: {} of collateral, health factor {} → {}{}.", m.chain_name, Fmt::usd(now.collateral), health(&now),
                             Fmt::num(after, 2), liquidation_words(",")));
        let rpc = rpc_of(m.chain_id)?;
        let before = token_balance(&rpc, m.usdc, &self.wallet.evm).await?;
        let hash = self.send_evm(&rpc, m.pool, &aave::borrow(m.usdc, units, &self.wallet.evm)?,
                                 &format!("Borrow {} USDC on Aave on {}", Fmt::num(usdc, 2), m.chain_name), &[], None, 0).await?;
        let account = self.aave_account_changed(m, &now).await?;
        let got = (Decimal::from(token_balance(&rpc, m.usdc, &self.wallet.evm).await?) - Decimal::from(before)) / Decimal::from(1_000_000);
        Ok(format!("Borrowed {} USDC on Aave on {}: health factor {}{}, debt {}. Say “repay all USDC on Aave” to pay it back. {}/tx/{hash}",
                   Fmt::num(if got > Decimal::ZERO { got } else { usdc }, 2), m.chain_name, health(&account), liquidation_words(","),
                   Fmt::usd(account.debt), explorer(m)))
    }

    /// Pays back `usdc` of the USDC debt on Aave (None = all of it, interest included) from the USDC on that network: an
    /// approve of what's owed (a tenth of a percent over, for the interest of the next blocks), then the repay.
    pub async fn aave_repay(&self, m: &aave::Market, usdc: Option<Decimal>) -> Result<String> {
        let rpc = rpc_of(m.chain_id)?;
        let Some(debt_token) = address_in(&rpc.read(m.pool, &aave::get_reserve_data(m.usdc)?).await?, 10) else {
            return fail(format!("Aave's USDC reserve on {} didn't answer. Nothing was signed.", m.chain_name));
        };
        let owed = self.balance(&debt_token, &rpc).await?;
        if owed <= Decimal::ZERO { return fail(format!("No USDC debt on Aave on {}.", m.chain_name)) }
        let units = match usdc { Some(u) => num((Num::of(u) * 1_000_000).rounded(0, Rounding::Down))?.min(owed), None => owed };
        let approve = if usdc.is_none() { num((Num::of(owed) + owed / Decimal::from(1000) + 10).rounded(0, Rounding::Down))? } else { units };
        let held = Decimal::from(token_balance(&rpc, m.usdc, &self.wallet.evm).await?);
        let was = self.aave_account(m).await?;
        if held < approve {
            return fail(format!("{} USDC on {}. Paying back {} needs it there first. Nothing was signed.", Fmt::num(millionths(held), 2), m.chain_name,
                                Fmt::num(millionths(approve), 2)));
        }
        let repay = aave::repay(m.usdc, if usdc.is_none() { None } else { Some(uint64(units)) }, &self.wallet.evm)?;
        let what = if usdc.is_none() { "all the".to_string() } else { Fmt::num(millionths(units), 2) };
        let hash = self.approve_and_send(&rpc, m.usdc, m.pool, u256(approve)?, &format!("Approve {} USDC to Aave", Fmt::num(millionths(approve), 2)), m.pool,
                                         &repay, &format!("Repay {what} USDC debt on Aave on {}", m.chain_name), &[], 0).await?;
        let now = self.aave_account_changed(m, &was).await?;
        // Aave counts the debt in dollars on its oracle (USDC ≈ $0.9999): said as dollars.
        Ok(format!("Paid back {} of the USDC debt on Aave on {}: {} {}/tx/{hash}", Fmt::usd((was.debt - now.debt).max(Decimal::ZERO)), m.chain_name,
                   if now.debt > Decimal::ZERO { format!("debt {}, health factor {}.", Fmt::usd(now.debt), health(&now)) } else { "no debt left.".into() },
                   explorer(m)))
    }

    /// ETH as collateral on Aave: wrapped into the network's WETH, an exact approve, the supply — to this wallet.
    pub async fn aave_supply_eth(&self, m: &aave::Market, eth: Decimal) -> Result<String> {
        let Some((_, weth)) = uniswap::v2(m.chain_id) else { return fail(format!("No WETH ihatecrypto knows on {}.", m.chain_name)) };
        let wei = num((Num::of(eth) * Decimal::from(1_000_000_000_000_000_000u64)).rounded(0, Rounding::Down))?;
        let Some(value) = wei.to_u64().filter(|v| wei > Decimal::ZERO && Decimal::from(*v) == wei) else { return fail("Say how much ETH, under 18.") };
        let rpc = rpc_of(m.chain_id)?;
        let have = rpc.balance(&self.wallet.evm).await?;
        if have <= value as u128 {
            return fail(format!("{} ETH on {}: its fees come out of it too. Nothing was signed.", Fmt::num(coin(have), 6), m.chain_name));
        }
        self.send_evm(&rpc, weth, &[0xd0, 0xe3, 0x0d, 0xb0], &format!("Wrap {} ETH", Fmt::num(eth, 6)), &[], None, value).await?;
        let hash = self.approve_and_send(&rpc, weth, m.pool, u256(wei)?, &format!("Approve exactly {} WETH to Aave", Fmt::num(eth, 6)), m.pool,
                                         &aave::supply(weth, value, &self.wallet.evm)?,
                                         &format!("Supply {} ETH as collateral on Aave on {}", Fmt::num(eth, 6), m.chain_name), &[], 0).await?;
        let now = self.aave_account(m).await?;
        Ok(format!("Supplied {} ETH as collateral on Aave on {}: {} of collateral, {} borrowable. {}/tx/{hash}", Fmt::num(eth, 6), m.chain_name,
                   Fmt::usd(now.collateral), Fmt::usd(now.available), explorer(m)))
    }
}

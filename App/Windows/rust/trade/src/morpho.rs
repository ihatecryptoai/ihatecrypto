//! A Morpho vault on any network ihatecrypto knows, like Aave (Morpho.swift): USDC missing there comes from Solana over
//! Relay; a withdrawal goes back to Solana. Also the vault's rate from Morpho's API and MorphoVault's two reads.

use std::time::Duration;

use evm::abi::{address_in, uint_in};
use evm::protocols::morpho;
use evm::{U256, erc20};
use ihatecrypto_core::foundation::{Num, Rounding, rounded};
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use serde_json::{Value, json};

use crate::aave::{EvmNetwork, rpc_of};
use crate::executor::uint64;
use crate::{Error, Executor, Result, fail, json_decimal, num, token_balance};

/// The vault's net rate (after its fee) from Morpho's public API (api.morpho.org, no key); None when it doesn't answer.
pub struct MorphoRates;

impl MorphoRates {
    /// The vault's net APY (after its fee), in %.
    pub async fn net_apy(v: &morpho::Vault) -> Option<Decimal> {
        let query = format!("{{ vaultByAddress(address: \"{}\", chainId: {}) {{ state {{ netApy }} }} }}", v.address, v.chain_id);
        let (data, _) = net::post_json("https://api.morpho.org/graphql", &json!({"query": query})).await.ok()?;
        let root: Value = serde_json::from_slice(&data).ok()?;
        let apy = json_decimal(root.pointer("/data/vaultByAddress/state/netApy")?)?;
        Some(rounded((Num::of(apy) * 100).0?, 2, Rounding::Plain))
    }
}

/// MorphoVault.verify: the vault must hold its network's USDC; otherwise nothing is signed.
pub async fn morpho_verify(v: &morpho::Vault) -> Result<()> {
    let asset = address_in(&rpc_of(v.chain_id)?.read(v.address, &morpho::ASSET).await?, 0);
    if asset.as_deref() != Some(v.usdc) {
        return Err(Error::Wallet(format!("The Morpho vault on {} doesn't hold USDC ({}). Nothing was signed.", v.chain_name, asset.as_deref().unwrap_or("?"))));
    }
    Ok(())
}

/// MorphoVault.value: the owner's USDC in the vault (6 decimals).
pub async fn morpho_value(owner: &str, v: &morpho::Vault) -> Result<u64> {
    Ok(uint_in(&rpc_of(v.chain_id)?.read(v.address, &morpho::max_withdraw(owner)?).await?, 0).unwrap_or(0))
}

fn units(d: Decimal) -> Result<u64> { Ok(uint64(num((Num::of(d) * 1_000_000).rounded(0, Rounding::Down))?)) }
fn show(u: u64) -> String { Fmt::num(Decimal::from(u) / Decimal::from(1_000_000), 6) }

impl Executor {
    /// A vault deposit or redeem is ≈ 450k gas — several times an Aave supply — so Base keeps more ETH for it: 0.000012 ETH
    /// covers one at twice today's fee, and $0.05 of ETH lands above that.
    // ponytail: fixed floor and top-up; derive them from the estimate and the ETH price if Base fees jump.
    pub const MORPHO_GAS: (u128, Decimal) = (12_000_000_000_000, Decimal::from_parts(5, 0, 0, false, 2));

    pub async fn morpho(&self, deposit: bool, usdc: Option<Decimal>, v: &morpho::Vault) -> Result<String> {
        let Some(net) = EvmNetwork::named(v.chain) else { return fail(format!("ihatecrypto doesn't move money on {}. Nothing was signed.", v.chain_name)) };
        let (rpc, vault, token, me) = (&net.rpc, v.address, v.usdc, self.wallet.evm.as_str());
        // The vault's asset is re-read from its own network before anything is signed.
        morpho_verify(v).await?;

        if !deposit {
            let inside = morpho_value(me, v).await?;
            if inside == 0 { return fail(format!("Nothing is in the Morpho vault on {}.", v.chain_name)) }
            // Everything: redeem every share, so no dust stays; a part: exactly that much USDC.
            let part = usdc.map(units).transpose()?;
            let data = match part {
                Some(part) if part < inside => morpho::withdraw(part, me, me)?,
                _ => {
                    let shares = rpc.read(vault, &erc20::balance_of(me)?).await?;
                    let Some(word) = shares.get(..32).and_then(|w| <[u8; 32]>::try_from(w).ok()) else {
                        return fail(format!("The Morpho vault on {} didn't say how many shares this wallet has. Nothing was signed.", v.chain_name));
                    };
                    morpho::redeem(&word, me, me)?
                }
            };
            self.ensure_evm(net, 0, Some(Self::MORPHO_GAS)).await?;
            let before = token_balance(rpc, token, me).await?;
            self.send_evm(rpc, vault, &data, &format!("Morpho withdraw {} USDC on {}", usdc.map_or("all".into(), |r| Fmt::num(r, 2)), v.chain_name), &[], None, 0)
                .await?;
            return self.bridge_back(net, before, part.map_or(inside, |p| p.min(inside))).await;
        }

        let Some(requested) = usdc.filter(|r| *r > Decimal::ZERO) else { return fail("Say how much, e.g. “deposit 2 USDC into Morpho”.") };
        let want = units(requested)?;
        self.ensure_evm(net, want, Some(Self::MORPHO_GAS)).await?;
        let before = morpho_value(me, v).await?;
        let hash = self.approve_and_send(rpc, token, vault, U256::from(want), &format!("Approve exactly {} USDC to Morpho on {}", show(want), v.chain_name),
                                         vault, &morpho::deposit(want, me)?, &format!("Morpho deposit {} USDC on {}", show(want), v.chain_name), &[], 0).await?;
        // A node can answer from a block before the receipt's for a moment: give it up to ten seconds.
        let mut after = morpho_value(me, v).await?;
        for _ in 0..10 {
            if after > before { break }
            tokio::time::sleep(Duration::from_secs(1)).await;
            after = morpho_value(me, v).await?;
        }
        if after <= before { return fail(format!("The deposit went through but the vault doesn't show it. {}/tx/{hash}", net.explorer)) }
        let apy = MorphoRates::net_apy(v).await;
        Ok(format!("Deposited {} USDC into Morpho · {} on {}{}: {} USDC there now. {}/tx/{hash}", show(want), v.name, v.chain_name,
                   apy.map(|a| format!(" at ≈ {}% a year", Fmt::num(a, 2))).unwrap_or_default(), show(after), net.explorer))
    }
}

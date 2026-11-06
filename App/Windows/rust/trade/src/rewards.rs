//! Rewards on Merkl claimed to this wallet (Rewards.swift): on each chain, one Distributor call paying every token owed
//! there. The proofs are asked for again right before signing and must be for the root the Distributor holds; the signer
//! checks every reward is paid to this wallet, and each token's balance must grow. Merkl's two network reads
//! (`Merkl.rewards`, `Merkl.root`) are here: the evm crate reads its answers offline.

use std::time::Duration;

use evm::erc20;
use evm::protocols::merkl::{self, Reward};
use ihatecrypto_core::foundation::Num;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::SpotAsset;
use net::evm_rpc::EvmRpc;
use rust_decimal::Decimal;
use wallet::to_hex;

use crate::aave::EvmNetwork;
use crate::executor::url_error;
use crate::wallet::{millionths, sleep, usd};
use crate::{Error, Executor, Result, fail, num};

/// What `user` can claim now on `chains`; a reward claimed in full isn't listed.
async fn merkl_rewards(user: &str, chains: &[u64]) -> Result<Vec<Reward>> {
    let mut out = vec![];
    for chain in chains {
        let url = format!("https://api.merkl.xyz/v4/users/{user}/rewards?chainId={chain}");
        let response = net::HTTP.get(&url).header("Accept", "application/json").timeout(Duration::from_secs(15)).send().await.map_err(url_error)?;
        let status = response.status().as_u16();
        let data = response.bytes().await.map_err(url_error)?;
        if status != 200 { return Err(Error::Wallet(format!("Merkl answered {status} for chain {chain}."))) }
        out.extend(merkl::parse(&data, user)?);
    }
    Ok(out)
}

/// The root the Distributor holds on this chain now (`getMerkleRoot()`): proofs for another one don't claim.
async fn merkl_root(rpc: &EvmRpc) -> Result<String> {
    let data = rpc.read(merkl::DISTRIBUTOR, &merkl::ROOT_CALL).await?;
    if data.len() < 32 { return Err(Error::Wallet(format!("The Merkl distributor gave no root on chain {}.", rpc.chain_id))) }
    Ok(format!("0x{}", to_hex(&data[..32])))
}

impl Executor {
    pub async fn claim_rewards(&self) -> Result<String> {
        let me = self.wallet.evm.as_str();
        let owed = merkl_rewards(me, &merkl::CHAINS).await?;
        if owed.is_empty() { return fail("Nothing to claim on Merkl for this wallet. Nothing was signed.") }
        let (mut lines, mut claimed) = (Vec::<String>::new(), 0);
        for chain in merkl::CHAINS {
            let here: Vec<Reward> = owed.iter().filter(|r| r.chain_id == chain).cloned().collect();
            if here.is_empty() { continue }
            let Some(net) = EvmNetwork::of(chain) else { continue };
            let names = here.iter().map(|r| format!("{} {}", Fmt::num(r.claimable(), 6), r.symbol)).collect::<Vec<_>>().join(", ");
            // Merkl moves its root every few hours: proofs for another one would revert.
            let root = merkl_root(&net.rpc).await?;
            if !here.iter().all(|r| r.root.to_lowercase() == root.to_lowercase()) {
                lines.push(format!("{}: Merkl is updating its rewards right now. Ask again in a few hours. Nothing was signed there.", net.name));
                continue;
            }
            // A claim is ≈ 265k gas for two tokens (Base, simulated 2026-09-30): Morpho's heavier floor covers it.
            self.ensure_evm(net, 0, Some(Self::MORPHO_GAS)).await?;
            let before = self.reward_balances(&net.rpc, &here, me).await?;
            let hash = self.send_evm(&net.rpc, merkl::DISTRIBUTOR, &merkl::claim(me, &here)?, &format!("Claim {names} from Merkl on {}", net.name), &[], None, 0).await?;
            // A node can answer from a block before the receipt's for a moment: give it up to ten seconds.
            let mut after = self.reward_balances(&net.rpc, &here, me).await?;
            for _ in 0..10 {
                if !before.iter().zip(&after).any(|(b, a)| b >= a) { break }
                sleep(1).await;
                after = self.reward_balances(&net.rpc, &here, me).await?;
            }
            if !before.iter().zip(&after).all(|(b, a)| b < a) {
                return fail(format!("The claim on {} went through but a balance didn't grow. {}/tx/{hash}", net.name, net.explorer));
            }
            claimed += 1;
            lines.push(format!("Claimed {names} on {} to this wallet. {}/tx/{hash}", net.name, net.explorer));
        }
        if claimed == 0 { return fail(lines.join("\n")) }
        Ok(lines.join("\n"))
    }

    /// «sweep rewards over $10 into USDC»: once Merkl's rewards together are worth the floor (merkl::sweep_floor), they're
    /// claimed, and each worth a dollar or more is sold for USDC on Solana; `into` a token, that USDC then buys it. Under
    /// the floor nothing is signed and the line says so, so a schedule of it just waits for its next run. A token that
    /// can't be sold doesn't stop the others.
    pub async fn sweep_rewards(&self, min_usd: Option<Decimal>, into: Option<&str>) -> Result<String> {
        let floor = merkl::sweep_floor(min_usd);
        let owed = merkl_rewards(&self.wallet.evm, &merkl::CHAINS).await?;
        let worth = num(owed.iter().filter_map(Reward::worth_usd).fold(Num::of(Decimal::ZERO), |sum, w| sum + w))?;
        if worth < floor { return Ok(format!("Merkl rewards ≈ {}, under {}: nothing claimed yet, the gas would eat them.", usd(worth), usd(floor))) }
        let mut lines = vec![self.claim_rewards().await?];
        let usdc_mint = SpotAsset::usdc().mint;
        for r in merkl::to_sell(&owed, into) {
            let Some(net) = EvmNetwork::of(r.chain_id) else { continue };
            let sold: Result<()> = async {
                let before = self.rpc.token_balance(&self.wallet.solana, &usdc_mint).await?;
                lines.push(self.sell_on(net, &r.token, Some(r.claimable()), None, false, None).await?);
                let Some(into) = into else { return Ok(()) };
                let after = self.rpc.token_balance(&self.wallet.solana, &usdc_mint).await?;
                lines.push(self.buy_on(net, into, millionths(after.saturating_sub(before)), None, None, None).await?);
                Ok(())
            }.await;
            if let Err(e) = sold { lines.push(format!("{} on {} stays as it is: {e}", r.symbol, net.name)) }
        }
        Ok(lines.join("\n"))
    }

    /// Each reward token's balance, in its smallest units (256-bit: an 18-decimal token passes 64 bits at 18.4 tokens).
    async fn reward_balances(&self, rpc: &EvmRpc, rewards: &[Reward], owner: &str) -> Result<Vec<Decimal>> {
        let mut out = vec![];
        for r in rewards {
            let data = rpc.read(&r.token, &erc20::balance_of(owner)?).await?;
            let Some(units) = crate::word(&data, 0) else { return fail(format!("{} gave no balance on chain {}.", r.symbol, rpc.chain_id)) };
            out.push(units);
        }
        Ok(out)
    }
}

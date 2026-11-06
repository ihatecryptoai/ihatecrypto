//! USDC between EVM networks through Circle's CCTP v2 with its Forwarding Service, beside Relay and 1Click
//! (CCTPBridge.swift): one signature on the source (an exact approve to the TokenMessenger and the burn, one 7702
//! transaction where it batches), then Circle attests and mints to this wallet on the destination itself — no gas
//! needed there, no address to allow. The bridge takes it only when it lands more than Relay.

use std::time::{Duration, Instant};

use evm::U256;
use evm::protocols::cctp;
use execution::Kind;
use ihatecrypto_core::foundation::{Num, Rounding};
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::aave::EvmNetwork;
use crate::executor::uint64;
use crate::{Executor, Result, fail, json_decimal};

fn millionths(u: u64) -> Decimal { Decimal::from(u) / Decimal::from(1_000_000) }

async fn get(url: &str) -> Option<Value> { serde_json::from_slice(&net::get(url, &[]).await.ok()?.0).ok() }

impl Executor {
    /// What CCTP charges now for `units` of USDC between two networks — the cap the burn carries (`max_fee`, a fifth over
    /// the quoted fees, so a rise between quote and block still forwards) — and what lands. None off CCTP's networks or
    /// when Circle doesn't answer.
    pub async fn cctp_quote(&self, from: &EvmNetwork, to: &EvmNetwork, units: u64) -> Option<(u64, Decimal)> {
        let (a, b) = (cctp::domain(from.rpc.chain_id)?, cctp::domain(to.rpc.chain_id)?);
        if a == b { return None }
        Self::cctp_fees(&get(&format!("{}/v2/burn/USDC/fees/{a}/{b}?forward=true", cctp::IRIS)).await?, units)
    }

    /// Circle's fee tiers read for `units`: the fast tier's basis points and its forward's median fee.
    pub(crate) fn cctp_fees(tiers: &Value, units: u64) -> Option<(u64, Decimal)> {
        let tiers = tiers.as_array()?;
        if !tiers.iter().all(Value::is_object) { return None }
        let tier = tiers.iter().find(|t| t.get("finalityThreshold").and_then(Value::as_u64) == Some(cctp::FAST as u64))?;
        let bps = json_decimal(tier.get("minimumFee")?)?;
        let forward = json_decimal(tier.get("forwardFee").filter(|f| f.is_object())?.get("med")?)?;
        let fees = (Num::of(Decimal::from(units)) * bps / 10_000 + forward).0?;
        let cap = uint64((Num::of(fees) * Decimal::new(12, 1)).rounded(0, Rounding::Up).0?);
        if cap >= units { return None }
        Some((cap, (Num::of(Decimal::from(units)) - Num::of(fees).rounded(0, Rounding::Up)).0? / Decimal::from(1_000_000)))
    }

    /// Circle's word on a burn: attested, and the mint it broadcast (its forward) — None while there's none, or when the
    /// API doesn't answer.
    pub async fn cctp_forward(source: u32, burn: &str) -> Option<String> {
        let answer = get(&format!("{}/v2/messages/{source}?transactionHash={burn}", cctp::IRIS)).await?;
        let message = answer.get("messages")?.as_array().filter(|m| m.iter().all(Value::is_object))?.first()?.clone();
        message.get("forwardTxHash")?.as_str().map(str::to_string)
    }

    /// The move: the burn on `from`, Circle's mint on `to` followed in its API, then the USDC on the balance there.
    /// Returns what arrived.
    pub async fn cctp_move(&self, from: &EvmNetwork, to: &EvmNetwork, units: u64, max_fee: u64) -> Result<Decimal> {
        let (Some(source), Some(destination)) = (cctp::domain(from.rpc.chain_id), cctp::domain(to.rpc.chain_id)) else {
            return fail("Circle's CCTP moves USDC between Ethereum, Arbitrum, Base and Polygon.");
        };
        let me = self.wallet.evm.as_str();
        let label = format!("{} USDC", Fmt::num(millionths(units), 2));
        self.ensure_evm(from, units, None).await?;
        let before = to.usdc_balance(me).await?;
        let burn = self.approve_and_send(&from.rpc, from.usdc, cctp::TOKEN_MESSENGER, U256::from(units), &format!("Approve exactly {label} to Circle's CCTP"),
                                         cctp::TOKEN_MESSENGER, &cctp::burn(units, destination, me, from.usdc, max_fee)?,
                                         &format!("CCTP: burn {label} on {}, minted on {}", from.name, to.name), &[], 0).await?;
        let id = format!("{source}:{burn}");
        self.record("cctp", &id, Kind::Signed, &format!("{} USDC→{} {} (CCTP), tx {burn}", from.name, to.name, Fmt::num(millionths(units), 2)))?;
        let started = Instant::now();
        let mut mint = None;
        while mint.is_none() && started.elapsed() < Duration::from_secs(900) {
            mint = Self::cctp_forward(source, &burn).await;
            if mint.is_none() { tokio::time::sleep(Duration::from_secs(3)).await }
        }
        let Some(mint) = mint else {
            return fail(format!("Circle hasn't minted on {} within 15 minutes; the burn is {}/tx/{burn}, in transit on the portfolio.", to.name, from.explorer));
        };
        self.report(&format!("CCTP · Circle minted on {} in {} s. {}/tx/{mint}", to.name, started.elapsed().as_secs(), to.explorer));
        let mut after = to.usdc_balance(me).await?;
        for _ in 0..30 {
            if after > before { break }
            tokio::time::sleep(Duration::from_secs(2)).await;
            after = to.usdc_balance(me).await?;
        }
        self.record("cctp", &id, Kind::Settled, &mint)?;
        Ok(millionths(after.saturating_sub(before)))
    }

    /// «move 500 USDC from base to arbitrum via CCTP»: CCTP whatever Relay would land.
    pub async fn cctp_bridge(&self, from: &EvmNetwork, to: &EvmNetwork, usdc: Option<Decimal>) -> Result<String> {
        let held = from.usdc_balance(&self.wallet.evm).await?;
        let units = match usdc {
            Some(u) => uint64(crate::num((Num::of(u) * 1_000_000).rounded(0, Rounding::Down))?),
            None => held,
        };
        if units == 0 || units > held { return fail(format!("{} USDC on {}. Nothing was signed.", Fmt::num(millionths(held), 2), from.name)) }
        let Some((max_fee, arrives)) = self.cctp_quote(from, to, units).await else {
            return fail(format!("Circle didn't quote CCTP from {} to {} now. Nothing was signed.", from.name, to.name));
        };
        self.report(&format!("Circle's CCTP: {} USDC from {} → ≈ {} on {}, minted by Circle.", Fmt::num(millionths(units), 2), from.name,
                             Fmt::num(arrives, 4), to.name));
        let started = Instant::now();
        let got = self.cctp_move(from, to, units, max_fee).await?;
        Ok(format!("Moved {} USDC from {} to {} through Circle's CCTP in {} s: {} USDC arrived.", Fmt::num(millionths(units), 2), from.name, to.name,
                   started.elapsed().as_secs(), Fmt::num(got, 2)))
    }
}

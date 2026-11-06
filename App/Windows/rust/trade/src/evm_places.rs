//! The USDC places on the EVM networks, for the planners (EVMPlaces.swift): their rate, what is in them, and what a
//! round trip there costs; and the USDC lying idle on each network.

use std::collections::HashMap;

use evm::protocols::{aave, morpho};
use ihatecrypto_core::foundation::Num;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::modules::ModuleKind;
use rust_decimal::Decimal;
use tokio::task::JoinSet;

use crate::aave::{EvmNetwork, aave_verify};
use crate::executor::hl_markets;
use crate::morpho::{MorphoRates, morpho_value, morpho_verify};
use crate::{Executor, modules, token_balance};

/// A USDC place on an EVM network, for the planners.
#[derive(Debug, Clone, PartialEq)]
pub struct EvmPlace {
    /// «Aave · Arbitrum», «Morpho · Ethereum».
    pub name: String,
    /// «Aave» or «Morpho»: the allocation limit is counted per protocol.
    pub protocol_name: String,
    pub apy: Decimal,
    /// USDC in it now.
    pub supplied: Decimal,
    /// Getting there and back: Relay both ways, the calls' gas at today's fee, and the delivery of a gas top-up when the
    /// network has none yet.
    pub round_trip_usd: Decimal,
}

impl EvmPlace {
    /// Why a slice worth `yearly` in interest isn't worth the trip, or None: the trip must cost less than half a year of it.
    pub fn too_costly(&self, yearly: Decimal) -> Option<String> {
        (self.round_trip_usd > yearly / Decimal::TWO).then(|| {
            format!("the bridges and gas there and back (≈ {}) cost more than half a year of this slice's interest (≈ {} a year)",
                    Fmt::usd(self.round_trip_usd), Fmt::usd(yearly))
        })
    }
}

fn millionths(u: u64) -> Decimal { Decimal::from(u) / Decimal::from(1_000_000) }

impl Executor {
    /// Gas a round trip takes: Aave's approve and supply, then its withdraw and Relay's approve and deposit back. A Morpho
    /// vault call is ≈ 450k gas each way.
    pub const AAVE_TRIP_GAS: u64 = 250_000 + 330_000;
    pub const MORPHO_TRIP_GAS: u64 = 500_000 + 580_000;

    /// Relay's fee each way: ≈ $0.03 to and from the L2s, more for Ethereum where the solver pays mainnet gas.
    // ponytail: fixed estimates; quote Relay per slice if the planners need them exact.
    pub fn bridge_usd(net: &EvmNetwork) -> Decimal { if net.key == "ethereum" { Decimal::new(5, 1) } else { Decimal::new(3, 2) } }

    /// Every Aave market and Morpho vault ihatecrypto knows, read at once. One whose rate, balance or gas can't be read is
    /// left out: nothing is planned into a place that wasn't checked.
    pub async fn evm_places(&self) -> Vec<EvmPlace> {
        let marks = hl_markets().await.unwrap_or_default();
        let prices: HashMap<&str, Option<Decimal>> = [("ETH", marks.get("ETH").map(|m| m.mark_px)), ("POL", marks.get("POL").map(|m| m.mark_px))].into();
        let price = |net: &EvmNetwork| prices.get(net.gas_coin).copied().flatten();
        let mut group = JoinSet::new();
        for m in &aave::MARKETS {
            let Some(net) = EvmNetwork::named(m.chain) else { continue };
            let (me, price) = (self.wallet.evm.clone(), price(net));
            group.spawn(async move {
                let apy = aave_verify(m).await.ok()?;
                let supplied = token_balance(&net.rpc, m.a_usdc, &me).await.ok()?;
                let trip = Self::round_trip(net, &me, Self::AAVE_TRIP_GAS, price).await?;
                Some(EvmPlace { name: m.place(), protocol_name: "Aave".into(), apy, supplied: millionths(supplied), round_trip_usd: trip })
            });
        }
        for v in &morpho::VAULTS {
            let Some(net) = EvmNetwork::named(v.chain) else { continue };
            let (me, price) = (self.wallet.evm.clone(), price(net));
            group.spawn(async move {
                morpho_verify(v).await.ok()?;
                let apy = MorphoRates::net_apy(v).await?;
                let supplied = morpho_value(&me, v).await.ok()?;
                let trip = Self::round_trip(net, &me, Self::MORPHO_TRIP_GAS, price).await?;
                Some(EvmPlace { name: v.place(), protocol_name: "Morpho".into(), apy, supplied: millionths(supplied), round_trip_usd: trip })
            });
        }
        let mut out: Vec<EvmPlace> = group.join_all().await.into_iter().flatten().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// USDC (USDG on Robinhood Chain) lying unused on each EVM network Settings › Modules allows, from half a dollar (a
    /// bridge's minimum); Ethereum last, so the cheaper networks are pulled from first. A network that can't be read
    /// counts as empty.
    pub async fn idle_usdc(&self) -> Vec<(&'static EvmNetwork, Decimal)> {
        if !modules().is_on(ModuleKind::Bridges) { return vec![] }
        let mut out = vec![];
        for net in EvmNetwork::searched() {
            let Ok(have) = net.usdc_balance(&self.wallet.evm).await else { continue };
            if have <= net.fee_reserve() { continue }
            let usdc = millionths(have - net.fee_reserve());
            if usdc >= Decimal::new(5, 1) { out.push((net, usdc)) }
        }
        out
    }

    /// What a round trip to `net` costs today, in USD; None when the fee or the gas coin's price can't be read.
    pub(crate) async fn round_trip(net: &EvmNetwork, owner: &str, gas: u64, price: Option<Decimal>) -> Option<Decimal> {
        let price = price.filter(|p| *p > Decimal::ZERO)?;
        let (tip, max_fee) = net.rpc.fees().await.ok()?;
        let have = net.rpc.balance(owner).await.ok()?;
        // fees() doubles the base fee for headroom; what a call pays today is the base fee plus the tip.
        let gas_price = max_fee.saturating_sub(tip) / 2 + tip;
        let gas_usd = Num::of(Decimal::from(gas)) * Decimal::from(gas_price) / Decimal::from(1_000_000_000_000_000_000u64) * price;
        // Without gas there yet, the first bridge carries a top-up: its delivery costs ≈ $0.08 on top (measured on Base).
        let topup = if have < net.gas_floor as u128 { Decimal::new(8, 2) } else { Decimal::ZERO };
        (Num::of(Decimal::TWO * Self::bridge_usd(net)) + gas_usd + topup).0
    }
}

//! Stablecoins for one another on Tempo through its own exchange (TempoSwap.swift): USDC (Stargate's USDC.e there),
//! pathUSD, OUSD and USDT0. An exact approve to the exchange, then the swap at no less than 0.1% under its quote; what
//! comes out pays this wallet. Fees come out of the USDC, as every Tempo call's do. Measured 2026-10-06: USDC ↔ OUSD
//! 1 bp and USDC ↔ pathUSD 2 bp, the same at $1000; USDT0 had no liquidity.

use evm::U256;
use evm::abi::uint_in;
use evm::tempo;
use execution::Kind;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::stablecoins;
use protocols::relay;
use rust_decimal::Decimal;

use crate::aave::EvmNetwork;
use crate::swap::{d, dollars, millionths, sleep};
use crate::{Executor, Result, fail, token_balance};

/// A dollar on Tempo: its symbol as said («pathUSD») and its contract.
pub type TempoStable = (String, &'static str);

/// Relay's swap on Tempo where Tempo's own exchange has no liquidity for the pair (tempo_relay).
#[derive(Debug, Clone, PartialEq)]
pub struct TempoRelay {
    pub units: u64,
    pub x: TempoStable,
    pub y: TempoStable,
    pub out: Decimal,
    pub quote: relay::Quote,
    pub call: relay::EvmStep,
    pub proxy: String,
}

impl Executor {
    pub fn tempo_stable(r: &str) -> Option<TempoStable> {
        let key = if r.to_uppercase() == "USDC.E" { "USDC".to_string() } else { stablecoins::find(r, "tempo").map_or(r, |c| c.symbol).to_uppercase() };
        tempo::STABLES.iter().find(|(k, _)| *k == key).map(|(_, address)| (if key == "PATHUSD" { "pathUSD".into() } else { key.clone() }, *address))
    }

    /// What goes in: the amount said, or all of it (all of USDC leaves a cent for fees).
    fn tempo_units(amount: Option<Decimal>, x: &TempoStable, held: u64) -> Result<u64> {
        match amount { Some(a) => millionths(a), None => Ok(if x.0 == "USDC" { held - held.min(10_000) } else { held }) }
    }

    /// What the exchange gives now for `amount` of one dollar in another (None = all of it; all of USDC leaves a cent
    /// for fees). Nothing is signed. `quote_only`: for the card, priced at the amount asked even when the wallet holds
    /// less. (units, out, from, to)
    pub async fn tempo_quote(&self, a: &str, b: &str, amount: Option<Decimal>, quote_only: bool) -> Result<(u64, u64, String, String)> {
        let (Some(x), Some(y)) = (Self::tempo_stable(a), Self::tempo_stable(b)) else {
            return fail("Tempo's exchange trades USDC, pathUSD, OUSD and USDT0 for one another.");
        };
        if x.1 == y.1 { return fail("Tempo's exchange trades USDC, pathUSD, OUSD and USDT0 for one another.") }
        let rpc = &EvmNetwork::tempo().rpc;
        let held = token_balance(rpc, x.1, &self.wallet.evm).await?;
        let units = Self::tempo_units(amount, &x, held)?;
        if units == 0 { return fail(format!("No {} on Tempo to swap.", x.0)) }
        if !(units <= held || quote_only && amount.is_some()) {
            return fail(format!("{} {} on Tempo: {} is more. Nothing was signed.", Fmt::num(dollars(held), 2), x.0, Fmt::num(dollars(units), 2)));
        }
        let no_liquidity = || fail(format!("Tempo's exchange has no {} → {} liquidity for {} now. Nothing was signed.", x.0, y.0, Fmt::num(dollars(units), 2)));
        let Ok(call) = tempo::quote(x.1, y.1, units) else { return no_liquidity() };
        let Ok(answer) = rpc.read(tempo::EXCHANGE, &call).await else { return no_liquidity() };
        let Some(out) = uint_in(&answer, 0).filter(|o| *o > 0) else { return fail(format!("Tempo's exchange didn't quote {} → {}.", x.0, y.0)) };
        Ok((units, out, x.0, y.0))
    }

    /// Relay's swap on Tempo where Tempo's own exchange has no liquidity for the pair (USDT0, 2026-10-08): what goes in,
    /// in units, and Relay's checked quote. None when Relay doesn't quote it either.
    pub async fn tempo_relay(&self, a: &str, b: &str, amount: Option<Decimal>, quote_only: bool) -> Result<Option<TempoRelay>> {
        let (Some(x), Some(y)) = (Self::tempo_stable(a), Self::tempo_stable(b)) else { return Ok(None) };
        if x.1 == y.1 { return Ok(None) }
        let held = token_balance(&EvmNetwork::tempo().rpc, x.1, &self.wallet.evm).await?;
        let units = Self::tempo_units(amount, &x, held)?;
        if !(units > 0 && (units <= held || quote_only && amount.is_some())) { return Ok(None) }
        let Ok(r) = self.relay_pair_quote(EvmNetwork::tempo(), x.1, y.1, Decimal::from(units), !quote_only).await else { return Ok(None) };
        Ok(Some(TempoRelay { units, x, y, out: r.out, quote: r.quote, call: r.call, proxy: r.proxy }))
    }

    pub async fn tempo_swap(&self, a: &str, b: &str, amount: Option<Decimal>) -> Result<String> {
        let me = self.wallet.evm.as_str();
        let net = EvmNetwork::tempo();
        let q = match self.tempo_quote(a, b, amount, false).await {
            Ok(q) => q,
            Err(error) => {
                let Some(r) = self.tempo_relay(a, b, amount, false).await? else { return Err(error) };
                let what = format!("{} {}", Fmt::num(dollars(r.units), 6), r.x.0);
                self.report(&format!("{what} → {} on Tempo through Relay's swap (Tempo's exchange has no liquidity for it): ≈ {} {}.", r.y.0,
                                     Fmt::num(r.out / Decimal::from(1_000_000), 6), r.y.0));
                let before = token_balance(&net.rpc, r.y.1, me).await?;
                let hash = self.approve_and_send(&net.rpc, r.x.1, &r.proxy, U256::from(r.units), &format!("Approve exactly {what} to Relay"), &r.call.to, &r.call.data,
                                                 &format!("Relay: swap {what} for {} on Tempo", r.y.0), &[], 0).await?;
                self.record("relay", &r.quote.request_id, Kind::Signed, &format!("Tempo {}→{}, tx {hash}", r.x.0, r.y.0))?;
                let mut after = before;
                for _ in 0..20 {
                    if after > before { break }
                    sleep(1).await;
                    after = token_balance(&net.rpc, r.y.1, me).await?;
                }
                return Ok(format!("Swapped {what} for {} {} on Tempo through Relay's swap. {}/tx/{hash}", Fmt::num(dollars(after.saturating_sub(before)), 6), r.y.0,
                                  net.explorer));
            }
        };
        let (Some(x), Some(y)) = (Self::tempo_stable(a), Self::tempo_stable(b)) else { return fail("Unknown dollar on Tempo.") };
        let (units, out, from, to) = q;
        if Decimal::from(out) < Decimal::from(units) * d("0.99") {
            return fail(format!("Tempo's exchange gives {} {to} for {} {from}: more than 1% off a dollar. Nothing was signed.", Fmt::num(dollars(out), 2),
                                Fmt::num(dollars(units), 2)));
        }
        let what = format!("{} {from}", Fmt::num(dollars(units), 6));
        let before = token_balance(&net.rpc, y.1, me).await?;
        self.approve(&net.rpc, x.1, tempo::EXCHANGE, U256::from(units), &format!("Approve exactly {what} to Tempo's exchange"), &[]).await?;
        let hash = self.send_evm(&net.rpc, tempo::EXCHANGE, &tempo::swap(x.1, y.1, units, out - out / 1000)?, &format!("Swap {what} for {to} on Tempo"), &[], None, 0)
            .await?;
        let after = token_balance(&net.rpc, y.1, me).await?;
        Ok(format!("Swapped {what} for {} {to} on Tempo's exchange. {}/tx/{hash}", Fmt::num(dollars(after.saturating_sub(before)), 6), net.explorer))
    }
}

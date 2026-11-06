//! USDT between networks over Tether's own bridge, USDT0 on LayerZero (evm::protocols::usdt0), beside Relay's swap
//! (USDT0Bridge.swift): one call on the source — after an exact approve on Ethereum, where the adapter locks the USDT —
//! and the same amount is minted on the other side at this wallet; LayerZero's message fee rides with the call in the
//! source's coin. Free but slow (20–35 minutes from Ethereum, Arbitrum and Optimism, half a day from HyperEVM), so it
//! goes only when asked for, with the wait and Relay's price beside it on the card. Also every bridge's line for USDC.

use std::time::{Duration, Instant};

use evm::abi::{u256_in, uint_in};
use evm::protocols::usdt0::{self, Deployment};
use evm::{U256, erc20};
use execution::Kind;
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded};
use ihatecrypto_core::intent::Fmt;
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::executor::uint64;
use crate::{EvmNetwork, Executor, Result, fail, num, token_balance};

/// A bridge that can make a move, as its card lists it: what it lands, what it costs and how long it takes («≈ 99.97
/// USDC · $0.03 · ≈ 4 s»), and what lands in dollars with every fee off.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeWay {
    pub way: String,
    pub gets: String,
    pub worth: Decimal,
}

impl BridgeWay {
    fn new(way: &str, gets: String, worth: Decimal) -> Self { Self { way: way.into(), gets, worth } }
}

fn millionths(units: u64) -> Decimal { Decimal::from(units) / Decimal::from(1_000_000) }

/// Millionths of a stated amount, rounded down (`NSDecimalNumber(decimal: (x * 1_000_000).rounded(0, .down)).uint64Value`).
fn micro(x: Decimal) -> Result<u64> { Ok(uint64(rounded(num(Num::of(x) * 1_000_000)?, 0, Rounding::Down))) }

fn show(u: u64) -> String { Fmt::num(millionths(u), 6) }

fn sleep(seconds: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_secs(seconds)) }

impl Executor {
    /// What the bridge takes now for `units` (millionths) of USDT: what lands, LayerZero's fee in the source's coin — wei
    /// to send, a twentieth over the quote so a rise before the block still delivers (the rest comes back) — and that
    /// fee in dollars. None off its networks, outside its limits, or when a node doesn't answer.
    pub async fn usdt0_quote(from: &EvmNetwork, to: &EvmNetwork, units: u64, recipient: &str) -> Option<(u64, u64, Decimal)> {
        let (a, b) = (usdt0::on(from.rpc.chain_id)?, usdt0::on(to.rpc.chain_id)?);
        if a.eid == b.eid || units == 0 { return None }
        let limits = from.rpc.read(a.oft, &usdt0::quote_oft(b.eid, recipient, units).ok()?).await.ok()?;
        let (least, most) = (u256_in(&limits, 0)?, u256_in(&limits, 1)?);
        if least > U256::from(units) || U256::from(units) > most { return None }
        let arrives = uint_in(&limits, 4)?;
        let fees = from.rpc.read(a.oft, &usdt0::quote_send(b.eid, recipient, units, arrives).ok()?).await.ok()?;
        let fee = uint_in(&fees, 0)?;
        let price = relay::native_price(from.rpc.chain_id).await.ok().flatten()?;
        let fee_usd = (Num::of(Decimal::from(fee)) / Decimal::from(1_000_000_000_000_000_000u64) * price).0?;
        Some((arrives, fee.checked_add(fee / 20)?, fee_usd))
    }

    /// LayerZero's word on a message by the transaction that sent it — "INFLIGHT", "CONFIRMING", "DELIVERED", "FAILED" —
    /// and the transaction that delivered it. None when its API doesn't answer or hasn't seen the transaction yet.
    pub async fn layer_zero_status(tx: &str) -> Option<(String, Option<String>)> {
        let (data, _) = net::get(&format!("https://scan.layerzero-api.com/v1/messages/tx/{tx}"), &[]).await.ok()?;
        let j: Value = serde_json::from_slice(&data).ok()?;
        let message = j.get("data")?.as_array().filter(|list| list.iter().all(Value::is_object))?.first()?;
        let status = message.get("status")?.get("name")?.as_str()?.to_string();
        Some((status, message.pointer("/destination/tx/txHash").and_then(Value::as_str).map(str::to_string)))
    }

    /// USDT held on a USDT0 network, and the amount a move takes of it (all of it when none is said), in millionths.
    async fn usdt0_amount(&self, a: &Deployment, net: &EvmNetwork, asked: Option<Decimal>) -> Result<(u64, u64)> {
        let held = token_balance(&net.rpc, a.token, &self.wallet.evm).await?;
        Ok((match asked { Some(x) => micro(x)?, None => held }, held))
    }

    /// The bridges that can move this USDT now, for the card — Relay's swap of the same token and Tether's USDT0 — each
    /// with what it lands, what it costs and how long it takes; the one the move goes by (`layer_zero`) first.
    pub async fn usdt_ways(&self, from: &EvmNetwork, to: &EvmNetwork, usdt: Option<Decimal>, layer_zero: bool) -> Result<Vec<BridgeWay>> {
        let (Some(a), Some(b)) = (usdt0::on(from.rpc.chain_id), usdt0::on(to.rpc.chain_id)) else { return Ok(vec![]) };
        let units = self.usdt0_amount(a, from, usdt).await?.0;
        let sent = millionths(units);
        if units == 0 { return Ok(vec![]) }
        let me = self.wallet.evm.as_str();
        let ask = relay::Ask {
            user: me.into(), recipient: me.into(), origin_chain: from.relay_chain, destination_chain: to.relay_chain, origin_currency: a.token.into(),
            destination_currency: b.token.into(), amount: units, ..Default::default()
        };
        let (own, relayed) = tokio::join!(Self::usdt0_quote(from, to, units, me), relay::quote(&ask));
        let mut ways = Vec::new();
        if let Some((arrives, _, fee_usd)) = own {
            let lands = millionths(arrives);
            ways.push(BridgeWay::new("layerzero", format!("{} USDT · {} · {}", Fmt::num(lands, 6), Self::cost(num(Num::of(sent) - lands + fee_usd)?),
                                                          usdt0::wait(a.minutes)), num(Num::of(lands) - fee_usd)?));
        }
        if let Ok(r) = relayed && let Some(out) = decimal(&r.amount_out_formatted) {
            ways.push(BridgeWay::new("relay", format!("≈ {} USDT · {} · ≈ {} s", Fmt::num(out, 4), Self::cost(num(Num::of(sent) - out)?), r.seconds.max(1)), out));
        }
        if !layer_zero { ways.reverse() }
        Ok(ways)
    }

    /// A bridge's cost for the card: under a cent it's said as it is («$0.0007»), not as «$0.00».
    pub fn cost(usd: Decimal) -> String {
        let usd = usd.max(Decimal::ZERO);
        if usd > Decimal::ZERO && usd < Decimal::new(1, 2) { format!("${}", Fmt::num(usd, 4)) } else { Fmt::usd(usd) }
    }

    /// «move 50 USDT from arbitrum to optimism»: Relay's swap of the same USDT — or the USDT0 bridge, when asked by name.
    pub async fn usdt_move(&self, from: &EvmNetwork, to: &EvmNetwork, usdt: Option<Decimal>, layer_zero: bool) -> Result<String> {
        if layer_zero { return self.usdt0_bridge(from, to, usdt).await }
        if usdt0::on(from.rpc.chain_id).is_none() || usdt0::on(to.rpc.chain_id).is_none() {
            return fail("ihatecrypto moves USDT like this between Ethereum, Arbitrum, Optimism, Polygon and HyperEVM. Nothing was signed.");
        }
        // By its ticker, which Stablecoins knows as the real one: by its contract the token lookup weighed USD₮0's own
        // pools on Arbitrum ($44k) and called it too thin for Relay (2026-10-13). On Optimism «USDT» is the old bridged one.
        let ticker = |net: &EvmNetwork| if net.key == "optimism" { "USDT0" } else { "USDT" };
        self.cross_swap(ticker(from), from.key, ticker(to), to.key, usdt, None).await
    }

    /// «move 50 USDT from arbitrum to optimism via LayerZero»: the USDT0 bridge, whatever Relay would land.
    pub async fn usdt0_bridge(&self, from: &EvmNetwork, to: &EvmNetwork, usdt: Option<Decimal>) -> Result<String> {
        let Some((a, b)) = usdt0::on(from.rpc.chain_id).zip(usdt0::on(to.rpc.chain_id)).filter(|(a, b)| a.eid != b.eid) else {
            return fail("Tether's USDT0 bridge moves USDT between Ethereum, Arbitrum, Optimism, Polygon and HyperEVM. Nothing was signed.");
        };
        let (me, rpc) = (self.wallet.evm.as_str(), &from.rpc);
        let (units, held) = self.usdt0_amount(a, from, usdt).await?;
        if units == 0 || units > held { return fail(format!("{} USDT on {}. Nothing was signed.", show(held), from.name)) }
        let Some((arrives, fee, fee_usd)) = Self::usdt0_quote(from, to, units, me).await else {
            return fail(format!("The USDT0 bridge didn't quote {} USDT from {} to {} now. Nothing was signed.", show(units), from.name, to.name));
        };
        // Tether takes nothing today: a fee it turns on is said, and one past half a percent stops the move.
        if arrives > units || units - arrives > units / 200 {
            return fail(format!("The USDT0 bridge would land {} of {} USDT. Nothing was signed.", show(arrives), show(units)));
        }
        self.report(&format!("USDT0 on LayerZero: {} USDT from {} → {} on {}, its message fee {} in {}. It lands in {}.", show(units), from.name, show(arrives),
                             to.name, Self::cost(fee_usd), from.gas_coin, usdt0::wait(a.minutes)));
        // The fee is coins sent with the call, which a send never tops up by itself: gas comes over first when it's short.
        if rpc.balance(me).await? < fee as u128 + from.gas_floor as u128 {
            self.report(&format!("Not enough {} on {} for LayerZero's fee and the gas: bringing some first.", from.gas_coin, from.name));
            let thrice = (Num::of(fee_usd) * 3).0.unwrap_or(Decimal::MAX);
            self.top_up_gas(from, from.topup_usd.max(thrice).max(Decimal::new(1, 1)).min(Decimal::from(20))).await?;
        }
        if a.approves {
            // Ethereum's USDT changes an allowance only from zero.
            let allowance = || async { Ok::<_, crate::Error>(u256_in(&rpc.read(a.token, &erc20::allowance(me, a.oft)?).await?, 0).unwrap_or(U256::ZERO)) };
            let left = allowance().await?;
            if left > U256::ZERO && left < U256::from(units) {
                self.send_evm(rpc, a.token, &erc20::approve(a.oft, U256::ZERO)?, "Clear the USDT0 bridge's old allowance", &[], None, 0).await?;
                let mut waited = 0;
                while allowance().await? > U256::ZERO && waited < 15 {
                    sleep(1).await;
                    waited += 1;
                }
            }
            self.approve(rpc, a.token, a.oft, U256::from(units), &format!("Approve exactly {} USDT to the USDT0 bridge", show(units)), &[]).await?;
        }
        let before = token_balance(&to.rpc, b.token, me).await?;
        let hash = self.send_evm(rpc, a.oft, &usdt0::send(b.eid, me, units, arrives, fee, me)?,
                                 &format!("USDT0: {} USDT from {} to {}", show(units), from.name, to.name), &[], None, fee).await?;
        self.record("layerzero", &hash, Kind::Signed, &format!("{} USDT→{} {} (USDT0), tx {hash}", from.name, to.name, show(units)))?;
        // Minutes away (from Polygon) it's waited for; a longer way stays in transit on the portfolio until it lands.
        let started = Instant::now();
        while a.minutes <= 3 && started.elapsed() < Duration::from_secs(300) {
            if token_balance(&to.rpc, b.token, me).await? > before {
                self.record("layerzero", &hash, Kind::Settled, "landed")?;
                return Ok(format!("Moved {} USDT from {} to {} over USDT0 in {} s: {} USDT arrived.", show(units), from.name, to.name,
                                  started.elapsed().as_secs(), show(arrives)));
            }
            sleep(5).await;
        }
        Ok(format!("Sent {} USDT from {} to {} over USDT0: {} USDT lands there in {}, in transit on the portfolio until then. https://layerzeroscan.com/tx/{hash}",
                   show(units), from.name, to.name, show(arrives), usdt0::wait(a.minutes)))
    }

    /// The bridges that can move this USDC between two EVM networks now, for the card, the one `bridge` would take
    /// first: Relay, unless Circle's CCTP or NEAR Intents' 1Click lands more by the margin worth their wait
    /// (one_click_edge) — the same rule, asked again before signing. A destination short of gas has Relay only: it
    /// brings the gas along.
    pub async fn bridge_ways(&self, net: &EvmNetwork, dest: &EvmNetwork, requested: Option<Decimal>) -> Result<Vec<BridgeWay>> {
        let me = self.wallet.evm.as_str();
        let have = net.usdc_balance(me).await?;
        let spendable = have.saturating_sub(net.fee_reserve());
        let units = match requested { Some(r) => micro(r)?.min(spendable), None => spendable };
        if units < 500_000 { return Ok(vec![]) }
        let sent = millionths(units);
        let stable = if dest.key == "robinhood" { "USDG" } else { "USDC" };
        let topup = if dest.fee_token.is_none() && dest.rpc.balance(me).await? < dest.gas_floor as u128 { Some(dest.topup_usd) } else { None };
        let quote = self.relay_quote(&relay::Ask {
            user: me.into(), recipient: me.into(), origin_chain: net.relay_chain, destination_chain: dest.relay_chain, origin_currency: net.usdc.into(),
            destination_currency: dest.usdc.into(), amount: units, topup_gas_usd: topup, units: Some(net.raw(units).to_string()), ..Default::default()
        }).await?;
        let relay_out = decimal(&quote.amount_out_formatted).unwrap_or_default();
        let extra = topup.unwrap_or_default();
        let relay = BridgeWay::new("relay", format!("≈ {} {stable} · {} · ≈ {} s{}", Fmt::num(relay_out, 4), Self::cost(num(Num::of(sent) - relay_out - extra)?),
                                                    quote.seconds.max(1), if topup.is_none() { "" } else { ", gas comes along" }),
                                   num(Num::of(relay_out) + extra)?);
        if topup.is_some() { return Ok(vec![relay]) }
        let (circle, intents) = tokio::join!(self.cctp_quote(net, dest, units), self.one_click_quote(Some(net.key), Some(dest.key), units));
        let mut others = Vec::new();
        if let Some((_, arrives)) = circle {
            others.push(BridgeWay::new("cctp", format!("≈ {} USDC · {} · ≈ 1 min", Fmt::num(arrives, 4), Self::cost(num(Num::of(sent) - arrives)?)), arrives));
        }
        if let Some(q) = intents && let Some(out) = decimal(&q.amount_out_formatted) {
            let took = q.time_estimate.map_or("minutes".into(), |t| format!("≈ {t} s"));
            others.push(BridgeWay::new("1click", format!("≈ {} USDC · {} · {took}", Fmt::num(out, 4), Self::cost(num(Num::of(sent) - out)?)), out));
        }
        // `bridge` asks CCTP first, then 1Click: the first that beats Relay by the margin goes.
        let edge = Self::one_click_edge(sent);
        let taken = others.iter().position(|w| (Num::of(w.worth) - relay_out).0.is_some_and(|more| more >= edge));
        let mut out: Vec<BridgeWay> = taken.map(|i| others.remove(i)).into_iter().collect();
        out.push(relay);
        out.extend(others);
        Ok(out)
    }
}

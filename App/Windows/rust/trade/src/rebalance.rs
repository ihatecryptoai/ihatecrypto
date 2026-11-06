//! «Переложи в лучшие ставки» (Rebalance.swift): where the USDC earns now, the plan (defi::rebalance::plan), and with
//! `execute` each move through Solana — out of one place, into the next — by the same checked paths as the single
//! commands.

use std::time::Duration;

use chrono::Utc;
use defi::jupiter_lend::JupiterLend;
use defi::kamino::Kamino;
use defi::rebalance::{self, Place, Plan};
use defi::yields;
use evm::protocols::{aave, morpho};
use ihatecrypto_core::foundation::{Num, Rounding, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::Hyperliquid;
use protocols::relay;
use rust_decimal::Decimal;

use crate::executor::hl_markets;
use crate::{Executor, HyperliquidAccount, Result, fail, modules, num};

fn moved(done: &[String]) -> String { if done.is_empty() { String::new() } else { format!(" Already moved: {}.", done.join(", ")) } }

impl Executor {
    /// The plan, as its card reads, and whether there's a move to make; with `execute` the moves made. `max_share`: % of
    /// the USDC one protocol may hold (Swift's default is 50).
    pub async fn rebalance(&self, max_share: Decimal, execute: bool) -> Result<(String, bool, Plan)> {
        let places = self.rebalance_places().await?;
        let plan = rebalance::plan(&places, max_share);
        let mut lines = vec![format!("USDC in {} places, at most {} per protocol. A move is made only if it pays back within 30 days:",
                                     places.iter().filter(|p| p.amount > Decimal::ZERO).count(), Fmt::pct(max_share))];
        lines.extend(plan.moves.iter().map(|m| format!("· {} → {}: {} USDC, {}% → {}%, +{} a year for ≈ {}", m.from, m.to, Fmt::num(m.amount, 2),
                                                       Fmt::num(rounded(m.from_apy, 2, Rounding::Plain), 2), Fmt::num(rounded(m.to_apy, 2, Rounding::Plain), 2),
                                                       Fmt::usd(m.yearly_gain()), Fmt::usd(m.cost))));
        lines.extend(plan.skipped.iter().map(|s| format!("· skipped {} → {}: {}", s.step.from, s.step.to, s.reason)));
        lines.extend(plan.notes.iter().map(|n| format!("· {n}")));
        if !execute || plan.moves.is_empty() {
            let ready = !plan.moves.is_empty();
            return Ok((lines.join("\n"), ready, plan));
        }
        self.report(&lines.join("\n"));
        let mut done: Vec<String> = Vec::new();
        let usdc = || self.rpc.token_balance(&self.wallet.solana, relay::currency::SOLANA_USDC);
        for (i, step) in plan.moves.iter().enumerate() {
            let name = format!("Step {} of {}, {} → {}", i + 1, plan.moves.len(), step.from, step.to);
            let before = usdc().await?;
            if let Err(e) = self.rebalance_withdraw(&step.from, step.amount, &places).await {
                return fail(format!("{name} stopped before any money left {}: {e}{}", step.from, moved(&done)));
            }
            // A bridge back delivers a little less than it took: deposit what actually arrived, never more than planned.
            let arrived = Decimal::from(i128::from(usdc().await?) - i128::from(before)) / Decimal::from(1_000_000);
            let piece = rounded(step.amount.min(if arrived > Decimal::ZERO { arrived } else { step.amount }), 2, Rounding::Down);
            if let Err(e) = self.rebalance_deposit(&step.to, piece).await {
                return fail(format!("{name} stopped after the withdrawal: {} USDC is on your Solana wallet, not in {}. {e}{}", Fmt::num(piece, 2), step.to,
                                    moved(&done)));
            }
            done.push(format!("{} USDC {} → {}", Fmt::num(piece, 2), step.from, step.to));
            self.report(&format!("{name}: done."));
        }
        let line = format!("Rebalanced: {}. About +{} a year for ≈ {} of costs.", done.join(", "), Fmt::usd(plan.yearly_gain()), Fmt::usd(plan.cost()));
        self.report(&line);
        Ok((line, false, plan))
    }

    /// Every place ihatecrypto can move USDC between, with what is in it, its rate and what getting out and in costs.
    pub(crate) async fn rebalance_places(&self) -> Result<Vec<Place>> {
        let (jupiter_lend, kamino_api) = (JupiterLend::default(), Kamino::default());
        let (options, vaults, jup_positions, kam_positions, evm, hlp, markets) = tokio::join!(
            yields::options(Some("USDC")), jupiter_lend.vaults(), jupiter_lend.positions(&self.wallet.solana), kamino_api.positions(&self.wallet.solana),
            self.evm_places(), HyperliquidAccount::vault_equity(&self.wallet.evm, guard::hl::HLP_VAULT), hl_markets(),
        );
        let usdc_vault = vaults.unwrap_or_default().into_iter().find(|v| v.asset_symbol.to_uppercase() == "USDC");
        let jup_positions = jup_positions.unwrap_or_default();
        let jupiter = usdc_vault.and_then(|v| jup_positions.iter().find(|p| p.vault == v.symbol))
            .map_or(Decimal::ZERO, |p| Decimal::from(p.assets) / Decimal::from(1_000_000));
        let kamino = num(kam_positions.unwrap_or_default().iter().filter(|p| p.symbol.to_uppercase() == "USDC")
            .fold(Num::of(Decimal::ZERO), |sum, p| sum + p.value_usd))?;
        let (hlp_equity, _, locked_until) = hlp.unwrap_or_default();
        let sol_price = markets.ok().and_then(|m| m.get("SOL").map(|s| s.mark_px)).unwrap_or_default();
        let apy = |place: &str| options.iter().find(|o| o.place == place).map(|o| o.apy);
        // Estimates, shown as such: Kamino's first deposit opens accounts for ≈ 0.03 SOL of rent at September's rate,
        // less as rent falls (kamino_first_rent; 0.0176 of it comes back on a full exit); an EVM place's round trip is
        // split half out, half in (evm_places.rs).
        let kamino_rent = if kamino > Decimal::ZERO { Decimal::ZERO } else { Self::kamino_first_rent(&self.rpc).await };
        let mut places = Vec::new();
        if let Some(a) = apy("Jupiter Lend") { places.push(Place::new("Jupiter Lend", a, jupiter)) }
        if let Some(a) = apy("Kamino Lend") {
            places.push(Place { entry_cost: num(Num::of(kamino_rent) * sol_price)?, ..Place::new("Kamino Lend", a, kamino) });
        }
        for p in evm {
            let half = p.round_trip_usd / Decimal::TWO;
            places.push(Place { exit_cost: half, entry_cost: half, protocol_name: p.protocol_name.clone(), ..Place::new(&p.name, p.apy, p.supplied) });
        }
        if let Some(a) = apy("HLP (Hyperliquid)").filter(|_| hlp_equity > Decimal::ZERO) {
            let locked = locked_until.filter(|u| *u > Utc::now()).map(|u| format!("HLP locks deposits for 4 days, until {}", Fmt::date(u)));
            places.push(Place {
                exit_cost: Decimal::new(2, 2), locked,
                closed: Some("HLP takes deposits by its own command («deposit 5 USDC to HLP»): $5 minimum and a 4-day lock".into()),
                ..Place::new("HLP (Hyperliquid)", a, hlp_equity)
            });
        }
        // A vault switched off in Settings › Modules is left alone: nothing moves into it or out of it.
        let modules = modules();
        Ok(places.into_iter().filter(|p| modules.refusal_vault(&p.name).is_none()).collect())
    }

    async fn rebalance_withdraw(&self, place: &str, amount: Decimal, places: &[Place]) -> Result<()> {
        // Everything that is there goes out whole (no dust, no rounding against the protocol's own accounting).
        let all = places.iter().find(|p| p.name == place).is_some_and(|p| (Num::of(p.amount) - Decimal::new(1, 2)).0.is_some_and(|least| amount >= least));
        let asked = (!all).then(|| rounded(amount, 2, Rounding::Down));
        match place {
            "Jupiter Lend" => { self.lend("USDC", asked, false).await?; }
            "Kamino Lend" => { self.kamino("USDC", asked, false, false, false).await?; }
            "HLP (Hyperliquid)" => {
                // Only what left HLP goes on to Solana: free trading margin and a HIP-3 exchange's stay where they are.
                let free = || async { Ok::<_, crate::Error>(Hyperliquid::balance(&self.wallet.evm).await?.withdrawable) };
                let before = free().await?;
                let can = HyperliquidAccount::vault_equity(&self.wallet.evm, guard::hl::HLP_VAULT).await?.1;
                let out = asked.map_or(can, |a| a.min(can));
                self.hlp(false, asked).await?;
                let mut now = before;
                for _ in 0..10 {
                    now = free().await?;
                    if now > before { break }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                let usdc = rounded(out.min(num(Num::of(now) - before)?), 2, Rounding::Down);
                if usdc <= Decimal::ZERO {
                    return fail("HLP's withdrawal isn't on the Hyperliquid balance yet, so nothing was bridged. «withdraw to Solana» brings it once it shows.");
                }
                self.withdraw_to_solana(Some(usdc)).await?;
            }
            _ => {
                if let Some(m) = aave::market(place) { self.aave(false, asked, m).await?; }
                else if let Some(v) = morpho::vault(place) { self.morpho(false, asked, v).await?; }
                else { return fail(format!("ihatecrypto can't take money out of {place}.")) }
            }
        }
        Ok(())
    }

    async fn rebalance_deposit(&self, place: &str, amount: Decimal) -> Result<()> {
        match place {
            "Jupiter Lend" => { self.lend("USDC", Some(amount), true).await?; }
            // The plan already counted Kamino's first-deposit rent against the move.
            "Kamino Lend" => { self.kamino("USDC", Some(amount), true, false, true).await?; }
            _ => {
                if let Some(m) = aave::market(place) { self.aave(true, Some(amount), m).await?; }
                else if let Some(v) = morpho::vault(place) { self.morpho(true, Some(amount), v).await?; }
                else { return fail(format!("ihatecrypto can't put money into {place} this way.")) }
            }
        }
        Ok(())
    }
}

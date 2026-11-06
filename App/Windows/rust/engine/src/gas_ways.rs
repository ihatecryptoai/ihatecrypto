//! Dollars where the fee coin is missing (GasWays.swift), as the Mac's ReceiveBoard.stuck/offerGasWays: the first move
//! there would stop, so the chat says so once per wallet and network, with the ways out as cards (the owner, 2026-10-23:
//! «делай так чтобы это работало и предлагало если нету газа»).

use std::collections::BTreeSet;

use evm::protocols::{Permit3009, SponsoredTransfer};
use ihatecrypto_core::foundation::{Rounding, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::WalletCommand;
use ihatecrypto_core::top_up::TopUpOrder;
use rust_decimal::Decimal;
use trade::EvmNetwork;

use crate::engine::Engine;
use crate::from_core::RouterOption;
use crate::portfolio::Row;
use crate::store;

#[derive(Debug, Clone, PartialEq)]
pub struct StuckDollars {
    pub network: String,
    pub key: String,
    pub coin: String,
    /// What to add of the coin, in dollars.
    pub usd: Decimal,
    /// The dollars there.
    pub usdc: Decimal,
    /// Cards that get the dollars moving without adding the coin first; empty: only adding it works there.
    pub ways: Vec<RouterOption>,
}

/// The least USDC Jupiter's wallet pays a swap's fee for (gasless, for a wallet that can't pay): $5. From $5 Jupiter's
/// own gasless swap backs the market makers up.
pub const GASLESS_SWAP_USD: Decimal = Decimal::from_parts(5, 0, 0, false, 0);

impl Engine {
    pub fn gas_usd(key: &str) -> Decimal {
        Decimal::ONE.max(rounded(EvmNetwork::named(key).map_or(Decimal::ONE, |n| n.topup_usd), 0, Rounding::Up))
    }

    /// The Mac's rule from plain numbers: Solana's USDC and SOL, Hyperliquid's free dollars, and each EVM network's
    /// dollars and gas coin (`held`). Solana without SOL swaps for it with Jupiter paying; an EVM network without its gas
    /// coin, where Solana and Hyperliquid can't bring it, moves its dollars by signature (Relay, or BNB Chain's MegaFuel).
    pub fn stuck(usd: Decimal, solana_sol: Decimal, hyperliquid_free: Decimal, say: impl Fn(&str, &str) -> String,
                 held: impl Fn(&EvmNetwork) -> (Decimal, Decimal)) -> Vec<StuckDollars> {
        let solana_pays = solana_sol >= Decimal::new(TopUpOrder::SOLANA_FEE_FLOOR as i64, 9);
        let mut out = vec![];
        if usd >= Decimal::ONE && !solana_pays {
            let swap = Fmt::num(GASLESS_SWAP_USD, 2);
            out.push(StuckDollars {
                network: "Solana".into(), key: "solana".into(), coin: "SOL".into(), usd: Decimal::ONE, usdc: usd,
                ways: if usd >= GASLESS_SWAP_USD { vec![RouterOption::new(
                    &say(&format!("Swap {swap} USDC for SOL"), &format!("Обменять {swap} USDC на SOL")),
                    &say("Jupiter pays this swap's fee, then SOL pays the rest", "Комиссию этого обмена платит Jupiter, дальше платит SOL"),
                    &format!("buy SOL for {swap} USDC"), true)] } else { vec![] },
            });
        }
        // Gas comes over by itself where Solana (USDC and SOL) or Hyperliquid can bring it (Executor.topUpGas): not stuck.
        if (solana_pays && usd >= Decimal::TWO) || hyperliquid_free >= Decimal::TWO { return out }
        for n in EvmNetwork::searched().into_iter().filter(|n| n.fee_token.is_none()) {
            let (there, gas) = held(n);
            if there < Decimal::ONE || gas >= Decimal::from_i128_with_scale(i128::from(n.gas_floor), 18) { continue }
            let stable = evm::dollar(n.usdc).unwrap_or("USDC");
            let all = Fmt::num(rounded(there, 2, Rounding::Down), 2);
            let (signs, sponsored) = (Permit3009::supports(n.rpc.chain_id, n.usdc), SponsoredTransfer::supports(n.rpc.chain_id, n.usdc));
            let by_signature = if signs || sponsored { vec![
                RouterOption::new(&say(&format!("Move {all} {stable} to Solana"), &format!("Перевести {all} {stable} на Solana")),
                                  &if signs { say(&format!("By signature: Relay pays the gas on {}", n.name), &format!("По подписи: газ на {} платит Relay", n.name)) }
                                   else { say("BNB Chain pays the gas for this transfer", "Газ за этот перевод платит BNB Chain") },
                                  &format!("move {all} USDC from {} to Solana", n.key), true),
                RouterOption::new(&say(&format!("Get {} for gas on {}", n.gas_coin, n.name), &format!("Взять {} на газ в {}", n.gas_coin, n.name)),
                                  &say(&format!("Out of these {stable}, over Solana"), &format!("Из этих {stable}, через Solana")),
                                  &format!("add {} of gas on {}", Fmt::usd(Self::gas_usd(n.key)), n.key), false),
            ] } else { vec![] };
            out.push(StuckDollars { network: n.name.into(), key: n.key.into(), coin: n.gas_coin.into(), usd: Self::gas_usd(n.key), usdc: there, ways: by_signature });
        }
        out
    }

    /// The wallet on screen's stuck dollars, from the portfolio's rows.
    pub fn stuck_dollars(&self) -> Vec<StuckDollars> {
        let st = self.st();
        let value = |key: &str, matches: &dyn Fn(&Row) -> bool| -> Decimal {
            st.portfolio.networks.iter().find(|n| n.key == key)
                .map_or(Decimal::ZERO, |n| n.rows.iter().filter(|r| matches(r)).fold(Decimal::ZERO, |sum, r| sum.saturating_add(r.value)))
        };
        Self::stuck(value("solana", &|r| r.symbol == "USDC"), value("solana", &|r| r.symbol == "SOL"),
                    // Hyperliquid's perps withdrawable, as the Mac's hl.withdrawable: what brings gas from there.
                    st.place_facts.hyperliquid.as_ref().map_or(Decimal::ZERO, |h| h.withdrawable), |en, ru| st.say(en, ru), |n| {
            let usdc = value(n.key, &|r| r.contract.as_ref().is_some_and(|c| c.to_lowercase() == n.usdc.to_lowercase())
                || (r.symbol == "USDC" && r.section == "tokens" && r.contract.as_deref() != Some("native")));
            (usdc, value(n.key, &|r| r.contract.as_deref() == Some("native")))
        })
    }

    /// Said once per wallet and network, when nothing else is going on, with the ways out as cards.
    pub fn offer_gas_ways(&self) {
        let wallet = {
            let st = self.st();
            if st.watching() || st.onboarding || st.card.is_some() || st.running.is_some() || st.thinking { return }
            let Some(wallet) = st.solana_address().or_else(|| st.evm_address()) else { return };
            wallet
        };
        let mut offered: BTreeSet<String> = store::load("gas-ways-offered.json").unwrap_or_default();
        let Some(stuck) = self.stuck_dollars().into_iter().find(|s| !offered.contains(&format!("{wallet}-{}", s.key))) else { return };
        offered.insert(format!("{wallet}-{}", stuck.key));
        store::save(&offered, "gas-ways-offered.json");
        let (dollars, add, coin, network) = (Fmt::num(rounded(stuck.usdc, 2, Rounding::Down), 2), Fmt::usd(stuck.usd), &stuck.coin, &stuck.network);
        let text = if stuck.ways.is_empty() {
            self.say(&format!("{dollars} USDC on {network} can't move yet: every move there pays its fee in {coin}, and the wallet has none. Add ≈ {add} of {coin}."),
                     &format!("{dollars} USDC на {network} пока не сдвинуть: каждое действие там платит комиссию в {coin}, а его на кошельке нет. Добавьте ≈ {add} {coin}."))
        } else if stuck.key == "solana" {
            self.say(&format!("{dollars} USDC on Solana, and no SOL for network fees. Swap a part for SOL: Jupiter pays that one swap's fee, and the SOL pays every move after it."),
                     &format!("{dollars} USDC на Solana, а SOL на комиссии нет. Обменяйте часть на SOL: комиссию этого обмена платит Jupiter, а дальше все действия оплачивает SOL."))
        } else if stuck.key == "bnb" {
            self.say(&format!("{dollars} USDC on BNB Chain, and no BNB for gas. It can still move: BNB Chain pays the gas for a dollar transfer, so it goes to Solana that way, or brings BNB for gas back from there."),
                     &format!("{dollars} USDC на BNB Chain, а BNB на газ нет. Сдвинуть их всё равно можно: газ за перевод долларов платит сама BNB Chain. Так они уйдут на Solana или вернут оттуда BNB на газ."))
        } else {
            self.say(&format!("{dollars} USDC on {network}, and no {coin} for gas. It can still move by signature, with Relay paying the gas: bring it to Solana, or take {coin} for {network} out of it."),
                     &format!("{dollars} USDC на {network}, а {coin} на газ нет. Сдвинуть их можно и так, по подписи, газ заплатит Relay: перевести на Solana или взять из них {coin} для {network}."))
        };
        self.reply(&text, None);
        let top_up = RouterOption::new(&self.say(&format!("Top up {coin}"), &format!("Пополнить {coin}")),
                                       &self.say(&format!("≈ {add} from a browser wallet, your phone or an exchange"), &format!("≈ {add} из кошелька в браузере, с телефона или с биржи")),
                                       &WalletCommand::Receive.summary(), stuck.ways.is_empty());
        self.set_options(stuck.ways.iter().cloned().chain(std::iter::once(top_up)).collect());
    }
}

#[cfg(test)]
#[path = "gas_ways/tests.rs"]
mod tests;

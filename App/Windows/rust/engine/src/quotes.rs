//! A Solana swap's price while its card waits for Confirm (Quotes.swift), as the Mac's quoteJupiter: Jupiter is asked
//! once (again every 15 s while it fails), the card's step shows what comes out, and Confirm holds the swap to it — the
//! Executor won't take 1% less than the card showed (`shown_out`).

use std::time::Duration;

use ihatecrypto_core::foundation::{Num, Rounding};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::{SpotAsset, WalletCommand};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::canvas::Step;
use crate::engine::{ChainStep, Engine, JupiterShown, Pending, WayQuotes};

/// 10^places, None past what a Decimal holds.
fn ten_to(places: i64) -> Option<Decimal> { Decimal::from_scientific(&format!("1e{places}")).ok() }

impl Engine {
    pub async fn quote_card(&self, command: &WalletCommand, board: &str) {
        // One loop per card, as the Mac's .task(id: QuoteRun(command, round)): asked anew (a new card, Refresh, back on its
        // canvas), every loop of an earlier round stops at its next turn.
        let round = {
            let mut st = self.st_mut();
            st.quote_round += 1;
            st.quote_round
        };
        // A Base or Arbitrum trade's two ways, a Robinhood Chain trade's (route_edit.rs).
        if crate::route_edit::quoted(command) {
            self.quote_ways(command, board, round).await;
            return;
        }
        // Everything but Jupiter's: the bridge, the swap off Solana, Aave's health factor, a coin's cost (move_routes.rs).
        if self.quote_route(command, board, round).await { return }
        let pair: Option<(SpotAsset, SpotAsset, Decimal)> = match command {
            WalletCommand::Buy { asset, usdc: Some(usdc) } => Some((SpotAsset::usdc(), asset.clone(), *usdc)),
            WalletCommand::Sell { asset, amount: Some(amount) } => Some((asset.clone(), SpotAsset::usdc(), *amount)),
            WalletCommand::SwapOn { place, from, to, amount: Some(x) } if place == "solana" => match self.solana_asset(from, None).await {
                Some(a) => self.solana_asset(to, None).await.map(|b| (a, b, *x)),
                None => None,
            },
            _ => None,
        };
        let jupiter = matches!(command, WalletCommand::Buy { usdc: Some(_), .. } | WalletCommand::Sell { amount: Some(_), .. })
            || matches!(command, WalletCommand::SwapOn { place, amount: Some(_), .. } if place == "solana");
        let units = pair.as_ref().and_then(|(from, _, x)| (Num::of(*x) * Num(ten_to(from.decimals))).rounded(0, Rounding::Down).0).and_then(|u| u.to_u64()).unwrap_or(0);
        let Some((from, to, _)) = pair.filter(|_| units > 0) else {
            // A token Jupiter can't name, or under one unit: Confirm shows, the run says why if it fails (the Mac).
            if jupiter && self.quote_live(command, board, round) {
                self.st_mut().unpriced = Some(command.clone());
                self.changed();
            }
            return;
        };
        while self.quote_live(command, board, round) {
            let q = protocols::jupiter::quote(&from.mint, &to.mint, units, 50, &[]).await;
            if !self.quote_live(command, board, round) { return }
            if let Ok(q) = q {
                let out = (Num::of(Decimal::from(q.out_amount)) / Num(ten_to(to.decimals))).0.unwrap_or_default();
                self.set_jupiter_shown(Some(JupiterShown {
                    command: command.clone(), out: q.out_amount, title: format!("≈ {} {}", Fmt::num(out, 6), to.symbol),
                    route: format!("Jupiter: {} → {}", from.symbol, to.symbol),
                }));
                // On its route's last step (routeSteps), a single move's or a chain's first: every redraw keeps it.
                self.redraw_route(board, command);
                return;
            }
            self.quote_failed(command);
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    }

    /// Whether a quote loop of `round` still asks for the waiting card's move: not after Refresh or a new card asked anew.
    pub fn quote_live(&self, c: &WalletCommand, board: &str, round: i64) -> bool {
        {
            let st = self.st();
            if round != st.quote_round || st.card.as_ref().is_none_or(|card| card.id != board) { return false }
        }
        self.quoted_move().as_ref() == Some(c)
    }

    /// A priced swap's quote asked and not given: the card shows Refresh and «the price is over 15 s old» while it keeps
    /// asking, as the Mac's empty swapQuote — never «Quoting…» for as long as the place doesn't answer. Its step says so
    /// at once, as the Mac's route drawn from that swapQuote: «no quote now · asked again every 30 s».
    pub fn quote_failed(&self, c: &WalletCommand) {
        if self.quoted_move().as_ref() != Some(c) || !self.priced(c) { return }
        {
            let st = self.st();
            if st.way_quotes.as_ref().map(|w| &w.command) == Some(c) || st.jupiter_shown.as_ref().map(|j| &j.command) == Some(c) { return }
        }
        self.set_way_quotes(Some(WayQuotes { command: c.clone(), ways: vec![] }));
        let id = self.st().card.as_ref().map(|card| card.id.clone());
        match id { Some(id) => self.redraw_route(&id, c), None => self.changed() }
    }

    /// The move a waiting card prices: a single move, or a chain's first, as the Mac's chain card is its first move's
    /// (Canvases.swift:3001-3005): quoted, counted down and held to its price the same way.
    pub fn quoted_move(&self) -> Option<WalletCommand> {
        match &self.st().pending {
            Some(Pending::Wallet(c)) => Some(c.clone()),
            Some(Pending::Chain(list)) => match list.first() { Some(ChainStep::Wallet(c)) => Some(c.clone()), _ => None },
            _ => None,
        }
    }

    /// The route's steps with Jupiter's quote on the step that gets it: the last of the route (the token bought, or the
    /// USDC back).
    pub fn with_jupiter(&self, c: &WalletCommand, mut steps: Vec<Step>) -> Vec<Step> {
        let shown = self.st().jupiter_shown.as_ref().filter(|j| &j.command == c).map(|j| (j.title.clone(), j.route.clone()));
        if let (Some((title, route)), Some(last)) = (shown, steps.last_mut()) {
            last.title = title;
            if !last.lines.contains(&route) { last.lines.insert(0, route) }
        }
        steps
    }

    /// The output the card showed for this very command, for the Executor's floor; None for any other.
    pub fn shown_out(&self, command: &WalletCommand) -> Option<u64> {
        self.st().jupiter_shown.as_ref().filter(|j| &j.command == command).map(|j| j.out)
    }
}

#[cfg(test)]
#[path = "quotes/tests.rs"]
mod tests;

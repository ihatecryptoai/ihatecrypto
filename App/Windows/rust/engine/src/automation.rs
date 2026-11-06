//! The automation wallet and scheduled moves (Automation.swift): a budget beside the main wallet that scheduled tasks
//! spend from without Windows Hello. The signer lets it sign only a task's moves, and only to the user's own wallets or
//! addresses already confirmed in person; what its tasks buy goes on to the wallet that's on, and selling it takes
//! Windows Hello there. Here: the task-local wallet a run signs with, what a task does, and the recurring buys' views.
//! Layer 2 adds here: the automation wallet (create, its executor, uses_automation, can_pick_pays, other_payer), the
//! recurring buys (recurring-buys.json, recurring_views, run_recurring_buys, recurring-assets.json) and forwarding what a
//! task bought to the wallet that's on (for-main.json).

use ihatecrypto_core::foundation::{Date, number};
use ihatecrypto_core::position_command::{PositionCommand, WalletCommand};
use ihatecrypto_core::scheduled_task::ScheduledTask;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use signer_api::SignerWallet;

use crate::json;

tokio::task_local! {
    /// The automation wallet a scheduled task runs on, for the moves inside that run only (Swift's `TaskRun.automation`;
    /// the executor picks it up). Outside a run: none.
    pub static TASK_RUN: Option<SignerWallet>;
}

/// The automation wallet of the run under way, if any.
pub fn task_run() -> Option<SignerWallet> { TASK_RUN.try_with(Clone::clone).ok().flatten() }

/// Below this the automation wallet can't pay Solana's fee plus a new account's rent (0.005 SOL).
pub const SOL_FLOOR: Decimal = Decimal::from_parts(5, 0, 0, false, 3);

/// What a scheduled task does, read from its steps (Automation.swift's and Schedule.swift's `extension ScheduledTask`).
pub trait TaskFacts {
    /// A step that moves money: without the automation wallet it waits for Confirm and Windows Hello.
    fn moves_money(&self) -> bool;
    /// It changes an open position (close, stop, take, trail): on the automation wallet it runs by itself, on that
    /// wallet's own account, as the Mac's confirmed runSteps carries a position out.
    fn manages_position(&self) -> bool;
    /// It sells a coin: from the wallet that's on, with Windows Hello, never by itself from the automation wallet.
    fn sells(&self) -> bool;
    /// It buys coins: what it gets goes on to the wallet that's on after a run from the automation wallet.
    fn buys_coins(&self) -> bool;
    /// It spreads USDC across the best rates: it spends what's free where it pays from.
    fn spreads_usdc(&self) -> bool;
}

impl TaskFacts for ScheduledTask {
    fn moves_money(&self) -> bool {
        self.steps.iter().any(|s| !matches!(WalletCommand::parse(s),
            None | Some(WalletCommand::Yields(_) | WalletCommand::Receive | WalletCommand::Schedule { .. } | WalletCommand::StopSchedules)))
    }

    fn manages_position(&self) -> bool { self.steps.iter().any(|s| WalletCommand::parse(s).is_none() && PositionCommand::parse(s).is_some()) }

    fn sells(&self) -> bool {
        self.steps.iter().any(|s| matches!(WalletCommand::parse(s), Some(WalletCommand::Sell { .. } | WalletCommand::SellOn { .. }
            | WalletCommand::SellNear { .. } | WalletCommand::SellStock { .. } | WalletCommand::HlSpotSell { .. })))
    }

    fn buys_coins(&self) -> bool {
        self.steps.iter().any(|s| matches!(WalletCommand::parse(s), Some(WalletCommand::Buy { .. } | WalletCommand::BuyStock { .. } | WalletCommand::BuyOn { .. })))
    }

    fn spreads_usdc(&self) -> bool {
        self.steps.iter().any(|s| matches!(WalletCommand::parse(s), Some(WalletCommand::Allocate { symbol, .. }) if symbol == "USDC"))
    }
}

/// A recurring buy as its board on the canvas draws it (the Mac's ScheduleBoard): the buy, how often, the next run and
/// the USDC free on Solana that pays for it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecurringView {
    pub id: String,
    pub symbol: String,
    #[serde(with = "number")]
    pub usdc: Decimal,
    pub days: i64,
    #[serde(with = "json::date")]
    pub next: Date,
    pub paused: bool,
    #[serde(skip_serializing_if = "Option::is_none", with = "number::option")]
    pub solana: Option<Decimal>,
}

/// The token a recurring buy's first Confirm bought, by its mint (recurring-assets.json): each run buys that token, as
/// the Mac's schedule keeps its asset, and never checks it against Jupiter's list again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptAsset {
    pub symbol: String,
    pub decimals: i64,
    pub token2022: bool,
}

//! Several moves under one Confirm (Chains.swift): «withdraw 5 USDC to Solana then deposit 5 USDC to kamino».
//! Layer 2 adds here: splitting a request into steps (chain_kind), the chain's card, «How much?» in a chain, running the
//! held steps after a card or a contract call.

use ihatecrypto_core::foundation::Date;
use ihatecrypto_core::position_command::WalletCommand;

/// A chain waiting on the person: the move whose «How much?» is open, with the steps held behind it; and whether the last
/// chain stopped before its card (a scheduled run's History marks it failed).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChainState {
    pub held: Option<Held>,
    pub stopped: bool,
    /// A contract call's card in a chain, and the steps after it: that card's Confirm names them, and once the call is
    /// through they run on it (the Mac's runSteps → continueTask), on the same canvas within half an hour.
    pub after_call: Option<AfterCall>,
    /// The steps held behind a card are running now: the model's request waits for them to end (continueAgent).
    pub running_held: bool,
    /// Cards drawn so far (show()): a held step's Confirm takes only the one card it drew itself.
    pub cards_drawn: i64,
}

/// The move whose «How much?» is open and the steps' words after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub command: WalletCommand,
    pub rest: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AfterCall {
    pub board: String,
    pub canvas: String,
    pub rest: Vec<String>,
    pub since: Date,
}

/// What a step of a chain is.
#[derive(Debug, Clone, PartialEq)]
pub enum ChainKind {
    /// It shows or sets something: run when it's reached, before the card if it comes first.
    Now,
    /// A move the chain's card signs.
    Move(WalletCommand),
    /// It has a card of its own (a position, a trade plan, a recurring buy): on the chain's card, run once it's confirmed.
    Later,
}

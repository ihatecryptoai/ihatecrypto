//! What the executor throws (the error types of Executor.swift, and the signer's and the device's that pass through).

use execution::trade_signer::TradeError;
use ihatecrypto_core::foundation::Num;
use ihatecrypto_core::intent::Fmt;
use rust_decimal::Decimal;
use wallet::WalletError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// ExecutionError: the move lacked money or a route, or something answered no; the chat may offer a way around.
    Execution(String),
    /// GuardStop: the guard, the signer or a scam check stopped a move. Nothing is offered around it.
    Guard(String),
    /// WalletError, and what a network, an answer's reader or the journal threw.
    Wallet(String),
    /// The hardware wallet's own answer (LedgerError): declined on its buttons, locked, the wrong app open.
    Device { status: Option<u16>, message: String },
    /// A move that would lose more than it may on its own: asked about («Send anyway»), not refused.
    Costly(CostlyMove),
    /// Executor.NoReply: signed and journaled, then no answer, so the action may be on the exchange. Says what the
    /// cause says.
    NoReply(Box<Error>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Execution(m) | Error::Guard(m) | Error::Wallet(m) | Error::Device { message: m, .. } => f.write_str(m),
            Error::Costly(c) => f.write_str(&c.description()),
            Error::NoReply(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<TradeError> for Error {
    fn from(e: TradeError) -> Self {
        match e {
            TradeError::Guard(m) => Error::Guard(m),
            TradeError::Failed(m) => Error::Execution(m),
            TradeError::Device { status, message } => Error::Device { status, message },
        }
    }
}

impl From<WalletError> for Error {
    fn from(e: WalletError) -> Self { Error::Wallet(e.0) }
}

impl From<CostlyMove> for Error {
    fn from(c: CostlyMove) -> Self { Error::Costly(c) }
}

/// `throw ExecutionError(…)`.
pub fn fail<T>(m: impl Into<String>) -> Result<T> { Err(Error::Execution(m.into())) }

/// `throw GuardStop(…)`.
pub fn stop<T>(m: impl Into<String>) -> Result<T> { Err(Error::Guard(m.into())) }

/// A move that would lose more than ihatecrypto lets one lose by itself — 3% of it, 15¢ on a small one, $1 at most (5%
/// for gas): stopped before anything is signed, with the numbers, so the user can take it anyway («Send anyway»).
#[derive(Debug, Clone, PartialEq)]
pub struct CostlyMove {
    pub lost: Decimal,
    pub paid: Decimal,
    /// Who keeps it: «Relay», «Relay's swap».
    pub what: String,
    /// Its own words, where the loss isn't a share the move keeps: Kamino's first-deposit rent comes on top, in SOL.
    pub(crate) words: Option<String>,
    /// A swap's price impact (0…1) when that's what stopped it: «Send anyway» takes that impact, not a dollar loss.
    pub impact: Option<Decimal>,
    /// The part of what goes in that moves the price only about 1% («160,000 WIF»), for a card.
    pub part: Option<(Decimal, String)>,
}

impl CostlyMove {
    pub fn new(lost: Decimal, paid: Decimal, what: &str) -> Self {
        Self { lost, paid, what: what.into(), words: None, impact: None, part: None }
    }

    /// Its errorDescription.
    pub fn description(&self) -> String {
        if let Some(words) = &self.words { return words.clone() }
        let share = (Num::of(self.lost) / self.paid.max(Decimal::new(1, 2)) * 100).0.map_or("NaN".into(), |s| Fmt::num(s, 0));
        format!("{} keeps ≈ {} of {} ({share}%): more than a move may lose on its own. Nothing was signed.", self.what, Fmt::usd(self.lost),
                Fmt::usd(self.paid))
    }
}

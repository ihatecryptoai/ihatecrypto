//! Paying for a service (Payments.swift): «pay GET https://…», a service's price waiting for Confirm. Services quoted
//! with the request's content are `State::priced_with_content`.
//! Layer 2 adds here: asking a service's price (price_asked), the payment's card, paying and asking again with the
//! receipt, the service's answer block.

use ihatecrypto_core::position_command::WalletCommand;

use crate::from_core::MppChallenge;

/// A service's price waiting for Confirm.
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentQuote {
    pub command: WalletCommand,
    pub challenge: MppChallenge,
    pub ask: String,
    pub service: String,
    /// Quoted without the request's content: Confirm sends it, asking the price again with it before paying.
    pub probed: bool,
}

/// What a service asks, read as its card reads it: its price, or its answer when it asks none.
#[derive(Debug, Clone, PartialEq)]
pub enum PriceAsked {
    Quoted(Box<PaymentQuote>),
    Answered(String),
}

//! Dollars where the fee coin is missing (GasWays.swift), as the Mac's ReceiveBoard.stuck/offerGasWays: the first move
//! there would stop, so the chat says so once per wallet and network, with the ways out as cards.
//! Layer 2 adds here: stuck_dollars, offer_gas_ways (gas-ways-offered.json) and the ways themselves.

use rust_decimal::Decimal;

use crate::from_core::RouterOption;

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

/// From $5 Jupiter's own gasless swap backs the market makers up.
pub const GASLESS_SWAP_USD: Decimal = Decimal::from_parts(5, 0, 0, false, 0);

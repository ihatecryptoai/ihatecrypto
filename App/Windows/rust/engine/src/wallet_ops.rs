//! The signer's wallets from the window (WalletOps.swift): status, the address book, new wallets. Whether a wallet is
//! being made is `State::creating_wallet`.
//! Layer 2 adds here: start_signer, read_status, read_book, signer_call, create_wallet, use_wallet, save/edit/delete
//! address, check_address, stuck_view and the rest of the file's `extension Engine`.

use ihatecrypto_core::foundation::number;
use rust_decimal::Decimal;
use serde::Serialize;

/// Dollars without the fee coin, for the onboarding's Top up.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StuckView {
    pub network: String,
    pub key: String,
    pub coin: String,
    /// What to add of the coin, in dollars, and the dollars there.
    #[serde(with = "number")]
    pub usd: Decimal,
    #[serde(with = "number")]
    pub usdc: Decimal,
    /// Cards the chat offers to move them without the coin; 0: only adding it works there.
    pub ways: i64,
}

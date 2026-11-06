//! Part of `PositionCommand.swift`, what round 1 needed (the PositionCommand round completes this file): two of
//! `WalletCommand`'s cases, and the names of the places money is on.

use rust_decimal::Decimal;

use crate::foundation::capitalized;

/// Moving the user's own money: back to Solana, into the USDC vault and out, spot buys and sells.
// pending: WalletCommand's other cases and its parser come with the PositionCommand round.
#[derive(Debug, Clone, PartialEq)]
pub enum WalletCommand {
    /// USDC from the Solana wallet to our own address on «base», «arbitrum» or «robinhood» (USDG there); nil = ask.
    SolanaTo(String, Option<Decimal>),
    /// A paid API on Tempo (MPP): the request as the service documents it; the price comes from its 402 answer and
    /// is paid in USDC on Tempo only after Confirm. `method` «GET» or «POST»; `body` JSON or nil.
    Pay { method: String, url: String, body: Option<String> },
}

impl WalletCommand {
    /// «base» → «Base», «bnb» → «BNB Chain»; any other key capitalized.
    pub fn place_name(place: &str) -> String {
        match place {
            "base" => "Base", "arbitrum" => "Arbitrum", "robinhood" => "Robinhood", "polygon" => "Polygon", "ethereum" => "Ethereum",
            "tempo" => "Tempo", "hyperevm" => "HyperEVM", "optimism" => "Optimism", "bnb" => "BNB Chain", "near" => "NEAR",
            _ => return capitalized(place),
        }
        .into()
    }
}

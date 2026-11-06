//! A top-up of this wallet (`TopUp.swift`): what lands on which of its networks. USDC is what ihatecrypto trades with;
//! the network's own coin pays the fees; «any coin» and «any amount» leave both to the sender.

use rust_decimal::Decimal;

use crate::foundation::{Rounding, pow10, rounded, text};
use crate::network::Solana;

/// A network the wallet receives on: Solana, or one of the EVM networks ihatecrypto moves money on (the same 0x
/// address).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Network {
    pub key: String,
    pub title: String,
    /// nil on Solana.
    pub chain_id: Option<u64>,
    /// Its dollar: Circle's USDC mint or contract; Robinhood Chain's is USDG.
    pub usdc: String,
    pub usdc_symbol: String,
    pub usdc_decimals: u32,
    /// The coin fees are paid in; nil on Tempo, which pays them in its dollar.
    pub coin: Option<String>,
    pub explorer: String,
    /// A public node, for a browser wallet that doesn't know the network yet.
    pub rpc: String,
    /// What a transfer there costs and how soon it lands.
    pub cost: String,
}

impl Network {
    /// USDC with 6 decimals, a fee under a cent: Swift's defaults; set the fields for anything else.
    pub fn new(key: &str, title: &str, chain_id: Option<u64>, usdc: &str, coin: Option<&str>, explorer: &str, rpc: &str) -> Self {
        Self {
            key: key.into(), title: title.into(), chain_id, usdc: usdc.into(), usdc_symbol: "USDC".into(), usdc_decimals: 6,
            coin: coin.map(Into::into), explorer: explorer.into(), rpc: rpc.into(), cost: "fee under 1¢ · lands in seconds".into(),
        }
    }

    pub fn solana() -> Self { Self::new("solana", "Solana", None, Solana::USDC_MINT, Some("SOL"), "https://solscan.io", "") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Coin {
    Usdc,
    Native,
    Any,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopUpOrder {
    pub network: Network,
    pub coin: Coin,
    /// nil: any amount, the sender's wallet asks.
    pub amount: Option<Decimal>,
    /// This wallet's address on `network`.
    pub recipient: String,
}

impl TopUpOrder {
    /// Below this much SOL a Solana wallet can't sign its moves for long (fees, a new token account's rent): USDC alone
    /// there is a dead end (the owner, 2026-10-23: «закинул USDC, а оказалось, что надо было ещё и газ»).
    pub const SOLANA_FEE_FLOOR: u64 = 5_000_000;
    /// What a top-up of USDC on Solana from a browser wallet brings along, in the same approval, to a wallet below it.
    pub const SOLANA_FEE_TOP_UP: u64 = 10_000_000;

    /// The network's own coin on a network that has none (Tempo) is its dollar.
    pub fn new(network: Network, coin: Coin, amount: Option<Decimal>, recipient: &str) -> Self {
        let coin = if network.coin.is_none() && coin == Coin::Native { Coin::Usdc } else { coin };
        Self { network, coin, amount, recipient: recipient.into() }
    }

    pub fn symbol(&self) -> String {
        match self.coin {
            Coin::Usdc => self.network.usdc_symbol.clone(),
            Coin::Native => self.network.coin.clone().unwrap_or_default(),
            Coin::Any => "any coin".into(),
        }
    }

    pub fn decimals(&self) -> u32 {
        match (self.coin, self.network.chain_id) { (Coin::Usdc, _) => self.network.usdc_decimals, (_, None) => 9, _ => 18 }
    }

    /// The amount in the coin's smallest units; nil for any coin or amount, nothing, too much, or more digits than the
    /// coin has.
    pub fn units(&self) -> Option<String> {
        let amount = self.amount.filter(|a| self.coin != Coin::Any && *a > Decimal::ZERO && *a <= Decimal::from(1_000_000))?;
        let scaled = amount * pow10(self.decimals());
        let whole = rounded(scaled, 0, Rounding::Plain);
        (whole == scaled).then(|| text(whole))
    }

    /// For a phone wallet's camera: Solana Pay's transfer request (Phantom, Solflare, Backpack) or EIP-681 (MetaMask,
    /// Rainbow, Coinbase Wallet), with the coin, the network and the amount filled in when they are chosen — without
    /// an amount the wallet asks for it. Exchanges' scanners want the bare address, which the address card keeps.
    pub fn pay_link(&self) -> Option<String> {
        let units = self.units();
        // A typed amount the coin can't hold makes no link.
        if self.coin != Coin::Any && self.amount.is_some() && units.is_none() { return None }
        let Some(chain) = self.network.chain_id else {
            let amount = units.as_ref().and(self.amount).map_or(String::new(), |a| format!("amount={}&", text(a)));
            let token = if self.coin == Coin::Usdc { format!("spl-token={}&", self.network.usdc) } else { String::new() };
            return Some(format!("solana:{}?{amount}{token}label=ihatecrypto&message=Top%20up", self.recipient));
        };
        Some(match self.coin {
            Coin::Usdc => format!("ethereum:{}@{chain}/transfer?address={}{}", self.network.usdc, self.recipient,
                                  units.map_or(String::new(), |u| format!("&uint256={u}"))),
            Coin::Native => format!("ethereum:{}@{chain}{}", self.recipient, units.map_or(String::new(), |u| format!("?value={u}"))),
            Coin::Any => format!("ethereum:{}@{chain}", self.recipient),
        })
    }

    /// Where a sent transfer shows.
    pub fn explorer(&self, id: &str) -> String { format!("{}/tx/{id}", self.network.explorer) }
}

//! Favorite tokens (Favorites.swift): «show token 0x…», «add 0x… to favorites», «remove token AERO from favorites» —
//! the shared TokenCommand and DexScreener's TokenLookup, as on the Mac; the favorites' prices refresh with the balances.
//! Layer 2 adds here: token_command, look_token, refreshing the favorites' prices, star.

use ihatecrypto_core::foundation::number;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// One favorite (favorites.json).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteToken {
    pub chain: String,
    pub address: String,
    pub name: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub price: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub change24h: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "number::option")]
    pub liquidity: Option<Decimal>,
}

impl FavoriteToken {
    pub fn id(&self) -> String { format!("{}:{}", self.chain, self.address.to_lowercase()) }
}

//! Jupiter Lend Earn (`JupiterLend.swift`): deposit a token, receive jl-shares that grow with interest, withdraw any
//! time. Jupiter builds the transactions; the wallet signs them after Confirm.

use std::collections::HashMap;
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use wallet::{Result, WalletError};

use crate::{dec, json, ns_dec, ns_int, ns_u64, object, objects, str_dec, string};

pub const BASE: &str = "https://api.jup.ag/lend/v1/earn";

#[derive(Debug, Clone, PartialEq)]
pub struct Vault {
    /// jlUSDC
    pub symbol: String,
    /// The vault's share mint.
    pub address: String,
    /// USDC
    pub asset_symbol: String,
    pub asset_mint: String,
    pub decimals: i64,
    /// Yearly rate in percent: supply interest plus rewards.
    pub apy: Decimal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub vault: String,
    /// Underlying tokens the shares are worth now, raw units.
    pub assets: u64,
    pub shares: u64,
    /// The vault token's address (what `earnings` is asked by), the coin it holds («USDC», «WSOL»), its decimals and
    /// % a year (supply interest plus rewards).
    pub address: String,
    pub coin: String,
    pub decimals: i64,
    pub apy: Option<Decimal>,
}

impl Default for Position {
    fn default() -> Self {
        Self { vault: String::new(), assets: 0, shares: 0, address: String::new(), coin: String::new(), decimals: 6, apy: None }
    }
}

/// `LendError`: «Jupiter Lend: » and what the service said (`status` 0: not known).
pub fn error(data: &[u8], status: u16) -> WalletError { WalletError(format!("Jupiter Lend: {}", net::said(data, status))) }

/// Where Jupiter Lend is asked, and how long a transaction's retries wait: the real API by default, a stand-in in tests.
#[derive(Debug, Clone)]
pub struct JupiterLend {
    pub base: String,
    pub delays: Vec<Duration>,
}

impl Default for JupiterLend {
    fn default() -> Self { Self { base: BASE.into(), delays: net::DELAYS.to_vec() } }
}

impl JupiterLend {
    pub async fn vaults(&self) -> Result<Vec<Vault>> { parse_vaults(&self.get(&format!("{}/tokens", self.base)).await?) }

    /// Unsigned transaction depositing `amount` raw units of `asset` for `user`.
    pub async fn deposit_transaction(&self, asset: &str, user: &str, amount: u64) -> Result<Vec<u8>> {
        self.transaction("/deposit", json!({"asset": asset, "signer": user, "amount": amount.to_string()})).await
    }

    /// Unsigned transaction withdrawing `amount` raw units of the underlying `asset`.
    pub async fn withdraw_transaction(&self, asset: &str, user: &str, amount: u64) -> Result<Vec<u8>> {
        self.transaction("/withdraw", json!({"asset": asset, "signer": user, "amount": amount.to_string()})).await
    }

    /// Unsigned transaction redeeming `shares`: the way to take everything out. The positions API rounds the assets
    /// up by a unit, so withdrawing that amount fails with «insufficient funds» (measured 2026-09-28).
    pub async fn redeem_transaction(&self, asset: &str, user: &str, shares: u64) -> Result<Vec<u8>> {
        self.transaction("/redeem", json!({"asset": asset, "signer": user, "shares": shares.to_string()})).await
    }

    pub async fn positions(&self, user: &str) -> Result<Vec<Position>> {
        parse_positions(&self.get(&format!("{}/positions?users={user}", self.base)).await?)
    }

    /// What each position has earned so far, in the coin's raw units, by the vault token's address.
    pub async fn earnings(&self, user: &str, positions: &[String]) -> Result<HashMap<String, Decimal>> {
        parse_earnings(&self.get(&format!("{}/earnings?user={user}&positions={}", self.base, positions.join(","))).await?)
    }

    /// The same host as Jupiter's swap API, whose 503 stopped a chain once: nothing is signed yet, so asked again.
    async fn transaction(&self, path: &str, body: Value) -> Result<Vec<u8>> {
        let url = format!("{}{path}", self.base);
        net::run(&self.delays, |_| net::post_json(&url, &body), parse_transaction).await
    }

    /// Any status is read as an answer: an error body fails as a list, in the service's words.
    async fn get(&self, url: &str) -> Result<Vec<u8>> { Ok(crate::send(net::HTTP.get(url)).await?.0) }
}

pub fn parse_vaults(data: &[u8]) -> Result<Vec<Vault>> {
    let j = json(data)?;
    let list = objects(Some(&j)).ok_or_else(|| error(data, 0))?;
    Ok(list.into_iter().filter_map(|t| {
        let t = Some(t);
        let asset = object(t, "asset")?;
        let mint = string(t, "assetAddress").or_else(|| string(Some(asset), "address"))?;
        let rate = dec(t?.get("totalRate")).unwrap_or_default();
        Some(Vault {
            symbol: string(t, "symbol").unwrap_or("?").into(), address: string(t, "address").unwrap_or("?").into(),
            asset_symbol: string(Some(asset), "symbol").unwrap_or("?").into(), asset_mint: mint.into(),
            decimals: ns_int(t?.get("decimals")).unwrap_or(6), apy: rate / Decimal::ONE_HUNDRED,
        })
    }).collect())
}

pub fn parse_positions(data: &[u8]) -> Result<Vec<Position>> {
    let j = json(data)?;
    let list = objects(Some(&j)).ok_or_else(|| error(data, 0))?;
    Ok(list.into_iter().filter_map(|p| {
        let token = object(Some(p), "token");
        // `UInt64(String)` first, then NSNumber's uint64Value.
        let raw = |key: &str| p.get(key).and_then(Value::as_str).and_then(|s| s.parse::<u64>().ok()).or_else(|| ns_u64(p.get(key)));
        let (assets, shares) = (raw("underlyingAssets")?, raw("shares")?);
        let asset = object(token, "asset");
        let number = |o: Option<&crate::Object>| ns_int(o.and_then(|o| o.get("decimals")));
        Some(Position {
            vault: string(token, "symbol").unwrap_or("?").into(), assets, shares,
            address: string(token, "address").unwrap_or("").into(), coin: string(asset, "symbol").unwrap_or("").into(),
            decimals: number(asset).or_else(|| number(token)).unwrap_or(6),
            apy: token.and_then(|t| dec(t.get("totalRate"))).map(|r| r / Decimal::ONE_HUNDRED),
        })
    }).collect())
}

/// By the vault token's address; the first answer for an address is kept.
pub fn parse_earnings(data: &[u8]) -> Result<HashMap<String, Decimal>> {
    let j = json(data)?;
    let list = objects(Some(&j)).ok_or_else(|| error(data, 0))?;
    let mut out = HashMap::new();
    for e in list {
        // NSNumber first here, then a string.
        let (Some(address), Some(raw)) = (string(Some(e), "address"), ns_dec(e.get("earnings")).or_else(|| str_dec(e.get("earnings")))) else { continue };
        out.entry(address.to_string()).or_insert(raw);
    }
    Ok(out)
}

/// A transaction's answer: its `transaction` in base64. Anything else is Jupiter's refusal, asked again on a throttle
/// or a server error.
pub fn parse_transaction(data: &[u8], status: u16) -> std::result::Result<Vec<u8>, net::Again> {
    let wire = serde_json::from_slice::<Value>(data).ok().and_then(|j| STANDARD.decode(j.get("transaction")?.as_str()?).ok());
    wire.ok_or_else(|| net::Again { again: status == 429 || status >= 500, error: error(data, status) })
}

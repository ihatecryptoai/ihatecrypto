//! Where an asset earns right now, across the protocols ihatecrypto knows, with what ihatecrypto can do there and the
//! catch (`Yields.swift`).

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde_json::json;
use wallet::{Result, WalletError};

use crate::jupiter_lend::{JupiterLend, Vault};
use crate::kamino::{self, Kamino, Reserve};
use crate::{READ_TIMEOUT, json, ns_dec};

pub const JITO_SOL: &str = "J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn";
pub const HLP: &str = "0xdfc24b077bc1425ad1dea75bcb6f8158e10df303";
pub const HYPERLIQUID_INFO: &str = "https://api.hyperliquid.xyz/info";

/// `Yields.Option`.
#[derive(Debug, Clone, PartialEq)]
pub struct YieldOption {
    pub asset: String,
    pub place: String,
    /// % a year.
    pub apy: Decimal,
    pub tvl_usd: Option<Decimal>,
    /// The chat phrase that puts money there, when ihatecrypto can.
    pub how: Option<String>,
    pub note: String,
}

/// All options for the stablecoins and SOL; `asset` narrows to one («USDC», «SOL»). Sorted by yield.
/// A source that does not answer is skipped rather than failing the whole list.
pub async fn options(asset: Option<&str>) -> Vec<YieldOption> {
    options_from(&JupiterLend::default(), &Kamino::default(), HYPERLIQUID_INFO, asset).await
}

/// `options` from these services (stand-ins in tests).
pub async fn options_from(jupiter: &JupiterLend, kamino: &Kamino, hyperliquid: &str, asset: Option<&str>) -> Vec<YieldOption> {
    let (jup, kam, staking, hlp) = tokio::join!(jupiter.vaults(), kamino.reserves(kamino::MAIN_MARKET), kamino.staking_yields(),
                                                hyperliquid_vault_apr_at(hyperliquid, HLP));
    gather(jup.ok(), kam.ok(), staking.ok(), hlp.ok(), asset)
}

/// The options from what each source answered (None: it didn't).
pub fn gather(jup: Option<Vec<Vault>>, kam: Option<Vec<Reserve>>, staking: Option<HashMap<String, Decimal>>, hlp_apr: Option<Decimal>,
              asset: Option<&str>) -> Vec<YieldOption> {
    let option = |asset: &str, place: &str, apy, tvl_usd, how: String, note: &str| YieldOption {
        asset: asset.into(), place: place.into(), apy, tvl_usd, how: Some(how), note: note.into(),
    };
    let mut out = Vec::new();
    for v in jup.unwrap_or_default() {
        let symbol = if v.asset_symbol == "WSOL" { "SOL" } else { &v.asset_symbol };
        out.push(option(symbol, "Jupiter Lend", v.apy, None, format!("deposit 10 {symbol} to the vault"),
                        "lending, withdraw any time; the first deposit opens a share account for ≈ 0.002 SOL, refunded on full withdrawal"));
    }
    for r in kam.unwrap_or_default().into_iter().filter(|r| r.supply_apy > Decimal::ZERO) {
        out.push(option(&r.symbol, "Kamino Lend", r.supply_apy, Some(r.tvl_usd), format!("deposit 50 {} to Kamino", r.symbol),
                        "lending; the first deposit opens Kamino accounts for ≈ 0.03 SOL rent, worth it from ≈ $35"));
    }
    if let Some(apy) = staking.and_then(|s| s.get(JITO_SOL).copied()) {
        out.push(option("SOL", "JitoSOL staking", apy, None, "stake 0.1 SOL".into(),
                        "liquid staking: SOL becomes JitoSOL, which earns by rising in price against SOL; swap back in seconds"));
    }
    if let Some(apr) = hlp_apr {
        out.push(option("USDC", "HLP (Hyperliquid)", apr, None, "deposit 5 USDC to HLP".into(),
                        "the exchange's market making: some weeks lose money; from $5; withdraw 4 days after a deposit"));
    }
    let wanted = asset.map(str::to_uppercase);
    out.retain(|o| wanted.as_ref().is_none_or(|w| o.asset.to_uppercase() == *w));
    out.sort_by_key(|o| std::cmp::Reverse(o.apy));
    out
}

/// A Hyperliquid vault's APR, % a year.
pub async fn hyperliquid_vault_apr(vault: &str) -> Result<Decimal> { hyperliquid_vault_apr_at(HYPERLIQUID_INFO, vault).await }

pub async fn hyperliquid_vault_apr_at(url: &str, vault: &str) -> Result<Decimal> {
    let body = json!({"type": "vaultDetails", "vaultAddress": vault}).to_string();
    let (data, _) = crate::send(net::HTTP.post(url).header("Content-Type", "application/json").body(body).timeout(READ_TIMEOUT)).await?;
    parse_vault_apr(&data)
}

/// `apr` as a number, times 100. Anything else is URLError's «cannot parse response» (never shown: every caller
/// takes a failure as no number).
pub fn parse_vault_apr(data: &[u8]) -> Result<Decimal> {
    let j = json(data)?;
    ns_dec(j.as_object().and_then(|o| o.get("apr"))).map(|a| a * Decimal::ONE_HUNDRED).ok_or_else(|| WalletError("cannot parse response".into()))
}

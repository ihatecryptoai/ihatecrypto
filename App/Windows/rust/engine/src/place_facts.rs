//! A place's details board beyond the portfolio's rows (PlaceFacts.swift, the Mac's PlaceDetailBoard): Hyperliquid's
//! account in full, and the rate each deposit earns now, read with the balances.

use std::collections::HashMap;

use chrono::Utc;
use ihatecrypto_core::foundation::{Date, Num, Rounding, number, rounded};
use ihatecrypto_core::holdings;
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::Hyperliquid;
use rust_decimal::Decimal;
use serde::Serialize;
use trade::HyperliquidAccount;

use crate::engine::Engine;
use crate::json;
use crate::portfolio::{Network, Read};

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct PlaceFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyperliquid: Option<HlFacts>,
    /// USDC's rate a year now, in %, by place: kamino, jupiter, aave (Base), morpho (Base), hlp.
    #[serde(with = "json::numbers")]
    pub rates: HashMap<String, Decimal>,
}

/// `PlaceFacts.HL`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HlFacts {
    #[serde(with = "number")]
    pub account_value: Decimal,
    #[serde(with = "number")]
    pub withdrawable: Decimal,
    #[serde(rename = "spotUSDC", with = "number")]
    pub spot_usdc: Decimal,
    /// HIP-3 exchanges' own margin (stocks, commodities): `xyz`.
    pub dexes: Vec<DexFacts>,
    #[serde(with = "number")]
    pub hlp_equity: Decimal,
    /// HLP keeps a deposit 4 days.
    #[serde(skip_serializing_if = "Option::is_none", with = "json::date::option")]
    pub hlp_locked_until: Option<Date>,
}

/// `PlaceFacts.Dex`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DexFacts {
    pub dex: String,
    #[serde(with = "number")]
    pub account_value: Decimal,
    #[serde(with = "number")]
    pub withdrawable: Decimal,
}

impl Engine {
    /// The account, its HIP-3 exchanges and its HLP share, read together; the network's card counts all of it (the
    /// Mac's hlTotal), margin in «Perps» and HLP as a deposit.
    pub async fn hyperliquid(user: &str) -> (Read, Option<HlFacts>) {
        let dexes = async {
            let mut out = vec![];
            for dex in Hyperliquid::HIP3_DEXES {
                if let Ok((account_value, withdrawable)) = Hyperliquid::dex_balance(user, dex).await { out.push(DexFacts { dex: dex.into(), account_value, withdrawable }) }
            }
            out
        };
        let (balance, spot, vault, dexes) = tokio::join!(Hyperliquid::balance(user), holdings::hyperliquid_spot(user),
                                                         HyperliquidAccount::vault_equity(user, guard::hl::HLP_VAULT), dexes);
        let b = match balance {
            Ok(b) => b,
            Err(e) => return (Read::Failure("hyperliquid".into(), format!("Hyperliquid: {e}")), None),
        };
        let vault = vault.ok();
        let Some(margin) = dexes.iter().fold(Num::of(b.account_value), |sum, d| sum + d.account_value).0 else {
            return (Read::Failure("hyperliquid".into(), "Hyperliquid: A number in the answer is out of range.".into()), None);
        };
        let now = Utc::now();
        let mut rows = vec![];
        if margin > Decimal::ZERO { rows.push(Self::row("Perps", margin, Some(margin), None, None, None, None, None)) }
        if b.spot_usdc > Decimal::ZERO { rows.push(Self::row("USDC", b.spot_usdc, Some(b.spot_usdc), None, None, None, None, None)) }
        rows.extend(spot.unwrap_or_default().iter().filter(|h| h.symbol != "USDC").map(Self::row_holding));
        if let Some((equity, _, locked_until)) = &vault && *equity > Decimal::ZERO {
            let note = locked_until.map_or_else(|| "earning".into(), |l| if l > now { format!("locked until {}", Fmt::date(l)) } else { "earning".into() });
            rows.push(Self::deposit("HLP", *equity, &note));
        }
        let facts = HlFacts {
            account_value: b.account_value, withdrawable: b.withdrawable, spot_usdc: b.spot_usdc, dexes,
            hlp_equity: vault.as_ref().map_or(Decimal::ZERO, |v| v.0), hlp_locked_until: vault.and_then(|v| v.2).filter(|l| *l > now),
        };
        (Read::Success(Some(Network { key: "hyperliquid".into(), name: "Hyperliquid".into(), usd: Decimal::ZERO, rows, note: None, hidden: vec![], hidden_more: 0 })),
         Some(facts))
    }

    /// The deposits' rates, read at most every 10 minutes (each is several protocols' APIs).
    pub async fn refresh_rates(&self) {
        let now = Utc::now();
        {
            let mut st = self.st_mut();
            if st.rates_at.is_some_and(|at| (now - at).num_milliseconds() < 600_000) { return }
            st.rates_at = Some(now);
        }
        let options: Vec<_> = self.load_yields(Some("USDC")).await.into_iter().filter(|o| o.asset == "USDC").collect();
        let rate = |matches: &dyn Fn(&str) -> bool| options.iter().find(|o| matches(&o.place)).map(|o| rounded(o.apy, 2, Rounding::Plain));
        let rates: HashMap<String, Decimal> = [
            ("kamino", rate(&|p| p.contains("Kamino"))),
            ("jupiter", rate(&|p| p.contains("Jupiter"))),
            ("aave", rate(&|p| p == "Aave · Base")),
            ("morpho", rate(&|p| p == "Morpho · Base")),
            ("hlp", rate(&|p| p.starts_with("HLP"))),
        ].into_iter().filter_map(|(k, r)| Some((k.to_string(), r?))).collect();
        self.st_mut().place_facts.rates = rates;
    }
}

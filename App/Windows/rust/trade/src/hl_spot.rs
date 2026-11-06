//! Hyperliquid's spot book (HLSpot.swift): HYPE and the other spot tokens bought with, and sold for, USDC on HyperCore.
//! A spot order is an order like a perp's, on asset 10000 + the pair's index (spotMeta), paid from the spot USDC
//! balance — which the perp balance tops up first when short (usdClassTransfer, the user's own signature; a unified
//! account has one balance and moves nothing). IOC within 1% of the mid; the exchange takes no order under 10 USDC.

use std::time::Duration;

use execution::Kind;
use hyperliquid::{MsgPack, Order, OrderKind, action};
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::network::Hyperliquid;
use ihatecrypto_core::plan::RiskCalc;
use ihatecrypto_core::stablecoins;
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};

use crate::executor::{hl_exchange, hl_post};
use crate::{Error, Executor, HlReply, Result, fail, num};

/// What JSONSerialization says of an answer that isn't JSON.
const UNREADABLE: &str = "The data couldn’t be read because it isn’t in the correct format.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotPair {
    /// 10000 + the pair's index in spotMeta's universe.
    pub asset: u32,
    /// The pair's key in allMids: «@107», or «PURR/USDC» for the first pairs.
    pub key: String,
    pub symbol: String,
    pub sz_decimals: i32,
}

/// `[[String: Any]]`: a list whose every item is an object, or None.
fn objects(v: Option<&Value>) -> Option<Vec<&Map<String, Value>>> { v?.as_array()?.iter().map(Value::as_object).collect() }

/// `NSNumber.intValue` of a JSON number.
fn int(v: Option<&Value>) -> Option<i64> { v.and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))) }

fn sleep(ms: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_millis(ms)) }

impl Executor {
    pub const SPOT_MINIMUM: Decimal = Decimal::TEN;

    /// Hyperliquid's info API (through `hl_url`): the answer as JSON, whatever it is.
    pub(crate) async fn hl_info(body: Value) -> Result<Value> {
        serde_json::from_slice(&hl_post("/info", &body).await?).map_err(|_| Error::Wallet(UNREADABLE.into()))
    }

    /// The token's pair against USDC (token 0) on HyperCore, from spotMeta: its tokens and its pairs.
    pub async fn spot_pair(symbol: &str) -> Result<SpotPair> {
        let meta = Self::hl_info(json!({"type": "spotMeta"})).await?;
        let (Some(tokens), Some(universe)) = (objects(meta.get("tokens")), objects(meta.get("universe"))) else {
            return fail("Hyperliquid's spot list didn't come. Nothing was signed.");
        };
        Self::spot_pair_in(symbol, &tokens, &universe)
    }

    /// `spot_pair` from spotMeta's two lists (Swift's `spotPair(_:tokens:universe:)`).
    pub fn spot_pair_in(symbol: &str, tokens: &[&Map<String, Value>], universe: &[&Map<String, Value>]) -> Result<SpotPair> {
        // «USDT» is USDT0 there, «USDe» USDE.
        let want = stablecoins::find(symbol, "hyperliquid").map_or(symbol, |c| c.symbol).to_uppercase();
        let name = |o: &Map<String, Value>| o.get("name").and_then(Value::as_str).map(str::to_string);
        let found = || -> Option<SpotPair> {
            let token = tokens.iter().find(|t| name(t).map(|n| n.to_uppercase()) == Some(want.clone()))?;
            let (index, sz_decimals) = (int(token.get("index"))?, int(token.get("szDecimals"))?);
            let pair = universe.iter().find(|p| {
                p.get("tokens").and_then(Value::as_array)
                    .and_then(|t| t.iter().map(|n| int(Some(n))).collect::<Option<Vec<i64>>>()) == Some(vec![index, 0])
            })?;
            let (pair_index, key) = (int(pair.get("index"))?, name(pair)?);
            Some(SpotPair {
                asset: u32::try_from(pair_index).ok()?.checked_add(10_000)?, key, symbol: name(token).unwrap_or_else(|| want.clone()),
                sz_decimals: i32::try_from(sz_decimals).ok()?,
            })
        };
        found().ok_or_else(|| Error::Execution(format!("{want} has no USDC pair on Hyperliquid's spot book. Nothing was signed.")))
    }

    pub async fn spot_mid(pair: &SpotPair) -> Result<Decimal> {
        let mids = Self::hl_info(json!({"type": "allMids"})).await?;
        // `as? [String: String]`: every value a string, or none of it reads.
        let mid = mids.as_object().filter(|m| m.values().all(Value::is_string))
            .and_then(|m| m.get(&pair.key)?.as_str().and_then(decimal)).filter(|m| *m > Decimal::ZERO);
        mid.ok_or_else(|| Error::Execution(format!("Hyperliquid has no price for {} now. Nothing was signed.", pair.symbol)))
    }

    /// What a spot buy (`usdc`) or sale (`sell`) gets at the book's mid now, fees taken off, for the route's card:
    /// «0.243 HYPE».
    // ponytail: the mid, not a walk of the book; the order itself takes at most 1% off it (IOC), a big one may get less.
    pub async fn spot_quote(symbol: &str, usdc: Option<Decimal>, sell: Option<Decimal>) -> Result<Option<String>> {
        let pair = Self::spot_pair(symbol).await?;
        let mid = Self::spot_mid(&pair).await?;
        if let Some(usdc) = usdc {
            return Ok(Some(format!("{} {}", Fmt::num(num(Num::of(usdc) / mid * (Decimal::ONE - RiskCalc::spot_taker_fee()))?, 6), pair.symbol)));
        }
        match sell {
            Some(sell) => Ok(Some(format!("{} USDC", Fmt::num(num(Num::of(sell) * mid * (Decimal::ONE - Self::spot_sell_fee()))?, 2)))),
            None => Ok(None),
        }
    }

    /// A spot sell pays the exchange's taker fee and ihatecrypto's; a buy only the exchange's.
    pub fn spot_sell_fee() -> Decimal { RiskCalc::spot_taker_fee() + RiskCalc::builder_fee() }

    /// One spot token for another at the books' mids less both orders' fees, for the card: «≈ 12.4 PURR».
    pub async fn spot_swap_quote(from: &str, to: &str, amount: Decimal) -> Result<String> {
        let a = Self::spot_pair(from).await?;
        let b = Self::spot_pair(to).await?;
        let (mid_a, mid_b) = (Self::spot_mid(&a).await?, Self::spot_mid(&b).await?);
        let out = num(Num::of(amount) * mid_a * (Decimal::ONE - Self::spot_sell_fee()) / mid_b * (Decimal::ONE - RiskCalc::spot_taker_fee()))?;
        Ok(format!("{} {}", Fmt::num(out, 6), b.symbol))
    }

    /// One spot token for another on Hyperliquid: its book trades each against USDC, so `from` is sold and the USDC it
    /// fetched buys `to`, each order IOC within 1% of the mid. None amount = all that's free of `from`. Both pairs are
    /// found first: a `to` the book can't buy (USDC itself, «USDC.e») or `from` again (USDT is USDT0 there) stops before
    /// the sale.
    pub async fn hl_spot_swap(&self, from: &str, to: &str, amount: Option<Decimal>) -> Result<String> {
        let a = Self::spot_pair(from).await?;
        let b = Self::spot_pair(to).await?;
        if a.asset == b.asset {
            return fail(format!("{} for {}: the same token on Hyperliquid's spot book. Nothing was signed.", a.symbol, b.symbol));
        }
        let before = self.spot_free("USDC").await?;
        let sold = self.hl_spot(false, from, None, amount).await?;
        let mut got = Decimal::ZERO;
        for _ in 0..20 {
            got = rounded(num(Num::of(self.spot_free("USDC").await?) - before)?, 2, Rounding::Down);
            if got > Decimal::ZERO { break }
            sleep(500).await;
        }
        if got < Self::SPOT_MINIMUM {
            return Ok(format!("{sold} The {} USDC it fetched stays on spot: the book takes orders from 10 USDC, so no {} was bought.",
                              Fmt::num(got, 2), to.to_uppercase()));
        }
        Ok(format!("{sold} {}", self.hl_spot(true, to, Some(got), None).await?))
    }

    /// What's free on the spot balance of `coin` (total less what open orders hold).
    pub(crate) async fn spot_free(&self, coin: &str) -> Result<Decimal> {
        let state = Self::hl_info(json!({"type": "spotClearinghouseState", "user": self.wallet.evm})).await?;
        let Some(balances) = objects(state.get("balances")) else { return fail("Hyperliquid's spot balance didn't come.") };
        let coin = coin.to_uppercase();
        let Some(row) = balances.into_iter().find(|b| b.get("coin").and_then(Value::as_str).map(str::to_uppercase) == Some(coin.clone())) else {
            return Ok(Decimal::ZERO);
        };
        let read = |k: &str| decimal(row.get(k).and_then(Value::as_str).unwrap_or("0")).unwrap_or_default();
        Ok(num(Num::of(read("total")) - read("hold"))?.max(Decimal::ZERO))
    }

    /// USDC from the perp balance to the spot one, the user's own signature, then the wait for it to show.
    pub(crate) async fn usdc_to_spot(&self, usdc: Decimal) -> Result<()> {
        let before = self.spot_free("USDC").await?;
        let n = self.nonce();
        let amount = action::number(usdc)?;
        let action = MsgPack::map(vec![
            ("type", MsgPack::str("usdClassTransfer")), ("amount", MsgPack::String(amount.clone())), ("toPerp", MsgPack::Bool(false)),
            ("nonce", MsgPack::Uint(n)), ("signatureChainId", MsgPack::str("0x66eee")), ("hyperliquidChain", MsgPack::str("Mainnet")),
        ]);
        let signature = self.signer.sign_hyperliquid_user(action.clone(), vec![]).await?;
        self.record("hyperliquid", &n.to_string(), Kind::Signed, &format!("usdClassTransfer perp → spot {amount}"))?;
        let reply = HlReply::parse(&hl_exchange(&action, n, &signature).await?);
        self.record("hyperliquid", &n.to_string(), if reply.is_error() { Kind::Failed } else { Kind::Confirmed }, &reply.to_string())?;
        if reply.is_error() { return fail(format!("Hyperliquid turned the move to spot away: {reply}. Nothing was bought.")) }
        let target = num(Num::of(before) + Num::of(usdc) * Decimal::new(999, 3))?;
        for _ in 0..20 {
            if self.spot_free("USDC").await? >= target {
                self.report(&format!("Spot balance: +{} USDC from perp.", Fmt::num(usdc, 2)));
                return Ok(());
            }
            sleep(500).await;
        }
        fail("The USDC didn't show on the spot balance within 10 s. Nothing was bought.")
    }

    /// Buys `symbol` on Hyperliquid's spot book for `usdc`, or sells `amount` of it (None = all that's free) for USDC.
    pub async fn hl_spot(&self, buy: bool, symbol: &str, usdc: Option<Decimal>, amount: Option<Decimal>) -> Result<String> {
        let pair = Self::spot_pair(symbol).await?;
        let mid = Self::spot_mid(&pair).await?;
        self.prepare_fee().await?;
        let places = pair.sz_decimals.max(0) as u32;
        let order = |is_buy: bool, price: Decimal, size: Decimal| Order {
            asset: pair.asset, is_buy, price, size, reduce_only: false, kind: OrderKind::Limit { tif: "Ioc".into() }, cloid: None,
        };
        if buy {
            let Some(usdc) = usdc.filter(|u| *u >= Self::SPOT_MINIMUM) else { return fail("Hyperliquid's spot book takes orders from 10 USDC.") };
            let free = self.spot_free("USDC").await?;
            if free < usdc {
                let need = rounded(num(Num::of(usdc) - free)?, 2, Rounding::Up);
                let hl = Hyperliquid::balance(&self.wallet.evm).await?;
                // A unified account has one balance: its spot USDC is already everything there is to move.
                if hl.unified {
                    return fail(format!("{} USDC free on Hyperliquid: {} is more. Nothing was signed.", Fmt::num(free, 2), Fmt::num(usdc, 2)));
                }
                if hl.withdrawable < need {
                    return fail(format!("{} USDC on spot and {} free on perp: {} is more. Nothing was signed.", Fmt::num(free, 2),
                                        Fmt::num(hl.withdrawable, 2), Fmt::num(usdc, 2)));
                }
                self.usdc_to_spot(need).await?;
            }
            let px = action::price(num(Num::of(mid) * Decimal::new(101, 2))?, pair.sz_decimals, true);
            // A price that rounds to nothing buys nothing (Swift's NaN size).
            let size = (Num::of(usdc) / px).0.map_or(Decimal::ZERO, |s| action::size(s, pair.sz_decimals));
            if size <= Decimal::ZERO { return fail(format!("{} USDC buys less than one step of {}.", Fmt::num(usdc, 2), pair.symbol)) }
            self.report(&format!("Hyperliquid spot: {} {} at most {} (mid {}), IOC.", Fmt::num(size, places), pair.symbol, Fmt::price(px), Fmt::price(mid)));
            let reply = self.post(&action::order(&[order(true, px, size)], "na")?, &format!("spot buy {}", pair.symbol)).await?;
            let HlReply::Filled { size: got, avg_px } = reply else {
                return fail(format!("The spot order didn't fill ({reply}). The USDC stays on your spot balance."));
            };
            return Ok(format!("Bought {} {} on Hyperliquid spot at {} — ≈ {}. It's on your spot balance.", Fmt::num(got, places), pair.symbol,
                              Fmt::price(avg_px), Fmt::usd((Num::of(got) * avg_px).0.unwrap_or_default())));
        }
        let free = self.spot_free(&pair.symbol).await?;
        let size = action::size(amount.unwrap_or(free).min(free), pair.sz_decimals);
        if size <= Decimal::ZERO { return fail(format!("No {} on your Hyperliquid spot balance.", pair.symbol)) }
        let worth = num(Num::of(size) * mid)?;
        if worth < Self::SPOT_MINIMUM {
            return fail(format!("{} {} is ≈ {}: Hyperliquid's spot book takes orders from 10 USDC.", Fmt::num(size, 2), pair.symbol, Fmt::usd(worth)));
        }
        let px = action::price(num(Num::of(mid) * Decimal::new(99, 2))?, pair.sz_decimals, true);
        self.report(&format!("Hyperliquid spot: sell {} {} at least {} (mid {}), IOC.", Fmt::num(size, places), pair.symbol, Fmt::price(px), Fmt::price(mid)));
        let reply = self.post(&action::order(&[order(false, px, size)], "na")?, &format!("spot sell {}", pair.symbol)).await?;
        let HlReply::Filled { size: sold, avg_px } = reply else {
            return fail(format!("The spot order didn't fill ({reply}). The {} stays on your spot balance.", pair.symbol));
        };
        Ok(format!("Sold {} {} on Hyperliquid spot at {} for ≈ {} USDC, on your spot balance.", Fmt::num(sold, places), pair.symbol, Fmt::price(avg_px),
                   Fmt::num((Num::of(sold) * avg_px).0.unwrap_or_default(), 2)))
    }
}

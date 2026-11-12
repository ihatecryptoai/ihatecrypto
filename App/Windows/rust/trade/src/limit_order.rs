//! Limit orders through NEAR Intents' 1Click (/v0/orders, LimitOrder.swift): a swap that waits for its price. Always
//! private (1Click takes only confidential orders), funded by one deposit from this wallet, paid out to its own address,
//! refunded to its own, with ihatecrypto's 0.05% (one_click::ORDER_FEE). Once funded it fills, in parts too, with no
//! signature: so before the deposit the order is read back from 1Click and checked whole (guard::order), and a price
//! already through the market is refused as the swap it would be.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use chrono::Utc;
use execution::Kind;
use ihatecrypto_core::foundation::{Date, Num, decimal, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::position_command::WalletCommand;
use protocols::one_click::{self, Order};
use rust_decimal::Decimal;

use crate::intent_swap::IntentSide;
use crate::swap::{dec, scale, tokens, whole};
use crate::{Executor, Result, fail, num, stop};

/// A limit order worked out: both sides as 1Click lists them, its quantity of the coin, the deposit it takes, and the
/// market now (dollars for one coin, from 1Click's own quote; None when it gives none).
#[derive(Debug, Clone, PartialEq)]
pub struct LimitPlan {
    pub coin: IntentSide,
    pub usdc: IntentSide,
    /// The coin in its smallest units: a sale's coins, a buy's target.
    pub quantity: String,
    /// What the wallet sends: a sale's coins, a buy's USDC (at most what was asked).
    pub deposit: Decimal,
    pub market: Option<Decimal>,
    /// The coins a buy gets or a sale gives, whole.
    pub coins: Decimal,
    /// The USDC a sale brings or a buy spends at the limit price, before fees.
    pub dollars: Decimal,
}

/// An open order in words for the board: the coin, side, its size and price, how much filled, until when, and 1Click's
/// price for what's left of it now (match_price; None while unquoted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitOrderView {
    pub id: String,
    pub sell: bool,
    pub coin: String,
    pub coins: Decimal,
    pub price: Decimal,
    pub filled: Decimal,
    pub status: String,
    pub until: Option<Date>,
    pub market: Option<Decimal>,
}

/// How an order that left the board ended: 1Click's status, what went in, and what comes out once the payout's network
/// fee (1Click's estimate) is off. `spent` and `got` are 0 for an order nothing filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitOrderEnd {
    pub status: String,
    pub spent: Decimal,
    pub got: Decimal,
    /// Dollars for one coin with every fee in: what the order cost, above a buy's limit and under a sale's.
    pub price: Option<Decimal>,
}

/// The price 1Click matches an order at, from a dry quote of its size: dollars for one coin with the payout's network fee
/// put back. The quote's output has that fee taken off, and the fee is fixed: on 0.7 USDC of SOL it read 1.4% over the
/// market (2026-11-12), while the order fills at its price before it. None without an output.
pub fn match_price(sell: bool, paid: Decimal, out: &str, withdraw_fee: Option<&str>, out_decimals: u32) -> Option<Decimal> {
    let out = decimal(out).filter(|o| *o > Decimal::ZERO)?;
    if paid <= Decimal::ZERO { return None }
    let got = (Num::of(out) + Num::of(withdraw_fee.and_then(decimal).unwrap_or_default()) / scale(out_decimals)).0?;
    (if sell { Num::of(got) / paid } else { Num::of(paid) / got }).0
}

/// A filled or expired order's sums from 1Click's order: the fills' input gross, their output less the payout's fee.
pub fn limit_end(o: &Order, sell: bool, base_decimals: u32, quote_decimals: u32) -> LimitOrderEnd {
    let n = |k: &str| dec(o.get(k).unwrap_or(""));
    let (in_unit, out_unit) = if sell { (scale(base_decimals), scale(quote_decimals)) } else { (scale(quote_decimals), scale(base_decimals)) };
    let spent = (Num::of(n("filledIn")) / in_unit).0.unwrap_or_default();
    let out = n("filledOut");
    let got = if out > Decimal::ZERO { (Num::of((out - n("estimatedWithdrawFee")).max(Decimal::ZERO)) / out_unit).0.unwrap_or_default() } else { Decimal::ZERO };
    let price = if spent > Decimal::ZERO && got > Decimal::ZERO { (if sell { Num::of(got) / spent } else { Num::of(spent) / got }).0 } else { None };
    LimitOrderEnd { status: o.get("fillStatus").unwrap_or("").into(), spent: spent.normalize(), got: got.normalize(), price }
}

/// The price 1Click would match an order's rest at now (match_price), quoted for that rest between its own addresses.
async fn limit_market(o: &Order, list: &[one_click::Token]) -> Option<Decimal> {
    let base = list.iter().find(|t| Some(t.asset_id.as_str()) == o.get("baseAsset"))?;
    let quote = list.iter().find(|t| Some(t.asset_id.as_str()) == o.get("quoteAsset"))?;
    let (recipient, refund) = (o.get("recipient")?, o.get("refundTo")?);
    let sell = o.get("side") == Some("SELL");
    let n = |k: &str| dec(o.get(k).unwrap_or(""));
    let rest = n(if sell { "quantity" } else { "maxAmountIn" }) - n("filledIn");
    if rest <= Decimal::ZERO { return None }
    let (origin, destination) = if sell { (&base.asset_id, &quote.asset_id) } else { (&quote.asset_id, &base.asset_id) };
    let q = one_click::quote(&one_click::Ask::new(true, origin, destination, &text(rest), refund, recipient)).await.ok()?;
    let (paid_decimals, out_decimals) = if sell { (base.decimals, quote.decimals) } else { (quote.decimals, base.decimals) };
    let paid = (Num::of(rest) / scale(u32::try_from(paid_decimals).ok()?)).0?;
    match_price(sell, paid, &q.amount_out_formatted, q.withdraw_fee.as_deref(), u32::try_from(out_decimals).ok()?)
}

impl LimitOrderView {
    pub fn state(&self) -> String { order_words(&self.status).map_or_else(|| self.status.to_lowercase(), str::to_string) }
}

/// The headroom a buy keeps under the USDC asked: 1Click's swap view rounds up and takes its fees from the input.
pub const LIMIT_BUY_HEADROOM: Decimal = Decimal::from_parts(997, 0, 0, false, 3);

/// 1Click's order states, in words.
pub fn order_words(status: &str) -> Option<&'static str> {
    Some(match status {
        "AWAITING_DEPOSIT" => "waiting for the deposit",
        "OPEN" => "resting",
        "PARTIALLY_FILLED" => "partly filled",
        "FILLED" => "filled",
        "PENDING_CANCEL" => "canceling",
        "CANCELED" => "canceled",
        "EXPIRED" => "expired",
        _ => return None,
    })
}

fn place_name(place: &str) -> String { WalletCommand::place_name(place) }

fn date(s: &str) -> Option<Date> { chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)) }

impl Executor {
    /// The order worked out, with the market as 1Click quotes this very size now. For the card too.
    #[allow(clippy::too_many_arguments)]
    pub async fn limit_plan(&self, sell: bool, coin: &str, on: &str, price: Decimal, amount: Decimal, usdc_on: &str, near: Option<&str>) -> Result<LimitPlan> {
        if price <= Decimal::ZERO || amount <= Decimal::ZERO { return fail("A limit order needs a price and an amount. Nothing was signed.") }
        let (c, u) = tokio::join!(self.intent_side(coin, on), self.intent_side("USDC", usdc_on));
        let (base, quote) = (c?, u?);
        // From the NEAR account ihatecrypto sends NEAR itself: said before 1Click makes an order nobody could fund.
        if (if sell { on } else { usdc_on }) == "near" && !(sell && base.symbol == "NEAR") {
            return fail(format!("From NEAR, ihatecrypto sends NEAR itself: {} there can't fund an order. Nothing was signed.", if sell { base.symbol.as_str() } else { "USDC" }));
        }
        if base.asset == quote.asset { return fail("That's USDC for USDC: nothing to wait for.") }
        let unit = scale(base.decimals);
        let (quantity, deposit) = if sell {
            let q = whole(num(Num::of(amount) * unit)?);
            (q, num(Num::of(q) / unit)?)
        } else {
            (whole(num(Num::of(amount) * LIMIT_BUY_HEADROOM / price * unit)?), amount)
        };
        if quantity <= Decimal::ZERO { return fail(format!("{} is less than one smallest unit of {}. Nothing was signed.", Fmt::num(amount, 2), base.symbol)) }
        let coins = num(Num::of(quantity) / unit)?;
        // The market as 1Click quotes this very size now: what the coins sell for, or what the USDC buys.
        let probe = if sell { text(quantity) } else { text(whole(num(Num::of(deposit) * scale(quote.decimals))?)) };
        let zero = "0".repeat(64);
        let near_or_zero = Some(near.unwrap_or(&zero));
        let market = match (self.intent_address(if sell { on } else { usdc_on }, near_or_zero), self.intent_address(if sell { usdc_on } else { on }, near_or_zero)) {
            (Ok(refund), Ok(recipient)) => {
                let (origin, destination) = if sell { (&base.asset, &quote.asset) } else { (&quote.asset, &base.asset) };
                one_click::quote(&one_click::Ask::new(true, origin, destination, &probe, &refund, &recipient)).await.ok().and_then(|q| {
                    match_price(sell, if sell { coins } else { deposit }, &q.amount_out_formatted, q.withdraw_fee.as_deref(),
                                if sell { quote.decimals } else { base.decimals })
                })
            }
            _ => None,
        };
        let dollars = num(Num::of(coins) * price)?;
        Ok(LimitPlan { coin: base, usdc: quote, quantity: text(quantity), deposit, market, coins, dollars })
    }

    /// Places the order: the market checked again, the order made, read back and guarded, the deposit sent, and 1Click
    /// followed until it holds the money. It then rests up to `days` days (None = 7).
    #[allow(clippy::too_many_arguments)]
    pub async fn limit_order(&self, sell: bool, coin: &str, on: &str, price: Decimal, amount: Decimal, usdc_on: &str, days: Option<i64>,
                             near: Option<&str>) -> Result<String> {
        let plan = self.limit_plan(sell, coin, on, price, amount, usdc_on, near).await?;
        let name = plan.coin.symbol.clone();
        let Some(market) = plan.market else {
            return fail(format!("1Click doesn't price {name} for USDC now, so an order on it may never fill. Nothing was signed."));
        };
        if guard::order::fills_at_once(sell, price, market) {
            return stop(format!("{name} is ≈ {} now: {} {} would fill at once, as a swap at the market price. Say it as a swap, or pick a price {} the market. \
                                 Nothing was signed.", Fmt::usd(market), if sell { "a sale at" } else { "a buy at" }, Fmt::usd(price), if sell { "above" } else { "below" }));
        }
        let (origin, destination) = if sell { (on, usdc_on) } else { (usdc_on, on) };
        let (refund, recipient) = (self.intent_address(origin, near)?, self.intent_address(destination, near)?);
        let ends = Utc::now() + chrono::Duration::days(days.unwrap_or(7));
        let order = one_click::create_order(sell, &plan.coin.asset, &plan.usdc.asset, &plan.quantity, &text(price), &refund, &recipient, ends).await?;
        let Some(address) = order.get("depositAddress").filter(|a| !a.is_empty()).map(str::to_string) else {
            return fail("1Click gave the order no deposit address. Nothing was signed.");
        };
        let funded = self.fund(&order, &plan, sell, coin, price, market, &address, origin, destination, &recipient, &refund, ends, near).await;
        if funded.is_err() {
            // Nothing sent to it: the order goes, so it neither shows as open nor stays behind a Try again.
            let sent = self.journal.entries().unwrap_or_default().iter().any(|e| e.chain == "1click-order" && e.id == order.id);
            if !sent { let _ = one_click::cancel_order(&order.id).await; }
        }
        funded
    }

    /// The order read back, guarded and funded, then followed until it rests. Nothing after the deposit fails.
    #[allow(clippy::too_many_arguments)]
    async fn fund(&self, order: &Order, plan: &LimitPlan, sell: bool, coin: &str, price: Decimal, market: Decimal, address: &str, origin: &str,
                  destination: &str, recipient: &str, refund: &str, ends: Date, near: Option<&str>) -> Result<String> {
        let name = &plan.coin.symbol;
        // Read back from 1Click by its id: the answer that made it is not what's checked.
        let bound = one_click::order(&order.id).await?;
        let paid = if sell { &plan.coin } else { &plan.usdc };
        let unit = scale(paid.decimals);
        let deposit = bound.get(if sell { "amountIn" } else { "maxAmountIn" }).unwrap_or("").to_string();
        let verdict = guard::order::check(&bound.fields, &guard::order::Asked {
            sell, base: &plan.coin.asset, quote: &plan.usdc.asset, quantity: &plan.quantity, price, deposit_address: address, recipient, refund_to: refund,
            deposit: &deposit, most: whole(num(Num::of(plan.deposit) * unit)?), fee: one_click::ORDER_FEE, deadline: ends.timestamp(),
            now: Utc::now().timestamp(),
        });
        if !verdict.ok() { return stop(format!("The guard stopped the order's deposit: {} Nothing was sent.", verdict.problems.join(" "))) }
        let sends = num(Num::of(dec(&deposit)) / unit)?;
        let what = format!("{} {} {name} at {} or {}", if sell { "sell" } else { "buy" }, Fmt::num(plan.coins, 8), Fmt::usd(price), if sell { "more" } else { "less" });
        self.report(&format!("Limit order (private): {what} · {name} ≈ {} now · until {}.", Fmt::usd(market), Fmt::date(ends)));
        let note = format!("limit {} {} {name} at {} · {} {} from {origin}", if sell { "sell" } else { "buy" }, Fmt::num(plan.coins, 8), text(price), Fmt::num(sends, 8), paid.symbol);
        self.intent_deposit(if sell { coin } else { "USDC" }, &paid.symbol, sends, &deposit, origin, address, near, ("1click-order", &order.id, &note)).await?;
        // Until 1Click sees the deposit: then the order rests. Nothing after the deposit may fail: it went, and a second
        // try would fund a second order.
        let mut status = "AWAITING_DEPOSIT".to_string();
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) && status == "AWAITING_DEPOSIT" {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Ok(o) = one_click::order(&order.id).await && let Some(s) = o.get("fillStatus") {
                if s != status { self.report(&format!("1Click · {} · {} s", order_words(s).map_or_else(|| s.to_lowercase(), str::to_string), started.elapsed().as_secs())) }
                status = s.to_string();
            }
        }
        if status != "AWAITING_DEPOSIT" { let _ = self.record("1click-order", &order.id, Kind::Confirmed, &status); }
        let rests = format!("It rests with 1Click until {} and fills, in parts too, when the market gets there: the {} comes to your own address on {} with no \
                             signature then. «show limit orders» shows it, «cancel limit orders» brings back what hasn't filled.",
                            Fmt::date(ends), if sell { "USDC" } else { name }, place_name(destination));
        Ok(match status.as_str() {
            "FILLED" | "PARTIALLY_FILLED" => format!("Limit order placed and already {}: {what}. {rests}", if status == "FILLED" { "filled" } else { "partly filled" }),
            "AWAITING_DEPOSIT" => format!("Limit order made and its deposit sent ({} {}): {what}. 1Click hasn't seen the deposit yet: it starts resting once it does. {rests}",
                                          Fmt::num(sends, 8), paid.symbol),
            _ => format!("Limit order placed, privately: {what}. {rests}"),
        })
    }

    /// What the journal learns about an order from 1Click now: confirmed once 1Click saw its deposit (it may see it after
    /// the move's three minutes), settled once its payout is final (filled, canceled or expired, the money out). None:
    /// nothing new for an entry last written as `kind`.
    pub fn limit_journal_step(kind: Kind, fill_status: &str, payout_final: bool) -> Option<Kind> {
        if payout_final { return Some(Kind::Settled) }
        (kind == Kind::Signed && fill_status != "AWAITING_DEPOSIT").then_some(Kind::Confirmed)
    }

    /// Brings the journal's orders up to date with 1Click, the newest 20 not yet settled (`only` those, when named): read
    /// with the board, so a deposit seen late or a cancel's refund is written down when the orders are next read. Returns
    /// those of `only` the journal now holds as settled.
    pub async fn sync_limit_journal(&self, only: Option<&HashSet<String>>) -> HashSet<String> {
        let Ok(all) = self.journal.entries() else { return HashSet::new() };
        let (mut latest, mut order) = (std::collections::HashMap::<String, Kind>::new(), Vec::<String>::new());
        for e in all.iter().filter(|e| e.chain == "1click-order") {
            if !latest.contains_key(&e.id) { order.push(e.id.clone()) }
            latest.insert(e.id.clone(), e.kind);
        }
        let named = |id: &String| only.is_none_or(|ids| ids.contains(id));
        let mut settled: HashSet<String> = order.iter().filter(|id| latest.get(*id) == Some(&Kind::Settled) && only.is_some_and(|ids| ids.contains(*id))).cloned().collect();
        for id in order.iter().rev().filter(|id| latest.get(*id) != Some(&Kind::Settled) && named(id)).take(20) {
            let Ok(o) = one_click::order(id).await else { continue };
            let fill = o.get("fillStatus").unwrap_or("").to_string();
            let Some(step) = Self::limit_journal_step(latest[id], &fill, o.get("isPayoutStatusFinal") == Some("true")) else { continue };
            if step == Kind::Settled { settled.insert(id.clone()); }
            // Another read (the board's, the loop's) may have written it while this one waited on 1Click.
            let last = self.journal.entries().unwrap_or_default().into_iter().rev().find(|e| e.chain == "1click-order" && &e.id == id).map(|e| e.kind);
            if last == Some(step) { continue }
            let amounts = format!("in {} · out {} · withdraw fee {}", o.get("filledIn").unwrap_or("?"), o.get("filledOut").unwrap_or("?"),
                                  o.get("estimatedWithdrawFee").unwrap_or("?"));
            let note = if step == Kind::Settled { format!("{fill} · payout {} · {amounts}", o.get("payoutStatus").unwrap_or("?")) } else { fill };
            let _ = self.record("1click-order", id, step, &note);
        }
        settled
    }

    /// The journal holds an order whose payout isn't final: one to follow, with the board or without.
    pub fn follows_limit_orders(&self) -> bool {
        let mut latest = std::collections::HashMap::<String, Kind>::new();
        for e in self.journal.entries().unwrap_or_default().into_iter().filter(|e| e.chain == "1click-order") { latest.insert(e.id, e.kind); }
        latest.values().any(|k| *k != Kind::Settled)
    }

    /// How one order ended, None when 1Click doesn't answer.
    pub async fn limit_order_end(&self, id: &str) -> Option<LimitOrderEnd> {
        let o = one_click::order(id).await.ok()?;
        o.get("fillStatus")?;
        let list = one_click::tokens().await.unwrap_or_default();
        let find = |k: &str| list.iter().find(|t| Some(t.asset_id.as_str()) == o.get(k)).and_then(|t| u32::try_from(t.decimals).ok());
        Some(match (find("baseAsset"), find("quoteAsset")) {
            (Some(b), Some(q)) => limit_end(&o, o.get("side") == Some("SELL"), b, q),
            _ => LimitOrderEnd { status: o.get("fillStatus").unwrap_or("").into(), spent: Decimal::ZERO, got: Decimal::ZERO, price: None },
        })
    }

    /// This wallet's open orders: those of the partner key that pay out to or refund to one of its own addresses, each
    /// with 1Click's price for its rest. `sync`: the journal brought up to date first (the board's read, not the loop's).
    pub async fn limit_orders(&self, near: Option<&str>, sync: bool) -> Result<Vec<(Order, LimitOrderView)>> {
        if sync { self.sync_limit_journal(None).await; }
        let own: HashSet<String> = [self.wallet.solana.as_str(), self.wallet.evm.as_str(), near.unwrap_or("")].iter()
            .filter(|a| !a.is_empty()).map(|a| a.to_lowercase()).collect();
        let list = one_click::tokens().await.unwrap_or_default();
        let mine = |key: &str, o: &Order| own.contains(&o.get(key).unwrap_or("").to_lowercase());
        let mut out: Vec<(Order, LimitOrderView)> = one_click::open_orders().await?.into_iter().filter(|o| o.is_open() && (mine("recipient", o) || mine("refundTo", o))).map(|o| {
            let t = list.iter().find(|t| Some(t.asset_id.as_str()) == o.get("baseAsset"));
            let decimals = t.map_or(0, |t| u32::try_from(t.decimals).unwrap_or(0));
            let sell = o.get("side") == Some("SELL");
            // A sale's fills spend the coin (their input), a buy's bring it (their output).
            let filled = tokens(dec(o.get(if sell { "filledIn" } else { "filledOut" }).unwrap_or("")), decimals);
            let coin = t.map_or_else(|| o.get("baseAsset").unwrap_or("?").to_string(), |t| if t.symbol == "wNEAR" { "NEAR".into() } else { t.symbol.clone() });
            let view = LimitOrderView {
                id: o.id.clone(), sell, coin, coins: tokens(dec(o.get("quantity").unwrap_or("")), decimals), price: dec(o.get("price").unwrap_or("")),
                filled, status: o.get("fillStatus").unwrap_or("").into(), until: o.get("deadline").and_then(date), market: None,
            };
            (o, view)
        }).collect();
        // 1Click's price for what each order has left, side by side: the board's market, not Hyperliquid's.
        let mut group = tokio::task::JoinSet::new();
        for (i, (o, _)) in out.iter().enumerate() {
            let (o, list) = (o.clone(), list.clone());
            group.spawn(async move { (i, limit_market(&o, &list).await) });
        }
        for (i, m) in group.join_all().await { out[i].1.market = m }
        Ok(out)
    }

    /// Cancels this wallet's open orders, those on `coin` only when it's named («id:<order id>»: that one order). What
    /// hasn't filled comes back to the address it came from. A last slice can still fill.
    pub async fn cancel_limit_orders(&self, coin: Option<&str>, near: Option<&str>) -> Result<String> {
        let id = coin.and_then(|c| c.strip_prefix("id:"));
        let open: Vec<LimitOrderView> = self.limit_orders(near, true).await?.into_iter().map(|o| o.1).filter(|v| {
            v.status != "PENDING_CANCEL" && match id {
                Some(id) => v.id == id,
                None => coin.is_none_or(|c| v.coin.eq_ignore_ascii_case(c)),
            }
        }).collect();
        if open.is_empty() {
            return Ok(if id.is_some() { "That limit order isn't open any more.".into() } else { coin.map_or("No open limit orders.".into(), |c| format!("No open limit order on {c}.")) });
        }
        let (mut done, mut failed) = (Vec::new(), Vec::new());
        for v in &open {
            let line = format!("{} {} {} at {}", if v.sell { "sell" } else { "buy" }, Fmt::num(v.coins, 8), v.coin, Fmt::usd(v.price));
            match one_click::cancel_order(&v.id).await {
                Ok(()) => done.push(line),
                Err(e) => failed.push(format!("{line}: {e}")),
            }
        }
        let n = done.len();
        let mut out = if n == 0 { String::new() } else {
            format!("Asked 1Click to cancel {}: {}. What hasn't filled comes back to the address it came from in a few minutes. A last slice can still fill.",
                    if n == 1 { "the order".into() } else { format!("{n} orders") }, done.join(", "))
        };
        if !failed.is_empty() { out += &format!("{}Not canceled: {}.", if out.is_empty() { "" } else { " " }, failed.join(". ")) }
        if n == 0 { return fail(out) }
        Ok(out)
    }
}

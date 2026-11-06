//! Robinhood Stock Tokens on Robinhood Chain (Stocks.swift): the registry says which contract is the real one; the chain
//! confirms it. Bought and sold against USDG on Uniswap (v4 or v3) or through Relay's swap, the price held to Robinhood's
//! own bid–ask (or DexScreener's price for a token by its contract).

use evm::U256;
use evm::abi::{string, uint_in};
use evm::erc20;
use evm::protocols::uniswap;
use execution::Kind;
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded, text};
use ihatecrypto_core::intent::Fmt;
use ihatecrypto_core::tokens::{LookupInfo, TokenLookup};
use net::evm_rpc::EvmRpc;
use protocols::relay;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::aave::EvmNetwork;
use crate::executor::{uint64, url_error};
use crate::swap::{DECIMALS, SYMBOL, V3_FEES, approves, d, dec, dollars, millionths, no_value, pairs, scale, sleep, tokens, web};
use crate::v4::StockWays;
use crate::{Executor, Result, fail, num, stop, token_balance, u256, word};

const NAME: [u8; 4] = [0x06, 0xfd, 0xde, 0x03];

/// A Robinhood Stock Token (or any token on Robinhood Chain by its contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub symbol: String,
    pub name: String,
    pub address: String,
    pub decimals: u32,
    pub logo: Option<String>,
}

/// Robinhood's own asset registry and prices (api.robinhood.com/rhj, documented at docs.robinhood.com/chain).
pub struct RobinhoodStocks;

impl RobinhoodStocks {
    /// Active tokens deployed on chain 4663.
    pub async fn registry() -> Result<Vec<Token>> {
        let (data, _) = net::get(&web("https://api.robinhood.com/rhj/assets"), &[]).await.map_err(url_error)?;
        Ok(Self::parse(&data))
    }

    pub fn parse(data: &[u8]) -> Vec<Token> {
        let Ok(j) = serde_json::from_slice::<Value>(data) else { return vec![] };
        let Some(assets) = j.get("assets").and_then(Value::as_array) else { return vec![] };
        assets.iter().filter_map(|a| {
            if a.get("status").and_then(Value::as_str) != Some("ASSET_STATUS_ACTIVE") { return None }
            let symbol = a.get("tokenSymbol")?.as_str()?;
            let deployment = a.get("deployments")?.as_array()?.iter().find(|d| d.get("chainId").and_then(Value::as_i64) == Some(4663))?;
            let address = deployment.get("contractAddress")?.as_str()?;
            Some(Token {
                symbol: symbol.into(), name: a.get("tokenName").and_then(Value::as_str).unwrap_or(symbol).replace(" • Robinhood Token", ""), address: address.into(),
                decimals: a.get("tokenDecimals").and_then(Value::as_u64).and_then(|d| u32::try_from(d).ok()).unwrap_or(18),
                logo: a.get("logoUrl").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
            })
        }).collect()
    }

    /// What Robinhood's registry says of a token: true for its own contract on Robinhood Chain, false for another
    /// contract there under one of its tickers (a copy), None for anything else. By the contract, never by the ticker alone.
    pub fn listed(address: &str, symbol: &str, chain: &str, registry: &[Token]) -> Option<bool> {
        if chain != "robinhood" { return None }
        if registry.iter().any(|t| t.address.to_lowercase() == address.to_lowercase()) { return Some(true) }
        registry.iter().any(|t| t.symbol.to_uppercase() == symbol.to_uppercase()).then_some(false)
    }

    /// Robinhood's own bid and ask for the token (api.robinhood.com/rhj/prices/{symbol}).
    pub async fn quote(symbol: &str) -> Result<Option<(Decimal, Decimal)>> {
        let (data, _) = net::get(&web(&format!("https://api.robinhood.com/rhj/prices/{symbol}")), &[]).await.map_err(url_error)?;
        Ok(Self::parse_quote(&data))
    }

    /// Robinhood's bid and ask; None for an error body (a 404's «no whitelisted asset», a 5xx), no quotes, or a side at zero.
    pub fn parse_quote(data: &[u8]) -> Option<(Decimal, Decimal)> {
        let j: Value = serde_json::from_slice(data).ok()?;
        let q = j.get("quotes")?.as_array()?.first()?;
        let side = |k: &str| q.get(k)?.as_str().and_then(decimal).filter(|x| *x > Decimal::ZERO);
        Some((side("bid")?, side("ask")?))
    }

    /// The registry's token, confirmed on-chain: same symbol and decimals at that address.
    pub async fn verified(symbol: &str, rpc: &EvmRpc) -> Result<Token> {
        let Some(token) = Self::registry().await?.into_iter().find(|t| t.symbol.to_uppercase() == symbol.to_uppercase()) else {
            return fail(format!("{symbol} isn't an active Robinhood Stock Token."));
        };
        let on_chain_symbol = string(&rpc.read(&token.address, &SYMBOL).await?);
        let on_chain_decimals = uint_in(&rpc.read(&token.address, &DECIMALS).await?, 0);
        if on_chain_symbol.as_deref() != Some(token.symbol.as_str()) || on_chain_decimals != Some(token.decimals as u64) {
            return fail(format!("The {symbol} contract on Robinhood Chain doesn't match Robinhood's registry. Nothing was signed."));
        }
        Ok(token)
    }

    /// A registry symbol, or any token by its contract («купи 0x… на $2»): it must trade on Robinhood Chain with at
    /// least $50k of liquidity (DexScreener), and its symbol and decimals are read from the contract itself.
    pub async fn resolve(symbol_or_contract: &str, rpc: &EvmRpc) -> Result<(Token, Option<LookupInfo>)> {
        if !symbol_or_contract.starts_with("0x") { return Ok((Self::verified(symbol_or_contract, rpc).await?, None)) }
        let address = symbol_or_contract;
        let Some(info) = TokenLookup::lookup(address).await?.into_iter().find(|i| i.chain == "robinhood") else {
            return fail("This contract doesn't trade on Robinhood Chain, and by contract ihatecrypto buys only there for now.");
        };
        if !info.liquidity_usd.is_some_and(|l| l >= Decimal::from(50_000)) {
            return fail(format!("{} has only {} of liquidity: too thin to trade safely.", info.symbol, Fmt::usd(info.liquidity_usd.unwrap_or_default())));
        }
        let Some(decimals) = uint_in(&rpc.read(address, &DECIMALS).await?, 0).filter(|d| *d <= 36) else {
            return fail("The contract doesn't answer decimals(): not a token ihatecrypto can trade.");
        };
        let symbol = string(&rpc.read(address, &SYMBOL).await?).unwrap_or_else(|| info.symbol.clone());
        let name = string(&rpc.read(address, &NAME).await?).unwrap_or_else(|| info.name.clone());
        Ok((Token { symbol, name, address: address.into(), decimals: decimals as u32, logo: info.image_url.clone() }, Some(info)))
    }
}

/// Relay's quote for a Robinhood Chain swap with every check passed (relay_swap's `checked`).
#[derive(Debug, Clone)]
struct Checked {
    quote: relay::Quote,
    call: relay::EvmStep,
    price: Decimal,
    out: Decimal,
}

/// One side of relay_swap: what goes in or comes out, and its decimals.
struct Side<'a> {
    token: &'a str,
    symbol: &'a str,
    decimals: u32,
}

impl Executor {
    /// The wallet's balance of a token, in its smallest units.
    pub async fn balance(&self, token: &str, rpc: &EvmRpc) -> Result<Decimal> {
        Ok(word(&rpc.read(token, &erc20::balance_of(&self.wallet.evm)?).await?, 0).unwrap_or_default())
    }

    /// The Uniswap v3 pool on Robinhood Chain with the most output for this trade, and that output: (fee, out). Amounts
    /// in the tokens' smallest units, as 256-bit words: 54 AI of 18 decimals is past 64 bits.
    pub async fn best_pool(&self, token_in: &str, token_out: &str, amount_in: Decimal) -> Result<Option<(u64, Decimal)>> {
        let (rpc, quoter) = (&EvmNetwork::robinhood().rpc, uniswap::v3(4663).map_or("", |u| u.0));
        let mut best: Option<(u64, Decimal)> = None;
        let Ok(amount) = u256(amount_in) else { return Ok(None) };
        for fee in V3_FEES {
            let Ok(call) = uniswap::quote_exact_input_single(token_in, token_out, amount, fee) else { continue };
            let Ok(answer) = rpc.read(quoter, &call).await else { continue };
            let Some(out) = word(&answer, 0).filter(|o| *o > best.map_or(Decimal::ZERO, |b| b.1)) else { continue };
            best = Some((fee, out));
        }
        Ok(best)
    }

    /// Buys a Stock Token (or any token by its contract) with USDC from Solana: Relay brings USDG (and gas) to
    /// Robinhood Chain, Uniswap swaps it. The price must sit within Robinhood's own bid–ask, or DexScreener's price for a
    /// contract (±5%); the swap pays only this wallet (the signer checks).
    pub async fn buy_stock(&self, symbol_or_contract: &str, usdc: Decimal) -> Result<String> {
        let net = EvmNetwork::robinhood();
        let (rpc, usdg, me) = (&net.rpc, evm::ROBINHOOD_USDG, self.wallet.evm.as_str());
        let (token, listing) = RobinhoodStocks::resolve(symbol_or_contract, rpc).await?;
        let symbol = token.symbol.as_str();
        let mut units = millionths(usdc)?;
        if units < 500_000 { return fail("At least 0.5 USDC: less isn't worth the bridge.") }
        // A few cents short on Robinhood Chain (300 asked, 299.86 USDG there after a bridge's fee): the buy spends what's
        // there, never a bridge from Solana for the cents. Up to $1 or 2% short.
        let there = token_balance(rpc, usdg, me).await?;
        if there < units && there > 0 && units - there <= 1_000_000.max(units / 50) { units = there }
        let (spend, spent) = (Decimal::from(units), dollars(units));
        // Every way quoted for this amount — Uniswap v4's pool without hooks, v3's best pool, Relay's swap — and the one
        // that gives the most taken (AI for 300 USDG: v4 1,906, Relay 1,880, v3's thin pool at $0.22 a token). The card
        // showed the same comparison (stock_quotes); it's asked again here, right before signing.
        let ways = self.stock_ways(&token, true, spend, listing.as_ref()).await;
        let Some(&(best, out)) = ways.ranked().first() else {
            let Some(listing) = &listing else { return fail(Self::no_pool(symbol, None)) };
            return self.relay_swap(&token, true, spend, listing).await;
        };
        self.report(&format!("{}: taking {}.", self.ways_line(&ways, |o| format!("{} {symbol}", Fmt::num(tokens(o, token.decimals), 2))), StockWays::title(best)));
        if let (true, Some(listing)) = (best == "relay", &listing) { return self.relay_swap(&token, true, spend, listing).await }
        let use_v4 = best == "v4";
        let price = (Num::of(spent) / tokens(out, token.decimals)).0.unwrap_or_default();
        let robinhood = if listing.is_none() { RobinhoodStocks::quote(symbol).await? } else { None };
        let q = Self::price_band(symbol, listing.as_ref(), robinhood)?;
        if Self::off_band(price, q) {
            // Relay's swap has its own checks (within 5% of DexScreener's price, at most 3% lost).
            if let Some(listing) = &listing {
                self.report(&format!("Uniswap would pay {} a token for this much, DexScreener says {}: its pools are thin. Relay's swap goes through the deeper ones.",
                                     Fmt::usd(price), Fmt::usd(q.0)));
                return self.relay_swap(&token, true, spend, listing).await;
            }
            return fail(format!("Uniswap's price {} is outside Robinhood's own {}–{}. Nothing was signed.", Fmt::usd(price), Fmt::usd(q.0), Fmt::usd(q.1)));
        }
        let pool = if use_v4 { "v4".to_string() } else { format!("v3 (pool fee {}%)", Fmt::num(Decimal::from(ways.v3.map_or(0, |v| v.0)) / Decimal::from(10_000), 2)) };
        self.report(&format!("{} ({symbol}): ≈ {} a token on Uniswap {pool}.", token.name, Fmt::usd(price)));
        self.ensure_evm(net, units, None).await?;
        let before = self.balance(&token.address, rpc).await?;
        // Quoted again with the money there: the price may have moved while the bridge ran. That quote is the one signed,
        // so it is held to the same band.
        let band = if listing.is_none() { format!("Robinhood's own {}–{}", Fmt::usd(q.0), Fmt::usd(q.1)) } else { format!("DexScreener's {}", Fmt::usd(q.0)) };
        let check_again = |fresh: Decimal| -> Result<()> {
            let now = if fresh > Decimal::ZERO { (Num::of(spent) / tokens(fresh, token.decimals)).0.unwrap_or_default() } else { Decimal::ZERO };
            if !Self::off_band(now, q) { return Ok(()) }
            fail(format!("Asked again with the USDG on Robinhood Chain, Uniswap's price {} is more than 5% off {band}. The USDG waits there. Nothing was swapped.",
                         Fmt::usd(now)))
        };
        let usdg_text = format!("{} USDG", Fmt::num(spent, 2));
        let hash = match (use_v4, &ways.v4) {
            (true, Some(route)) => {
                let Some(fresh) = self.v4_quote(&route.key, route.zero_for_one, spend, rpc).await else {
                    return fail(format!("Uniswap v4 no longer quotes {symbol}. The USDG waits on Robinhood Chain. Nothing was swapped."));
                };
                check_again(fresh)?;
                self.v4_swap(route, usdg, spend, rounded(num(Num::of(fresh) * 99 / 100)?, 0, Rounding::Down), &usdg_text,
                             &format!("Buy {symbol} for {usdg_text} on Uniswap v4"), rpc).await?
            }
            _ => {
                let Some(fresh) = self.best_pool(usdg, &token.address, spend).await? else { return fail(Self::no_pool(symbol, listing.as_ref())) };
                check_again(fresh.1)?;
                let minimum = rounded(num(Num::of(fresh.1) * 99 / 100)?, 0, Rounding::Down);
                let router = uniswap::ROBINHOOD_ROUTER;
                let swap = uniswap::exact_input_single(usdg, &token.address, fresh.0, me, U256::from(units), u256(minimum)?)?;
                self.approve_and_send(rpc, usdg, router, U256::from(units), &format!("Approve exactly {usdg_text} to Uniswap"), router, &swap,
                                      &format!("Buy {symbol} for {usdg_text}"), &[], 0).await?
            }
        };
        let mut after = self.balance(&token.address, rpc).await?;
        for _ in 0..10 {
            if after > before { break }
            sleep(1).await;
            after = self.balance(&token.address, rpc).await?;
        }
        let got = tokens(after - before, token.decimals);
        if let Some(traded) = &self.traded { traded(&format!("robinhood:{}", token.address.to_lowercase()), got, spent) }
        let each = if got > Decimal::ZERO { (Num::of(spent) / got).0.unwrap_or_default() } else { price };
        Ok(format!("Bought {} {symbol} ({}) for {usdg_text} — ≈ {} a token. You hold {} {symbol} on Robinhood Chain. {}/tx/{hash}", Fmt::num(got, 6), token.name,
                   Fmt::usd(each), Fmt::num(tokens(after, token.decimals), 6), net.explorer))
    }

    /// Relay's quote with every check: from and to this wallet, its call to the proxy after an exact approve, at most 3%
    /// (or $0.15) lost, the price within 5% of DexScreener's `p`.
    #[allow(clippy::too_many_arguments)]
    async fn relay_checked(&self, buy: bool, units: Decimal, amount_in: Decimal, from: &Side<'_>, to: &Side<'_>, proxy: &str, p: Decimal) -> Result<Checked> {
        let me = self.wallet.evm.as_str();
        let chain = relay::chain::ROBINHOOD;
        let quote = self.relay_quote(&relay::Ask {
            user: me.into(), recipient: me.into(), origin_chain: chain, destination_chain: chain, origin_currency: from.token.into(),
            destination_currency: to.token.into(), units: Some(text(units)), ..Default::default()
        }).await?;
        let binding = guard::binding_check(&pairs(&[("sender", &quote.sender.to_lowercase()), ("recipient", &quote.recipient.to_lowercase())]),
                                           &pairs(&[("sender", &me.to_lowercase()), ("recipient", &me.to_lowercase())]), "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let Some(call) = quote.evm.last().cloned().filter(|call| {
            call.to.to_lowercase() == proxy && call.chain_id == chain && no_value(call)
                && quote.evm[..quote.evm.len() - 1].iter().all(|s| approves(s, &from.token.to_lowercase(), proxy, units))
        }) else {
            return fail("Relay's transactions don't go to the approval proxy it publishes for Robinhood Chain, for exactly this amount. Nothing was signed.");
        };
        let out = tokens(dec(&quote.amount_out), to.decimals);
        // What goes in and what comes out, in dollars: USDG counts as a dollar, the token at Relay's own valuation.
        let (worth_in, worth_out) = if buy { (amount_in, dec(&quote.amount_out_usd)) } else { (dec(&quote.amount_in_usd), out) };
        if out <= Decimal::ZERO { return fail(format!("Relay doesn't quote {} now. Nothing was signed.", from.symbol)) }
        Self::check_loss(num(Num::of(worth_in) - worth_out)?, worth_in, num(Num::of(worth_in) * d("0.03"))?.max(d("0.15")), "Relay's swap")?;
        let price = if buy { num(Num::of(amount_in) / out)? } else { num(Num::of(out) / amount_in)? };
        if (Num::of(p) * d("1.05")).0.is_some_and(|hi| price > hi) || price < p * d("0.95") {
            return fail(format!("Relay's price {} is off DexScreener's {} by more than 5%. Nothing was signed.", Fmt::usd(price), Fmt::usd(p)));
        }
        Ok(Checked { quote, call, price, out })
    }

    /// A token Uniswap doesn't pool with USDG well enough, bought (`buy`) or sold by Relay's swap on Robinhood Chain: an
    /// exact approve of what goes in to the approval proxy Relay publishes for the chain, then its call to that same proxy,
    /// from and to this wallet; the price within 5% of DexScreener's and at most 3% (or $0.15) lost. `units`: what goes in,
    /// smallest units.
    async fn relay_swap(&self, token: &Token, buy: bool, units: Decimal, listing: &LookupInfo) -> Result<String> {
        let net = EvmNetwork::robinhood();
        let (rpc, usdg) = (&net.rpc, evm::ROBINHOOD_USDG);
        let stock = Side { token: &token.address, symbol: &token.symbol, decimals: token.decimals };
        let dollar = Side { token: usdg, symbol: "USDG", decimals: 6 };
        let (from, to) = if buy { (&dollar, &stock) } else { (&stock, &dollar) };
        let amount_in = tokens(units, from.decimals);
        // DexScreener's price, the one Relay's is checked against: none, and nothing is bridged or signed.
        let p = Self::price_band(&token.symbol, Some(listing), None)?.0;
        self.ensure_evm(net, if buy { uint64(units) } else { 0 }, None).await?;
        let Some(proxy) = relay::approval_proxy(relay::chain::ROBINHOOD).await?.map(|p| p.to_lowercase()) else {
            return fail("Relay publishes no approval proxy for Robinhood Chain.");
        };
        let first = self.relay_checked(buy, units, amount_in, from, to, &proxy, p).await?;
        self.report(&format!("{} ({}): no Uniswap v3 pool with USDG, so Relay {} it on Robinhood Chain: ≈ {} a token.", token.name, token.symbol,
                             if buy { "buys" } else { "sells" }, Fmt::usd(first.price)));
        // Batched (EIP-7702), the approve goes with the swap; otherwise first, on its own.
        let approve_note = format!("Approve exactly {} {} to Relay", Fmt::num(amount_in, 6), from.symbol);
        let batched = self.can_batch(rpc);
        if !batched { self.approve(rpc, from.token, &proxy, u256(units)?, &approve_note, &[]).await? }
        let before = self.relay_held(buy, token).await?;
        // Relay's route is priced for the moment it's quoted: after the approve's wait it's asked again, and a dry run that
        // still reverts (a pool moved: «T8: Insufficient Liquidity» on SHROOM) asks once more. The dry run comes before any
        // signature, so a retry signs nothing twice.
        let mut attempt = 0;
        let (hash, q) = loop {
            let q = self.relay_checked(buy, units, amount_in, from, to, &proxy, p).await?;
            // Asked again, the price may have moved: more than 10% less out than when the move started stops here and says
            // so, rather than trading at a price nobody saw.
            if let Some(drop) = Self::worse_by_more_than_ten_percent(first.out, q.out) {
                return fail(format!("⚠️ Asked again, Relay gives {} {} for {} {} instead of {} (−{}%): more than 10% worse, so nothing was swapped. \
                                     {}say «do it» again to {} at the new price.", Fmt::num(q.out, 2), to.symbol, Fmt::num(amount_in, 6), from.symbol,
                                    Fmt::num(first.out, 2), Fmt::num(drop, 2), if batched { "" } else { "The exact approve stays: " }, if buy { "buy" } else { "sell" }));
            }
            let note = format!("Relay: {} {} for {}", if buy { "buy" } else { "sell" }, token.symbol,
                               if buy { format!("{} USDG", Fmt::num(amount_in, 2)) } else { "USDG".into() });
            match self.approve_and_send(rpc, from.token, &proxy, u256(units)?, &approve_note, &q.call.to, &q.call.data, &note, &[], 0).await {
                Ok(h) => break (h, q),
                Err(error) if error.to_string().contains("eth_estimateGas") => {
                    if attempt >= 2 {
                        return fail(format!("Relay couldn't route {} {} into {} just now ({error}). Nothing was swapped{}. Try again in a minute, or with less.",
                                            Fmt::num(amount_in, 6), from.symbol, to.symbol, if batched { "" } else { ", and the exact approve stays for the next try" }));
                    }
                    self.report(&format!("Relay's route for {} moved before the swap: asking it again.", token.symbol));
                    sleep(2).await;
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        };
        self.record("relay", &q.quote.request_id, Kind::Signed, &format!("Robinhood {}→{}, tx {hash}", from.symbol, to.symbol))?;
        let mut after = self.relay_held(buy, token).await?;
        for _ in 0..30 {
            if after > before { break }
            sleep(1).await;
            after = self.relay_held(buy, token).await?;
        }
        if after <= before {
            return fail(format!("Relay took the {} but no {} arrived yet. Order {} is in the journal. {}/tx/{hash}", from.symbol, to.symbol, q.quote.request_id,
                                net.explorer));
        }
        let got = tokens(after - before, to.decimals);
        if let Some(traded) = &self.traded {
            traded(&format!("robinhood:{}", token.address.to_lowercase()), if buy { got } else { -amount_in }, if buy { amount_in } else { got });
        }
        if buy {
            return Ok(format!("Bought {} {} ({}) for {} USDG via Relay — ≈ {} a token. You hold {} {} on Robinhood Chain. {}/tx/{hash}", Fmt::num(got, 6), token.symbol,
                              token.name, Fmt::num(amount_in, 2), Fmt::usd((Num::of(amount_in) / got).0.unwrap_or_default()), Fmt::num(tokens(after, token.decimals), 6),
                              token.symbol, net.explorer));
        }
        Ok(format!("Sold {} {} for {} USDG via Relay. Say “move USDG from Robinhood to Solana” to bring it back. {}/tx/{hash}", Fmt::num(amount_in, 6), token.symbol,
                   Fmt::num(got, 2), net.explorer))
    }

    /// What relay_swap waits to grow: the token bought, or the USDG a sale brings.
    async fn relay_held(&self, buy: bool, token: &Token) -> Result<Decimal> {
        let rpc = &EvmNetwork::robinhood().rpc;
        if buy { self.balance(&token.address, rpc).await } else { Ok(Decimal::from(token_balance(rpc, evm::ROBINHOOD_USDG, &self.wallet.evm).await?)) }
    }

    /// What a Uniswap or Relay price is checked against: DexScreener's price for a token bought by its contract,
    /// Robinhood's own bid–ask for a Stock Token. Neither, and the trade stops: a price nobody can check isn't signed.
    /// (bid, ask)
    pub fn price_band(symbol: &str, listing: Option<&LookupInfo>, robinhood: Option<(Decimal, Decimal)>) -> Result<(Decimal, Decimal)> {
        if let Some(listing) = listing {
            let Some(p) = listing.price_usd.filter(|p| *p > Decimal::ZERO) else {
                return fail(format!("DexScreener gives no price for {symbol} right now, so the swap's price can't be checked. Nothing was signed."));
            };
            return Ok((p, p));
        }
        robinhood.map_or_else(|| fail(format!("Robinhood gives no quote for {symbol} right now, so Uniswap's price can't be checked against it. Nothing was signed. \
                                                Try again in a minute.")), Ok)
    }

    /// A buy's price more than 5% outside the band, or no price at all (zero, or a quote of nothing).
    pub fn off_band(price: Decimal, q: (Decimal, Decimal)) -> bool {
        price <= Decimal::ZERO || (Num::of(q.1) * d("1.05")).0.is_some_and(|hi| price > hi) || price < q.0 * d("0.95")
    }

    /// How much worse (%, one decimal) a fresh quote is than the first, when that's more than 10%; None otherwise.
    pub fn worse_by_more_than_ten_percent(first: Decimal, now: Decimal) -> Option<Decimal> {
        if !(first > Decimal::ZERO && now < first * d("0.9")) { return None }
        ((Num::of(Decimal::ONE) - Num::of(now) / first) * 100).rounded(1, Rounding::Plain).0
    }

    fn no_pool(symbol: &str, listing: Option<&LookupInfo>) -> String {
        if listing.is_none() { format!("No Uniswap pool trades {symbol} for USDG.") } else { format!("No Uniswap v3 pool on Robinhood Chain trades {symbol} for USDG.") }
    }

    /// Sells a Stock Token for USDG on Robinhood Chain; None = all of it.
    pub async fn sell_stock(&self, symbol_or_contract: &str, amount: Option<Decimal>) -> Result<String> {
        let net = EvmNetwork::robinhood();
        let (rpc, usdg, me) = (&net.rpc, evm::ROBINHOOD_USDG, self.wallet.evm.as_str());
        let (token, listing) = RobinhoodStocks::resolve(symbol_or_contract, rpc).await?;
        let symbol = token.symbol.as_str();
        let held = self.balance(&token.address, rpc).await?;
        let wanted = rounded(match amount { Some(a) => num(Num::of(a) * scale(token.decimals))?.min(held), None => held }, 0, Rounding::Down);
        if wanted <= Decimal::ZERO { return fail(format!("No {symbol} on Robinhood Chain.")) }
        let sold = tokens(wanted, token.decimals);
        // The way that pays the most of v4's pool, v3's best pool and Relay's swap; one far under the market (a thin pool:
        // selling 1,880 AI into v3's would have lost about a third) goes through Relay's deep pools instead.
        let ways = self.stock_ways(&token, false, wanted, listing.as_ref()).await;
        let Some(&(best, out)) = ways.ranked().first() else {
            let Some(listing) = &listing else { return fail(Self::no_pool(symbol, None)) };
            return self.relay_swap(&token, false, wanted, listing).await;
        };
        self.report(&format!("{}: taking {}.", self.ways_line(&ways, |o| format!("{} USDG", Fmt::num(o / Decimal::from(1_000_000), 2))), StockWays::title(best)));
        if let (true, Some(listing)) = (best == "relay", &listing) { return self.relay_swap(&token, false, wanted, listing).await }
        let use_v4 = best == "v4";
        let per_token = |out: Decimal| (Num::of(out) / 1_000_000 / sold).0.unwrap_or_default();
        let got = per_token(out);
        let robinhood = if listing.is_none() { RobinhoodStocks::quote(symbol).await? } else { None };
        let q = Self::price_band(symbol, listing.as_ref(), robinhood)?;
        if got < q.0 * d("0.95") {
            if let Some(listing) = &listing {
                self.report(&format!("Uniswap would pay {} a token for this much, DexScreener says {}: its pools are thin. Relay's swap goes through the deeper ones.",
                                     Fmt::usd(got), Fmt::usd(q.0)));
                return self.relay_swap(&token, false, wanted, listing).await;
            }
            return fail(format!("Uniswap would pay {} a token, more than 5% under Robinhood's own bid {}. Nothing was signed.", Fmt::usd(got), Fmt::usd(q.0)));
        }
        self.ensure_evm(net, 0, None).await?;
        let before = token_balance(rpc, usdg, me).await?;
        let amount_text = format!("{} {symbol}", Fmt::num(sold, 6));
        let hash = match (use_v4, &ways.v4) {
            (true, Some(route)) => {
                let Some(fresh) = self.v4_quote(&route.key, route.zero_for_one, wanted, rpc).await else {
                    return fail(format!("Uniswap v4 no longer quotes {symbol}. Nothing was swapped."));
                };
                // The quote signed, held to the same bid.
                let now = per_token(fresh);
                if now < q.0 * d("0.95") {
                    return fail(format!("Asked again, Uniswap would pay {} a token, more than 5% under {} {}. Nothing was swapped.", Fmt::usd(now),
                                        if listing.is_none() { "Robinhood's own bid" } else { "DexScreener's price" }, Fmt::usd(q.0)));
                }
                self.v4_swap(route, &token.address, wanted, rounded(num(Num::of(fresh) * 99 / 100)?, 0, Rounding::Down), &amount_text,
                             &format!("Sell {amount_text} on Uniswap v4"), rpc).await?
            }
            _ => {
                let Some(pool) = ways.v3 else { return fail(Self::no_pool(symbol, listing.as_ref())) };
                let router = uniswap::ROBINHOOD_ROUTER;
                let minimum = rounded(num(Num::of(pool.1) * 99 / 100)?, 0, Rounding::Down);
                let swap = uniswap::exact_input_single(&token.address, usdg, pool.0, me, u256(wanted)?, u256(minimum)?)?;
                self.approve_and_send(rpc, &token.address, router, u256(wanted)?, &format!("Approve exactly {amount_text} to Uniswap"), router, &swap,
                                      &format!("Sell {amount_text}"), &[], 0).await?
            }
        };
        let mut after = token_balance(rpc, usdg, me).await?;
        for _ in 0..10 {
            if after > before { break }
            sleep(1).await;
            after = token_balance(rpc, usdg, me).await?;
        }
        if let Some(traded) = &self.traded { traded(&format!("robinhood:{}", token.address.to_lowercase()), -sold, dollars(after.saturating_sub(before))) }
        // ponytail: Swift's UInt64 `after - before` would trap if the balance fell; here it says the fall.
        let change = Decimal::from(i128::from(after) - i128::from(before)) / Decimal::from(1_000_000);
        Ok(format!("Sold {amount_text} for {} USDG. Say “move USDG from Robinhood to Solana” to bring it back. {}/tx/{hash}", Fmt::num(change, 2), net.explorer))
    }
}

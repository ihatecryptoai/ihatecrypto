//! Where the USDC a move needs comes from when the place it's spent on hasn't it and the user named no source
//! (`FundingPlan.swift`): the free money across the portfolio, one leg a place, best first — Solana, the EVM networks
//! (in the order given, Ethereum last), Hyperliquid's withdrawable, NEAR sold for USDC. Hyperliquid's and NEAR's money
//! lands on Solana and crosses to an EVM target in one leg with Solana's own. Each leg takes a dollar more for Relay's
//! or 1Click's fee. The legs are the chat's own command forms, run as a chain that waits for each arrival
//! (Workspace.runSteps). A leg from an EVM network or Hyperliquid names what it sends; «move X USDC from Solana to …»
//! names what arrives, its fee on top.

use rust_decimal::Decimal;

use crate::foundation::{Rounding, dec, rounded, text};

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub key: String,
    pub name: String,
    pub usdc: Decimal,
}

impl Place {
    pub fn new(key: &str, name: &str, usdc: Decimal) -> Self { Self { key: key.into(), name: name.into(), usdc } }
}

/// The legs bringing `short` USDC to `target` («solana» or an EVM network's key); nil when all the free money together
/// isn't enough — the move's own check says what's missing.
#[allow(clippy::too_many_arguments)]
/// `intents`: the EVM networks NEAR Intents pays USDC on, when every leg must go through NEAR Intents (a loan guard's
/// top-up): Hyperliquid is left out, and each move says «via NEAR Intents». A target outside them plans as usual.
#[allow(clippy::too_many_arguments)]
pub fn legs(short: Decimal, target: &str, target_name: &str, solana: Decimal, evm: &[Place], hyperliquid: Decimal, near: Decimal,
            near_price: Option<Decimal>, intents: Option<&[&str]>) -> Option<Vec<String>> {
    if short <= Decimal::ZERO { return Some(vec![]) }
    // rust_decimal panics past 7.9·10²⁸ where Swift's Decimal goes on, and a panic stops the engine: amounts and the
    // price are held to bounds no real balance comes near (a trillion dollars, a millionth of a dollar), so a garbage
    // answer from a node or an API plans nothing odd instead.
    let big = Decimal::from(1_000_000_000_000u64);
    let cap = |x: Decimal| x.min(big);
    let (short, solana, hyperliquid, near) = (cap(short), cap(solana), cap(hyperliquid), cap(near));
    let evm: Vec<Place> = evm.iter().map(|p| Place { usdc: cap(p.usdc), ..p.clone() }).collect();
    let near_price = near_price.filter(|p| *p >= dec("0.000001")).map(cap);
    let fee = Decimal::ONE;
    // Less than this isn't worth a leg: Relay's minimum and its fee would eat it.
    let floor = Decimal::TWO;
    let half = dec("0.5");
    // NEAR left on the account for its storage and fees.
    let near_reserve = dec("0.05");
    let to_solana = target == "solana";
    let intents = intents.filter(|i| to_solana || i.contains(&target));
    let via = if intents.is_some() { " via NEAR Intents" } else { "" };
    let (mut direct, mut into_solana) = (Vec::new(), Vec::new());
    let (mut own, mut landed, mut delivered) = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    let need = |own: Decimal, landed: Decimal, delivered: Decimal| {
        short - delivered - if to_solana { landed } else { (own + landed - fee).max(Decimal::ZERO) }
    };
    // A first leg into the Solana pool also pays the one Solana → target leg's fee.
    let pool_fee = |own: Decimal, landed: Decimal| if !to_solana && own + landed == Decimal::ZERO { fee } else { Decimal::ZERO };
    // What a place gives: the cents wanted rounded up, or all it has (to the cent below) when that's less — rounding
    // every leg down left a buy a cent short and no plan at all.
    let take = |have: Decimal, want: Decimal| {
        let w = rounded(want, 2, Rounding::Up);
        if w <= have { w } else { rounded(have, 2, Rounding::Down) }
    };

    // Solana's own leg delivers at least Relay's half-dollar minimum.
    if !to_solana && solana >= fee + half && need(own, landed, delivered) > Decimal::ZERO {
        own = take(solana, need(own, landed, delivered).max(half) + fee);
    }
    for p in &evm {
        if p.key == target || need(own, landed, delivered) <= Decimal::ZERO || p.usdc < floor || intents.is_some_and(|i| !i.contains(&p.key.as_str())) { continue }
        let x = take(p.usdc, need(own, landed, delivered) + fee);
        if to_solana {
            into_solana.push(format!("move {} USDC from {} to Solana{via}", text(x), p.name));
            landed += x - fee;
        } else {
            direct.push(format!("move {} USDC from {} to {target_name}{via}", text(x), p.name));
            delivered += x - fee;
        }
    }
    if need(own, landed, delivered) > Decimal::ZERO && hyperliquid >= floor && intents.is_none() {
        let x = take(hyperliquid, need(own, landed, delivered) + fee + pool_fee(own, landed));
        into_solana.push(format!("withdraw {} USDC to Solana", text(x)));
        landed += x - fee;
    }
    if let Some(price) = near_price
        && need(own, landed, delivered) > Decimal::ZERO && price > Decimal::ZERO && near - near_reserve > Decimal::ZERO
        && (near - near_reserve) * price >= floor
    {
        let usd = need(own, landed, delivered) + fee + pool_fee(own, landed);
        let coins = (near - near_reserve).min(rounded(usd / price * dec("1.02"), 4, Rounding::Up));
        into_solana.push(format!("sell {} NEAR", text(coins)));
        landed += coins * price - fee;
    }
    if need(own, landed, delivered) > Decimal::ZERO { return None }
    // What arrives: the pool less the dollar kept for the fee. Asking the whole pool to arrive left Solana short of
    // the fee on top, and the chain stopped after its first legs.
    // Through NEAR Intents the leg names what's sent, not what arrives: the whole pool goes, the fee dollar covering 1Click's cents.
    let arriving = rounded(own + landed - if intents.is_none() { fee } else { Decimal::ZERO }, 2, Rounding::Down);
    let mut out = direct;
    out.extend(into_solana);
    if !to_solana && arriving > Decimal::ZERO { out.push(format!("move {} USDC from Solana to {target_name}{via}", text(arriving))) }
    Some(out)
}

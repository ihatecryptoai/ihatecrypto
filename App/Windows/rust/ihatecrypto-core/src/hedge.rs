//! «hedge SOL» (`Hedge.swift`): a short on Hyperliquid the size of the coins the wallet holds, so a fall in them is
//! covered. It has no take-profit, and its stop sits far out only to keep the margin from liquidation; the risk rules
//! count it net of the coins it covers.

use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::foundation::{Rounding, dec, group, rounded, whole};
use crate::plan::Market;

/// Leverage a hedge takes when none is named: liquidation ≈ 46% above the entry, half the notional as margin.
pub const LEVERAGE: Decimal = Decimal::TWO;

/// Wrapped coins and the market they track one to one.
const WRAPPED: [(&str, &str); 9] = [
    ("WETH", "ETH"), ("UETH", "ETH"), ("WBTC", "BTC"), ("CBBTC", "BTC"), ("UBTC", "BTC"), ("BTCB", "BTC"), ("USOL", "SOL"), ("WSOL", "SOL"),
    ("WHYPE", "HYPE"),
];

/// What the wallet holds of each market's coin, from what it holds on every network: (symbol, amount, price). `price`
/// nil is a network's own balance (SOL, ETH), taken as it is. A token counts only when its own price is within a tenth
/// of its market's `marks` price: an airdropped look-alike, a namesake (MetaDAO's META is not Meta's stock, a GOLD
/// memecoin not gold) or a token in other units (kPEPE, a share multiplier) would otherwise size a hedge the rules
/// count as riskless. The market is the coin itself, its wrapped form, or a stock token's HIP-3 market (Robinhood's
/// «TSLA», Ondo's «NVDAon» → «xyz:TSLA», «xyz:NVDA»).
pub fn spot(held: &[(&str, Decimal, Option<Decimal>)], marks: &HashMap<String, Decimal>) -> HashMap<String, Decimal> {
    // How far a token's own price may be from its market's for it to count as that coin.
    let tolerance = dec("0.1");
    // A ticker's market: the main exchange's, else the first HIP-3 one by name.
    let mut names: Vec<&String> = marks.keys().collect();
    names.sort();
    let mut by_ticker: HashMap<&str, &str> = HashMap::new();
    for name in names {
        let ticker = name.split(':').rfind(|s| !s.is_empty()).unwrap_or(name);
        if !by_ticker.contains_key(ticker) || !name.contains(':') { by_ticker.insert(ticker, name.as_str()); }
    }
    let mut out: HashMap<String, Decimal> = HashMap::new();
    for &(symbol, amount, price) in held {
        let mut ticker = symbol.to_uppercase();
        if let Some(m) = whole(symbol, "([A-Z]{1,6})on") { ticker = group(&m, 1).into() }
        if let Some((_, coin)) = WRAPPED.iter().find(|(w, _)| *w == ticker) { ticker = coin.to_string() }
        let Some(&market) = by_ticker.get(ticker.as_str()) else { continue };
        let Some(&mark) = marks.get(market) else { continue };
        if amount <= Decimal::ZERO || mark <= Decimal::ZERO { continue }
        // Checked: a garbage mark or amount mustn't stop the engine (rust_decimal panics on overflow, Swift's Decimal doesn't).
        let Some(band) = mark.checked_mul(tolerance) else { continue };
        if price.is_some_and(|p| p.checked_sub(mark).is_none_or(|d| d.abs() > band)) { continue }
        let held = out.entry(market.to_string()).or_default();
        *held = held.saturating_add(amount);
    }
    out
}

/// The stop a hedge gets when none is named, in %: four fifths of the way to liquidation, so the exchange closes it
/// before liquidation takes the margin and its fee. None where Swift gets NaN: no leverage, or a market with none.
pub fn guard_stop(leverage: Decimal, market: &Market) -> Option<Decimal> {
    let l = Decimal::ONE.checked_div(Decimal::TWO.checked_mul(Decimal::from(market.max_leverage))?)?;
    let pct = Decimal::ONE.checked_div(leverage)?.checked_sub(l)?.checked_div(Decimal::ONE + l)?.checked_mul(Decimal::from(80))?;
    Some(rounded(pct, 1, Rounding::Down))
}

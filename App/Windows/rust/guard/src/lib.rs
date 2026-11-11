//! The last check before signing (`Guard/Core/Guard.swift`) and the screening of outside text before a model reads it
//! (`Injection.swift`). Plain code, not a model: whatever a web page, token metadata or a prompt injection made the
//! planner or an API return, only programs on the list run, and money goes only to the person's own accounts or to
//! addresses bound to a quote they approved. Same decisions and the same words as the Swift: the window shows them.

pub mod injection;
pub mod jev;
pub mod sol;

use std::collections::{BTreeMap, HashMap, HashSet};

use hyperliquid::{MsgPack, builder};
use rust_decimal::Decimal;

/// What was verified, in words for the confirmation screen, and why not to sign: any problem means don't.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    pub checks: Vec<String>,
    pub problems: Vec<String>,
    /// Signable, but only after a fresh in-person prompt that names these, whatever session is open (a swap whose
    /// slippage is past the person's limit).
    pub ask_afresh: Vec<String>,
    /// What leaves the wallet, as read: a transfer (with whom it goes to: an address, or its owner for a token account the
    /// signer could tie to one), or what a swap gives (no `to`). The automation wallet's dollars are counted from these.
    pub outflows: Vec<Outflow>,
    /// A swap is in it: its rate is the person's to judge (the quote it's held to comes with the request), so a person
    /// signs it with a prompt of its own, never inside an open session (the 2026-11-08 review).
    pub swaps: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outflow {
    /// A mint, «SOL», or an EVM token contract / «native».
    pub asset: String,
    pub units: u64,
    pub to: Option<String>,
}

impl Verdict {
    pub fn ok(&self) -> bool { self.problems.is_empty() }

    pub fn merge(&mut self, other: Verdict) {
        self.checks.extend(other.checks);
        self.problems.extend(other.problems);
        self.ask_afresh.extend(other.ask_afresh);
        self.outflows.extend(other.outflows);
        self.swaps |= other.swaps;
    }
}

pub(crate) fn prefix(s: &str, n: usize) -> String { s.chars().take(n).collect() }

pub(crate) fn suffix(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

/// `\(Decimal(raw) / pow(10, decimals))`: exact, no trailing zeros, never an exponent.
pub(crate) fn units(raw: u64, decimals: u32) -> String {
    Decimal::from_i128_with_scale(raw as i128, decimals).normalize().to_string()
}

/// A Double as Swift's `description` writes it: the shortest digits that read back, «1.0» for a whole number, and
/// «5e-05» below 10⁻⁴ or past 2⁵³.
pub(crate) fn swift_double(x: f64) -> String {
    if x == 0.0 { return "0.0".into() }
    let sign = if x < 0.0 { "-" } else { "" };
    let e = format!("{:e}", x.abs());
    let (mantissa, exp) = e.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an integer exponent");
    if x.abs() < 1e-4 || x.abs() > 9_007_199_254_740_992.0 {
        return format!("{sign}{mantissa}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let digits = mantissa.replace('.', "");
    let point = exp + 1; // digits before the decimal point
    if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{sign}{digits}{}.0", "0".repeat(point as usize - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..point as usize], &digits[point as usize..])
    }
}

/// Lamports as SOL in the words the Swift guard uses: `\(Double(lamports) / 1e9)`.
pub(crate) fn sol_text(lamports: u64) -> String { swift_double(lamports as f64 / 1e9) }

// MARK: - A second simulation

/// What a second simulation, run after the person pressed Sign, may differ in from the one the window showed (research
/// E-4; ScamSniffer: a site changes the state in the seconds between the look and the signature). Why what leaves now
/// isn't what was shown: an asset leaves that didn't, or more than 1 % more of one leaves. Keys are assets, values their
/// signed change. Arrivals aren't compared: the transaction's own minimum holds them. `names`: how an asset reads.
pub fn outflows_grew(shown: &HashMap<String, Decimal>, now: &HashMap<String, Decimal>, names: &HashMap<String, String>) -> Option<String> {
    let sorted: BTreeMap<_, _> = now.iter().collect();
    for (asset, change) in sorted {
        if *change >= Decimal::ZERO { continue }
        let name = names.get(asset).unwrap_or(asset);
        let Some(was) = shown.get(asset).filter(|w| **w < Decimal::ZERO) else {
            return Some(format!("{name} would leave too, which the window didn't show"));
        };
        if -*change > -*was * Decimal::new(101, 2) { return Some(format!("more {name} would leave than the window showed")) }
    }
    None
}

// MARK: - Hyperliquid

pub mod hl {
    use super::*;

    /// Hyperliquid's own market-making vault; the only vault money may go to without the person naming it.
    pub const HLP_VAULT: &str = "0xdfc24b077bc1425ad1dea75bcb6f8158e10df303";
    /// The top of Settings › Trading's leverage stepper.
    pub const MAX_LEVERAGE: u64 = 20;

    fn string(v: Option<&MsgPack>) -> Option<&str> { match v { Some(MsgPack::String(s)) => Some(s), _ => None } }

    fn market(v: Option<&MsgPack>) -> String {
        match v { Some(MsgPack::Uint(a)) => format!("market #{a}"), _ => "a market".into() }
    }

    fn side(v: Option<&MsgPack>) -> &'static str { if v == Some(&MsgPack::Bool(true)) { "buy" } else { "sell" } }

    /// Orders and cancels trade inside the account. Anything that moves money must go to an approved address.
    pub fn check(action: &MsgPack, owner: &str, recipients: &HashSet<String>, agent: Option<&str>) -> Verdict {
        let mut v = Verdict::default();
        if !matches!(action, MsgPack::Map(_)) {
            v.problems.push("Unrecognized action.".into());
            return v;
        }
        let text = |k: &str| action.text(k);
        let mut allowed: HashSet<String> = recipients.iter().map(|r| r.to_lowercase()).collect();
        allowed.insert(owner.to_lowercase());
        match text("type") {
            Some("updateLeverage") => {
                // The trading key signs this without a prompt: the signer holds it to the most the risk rules allow.
                match action.get("leverage") {
                    Some(MsgPack::Uint(l)) if (1..=MAX_LEVERAGE).contains(l) => v.checks.push(format!("Leverage {l}× inside your account.")),
                    _ => v.problems.push(format!("Leverage above {MAX_LEVERAGE}×: the most ihatecrypto's risk rules allow.")),
                }
            }
            Some("order") => {
                // What each order does, for a prompt when the main key signs it: side, size, limit price, market.
                if let Some(MsgPack::Array(orders)) = action.get("orders") {
                    for o in orders.iter().filter(|o| matches!(o, MsgPack::Map(_))) {
                        let reduces = if o.get("r") == Some(&MsgPack::Bool(true)) { ", only reducing a position" } else { "" };
                        v.checks.push(format!("{} {} on {} at {}{reduces} → your account", side(o.get("b")), string(o.get("s")).unwrap_or("?"),
                                              market(o.get("a")), string(o.get("p")).unwrap_or("?")));
                    }
                }
                // The trading key signs orders without a prompt: a fee may go to ihatecrypto, at its rate, and nowhere else.
                let Some(b) = action.get("builder") else {
                    v.checks.push("Trading action inside your account.".into());
                    return v;
                };
                let ours = match b {
                    MsgPack::Map(pairs) => pairs.len() == 2 && b.get("b") == Some(&MsgPack::str(builder::ADDRESS))
                        && matches!(b.get("f"), Some(MsgPack::Uint(f)) if *f <= builder::FEE),
                    _ => false,
                };
                if !ours {
                    v.problems.push(format!("The order pays a fee to someone other than ihatecrypto, or more than {}.", builder::RATE));
                    return v;
                }
                v.checks.push(format!("Trading action inside your account, with ihatecrypto's fee {}.", builder::RATE));
            }
            Some("cancel") => v.checks.push("Trading action inside your account.".into()),
            Some("trailingStop") => {
                // Signed without a prompt: only one that can close, never open or add to, a position.
                if action.get("reduceOnly") != Some(&MsgPack::Bool(true)) {
                    v.problems.push("A trailing stop that could open or add to a position is not allowed.".into());
                    return v;
                }
                let pct = match action.get("retracement") {
                    Some(MsgPack::Map(r)) => match r.first() { Some((_, MsgPack::String(p))) => p.as_str(), _ => "?" },
                    _ => "?",
                };
                v.checks.push(format!("Trailing stop: {} {} on {} once the price falls back {pct} from its best, only reducing a position → your account",
                                      side(action.get("isBuy")), text("sz").unwrap_or("?"), market(action.get("asset"))));
            }
            Some("twapOrder") => {
                // Parts at the market inside the account, like an order; it takes no builder field. Up to 4 hours: a stop
                // that fires leaves a TWAP buying, and only the app watching stops it.
                let t = action.get("twap");
                let minutes = match t {
                    Some(MsgPack::Map(pairs)) if pairs.len() == 6 => match t.and_then(|t| t.get("m")) {
                        Some(MsgPack::Uint(m)) if (5..=240).contains(m) => Some(*m),
                        _ => None,
                    },
                    _ => None,
                };
                let (Some(minutes), Some(t)) = (minutes, t) else {
                    v.problems.push("A TWAP that isn't 5 minutes to 4 hours of parts inside your account is not allowed.".into());
                    return v;
                };
                let reduces = if t.get("r") == Some(&MsgPack::Bool(true)) { ", only reducing a position" } else { "" };
                v.checks.push(format!("TWAP: {} {} on {} over {minutes} min at the market{reduces} → your account",
                                      side(t.get("b")), string(t.get("s")).unwrap_or("?"), market(t.get("a"))));
            }
            Some("twapCancel") => v.checks.push("Stops a TWAP inside your account.".into()),
            Some("setReferrer") => {
                if text("code") == Some(builder::REFERRAL_CODE) {
                    v.checks.push(format!("Referral code {}: 4% off your Hyperliquid fees.", builder::REFERRAL_CODE));
                } else {
                    v.problems.push(format!("Referral code {} is not ihatecrypto's.", text("code").unwrap_or("?")));
                }
            }
            Some("approveBuilderFee") => {
                let ours = text("builder").map(str::to_lowercase).as_deref() == Some(builder::ADDRESS);
                match text("maxFeeRate") {
                    Some(builder::RATE) if ours => v.checks.push(format!("ihatecrypto's fee {} of each Hyperliquid trade → {}…{}.",
                                                                         builder::RATE, prefix(builder::ADDRESS, 6), suffix(builder::ADDRESS, 4))),
                    Some(builder::REVOKE) if ours => v.checks.push("Takes back the approval of ihatecrypto's fee.".into()),
                    rate => v.problems.push(format!("Approves a fee of {} for {}: only ihatecrypto's {} is allowed.",
                                                    rate.unwrap_or("?"), text("builder").unwrap_or("?"), builder::RATE)),
                }
            }
            Some("usdClassTransfer") => v.checks.push("Transfer between your spot and perp balances.".into()),
            Some("spotSend" | "withdraw3" | "usdSend" | "sendAsset") => {
                let to = text("destination").map(str::to_lowercase).unwrap_or_default();
                // What moves, for the prompt: the amount and, where it says, the token (a spot token is «NAME:0x…»).
                let token = text("token").map_or("USDC", |t| t.split(':').find(|p| !p.is_empty()).unwrap_or(""));
                let amount = format!("{} {token}", text("amount").unwrap_or("?"));
                if allowed.contains(&to) { v.checks.push(format!("{amount} → {}…{}.", prefix(&to, 6), suffix(&to, 4))) }
                else { v.problems.push(format!("{amount} go to someone else's address {to}.")) }
            }
            Some("vaultTransfer") => {
                let vault = text("vaultAddress").map(str::to_lowercase).unwrap_or_default();
                // usd: micro-dollars (6 decimals); isDeposit: into the vault, else out of it.
                let deposit = action.get("isDeposit") == Some(&MsgPack::Bool(true));
                let usd = match action.get("usd") { Some(MsgPack::Uint(u)) => units(*u, 6), _ => "0".into() };
                let named = if vault == HLP_VAULT { "the HLP vault".to_string() } else { format!("the vault {}…{}", prefix(&vault, 6), suffix(&vault, 4)) };
                if vault == HLP_VAULT || allowed.contains(&vault) {
                    v.checks.push(if deposit { format!("${usd} → {named} (locked for days).") } else { format!("${usd} out of {named} → your account.") });
                } else {
                    v.problems.push(format!("Transfer to an unknown vault {vault}."));
                }
            }
            Some("approveAgent") => {
                let given = text("agentAddress");
                if agent.is_some_and(|a| given.map(str::to_lowercase) == Some(a.to_lowercase())) { v.checks.push("Approves your agent key.".into()) }
                else { v.problems.push(format!("Approves an unknown agent {}.", given.unwrap_or("?"))) }
            }
            other => v.problems.push(format!("Action {} is not allowed.", other.unwrap_or("?"))),
        }
        v
    }
}

// MARK: - Quote bindings (1Click, Relay)

/// Every field a quote binds (recipient, refund address, assets, amount) must be what the person approved.
pub fn binding_check(bound: &HashMap<String, String>, expected: &HashMap<String, String>, subject: &str) -> Verdict {
    let mut v = Verdict::default();
    for (field, value) in expected.iter().collect::<BTreeMap<_, _>>() {
        if bound.get(field).map(|b| b.to_lowercase()) != Some(value.to_lowercase()) {
            v.problems.push(format!("{subject}: {field} = {} instead of {value}.", bound.get(field).map_or("none", String::as_str)));
        }
    }
    if v.ok() { v.checks.push(format!("{subject}: recipient, sender and amount are yours.")) }
    v
}

/// A 1Click deposit address is safe to pay only if 1Click has it bound to a quote with our recipient and refund
/// address, the assets we asked for and our amount. A substituted address fails this.
pub fn deposit_check(bound: &HashMap<String, String>, recipient: &str, refund_to: &str, origin: &str, destination: &str, amount: &str) -> Verdict {
    let mut v = Verdict::default();
    let expected = [("amount", amount), ("destinationAsset", destination), ("originAsset", origin), ("recipient", recipient), ("refundTo", refund_to)];
    for (field, value) in expected {
        if bound.get(field).map(|b| b.to_lowercase()) == Some(value.to_lowercase()) { continue }
        v.problems.push(format!("The deposit address is bound to a different {field}: {} instead of {value}.", bound.get(field).map_or("none", String::as_str)));
    }
    if v.ok() { v.checks.push("The deposit address is bound to a quote with your recipient, refund address and amount.".into()) }
    v
}

/// A private move goes only where 1Click itself holds the deposit as confidential (Confidential Intents): else it would
/// go in the open.
pub fn deposit_confidential(bound: &HashMap<String, String>) -> Verdict {
    let mut v = Verdict::default();
    if bound.get("confidentiality").is_some_and(|c| c.eq_ignore_ascii_case("basic")) {
        v.checks.push("1Click holds the deposit as confidential.".into());
    } else {
        v.problems.push(format!("1Click didn't take it as private (Confidential Intents): it says «{}».", bound.get("confidentiality").map_or("nothing", String::as_str)));
    }
    v
}

/// A deposit an MPP service asks for through NEAR Intents (its quote, not ours): 1Click itself must hold the address for
/// an exact-output quote paying the service's recipient exactly `amount_out`, refunding to the address the service named,
/// taking USDC from the network paid on and, when it's known, paying the asset the card shows. And the deposit can't be
/// more than 1Click's quote takes plus its slippage, under its minimum, or after its quote runs out: all of those would
/// only come back as a refund — to the service. `bound` as one_click::bound_request reads it; `now` in Unix seconds.
#[allow(clippy::too_many_arguments)]
pub fn deposit_charge(bound: &HashMap<String, String>, recipient: &str, refund_to: &str, origin: &str, destination: Option<&str>,
                      amount_out: &str, deposit: u64, now: i64) -> Verdict {
    let mut v = Verdict::default();
    if bound.get("swapType").map(String::as_str) != Some("EXACT_OUTPUT") {
        v.problems.push(format!("1Click holds it as {} quote, not one paying an exact amount.", bound.get("swapType").map_or("an unknown", String::as_str)));
    }
    let mut expected = vec![("amount", amount_out), ("originAsset", origin), ("recipient", recipient), ("refundTo", refund_to)];
    if let Some(d) = destination { expected.push(("destinationAsset", d)) }
    expected.sort_by_key(|e| e.0);
    for (field, value) in expected {
        if bound.get(field).map(|b| b.to_lowercase()) == Some(value.to_lowercase()) { continue }
        v.problems.push(format!("The deposit address is bound to a different {field}: {} instead of {value}.", bound.get(field).map_or("none", String::as_str)));
    }
    match bound.get("quoteAmountIn").and_then(|a| a.parse::<u64>().ok()) {
        // 1Click's own price for this deposit, plus 1% for its slippage and rounding.
        Some(asked) if deposit > asked + asked / 100 + 1 => v.problems.push(format!("The service asks {deposit} units, more than the {asked} 1Click takes for it.")),
        Some(_) => {}
        None => v.problems.push("1Click didn't say what this deposit costs.".into()),
    }
    if let Some(least) = bound.get("quoteMinAmountIn").and_then(|a| a.parse::<u64>().ok()) && deposit < least {
        v.problems.push(format!("The deposit is under the {least} units 1Click needs for it."));
    }
    if let Some(d) = bound.get("deadline") && let Ok(t) = chrono::DateTime::parse_from_rfc3339(d) && t.timestamp() < now + 120 {
        v.problems.push(format!("1Click's quote for this deposit runs out at {d}"));
    }
    if v.ok() { v.checks.push("1Click holds the deposit for the service's recipient, its exact amount and its refund address, at 1Click's own price.".into()) }
    v
}

/// A limit order's deposit goes only to an order 1Click itself holds as asked: read back from 1Click by its id, never taken
/// from the answer that created it alone. Money then rests with 1Click and pays out later without another signature, so
/// whatever this check lets through is the only check the order gets (`OrderGuard`).
pub mod order {
    use std::collections::HashMap;

    use rust_decimal::Decimal;

    use crate::Verdict;

    /// 1Click's own fee on an order, as it lists it: 0.20%.
    pub const PROTOCOL_BPS: i64 = 20;

    fn dec(s: &str) -> Option<Decimal> { let s = s.trim(); s.parse::<Decimal>().ok().or_else(|| Decimal::from_scientific(s).ok()) }

    /// What the order should be: `bound` as one_click::order reads it. `deposit`: the units the wallet is about to send;
    /// `most`: the units the user approved (a sale's coins, a buy's USDC). `fee`: ihatecrypto's own fee (recipient, bps),
    /// the only one an order may carry besides 1Click's. `deadline` and `now` in Unix seconds.
    pub struct Asked<'a> {
        pub sell: bool,
        pub base: &'a str,
        pub quote: &'a str,
        pub quantity: &'a str,
        pub price: Decimal,
        pub deposit_address: &'a str,
        pub recipient: &'a str,
        pub refund_to: &'a str,
        pub deposit: &'a str,
        pub most: Decimal,
        pub fee: (&'a str, i64),
        pub deadline: i64,
        pub now: i64,
    }

    pub fn check(bound: &HashMap<String, String>, a: &Asked) -> Verdict {
        let mut v = Verdict::default();
        let mut expected = [("orderType", "LIMIT"), ("side", if a.sell { "SELL" } else { "BUY" }), ("baseAsset", a.base), ("quoteAsset", a.quote),
            ("quantity", a.quantity), ("depositAddress", a.deposit_address), ("depositType", "ORIGIN_CHAIN"), ("recipient", a.recipient),
            ("recipientType", "DESTINATION_CHAIN"), ("refundTo", a.refund_to), ("refundType", "ORIGIN_CHAIN")];
        expected.sort_by_key(|e| e.0);
        for (field, value) in expected {
            if bound.get(field).map(|b| b.to_lowercase()) == Some(value.to_lowercase()) { continue }
            v.problems.push(format!("1Click holds the order with a different {field}: {} instead of {value}.", bound.get(field).map_or("none", String::as_str)));
        }
        if bound.get("price").and_then(|p| dec(p)) != Some(a.price) {
            v.problems.push(format!("1Click holds the order at {} price, not {}.", bound.get("price").map_or("no", String::as_str), a.price.normalize()));
        }
        if !["basic", "advanced"].contains(&bound.get("confidentiality").map(|c| c.to_lowercase()).unwrap_or_default().as_str()) {
            v.problems.push(format!("1Click didn't take the order as private (Confidential Intents): it says «{}».", bound.get("confidentiality").map_or("nothing", String::as_str)));
        }
        // A memo the transfer can't carry would leave the deposit unmatched.
        if bound.get("depositMemo").is_some_and(|m| !m.is_empty()) { v.problems.push("1Click asks for a deposit memo this wallet can't send.".into()) }
        // 1Click lists its own platform fee among the order's fees (20 basis points to its account, seen 2026-11-11): that
        // much is allowed besides ihatecrypto's, nothing more.
        let mut others = 0i64;
        for f in bound.get("appFees").map(String::as_str).unwrap_or("").split(',').filter(|f| !f.is_empty()) {
            let parts: Vec<&str> = f.split(':').filter(|p| !p.is_empty()).collect();
            let bps = if parts.len() == 2 { parts[1].parse::<i64>().unwrap_or(-1) } else { -1 };
            if parts.len() != 2 || bps < 0 { v.problems.push(format!("The order carries a fee 1Click doesn't spell out: {f}.")); continue }
            if parts[0] == a.fee.0 {
                if bps > a.fee.1 { v.problems.push(format!("The order gives ihatecrypto {bps} basis points, not {}.", a.fee.1)) }
            } else {
                others = others.saturating_add(bps);
            }
        }
        if others > PROTOCOL_BPS {
            v.problems.push(format!("The order carries {others} basis points of fees besides ihatecrypto's, more than 1Click's own {PROTOCOL_BPS}."));
        }
        // What 1Click's swap view takes is what the wallet sends, and never more than was approved.
        let takes = bound.get(if a.sell { "amountIn" } else { "maxAmountIn" });
        if takes.map(String::as_str) != Some(a.deposit) {
            v.problems.push(format!("1Click's order takes {}, not the {} units about to be sent.", takes.map_or("an unknown amount", String::as_str), a.deposit));
        }
        if a.sell && a.deposit != a.quantity { v.problems.push(format!("A sale deposits its own quantity, {} units, not {}.", a.quantity, a.deposit)) }
        match dec(a.deposit) {
            Some(d) if d > a.most || d <= Decimal::ZERO => v.problems.push(format!("The deposit, {} units, is more than the {} approved.", a.deposit, a.most.normalize())),
            Some(_) => {}
            None => v.problems.push(format!("The deposit isn't a number of units: {}.", a.deposit)),
        }
        match bound.get("deadline").and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()) {
            Some(ends) => {
                let d = bound.get("deadline").map_or("", String::as_str);
                if ends.timestamp() > a.deadline + 3600 { v.problems.push(format!("1Click keeps the order until {d}, later than asked.")) }
                if ends.timestamp() < a.now + 600 { v.problems.push(format!("The order runs out at {d}, before it could rest.")) }
            }
            None => v.problems.push("1Click didn't say until when the order rests.".into()),
        }
        if v.ok() { v.checks.push("1Click holds the order as asked: the pair, side, amount and price, your own addresses, private, with only ihatecrypto's fee.".into()) }
        v
    }

    /// Whether the price is already through the market, so the order would fill at once — a swap at market, not a limit:
    /// a sale at or under what one coin sells for now, a buy at or over what it costs.
    pub fn fills_at_once(sell: bool, price: Decimal, market: Decimal) -> bool {
        market > Decimal::ZERO && if sell { price <= market } else { price >= market }
    }
}

#[cfg(test)]
mod tests;

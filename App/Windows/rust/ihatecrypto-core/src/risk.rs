//! The user's money rules and the checks that hold a plan to them (`Risk.swift`): the risk per trade and of all open
//! positions, reward to risk, the stop against ordinary noise, leverage, the entry and funding; and what the rules fill
//! in (`Plan::resolve`), a hedge, the risk block, and the one change that lets a stopped plan pass.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::foundation::{Num, Rounding, dec, number, rounded};
use crate::hedge;
use crate::intent::{Fmt, IntentParser, Side, TradeIntent};
use crate::plan::{Block, BlockStatus, Evaluation, Live, Market, OUT_OF_RANGE, Plan, RiskCalc, Sizing, pct};

/// The user's money rules. Defaults follow the common teaching for crypto perps: risk 1–2% of capital per trade,
/// reward at least 1.5–2 times the risk, a stop outside ordinary volatility, moderate leverage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskPolicy {
    /// Hard limit: a trade may lose at most this share of capital at its stop.
    #[serde(with = "number")]
    pub max_risk_pct: Decimal,
    /// Risk used when the user asks for "by the book" and names no amount.
    #[serde(with = "number")]
    pub default_risk_pct: Decimal,
    #[serde(with = "number")]
    pub min_reward_risk: Decimal,
    /// Take as a multiple of the stop distance when the user asks for the rules.
    #[serde(with = "number")]
    pub default_reward_risk: Decimal,
    #[serde(with = "number")]
    pub max_leverage: Decimal,
    /// Stop "by volatility": this many 4-hour ATRs.
    #[serde(with = "number")]
    pub atr_stop_multiple: Decimal,
    /// All open positions together may lose at most this share of capital at their stops ("portfolio heat").
    #[serde(with = "number")]
    pub max_open_risk_pct: Decimal,
}

impl Default for RiskPolicy {
    fn default() -> Self {
        Self {
            max_risk_pct: Decimal::TWO, default_risk_pct: Decimal::ONE, min_reward_risk: dec("1.5"), default_reward_risk: Decimal::TWO,
            max_leverage: Decimal::from(5), atr_stop_multiple: dec("1.5"), max_open_risk_pct: Decimal::from(6),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    Ok,
    Warn,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    pub level: Level,
    pub text: String,
}

impl Advice {
    pub fn new(level: Level, text: impl Into<String>) -> Self { Self { level, text: text.into() } }
}

pub struct RiskRules;

impl RiskRules {
    /// Checks a sized plan against the policy and the market. `Block` stops execution; `Warn` is shown. `open_risk`:
    /// what the positions already open lose at their stops; None when a position has no stop (Swift's default is 0).
    /// Numbers past range (NaN or an overflow in Swift) are one `Block`.
    pub fn check(i: &TradeIntent, s: &Sizing, m: &Market, equity: Option<Decimal>, p: &RiskPolicy, open_risk: Option<Decimal>) -> Vec<Advice> {
        Self::advice(i, s, m, equity, p, open_risk).unwrap_or_else(|| vec![Advice::new(Level::Block, OUT_OF_RANGE)])
    }

    fn advice(i: &TradeIntent, s: &Sizing, m: &Market, equity: Option<Decimal>, p: &RiskPolicy, open_risk: Option<Decimal>) -> Option<Vec<Advice>> {
        let mut out = Vec::new();
        let long = i.side == Side::Long;
        let zero = Decimal::ZERO;
        let capital = equity.filter(|e| *e > zero);
        let loss = Num::of(s.loss_at_stop_usd);

        if let Some(equity) = capital {
            if let Some(open) = open_risk {
                let at_stops = (Num::of(open) + loss).0?;
                let total = (Num::of(at_stops) / equity * 100).0?;
                if open > zero && total > p.max_open_risk_pct {
                    out.push(Advice::new(Level::Block, format!(
                        "With open positions, −{} at the stops: {} of capital. Rule: all positions together at most {}.", Fmt::usd(at_stops),
                        Fmt::pct(rounded(total, 1, Rounding::Plain)), Fmt::pct(p.max_open_risk_pct)
                    )));
                } else if open > zero {
                    out.push(Advice::new(Level::Ok, format!("All positions together: {} of capital at the stops, limit {}.",
                                                            Fmt::pct(rounded(total, 1, Rounding::Plain)), Fmt::pct(p.max_open_risk_pct))));
                }
            } else {
                out.push(Advice::new(Level::Warn, "An open position has no stop: total risk is unlimited."));
            }
        }

        // Risk per trade as a share of capital.
        if let Some(equity) = capital {
            let share = (loss / equity * 100).0?;
            if share > p.max_risk_pct {
                let allowed = (Num::of(equity) * p.max_risk_pct / 100).rounded(2, Rounding::Down).0?;
                out.push(Advice::new(Level::Block, format!(
                    "At the stop −{}: {} of {} capital. Rule: at most {} per trade, i.e. up to {}. Lower the risk or top up. The limit is in Settings.",
                    Fmt::usd(s.loss_at_stop_usd), Fmt::pct(rounded(share, 1, Rounding::Plain)), Fmt::usd(equity), Fmt::pct(p.max_risk_pct), Fmt::usd(allowed)
                )));
            } else {
                out.push(Advice::new(Level::Ok, format!("Risk {} of capital, within {}.", Fmt::pct(rounded(share, 1, Rounding::Plain)), Fmt::pct(p.max_risk_pct))));
            }
            if s.margin > equity / Decimal::TWO {
                let taken = (Num::of(s.margin) / equity * 100).rounded(0, Rounding::Plain).0?;
                out.push(Advice::new(Level::Warn, format!("Margin takes {} of capital: little is left for other trades.", Fmt::pct(taken))));
            }
        } else {
            out.push(Advice::new(Level::Warn, "Capital unknown: no balance, so risk in % isn't checked."));
        }

        // Reward to risk. A hedge has no take and no bet on direction: the stop, the trend and the entry are beside the point.
        let rr = Fmt::num(p.default_reward_risk, 2);
        if i.is_hedge() {
            out.push(Advice::new(Level::Ok, "Hedge: no take-profit, the stop only guards the margin. Net of the coins it covers, the trade costs its fees."));
        } else if let Some(profit) = s.profit_at_take_usd.filter(|_| s.loss_at_stop_usd > zero) {
            let r = (Num::of(profit) / loss).rounded(1, Rounding::Plain).0?;
            out.push(if r < p.min_reward_risk {
                Advice::new(Level::Warn, format!("Reward/risk 1:{}: the take-profit is close. The usual minimum is 1:{rr}, “take {rr}R”.", Fmt::num(r, 2)))
            } else {
                Advice::new(Level::Ok, format!("Reward/risk 1:{}.", Fmt::num(r, 2)))
            });
        } else {
            out.push(Advice::new(Level::Warn, format!("No take-profit. By the 1:{rr} rule: “take {rr}R”.")));
        }

        // The stop against ordinary noise.
        if !i.is_hedge() && let (Some(atr), Some(stop)) = (m.atr_pct, i.stop_pct) && stop < atr {
            out.push(Advice::new(Level::Warn, format!(
                "Stop {} is below the average 4-hour move of {} (ATR {}): normal noise can hit it. “Stop by the book” sets {} ATR.", Fmt::pct(stop), m.name,
                Fmt::pct(rounded(atr, 2, Rounding::Plain)), Fmt::num(p.atr_stop_multiple, 2)
            )));
        }

        // Leverage and liquidation distance.
        let lev = i.leverage.unwrap_or(Decimal::ONE);
        if lev > p.max_leverage {
            out.push(Advice::new(Level::Warn, format!(
                "Leverage {}× is above {}×: risk comes from the stop, but liquidation is closer and mistakes cost more.", Fmt::num(lev, 2), Fmt::num(p.max_leverage, 2)
            )));
        }
        let stop_distance = (Num::of(s.entry) - s.stop_price).abs().0?;
        let liq_distance = (Num::of(s.entry) - s.liquidation_price).abs().0?;
        if !i.is_hedge() && stop_distance > zero && liq_distance < (Num::of(stop_distance) * 2).0? {
            out.push(Advice::new(Level::Warn, format!(
                "Liquidation ({}) is less than two stops from entry: a slipping stop could reach it. Lower the leverage.", Fmt::price(s.liquidation_price)
            )));
        }

        // Entry point: the last day's move and the cost of holding.
        if !i.is_hedge() && let Some(prev) = m.prev_day_px.filter(|p| *p > zero) {
            let change = ((Num::of(m.mark_px) - prev) / prev * 100).rounded(1, Rounding::Plain).0?;
            let five = Decimal::from(5);
            let signed = format!("{}{}", if change > zero { "+" } else { "" }, Fmt::pct(change));
            let at_market = i.entry_px.is_none();
            let level = if at_market { Level::Warn } else { Level::Ok };
            if long && change >= five {
                out.push(Advice::new(level, if at_market {
                    format!("Not the best entry: {} +{} in 24h. A sharp rise is often followed by a pullback: “pullback 2%” sets a limit below.", m.name, Fmt::pct(change))
                } else {
                    format!("{} +{} in 24h, so the entry is a limit below the market.", m.name, Fmt::pct(change))
                }));
            } else if !long && change <= -five {
                out.push(Advice::new(level, if at_market {
                    format!("Not the best entry: {} {} in 24h. A sharp drop is often followed by a bounce: “pullback 2%” sets a limit above.", m.name, Fmt::pct(change))
                } else {
                    format!("{} {} in 24h, so the entry is a limit above the market.", m.name, Fmt::pct(change))
                }));
            } else if if long { change <= -five } else { change >= five } {
                out.push(Advice::new(Level::Warn, format!("Trade against the 24h move ({signed}): catching a reversal is riskier than following the trend.")));
            } else {
                out.push(Advice::new(Level::Ok, format!("24h: {signed}, no sharp move before entry.")));
            }
        }
        if let Some(funding) = m.funding {
            // Hyperliquid funding is hourly; longs pay when it is positive.
            let pays = if long { funding > zero } else { funding < zero };
            let yearly = (Num::of(funding.abs()) * 24 * 365 * 100).rounded(0, Rounding::Plain).0?;
            if pays && yearly >= Decimal::from(30) {
                let daily = (Num::of(s.notional) * funding.abs() * 24).rounded(2, Rounding::Up).0?;
                out.push(Advice::new(Level::Warn, format!("Funding: {} pay ≈ {} a year, ≈ {} a day for this position.", if long { "longs" } else { "shorts" },
                                                          Fmt::pct(yearly), Fmt::usd(daily))));
            }
        }
        Some(out)
    }
}

impl Plan {
    /// Fills what the user left to the rules ("by the book", "take 2R"): stop from volatility, take from the stop,
    /// risk from capital. Returns the concrete intent and one line per filled value. `spot`: what the wallet holds of
    /// the plan's coin, which sizes a hedge; None when not read. A step whose numbers are past range (NaN or an
    /// overflow in Swift) is left out, its value unset.
    pub fn resolve(i: &TradeIntent, market: Option<&Market>, equity: Option<Decimal>, p: &RiskPolicy, spot: Option<Decimal>) -> (TradeIntent, Vec<String>) {
        if i.is_hedge() { return Self::hedge(i, market, spot) }
        let mut r = i.clone();
        let mut notes: Vec<String> = Vec::new();
        let auto = i.auto_rules == Some(true);
        let zero = Decimal::ZERO;
        let dir: i64 = if i.side == Side::Long { 1 } else { -1 };
        let capital = equity.filter(|e| *e > zero);
        if let (Some(pullback), Some(m)) = (i.pullback_pct, market) && let Some(px) = (Num::of(m.mark_px) * (1 - dir * Num::of(pullback) / 100)).0 {
            r.entry_px = Some(px);
            notes.push(format!("Limit entry on a {} pullback: {} vs. {} now.", Fmt::pct(pullback), Fmt::price(px), Fmt::price(m.mark_px)));
        }
        if r.stop_pct.is_none() && auto && let Some(atr) = market.and_then(|m| m.atr_pct)
            && let Some(stop) = (Num::of(atr) * p.atr_stop_multiple).rounded(1, Rounding::Up).0
        {
            r.stop_pct = Some(stop);
            notes.push(format!("Stop by volatility: {} × 4h ATR ({}) ≈ {}.", Fmt::num(p.atr_stop_multiple, 2), Fmt::pct(rounded(atr, 2, Rounding::Plain)),
                               Fmt::pct(stop)));
        }
        if r.take_pct.is_none() && let Some(stop) = r.stop_pct && let Some(multiple) = i.take_r.or(auto.then_some(p.default_reward_risk))
            && let Some(take) = (Num::of(stop) * multiple).rounded(2, Rounding::Plain).0
        {
            r.take_pct = Some(take);
            notes.push(format!("Take-profit {}R: {} × stop = {}.", Fmt::num(multiple, 2), Fmt::num(multiple, 2), Fmt::pct(take)));
        }
        // Leverage left out is the rules' cap even with the rest named: at 1× «risk $0.5 stop 1.2%» needed $39 of margin.
        if r.leverage.is_none() && let Some(m) = market {
            // Risk comes from the stop, not the leverage: the rules' ceiling just keeps margin small.
            let lev = p.max_leverage.min(Decimal::from(m.max_leverage));
            r.leverage = Some(lev);
            notes.push(format!("Leverage {}×, the rules' cap. The stop sets the loss, leverage only shrinks the margin.", Fmt::num(lev, 2)));
        }
        // A TWAP is the whole entry (Plan::evaluate): no half at 1R beside it.
        if r.partial_take.is_none() && auto && r.twap_minutes.is_none() {
            r.partial_take = Some(true);
            notes.push("Exit in parts: half at 1R, then stop to breakeven, the rest runs to the take-profit.".into());
        }
        // «5 USDC» and no risk named: the money in the trade is its margin, so the size and the risk follow from it and
        // the stop, and the bridge brings what that margin and the fees need.
        if r.risk_usd.is_none() && let Some(amount) = i.amount_usdc.filter(|a| *a > zero) && let Some(stop) = r.stop_pct {
            let mut lev = r.leverage.unwrap_or(Decimal::ONE);
            // What a dollar of position loses at the stop, fees on both legs included, so the margin is the money. A TWAP's
            // parts may fill up to Plan::twap_drift worse: its risk counts that, as RiskCalc::size does.
            let per_dollar = Num::of(stop) / 100 + RiskCalc::trade_fee() * Decimal::TWO + if r.twap_minutes.is_none() { zero } else { Self::twap_drift() };
            // Leverage the rules chose is lowered until that money's risk fits the limit: «8 USDC» at 5× would break it.
            if i.leverage.is_none() && let Some(equity) = capital
                && let Some(fit) = (Num::of(equity) * p.max_risk_pct / 100 / (Num::of(amount) * per_dollar)).rounded(0, Rounding::Down).0
                && fit >= Decimal::ONE && fit < lev
            {
                lev = fit;
                r.leverage = Some(fit);
                notes.push(format!("Leverage {}× keeps {} USDC in the trade within the {} risk limit.", Fmt::num(fit, 2), Fmt::num(amount, 2),
                                   Fmt::pct(p.max_risk_pct)));
            }
            if let Some(risk) = (Num::of(amount) * lev * per_dollar).rounded(2, Rounding::Down).0 {
                r.risk_usd = Some(risk);
                r.amount_usdc = None;
                notes.push(format!("{} USDC in the trade at {}× with a {} stop risks {}.", Fmt::num(amount, 2), Fmt::num(lev, 2), Fmt::pct(stop), Fmt::usd(risk)));
            }
        }
        if r.risk_usd.is_none() && auto && let Some(equity) = capital
            && let Some(risk) = (Num::of(equity) * p.default_risk_pct / 100).rounded(2, Rounding::Down).0
        {
            r.risk_usd = Some(risk);
            notes.push(format!("Risk {} of {} capital = {}.", Fmt::pct(p.default_risk_pct), Fmt::usd(equity), Fmt::usd(risk)));
        }
        // A small account's share sizes under Hyperliquid's $10 minimum order: raise the risk to the least that reaches
        // it while the rule's limit allows, rather than draw a plan that can't be placed.
        if i.risk_usd.is_none() && let (Some(risk), Some(stop), Some(m), Some(equity)) = (r.risk_usd, r.stop_pct, market, capital) && m.mark_px > zero {
            let entry = Num::of(r.entry_px.unwrap_or(m.mark_px));
            let rdir: i64 = if r.side == Side::Long { 1 } else { -1 };
            let stop_px = entry * (1 - rdir * Num::of(stop) / 100);
            let drift = if r.twap_minutes.is_none() { zero } else { Self::twap_drift() };
            let per_coin = entry * (Num::of(stop) / 100 + drift) + (entry + stop_px) * RiskCalc::trade_fee();
            let places = u32::try_from(m.sz_decimals).unwrap_or(0);
            let least = ((Num::of(Self::MIN_NOTIONAL) / entry).rounded(places, Rounding::Up) * per_coin).rounded(2, Rounding::Up);
            // Money the user named caps it: «4 USDC at 2×» is $8, and raising it to $10 would spend more than they said.
            let within_money = i.amount_usdc.is_none_or(|money| {
                (Num::of(Self::MIN_NOTIONAL) / r.leverage.unwrap_or(Decimal::ONE)).0.is_some_and(|need| need <= money)
            });
            let limit = (Num::of(equity) * p.max_risk_pct / 100).0;
            if let (Some(least), Some(limit)) = (least.0, limit) && least > risk && least <= limit && within_money {
                r.risk_usd = Some(least);
                notes.push(format!("Risk raised to {} so the order reaches Hyperliquid's $10 minimum.", Fmt::usd(least)));
            }
        }
        (r, notes)
    }

    /// A hedge: a short of the coins held (all, a share, or the number named, never more than there is), at 2× unless
    /// named, with a stop that only guards the margin and nothing else a trade has.
    pub fn hedge(i: &TradeIntent, market: Option<&Market>, spot: Option<Decimal>) -> (TradeIntent, Vec<String>) {
        let mut r = TradeIntent { asset: i.asset.clone(), side: Side::Short, leverage: i.leverage, stop_pct: i.stop_pct, ..TradeIntent::default() };
        // Still a hedge while the balance isn't read: `hedge` stays None and the plan waits for it.
        r.hedge_share = Some(i.hedge_share.unwrap_or(Decimal::ONE));
        let mut notes: Vec<String> = Vec::new();
        if let Some(spot) = spot {
            match i.hedge.map_or(Num::of(spot) * i.hedge_share.unwrap_or(Decimal::ONE), Num::of).0 {
                Some(asked) => {
                    r.hedge = Some(asked.min(spot));
                    if asked > spot {
                        notes.push(format!("The wallet holds {} {}: the hedge covers that, not {}.", Fmt::num(spot, 6), IntentParser::pretty(&i.asset),
                                           Fmt::num(asked, 6)));
                    }
                }
                // More than any number holds: what there is.
                None => r.hedge = Some(spot),
            }
        }
        let Some(m) = market else { return (r, notes) };
        let lev = match r.leverage {
            Some(lev) => lev,
            None => {
                let lev = hedge::LEVERAGE.min(Decimal::from(m.max_leverage));
                r.leverage = Some(lev);
                notes.push(format!("Leverage {}× for a hedge: margin is {} of the position, liquidation far above.", Fmt::num(lev, 2),
                                   pct((Num::of(Decimal::ONE_HUNDRED) / lev).rounded(0, Rounding::Plain))));
                lev
            }
        };
        if r.stop_pct.is_none() {
            r.stop_pct = hedge::guard_stop(lev, m);
            if let Some(stop) = r.stop_pct {
                notes.push(format!("Stop {} above the entry, four fifths of the way to liquidation: it only keeps the margin.", Fmt::pct(stop)));
            }
        }
        (r, notes)
    }

    pub fn risk(i: &TradeIntent, e: &Evaluation, live: &Live) -> Block {
        let label = "Risk management";
        let (Some(s), Some(m)) = (&e.sizing, &live.market) else {
            let mut lines = live.notes.clone();
            lines.push("Checks appear once the position is sized.".into());
            return Block::new("risk", label, "Rules", lines, BlockStatus::Info);
        };
        let advice = RiskRules::check(i, s, m, live.equity, &live.policy, live.open_risk);
        let mut lines = live.notes.clone();
        if i.is_hedge() {
            lines.push(format!("Net of the coins it covers: what the short loses, they gain, and back. The trade costs ≈ {} in fees, plus funding while it's open.",
                               Fmt::usd(s.fees_usd)));
        } else if let Some(equity) = live.equity.filter(|e| *e > Decimal::ZERO) {
            let share = |x: Decimal| pct((Num::of(x) / equity * 100).rounded(1, Rounding::Plain));
            let mut result = format!("If the stop hits: −{} (−{} of capital).", Fmt::usd(s.loss_at_stop_usd), share(s.loss_at_stop_usd));
            if let Some(profit) = s.profit_at_take_usd {
                result += &format!(" If the take-profit hits: +{} (+{}).", Fmt::usd(profit), share(profit));
            }
            lines.push(result);
        }
        lines.extend(advice.iter().map(|a| format!("{} {}", match a.level { Level::Ok => "✓", Level::Warn => "⚠", Level::Block => "✗" }, a.text)));
        let status = if advice.iter().any(|a| a.level == Level::Block) {
            BlockStatus::Blocked
        } else if advice.iter().any(|a| a.level == Level::Warn) {
            BlockStatus::Warning
        } else {
            BlockStatus::Ready
        };
        let title = match status { BlockStatus::Blocked => "Rule broken", BlockStatus::Warning => "Warnings", _ => "Rules met" };
        Block::new("risk", label, title, lines, status)
    }

    /// A stopped plan's one change that passes every rule, the $10 minimum and the money, as the chat's form («risk
    /// $0.32», «5 USDC») with the trade it makes. Too small: the least risk — or money, when the user named the money —
    /// that reaches the minimum order. Too big: the largest that fits. None when no such change is found.
    pub fn fix(raw: &TradeIntent, m: &Market, equity: Option<Decimal>, policy: &RiskPolicy, open_risk: Option<Decimal>,
               available: Option<Decimal>) -> Option<(String, Sizing)> {
        let passes = |j: &TradeIntent| -> Option<Sizing> {
            let r = Self::resolve(j, Some(m), equity, policy, None).0;
            let e = Self::evaluate(&r, Some(m));
            let s = e.sizing.filter(|s| e.problem.is_none() && s.notional >= Self::MIN_NOTIONAL)?;
            if let Some(available) = available && (Num::of(s.margin) + s.fees_usd).0.is_none_or(|need| need > available) { return None }
            if RiskRules::check(&r, &s, m, equity, policy, open_risk).iter().any(|a| a.level == Level::Block) { None } else { Some(s) }
        };
        let resolved = Self::resolve(raw, Some(m), equity, policy, None).0;
        let s0 = Self::evaluate(&resolved, Some(m)).sizing?;
        if passes(raw).is_some() { return None }
        let too_small = s0.notional < Self::MIN_NOTIONAL;
        let (up, down) = (dec("1.03"), dec("0.95"));
        let (step, mode) = if too_small { (up, Rounding::Up) } else { (down, Rounding::Down) };
        // ponytail: walks 3% steps up or 5% down (at most 60); a closed form would need fees and size steps inverted.
        if raw.risk_usd.is_none() && let Some(money) = raw.amount_usdc {
            let mut a = if too_small {
                (Num::of(Self::MIN_NOTIONAL) / resolved.leverage.unwrap_or(Decimal::ONE)).rounded(1, Rounding::Up).0?
            } else {
                money
            };
            for _ in 0..60 {
                if a <= Decimal::ZERO { break }
                let j = TradeIntent { amount_usdc: Some(a), ..raw.clone() };
                if let Some(s) = passes(&j) { return if a == money { None } else { Some((format!("{} USDC", Fmt::num(a, 2)), s)) } }
                a = (Num::of(a) * step).rounded(1, mode).0?;
            }
            return None;
        }
        let risk = resolved.risk_usd?;
        let mut r = if too_small {
            (Num::of(risk) * Self::MIN_NOTIONAL / s0.notional.max(dec("0.01"))).rounded(2, Rounding::Up).0?
        } else {
            risk
        };
        for _ in 0..60 {
            if r <= Decimal::ZERO { break }
            let j = TradeIntent { risk_usd: Some(r), ..raw.clone() };
            if let Some(s) = passes(&j) { return if r == risk { None } else { Some((format!("risk ${}", Fmt::num(r, 2)), s)) } }
            r = (Num::of(r) * step).rounded(2, mode).0?;
        }
        None
    }

    /// For a plan the rules stop: the largest risk, in whole cents and below what was asked, that passes every rule,
    /// the $10 minimum and fits the money available. None when nothing smaller works either.
    pub fn fitting_risk(i: &TradeIntent, m: &Market, equity: Option<Decimal>, policy: &RiskPolicy, open_risk: Option<Decimal>,
                        available: Option<Decimal>) -> Option<Decimal> {
        let asked = i.risk_usd?;
        let mut r = asked;
        if let Some(equity) = equity.filter(|e| *e > Decimal::ZERO) {
            r = r.min((Num::of(equity) * policy.max_risk_pct / 100).0?);
            if let Some(open) = open_risk.filter(|o| *o > Decimal::ZERO) { r = r.min((Num::of(equity) * policy.max_open_risk_pct / 100 - open).0?) }
        }
        r = rounded(r, 2, Rounding::Down);
        // ponytail: steps down 5% at a time; a closed form would need the fee and size-step rounding inverted.
        for _ in 0..80 {
            if r <= Decimal::ZERO { break }
            let j = TradeIntent { risk_usd: Some(r), ..i.clone() };
            let e = Self::evaluate(&j, Some(m));
            let s = e.sizing.as_ref().filter(|s| s.notional >= Self::MIN_NOTIONAL)?;
            let fits = available.is_none_or(|a| (Num::of(s.margin) + s.fees_usd).0.is_some_and(|need| need <= a));
            let rules = RiskRules::check(&j, s, m, equity, policy, open_risk);
            if e.problem.is_none() && fits && !rules.iter().any(|a| a.level == Level::Block) { return (r < asked).then_some(r) }
            r = (Num::of(r) * dec("0.95")).rounded(2, Rounding::Down).0?;
        }
        None
    }
}

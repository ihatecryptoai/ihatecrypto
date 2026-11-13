//! A trade plan (`Plan.swift`): a Hyperliquid market's facts, the position's size from its risk and stop, the checks
//! that stop a plan, and the five blocks the canvas draws (source, bridge, position, protection, risk). Code decides
//! the numbers; the model never does. Every sum on an outside number (the chat's, the exchange's, a node's) is
//! checked: where rust_decimal would panic and Swift's Decimal would not, the plan is refused (`OUT_OF_RANGE`).

use rust_decimal::Decimal;

use crate::foundation::{Num, Rounding, dec, rounded};
use crate::intent::{Fmt, IntentParser, Side, TradeIntent};
use crate::risk::RiskPolicy;

/// A plan whose numbers are past what the arithmetic holds (NaN or an overflow in Swift): refused instead.
// ponytail: no Swift words for it, since Swift's Decimal goes on with NaN or 38 digits.
pub const OUT_OF_RANGE: &str = "The plan's numbers are out of range: check the price, the risk and the stop.";
/// `OUT_OF_RANGE` in Russian, for a risk check shown in a Russian chat.
pub const OUT_OF_RANGE_RU: &str = "Числа плана вне допустимого диапазона: проверь цену, риск и стоп.";

#[derive(Debug, Clone, PartialEq)]
pub struct Market {
    pub name: String,
    pub mark_px: Decimal,
    pub sz_decimals: i64,
    pub max_leverage: i64,
    /// Position in Hyperliquid's perp universe, the asset id in orders; −1 when not loaded from the exchange.
    pub index: i64,
    /// Price 24 hours ago, for the entry-point check.
    pub prev_day_px: Option<Decimal>,
    /// Hourly funding rate; positive means longs pay shorts.
    pub funding: Option<Decimal>,
    /// Average true range of 4-hour candles, % of price; loaded separately from candles.
    pub atr_pct: Option<Decimal>,
    /// The HIP-3 exchange ("xyz") for markets outside the main one; its margin is a separate balance.
    pub dex: Option<String>,
    /// Traded in the last 24 hours, USD; orders the market picker.
    pub volume24h: Decimal,
}

/// `Market.Group`: what a market tracks, for the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketGroup {
    Crypto,
    Commodities,
    Stocks,
    Indices,
}

impl MarketGroup {
    pub const ALL: [Self; 4] = [Self::Crypto, Self::Commodities, Self::Stocks, Self::Indices];

    pub fn raw(self) -> &'static str {
        match self { Self::Crypto => "Crypto", Self::Commodities => "Commodities", Self::Stocks => "Stocks", Self::Indices => "Indices & FX" }
    }
}

impl Market {
    const COMMODITIES: [&str; 14] =
        ["CL", "BRENTOIL", "GOLD", "SILVER", "NATGAS", "COPPER", "PLATINUM", "PALLADIUM", "URANIUM", "WHEAT", "CORN", "COCOA", "COFFEE", "SUGAR"];
    const INDICES: [&str; 15] = ["SP500", "XYZ100", "NDX", "DJI", "VIX", "DXY", "JPY", "EUR", "GBP", "CNH", "KRW", "EWY", "XLE", "SOXL", "KORU"];

    /// The rest as Swift's init defaults them: index −1, no 24 h price, funding, ATR or HIP-3 exchange.
    pub fn new(name: &str, mark_px: Decimal, sz_decimals: i64, max_leverage: i64) -> Self {
        Self {
            name: name.into(), mark_px, sz_decimals, max_leverage, index: -1, prev_day_px: None, funding: None, atr_pct: None, dex: None,
            volume24h: Decimal::ZERO,
        }
    }

    /// How far the price moved in 24 hours, in %; None without the price from then.
    pub fn change24h(&self) -> Option<Decimal> {
        let prev = self.prev_day_px.filter(|p| *p > Decimal::ZERO)?;
        ((Num::of(self.mark_px) - prev) / prev * 100).0
    }

    /// Main-exchange markets are crypto; HIP-3 ones are sorted by what they track.
    pub fn group(&self) -> MarketGroup {
        let Some(base) = self.dex.as_ref().and(self.name.split(':').rfind(|s| !s.is_empty())) else { return MarketGroup::Crypto };
        if Self::COMMODITIES.contains(&base) { MarketGroup::Commodities } else if Self::INDICES.contains(&base) { MarketGroup::Indices } else { MarketGroup::Stocks }
    }

    /// The size step's places, as rounding takes them.
    fn places(&self) -> u32 { u32::try_from(self.sz_decimals).unwrap_or(0) }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sizing {
    pub size: Decimal,
    pub entry: Decimal,
    pub notional: Decimal,
    pub margin: Decimal,
    pub stop_price: Decimal,
    pub take_price: Option<Decimal>,
    pub liquidation_price: Decimal,
    /// Fees for entry and exit at the stop: Hyperliquid's taker fee and ihatecrypto's.
    pub fees_usd: Decimal,
    /// ihatecrypto's part of `fees_usd`.
    pub builder_fees_usd: Decimal,
    pub loss_at_stop_usd: Decimal,
    pub profit_at_take_usd: Option<Decimal>,
}

/// Position math. Code decides the numbers; the model never does.
pub struct RiskCalc;

impl RiskCalc {
    /// Hyperliquid tier-0 taker fee (RESEARCH.md §8).
    pub fn taker_fee() -> Decimal { Decimal::new(45, 5) }

    /// ihatecrypto's fee on each side (HLBuilder), on top of Hyperliquid's.
    pub fn builder_fee() -> Decimal { hyperliquid::builder::share() }

    /// What each side of a trade pays in all.
    pub fn trade_fee() -> Decimal { Self::taker_fee() + Self::builder_fee() }

    /// Hyperliquid's tier-0 spot taker fee; ihatecrypto's fee is taken on spot sells only.
    pub fn spot_taker_fee() -> Decimal { Decimal::new(7, 4) }

    /// `entry`: a limit price; the mark when None. `hedge`: the coins a hedge covers, which set the size instead of the
    /// risk. `drift`: how far against the trade the entry may still go (a TWAP's parts, `Plan::twap_drift`), as a share;
    /// the stop stays where `entry` puts it, and the loss at it counts the worst entry. None where the numbers are past
    /// range (NaN or an overflow in Swift).
    #[allow(clippy::too_many_arguments)]
    pub fn size(
        risk_usd: Decimal, stop_pct: Decimal, take_pct: Option<Decimal>, leverage: Decimal, side: Side, market: &Market, entry: Option<Decimal>,
        hedge: Option<Decimal>, drift: Decimal,
    ) -> Option<Sizing> {
        let entry = entry.unwrap_or(market.mark_px);
        let dir: i64 = if side == Side::Long { 1 } else { -1 };
        let e = Num::of(entry);
        let s = Num::of(stop_pct) / 100;
        let stop = e * (1 - dir * s);
        let worst = e * (1 + dir * Num::of(drift));
        // Loss per coin if the stop fills at its price: the move plus the fees on both legs.
        let per_coin_loss = e * (s + drift) + (worst + stop) * Self::trade_fee();
        let size = hedge.map_or(Num::of(risk_usd) / per_coin_loss, Num::of).rounded(market.places(), Rounding::Down);
        let notional = size * entry;
        // A TWAP's parts (drift) pay Hyperliquid only: it takes no builder fee on them. The exit pays both.
        // ponytail: the size above still counts both fees on the entry, a few cents of headroom on a TWAP.
        let entry_fee = if drift > Decimal::ZERO { Self::taker_fee() } else { Self::trade_fee() };
        let fees = size * (worst * entry_fee + stop * Self::trade_fee());
        let take = take_pct.map(|t| e * (1 + dir * Num::of(t) / 100));
        let profit = take.map(|tp| size * (tp - entry).abs() - size * (e + tp) * Self::trade_fee());
        // Isolated margin, lowest tier: maintenance = 1 / (2 · max leverage).
        // ponytail: ignores Hyperliquid margin tiers, fine below the first tier's notional; read marginTables for big positions.
        let l = Num::of(Decimal::ONE) / (2 * Num::of(Decimal::from(market.max_leverage)));
        let liq = e * (1 - dir * (Num::of(Decimal::ONE) / leverage - l) / (1 - dir * l));
        let builder = size * (if drift > Decimal::ZERO { Num::of(Decimal::ZERO) } else { e } + stop) * Self::builder_fee();
        // A hedge loses at its stop what the coins it covers gain: the fees are what's left.
        let loss = if hedge.is_none() { size * entry * (s + drift) } else { Num::of(Decimal::ZERO) } + fees;
        Some(Sizing {
            size: size.0?,
            entry,
            notional: notional.0?,
            margin: (notional / leverage).0?,
            stop_price: stop.0?,
            take_price: match take { Some(t) => Some(t.0?), None => None },
            liquidation_price: liq.0?,
            fees_usd: fees.0?,
            builder_fees_usd: builder.0?,
            loss_at_stop_usd: loss.0?,
            profit_at_take_usd: match profit { Some(p) => Some(p.0?), None => None },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Evaluation {
    pub missing: Vec<String>,
    pub problem: Option<String>,
    pub sizing: Option<Sizing>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeQuote {
    /// "Relay" or "NEAR 1Click".
    pub provider: String,
    /// What to send, including `reserve` for slippage.
    pub amount_in: Decimal,
    pub reserve: Decimal,
    pub amount_out: Decimal,
    /// Time as measured in live runs, not the provider's estimate.
    pub time: String,
    /// The other provider's price, for comparison.
    pub alternative: Option<String>,
    /// The other provider's fee, for the choice of how to open a new Hyperliquid account.
    pub alternative_fee: Option<Decimal>,
    /// The first deposit into a Hyperliquid account that doesn't exist yet: the price includes its one-time opening fee.
    pub opening: bool,
}

impl BridgeQuote {
    pub fn new(provider: &str, amount_in: Decimal, reserve: Decimal, amount_out: Decimal, time: &str, alternative: Option<String>) -> Self {
        Self { provider: provider.into(), amount_in, reserve, amount_out, time: time.into(), alternative, alternative_fee: None, opening: false }
    }
}

/// What the network told us. None means «no data», never zero.
#[derive(Debug, Clone, PartialEq)]
pub struct Live {
    pub solana_address_set: bool,
    pub solana_usdc: Option<Decimal>,
    pub hl_address_set: bool,
    pub hl_withdrawable: Option<Decimal>,
    pub markets_loaded: bool,
    pub market: Option<Market>,
    pub quote: Option<BridgeQuote>,
    /// The EVM network the trade's USDC leaves from when Solana is short, and what it holds there; None = Solana.
    pub from: Option<(String, Decimal)>,
    /// USDC on Solana + Hyperliquid account value + vaults: what the risk rules measure against.
    pub equity: Option<Decimal>,
    pub policy: RiskPolicy,
    /// Loss at the stops of positions already open; None when one has no stop.
    pub open_risk: Option<Decimal>,
    /// What the wallet holds of the plan's coin, for a hedge; None when not read.
    pub spot: Option<Decimal>,
    /// What `Plan::resolve` filled in, shown in the risk block.
    pub notes: Vec<String>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            solana_address_set: false, solana_usdc: None, hl_address_set: false, hl_withdrawable: None, markets_loaded: false, market: None,
            quote: None, from: None, equity: None, policy: RiskPolicy::default(), open_risk: Some(Decimal::ZERO), spot: None, notes: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    Ready,
    Warning,
    NeedsInput,
    Blocked,
    NoData,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: String,
    pub label: String,
    pub title: String,
    pub lines: Vec<String>,
    pub status: BlockStatus,
}

impl Block {
    pub fn new(id: &str, label: &str, title: &str, lines: Vec<String>, status: BlockStatus) -> Self {
        Self { id: id.into(), label: label.into(), title: title.into(), lines, status }
    }
}

/// A checked number as `Fmt` prints it; «NaN» where Swift's Decimal would have gone NaN or past range.
pub(crate) fn num(n: Num) -> String { n.0.map_or("NaN".into(), |d| Fmt::num(d, 2)) }
pub(crate) fn usd(n: Num) -> String { n.0.map_or("$NaN".into(), Fmt::usd) }
pub(crate) fn price(n: Num) -> String { n.0.map_or("NaN".into(), Fmt::price) }
pub(crate) fn pct(n: Num) -> String { n.0.map_or("NaN%".into(), Fmt::pct) }

pub struct Plan;

impl Plan {
    /// Hyperliquid rejects orders below $10 notional.
    pub const MIN_NOTIONAL: Decimal = Decimal::TEN;
    /// The smallest TWAP Hyperliquid runs, in dollars of the whole order (order-types doc).
    pub const TWAP_MINIMUM: Decimal = Decimal::ONE_HUNDRED;
    /// The longest TWAP ihatecrypto starts (Hyperliquid runs 7 days): past a few hours a stop that fired leaves it
    /// buying unwatched more likely than not (a closed lid, a quit app).
    pub const TWAP_MAX_MINUTES: i64 = 240;

    /// How far a TWAP's parts may fill against the trade: Hyperliquid's 3% slippage cap on each part. Its risk is sized
    /// from that worst entry, the stop staying where the price now puts it.
    pub fn twap_drift() -> Decimal { dec("0.03") }

    pub fn evaluate(i: &TradeIntent, market: Option<&Market>) -> Evaluation {
        let mut e = Evaluation::default();
        if i.risk_usd.is_none() && !i.is_hedge() { e.missing.push("risk in $".into()) }
        if i.is_hedge() && i.hedge.is_none() { e.missing.push(format!("what the wallet holds of {}", i.asset)) }
        if i.stop_pct.is_none() { e.missing.push("stop in %".into()) }
        let (true, Some(m), Some(stop)) = (e.missing.is_empty(), market, i.stop_pct) else { return e };
        let risk = i.risk_usd.unwrap_or_default();
        let lev = i.leverage.unwrap_or(Decimal::ONE);
        let zero = Decimal::ZERO;
        let mut refuse = |p: String| { e.problem = Some(p); };

        if i.hedge.is_some_and(|h| h <= zero) {
            refuse(format!("Nothing to hedge: the wallet holds no {}.", IntentParser::pretty(&i.asset)));
            return e;
        }
        if i.hedge.is_none() && risk <= zero { refuse("Risk must be above zero.".into()); return e }
        if stop <= zero || stop >= Decimal::ONE_HUNDRED { refuse("Stop must be above 0% and below 100%.".into()); return e }
        if i.take_pct.is_some_and(|t| t <= zero || (i.side == Side::Short && t >= Decimal::ONE_HUNDRED)) {
            refuse("Take-profit is out of range.".into());
            return e;
        }
        // The signer refuses more than the top of the leverage steps whatever the market allows (HLGuard::MAX_LEVERAGE):
        // said here, before money moves.
        let max_lev = m.max_leverage.min(50);
        if lev < Decimal::ONE || lev > Decimal::from(max_lev) { refuse(format!("Leverage for {}: 1× to {max_lev}×.", m.name)); return e }
        if lev != rounded(lev, 0, Rounding::Down) {
            refuse(format!("Leverage on Hyperliquid is a whole number: {}× or {}×.", Fmt::num(rounded(lev, 0, Rounding::Down), 2),
                           Fmt::num(rounded(lev, 0, Rounding::Up), 2)));
            return e;
        }
        if let Some(minutes) = i.twap_minutes {
            if i.is_hedge() || i.entry_px.is_some() || i.partial_take == Some(true) || i.trailing_pct.is_some() {
                refuse("A TWAP is the whole entry, at the market: no limit, hedge, take in parts or trailing with it.".into());
                return e;
            }
            if minutes < 5 { refuse("A TWAP on Hyperliquid runs 5 minutes at least.".into()); return e }
            if minutes > Self::TWAP_MAX_MINUTES {
                refuse("A TWAP here runs 4 hours at most: a longer one could keep buying with no stop after its stop fires and no one is watching. \
                        For days or weeks, use recurring buys («buy ETH for 50 USDC every day»).".into());
                return e;
            }
        }
        let long = i.side == Side::Long;
        if let Some(px) = i.entry_px.filter(|&px| if long { px >= m.mark_px } else { px <= m.mark_px }) {
            refuse(format!("Limit {} is {} the market {} and would fill at once. For a market entry, type “at market”.", Fmt::price(px),
                           if long { "above" } else { "below" }, Fmt::price(m.mark_px)));
            return e;
        }
        let drift = if i.twap_minutes.is_none() { zero } else { Self::twap_drift() };
        let Some(s) = RiskCalc::size(risk, stop, i.take_pct, lev, i.side, m, i.entry_px, i.hedge, drift) else {
            refuse(OUT_OF_RANGE.into());
            return e;
        };
        let liq_before_stop = if long { s.liquidation_price >= s.stop_price } else { s.liquidation_price <= s.stop_price };
        let problem = if s.size <= zero {
            Some(if i.is_hedge() { format!("Too little {} to hedge: below the market's size step.", m.name) }
                 else { format!("Risk is too small for the minimum {} size step.", m.name) })
        } else if s.notional < Self::MIN_NOTIONAL {
            Some(if i.is_hedge() {
                format!("A hedge of {} is below Hyperliquid's $10 minimum order: hedge at least $10 of {}.", Fmt::usd(s.notional), m.name)
            } else {
                format!("Notional {} is below the Hyperliquid $10 minimum order. Raise the risk or tighten the stop.", Fmt::usd(s.notional))
            })
        } else if i.twap_minutes.is_some() && s.notional < Self::TWAP_MINIMUM {
            Some(format!("A TWAP on Hyperliquid is from $100: this one is {}. Put more into the trade, or say «one order».", Fmt::usd(s.notional)))
        } else if liq_before_stop {
            Some(format!("Liquidation ({}) comes before the stop. Lower the leverage.", Fmt::price(s.liquidation_price)))
        } else {
            None
        };
        e.sizing = Some(s);
        e.problem = problem;
        e
    }

    /// The money a plan holds against what its command named, by code rather than by a reader: «N USDC» is the most
    /// that goes in as margin, «risk $N» the most lost at the stop. None when the plan keeps to it, else what differs.
    /// `named` is the intent as the commands left it, before the rules (resolve) turned the money into a risk. Only
    /// «more than named» is a problem: the size rounds down to the market's step, so a little less is the rule. What
    /// the rules themselves may add is allowed for: an order raised to Hyperliquid's $10 minimum rounds its size up by
    /// a step and its risk up by a cent, and a cent of risk at a 0.5% stop is $2 of position. Numbers past range count
    /// as more than named.
    pub fn money_problem(named: &TradeIntent, s: &Sizing, m: &Market) -> Option<String> {
        if s.notional <= Decimal::ZERO || s.loss_at_stop_usd <= Decimal::ZERO { return None }
        let cent = dec("0.01");
        // In dollars of position: one step of the size, and what a cent of risk buys.
        let step = (0..m.sz_decimals.max(0)).fold(Num::of(Decimal::ONE), |a, _| a * 10);
        let slack = Num::of(s.entry) / step + Num::of(cent) * s.notional / s.loss_at_stop_usd;
        let over = |got: Decimal, named: Decimal| (Num::of(named) * dec("1.01") + slack * got / s.notional + cent).0.is_none_or(|limit| got > limit);
        if named.risk_usd.is_none() && let Some(money) = named.amount_usdc.filter(|&m| m > Decimal::ZERO) && over(s.margin, money) {
            return Some(format!("The plan puts {} into the trade, more than the {} USDC its command named.", Fmt::usd(s.margin), Fmt::num(money, 2)));
        }
        if let Some(risk) = named.risk_usd.filter(|&r| r > Decimal::ZERO) && over(s.loss_at_stop_usd, risk) {
            return Some(format!("The plan loses {} at the stop, more than the ${} risk its command named.", Fmt::usd(s.loss_at_stop_usd), Fmt::num(risk, 2)));
        }
        None
    }

    /// USDC that must arrive on HyperCore, or None when the trading balance already covers margin and fees or the
    /// position has not passed the checks yet. `mark`: Hyperliquid holds an order's margin at the current price when
    /// that's above the limit — a long on a pullback (an order funded to the limit's margin was refused with
    /// «Insufficient margin», test wallet, 2026-10-03).
    // ponytail: a need past range (an absurd mark or balance) reads as none, and the order is then refused by the exchange.
    pub fn bridge_need(i: &TradeIntent, e: &Evaluation, hl_withdrawable: Option<Decimal>, mark: Option<Decimal>) -> Option<Decimal> {
        if i.amount_usdc.is_some() || e.problem.is_some() { return None }
        let s = e.sizing.as_ref()?;
        let held = if s.entry > Decimal::ZERO {
            let ratio = (Num::of(mark.unwrap_or(s.entry)) / s.entry).0?;
            Num::of(s.margin) * ratio.max(Decimal::ONE)
        } else {
            Num::of(s.margin)
        };
        let need = (held + s.fees_usd - hl_withdrawable.unwrap_or_default()).0?;
        (need > Decimal::ZERO).then(|| rounded(need, 2, Rounding::Up))
    }

    pub fn blocks(i: &TradeIntent, e: &Evaluation, live: &Live) -> Vec<Block> {
        let need = Self::bridge_need(i, e, live.hl_withdrawable, live.market.as_ref().map(|m| m.mark_px));
        let bridging = i.amount_usdc.is_some() || need.is_some();
        let send = live.quote.as_ref().map(|q| q.amount_in).or(i.amount_usdc).or(need);
        vec![
            Self::source(live, if bridging { send } else { None }),
            Self::bridge(i, e, live, need),
            Self::position(i, e, live),
            Self::protection(i, e),
            Self::risk(i, e, live),
        ]
    }

    pub fn source(live: &Live, send: Option<Decimal>) -> Block {
        let title = format!("USDC · {}", live.from.as_ref().map_or("Solana", |f| &f.0));
        if live.from.is_none() && !live.solana_address_set {
            return Block::new("source", "Source", &title, vec!["Add a Solana address in Settings.".into()], BlockStatus::NeedsInput);
        }
        let Some(have) = live.from.as_ref().map(|f| f.1).or(live.solana_usdc) else {
            return Block::new("source", "Source", &title, vec!["Balance: no data.".into()], BlockStatus::NoData);
        };
        let mut lines = vec![format!("Wallet: {} USDC.", Fmt::num(have, 2))];
        if let Some(send) = send {
            lines.push(format!("Send {} USDC.", Fmt::num(send, 2)));
            if send > have {
                lines.push(format!("Short by {} USDC.", num(Num::of(send) - have)));
                return Block::new("source", "Source", &title, lines, BlockStatus::Blocked);
            }
        }
        Block::new("source", "Source", &title, lines, BlockStatus::Ready)
    }

    pub fn bridge(i: &TradeIntent, e: &Evaluation, live: &Live, need: Option<Decimal>) -> Block {
        let label = format!("Bridge · {}", live.quote.as_ref().map_or("Relay or 1Click", |q| &q.provider));
        let title = format!("{} → HyperCore USDC", live.from.as_ref().map_or("Solana", |f| &f.0));
        let block = |lines: Vec<String>, status| Block::new("bridge", &label, &title, lines, status);
        if i.amount_usdc.is_none() && e.problem.is_some() {
            return block(vec!["The amount appears once the position passes the checks.".into()], BlockStatus::NeedsInput);
        }
        if i.amount_usdc.is_none() && need.is_none() {
            let sized = e.sizing.is_some();
            let line = if sized { "Not needed: the Hyperliquid balance covers margin and fees." } else { "The amount appears once the position is sized." };
            return block(vec![line.into()], if sized { BlockStatus::Info } else { BlockStatus::NeedsInput });
        }
        let Some(q) = &live.quote else { return block(vec!["Quote: no data.".into()], BlockStatus::NoData) };
        let cost = Num::of(q.amount_in) - q.reserve - q.amount_out;
        let mut lines = vec![
            format!("Send {} → receive {} USDC.", Fmt::num(q.amount_in, 2), Fmt::num(q.amount_out, 2)),
            if q.reserve > Decimal::ZERO {
                format!("Fees and spread ≈ {}, slippage reserve {} USDC.", usd(cost), Fmt::num(q.reserve, 2))
            } else {
                format!("Fees and spread ≈ {}.", usd(Num::of(q.amount_in) - q.amount_out))
            },
            q.time.clone(),
        ];
        if q.opening {
            lines.push(if q.provider == "Relay" { "Your Hyperliquid account is new: this first deposit includes its one-time opening fee." }
                       else { "Your Hyperliquid account is new: NEAR Intents opens it without the opening fee." }.into());
        }
        // Worth saying before the signature: a bridge that costs more than the take-profit earns makes the trade a loss.
        let profit = e.sizing.as_ref().and_then(|s| s.profit_at_take_usd);
        let costly = profit.is_some_and(|p| cost.0.is_none_or(|c| c > p));
        if let Some(profit) = profit.filter(|_| costly) {
            lines.push(format!("The bridge costs more than the take-profit earns ({}): even at the take this trade loses money{}", Fmt::usd(profit),
                               if q.opening { ", unless the account is used again." } else { "." }));
        }
        if let Some(alternative) = &q.alternative { lines.push(alternative.clone()) }
        lines.push("Dry quote: no addresses are sent.".into());
        // A set amount («risk $1, 5 USDC») must still carry the margin once the bridge has taken its fee.
        if i.amount_usdc.is_some() && let Some(s) = &e.sizing {
            let (arrives, needs) = (Num::of(q.amount_out) + live.hl_withdrawable.unwrap_or_default(), Num::of(s.margin) + s.fees_usd);
            let short = match (arrives.0, needs.0) { (Some(a), Some(n)) => a < n, (None, _) => false, (_, None) => true };
            if short {
                lines.insert(0, format!("Only {} USDC arrive, and the position needs {} for margin and fees: send more or lower the risk.",
                                        Fmt::num(q.amount_out, 2), usd(needs)));
                return block(lines, BlockStatus::Blocked);
            }
        }
        block(lines, if costly { BlockStatus::Warning } else { BlockStatus::Ready })
    }

    pub fn position(i: &TradeIntent, e: &Evaluation, live: &Live) -> Block {
        let label = "Position · Hyperliquid";
        let title = if i.is_hedge() {
            format!("Hedge {} · short", IntentParser::pretty(&i.asset))
        } else {
            format!("{} {}", if i.side == Side::Long { "Long" } else { "Short" }, IntentParser::pretty(&i.asset))
        };
        let Some(m) = &live.market else {
            let line = if live.markets_loaded { format!("No {} market on Hyperliquid.", i.asset) } else { "Price: no data.".into() };
            return Block::new("position", label, &title, vec![line], if live.markets_loaded { BlockStatus::Blocked } else { BlockStatus::NoData });
        };
        if !e.missing.is_empty() {
            return Block::new("position", label, &title, vec![format!("Mark {}.", Fmt::price(m.mark_px)), format!("Set {}.", e.missing.join(" and "))],
                              BlockStatus::NeedsInput);
        }
        let Some(s) = &e.sizing else {
            return Block::new("position", label, &title, vec![e.problem.clone().unwrap_or_default()], BlockStatus::Blocked);
        };
        let hundred = Decimal::ONE_HUNDRED;
        let fees = format!("Hyperliquid {}%, ihatecrypto {}%", Fmt::num(RiskCalc::taker_fee() * hundred, 3), Fmt::num(RiskCalc::builder_fee() * hundred, 3));
        let size = Fmt::num(s.size, m.places());
        let mut lines = vec![
            match i.twap_minutes {
                Some(minutes) => format!(
                    "TWAP: {size} {} over {}, parts at the market every 30 s or more, each within 3% of the price{}", m.name, TradeIntent::duration(minutes),
                    if i.twap_randomize == Some(true) { ", sizes randomized ±20%." } else { ", equal sizes." }
                ),
                None if i.entry_px.is_none() => format!("{size} {} at ≈ {} (mark).", m.name, Fmt::price(s.entry)),
                None => format!("Limit: {size} {} at {}, now {}. Waiting. Stop and take-profit start after entry.", m.name, Fmt::price(s.entry),
                                Fmt::price(m.mark_px)),
            },
            format!("Notional {}, margin {} at {}× leverage{}.", Fmt::usd(s.notional), Fmt::usd(s.margin), Fmt::num(i.leverage.unwrap_or(Decimal::ONE), 2),
                    if i.leverage.is_none() { " (default)" } else { "" }),
            format!("Liquidation ≈ {}, isolated margin.", Fmt::price(s.liquidation_price)),
            if i.twap_minutes.is_none() {
                format!("Fees per side: {fees}.")
            } else {
                format!("Fees: the TWAP's parts pay Hyperliquid {}% only, ihatecrypto's 0.05% isn't on a TWAP. The exit pays {fees}.",
                        Fmt::num(RiskCalc::taker_fee() * hundred, 3))
            },
        ];
        if i.is_hedge() {
            lines.insert(1, format!("Covers {size}{} {} the wallet holds: what they lose in a fall, the short makes back.",
                                    live.spot.map_or(String::new(), |h| format!(" of the {}", Fmt::num(h, 6))), m.name));
        }
        if let Some(p) = &e.problem { lines.push(p.clone()) }
        Block::new("position", label, &title, lines, if e.problem.is_none() { BlockStatus::Ready } else { BlockStatus::Blocked })
    }

    pub fn protection(i: &TradeIntent, e: &Evaluation) -> Block {
        let label = "Protection";
        let (Some(s), Some(stop_pct)) = (&e.sizing, i.stop_pct) else {
            return Block::new("protection", label, "Stop & take-profit", vec!["Set a stop in %, e.g. “stop 3%”.".into()], BlockStatus::NeedsInput);
        };
        let status = if e.problem.is_none() { BlockStatus::Ready } else { BlockStatus::Blocked };
        if i.is_hedge() {
            // Far out, the stop only keeps the margin; near, a rise past it ends the hedge.
            let (to_stop, to_liq) = ((Num::of(s.stop_price) - s.entry).abs() * 2, (Num::of(s.liquidation_price) - s.entry).abs());
            let guarding = match (to_stop.0, to_liq.0) { (Some(a), Some(b)) => a >= b, _ => true };
            let close = i.asset.strip_prefix("xyz:").unwrap_or(&i.asset);
            return Block::new("protection", label, "Stop · no take-profit", vec![
                if guarding {
                    format!("Stop {} → {}: only keeps the margin from liquidation (≈ {}). Below it the short covers the coins.", Fmt::pct(stop_pct),
                            Fmt::price(s.stop_price), Fmt::price(s.liquidation_price))
                } else {
                    format!("Stop {} → {}: a rise past it closes the hedge, and the coins are uncovered from there.", Fmt::pct(stop_pct), Fmt::price(s.stop_price))
                },
                format!("No take-profit: the hedge stays until you close it («close {close}»)."),
                "If you sell the coins, the short is left on its own: close it too.".into(),
            ], status);
        }
        let mut lines = vec![format!("Stop {} → {}. Loss ≈ {} incl. {} fees (ihatecrypto {}).", Fmt::pct(stop_pct), Fmt::price(s.stop_price),
                                     Fmt::usd(s.loss_at_stop_usd), Fmt::usd(s.fees_usd), Fmt::usd(s.builder_fees_usd))];
        match (i.take_pct, s.take_price, s.profit_at_take_usd) {
            (Some(t), Some(tp), Some(profit)) => lines.push(format!("Take-profit {} → {}. Profit ≈ {}.", Fmt::pct(t), Fmt::price(tp), Fmt::usd(profit))),
            _ => lines.push("No take-profit set.".into()),
        }
        if i.partial_take == Some(true) {
            let one_r = Num::of(s.entry) + (Num::of(s.entry) - s.stop_price);
            lines.push(if s.notional / Decimal::TWO >= Self::MIN_NOTIONAL {
                format!("In parts: half at 1R → {}, then stop to breakeven (while ihatecrypto is open).", price(one_r))
            } else {
                "Can't take in parts: half the position is below the $10 minimum order.".into()
            });
        }
        if let Some(trail) = i.trailing_pct {
            lines.push(if i.entry_px.is_none() {
                format!("Trailing {} from the best price, held by Hyperliquid itself.", Fmt::pct(trail))
            } else {
                format!("Trailing {} from the best price, moved by ihatecrypto while it's open.", Fmt::pct(trail))
            });
        }
        if i.twap_minutes.is_some() {
            lines.push("They go on once the TWAP's first part fills, for the whole position: as it grows, they cover all of it.".into());
            lines.push(format!("The risk above counts parts filling up to 3% {}, each part's limit on Hyperliquid. \
                                If the loss at the stop grows past 1.5 × that risk, ihatecrypto stops the TWAP.",
                               if i.side == Side::Long { "higher" } else { "lower" }));
            lines.push("If the stop or take fires while the TWAP still buys, ihatecrypto stops it and closes what it bought after. \
                        The Mac stays awake for that while the TWAP runs. A closed lid, a quit app or a locked trading key leave it buying unwatched.".into());
        }
        lines.push("Triggers on mark price. A market exit may fill worse.".into());
        Block::new("protection", label, "Stop & take-profit", lines, status)
    }
}

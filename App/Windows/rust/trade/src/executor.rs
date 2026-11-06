//! Runs an approved plan on mainnet (Executor.swift): bridge if the trading balance is short, leverage, entry, stop and
//! take, then checks on the exchange that the position is protected. A position is never left without its stop: if the
//! stop cannot be placed, the position is closed at once. Also Hyperliquid's replies and the account's state.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use execution::trade_signer::{TradeSigner, TradeWallet};
use execution::{Entry, Journal, Kind};
use hyperliquid::{MsgPack, OrderKind, action, builder};
use ihatecrypto_core::foundation::{Num, Rounding, decimal, rounded, text};
use ihatecrypto_core::guardian;
use ihatecrypto_core::intent::{Fmt, Side, TradeIntent};
use ihatecrypto_core::network::{Amount, Bridge, Hyperliquid, Origin};
use ihatecrypto_core::plan::{BridgeQuote, Market, Plan, RiskCalc, Sizing};
use ihatecrypto_core::position_command::{PositionCommand, SpotAsset};
use ihatecrypto_core::risk::{Level, RiskPolicy, RiskRules};
use net::solana_rpc::{Outcome, SolanaRpc};
use protocols::relay;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Map, Value, json};
use solana::message::Message;
use wallet::evm::EvmSignature;
use wallet::keys::base58;

use crate::aave::EvmNetwork;
use crate::{CostlyMove, Error, IMPACT_STOP_TEST, LIMIT_FILL_TEST, Result, STOP_MOVE_FAIL_TEST, fail, hl_url, impact_accepted, loss_accepted, num, stop};

pub type Report = Box<dyn Fn(&str) + Send + Sync>;
/// A Jupiter swap that went through: from, to, how much went in and how much arrived (the app keeps spot costs).
pub type Swapped = Box<dyn Fn(&SpotAsset, &SpotAsset, Decimal, Decimal) + Send + Sync>;
/// A token bought (+tokens, for dollars) or sold (−tokens) on an EVM network, under «chain:contract».
pub type Traded = Box<dyn Fn(&str, Decimal, Decimal) + Send + Sync>;

/// What Swift's init takes besides the wallet, the signer, the journal and `report`: its defaults by `Default`.
#[derive(Default)]
pub struct Setup {
    pub rpc: SolanaRpc,
    /// The user confirmed this move in the window: it may ask them to approve ihatecrypto's fee (prepare_fee). A
    /// scheduled task's run never asks.
    pub ask_fee: bool,
    pub swapped: Option<Swapped>,
    pub traded: Option<Traded>,
}

pub struct Executor {
    me: Weak<Executor>,
    pub(crate) wallet: TradeWallet,
    pub(crate) signer: Arc<dyn TradeSigner>,
    pub(crate) journal: Journal,
    pub(crate) rpc: SolanaRpc,
    reporter: Report,
    pub(crate) swapped: Option<Swapped>,
    pub(crate) traded: Option<Traded>,
    pub(crate) ask_fee: bool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    last_nonce: u64,
    /// The fee this account lets ihatecrypto take (maxBuilderFee), read once per move; None when it couldn't be read.
    allowed: Option<u64>,
    allowed_read: bool,
}

/// Accounts whose referral was looked at in this run of the app: once each.
static REFERRAL_LOOKED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Worst price accepted for a market entry or exit, from the mark.
pub(crate) const SLIPPAGE: Decimal = Decimal::from_parts(1, 0, 0, false, 2);
/// Market stop and take fill up to this far past the trigger, as Hyperliquid's own interface does.
pub(crate) const TRIGGER_SLIPPAGE: Decimal = Decimal::from_parts(1, 0, 0, false, 1);

/// What JSONDecoder says of an answer that isn't the shape asked for.
const UNREADABLE: &str = "The data couldn’t be read because it isn’t in the correct format.";

fn sleep(ms: u64) -> tokio::time::Sleep { tokio::time::sleep(Duration::from_millis(ms)) }
fn d(s: &str) -> Decimal { s.parse().expect("a decimal literal") }
/// `HLAction.price` of a perp.
fn price(px: Decimal, sz_decimals: i64) -> Decimal { action::price(px, sz_decimals as i32, false) }
fn usd(x: Decimal) -> String { Fmt::usd(x) }
/// `NSDecimalNumber(decimal:).uint64Value` of a whole amount: 0 when it doesn't fit.
pub(crate) fn uint64(d: Decimal) -> u64 { d.trunc().to_u64().unwrap_or(0) }
fn millionths(units: u64) -> Decimal { Decimal::from(units) / Decimal::from(1_000_000) }

impl Executor {
    pub fn new(wallet: TradeWallet, signer: Arc<dyn TradeSigner>, journal: Journal, report: impl Fn(&str) + Send + Sync + 'static) -> Arc<Self> {
        Self::with(wallet, signer, journal, report, Setup::default())
    }

    pub fn with(wallet: TradeWallet, signer: Arc<dyn TradeSigner>, journal: Journal, report: impl Fn(&str) + Send + Sync + 'static,
                setup: Setup) -> Arc<Self> {
        Arc::new_cyclic(|me| Executor {
            me: me.clone(), wallet, signer, journal, rpc: setup.rpc, reporter: Box::new(report), swapped: setup.swapped, traded: setup.traded,
            ask_fee: setup.ask_fee, state: Mutex::default(),
        })
    }

    /// An owned handle, for a task that outlives the call.
    pub(crate) fn arc(&self) -> Arc<Self> { self.me.upgrade().expect("an executor lives in its Arc") }

    /// A line for the chat.
    pub(crate) fn report(&self, line: &str) { (self.reporter)(line) }

    /// `journal.append(.init(chain:id:kind:note:))`.
    pub(crate) fn record(&self, chain: &str, id: &str, kind: Kind, note: &str) -> Result<()> {
        self.journal.append(&Entry::new(chain, id, kind, note)).map_err(Error::Wallet)
    }

    fn state(&self) -> MutexGuard<'_, State> { self.state.lock().unwrap_or_else(|e| e.into_inner()) }

    /// Smallest bridge worth its fee (Relay ≈ $0.02 flat).
    pub const MIN_BRIDGE_USDC: Decimal = Decimal::TWO;

    /// What a plan brings to Hyperliquid. A little over its need: the entry may fill up to 1% worse than the mark the
    /// plan used. A tiny top-up would pay about as much in bridge fees as it moves, so at least `MIN_BRIDGE_USDC` when
    /// Solana has it; the rest stays free on Hyperliquid. The plan's card quotes the same number.
    pub fn bridge_arrive(need: Decimal, margin: Decimal, solana_usdc: Option<Decimal>) -> Decimal {
        // ponytail: the plan's own numbers, checked by it; one past range arrives as `need`.
        let mut arrive = (Num::of(need) + Num::of(margin) * SLIPPAGE).rounded(2, Rounding::Up).0.unwrap_or(need);
        if arrive < Self::MIN_BRIDGE_USDC {
            if let Some(on_solana) = solana_usdc {
                arrive = arrive.max(Self::MIN_BRIDGE_USDC.min((Num::of(on_solana) - d("0.05")).rounded(2, Rounding::Down).0.unwrap_or(Decimal::ZERO)));
            }
        }
        arrive
    }

    /// What a move may lose: up to `cap` by itself, up to what the user accepted with «Send anyway»; past both it stops.
    pub fn check_loss(lost: Decimal, paid: Decimal, cap: Decimal, what: &str) -> Result<()> {
        if lost <= cap || lost <= loss_accepted() { return Ok(()) }
        Err(CostlyMove::new(lost, paid, what).into())
    }

    /// How far a swap may move the price on its own. Past it (and past $1), it stops before anything is signed and asks.
    pub const IMPACT_CAP: Decimal = Decimal::from_parts(2, 0, 0, false, 2);

    /// Stops a swap whose price impact (0…1) is past `IMPACT_CAP` and what was accepted: `paid` is what goes in, in USD at
    /// the market price (0 = nobody prices it: the impact alone decides), `amount` the same in the input's own units, for
    /// the part that moves the price ≈ 1%. `signed`: what already went and where the money waits — then it's a plain
    /// refusal, since «Nothing was signed» would be untrue.
    pub fn check_impact(impact: Decimal, paid: Decimal, amount: Decimal, symbol: &str, venue: &str, signed: Option<&str>) -> Result<()> {
        let test = IMPACT_STOP_TEST.load(Relaxed);
        let impact = if test { impact.max(d("0.03")) } else { impact };
        let lost = (Num::of(paid) * impact).0;
        if impact <= Self::IMPACT_CAP || impact <= impact_accepted() || !(test || paid.is_zero() || lost.is_some_and(|l| l > Decimal::ONE)) {
            return Ok(());
        }
        let lost = lost.unwrap_or_default();
        // ponytail: impact grows about in step with size; the part is quoted and checked again when it runs.
        let part = (Num::of(amount) * d("0.01") / impact).0
            .map_or(Decimal::ZERO, |share| rounded(share, if share < Decimal::TEN { 6 } else { 0 }, Rounding::Down));
        let moved = (Num::of(impact) * 100).rounded(1, Rounding::Plain).0.map_or("NaN".into(), |m| Fmt::num(m, 2));
        let words = format!("At once, this moves {venue}'s price by {moved}%{}{}",
                            if paid > Decimal::ZERO { format!(": ≈ {} of {} lost to the thin market. ", usd(lost), usd(paid)) }
                            else { format!(", and nobody prices {symbol} to say it in dollars. ") },
                            if part > Decimal::ZERO { format!("{} {symbol} at a time moves it about 1%. ", Fmt::num(part, 6)) } else { String::new() });
        if let Some(signed) = signed { return fail(words + signed) }
        Err(Error::Costly(CostlyMove {
            lost, paid, what: venue.into(), words: Some(words + "Nothing was signed."), impact: Some(impact),
            part: (part > Decimal::ZERO).then(|| (part, symbol.to_string())),
        }))
    }
}

/// What an opened plan came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub asset: String,
    pub size: Decimal,
    pub entry: Decimal,
    pub stop: Decimal,
    pub take: Option<Decimal>,
    /// A limit entry still waiting on the book; its stop and take switch on when it fills.
    pub pending: bool,
    /// First partial take (half at 1R), when the plan asked for one.
    pub partial_take: Option<Decimal>,
    /// The plan's trailing stop went on the exchange, which moves it from now on; false leaves it to the app.
    pub trailing_on_exchange: bool,
    /// The entry runs as this TWAP on the exchange; `size` is what it buys in all, `entry` its first part's price.
    pub twap_id: Option<u64>,
}

impl Opened {
    pub fn new(asset: &str, size: Decimal, entry: Decimal, stop: Decimal, take: Option<Decimal>) -> Self {
        Self { asset: asset.into(), size, entry, stop, take, pending: false, partial_take: None, trailing_on_exchange: false, twap_id: None }
    }
}

/// What a try at the half at 1R came to: on the book, not yet (the limit is only partly filled), or skipped (under $10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialTake {
    Placed(Decimal),
    Waiting,
    Skipped,
}

/// The bridge transaction exactly as it will be signed, with its simulated effect.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeStep {
    pub request_id: String,
    pub unsigned: Vec<u8>,
    pub accounts: Vec<String>,
    pub token_owners: BTreeMap<String, String>,
    pub last_valid: u64,
    pub spend: Decimal,
    pub arriving: Decimal,
    pub fees_usd: String,
    /// Where the USDC goes: Hyperliquid for trades, an EVM network.
    pub chain: u64,
    /// Part of the fee that is Hyperliquid's one-time charge for opening the account; 0 for an existing one.
    pub opening: Decimal,
    /// The journal's words for the order, when it isn't Solana's USDC (a cross-network swap).
    pub note: Option<String>,
}

/// Everything the user confirms: what will be signed, in order, and what it does to each balance.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub intent: TradeIntent,
    pub market: Market,
    pub sizing: Sizing,
    pub leverage: i64,
    pub bridge: Option<BridgeStep>,
    /// USDC to move from the main Hyperliquid balance to a HIP-3 exchange's margin («xyz»).
    pub dex_transfer: Option<(String, Decimal)>,
    /// Solana was short: this much USDC goes straight from an EVM network to Hyperliquid instead of `bridge`.
    pub fund_from: Option<(EvmNetwork, Decimal)>,
    /// A new Hyperliquid account opened through NEAR Intents' 1Click: USDC units sent from Solana and what lands.
    pub open_via_1click: Option<(u64, Decimal)>,
    /// «Solana USDC: 5.80 → 3.75 (−2.05)», one line per balance.
    pub changes: Vec<String>,
    /// One line per signature, in order.
    pub steps: Vec<String>,
    pub prepared_at: DateTime<Utc>,
    /// The first trade asks to approve ihatecrypto's fee: one signature more than the plan's steps.
    pub asks_fee: bool,
}

impl Prepared {
    pub fn new(intent: TradeIntent, market: Market, sizing: Sizing, leverage: i64, bridge: Option<BridgeStep>, changes: Vec<String>,
               steps: Vec<String>, prepared_at: DateTime<Utc>) -> Self {
        Self { intent, market, sizing, leverage, bridge, dex_transfer: None, fund_from: None, open_via_1click: None, changes, steps, prepared_at,
               asks_fee: false }
    }

    /// The bridge transaction's blockhash lives about a minute; after that the preview is redone.
    pub fn is_fresh(&self) -> bool { (Utc::now() - self.prepared_at).num_milliseconds() < 45_000 }
}

/// What the guardian's look at a TWAP entry came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TwapWatch {
    /// Running with its stop on the exchange, the loss at it within bounds: nothing to do.
    Running,
    /// Hyperliquid runs it no more (finished, or stopped elsewhere): nothing left to watch.
    Ended,
    /// Stopped now, and why, for the chat.
    Stopped(String),
}

impl Executor {
    // MARK: Prepare

    /// Fresh prices and balances, the bridge quote and transaction, and a simulation of it. Nothing is signed. The risk
    /// rules are applied here too, on fresh balances. `open_via_1click`: a new Hyperliquid account opens through NEAR
    /// Intents instead of Relay. `spot`: what the wallet holds of each market's coin, which sizes a hedge.
    pub async fn prepare(&self, requested: &TradeIntent, policy: &RiskPolicy, open_via_1click: bool, spot: &HashMap<String, Decimal>)
        -> Result<Prepared> {
        let markets = hl_markets().await?;
        let Some(mut market) = markets.get(&requested.asset).filter(|m| m.index >= 0).cloned() else {
            return fail(format!("No {} market on Hyperliquid.", requested.asset));
        };
        market.atr_pct = Hyperliquid::atr_pct(&market.name, Utc::now()).await.ok();
        let (evm, sol) = (self.wallet.evm.as_str(), self.wallet.solana.as_str());
        let hl = Hyperliquid::balance(evm).await?;
        let solana_usdc = millionths(self.rpc.token_balance(sol, relay::currency::SOLANA_USDC).await?);
        let mut equity = Num::of(hl.account_value) + solana_usdc;
        for dex in Hyperliquid::HIP3_DEXES {
            equity = equity + Hyperliquid::dex_balance(evm, dex).await.map_or(Decimal::ZERO, |b| b.0);
        }
        // Idle USDC on the EVM networks is capital too, and the trade can be paid from it when Solana is short.
        let mut held: HashMap<String, Decimal> = HashMap::new();
        for net in EvmNetwork::searched() {
            if let Ok(u) = net.usdc_balance(evm).await { held.insert(net.key.into(), millionths(u)); }
        }
        let equity = num(held.values().fold(equity, |a, &v| a + v))?;
        let mut spot = spot.clone();
        // The coins a hedge covers, read again where it's cheap. ponytail: only SOL is re-read.
        if requested.is_hedge() && market.name == "SOL" {
            let lamports = Decimal::from(self.rpc.balance(sol).await?) / Decimal::from(1_000_000_000u64);
            spot.insert("SOL".into(), spot.get("SOL").copied().unwrap_or_default().min(lamports));
        }
        let hedged = requested.is_hedge().then(|| spot.get(&market.name).copied().unwrap_or_default());
        let intent = Plan::resolve(requested, Some(&market), Some(equity), policy, hedged).0;
        let evaluation = Plan::evaluate(&intent, Some(&market));
        let (true, None, Some(sizing), Some(stop_pct)) = (evaluation.missing.is_empty(), &evaluation.problem, evaluation.sizing.clone(), intent.stop_pct)
        else {
            return fail(evaluation.problem.clone().unwrap_or_else(|| format!("The plan isn't ready: missing {}.", evaluation.missing.join(", "))));
        };
        // One plan per asset: a second entry would merge into the open position and its stop.
        let positions: Vec<Position> = HyperliquidAccount::positions(evm).await?.into_iter().filter(|p| !p.size.is_zero()).collect();
        let orders = HyperliquidAccount::open_orders(evm).await?;
        if positions.iter().any(|p| p.coin == market.name) || orders.iter().any(|o| o.coin == market.name) {
            return fail(format!("There's already a position or order on {}. Manage it in chat («stop to breakeven», «close half», «cancel the limit») \
                                 or pick another asset.", market.name));
        }
        let twaps = HyperliquidAccount::running_twaps(evm, None).await?;
        if twaps.iter().any(|t| t.coin == market.name) {
            return fail(format!("A TWAP is still running on {}. «cancel the TWAP» stops it, or pick another asset.", market.name));
        }
        let marks: HashMap<String, Decimal> = markets.iter().map(|(k, m)| (k.clone(), m.mark_px)).collect();
        let open_risk = HyperliquidAccount::risk_at_stops(&positions, &orders, &spot)
            .and_then(|at| HyperliquidAccount::twap_risk(&twaps, &positions, &orders, &marks).and_then(|t| (Num::of(at) + t).0));
        let blocked: Vec<String> = RiskRules::check(&intent, &sizing, &market, Some(equity), policy, open_risk).into_iter()
            .filter(|a| a.level == Level::Block).map(|a| a.text).collect();
        if !blocked.is_empty() { return fail(blocked.join(" ")) }
        let leverage = intent.leverage.unwrap_or(Decimal::ONE).trunc().to_i64().unwrap_or(0);

        // HIP-3 markets («xyz:CL») keep margin on their own exchange: what is free there counts, and the rest moves over
        // from the main balance before the order. A unified account trades them all from its one balance.
        let mut dex_free = Decimal::ZERO;
        if let Some(dex) = &market.dex { dex_free = Hyperliquid::dex_balance(evm, dex).await?.1 }
        let mut dex_transfer = None;
        if let (Some(dex), false) = (&market.dex, hl.unified) {
            let needed = num(Num::of(sizing.margin) * (Decimal::ONE + SLIPPAGE) + sizing.fees_usd)?;
            if needed > dex_free { dex_transfer = Some((dex.clone(), num((Num::of(needed) - dex_free).rounded(2, Rounding::Up))?)) }
        }
        let mut amount = None;
        if let Some(usdc) = intent.amount_usdc {
            amount = Some(Amount::Send(usdc));
        } else if let Some(need) = Plan::bridge_need(&intent, &evaluation, (Num::of(hl.withdrawable) + dex_free).0, Some(market.mark_px)) {
            let mut arrive = Self::bridge_arrive(need, sizing.margin, None);
            if arrive < Self::MIN_BRIDGE_USDC {
                let on_solana = millionths(self.rpc.token_balance(sol, relay::currency::SOLANA_USDC).await?);
                arrive = Self::bridge_arrive(need, sizing.margin, Some(on_solana));
            }
            amount = Some(Amount::Arrive(arrive));
        }
        let mut prepared: Option<(BridgeStep, Vec<String>)> = None;
        let mut fund: Option<(&'static EvmNetwork, Decimal, BridgeQuote, bool, Decimal)> = None;
        let mut opening: Option<(u64, Decimal)> = None;
        if let Some(amount) = amount {
            // Solana first. Short there, the money goes straight from an EVM network that holds it — the same choice, from
            // the same dry quote, as the plan's card.
            let new_account = Hyperliquid::is_new_account(evm).await.unwrap_or(false);
            let via_solana = match amount {
                Amount::Send(v) => v,
                Amount::Arrive(_) => Bridge::dry_quote(amount, new_account, None, false).await?.amount_in,
            };
            let from = if via_solana > solana_usdc { EvmNetwork::funding(via_solana, |k| held.get(k).copied()) } else { None };
            if let Some(net) = from {
                let origin = Origin { chain: net.relay_chain, usdc: net.usdc.into(), decimals: net.usdc_decimals };
                let q = Bridge::dry_quote(Amount::Send(via_solana), new_account, Some(&origin), false).await?;
                // No gas coin there: a little comes from Solana first (ensure_evm), which needs half a dollar there.
                let gas = if net.fee_token.is_none() { net.rpc.balance(evm).await? >= net.gas_floor as u128 } else { true };
                if !gas && solana_usdc < d("0.7") {
                    return fail(format!("{} has the USDC but no {} for gas, and Solana has too little USDC to bring it. Send a little {} to {} or USDC to Solana.",
                                        net.name, net.gas_coin, net.gas_coin, net.name));
                }
                fund = Some((net, via_solana, q, gas, held.get(net.key).copied().unwrap_or_default()));
            } else if new_account && open_via_1click {
                opening = Some(self.one_click_opening(amount, solana_usdc).await?);
            } else {
                prepared = Some(self.prepare_bridge(amount, relay::chain::HYPERLIQUID, relay::currency::HYPERLIQUID_USDC, 8, None, false).await?);
            }
        }
        let bridge = prepared.as_ref().map(|p| p.0.clone());
        let mut changes = prepared.map(|p| p.1).unwrap_or_default();
        let mut steps = vec![];
        if let Some(b) = &bridge {
            steps.push(format!("Relay bridge: {} USDC from Solana → {} USDC on Hyperliquid, fee {}{}", Fmt::num(b.spend, 2), Fmt::num(b.arriving, 2),
                               decimal(&b.fees_usd).map_or_else(|| format!("${}", b.fees_usd), usd),
                               if b.opening > Decimal::ZERO { format!(", of it {} once for opening your Hyperliquid account.", usd(b.opening)) } else { ".".into() }));
        }
        if let Some((net, usdc, q, gas, have)) = &fund {
            if !gas {
                steps.push(format!("{} for gas on {} first: 0.5 USDC from Solana over Relay, the {} arrives with it.", net.gas_coin, net.name, net.gas_coin));
            }
            steps.push(format!("Relay bridge straight from {}: approve exactly {} USDC, then Relay's deposit → ≈ {} USDC on Hyperliquid, fee ≈ {}. \
                                Solana has {} USDC, less than the trade needs.", net.name, Fmt::num(*usdc, 2), Fmt::num(q.amount_out, 2),
                               usd(num(Num::of(q.amount_in) - q.amount_out)?), Fmt::num(solana_usdc, 2)));
            changes.push(format!("{} USDC: {} → {} (−{}).", net.name, Fmt::num(*have, 2), Fmt::num(num(Num::of(*have) - *usdc)?, 2), Fmt::num(*usdc, 2)));
        }
        if let Some((units, arriving)) = opening {
            let send = millionths(units);
            steps.push(format!("NEAR Intents opens your Hyperliquid account: {} USDC from Solana → ≈ {} USDC on Hyperliquid, fee {}, no opening fee. \
                                Usually 2–4 min; the trade waits for it and re-checks the price.", Fmt::num(send, 2), Fmt::num(arriving, 2),
                               usd(num(Num::of(send) - arriving)?)));
            changes.push(format!("Solana USDC: {} → {} (−{}).", Fmt::num(solana_usdc, 2), Fmt::num(solana_usdc - send, 2), Fmt::num(send, 2)));
        }
        let arriving = bridge.as_ref().map(|b| b.arriving).or(fund.as_ref().map(|f| f.2.amount_out)).or(opening.map(|o| o.1)).unwrap_or_default();
        let long = intent.side == Side::Long;
        let side = if long { "Long" } else { "Short" };
        let sz = market.sz_decimals;
        if let Some((dex, usdc)) = &dex_transfer {
            steps.push(format!("Transfer {} USDC from the main Hyperliquid balance to the {dex} exchange — its markets have their own margin.", Fmt::num(*usdc, 2)));
            changes.push(format!("Hyperliquid {dex}, free: {} → {} before entry.", usd(dex_free), usd(num(Num::of(dex_free) + *usdc)?)));
        }
        // ihatecrypto's fee is asked for once, here, by the signer; it stays off a card where Hyperliquid has it approved.
        let fee = if self.ask_fee { self.allowed_fee().await } else { None };
        let asks_fee = fee.is_some_and(|f| f < builder::FEE);
        if asks_fee {
            steps.push(format!("Approve ihatecrypto's fee once: {} of each Hyperliquid trade, on top of the exchange's own. Settings › Wallet › \
                                Hyperliquid can take it back.", builder::RATE));
        }
        steps.push(format!("Leverage {leverage}×, isolated margin."));
        let take_words = |prefix: &str| intent.take_pct.map(|t| format!(" and take-profit {prefix}{}", Fmt::pct(t))).unwrap_or_default();
        if let Some(minutes) = intent.twap_minutes {
            steps.push(format!("TWAP on Hyperliquid: {} {} {} over {}, parts at the market every 30 s or more, each within 3% of the price, {}. \
                                ihatecrypto's {} isn't charged on it: Hyperliquid's TWAP takes no builder fee.",
                               if long { "buy" } else { "sell" }, Fmt::num(sizing.size, sz as u32), market.name, TradeIntent::duration(minutes),
                               if intent.twap_randomize == Some(true) { "sizes randomized ±20%" } else { "equal sizes" }, builder::RATE));
            steps.push(format!("Stop {}{} from the first part's price, on the exchange for the whole position, once that part fills. «cancel the TWAP» stops \
                                the rest, and ihatecrypto stops it if the stop or take fires first or the loss at the stop grows past 1.5 × the plan's, \
                                keeping the Mac awake while it runs. A closed lid, a quit app or a locked trading key leave it unwatched.",
                               Fmt::pct(stop_pct), take_words("")));
        } else if let Some(limit) = intent.entry_px {
            steps.push(format!("{side} {} {} as a limit at {} (now {}), waiting on the exchange, with stop −{}{}, active once it fills.",
                               Fmt::num(sizing.size, sz as u32), market.name, Fmt::price(price(limit, sz)), Fmt::price(market.mark_px), Fmt::pct(stop_pct),
                               take_words("+")));
        } else {
            let worst = num(Num::of(market.mark_px) * if long { Decimal::ONE + SLIPPAGE } else { Decimal::ONE - SLIPPAGE })?;
            steps.push(format!("{side} {} {} at market, no worse than {}.", Fmt::num(sizing.size, sz as u32), market.name, Fmt::price(price(worst, sz))));
            steps.push(if intent.is_hedge() {
                format!("Stop {} above the entry price, on the exchange: it only closes the hedge before liquidation. No take-profit.", Fmt::pct(stop_pct))
            } else {
                format!("Stop {}{} from the entry price ({}), on the exchange.", Fmt::pct(stop_pct), take_words(""),
                        if long { "stop below, take above" } else { "stop above, take below" })
            });
        }
        if intent.partial_take == Some(true) && intent.entry_px.is_none() {
            steps.push("Partial take-profit: half the position at 1R, as a separate order on the exchange; then the stop moves to breakeven.".into());
        }
        if let Some(trail) = intent.trailing_pct {
            steps.push(if intent.entry_px.is_none() {
                format!("Trailing stop {} on the exchange after the entry, reduce-only: Hyperliquid moves it behind the best price, with ihatecrypto closed too.",
                        Fmt::pct(trail))
            } else {
                format!("Trailing {}: ihatecrypto moves the stop once the limit fills, while the app is open (each move is a signature).", Fmt::pct(trail))
            });
        }
        let free_after = num(Num::of(hl.withdrawable) + arriving - sizing.margin - sizing.fees_usd / Decimal::TWO)?;
        changes.push(format!("Hyperliquid, free: {} → ≈ {} (margin {} goes into the position).", usd(hl.withdrawable), usd(free_after), usd(sizing.margin)));
        changes.push(if intent.is_hedge() {
            format!("Position: short {} {}, a hedge of the {} the wallet holds; net of them it costs ≈ {} in fees.", Fmt::num(sizing.size, sz as u32),
                    market.name, market.name, usd(sizing.fees_usd))
        } else {
            format!("Position: {} {} {}, at the stop ≈ −{}{}.", side.to_lowercase(), Fmt::num(sizing.size, sz as u32), market.name, usd(sizing.loss_at_stop_usd),
                    sizing.profit_at_take_usd.map(|p| format!(", at the take ≈ +{}", usd(p))).unwrap_or_default())
        });
        let mut p = Prepared::new(intent, market, sizing, leverage, bridge, changes, steps, Utc::now());
        p.dex_transfer = dex_transfer;
        p.fund_from = fund.map(|f| (f.0.clone(), f.1));
        p.open_via_1click = opening;
        p.asks_fee = asks_fee;
        Ok(p)
    }

    /// The USDC from Solana that opens a new Hyperliquid account through 1Click, and what its quote lands.
    pub(crate) async fn one_click_opening(&self, amount: Amount, solana_usdc: Decimal) -> Result<(u64, Decimal)> {
        let send = match amount {
            Amount::Send(v) => v,
            Amount::Arrive(_) => Bridge::one_click_quote(amount, Bridge::PLACEHOLDER_RECIPIENT).await?.amount_in,
        };
        if send > solana_usdc {
            return fail(format!("Solana has {} USDC; opening through NEAR Intents needs {}. Nothing was signed.", Fmt::num(solana_usdc, 2), Fmt::num(send, 2)));
        }
        let units = uint64(num((Num::of(send) * 1_000_000).rounded(0, Rounding::Up))?);
        match self.one_click_quote(None, Some("hyperliquid"), units).await.and_then(|q| decimal(&q.amount_out_formatted)) {
            Some(out) if out > Decimal::ZERO => Ok((units, out)),
            _ => fail("NEAR Intents didn't quote Solana → Hyperliquid just now. Nothing was signed; Relay can open it instead."),
        }
    }

    // MARK: Open

    /// Signs and sends what `prepare` showed. Prices may move inside the approved limits; outside them it stops.
    pub async fn execute(&self, p: &Prepared) -> Result<Opened> {
        if !p.is_fresh() { return fail("This confirmation is stale: the numbers need a refresh.") }
        let moved = |m: &Market| (Num::of(m.mark_px) - p.market.mark_px).abs() / p.market.mark_px;
        let mut market = hl_markets().await?.remove(&p.market.name).unwrap_or_else(|| p.market.clone());
        if moved(&market).0.is_none_or(|x| x > SLIPPAGE) {
            return fail(format!("{} moved more than 1% since you confirmed. Nothing was signed.", market.name));
        }
        let long = p.intent.side == Side::Long;
        if let Some(b) = &p.bridge { self.send_bridge(b).await?; }
        if let Some((units, arriving)) = p.open_via_1click {
            let got = self.one_click_move(None, Some("hyperliquid"), units, arriving, "USDC that opens Hyperliquid").await?;
            self.report(&format!("{} USDC are on Hyperliquid: the account is open.", Fmt::num(got, 2)));
            // Minutes have passed: the entry goes in only at the price confirmed.
            market = hl_markets().await?.remove(&p.market.name).unwrap_or(market);
            if moved(&market).0.is_none_or(|x| x > SLIPPAGE) {
                return fail(format!("{} moved more than 1% while the USDC travelled. No trade; the money is on your Hyperliquid balance.", market.name));
            }
        }
        if let Some((net, usdc)) = &p.fund_from {
            self.ensure_evm(net, 0, None).await?;
            let line = self.bridge(net, None, true, Some(*usdc)).await?;
            self.report(&line);
        }
        if let Some((dex, usdc)) = &p.dex_transfer { self.move_margin("", dex, *usdc).await? }
        self.prepare_fee().await?;

        let lev = self.post(&Self::leverage_action(p.market.index, p.leverage), "leverage").await?;
        if lev != HlReply::Ok { return fail(format!("Hyperliquid rejected leverage {}×: {lev}.", p.leverage)) }
        self.report(&format!("Leverage {}×, isolated margin.", p.leverage));

        if let Some(minutes) = p.intent.twap_minutes { return self.run_twap(p, &market, minutes).await }
        if let Some(limit) = p.intent.entry_px { return self.place_limit(p, &market, limit).await }

        let sz = market.sz_decimals;
        let entry_px = price(num(Num::of(market.mark_px) * if long { Decimal::ONE + SLIPPAGE } else { Decimal::ONE - SLIPPAGE })?, sz);
        let order = action::order(&[hyperliquid::Order {
            asset: market.index as u32, is_buy: long, price: entry_px, size: p.sizing.size, reduce_only: false,
            kind: OrderKind::Limit { tif: "Ioc".into() }, cloid: None,
        }], "na")?;
        let entry = match self.post(&order, "entry").await {
            Ok(r) => r,
            // The reply was lost, not necessarily the order: an entry that filled is a position, and it gets its stop as
            // usual. A stop before it went out (the guard, the signer, a Ledger declined) isn't caught here.
            Err(Error::NoReply(lost)) => {
                let Ok(positions) = HyperliquidAccount::positions(&self.wallet.evm).await else {
                    return fail(format!("The entry's reply didn't come ({lost}) and the positions can't be read now. Check {} on Hyperliquid: if it \
                                         opened, it has no stop yet.", market.name));
                };
                let Some(open) = positions.into_iter().find(|o| o.coin == market.name && !o.size.is_zero()) else {
                    return fail(format!("The entry's reply didn't come ({lost}); no {} position is open, the money is on your Hyperliquid balance.", market.name));
                };
                self.report(&format!("The entry's reply didn't come, but the {} position is open: placing its stop.", market.name));
                HlReply::Filled { size: open.size.abs(), avg_px: open.entry }
            }
            Err(e) => return Err(e),
        };
        let HlReply::Filled { size, avg_px } = entry else {
            return fail(format!("The entry didn't fill ({entry}). No position; the money is on your Hyperliquid balance."));
        };
        self.report(&format!("{} {} {} at {}.", if long { "Long" } else { "Short" }, Fmt::num(size, sz as u32), market.name, Fmt::price(avg_px)));

        // Stop and take from the actual fill, so the loss at the stop stays the approved risk.
        let (stop, take) = Self::protection_prices(avg_px, p.intent.stop_pct.unwrap_or_default(), p.intent.take_pct, long, sz)?;
        self.protect(&market, long, size, stop, take).await?;
        let mut opened = Opened::new(&market.name, size, avg_px, stop, take);
        if p.intent.partial_take == Some(true) { opened.partial_take = self.partial_take_after_entry(&market, long, size, avg_px, stop).await }
        if let Some(pct) = p.intent.trailing_pct {
            // The position is open and has its stop: a trailing stop that didn't go on is left to the app, not a failed trade.
            match self.place_trailing(&market, long, size, pct).await {
                Ok(line) => {
                    self.report(&line);
                    opened.trailing_on_exchange = true;
                }
                Err(Error::Guard(refused)) => self.report(&format!("The trailing stop wasn't signed ({refused}): ihatecrypto moves the stop while it's open.")),
                Err(e) => self.report(&format!("The trailing stop didn't go on ({e}): ihatecrypto moves the stop while it's open.")),
            }
        }
        Ok(opened)
    }

    /// The open position on `coin`: None when the positions don't read, Some(None) when there's none.
    async fn open_position(&self, coin: &str) -> Option<Option<Position>> {
        HyperliquidAccount::positions(&self.wallet.evm).await.ok().map(|ps| ps.into_iter().find(|p| p.coin == coin && !p.size.is_zero()))
    }

    /// The TWAP stopped by its id, in words: it never builds a position without a stop.
    async fn stop_twap(&self, market: &Market, id: u64) -> String {
        match self.post(&action::twap_cancel(market.index as u32, id), "cancel TWAP").await {
            Err(_) => "The TWAP's cancel got no answer: «cancel the TWAP» stops it.".into(),
            Ok(r) if r.is_error() => format!("The TWAP didn't stop ({r}): «cancel the TWAP» stops it."),
            Ok(_) => "The TWAP is stopped.".into(),
        }
    }

    /// The entry as one TWAP: started, then the stop and take for the whole position once its first part fills (size 0:
    /// Hyperliquid's position TP/SL grows with it). A TWAP that fills nothing in 90 s, or whose stop doesn't go on, is
    /// stopped: it never builds a position without a stop.
    async fn run_twap(&self, p: &Prepared, market: &Market, minutes: i64) -> Result<Opened> {
        let long = p.intent.side == Side::Long;
        let sz = market.sz_decimals;
        let twap = action::twap_order(market.index as u32, long, p.sizing.size, false, minutes, p.intent.twap_randomize == Some(true))?;
        let id = match self.post(&twap, &format!("TWAP over {minutes} min")).await {
            Ok(HlReply::Running(started)) => started,
            Ok(reply) => return fail(format!("Hyperliquid didn't start the TWAP ({reply}). Nothing was bought.")),
            // The reply was lost, not necessarily the TWAP: one running on the coin now is this one. Asked a few times: a
            // read that fails isn't «none».
            Err(Error::NoReply(lost)) => {
                let mut found = None;
                for attempt in 0..5 {
                    if found.is_some() { break }
                    found = HyperliquidAccount::running_twaps(&self.wallet.evm, Some(&market.name)).await.ok().and_then(|t| t.first().map(|t| t.id));
                    if found.is_none() && attempt < 4 { tokio::time::sleep(Duration::from_secs(2)).await }
                }
                let Some(found) = found else {
                    return fail(format!("The TWAP's reply didn't come ({lost}) and Hyperliquid doesn't list it. It may still run with no stop: check {} on \
                                         Hyperliquid, «cancel the TWAP» stops it.", market.name));
                };
                found
            }
            Err(e) => return Err(e),
        };
        self.report(&format!("TWAP {id} on Hyperliquid: {} {} {} over {}.", if long { "buying" } else { "selling" }, Fmt::num(p.sizing.size, sz as u32),
                             market.name, TradeIntent::duration(minutes)));
        // The first part goes at once: the position shows within seconds.
        let mut first = None;
        for attempt in 0..45 {
            first = self.open_position(&market.name).await.flatten();
            if first.is_some() || attempt == 44 { break }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let Some(first) = first else {
            let stopped = self.stop_twap(market, id).await;
            // A part that filled late, or while the positions didn't read, is closed rather than left without a stop.
            return match self.open_position(&market.name).await {
                Some(None) => fail(format!("The TWAP's first part didn't fill within 90 s. {stopped} Nothing is open.")),
                Some(Some(_)) => {
                    let closed = self.close(&market.name).await.unwrap_or_else(|_| {
                        format!("The close didn't go through: check {} on Hyperliquid, it may be open without a stop.", market.name)
                    });
                    fail(format!("The TWAP's first part came late. {stopped} {closed}"))
                }
                None => fail(format!("The TWAP's first part didn't show within 90 s and the positions don't read now. {stopped} Check {} on Hyperliquid.",
                                     market.name)),
            };
        };
        let (stop, take) = Self::protection_prices(first.entry, p.intent.stop_pct.unwrap_or_default(), p.intent.take_pct, long, sz)?;
        let protected = async {
            let protection = |size: Decimal| async move {
                self.post(&Self::protection_action(market.index, long, size, stop, take, sz)?, "stop and take-profit").await
            };
            // Size 0 is the whole position as it grows; refused, the whole TWAP's size (reduce-only).
            let mut placed = protection(Decimal::ZERO).await?;
            if placed.is_error() { placed = protection(p.sizing.size).await? }
            if let HlReply::Error(e) = placed { return fail(e) }
            self.verify_protection(&market.name, stop, take).await
        }.await;
        if let Err(error) = protected {
            let stopped = self.stop_twap(market, id).await;
            let closed = match self.close(&market.name).await {
                Ok(c) => c,
                Err(failed) => format!("The close failed too: {failed} {}", self.restore_stop(market, stop).await),
            };
            return fail(format!("The TWAP's stop didn't go on ({error}). {stopped} {closed}"));
        }
        self.report(&format!("On the exchange: stop {}{} for the whole {} position as the TWAP builds it.", Fmt::price(stop),
                             take.map(|t| format!(" and take-profit {}", Fmt::price(t))).unwrap_or_default(), market.name));
        let mut opened = Opened::new(&market.name, p.sizing.size, first.entry, stop, take);
        opened.twap_id = Some(id);
        Ok(opened)
    }

    /// A trailing stop on the exchange for the whole position, reduce-only. Returns the line for the user.
    async fn place_trailing(&self, market: &Market, long: bool, size: Decimal, pct: Decimal) -> Result<String> {
        let reply = self.post(&action::trailing_stop(market.index as u32, !long, size, pct)?, &format!("trailing stop {}", Fmt::pct(pct))).await?;
        if let HlReply::Error(e) = reply { return fail(e) }
        Ok(format!("On the exchange: a trailing stop {} behind the best price. Hyperliquid moves it, with ihatecrypto closed too.", Fmt::pct(pct)))
    }

    /// «trail 2%»: a trailing stop on the exchange for the open position, then any earlier one on it cancelled. The fixed
    /// stop stays: whichever is nearer fires first.
    async fn trail(&self, asset: &str, pct: Decimal) -> Result<String> {
        if pct <= Decimal::ZERO || pct >= Decimal::ONE_HUNDRED { return fail("A trailing stop needs a distance between 0 and 100%.") }
        let market = self.market(asset).await?;
        let evm = &self.wallet.evm;
        let Some(position) = HyperliquidAccount::positions(evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero()) else {
            return Ok(format!("No open {asset} position."));
        };
        let old: Vec<Order> = HyperliquidAccount::open_orders(evm).await?.into_iter().filter(|o| o.coin == asset && o.kind().contains("Trailing")).collect();
        let line = self.place_trailing(&market, position.size > Decimal::ZERO, position.size.abs(), pct).await?;
        if !old.is_empty() {
            let _ = self.post(&action::cancel(&cancels(&market, &old)), "cancel old trailing stop").await;
        }
        self.report(&line);
        Ok(line)
    }

    /// Stop and take on an open position as one positionTpsl group, checked among open orders. When they don't go on,
    /// the position is closed so it isn't left unprotected, and when that close fails too, restore_stop puts a stop back.
    async fn protect(&self, market: &Market, long: bool, size: Decimal, stop: Decimal, take: Option<Decimal>) -> Result<()> {
        let placed = async {
            let placed = self.post(&Self::protection_action(market.index, long, size, stop, take, market.sz_decimals)?, "stop and take-profit").await?;
            if let HlReply::Error(e) = placed { return fail(e) }
            self.verify_protection(&market.name, stop, take).await
        }.await;
        if let Err(error) = placed {
            self.report(&format!("The stop didn't go through: {error}. Closing the position so it isn't left unprotected."));
            let closed = match self.close(&market.name).await {
                Ok(c) => c,
                Err(failed) => {
                    let left = self.restore_stop(market, stop).await;
                    return fail(format!("The stop didn't go through and the emergency close failed too: {failed} {left}"));
                }
            };
            return fail(format!("Position closed: the exchange didn't confirm the stop. {closed}"));
        }
        self.report(&format!("On the exchange: stop {}{}. The position is protected.", Fmt::price(stop),
                             take.map(|t| format!(" and take-profit {}", Fmt::price(t))).unwrap_or_default()));
        Ok(())
    }

    /// After an emergency close that failed, at whatever step: what is still open keeps the stop it has on the exchange
    /// (one that showed up late), or gets `stop`.
    pub(crate) async fn restore_stop(&self, market: &Market, stop: Decimal) -> String {
        let evm = &self.wallet.evm;
        let Ok(positions) = HyperliquidAccount::positions(evm).await else {
            return format!("The positions can't be read now: check {} on Hyperliquid, it may be open without a stop.", market.name);
        };
        let Some(p) = positions.into_iter().find(|p| p.coin == market.name && !p.size.is_zero()) else {
            return format!("No {} position is open now.", market.name);
        };
        let stops: Vec<Decimal> = HyperliquidAccount::open_orders(evm).await.map(|o| o.into_iter()
            .filter(|o| o.coin == market.name && o.is_trigger && o.reduce_only && o.kind().contains("Stop")).filter_map(|o| o.trigger_px).collect())
            .unwrap_or_default();
        let on = if p.size > Decimal::ZERO { stops.iter().max() } else { stops.iter().min() };
        if let Some(on) = on { return format!("Its stop at {} is on the exchange.", Fmt::price(*on)) }
        let back = async {
            let action = Self::protection_action(market.index, p.size > Decimal::ZERO, p.size.abs(), stop, None, market.sz_decimals)?;
            if let HlReply::Error(e) = self.post(&action, "stop back on").await? { return fail(e) }
            self.verify_protection(&market.name, stop, None).await
        }.await;
        match back {
            Ok(()) => format!("Its stop at {} is back on the exchange now.", Fmt::price(stop)),
            Err(e) => format!("It is WITHOUT a stop ({e}): set one or close it on Hyperliquid now."),
        }
    }

    /// For a limit entry that has filled: half at 1R from the actual entry and stop on the exchange. `Waiting` while the
    /// limit is only partly filled. A failure throws, so the guardian can try again; a Guard error is a declined signature.
    pub async fn add_partial_take(&self, asset: &str) -> Result<PartialTake> {
        let market = self.market(asset).await?;
        let evm = &self.wallet.evm;
        let orders: Vec<Order> = HyperliquidAccount::open_orders(evm).await?.into_iter().filter(|o| o.coin == asset).collect();
        let Some(p) = HyperliquidAccount::positions(evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero()) else {
            return Ok(PartialTake::Waiting);
        };
        // 1R from the fixed stop: a trailing stop's trigger follows the price.
        let fixed = orders.iter().find(|o| o.is_trigger && o.kind().contains("Stop") && !o.kind().contains("Trailing")).and_then(|o| o.trigger_px);
        let Some(stop) = fixed else {
            if Order::fills_in_part(&orders) { return Ok(PartialTake::Waiting) }
            return fail(format!("{asset} has no stop on the exchange, so the half has no 1R to go at."));
        };
        Ok(self.place_partial_take(&market, p.size > Decimal::ZERO, p.size.abs(), p.entry, stop, &orders).await?
            .map_or(PartialTake::Skipped, PartialTake::Placed))
    }

    /// The half at 1R right after an entry filled: a failure is said, and the position keeps its stop and take.
    async fn partial_take_after_entry(&self, market: &Market, long: bool, size: Decimal, entry: Decimal, stop: Decimal) -> Option<Decimal> {
        match self.place_partial_take(market, long, size, entry, stop, &[]).await {
            Ok(r) => return r,
            Err(Error::NoReply(_)) => self.report("Partial take-profit: the exchange's reply didn't come, so it may or may not be on. The stop and full \
                                                   take-profit are in place; look for a take at 1R among Hyperliquid's open orders."),
            Err(e) => self.report(&format!("Partial take-profit didn't go through ({e}); the stop and full take-profit are still in place.")),
        }
        None
    }

    /// Half the position at 1R as a separate reduce-only take; None when the half is under the $10 minimum. One already on
    /// the book (`on_book`: an earlier try whose reply was lost) counts as placed. Throws when Hyperliquid turns it down.
    async fn place_partial_take(&self, market: &Market, long: bool, size: Decimal, entry: Decimal, stop: Decimal, on_book: &[Order])
        -> Result<Option<Decimal>> {
        let sz = market.sz_decimals;
        let half = action::size(size / Decimal::TWO, sz as i32);
        if num(Num::of(half) * entry)? < Decimal::TEN {
            self.report("Partial take-profit skipped: half the position is below the $10 minimum order.");
            return Ok(None);
        }
        let one_r = price(num(Num::of(entry) + (Num::of(entry) - stop))?, sz);
        // ponytail: found by price and size; a stop moved by hand between a lost reply and the next try moves 1R, and a
        // second half goes on (reduce-only, so it can't open anything).
        let placed = format!("Partial take-profit: {} {} at 1R → {}. After it fills, the stop moves to breakeven.", Fmt::num(half, sz as u32), market.name,
                             Fmt::price(one_r));
        if on_book.iter().any(|o| o.is_trigger && o.reduce_only && o.trigger_px == Some(one_r) && o.size == Some(half)) {
            self.report(&placed);
            return Ok(Some(one_r));
        }
        let worst = price(num(Num::of(one_r) * if long { Decimal::ONE - TRIGGER_SLIPPAGE } else { Decimal::ONE + TRIGGER_SLIPPAGE })?, sz);
        let order = hyperliquid::Order {
            asset: market.index as u32, is_buy: !long, price: worst, size: half, reduce_only: true,
            kind: OrderKind::Trigger { price: one_r, is_market: true, tpsl: "tp".into() }, cloid: None,
        };
        let reply = self.post(&action::order(&[order], "na")?, "partial take-profit").await?;
        if reply.is_error() { return fail(format!("Hyperliquid turned it down: {reply}")) }
        self.report(&placed);
        Ok(Some(one_r))
    }

    /// A resting limit entry with its stop and take in one normalTpsl group: the exchange arms them when the entry fills,
    /// so there is no moment with a position and no stop. One that fills at once and whose stop the exchange doesn't list
    /// gets the stop and take again, and is closed when they don't go on.
    async fn place_limit(&self, p: &Prepared, market: &Market, limit: Decimal) -> Result<Opened> {
        let long = p.intent.side == Side::Long;
        let sz = market.sz_decimals;
        let px = if LIMIT_FILL_TEST.load(Relaxed) {
            price(num(Num::of(market.mark_px) * if long { d("1.003") } else { d("0.997") })?, sz)
        } else {
            price(limit, sz)
        };
        let (stop, take) = Self::protection_prices(px, p.intent.stop_pct.unwrap_or_default(), p.intent.take_pct, long, sz)?;
        let entry = hyperliquid::Order {
            asset: market.index as u32, is_buy: long, price: px, size: p.sizing.size, reduce_only: false, kind: OrderKind::Limit { tif: "Gtc".into() },
            cloid: None,
        };
        let mut orders = vec![entry];
        orders.extend(Self::protection_orders(market.index, long, p.sizing.size, stop, take, sz)?);
        let reply = self.post(&action::order(&orders, "normalTpsl")?, "limit with stop and take-profit").await?;
        let mut filled = match reply { HlReply::Filled { size, avg_px } => Some((size, avg_px)), _ => None };
        // Any status's error wins: the entry may have filled, or rest, while its stop or take was turned down.
        let refused = match &reply { HlReply::Error(e) => Some(e.clone()), _ => None };
        let mut unconfirmed = refused.clone();
        // Otherwise what's there is read back a few times (≈ 2.4 s): /info may list the limit or show its fill late.
        let (mut read, mut cancelled) = (false, false);
        let evm = &self.wallet.evm;
        for attempt in 0..5 {
            if filled.is_some() { break }
            let resting = HyperliquidAccount::open_orders(evm).await.ok()
                .and_then(|o| o.into_iter().find(|o| o.coin == market.name && !o.is_trigger && o.limit_px == Some(px)));
            if let Some(entry) = resting {
                let Some(refused) = &refused else {
                    self.report(&format!("Limit placed: {} {} {} at {}. Stop {}{}: active once it fills.", if long { "long" } else { "short" },
                                         Fmt::num(p.sizing.size, sz as u32), market.name, Fmt::price(px), Fmt::price(stop),
                                         take.map(|t| format!(" and take-profit {}", Fmt::price(t))).unwrap_or_default()));
                    let mut opened = Opened::new(&market.name, p.sizing.size, px, stop, take);
                    opened.pending = true;
                    return Ok(opened);
                };
                // On the book without the stop or take that was turned down: it goes before it fills unprotected.
                let cancel = self.post(&action::cancel(&[(market.index as u32, entry.oid)]), "cancel limit").await?;
                if cancel.is_error() {
                    return fail(format!("Limit rejected: {refused}. Its entry at {} waits on the book without it, and Hyperliquid didn't cancel it ({cancel}): \
                                         cancel the {} limit there now.", Fmt::price(px), market.name));
                }
                cancelled = true;
            }
            if let Ok(positions) = HyperliquidAccount::positions(evm).await {
                read = true;
                if let Some(open) = positions.into_iter().find(|o| o.coin == market.name && !o.size.is_zero()) { filled = Some((open.size.abs(), open.entry)) }
            }
            if filled.is_none() && !cancelled && attempt < 4 { sleep(600).await } else { break }
        }
        let Some((filled_size, avg_px)) = filled else {
            let Some(refused) = refused else { return fail(format!("The limit isn't among open orders: {reply}.")) };
            return fail(format!("Limit rejected: {refused}. {}", if cancelled {
                "Its entry, waiting on the book without it, was cancelled; no position.".to_string()
            } else if read {
                "No position.".into()
            } else {
                format!("The positions can't be read now: check {} on Hyperliquid, it may be open without a stop.", market.name)
            }));
        };
        // The price was already there: the group armed its stop and take at once.
        if unconfirmed.is_none() {
            if LIMIT_FILL_TEST.load(Relaxed) {
                unconfirmed = Some("limitFillTest".into());
            } else if let Err(e) = self.verify_protection(&market.name, stop, take).await {
                unconfirmed = Some(e.to_string());
            }
        }
        let mut opened = match unconfirmed {
            Some(why) => self.protect_filled_limit(market, long, stop, take, &why).await?,
            None => {
                self.report(&format!("The limit filled at once: {} {} at {}; stop and take-profit are on the exchange.", Fmt::num(filled_size, sz as u32),
                                     market.name, Fmt::price(avg_px)));
                Opened::new(&market.name, filled_size, avg_px, stop, take)
            }
        };
        // The half at 1R waited for a fill that has happened: it goes on now, as on a market entry.
        if p.intent.partial_take == Some(true) {
            opened.partial_take = self.partial_take_after_entry(market, long, opened.size, opened.entry, stop).await;
        }
        Ok(opened)
    }

    /// A limit that filled at once without its stop confirmed (`why`): the position read back decides. None open, its stop
    /// or take has closed it already. Open, the stop and take go on it again, at the prices the card showed from the limit.
    async fn protect_filled_limit(&self, market: &Market, long: bool, stop: Decimal, take: Option<Decimal>, why: &str) -> Result<Opened> {
        let Ok(positions) = HyperliquidAccount::positions(&self.wallet.evm).await else {
            return fail(format!("The limit filled at once, its stop isn't confirmed ({why}) and the positions can't be read now. Check {} on Hyperliquid: \
                                 it may be open without a stop.", market.name));
        };
        // prepare() refuses a plan on a coin with any position or order: what is open here is this limit.
        let Some(open) = positions.into_iter().find(|p| p.coin == market.name && !p.size.is_zero()) else {
            return fail(format!("The limit filled at once, and no {} position is open now: its stop or take-profit has already closed it.", market.name));
        };
        self.report(&format!("The limit filled at once, but the exchange didn't confirm its stop ({why}): placing it again."));
        // ponytail: the group's own stop and take may still show up late; then there are two of each, all reduce-only,
        // and «close» clears them.
        self.protect(market, long, open.size.abs(), stop, take).await?;
        Ok(Opened::new(&market.name, open.size.abs(), open.entry, stop, take))
    }

    // MARK: Close

    /// Closes the position at market. Its stop and take stay on the exchange until the position is flat: a close that
    /// doesn't fill, fills in part, loses its reply or isn't signed leaves what is open under its stop. What could open
    /// it again (a limit entry waiting on the book) goes first. Returns a line for the user.
    pub async fn close(&self, asset: &str) -> Result<String> {
        let market = self.market(asset).await?;
        let evm = &self.wallet.evm;
        // A TWAP still buying would open it again: it stops first. A close never waits on that read.
        let mut twap_unchecked = None;
        if let Err(e) = self.stop_twaps(&market).await {
            self.report(&format!("A running TWAP on {asset} couldn't be checked or stopped ({e}): closing anyway."));
            twap_unchecked = Some(e.to_string());
        }
        let sz = market.sz_decimals as u32;
        let orders: Vec<Order> = HyperliquidAccount::open_orders(evm).await?.into_iter().filter(|o| o.coin == asset).collect();
        let entries: Vec<Order> = orders.iter().filter(|o| !o.reduce_only).cloned().collect();
        if !entries.is_empty() {
            let reply = self.post(&action::cancel(&cancels(&market, &entries)), "cancel entries").await?;
            if reply.is_error() { return fail(format!("Hyperliquid didn't cancel the {asset} limit: {reply}. Nothing was closed.")) }
        }
        let Some(position) = HyperliquidAccount::positions(evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero()) else {
            let rest: Vec<Order> = orders.iter().filter(|o| !entries.contains(o)).cloned().collect();
            if !rest.is_empty() { self.post(&action::cancel(&cancels(&market, &rest)), "cancel orders").await?; }
            let line = if orders.is_empty() { format!("No open {asset} position.") } else { format!("{asset} orders cancelled ({}); there was no position.", orders.len()) };
            self.report(&line);
            return Ok(line);
        };
        let buy = position.size < Decimal::ZERO;
        let two = SLIPPAGE * Decimal::TWO;
        let px = price(num(Num::of(market.mark_px) * if buy { Decimal::ONE + two } else { Decimal::ONE - two })?, market.sz_decimals);
        let order = hyperliquid::Order {
            asset: market.index as u32, is_buy: buy, price: px, size: position.size.abs(), reduce_only: true, kind: OrderKind::Limit { tif: "Ioc".into() },
            cloid: None,
        };
        let result = match self.post(&action::order(&[order], "na")?, "close").await {
            Ok(r) => r,
            Err(refused @ Error::Guard(_)) => return Err(refused),   // not sent: the position and its stop are as they were
            Err(e) => HlReply::Error(e.to_string()),
        };
        // What is open now decides, not the reply. A fill of the whole is taken as it is.
        let whole = matches!(result, HlReply::Filled { size, .. } if size >= position.size.abs());
        let after = if whole { Some(vec![]) } else { HyperliquidAccount::positions(evm).await.ok() };
        let open = after.as_ref().and_then(|a| a.iter().find(|p| p.coin == asset && !p.size.is_zero()).cloned());
        if after.is_none() || open.is_some() {
            let stops: Vec<Decimal> = orders.iter().filter(|o| o.is_trigger && o.reduce_only && o.kind().contains("Stop")).filter_map(|o| o.trigger_px).collect();
            let kept = match if buy { stops.iter().min() } else { stops.iter().max() } {
                Some(on) => format!("Its stop at {} stays on the exchange.", Fmt::price(*on)),
                None => "It has no stop on the exchange.".into(),
            };
            let what = match &open {
                Some(open) if open.size.abs() < position.size.abs() => format!("{} of {} {asset} closed; {} is still open.",
                    Fmt::num(position.size.abs() - open.size.abs(), sz), Fmt::num(position.size.abs(), sz), Fmt::num(open.size.abs(), sz)),
                Some(_) => format!("The {asset} position is still open."),
                None => format!("The positions can't be read now: check on Hyperliquid whether {asset} closed."),
            };
            return fail(format!("The close didn't finish ({result}). {what} {kept}"));
        }
        let mut line = match result {
            HlReply::Filled { size, avg_px } => format!("{asset} position closed: {} at {}; PnL before the close {}.", Fmt::num(size, sz), Fmt::price(avg_px),
                                                        usd(position.unrealized_pnl)),
            _ => format!("{asset} position is closed (the close order: {result}): its stop or take-profit may have filled first."),
        };
        // Flat now: what is left of its stop and take goes, so a new plan on the asset isn't refused over them.
        if let Some(why) = twap_unchecked {
            line += &format!(" A TWAP on {asset} may still be running ({why}): its stop and take stay, «cancel the TWAP» stops it.");
            self.report(&line);
            return Ok(line);
        }
        let rest: Vec<Order> = match HyperliquidAccount::open_orders(evm).await {
            Ok(o) => o.into_iter().filter(|o| o.coin == asset).collect(),
            Err(_) => orders.iter().filter(|o| !entries.contains(o)).cloned().collect(),
        };
        if !rest.is_empty() {
            let reply = self.post(&action::cancel(&cancels(&market, &rest)), "cancel stop and take-profit").await.ok();
            if reply.as_ref().is_none_or(HlReply::is_error) {
                line += &format!(" Its leftover {asset} orders didn't cancel ({}): «close {asset}» again clears them.",
                                 reply.map_or("no answer".into(), |r| r.to_string()));
            }
        }
        self.report(&line);
        Ok(line)
    }

    /// sendAsset to our own address between perp balances — "" is the main one, "xyz" a HIP-3 exchange — then wait for the
    /// credit.
    pub async fn move_margin(&self, source: &str, dex: &str, usdc: Decimal) -> Result<()> {
        let evm = &self.wallet.evm;
        let free = || async move {
            Ok::<_, Error>(if dex.is_empty() { Hyperliquid::balance(evm).await?.withdrawable } else { Hyperliquid::dex_balance(evm, dex).await?.1 })
        };
        let name = |d: &str| if d.is_empty() { "the main balance".to_string() } else { d.to_string() };
        let before = free().await?;
        let n = self.nonce();
        let action = hyperliquid::signer::send_asset_action(evm, source, dex, "USDC:0x6d1e7cde53ba9467b783cb7c530ce054", &action::number(usdc)?, n, true);
        let signature = self.signer.sign_hyperliquid_user(action.clone(), vec![]).await?;
        self.record("hyperliquid", &n.to_string(), Kind::Signed, &format!("sendAsset {} → {} {}", name(source), name(dex), text(usdc)))?;
        let reply = HlReply::parse(&hl_exchange(&action, n, &signature).await?);
        self.record("hyperliquid", &n.to_string(), if reply.is_error() { Kind::Failed } else { Kind::Confirmed }, &reply.to_string())?;
        if reply.is_error() { return fail(format!("Transfer to {} rejected: {reply}.", name(dex))) }
        let target = num(Num::of(before) + Num::of(usdc) * d("0.99"))?;
        for _ in 0..20 {
            if free().await? >= target {
                self.report(&format!("Margin on {}: +{} USDC from {}.", name(dex), Fmt::num(usdc, 2), name(source)));
                return Ok(());
            }
            sleep(500).await;
        }
        fail(format!("Transfer to {} wasn't credited within 10 s.", name(dex)))
    }

    // MARK: Manage

    /// Carries out a command about an open position. Returns a line for the user. `policy` and `equity`: a stop moved
    /// wider than the per-trade rule is refused (None equity: not checked).
    pub async fn manage(&self, command: &PositionCommand, asset: &str, policy: &RiskPolicy, equity: Option<Decimal>) -> Result<String> {
        match command {
            PositionCommand::CancelOrders => self.cancel_entries(asset).await,
            PositionCommand::Close(f) if *f == Decimal::ONE => self.close(asset).await,
            PositionCommand::Close(f) => self.close_part(asset, *f).await,
            PositionCommand::StopToBreakeven => self.move_stop(asset, None, policy, equity).await,
            PositionCommand::StopTo(p) => self.move_stop(asset, Some(*p), policy, equity).await,
            PositionCommand::TakeTo(p) => self.move_take(asset, *p).await,
            PositionCommand::Trail(p) => self.trail(asset, *p).await,
        }
    }

    /// «cancel the limit»: only the entries waiting on the book go. A position, its stop and its take stay as they are.
    async fn cancel_entries(&self, asset: &str) -> Result<String> {
        let market = self.market(asset).await?;
        // A TWAP still building the position is an entry too: it stops by its id. One that won't read or stop doesn't keep
        // the limits from going: it's said after them.
        let (twaps, twap_problem) = match self.stop_twaps(&market).await {
            Ok(t) => (t, None),
            Err(e) => (vec![], Some(format!("The {asset} TWAP couldn't be stopped: {e}"))),
        };
        let entries: Vec<Order> = HyperliquidAccount::open_orders(&self.wallet.evm).await?.into_iter()
            .filter(|o| o.coin == asset && !o.is_trigger && !o.reduce_only).collect();
        if entries.is_empty() && (!twaps.is_empty() || twap_problem.is_some()) {
            if let Some(problem) = twap_problem { return fail(problem) }
            let line = format!("{asset} TWAP stopped ({}). What it bought stays open with its stop and take-profit.", twaps.len());
            self.report(&line);
            return Ok(line);
        }
        if let Some(problem) = &twap_problem { self.report(problem) }
        if entries.is_empty() {
            let line = format!("No {asset} limit is waiting on the book; nothing was cancelled.");
            self.report(&line);
            return Ok(line);
        }
        let reply = self.post(&action::cancel(&cancels(&market, &entries)), "cancel limit").await?;
        if reply.is_error() { return fail(format!("Hyperliquid didn't cancel the {asset} limit: {reply}.")) }
        let open = HyperliquidAccount::positions(&self.wallet.evm).await.ok().map(|p| p.iter().any(|p| p.coin == asset && !p.size.is_zero()));
        let line = format!("{asset} limit cancelled ({}).{}", entries.len(),
                           if open == Some(true) { " The open position keeps its stop and take-profit." } else { "" });
        self.report(&line);
        Ok(line)
    }

    /// The guardian's look at a TWAP entry each pass, read afresh. A stop or take that fired leaves the TWAP buying: it is
    /// stopped first, then what it bought with no stop is closed. A TWAP whose loss at its stop has grown past 1.5 × `risk`
    /// is stopped too. A TWAP that won't stop throws: the next pass tries again.
    pub async fn guard_twap(&self, asset: &str, risk: Option<Decimal>) -> Result<TwapWatch> {
        let market = self.market(asset).await?;
        let evm = &self.wallet.evm;
        let twap = HyperliquidAccount::running_twaps(evm, Some(asset)).await?.into_iter().next();
        let position = HyperliquidAccount::positions(evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero());
        let stops: Vec<Decimal> = HyperliquidAccount::open_orders(evm).await?.into_iter()
            .filter(|o| o.coin == asset && o.is_trigger && o.reduce_only && o.kind().contains("Stop") && !o.kind().contains("Trailing"))
            .filter_map(|o| o.trigger_px).collect();
        if position.is_some() && stops.is_empty() {
            self.stop_twaps(&market).await?;
            return Ok(TwapWatch::Stopped(format!("The {asset} TWAP kept buying after its stop: it's stopped. {}", self.close(asset).await?)));
        }
        let Some(twap) = twap else { return Ok(TwapWatch::Ended) };
        let Some(position) = position else {
            self.stop_twaps(&market).await?;
            return Ok(TwapWatch::Stopped(format!("The {asset} TWAP is stopped: its position's stop or take closed it.")));
        };
        let long = position.size > Decimal::ZERO;
        let stop = if long { stops.iter().max() } else { stops.iter().min() };
        if let (Some(risk), Some(&stop)) = (risk, stop) {
            let held = position.size.abs();
            let loss = Self::twap_loss_at_stop(held, position.entry, (Num::of(twap.size) - held).0.unwrap_or_default().max(Decimal::ZERO),
                                               market.mark_px, stop, long);
            let bound = (Num::of(risk) * d("1.5")).0.unwrap_or(Decimal::MAX);
            if loss > bound {
                self.stop_twaps(&market).await?;
                return Ok(TwapWatch::Stopped(format!("The {asset} TWAP is stopped: at its stop it would lose {}, more than 1.5 × the plan's {}. What it \
                                                      bought keeps its stop and take.", usd(loss), usd(risk))));
            }
        }
        Ok(TwapWatch::Running)
    }

    /// What a TWAP's position loses at `stop`: what it holds from its entry, and what it still buys from the price now.
    /// ponytail: a loss past range counts as the most there is, so the TWAP stops.
    pub fn twap_loss_at_stop(held: Decimal, entry: Decimal, remaining: Decimal, mark: Decimal, stop: Decimal, long: bool) -> Decimal {
        let dir = if long { Decimal::ONE } else { Decimal::NEGATIVE_ONE };
        let part = |from: Decimal| (Num::of(dir) * (Num::of(from) - stop)).0.map(|x| Num::of(x.max(Decimal::ZERO)));
        match (part(entry), part(mark)) {
            (Some(a), Some(b)) => (Num::of(held) * a + Num::of(remaining) * b).0.unwrap_or(Decimal::MAX),
            _ => Decimal::MAX,
        }
    }

    /// Every TWAP running on the market, stopped by its id. Returns their ids; one that won't stop throws.
    async fn stop_twaps(&self, market: &Market) -> Result<Vec<u64>> {
        let ids: Vec<u64> = HyperliquidAccount::running_twaps(&self.wallet.evm, Some(&market.name)).await?.into_iter().map(|t| t.id).collect();
        for &id in &ids {
            let reply = self.post(&action::twap_cancel(market.index as u32, id), &format!("cancel TWAP {id}")).await?;
            // Finished or stopped meanwhile is what was asked.
            if let HlReply::Error(e) = reply {
                if !e.contains("never placed, already canceled, or filled") {
                    return fail(format!("Hyperliquid didn't stop the {} TWAP: {e}. Nothing else was changed.", market.name));
                }
            }
        }
        Ok(ids)
    }

    /// New take first, then the old one is cancelled.
    async fn move_take(&self, asset: &str, target: Decimal) -> Result<String> {
        let market = self.market(asset).await?;
        let evm = &self.wallet.evm;
        let Some(position) = HyperliquidAccount::positions(evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero()) else {
            return Ok(format!("No open {asset} position."));
        };
        let long = position.size > Decimal::ZERO;
        let sz = market.sz_decimals;
        let take = price(target, sz);
        if !(if long { take > market.mark_px } else { take < market.mark_px }) {
            return fail(format!("Take-profit {} is {} the current price {}: it would trigger at once. To exit, type «close».", Fmt::price(take),
                                if long { "below" } else { "above" }, Fmt::price(market.mark_px)));
        }
        let old: Vec<Order> = HyperliquidAccount::open_orders(evm).await?.into_iter().filter(|o| o.coin == asset && o.is_trigger && o.kind().contains("Take")).collect();
        let worst = price(num(Num::of(take) * if long { Decimal::ONE - TRIGGER_SLIPPAGE } else { Decimal::ONE + TRIGGER_SLIPPAGE })?, sz);
        // A take for the whole position (size 0, as a TWAP's) stays one: a sized one would leave the TWAP's later parts out.
        let size = if old.iter().any(|o| o.size == Some(Decimal::ZERO)) { Decimal::ZERO } else { position.size.abs() };
        let order = hyperliquid::Order {
            asset: market.index as u32, is_buy: !long, price: worst, size, reduce_only: true,
            kind: OrderKind::Trigger { price: take, is_market: true, tpsl: "tp".into() }, cloid: None,
        };
        let placed = self.post(&action::order(&[order], "positionTpsl")?, "new take-profit").await?;
        if let HlReply::Error(e) = placed { return fail(format!("New take-profit rejected: {e}. The old one stays.")) }
        let stale: Vec<Order> = old.into_iter().filter(|o| o.trigger_px != Some(take)).collect();
        if !stale.is_empty() { self.post(&action::cancel(&cancels(&market, &stale)), "cancel old take-profit").await?; }
        let line = format!("Take-profit moved to {}.", Fmt::price(take));
        self.report(&line);
        Ok(line)
    }

    async fn close_part(&self, asset: &str, fraction: Decimal) -> Result<String> {
        let market = self.market(asset).await?;
        let Some(position) = HyperliquidAccount::positions(&self.wallet.evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero()) else {
            return Ok(format!("No open {asset} position."));
        };
        let sz = market.sz_decimals;
        let size = Self::partial_close_size(position.size, fraction, sz, asset)?;
        if size >= position.size.abs() { return self.close(asset).await }   // a share of 1 or more is the whole
        let buy = position.size < Decimal::ZERO;
        let two = SLIPPAGE * Decimal::TWO;
        let px = price(num(Num::of(market.mark_px) * if buy { Decimal::ONE + two } else { Decimal::ONE - two })?, sz);
        let order = hyperliquid::Order {
            asset: market.index as u32, is_buy: buy, price: px, size, reduce_only: true, kind: OrderKind::Limit { tif: "Ioc".into() }, cloid: None,
        };
        let result = self.post(&action::order(&[order], "na")?, "partial close").await?;
        let HlReply::Filled { size: filled, avg_px } = result else { return fail(format!("The partial close didn't fill: {result}.")) };
        let left = num(Num::of(position.size.abs()) - filled)?;
        let line = format!("Closed {} {asset} at {}; {} left, stop and take-profit in place.", Fmt::num(filled, sz as u32), Fmt::price(avg_px),
                           Fmt::num(left, sz as u32));
        self.report(&line);
        Ok(line)
    }

    /// New stop first, then the old one is cancelled: the position is never without a stop. `target` None = breakeven:
    /// the entry plus both taker fees, so a stop-out costs nothing.
    async fn move_stop(&self, asset: &str, target: Option<Decimal>, policy: &RiskPolicy, equity: Option<Decimal>) -> Result<String> {
        let market = self.market(asset).await?;
        let evm = &self.wallet.evm;
        let Some(position) = HyperliquidAccount::positions(evm).await?.into_iter().find(|p| p.coin == asset && !p.size.is_zero()) else {
            return Ok(format!("No open {asset} position."));
        };
        let long = position.size > Decimal::ZERO;
        if STOP_MOVE_FAIL_TEST.fetch_update(SeqCst, SeqCst, |n| n.checked_sub(1)).is_ok() { return fail("stopMoveFailTest: nothing sent.") }
        let sz = market.sz_decimals;
        let stop = price(target.unwrap_or_else(|| guardian::breakeven(position.entry, long)), sz);
        if !(if long { stop < market.mark_px } else { stop > market.mark_px }) {
            return fail(format!("Stop {} is {} the current price {}: it would trigger at once. To exit, type «close».", Fmt::price(stop),
                                if long { "above" } else { "below" }, Fmt::price(market.mark_px)));
        }
        // A trailing stop on the exchange («Trailing Stop Market») isn't this stop: it stays and moves by itself.
        let old: Vec<Order> = HyperliquidAccount::open_orders(evm).await?.into_iter()
            .filter(|o| o.coin == asset && o.is_trigger && o.kind().contains("Stop") && !o.kind().contains("Trailing")).collect();
        let triggers = old.iter().filter_map(|o| o.trigger_px);
        let current = if long { triggers.max() } else { triggers.min() };
        // A TWAP still buying puts the stop on the whole position it builds: the rule counts what it holds at its entry and
        // what it still buys at the price now.
        let held = position.size.abs();
        let twap = HyperliquidAccount::running_twaps(evm, Some(asset)).await?.first().map_or(Decimal::ZERO, |t| t.size);
        let rest = num(Num::of(twap) - held)?.max(Decimal::ZERO);
        let entry = num((Num::of(held) * position.entry + Num::of(rest) * market.mark_px) / (Num::of(held) + rest))?;
        if let (Some(loss), Some(equity)) = (Self::stop_breaks_rule(long, entry, held + rest, stop, current, equity, policy), equity) {
            let share = (Num::of(loss) / equity * 100).rounded(1, Rounding::Plain).0.map_or("NaN%".into(), Fmt::pct);
            return fail(format!("A stop at {} means −{}, {share} of capital. The rule: at most {}. Stop not moved.", Fmt::price(stop), usd(loss),
                                Fmt::pct(policy.max_risk_pct)));
        }
        // A stop for the whole position (size 0, as a TWAP's) stays one: a sized one would leave the TWAP's later parts out.
        let size = if old.iter().any(|o| o.size == Some(Decimal::ZERO)) { Decimal::ZERO } else { held };
        let orders = Self::protection_orders(market.index, long, size, stop, None, sz)?;
        let placed = self.post(&action::order(&orders, "positionTpsl")?, "new stop").await?;
        if let HlReply::Error(e) = placed { return fail(format!("New stop rejected: {e}. The old one stays.")) }
        self.verify_protection(asset, stop, None).await?;
        let stale: Vec<Order> = old.into_iter().filter(|o| o.trigger_px != Some(stop)).collect();
        if !stale.is_empty() { self.post(&action::cancel(&cancels(&market, &stale)), "cancel old stop").await?; }
        let line = if target.is_none() {
            format!("Stop at breakeven: {} (entry {} plus fees). The trade can no longer end in a loss, barring slippage.", Fmt::price(stop),
                    Fmt::price(position.entry))
        } else {
            format!("Stop moved to {}.", Fmt::price(stop))
        };
        self.report(&line);
        Ok(line)
    }

    /// The loss at a new stop when it is wider than the per-trade rule allows; None when it's within the rule, when it
    /// tightens the current stop (or there is none), or when the capital is unknown — the same rule as «stop N%».
    pub fn stop_breaks_rule(long: bool, entry: Decimal, size: Decimal, stop: Decimal, current: Option<Decimal>, equity: Option<Decimal>,
                            policy: &RiskPolicy) -> Option<Decimal> {
        let current = current?;
        if if long { stop >= current } else { stop <= current } { return None }
        let equity = equity.filter(|e| *e > Decimal::ZERO)?;
        let gap = (if long { Num::of(entry) - stop } else { Num::of(stop) - entry }).0?.max(Decimal::ZERO);
        let loss = (Num::of(gap) * size + Num::of(size) * (Num::of(entry) + stop) * RiskCalc::trade_fee()).0?;
        ((Num::of(loss) / equity * 100).0? > policy.max_risk_pct).then_some(loss)
    }

    /// The size a partial close sends: the share in whole steps of the market's szDecimals, rounded down. A share smaller
    /// than one step is refused: it used to fall through to close, which closed the whole position and its stop.
    pub fn partial_close_size(position: Decimal, fraction: Decimal, sz_decimals: i64, asset: &str) -> Result<Decimal> {
        // A third is 0.333…3: a third of 3 steps came to 0.99…9 steps and rounded down to none.
        let wanted = num((Num::of(position.abs()) * fraction).rounded(20, Rounding::Plain))?;
        let size = action::size(wanted, sz_decimals as i32);
        if size <= Decimal::ZERO {
            let places = sz_decimals as u32;
            let step = Decimal::ONE / Decimal::from_i128_with_scale(10i128.pow(places), 0);
            return fail(format!("Closing {} of {} {asset} is {} {asset}, less than the smallest step, {} {asset}. Nothing was closed; the stop and \
                                 take-profit stay. Close a bigger share, or type «close» for the whole position.",
                                (Num::of(fraction) * 100).0.map_or("NaN%".into(), Fmt::pct), Fmt::num(position.abs(), places),
                                Fmt::num(wanted, 6.max(places + 3)), Fmt::num(step, places)));
        }
        Ok(size)
    }

    pub(crate) async fn market(&self, asset: &str) -> Result<Market> {
        match hl_markets().await?.remove(asset) {
            Some(m) if m.index >= 0 => Ok(m),
            _ => fail(format!("No {asset} market.")),
        }
    }

    // MARK: Hyperliquid

    pub(crate) fn nonce(&self) -> u64 {
        let now = Utc::now().timestamp_millis().max(0) as u64;
        let mut s = self.state();
        s.last_nonce = now.max(s.last_nonce + 1);
        s.last_nonce
    }

    /// Signs, journals before sending, sends, journals the reply. An order carries ihatecrypto's fee once the account has
    /// approved it; a refusal over the fee never stops an order: it goes again without.
    pub(crate) async fn post(&self, action: &MsgPack, label: &str) -> Result<HlReply> {
        if action::kind(action) == Some("order") && self.allowed_fee().await.unwrap_or(0) >= builder::FEE {
            let reply = self.send(&action::with_builder(action), label).await?;
            match &reply {
                HlReply::Error(e) if e.to_lowercase().contains("builder") => self.state().allowed = Some(0),
                _ => return Ok(reply),
            }
        }
        self.send(action, label).await
    }

    /// Everything after the `signed` line that throws is a NoReply, so a caller tells «may have gone» from a stop before it
    /// went out.
    async fn send(&self, action: &MsgPack, label: &str) -> Result<HlReply> {
        let nonce = self.nonce();
        let signature = self.signer.sign_hyperliquid(action.clone(), nonce).await?;
        self.record("hyperliquid", &nonce.to_string(), Kind::Signed, label)?;
        async {
            let reply = HlReply::parse(&hl_exchange(action, nonce, &signature).await?);
            self.record("hyperliquid", &nonce.to_string(), if reply.is_error() { Kind::Failed } else { Kind::Confirmed }, &reply.to_string())?;
            Ok(reply)
        }.await.map_err(|e| Error::NoReply(Box::new(e)))
    }

    // MARK: ihatecrypto's fee and referral code

    /// What this account lets ihatecrypto take, in tenths of a basis point; None when Hyperliquid didn't answer. (Swift's
    /// static `allowedFee(_:)`.)
    pub async fn allowed_fee_of(user: &str) -> Option<u64> {
        let fee = Self::hl_info(json!({"type": "maxBuilderFee", "user": user.to_lowercase(), "builder": builder::ADDRESS})).await.ok()?;
        fee.as_u64().or_else(|| fee.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
    }

    pub(crate) async fn allowed_fee(&self) -> Option<u64> {
        if !self.state().allowed_read {
            let fee = Self::allowed_fee_of(&self.wallet.evm).await;
            let mut s = self.state();
            (s.allowed, s.allowed_read) = (fee, true);
        }
        self.state().allowed
    }

    /// Before the first order of a move the user confirmed: ihatecrypto's fee approved once (turned down, the move stops),
    /// and the referral code bound when the account has none. A fee Hyperliquid won't take, or a signer that can't sign
    /// it, leaves the trade without the fee.
    pub(crate) async fn prepare_fee(&self) -> Result<()> {
        if self.ask_fee && self.allowed_fee().await.is_some_and(|f| f < builder::FEE) {
            if let Err(stop @ Error::Guard(_)) = self.set_fee_approval(true).await { return Err(stop) }
        }
        self.bind_referral().await;
        Ok(())
    }

    /// Approves ihatecrypto's fee for this account, or takes the approval back (`on` false).
    pub async fn set_fee_approval(&self, on: bool) -> Result<()> {
        let n = self.nonce();
        let action = builder::approve_action(if on { builder::RATE } else { builder::REVOKE }, n, true);
        let note = if on { format!("approve ihatecrypto's fee {}", builder::RATE) } else { "take back ihatecrypto's fee".into() };
        let signature = self.signer.sign_hyperliquid_user(action.clone(), vec![]).await?;
        self.record("hyperliquid", &n.to_string(), Kind::Signed, &note)?;
        let reply = HlReply::parse(&hl_exchange(&action, n, &signature).await?);
        self.record("hyperliquid", &n.to_string(), if reply.is_error() { Kind::Failed } else { Kind::Confirmed }, &format!("{note}: {reply}"))?;
        if reply.is_error() { return fail(format!("Hyperliquid didn't take it: {reply}.")) }
        {
            let mut s = self.state();
            (s.allowed, s.allowed_read) = (Some(if on { builder::FEE } else { 0 }), true);
        }
        if on { self.report(&format!("ihatecrypto's fee approved: {} of each Hyperliquid trade, on top of the exchange's own.", builder::RATE)) }
        Ok(())
    }

    /// Binds ihatecrypto's referral code to an account that has none, signed like an order. Once per account per run of the
    /// app; nothing in it stops a trade.
    pub(crate) async fn bind_referral(&self) {
        let user = self.wallet.evm.to_lowercase();
        if !REFERRAL_LOOKED.lock().unwrap_or_else(|e| e.into_inner()).insert(user.clone()) { return }
        let Ok(state) = Self::hl_info(json!({"type": "referral", "user": user})).await else { return };
        if !matches!(state.as_object().and_then(|s| s.get("referredBy")), Some(Value::Null)) { return }
        match self.post(&builder::referrer_action(), &format!("referral code {}", builder::REFERRAL_CODE)).await {
            Ok(reply) if !reply.is_error() => self.report(&format!("Referral code {} on: 4% off your Hyperliquid fees.", builder::REFERRAL_CODE)),
            _ => {}
        }
    }

    async fn verify_protection(&self, coin: &str, stop: Decimal, take: Option<Decimal>) -> Result<()> {
        // The exchange lists new trigger orders within a moment; ask a few times.
        for attempt in 0..5 {
            let prices: HashSet<Decimal> = HyperliquidAccount::open_orders(&self.wallet.evm).await?.into_iter()
                .filter(|o| o.coin == coin && o.is_trigger).filter_map(|o| o.trigger_px).collect();
            if prices.contains(&stop) && take.is_none_or(|t| prices.contains(&t)) { return Ok(()) }
            if attempt < 4 { sleep(600).await }
        }
        fail("the stop isn't among open orders")
    }

    /// Sends a signed Solana transaction already journaled as signed, and waits for its outcome. A node that refuses it
    /// at preflight hasn't sent it, so it's waited out to its last valid block like any other: one that never lands is
    /// «expired», a move that failed with nothing sent.
    pub(crate) async fn broadcast_solana(&self, signed: &[u8], signature: &str, last_valid: u64) -> Result<Outcome> {
        match self.rpc.send(signed).await {
            Ok(_) => {}
            Err(e) if e.0.contains("simulation failed") => {
                self.report("Solana refused the transaction at its check before sending (the route moved): making sure it didn't land.")
            }
            Err(e) => return Err(e.into()),
        }
        Ok(self.rpc.wait_for_confirmation(signature, last_valid, Duration::from_secs(120)).await?)
    }

    pub(crate) fn leverage_action(asset: i64, leverage: i64) -> MsgPack {
        MsgPack::map(vec![("type", MsgPack::str("updateLeverage")), ("asset", MsgPack::Uint(asset.max(0) as u64)), ("isCross", MsgPack::Bool(false)),
                          ("leverage", MsgPack::Uint(leverage.max(0) as u64))])
    }

    pub(crate) fn protection_prices(entry: Decimal, stop_pct: Decimal, take_pct: Option<Decimal>, long: bool, sz_decimals: i64)
        -> Result<(Decimal, Option<Decimal>)> {
        let dir = Num::of(if long { Decimal::ONE } else { Decimal::NEGATIVE_ONE });
        let stop = price(num(Num::of(entry) * (Num::of(Decimal::ONE) - dir * stop_pct / 100))?, sz_decimals);
        let take = match take_pct {
            Some(t) => Some(price(num(Num::of(entry) * (Num::of(Decimal::ONE) + dir * t / 100))?, sz_decimals)),
            None => None,
        };
        Ok((stop, take))
    }

    /// Stop and take as one positionTpsl group: reduce-only market triggers on the opposite side.
    pub(crate) fn protection_action(asset: i64, long: bool, size: Decimal, stop: Decimal, take: Option<Decimal>, sz_decimals: i64) -> Result<MsgPack> {
        Ok(action::order(&Self::protection_orders(asset, long, size, stop, take, sz_decimals)?, "positionTpsl")?)
    }

    pub(crate) fn protection_orders(asset: i64, long: bool, size: Decimal, stop: Decimal, take: Option<Decimal>, sz_decimals: i64)
        -> Result<Vec<hyperliquid::Order>> {
        let order = |trigger: Decimal, tpsl: &str| -> Result<hyperliquid::Order> {
            let worst = price(num(Num::of(trigger) * if long { Decimal::ONE - TRIGGER_SLIPPAGE } else { Decimal::ONE + TRIGGER_SLIPPAGE })?, sz_decimals);
            Ok(hyperliquid::Order {
                asset: asset as u32, is_buy: !long, price: worst, size, reduce_only: true,
                kind: OrderKind::Trigger { price: trigger, is_market: true, tpsl: tpsl.into() }, cloid: None,
            })
        };
        let mut orders = vec![order(stop, "sl")?];
        if let Some(take) = take { orders.push(order(take, "tp")?) }
        Ok(orders)
    }

    // MARK: Bridge (Relay, Solana USDC → Hyperliquid perp or USDC on an EVM chain)

    /// Relay's quote for a move that's about to run, its order handed to the signer: a deposit naming that order signs only
    /// once the signer has checked whom it pays. (Swift's thirteen parameters are relay::Ask's fields.)
    pub(crate) async fn relay_quote(&self, ask: &relay::Ask) -> Result<relay::Quote> {
        let quote = relay::quote(ask).await?;
        if let Some(order) = &quote.order { self.signer.register_relay_order(order.clone()).await? }
        Ok(quote)
    }

    /// The bridge transaction and the balance lines its simulation gives. `decimals` are the destination's;
    /// `topup_gas_usd` adds that much native coin for gas on an EVM destination. `swap`: the destination is another token
    /// (Relay swaps on the way), so the cost is judged by USD in against USD out.
    pub(crate) async fn prepare_bridge(&self, amount: Amount, chain: u64, currency: &str, decimals: u32, topup_gas_usd: Option<Decimal>, swap: bool)
        -> Result<(BridgeStep, Vec<String>)> {
        let usdc_mint = relay::currency::SOLANA_USDC;
        let (sol, evm) = (self.wallet.solana.clone(), self.wallet.evm.clone());
        let (units, exact_output) = match amount {
            Amount::Arrive(v) => (uint64(num((Num::of(v) * Decimal::from_i128_with_scale(10i128.pow(decimals.min(28)), 0)).rounded(0, Rounding::Up))?), true),
            Amount::Send(v) => (uint64(num((Num::of(v) * 1_000_000).rounded(0, Rounding::Down))?), false),
        };
        let quote = self.relay_quote(&relay::Ask {
            user: sol.clone(), recipient: evm.clone(), origin_chain: relay::chain::SOLANA, destination_chain: chain, origin_currency: usdc_mint.into(),
            destination_currency: currency.into(), amount: units, exact_output, topup_gas_usd, ..Default::default()
        }).await?;
        let binding = guard::binding_check(&pairs(&[("sender", &quote.sender), ("recipient", &quote.recipient)]),
                                           &pairs(&[("sender", &sol), ("recipient", &evm)]), "Relay quote");
        if !binding.ok() { return stop(binding.problems.join(" ")) }
        let Some(spend_units) = quote.amount_in.parse::<u64>().ok().filter(|&u| u > 0) else { return fail("Relay didn't give the amount to send.") };
        let spend = millionths(spend_units);
        let arriving = decimal(&quote.amount_out_formatted).unwrap_or_default();
        // Hyperliquid charges once for opening an account and Relay passes it on as its gas. It's allowed on top of the cap
        // only while Hyperliquid says the account doesn't exist, and only up to $1.50.
        let mut opening = Decimal::ZERO;
        if chain == relay::chain::HYPERLIQUID && Hyperliquid::is_new_account(&evm).await.ok() == Some(true) {
            opening = decimal(&quote.relayer_gas_usd).unwrap_or_default().min(d("1.5"));
        }
        // Bridge costs are capped: over what arrives (and the gas asked for) at most 3%, never more than $1; small bridges
        // may cost up to $0.15, Relay's floor for delivering gas.
        let worth = if swap { decimal(&quote.amount_out_usd).unwrap_or_default() } else { arriving };
        let lost = num(Num::of(spend) - worth - topup_gas_usd.unwrap_or_default() - opening)?;
        Self::check_loss(lost, spend, num(Num::of(worth) * d("0.03"))?.max(d("0.15")).min(Decimal::ONE), if swap { "Relay's swap" } else { "Relay" })?;
        let have = self.rpc.token_balance(&sol, usdc_mint).await?;
        if have < spend_units {
            return fail(format!("{} USDC on Solana; the bridge needs {}.", Fmt::num(millionths(have), 2), Fmt::num(spend, 2)));
        }

        let (blockhash, last_valid) = self.rpc.latest_blockhash().await?;
        let unsigned = solana::unsigned_transaction(&solana::compile_message(&quote.solana_instructions, &sol, &blockhash, 20_000)?);
        let message = Message::parse_transaction(&unsigned)?;
        let my_usdc = solana::spl::associated_account(&sol, usdc_mint, solana::spl::TOKEN_PROGRAM)?;
        let owners: BTreeMap<String, String> = [(my_usdc.clone(), sol.clone())].into();
        // Checked here for a readable reason and for the preview; the signer checks again on its own.
        let mut verdict = guard::sol::check(&message, &message.static_keys, &guard::sol::Policy::new(&sol), &owners.clone().into_iter().collect());
        let sol_before = self.rpc.balance(&sol).await?;
        let simulation = self.rpc.simulate(&unsigned, &[my_usdc.clone(), sol.clone()], false).await?;
        // A failed simulation says nothing about balances: stop here rather than quote made-up changes.
        if let Some(error) = simulation.error { return fail(format!("The simulation failed, so nothing was signed: {error}")) }
        let usdc_after = simulation.after.get(&my_usdc).and_then(|s| s.1).unwrap_or(0);
        let sol_after = simulation.after.get(&sol).map_or(0, |s| s.0);
        let i64_of = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        verdict.merge(guard::sol::balances(&[
            guard::sol::BalanceRule { label: "USDC".into(), before: i64_of(have), after: i64_of(usdc_after), at_least: -i64_of(spend_units), at_most: None },
            guard::sol::BalanceRule { label: "lamports".into(), before: i64_of(sol_before), after: i64_of(sol_after), at_least: -5_000_000, at_most: None },
        ]));
        if !verdict.ok() { return stop(format!("The guard stopped the bridge: {}", verdict.problems.join(" "))) }
        let usdc = |u: u64| Fmt::num(millionths(u), 6);
        let lamports = |l: u64| Fmt::num(Decimal::from(l) / Decimal::from(1_000_000_000u64), 6);
        let mut changes = vec![
            format!("Solana USDC: {} → {} (−{}), simulated.", usdc(have), usdc(usdc_after), usdc(have - have.min(usdc_after))),
            format!("Solana SOL: {} → {} (network fee), simulated.", lamports(sol_before), lamports(sol_after)),
        ];
        if let Some(gas) = decimal(&quote.gas_topup).filter(|g| *g > Decimal::ZERO) {
            let coin = EvmNetwork::all().iter().find(|n| n.relay_chain == chain).map_or("ETH", |n| n.gas_coin);
            changes.push(format!("Gas for the destination: +{} {coin} arrives with the USDC.", Fmt::num(gas, 6)));
        }
        let step = BridgeStep {
            request_id: quote.request_id, unsigned, accounts: message.static_keys, token_owners: owners, last_valid, spend, arriving,
            fees_usd: quote.fees_usd, chain, opening, note: None,
        };
        Ok((step, changes))
    }

    /// Returns the Solana transaction's signature.
    pub(crate) async fn send_bridge(&self, b: &BridgeStep) -> Result<String> {
        let signed = self.signer.sign_solana(b.unsigned.clone(), b.accounts.clone(), vec![], b.token_owners.clone()).await?;
        if signed.len() != b.unsigned.len() || signed.len() < 65 || signed[65..] != b.unsigned[65..] { return stop("What was signed isn't what was checked.") }
        let signature = base58::encode(&signed[1..65]);
        let evm = &self.wallet.evm;
        let to_hyperliquid = b.chain == relay::chain::HYPERLIQUID;
        let hl_before = if to_hyperliquid { Hyperliquid::balance(evm).await?.withdrawable } else { Decimal::ZERO };
        let to = if to_hyperliquid { "Hyperliquid".to_string() } else {
            EvmNetwork::all().iter().find(|n| n.relay_chain == b.chain).map_or_else(|| format!("chain {}", b.chain), |n| n.name.to_string())
        };
        self.record("relay", &b.request_id, Kind::Signed, &format!("{}, tx {signature}", b.note.clone().unwrap_or_else(|| format!("Solana USDC→{to}"))))?;
        self.record("solana", &signature, Kind::Signed, &format!("Relay deposit {}; valid until block {}", b.request_id, b.last_valid))?;
        let started = Instant::now();
        match self.broadcast_solana(&signed, &signature, b.last_valid).await? {
            Outcome::Confirmed => self.record("solana", &signature, Kind::Confirmed, "")?,
            // Relay's order ends with its deposit: left «signed», the app would take the move for one still on its way.
            Outcome::Failed(e) => {
                self.record("solana", &signature, Kind::Failed, &e)?;
                self.record("relay", &b.request_id, Kind::Failed, "its Solana deposit failed")?;
                return fail(format!("The bridge transaction failed: {e}. The money stayed on Solana."));
            }
            Outcome::Expired => {
                self.record("solana", &signature, Kind::Expired, "")?;
                self.record("relay", &b.request_id, Kind::Expired, "")?;
                return fail("The bridge transaction didn't make it into a block. The money stayed on Solana; you can retry.");
            }
        }
        while started.elapsed() < Duration::from_secs(180) {
            let status = relay::status(&b.request_id).await?.0;
            if status == "success" {
                self.record("relay", &b.request_id, Kind::Settled, &status)?;
                break;
            }
            if ["refund", "failure"].contains(&status.as_str()) {
                self.record("relay", &b.request_id, Kind::Failed, &status)?;
                return fail(format!("Relay returned {status}. Check your Solana balance."));
            }
            sleep(1000).await;
        }
        // An EVM destination: the caller waits for the tokens on its own chain.
        if !to_hyperliquid { return Ok(signature) }
        // Wait for the credit itself: the trade needs it on the balance, not just in Relay's status.
        let target = num(Num::of(hl_before) + Num::of(b.arriving) * d("0.99"))?;
        while started.elapsed() < Duration::from_secs(240) {
            if Hyperliquid::balance(evm).await?.withdrawable >= target {
                self.report(&format!("Bridge done in {} s: {} USDC on Hyperliquid. https://solscan.io/tx/{signature}", started.elapsed().as_secs(),
                                     Fmt::num(b.arriving, 2)));
                return Ok(signature);
            }
            sleep(1000).await;
        }
        fail(format!("Relay didn't credit the USDC within 4 minutes. Order {} is in the journal; the trade wasn't opened.", b.request_id))
    }
}

fn pairs(p: &[(&str, &str)]) -> HashMap<String, String> { p.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }

/// The orders to cancel on a market, by (asset, oid).
fn cancels(market: &Market, orders: &[Order]) -> Vec<(u32, u64)> { orders.iter().map(|o| (market.index as u32, o.oid)).collect() }

// MARK: Hyperliquid's API

/// A reqwest failure in the words Swift's URLError gives the two common ones.
pub(crate) fn url_error(e: reqwest::Error) -> Error {
    Error::Wallet(if e.is_timeout() { "The request timed out.".into() } else if e.is_connect() { "Could not connect to the server.".into() } else { e.to_string() })
}

/// POSTs `body` to Hyperliquid's `path` (20 s) and gives the body, whatever the status.
pub(crate) async fn hl_post(path: &str, body: &Value) -> Result<Vec<u8>> {
    let response = net::HTTP.post(hl_url(path)).header("Content-Type", "application/json").body(body.to_string())
        .timeout(Duration::from_secs(20)).send().await.map_err(url_error)?;
    Ok(response.bytes().await.map_err(url_error)?.to_vec())
}

/// HLExchange.send on mainnet: the action, its nonce and signature to /exchange.
pub(crate) async fn hl_exchange(action: &MsgPack, nonce: u64, signature: &EvmSignature) -> Result<Vec<u8>> {
    hl_post("/exchange", &hyperliquid::exchange_body(action, nonce, signature, None)).await
}

/// `Hyperliquid.markets()` read through `hl_url`, each answer read by core's own readers: the main exchange's markets
/// and the HIP-3 ones ihatecrypto shows.
pub(crate) async fn hl_markets() -> Result<HashMap<String, Market>> {
    async fn on(dex: Option<&str>, index: i64) -> Result<HashMap<String, Market>> {
        let mut body = json!({"type": "metaAndAssetCtxs"});
        if let Some(dex) = dex { body["dex"] = json!(dex) }
        Ok(Hyperliquid::parse_markets(&hl_post("/info", &body).await?, dex, index)?)
    }
    let dexes_body = json!({"type": "perpDexs"});
    let (main, order) = tokio::join!(on(None, 0), hl_post("/info", &dexes_body));
    let mut out = main?;
    // Asset ids of HIP-3 markets depend on the exchange's position in perpDexs.
    let dexes = order.map(|d| Hyperliquid::dex_names(&d)).unwrap_or_default();
    for dex in Hyperliquid::HIP3_DEXES {
        let Some(i) = dexes.iter().position(|d| d.as_deref() == Some(dex)) else { continue };
        let Ok(extra) = on(Some(dex), i as i64).await else { continue };
        for (name, m) in extra { out.entry(name).or_insert(m); }
    }
    Ok(out)
}

// MARK: - Replies and account state

/// What /exchange answered for one action (the first order status for order actions).
#[derive(Debug, Clone, PartialEq)]
pub enum HlReply {
    Ok,
    Filled { size: Decimal, avg_px: Decimal },
    Resting,
    /// A TWAP the exchange runs now, by its id.
    Running(u64),
    Error(String),
}

impl std::fmt::Display for HlReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HlReply::Ok => f.write_str("ok"),
            HlReply::Filled { size, avg_px } => write!(f, "filled {} at {}", text(*size), text(*avg_px)),
            HlReply::Resting => f.write_str("resting on the book"),
            HlReply::Running(id) => write!(f, "TWAP {id} running"),
            HlReply::Error(e) => f.write_str(e),
        }
    }
}

/// `"\(x)"` of a JSON value: a string as is, anything else as JSON.
// ponytail: Swift prints a dictionary's description («["key": …]»); JSON here, the same words inside.
fn describe(v: &Value) -> String { v.as_str().map_or_else(|| v.to_string(), str::to_string) }

fn number_u64(v: &Value) -> Option<u64> { v.as_u64().or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)) }

impl HlReply {
    pub fn is_error(&self) -> bool { matches!(self, HlReply::Error(_)) }

    pub fn parse(data: &[u8]) -> Self {
        let Ok(Value::Object(j)) = serde_json::from_slice::<Value>(data) else {
            return HlReply::Error(String::from_utf8_lossy(&data[..data.len().min(200)]).into_owned());
        };
        if j.get("status").and_then(Value::as_str) != Some("ok") {
            return HlReply::Error(j.get("response").map_or_else(|| Value::Object(j.clone()).to_string(), describe));
        }
        let data = j.get("response").and_then(|r| r.get("data")).filter(|d| d.is_object());
        // One status, not a list: a TWAP's or a trailing stop's.
        let status = data.and_then(|d| d.get("status")).filter(|s| s.is_object());
        if let Some(e) = status.and_then(|s| s.get("error")).and_then(Value::as_str) { return HlReply::Error(e.into()) }
        if let Some(id) = status.and_then(|s| s.get("running")).filter(|r| r.is_object()).and_then(|r| r.get("twapId")).and_then(number_u64) {
            return HlReply::Running(id);
        }
        let statuses = data.and_then(|d| d.get("statuses")).and_then(Value::as_array).cloned().unwrap_or_default();
        // Several statuses (stop and take): any error wins, otherwise the first one describes the action.
        if let Some(e) = statuses.iter().filter_map(|s| s.as_object()?.get("error")?.as_str()).next() { return HlReply::Error(e.into()) }
        let Some(first) = statuses.first() else { return HlReply::Ok };
        if let Some(f) = first.get("filled").filter(|f| f.is_object()) {
            if let (Some(size), Some(px)) = (f.get("totalSz").and_then(Value::as_str).and_then(decimal), f.get("avgPx").and_then(Value::as_str).and_then(decimal)) {
                return HlReply::Filled { size, avg_px: px };
            }
        }
        if first.as_object().is_some_and(|o| o.contains_key("resting")) { return HlReply::Resting }
        HlReply::Ok
    }
}

/// An open order as frontendOpenOrders lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub coin: String,
    pub oid: u64,
    pub is_trigger: bool,
    pub trigger_px: Option<Decimal>,
    pub limit_px: Option<Decimal>,
    pub size: Option<Decimal>,
    /// "B" buy, "A" sell.
    pub side: Option<String>,
    /// "Limit", "Stop Market", "Take Profit Market"…
    pub order_type: Option<String>,
    /// Only shrinks a position (a stop, a take, a close), never opens one.
    pub reduce_only: bool,
    /// A resting entry's own stop and take (normalTpsl): armed when it fills.
    pub children: Vec<Order>,
}

/// A field JSONDecoder's `decodeIfPresent` reads: Some(None) when absent or null, None when it's another type.
fn present<'a, T>(o: &'a Map<String, Value>, key: &str, read: impl Fn(&'a Value) -> Option<T>) -> Option<Option<T>> {
    match o.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(v) => read(v).map(Some),
    }
}

impl Order {
    /// `orderType ?? ""`.
    pub(crate) fn kind(&self) -> &str { self.order_type.as_deref().unwrap_or("") }

    /// One coin's open orders while its limit is only partly filled: the rest waits on the book, and the stop with it as
    /// a child, armed on the full fill (Hyperliquid's normalTpsl).
    pub fn fills_in_part(orders: &[Order]) -> bool {
        orders.iter().any(|o| !o.is_trigger && !o.reduce_only) && !orders.iter().any(|o| o.is_trigger && o.kind().contains("Stop"))
    }

    pub fn read(v: &Value) -> Option<Order> {
        let o = v.as_object()?;
        let text = |k: &str| present(o, k, |v| v.as_str().map(str::to_string));
        Some(Order {
            coin: o.get("coin")?.as_str()?.into(),
            oid: o.get("oid")?.as_u64()?,
            is_trigger: present(o, "isTrigger", Value::as_bool)?.unwrap_or(false),
            trigger_px: text("triggerPx")?.and_then(|s| decimal(&s)),
            limit_px: text("limitPx")?.and_then(|s| decimal(&s)),
            size: text("sz")?.and_then(|s| decimal(&s)),
            side: text("side")?,
            order_type: text("orderType")?,
            reduce_only: present(o, "reduceOnly", Value::as_bool)?.unwrap_or(false),
            // Read for the risk sum only: a child in an unexpected shape must not hide the open orders.
            children: o.get("children").and_then(Value::as_array).and_then(|a| a.iter().map(Order::read).collect()).unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub coin: String,
    /// Signed: negative for a short.
    pub size: Decimal,
    pub entry: Decimal,
    pub unrealized_pnl: Decimal,
    pub liquidation: Option<Decimal>,
}

/// A TWAP the exchange still runs: its id, coin, whole size and side.
#[derive(Debug, Clone, PartialEq)]
pub struct Twap {
    pub id: u64,
    pub coin: String,
    pub size: Decimal,
    pub is_buy: bool,
}

pub struct HyperliquidAccount;

fn unreadable() -> Error { Error::Wallet(UNREADABLE.into()) }

impl HyperliquidAccount {
    /// What the open positions and the resting limit entries lose if every stop fills; None when one of them has no stop.
    /// `spot`: coins the wallet holds by market: a short of them loses at its stop what they gain, so only the part of a
    /// short beyond them counts.
    pub fn risk_at_stops(positions: &[Position], orders: &[Order], spot: &HashMap<String, Decimal>) -> Option<Decimal> {
        let stops = |coin: &str, own: &[Order]| -> Vec<Decimal> {
            own.iter().chain(orders.iter().filter(|o| o.coin == coin)).filter(|o| o.is_trigger && o.kind().contains("Stop")).filter_map(|o| o.trigger_px).collect()
        };
        let mut total = Num::of(Decimal::ZERO);
        for p in positions.iter().filter(|p| !p.size.is_zero()) {
            // The stop that fills first is the one nearest the entry on the losing side.
            let s = stops(&p.coin, &[]);
            let stop = *(if p.size > Decimal::ZERO { s.iter().max() } else { s.iter().min() })?;
            let covered = if p.size < Decimal::ZERO { p.size.abs().min(spot.get(&p.coin).copied().unwrap_or_default()) } else { Decimal::ZERO };
            let gap = if p.size > Decimal::ZERO { Num::of(p.entry) - stop } else { Num::of(stop) - p.entry };
            total = total + (gap * (Num::of(p.size.abs()) - covered)).0.map_or(Num(None), |x| Num::of(x.max(Decimal::ZERO)));
        }
        // A limit entry still waiting loses as much once it fills: at its own stop (armed with it) or the coin's.
        for o in orders.iter().filter(|o| !o.is_trigger && !o.reduce_only) {
            let (Some(px), Some(size)) = (o.limit_px, o.size.filter(|s| *s > Decimal::ZERO)) else { continue };
            let buy = o.side.as_deref() == Some("B");
            let s = stops(&o.coin, &o.children);
            let stop = *(if buy { s.iter().max() } else { s.iter().min() })?;
            let gap = if buy { Num::of(px) - stop } else { Num::of(stop) - px };
            total = total + (gap * size).0.map_or(Num(None), |x| Num::of(x.max(Decimal::ZERO)));
        }
        total.0
    }

    /// What the parts running TWAPs haven't bought yet lose at their coin's stop, bought at today's price: the open
    /// positions count what they hold, this the rest. None when a TWAP's coin has no stop or no price.
    // ponytail: the part not bought yet is the TWAP's size less the position on its coin; a position from before it hides part of it.
    pub fn twap_risk(twaps: &[Twap], positions: &[Position], orders: &[Order], marks: &HashMap<String, Decimal>) -> Option<Decimal> {
        let mut total = Num::of(Decimal::ZERO);
        for t in twaps {
            let held = positions.iter().find(|p| p.coin == t.coin).map_or(Decimal::ZERO, |p| p.size.abs());
            let rest = (Num::of(t.size) - held).0?.max(Decimal::ZERO);
            if rest <= Decimal::ZERO { continue }
            let stops: Vec<Decimal> = orders.iter().filter(|o| o.coin == t.coin && o.is_trigger && o.kind().contains("Stop")).filter_map(|o| o.trigger_px).collect();
            let mark = *marks.get(&t.coin)?;
            let stop = *(if t.is_buy { stops.iter().max() } else { stops.iter().min() })?;
            let gap = if t.is_buy { Num::of(mark) - stop } else { Num::of(stop) - mark };
            total = total + gap.0.map_or(Num(None), |g| Num::of(g.max(Decimal::ZERO)) * rest);
        }
        total.0
    }

    /// What the user has in a vault: its equity and what can be withdrawn now (lockups hold the rest).
    pub async fn vault_equity(user: &str, vault: &str) -> Result<(Decimal, Decimal, Option<DateTime<Utc>>)> {
        let rows: Vec<Value> = serde_json::from_slice(&Self::info(json!({"type": "userVaultEquities", "user": user})).await?).map_err(|_| unreadable())?;
        let mut found = None;
        for r in &rows {
            let o = r.as_object().ok_or_else(unreadable)?;
            let address = o.get("vaultAddress").and_then(Value::as_str).ok_or_else(unreadable)?;
            let equity = o.get("equity").and_then(Value::as_str).ok_or_else(unreadable)?;
            let until = present(o, "lockedUntilTimestamp", Value::as_f64).ok_or_else(unreadable)?;
            if found.is_none() && address.to_lowercase() == vault.to_lowercase() { found = Some((decimal(equity).unwrap_or_default(), until)) }
        }
        let Some((equity, until)) = found else { return Ok((Decimal::ZERO, Decimal::ZERO, None)) };
        let until = until.and_then(|ms| DateTime::from_timestamp_millis(ms as i64));
        let unlocked = until.is_none_or(|u| u <= Utc::now());
        Ok((equity, if unlocked { equity } else { Decimal::ZERO }, if unlocked { None } else { until }))
    }

    /// The latest fill that closed (part of) a position in `coin`: price and realised PnL.
    pub async fn last_close(user: &str, coin: &str) -> Result<Option<(Decimal, Decimal, DateTime<Utc>)>> {
        let fills: Vec<Value> = serde_json::from_slice(&Self::info(json!({"type": "userFills", "user": user})).await?).map_err(|_| unreadable())?;
        let mut best: Option<(f64, &str, &str)> = None;
        for f in &fills {
            let o = f.as_object().ok_or_else(unreadable)?;
            let field = |k: &str| o.get(k).and_then(Value::as_str).ok_or_else(unreadable);
            let (c, px, pnl, dir) = (field("coin")?, field("px")?, field("closedPnl")?, field("dir")?);
            let time = o.get("time").and_then(Value::as_f64).ok_or_else(unreadable)?;
            if c == coin && dir.starts_with("Close") && best.is_none_or(|b| time > b.0) { best = Some((time, px, pnl)) }
        }
        Ok(best.map(|(time, px, pnl)| (decimal(px).unwrap_or_default(), decimal(pnl).unwrap_or_default(),
                                         DateTime::from_timestamp_millis(time as i64).unwrap_or_default())))
    }

    /// Open orders on the main exchange and the HIP-3 ones ihatecrypto shows; coins are unique («xyz:CL»).
    pub async fn open_orders(user: &str) -> Result<Vec<Order>> {
        let mut out = vec![];
        for dex in std::iter::once(None).chain(Hyperliquid::HIP3_DEXES.map(Some)) {
            let mut body = json!({"type": "frontendOpenOrders", "user": user});
            if let Some(dex) = dex { body["dex"] = json!(dex) }
            out.extend(Self::parse_orders(&Self::info(body).await?)?);
        }
        Ok(out)
    }

    pub fn parse_orders(data: &[u8]) -> Result<Vec<Order>> {
        let list: Vec<Value> = serde_json::from_slice(data).map_err(|_| unreadable())?;
        list.iter().map(|o| Order::read(o).ok_or_else(unreadable)).collect()
    }

    /// The TWAPs still running, on `coin` or all of them. twapHistory is a log, a row per change («activated», then
    /// «finished», «terminated», «stopped» or «error»): one runs while no row of its id has ended it.
    pub async fn running_twaps(user: &str, coin: Option<&str>) -> Result<Vec<Twap>> {
        Self::parse_twaps(&Self::info(json!({"type": "twapHistory", "user": user})).await?, coin)
    }

    pub fn parse_twaps(data: &[u8], coin: Option<&str>) -> Result<Vec<Twap>> {
        // An account Hyperliquid hasn't seen answers null: no TWAPs.
        let rows: Option<Vec<Value>> = serde_json::from_slice(data).map_err(|_| unreadable())?;
        let mut read = vec![];
        for r in rows.unwrap_or_default() {
            let row = (|| {
                let o = r.as_object()?;
                let state = o.get("state")?.as_object()?;
                let text = |k: &str| present(state, k, |v| v.as_str().map(str::to_string));
                let status = o.get("status")?.as_object()?.get("status")?.as_str()?.to_string();
                Some((state.get("coin")?.as_str()?.to_string(), text("sz")?, text("side")?, status, present(o, "twapId", Value::as_u64)?))
            })().ok_or_else(unreadable)?;
            read.push(row);
        }
        let ended: HashSet<u64> = read.iter().filter(|r| ["finished", "terminated", "stopped", "error"].contains(&r.3.as_str())).filter_map(|r| r.4).collect();
        let mut seen = HashSet::new();
        Ok(read.into_iter().filter_map(|(c, sz, side, _, id)| {
            let id = id?;
            if ended.contains(&id) || coin.is_some_and(|w| w != c) || !seen.insert(id) { return None }
            Some(Twap { id, coin: c, size: sz.and_then(|s| decimal(&s)).unwrap_or_default(), is_buy: side.as_deref() == Some("B") })
        }).collect())
    }

    /// Positions on the main exchange and the HIP-3 ones.
    pub async fn positions(user: &str) -> Result<Vec<Position>> {
        let mut out = vec![];
        for dex in std::iter::once(None).chain(Hyperliquid::HIP3_DEXES.map(Some)) { out.extend(Self::positions_on(user, dex).await?) }
        Ok(out)
    }

    async fn positions_on(user: &str, dex: Option<&str>) -> Result<Vec<Position>> {
        let mut body = json!({"type": "clearinghouseState", "user": user});
        if let Some(dex) = dex { body["dex"] = json!(dex) }
        Self::parse_positions(&Self::info(body).await?)
    }

    pub fn parse_positions(data: &[u8]) -> Result<Vec<Position>> {
        let state: Value = serde_json::from_slice(data).map_err(|_| unreadable())?;
        let rows = state.as_object().and_then(|s| s.get("assetPositions")).and_then(Value::as_array).ok_or_else(unreadable)?;
        rows.iter().map(|r| {
            let p = r.as_object()?.get("position")?.as_object()?;
            let text = |k: &str| present(p, k, |v| v.as_str().map(str::to_string));
            let (coin, szi, pnl) = (p.get("coin")?.as_str()?, p.get("szi")?.as_str()?, p.get("unrealizedPnl")?.as_str()?);
            let (entry, liquidation) = (text("entryPx")?, text("liquidationPx")?);
            Some(Position {
                coin: coin.into(), size: decimal(szi).unwrap_or_default(), entry: entry.as_deref().and_then(decimal).unwrap_or_default(),
                unrealized_pnl: decimal(pnl).unwrap_or_default(), liquidation: liquidation.as_deref().and_then(decimal),
            })
        }).map(|p| p.ok_or_else(unreadable)).collect()
    }

    pub(crate) async fn info(body: Value) -> Result<Vec<u8>> { hl_post("/info", &body).await }
}
